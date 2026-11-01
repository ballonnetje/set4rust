//! Entity creation: the managers' `Add*` functions and the constructors they call, as far as
//! they touch ported state. RNG draws in constructors are reproduced, because they shift every
//! later random number. Parts that need unported systems are marked `TODO(port)`.

use crate::entity::{
    BuildingData, Entity, EntityData, ObjType, SettlerData, flag,
};
use crate::grid::Y16X16;
use crate::spiral;
use crate::world::World;

pub use crate::deco::{DecoPool, DecoPools, DecoSkip, POOL_IDS, add_deco_obj_without_flags};

/// `CSettlerMgr::AddSettler(x, y, player, type, 0)` (release 0x582600) + `CSettler` ctor (release 0x580330).
/// Returns the entity index, or `None` when the 0x7FFF settler cap is reached.
pub fn add_settler(world: &mut World, x: i32, y: i32, player: u8, kind: u16) -> Option<u16> {
    add_settler_ex(world, x, y, player, kind, 0)
}

/// `AddSettler` with its last argument `how`: a settler of a listed type (warrior type ≠ 0) made
/// with `how` > 0 is announced to the AI (`PostAIEvent(0x14, player, id, how)`, 0x58270c). The
/// release passes 2 from the spells 0x4f3e60 / 0x4f4550 / 0x4f48f0 and the shaman's slave
/// (future event 8, 0x4e555e), the dark temple's own value (0x545738: 1 from its unit
/// production, 0 for mushroom farmers); the other callers pass 0.
pub fn add_settler_ex(world: &mut World, x: i32, y: i32, player: u8, kind: u16, how: i32) -> Option<u16> {
    let id = add_settler_inner(world, x, y, player, kind)?;
    if how > 0 && world.rules.warrior_type(kind) != 0 {
        crate::ai::post_event(world, 0x14, player as i32, id as i32, how);
    }
    Some(id)
}

fn add_settler_inner(world: &mut World, x: i32, y: i32, player: u8, kind: u16) -> Option<u16> {
    if world.counters.settlers >= 0x7FFF {
        return None; // "No more settler allowed!"
    }
    world.counters.settlers += 1;
    let node = world.grid.checked_index(x, y)?;
    let race = world.race_of(player);
    // A carrier on land it does not own gets a CFleeRole instead of its normal role.
    let fleeing = kind == 1 && world.owner_of(node) != player;
    // CreateSettlerRole (its constructor's draws come before the settler's), CFleeRole for a
    // carrier on foreign land (0x582693).
    let role = if fleeing { crate::jobs::role::FLEE } else { world.rules.jobs.settler_role(race, kind) };
    let role = crate::jobs::roles::create_settler_role(world, role);
    let slot = world.entities.free_slot()?;
    // CSettler ctor.
    let direction = (world.rng.rand() % 6) as u8;
    let health = world.rules.settler_health(race, kind);
    let data = SettlerData { direction, health, fleeing, job: crate::jobs::JobState::default() };
    let mut e = Entity::new(ObjType::Settler, kind, player, race, Y16X16::pack(x, y), EntityData::Settler(data));
    e.flags &= !0x2800;
    e.flags |= flag::READY | flag::VISIBLE | 0x1000;
    e.flags |= world.rules.warrior_type(kind) as u32;
    world.entities.insert(slot, e);
    // Non-warriors on their owner's eco-sector are counted; all but carriers are offered (carriers
    // offer themselves in their role's Init).
    if world.rules.warrior_type(kind) == 0 {
        let eco = crate::eco::eco_of(world, node);
        if eco != 0 && world.economy.owner(eco) == player {
            crate::eco::change_nr_of_settler(world, eco, kind, 1);
            if kind != 1 {
                crate::eco::set_settler_offer(world, eco, kind, slot);
            }
        }
    }
    world.layers.map_object[node] = slot;
    // Displacement costs 0, role InitWalking / Init, TakeWaitList.
    crate::jobs::roles::settler_constructed(world, slot, role);
    // TODO(port): role PostLoadInit; the `CSettlerMgr` per-type count 0x1545658.
    // The flags the specialist role Inits set (the rest of those roles is not ported):
    // CShamanRole (a CSoldierRole::Init 0x5890f0 subclass; the ported CSquadLeaderRole 23 and
    // CSaboteurRole 10 set them themselves) and
    // CPriestRole / CPioneerRole / CGeologistRole / CGardenerRole / CDarkGardenerRole /
    // CThiefRole::Init set VulnerableMask|Selectable; CDonkeyRole::Init sets 0x4000.
    // CSoldierRole (7) is ported (`jobs::soldier`).
    let role_flags = match world.rules.settler_role(race, kind) {
        9..=14 | 16 | 19 | 23 => flag::VULNERABLE_MASK | flag::SELECTABLE,
        20 => 0x4000,
        _ => 0,
    };
    if let Some(e) = world.entities.by_index_mut(slot as u32) {
        e.flags |= role_flags;
    }
    // `AddSettler` (release 0x582600, 0x582723 / 0x58272a): the manager and the statistic count it.
    world.settler_lists.settler_added(player, kind);
    crate::statistic::settler_added(world, player, kind);
    // `AddSettler` (release 0x582600): every type with a warrior type joins the per-type list.
    if world.rules.warrior_type(kind) != 0 {
        crate::settler_lists::attach(world, slot);
    }
    Some(slot)
}

