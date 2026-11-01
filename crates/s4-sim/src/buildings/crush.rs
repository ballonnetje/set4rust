//! `CBuilding::CrushBuilding` (release 0x536960; command 5002): the player tears a building down.
//!
//! In order:
//! 1. The building's map-object entry at its position is cleared.
//! 2. Role vtable +0x64: the inhabitant leaves (default 0x5400a0, [`inhabitant_leaves`];
//!    `CMilitaryBuildingRole` / `CCastleRole` 0x549390 let the soldiers out: `military::all_leave`).
//! 3. `CBuilding::RemoveFromWorld` (0x536d00, [`remove_from_world`]).
//! 4. Role vtable +0x60: half the building cost is dropped as goods (default 0x540200,
//!    [`drop_half_cost`]; `CBuildingSiteRole` 0x543030: `build::site::site_drop_materials`).
//! 5. A crush effect (`CEffects::AddEffect`, vtable +0x10 of `[g_pLogic+0x24]`): effect 0x3e with
//!    sound 3 (`SOUND_AMB_CRUSHSMALLHOUSE`) when the `bitBoundingRect` area is under 90, else
//!    0x3c with sound 2 (`SOUND_AMB_CRUSHBIGHOUSE`); emitted as [`SimEvent::Effect`] ([`effect`]).
//! 6. Unless the owner is Dark Tribe: the eco-sector at the ensign forgets the building's
//!    requests (`CEcoSector::CleanUpBuildingNeed`, 0x4d5b70).
//! 7. The building is killed (0x56d030, `World::kill`).
//! 8. Role vtable +0x2c, the role's own clean-up: most roles return at once; storage
//!    (0x550190) unregisters its goods; military (0x549370) recomputes the territory around it
//!    (0x60cd30, `military::crushed`); site (0x542d50): `build::site::site_crush`; residence
//!    (0x54df40) only lowers a statistic (beds); trading (0x5570b0): `trading::crush`. Not ported,
//!    as its role is not: workshop (0x5596e0: the vehicle
//!    under construction (+0x17e) gets effect 0x3e / sound 3 at its position and is removed
//!    (0x597100)).

use super::{RoleKind, house_kind, role};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::event::SimEvent;
use crate::spiral;

/// The effect `CrushBuilding` (0x5369c4..0x536a23) and the destroy path (0x536bbd..0x536c3e) add
/// at the building's position, `(effect, sound)` by the `bitBoundingRect` area
/// `(maxY − minY)·(maxX − minX)` of the `(race, kind)` record. Destroy: kind 0x31 always gets
/// (0x4d, 90 `SOUND_AMB_ATTACKBIGBUILDING`), else (0x3f, 91 `SOUND_AMB_ATTACKSMALLBUILDING`) under
/// 90, (0x3d, 90) otherwise. Crush: (0x3e, 3 `SOUND_AMB_CRUSHSMALLHOUSE`) under 90, else (0x3c, 2
/// `SOUND_AMB_CRUSHBIGHOUSE`). The effect itself (`CEffects`) is client-side.
fn effect(world: &mut World, id: u16, destroyed: bool) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, kind) = (e.x(), e.y(), e.kind);
    let [min_x, max_x, min_y, max_y] = world.rules.building(e.race & 0xf, kind as u8).bounding_rect;
    let small = (max_y as i32 - min_y as i32) * (max_x as i32 - min_x as i32) < 0x5a;
    let (effect, sound) = match (destroyed, small) {
        (true, _) if kind == 0x31 => (0x4d, 90),
        (true, true) => (0x3f, 91),
        (true, false) => (0x3d, 90),
        (false, true) => (0x3e, 3),
        (false, false) => (0x3c, 2),
    };
    world.events.push(SimEvent::Effect { effect, sound, x, y });
}