/// `CSettlerMgr` 0x582770 `(x, y, player, type, count, ai)`: `count` settlers on the first nodes
/// of the spiral around `(x, y)` that lie on the map with no map object and none of node flags
/// 0x39 (`AddSettler`; the last argument only goes to the AI's notice). Returns how many were
/// placed.
pub fn add_settlers_around(world: &mut World, x: i32, y: i32, player: u8, kind: u16, count: u32) -> u32 {
    add_settlers_around_ex(world, x, y, player, kind, count, 0)
}

/// [`add_settlers_around`] with its last argument `ai` (0x5827e4 passes it to `AddSettler` as
/// `how`: `Settlers.AddSettlers` gives 1).
pub fn add_settlers_around_ex(world: &mut World, x: i32, y: i32, player: u8, kind: u16, count: u32, ai: i32) -> u32 {
    let mut left = count;
    let mut k = 0;
    while left > 0 && k < spiral::LEN {
        let (nx, ny) = (x + spiral::dx(k), y + spiral::dy(k));
        k += 1;
        let Some(n) = world.grid.checked_index(nx, ny) else { continue };
        if world.layers.map_object[n] != 0 || world.layers.flags[n] & 0x39 != 0 {
            continue;
        }
        if add_settler_ex(world, nx, ny, player, kind, ai).is_some() {
            left -= 1;
        }
    }
    count - left
}

/// `CSettlerMgr` 0x582810: the invisible door settler of a military building — type 69, a
/// `CDoorRole`, built by the `CSettler` ctor variant 0x5806d0 at `(x, y)` (the door), counted in
/// the 0x7FFF settler cap. Ctor: map-object entry, race of the owner, displacement costs 10,
/// flags `(f & 0xfffdd6ff) | 0x201000d` (READY, 0x10000, warrior nibble 0xd; not visible, not
/// selectable), war map, role `Init`, then `NewToDoList(list 1 of the race, 1)` (`CSettler`
/// 0x580fb0 with the race's list table +4; `CDoorRole::TakeJob` is empty). Returns the id (0 at
/// the cap).
pub fn add_door_settler(world: &mut World, x: i32, y: i32, player: u8) -> u16 {
    if world.counters.settlers >= 0x7FFF {
        return 0;
    }
    world.counters.settlers += 1;
    let Some(slot) = world.entities.free_slot() else { return 0 };
    let race = world.race_of(player);
    let data = SettlerData { direction: 0, health: 0, fleeing: false, job: crate::jobs::JobState::default() };
    let mut e = Entity::new(ObjType::Settler, DOOR_SETTLER, player, race, Y16X16::pack(x, y), EntityData::Settler(data));
    e.flags = (e.flags & 0xfffd_d6ff) | 0x201_000d;
    world.entities.insert(slot, e);
    if let Some(n) = world.grid.checked_index(x, y) {
        world.layers.map_object[n] = slot;
    }
    if let Some(j) = crate::jobs::core::job_mut(world, slot) {
        j.displacement_costs = 10;
    }
    crate::warmap::readd(world, slot);
    crate::jobs::roles::new_role(world, slot, crate::jobs::role::DOOR, 0);
    let list = world.rules.jobs.list(race, 1).map(|_| crate::jobs::ToDo::Shared { race, list: 1 });
    if list.is_some() {
        crate::jobs::core::new_to_do_list(world, slot, list, 1);
    }
    slot
}