/// `CBuilding::CrushBuilding` (release 0x536960).
pub fn crush_building(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.obj_type != ObjType::Building {
        return;
    }
    let (x, y, race) = (e.x(), e.y(), e.race);
    let ensign = match &e.data {
        EntityData::Building(b) => b.ensign,
        _ => return,
    };
    if let Some(n) = world.grid.checked_index(x, y) {
        let m = world.layers.map_object[n];
        if world.entities.by_index(m as u32).is_some_and(|o| o.obj_type == ObjType::Building) {
            world.layers.map_object[n] = 0;
        }
    }
    let hk = role(world, id).map_or(0, |r| r.base.house_kind);
    let site = world.sites.roles.contains_key(&id);
    // +0x64
    if hk == house_kind::MILITARY || hk == house_kind::CASTLE {
        super::military::all_leave(world, id);
    } else {
        inhabitant_leaves(world, id);
    }
    remove_from_world(world, id);
    // +0x60
    if site {
        crate::build::site::site_drop_materials(world, id);
    } else {
        drop_half_cost(world, id);
    }
    effect(world, id, false);
    if race != 3
        && let Some(n) = world.grid.checked_index(ensign.x(), ensign.y())
    {
        let eco = crate::eco::eco_of(world, n);
        crate::eco::clean_up_building_need(world, eco, id);
    }
    world.kill(id, 0);
    // +0x2c
    if site {
        crate::build::site::site_crush(world, id);
        return;
    }
    match role(world, id).map(|r| &r.kind) {
        Some(RoleKind::Storage(_)) => super::storage::crush(world, id),
        _ => match hk {
            house_kind::MILITARY | house_kind::CASTLE => super::military::crushed(world, id),
            house_kind::TRADE => super::trading::crush(world, id),
            house_kind::WORKSHOP => super::workshop::crush(world, id),
            // CResidenceBuildingRole +0x2c (0x54df40): the bed total.
            house_kind::RESIDENCE => super::residence::crush(world, id),
            _ => {}
        },
    }
}

/// `IBuildingRole` vtable +0x64 default (release 0x5400a0): an inhabitant inside the building
/// (standing on its position) is put on the first free node of the door's eco-sector within
/// `CSpiralWalk(door, 15)` (0x5857c0; else of the building position's eco-sector, else on the
/// door itself) by `CSettler` vtable +0x34 (0x567080: ON_BOARD cleared, position and map object
/// set, out of the war map if it was in (0x5bb440) and in at the new position (0x5bb330)).
/// Inside or not, it then gets VISIBLE and loses ON_BOARD and 0x80000; the role forgets it.
pub fn inhabitant_leaves(world: &mut World, id: u16) {
    let Some(inh) = role(world, id).map(|r| r.base.inhabitant) else { return };
    if inh == 0 {
        return;
    }
    let Some(b) = world.entities.by_index(id as u32) else { return };
    let (bpos, door) = match &b.data {
        EntityData::Building(d) => (b.pos, d.door),
        _ => return,
    };
    let Some(s) = world.entities.by_index(inh as u32) else { return };
    let (spos, owner) = (s.pos, s.owner);
    if spos == bpos {
        let (mut x, mut y) = (door.x(), door.y());
        let eco_at = |w: &World, p: crate::Y16X16| w.grid.checked_index(p.x(), p.y()).map_or(0, |n| crate::eco::eco_of(w, n));
        for e in [eco_at(world, door), eco_at(world, bpos)] {
            if e != 0
                && world.economy.owner(e) == owner
                && let Some((fx, fy)) = free_spot(world, x, y, e)
            {
                (x, y) = (fx, fy);
                break;
            }
        }
        if let Some(n) = world.grid.checked_index(x, y) {
            world.layers.map_object[n] = inh;
        }
        if let Some(s) = world.entities.by_index_mut(inh as u32) {
            s.flags &= !flag::ON_BOARD;
            s.pos = crate::Y16X16::pack(x, y);
        }
        crate::warmap::readd(world, inh);
    }
    if let Some(s) = world.entities.by_index_mut(inh as u32) {
        s.flags = (s.flags & 0xfff7_7fff) | flag::VISIBLE;
    }
    if let Some(r) = super::role_mut(world, id) {
        r.base.inhabitant = 0;
    }
}

/// 0x5857c0 `(x, y, eco)`: the first node of `CSpiralWalk(x, y, 15)` that is not blocked land
/// (flags 0x21 clear), holds no map object and lies in `eco`.
fn free_spot(world: &World, x: i32, y: i32, eco: u16) -> Option<(i32, i32)> {
    spiral::walk(world.grid, x, y, 15).find(|&(px, py)| {
        let n = world.grid.index(px as u32, py as u32);
        world.layers.flags[n] & 0x21 == 0 && world.layers.map_object[n] == 0 && crate::eco::eco_of(world, n) == eco
    })
}

/// `CBuilding::RemoveFromWorld` (release 0x536d00), the undoing of `create::set_to_world` over the
/// building cells (info vector +0x2cc): walk count 10 and move cost 2 (byte 0x52), flags 0x02,
/// 0x08, 0x10 and 0x20 cleared (a pile with goods on the node keeps cost 7 and flag 0x02), then
/// `CTiling::ClearBlockedLand` (g_pTiling +0x14) on the node. Ports and shipyards (info +6)
/// then, with the mask walker 0x535ac0:
/// * every +0x314 cell (water | waterBlock): on water (ground ≤ 8) `ClearBlockedWater` (tiling
///   +0x18), water-flag bit 0x1000 and node flag 0x08 cleared; on land `ClearBlockedLand`, then
///   with any pile on the node (no amount check here) move cost 7 and flags `& 0xc7 | 2`, else
///   move cost bits `& 0xfa | 2` and flags `& 0xc5`;
/// * every +0x320 cell (waterFree): node flag 0x08 cleared, and `ClearBlockedWater` — at the
///   position the first walker ended on, not the cell's (the release passes the stale
///   coordinates, [`crate::rules::walker_end`]);
/// * the water-flag bit 0x1000 at the `workingpos` (info +0x2a / +0x2b) cleared.
pub fn remove_from_world(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let EntityData::Building(d) = &e.data else { return };
    let (x, y) = (e.x(), e.y());
    let rule = world.rules.building(e.race, d.type_ex).clone();
    for &(dx, dy) in &rule.building_cells {
        let (cx, cy) = (x + dx as i32, y + dy as i32);
        let Some(n) = world.grid.checked_index(cx, cy) else { continue };
        world.layers.walk_move[n] = 0x52;
        world.layers.flags[n] &= 0xc5;
        let p = world.layers.deco_object[n];
        if crate::pile::get(world, p).is_some_and(|p| p.amount > 0) {
            world.layers.walk_move[n] |= 7;
            world.layers.flags[n] |= crate::grid::flag::BLOCKED_OBJECT;
        }
        world.clear_blocked_land(cx, cy);
    }
    if !rule.has_water_masks {
        return;
    }
    for &(dx, dy) in &rule.water_block_cells {
        let (cx, cy) = (x + dx as i32, y + dy as i32);
        let Some(n) = world.grid.checked_index(cx, cy) else { continue };
        if world.layers.ground[n] <= 8 {
            world.clear_blocked_water(cx, cy);
            world.tiling.water_flags[n] &= !0x1000;
            world.layers.flags[n] &= !0x08;
            continue;
        }
        world.clear_blocked_land(cx, cy);
        if crate::pile::get(world, world.layers.deco_object[n]).is_some() {
            world.layers.walk_move[n] |= 7;
            world.layers.flags[n] = (world.layers.flags[n] & 0xc7) | crate::grid::flag::BLOCKED_OBJECT;
        } else {
            world.layers.walk_move[n] = (world.layers.walk_move[n] & 0xfa) | 2;
            world.layers.flags[n] &= 0xc5;
        }
    }
    let (sx, sy) = (x + rule.water_free_stale.0 as i32, y + rule.water_free_stale.1 as i32);
    for &(dx, dy) in &rule.water_free_cells {
        world.clear_blocked_water(sx, sy);
        if let Some(n) = world.grid.checked_index(x + dx as i32, y + dy as i32) {
            world.layers.flags[n] &= !0x08;
        }
    }
    if let Some(n) = world.grid.checked_index(x + rule.working.0 as i32, y + rule.working.1 as i32) {
        world.tiling.water_flags[n] &= !0x1000;
    }
}