/// Settler type of the door settler (`SETTLER_MAX` + 2; outside every settler table).
pub const DOOR_SETTLER: u16 = 69;

/// `CSettlerMgr` 0x582a40: put an off-map settler on the first node of the 1000-entry spiral
/// around `(x, y)` that is inside the map, has no map object and lies in a sector (land or
/// walkable water; tile sector id != 0), then give it its default list (`NewToDoList` with the
/// list `type` of its race). Returns whether a node was found.
pub fn put_settler_near(world: &mut World, settler: u16, x: i32, y: i32) -> bool {
    let size = world.grid.size() as i32;
    let found = (0..1000).find_map(|i| {
        let (nx, ny) = (x + spiral::dx(i), y + spiral::dy(i));
        if !(0..size).contains(&nx) || !(0..size).contains(&ny) {
            return None;
        }
        let n = world.grid.index(nx as u32, ny as u32);
        (world.layers.map_object[n] == 0 && world.tiling.sector_of(n) != 0).then_some((nx, ny, n))
    });
    let Some((nx, ny, n)) = found else { return false };
    world.layers.map_object[n] = settler;
    let Some(e) = world.entities.by_index_mut(settler as u32) else { return false };
    e.pos = Y16X16::pack(nx, ny);
    let kind = e.kind;
    crate::jobs::core::new_to_do_list(world, settler, None, kind as i16);
    true
}

/// `CSettler::Delete` (release 0x580b20, from `CMapObjectMgr::Kill`), the parts ported so far:
/// a settler on the map leaves it (vtbl+0x38, 0x5670e0: ON_BOARD, map-object entry cleared),
/// it is unregistered, its role is set free (vtbl+0x40 with −1), then `CSettlerMgr` 0x582940:
/// warriors leave their per-type list; a non-warrior standing in its owner's eco-sector leaves the
/// offer list if OFFERED (0x4d4180) and, unless it flees (role 0x12) or has flag 0x80, leaves the
/// eco-sector's count (0x4d4250, −1) when `flags & 0x10008000 != 0x8000` (0x10000000 is set at
/// the start, 0x580b23, for a non-warrior on the map; cleared here);
/// the per-type counts and the settler cap count drop.
pub fn settler_delete(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    let (pos, kind, warrior) = (e.pos, e.kind, e.warrior_type());
    // 0x580b23: flag 0x10000000 marks a non-warrior that was on the map (then 0x582940 takes it
    // out of the eco-sector's count).
    e.flags &= !0x1000_0000;
    if kind < 67 && e.flags & 0x800f == 0 {
        e.flags |= 0x1000_0000;
    }
    if e.flags & flag::ON_BOARD == 0 {
        e.flags |= flag::ON_BOARD;
        if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
            world.layers.map_object[n] = 0;
        }
    }
    if world.warmap.contains(id) {
        crate::warmap::remove(world, id);
    }
    world.unregister_from_logic_update(id);
    crate::jobs::roles::set_free(world, id, -1);
    // A visible warrior (or fleeing settler) leaves a death effect: one of three, by a draw on
    // the game RNG (< 33 %: 1, < 66 %: 2, else 3), the owner's tribe effect with sound 0x29 at
    // its position (0x580bff, `CEffects` vtbl+0x14).
    let role = crate::jobs::core::job(world, id).map_or(0, |j| j.role);
    let visible = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::VISIBLE != 0);
    if kind < 67 && (warrior != 0 || role == crate::jobs::role::FLEE) && visible {
        let r = world.rng.rand();
        let effect = if r < 0x547a { 1 } else if r < 0xa8f5 { 2 } else { 3 };
        let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
        world.events.push(crate::event::SimEvent::TribeEffect { effect, sound: 0x29, x: pos.x(), y: pos.y(), player: owner });
    }
    if kind < 67 && warrior != 0 {
        crate::settler_lists::detach(world, id);
    } else if kind < 67
        && let Some(n) = world.grid.checked_index(pos.x(), pos.y())
    {
        // 0x58296b: the eco-sector of the node, if its owner is the settler's.
        let eco = crate::eco::eco_of(world, n);
        let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
        if eco != 0 && world.economy.owner(eco) == owner {
            if world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::OFFERED != 0) {
                crate::eco::get_settler_out_of_offer(world, eco, id);
            }
            let role = crate::jobs::core::job(world, id).map_or(0, |j| j.role);
            let flags = world.entities.by_index(id as u32).map_or(0, |e| e.flags);
            if role != crate::jobs::role::FLEE && flags & 0x80 == 0 && flags & 0x1000_8000 != 0x8000 {
                crate::eco::change_nr_of_settler(world, eco, kind, -1);
                if let Some(e) = world.entities.by_index_mut(id as u32) {
                    e.flags &= !0x1000_0000;
                }
            }
        }
    }
    // 0x5829fe / 0x582a13: the manager's count and `DecNumberOfSettler` (types < 0x43).
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    if kind < 67 {
        world.settler_lists.settler_removed(owner, kind);
    }
    crate::statistic::settler_removed(world, owner, kind);
    world.counters.settlers = world.counters.settlers.saturating_sub(1);
}

fn is_port_ex(t: u8) -> bool {
    (52..=57).contains(&t) || (78..=79).contains(&t)
}

fn is_shipyard_ex(t: u8) -> bool {
    (58..=63).contains(&t) || (76..=77).contains(&t)
}

/// The entity kind (+0xc) the `CBuilding` ctor (0x535b40) stores for a building type: a port
/// orientation becomes 32, a shipyard orientation 31, every other type itself.
pub(crate) fn building_kind(type_ex: u8) -> u16 {
    if is_port_ex(type_ex) {
        32
    } else if is_shipyard_ex(type_ex) {
        31
    } else {
        type_ex as u16
    }
}

/// Why a building was not created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BuildingSkip {
    /// More than 4000 buildings.
    Cap,
    /// A player's second manakopter hall: created, then destroyed again (`AddBuilding` returns 0).
    SecondManakopterHall,
    NoSlot,
}