/// `IBuildingRole` vtable +0x60 default (release 0x540200): half of the building's boards
/// (info +3), stone (+2) and gold (+4) cost, each rounded down, go into
/// `CPileMgr::SearchSpaceForGoods` at the building position in portions of at most 8.
pub fn drop_half_cost(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let EntityData::Building(d) = &e.data else { return };
    let (x, y) = (e.x(), e.y());
    let rule = world.rules.building(e.race, d.type_ex);
    let goods = [(rule.cost_boards, 7u8), (rule.cost_stone, 32), (rule.cost_gold, 14)];
    for (cost, good) in goods {
        let mut left = cost as i32 / 2;
        while left > 0 {
            let n = left.min(8);
            crate::pile::search_space_for_goods(world, x, y, good, n as u8);
            left -= n;
        }
    }
}

/// `CBuilding` vtbl+0x20 (release 0x5371f0): `n` damage. The role turns it into hit points
/// (+0x6c: military 0x549520, the `IBuildingRole` default 0x5401d0 — both `max(1, (n − 5) / 2)`
/// for `n > 0`, the military role also updating its damage level); at or above the health the
/// building is destroyed: role +0x74 (military 0x549440: everyone inside dies), then 0x536a80.
pub fn damage(world: &mut World, id: u16, n: i32) {
    let military = matches!(role(world, id).map(|r| &r.kind), Some(RoleKind::Military(_)));
    let d = if military { super::military::hit(world, id, n) } else if n < 1 { 0 } else { ((n - 5) / 2).max(1) };
    if d <= 0 {
        return;
    }
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    let dead = e.is_dead();
    let EntityData::Building(b) = &mut e.data else { return };
    if d < b.health as i32 {
        b.health -= d as u8;
        return;
    }
    b.health = 0;
    if dead {
        return;
    }
    // Role vtbl+0x74: the military roles' 0x549440; every other building role has the
    // `IBuildingRole` default 0x5410f0 (`ret 4`).
    if military {
        super::military::destroyed(world, id);
    }
    destroy(world, id, 0);
}