/// `CBuildingMgr::AddBuilding(x, y, player, type, false)` (release 0x53c350) + `CBuilding` ctor (release 0x535b40),
/// for a finished building as placed by the map loader.
pub fn add_building(world: &mut World, x: i32, y: i32, player: u8, type_ex: u8) -> Result<u16, BuildingSkip> {
    if world.counters.buildings >= 4000 {
        return Err(BuildingSkip::Cap);
    }
    // Type 0x50: the position joins the player's hall list (0x53f810) first.
    let second_hall = type_ex == 80 && crate::build::halls::before_add(world, player, x, y);
    world.counters.buildings += 1;
    let race = world.race_of(player);
    let slot = world.entities.free_slot().ok_or(BuildingSkip::NoSlot)?;
    let rule = world.rules.building(race, type_ex);
    let (door, flag_off) = (rule.door, rule.flag);
    let door = Y16X16::pack(x + door.0 as i32, y + door.1 as i32);
    let ensign = Y16X16::pack(x + flag_off.0 as i32, y + flag_off.1 as i32);
    let kind = building_kind(type_ex);
    let data = BuildingData { type_ex, door, ensign, working_area: ensign, health: 0, events: Vec::new() };
    let mut e = Entity::new(ObjType::Building, kind, player, race, Y16X16::pack(x, y), EntityData::Building(data));
    e.flags |= 0x1100;
    if world.rules.is_military(kind) {
        e.flags |= 0xC;
    }
    world.entities.insert(slot, e);
    // `AddBuilding` (0x53c4ab..0x53c4e9): the head of its (player, type) list.
    crate::buildings::list_add(world, slot);
    // The building's own node holds its id (`BuildingId` lookups in `SetOwner`; FirstSettlerEnter
    // re-sets it after `SetToWorld` on capture). Footprint nodes do not (save chunk 202 has ids
    // only at building positions).
    if let Some(n) = world.grid.checked_index(x, y) {
        world.layers.map_object[n] = slot;
    }
    // `CWarMap::AddEntity` (ctor 0x535d24; buildings have no strength).
    crate::warmap::readd(world, slot);
    // CreateBuildingRole + role Init (vtable +24, in the CBuilding constructor).
    crate::buildings::create_role(world, slot);
    crate::buildings::init(world, slot);
    set_to_world(world, slot);
    // The placing wrapper 0x53c350 (0x53c45f..0x53c504; 0x4f7f40 is the same code but has no
    // callers): 0x4f8730 +1 (fighting strength) and the finished count 0x14ef624 +1 (with it the
    // statistic's 0x11ac05c / 0x11ac440).
    crate::statistic::building(world, player, kind, 1);
    if let Some(e) = world.entities.by_index_mut(slot as u32) {
        e.flags |= 0x10000;
    }
    if second_hall {
        crate::build::halls::destroy_second(world, player, x, y);
        return Err(BuildingSkip::SecondManakopterHall);
    }
    Ok(slot)
}

/// `CBuildingMgr::AddSoldierToStartTower(building, j)` (release 0x53dab0; from
/// `LoadBuildingData` 0x53d940 and the scripted placement 0x53c580): soldier type `29 + j`. Only
/// swordsmen (warrior type 2) and bowmen (3) fit, and only while the wish and the ordered + present
/// count are below the building's slots of that kind. The soldier is created (`AddSettler`) on
/// the first node of the radius-15 spiral around the ensign that is not blocked (flag 0x01) and
/// has no map object; then `IncWishAndOrder` (0x549580: wish and order +1), it loses
/// SELECTABLE, is unregistered and taken off the map (ON_BOARD), `CBuilding::Attach`es to the
/// building and enters it through the role's `SettlerEnter`. Returns whether a soldier was added.
pub fn add_soldier_to_start_tower(world: &mut World, building: u16, j: u8) -> bool {
    let kind = 29 + j as u16;
    let wt = world.rules.warrior_type(kind);
    let Some(b) = world.entities.by_index(building as u32) else { return false };
    let (owner, ensign) = match &b.data {
        EntityData::Building(d) => (b.owner, d.ensign),
        _ => return false,
    };
    if !crate::buildings::military::can_inc_wish_and_order(world, building, wt) {
        return false;
    }
    let spot = spiral::walk(world.grid, ensign.x(), ensign.y(), 15).find(|&(px, py)| {
        let i = world.grid.index(px as u32, py as u32);
        world.layers.flags[i] & crate::grid::flag::BLOCKED_LAND == 0 && world.layers.map_object[i] == 0
    });
    let Some((sx, sy)) = spot else { return false };
    let Some(s) = add_settler(world, sx, sy, owner, kind) else { return false };
    crate::buildings::military::inc_wish_and_order(world, building, wt, false);
    if let Some(e) = world.entities.by_index_mut(s as u32) {
        e.flags &= !flag::SELECTABLE;
    }
    world.unregister_from_logic_update(s);
    if world.warmap.contains(s) {
        crate::warmap::remove(world, s);
    }
    let node = world.grid.index(sx as u32, sy as u32);
    world.layers.map_object[node] = 0;
    if let Some(e) = world.entities.by_index_mut(s as u32) {
        e.flags |= flag::ON_BOARD;
    }
    crate::buildings::military::attach_settler(world, building, s);
    crate::buildings::settler_enter(world, building, s);
    true
}

/// `CVehicleMgr::AddVehicle(x, y, player, type, -1, false)` (release 0x596ad0) → `CVehicleMgr::Create`. Warships and
/// catapults have no Dark Tribe class and types outside 1..=5 none at all: `Create` then reports
/// "can't use this vehicle" and returns an id with no entity; here nothing is created.
pub fn add_vehicle(world: &mut World, x: i32, y: i32, player: u8, kind: u8) -> Option<u16> {
    crate::vehicles::add_vehicle(world, x, y, player, kind as u16, -1, false)
}

/// `CPileMgr::AddPile(x, y, good, amount, role, 0, 0, 0, 0)`: see [`crate::pile::add_pile_ex`].
pub fn add_pile(world: &mut World, x: i32, y: i32, good: u8, amount: u8, role: u8) -> Option<u16> {
    crate::pile::add_pile(world, x, y, good, amount, role)
}

/// `CBuilding::SetToWorld` (release 0x536470): stamp the footprint of a finished building
/// (info vectors +0x2f0 repealing, +0x2cc building, +0x2e4 block, +0x314 water; the savegame
/// oracle `tests/sim_footprint.rs` checks the first three):
/// 1. repealing cells: move cost 7, flag 0x02 (`SetFlagBits(i, 2)`);
/// 2. building cells: flag 0x08; military buildings (the **Roman** record's house kind 10 / 15)
///    also flag 0x80 and the owner (tiling vtbl+0x1c `ChangeOwner`; water stays unowned, see
///    `territory`);
/// 3. block cells: tiling vtbl+0xc `SetBlockedLand` (flag 0x01 + tiling update);
/// 4. ports / shipyards (info +6): the +0x314 cells (water | waterBlock) through tiling vtbl+0x10
///    `SetBlockedWater` (mask walker 0x535ac0);
/// 5. Viking (race 1) type 47 unblocks (x-2, y+1), Maya (race 2) type 48 (x-2, y+4)
///    (tiling vtbl+0x14 `ClearBlockedLand`).
pub fn set_to_world(world: &mut World, building: u16) {
    let Some(e) = world.entities.by_index(building as u32) else { return };
    let EntityData::Building(d) = &e.data else { return };
    let (x, y, race, owner, kind, type_ex) = (e.x(), e.y(), e.race, e.owner, e.kind, d.type_ex);
    let rule = world.rules.building(race, type_ex).clone();
    let military = world.rules.is_military(kind);
    let node = |w: &World, (dx, dy): (i8, i8)| w.grid.checked_index(x + dx as i32, y + dy as i32);
    for &c in &rule.repealing_cells {
        if let Some(n) = node(world, c) {
            world.layers.set_move_cost(n, 7);
            world.layers.flags[n] |= crate::grid::flag::BLOCKED_OBJECT;
        }
    }
    for &c in &rule.building_cells {
        if let Some(n) = node(world, c) {
            world.layers.flags[n] |= crate::territory::FLAG_BUILDING;
            if military {
                world.layers.flags[n] |= crate::territory::FLAG_INFLUENCE;
                world.change_owner(x + c.0 as i32, y + c.1 as i32, owner);
            }
        }
    }
    for &c in &rule.block_cells {
        world.set_blocked_land(x + c.0 as i32, y + c.1 as i32);
    }
    if rule.has_water_masks {
        for &c in &rule.water_block_cells {
            world.set_blocked_water(x + c.0 as i32, y + c.1 as i32);
        }
    }
    let unblock = match (race, kind) {
        (1, 47) => Some((x - 2, y + 1)),
        (2, 48) => Some((x - 2, y + 4)),
        _ => None,
    };
    if let Some((ux, uy)) = unblock {
        world.clear_blocked_land(ux, uy);
    }
}