/// 0x536a80 `(cause)`: a destroyed building — the map-object entry, `RemoveFromWorld`, an effect
/// ([`effect`]: 0x3d/0x3f/0x4d), role +0x64, `CleanUpBuildingNeed` (not for the Dark
/// Tribe), the building byte +0x25 = `cause` while it is killed (`Delete` sends it as event 9's
/// second datum), role +0x2c. Unlike `CrushBuilding` nothing is dropped (+0x60).
/// In a campaign's last mission ([`World::final_temple`]) a building of type 0x33 instead posts
/// event 0x38 and marks its owner lost (0x149b7f4) and stays where it is: no map-object entry
/// cleared, no `RemoveFromWorld`, no effect, no kill (0x536aaf..0x536b68, 0x536c9e).
pub fn destroy(world: &mut World, id: u16, cause: u8) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, race, owner) = (e.x(), e.y(), e.race, e.owner);
    let stays = world.final_temple && e.kind == 0x33;
    let ensign = match &e.data {
        EntityData::Building(b) => b.ensign,
        _ => return,
    };
    if stays {
        world.script_events.lost |= 1 << owner;
    } else {
        if let Some(n) = world.grid.checked_index(x, y) {
            let m = world.layers.map_object[n];
            if world.entities.by_index(m as u32).is_some_and(|o| o.obj_type == ObjType::Building) {
                world.layers.map_object[n] = 0;
            }
        }
        remove_from_world(world, id);
        effect(world, id, true);
    }
    let hk = role(world, id).map_or(0, |r| r.base.house_kind);
    if hk == house_kind::MILITARY || hk == house_kind::CASTLE {
        super::military::all_leave(world, id);
    } else {
        inhabitant_leaves(world, id);
    }
    if race != 3
        && let Some(n) = world.grid.checked_index(ensign.x(), ensign.y())
    {
        let eco = crate::eco::eco_of(world, n);
        crate::eco::clean_up_building_need(world, eco, id);
    }
    // GUI_WARN_DESTROY_BLD (0x536cad).
    if let Some(b) = world.entities.by_index(id as u32) {
        let ev = crate::event::SimEvent::warning(b.owner, 0x9a6, b.pos);
        world.events.push(ev);
    }
    world.delete_cause = cause;
    if !stays {
        world.kill(id, 0);
    }
    world.delete_cause = 0;
    match role(world, id).map(|r| &r.kind) {
        Some(RoleKind::Storage(_)) => super::storage::crush(world, id),
        Some(RoleKind::Military(_)) => super::military::crushed(world, id),
        Some(RoleKind::Trading(_)) => super::trading::crush(world, id),
        Some(RoleKind::Workshop(_)) => super::workshop::crush(world, id),
        Some(RoleKind::MushroomFarm(_)) => super::mushfarm::destroyed(world, cause),
        Some(RoleKind::Residence(_)) => super::residence::crush(world, id),
        _ => {}
    }
}

/// `CBuildingMgr::DestroyBuilding(x, y, who)` (release 0x53ce10): nothing on water (ground type
/// below 9). Otherwise the radius-57 spiral (9900 entries) around `(x, y)` is searched for
/// buildings in the map-object layer (finished buildings) or the deco layer (sites); the first
/// whose footprint (`buildingPosLines`, or for ports / shipyards also `waterPosLines`, info
/// +0x2fc) covers `(x, y)` is destroyed (0x536a80, cause `who` when ≥ 0, else 0) if its race fits
/// `who` (−1 any, 0 not the Dark Tribe, > 0 only the Dark Tribe).
/// Every hall (type 0x50) the spiral meets, covering or not, sets its owner's hall tick to the
/// game tick (0x53cec7, [`crate::build::halls`]).
pub fn destroy_building_at(world: &mut World, x: i32, y: i32, who: i32) {
    let Some(n) = world.grid.checked_index(x, y) else { return };
    if world.layers.ground[n] < 9 {
        return;
    }
    let size = world.grid.size() as i32;
    for k in 0..9900 {
        let (bx, by) = (x + spiral::dx(k), y + spiral::dy(k));
        if !(0..size).contains(&bx) || !(0..size).contains(&by) {
            continue;
        }
        let m = world.grid.index(bx as u32, by as u32);
        let is_building = |id: u16| world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Building);
        let mut b = world.layers.map_object[m];
        if !is_building(b) {
            b = world.layers.deco_object[m];
            if !is_building(b) {
                continue;
            }
        }
        let Some(e) = world.entities.by_index(b as u32) else { continue };
        if e.kind == 80 {
            let owner = e.owner;
            crate::build::halls::met_in_destroy(world, owner);
        }
        let Some(e) = world.entities.by_index(b as u32) else { continue };
        let EntityData::Building(d) = &e.data else { continue };
        let (ex, ey, race) = (e.x(), e.y(), e.race);
        let rule = world.rules.building(race, d.type_ex);
        let hit = |&(cx, cy): &(i8, i8)| ex + cx as i32 == x && ey + cy as i32 == y;
        let covers = rule.building_cells.iter().any(hit) || (rule.has_water_masks && rule.water_cells.iter().any(hit));
        if !covers {
            continue;
        }
        let fits = match who {
            -1 => true,
            0 => race != 3,
            w => w > 0 && race == 3,
        };
        if fits {
            destroy(world, b, who.max(0) as u8);
        }
        return;
    }
}
