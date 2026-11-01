//! Territory: military influence and node ownership (`CWorldManager::SetOwner`,
//! `SetTowerBits`, `GetBuildingInfluenceInfo`, World.cpp).
//!
//! Only inhabited small towers (46), large towers (47) and castles (48) radiate influence
//! (`GetBuildingInfluenceInfo`: last spiral index 3499 / 3999 / 4999, extra reach 37 / 40 / 44).
//! A building's influence on spiral entry `i` around it is `50 - Radius(i)`, summed per player
//! and capped at 254; its own node counts as 255 (then capped like the rest).
//!
//! `SetOwner(x, y)` re-evaluates a radius-40 spiral around `(x, y)` from every inhabited
//! influencing building within ±100 nodes. Ownership is **sticky**: a node changes hands only when
//! its current owner has no influence on it (or it has no owner) and some player has; it then
//! goes to the player with the strictly highest influence (ties: the first building scanned,
//! row by row). Nodes without any influence keep their owner. Flag 0x80 marks nodes with
//! influence. So territory depends on the order of events, not only on the current buildings.

use crate::entity::{EntityData, ObjType};
use crate::spiral;
use crate::world::World;

/// Flag 0x80: some player's military influence reaches the node (set/cleared by `SetOwner`,
/// set by `SetTowerBits`).
pub const FLAG_INFLUENCE: u8 = 0x80;
/// Flag 0x08: a building stands on the node (`SetOwner` destroys it when the node changes hands).
/// Set by `CBuilding::SetToWorld` (TODO(port): footprints).
pub const FLAG_BUILDING: u8 = 0x08;

/// `CWorldManager::GetBuildingInfluenceInfo(type)`: `(last spiral index, extra reach)`.
pub fn influence_info(kind: u16) -> (usize, i32) {
    match kind {
        46 => (3499, 37),
        47 => (3999, 40),
        48 => (4999, 44),
        _ => (0, 0),
    }
}

/// `CWorldManager::SetTowerBits(x, y, type, owner)` (World.cpp:411): used instead of `SetOwner`
/// while the map loader garrisons start towers — only marks the influence spiral with flag 0x80
/// where the node already belongs to `owner` (territory comes from the map's owner layer).
pub fn set_tower_bits(world: &mut World, x: i32, y: i32, kind: u16, owner: u8) {
    let (last, _) = influence_info(kind);
    if last == 0 {
        return;
    }
    for i in 0..=last {
        if let Some(n) = world.grid.checked_index(x + spiral::dx(i), y + spiral::dy(i))
            && world.owner_of(n) == owner
        {
            world.layers.flags[n] |= FLAG_INFLUENCE;
        }
    }
}

/// Building entity standing on `node` (`CWorldManager::BuildingId`: the node's map object if it
/// is a building).
fn building_at(world: &World, node: usize) -> Option<(u16, u8, bool)> {
    let id = world.layers.map_object[node];
    let e = world.entities.by_index(id as u32)?;
    match (&e.obj_type, &e.data) {
        (ObjType::Building, EntityData::Building(_)) => Some((e.kind, e.owner, crate::buildings::have_inhabitant(world, id))),
        _ => None,
    }
}

const W: usize = 101;
const C: i32 = 50;

/// One cell of `SetOwner`'s 101×101×11-byte scratch grid.
#[derive(Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Cell {
    /// [0] highest influence.
    max: u8,
    /// [1..=8] influence per player.
    per: [u8; 9],
    /// [9] current owner | 0x80 (| 0x40 for pseudo tiles, unused).
    tile: u8,
    /// [10] player with the highest influence.
    best: u8,
}

/// Effects of one `SetOwner` call the caller has to apply (not yet ported).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OwnerChange {
    /// Nodes that changed owner.
    pub changed: u32,
    /// Building nodes (flag 0x08) taken from a player: `CBuildingMgr::DestroyBuilding(x, y, 0)`.
    /// TODO(port): DestroyBuilding.
    pub destroy: Vec<(i32, i32)>,
}

/// `CWorldManager::SetOwner(x, y)` (World.cpp:447..720; release 0x60cd30) **without** the
/// building destruction: the nodes that would destroy a building are only listed in
/// [`OwnerChange::destroy`]. For tools and tests that rebuild a start territory; the game uses
/// [`update_owner`].
pub fn set_owner(world: &mut World, x: i32, y: i32) -> OwnerChange {
    owner_pass(world, x, y, false)
}

/// `CWorldManager::SetOwner(x, y)` (release 0x60cd30) as the game runs it: a node another player
/// takes from its owner while flag 0x08 (a building) is set first destroys the non-Dark-Tribe
/// building whose footprint covers it (`CBuildingMgr::DestroyBuilding(x, y, 0)` 0x53ce10, see
/// `buildings::crush::destroy_building_at`), then changes hands. Called when a military
/// building gets its first soldier (0x5484f0) and after one is crushed or destroyed (+0x2c
/// 0x549370).
pub fn update_owner(world: &mut World, x: i32, y: i32) -> OwnerChange {
    owner_pass(world, x, y, true)
}

fn owner_pass(world: &mut World, x: i32, y: i32, destroy: bool) -> OwnerChange {
    let size = world.grid.size() as i32;
    let mut g = vec![Cell::default(); W * W];
    let at = |cx: i32, cy: i32| (cy as usize) * W + cx as usize;
    let in_quad = |cx: i32, cy: i32| (0..W as i32).contains(&cx) && (0..W as i32).contains(&cy);

    // Influence of every inhabited military building within ±100 (clipped to the inner world).
    let (x0, x1) = ((1 - x).max(-100), (size - 2 - x).min(100));
    let (y0, y1) = ((1 - y).max(-100), (size - 2 - y).min(100));
    for dy in y0..=y1 {
        for dx in x0..=x1 {
            let node = world.grid.index((x + dx) as u32, (y + dy) as u32);
            let Some((kind, owner, inhabited)) = building_at(world, node) else { continue };
            let (last, reach) = if inhabited { influence_info(kind) } else { (0, 0) };
            if last == 0 || reach + 50 < spiral::grid_distance(dx, dy) {
                continue;
            }
            let p = owner as usize;
            if !(1..=8).contains(&p) {
                continue; // asserted in the original
            }
            let (bx, by) = (dx + C, dy + C);
            if in_quad(bx, by) {
                let c = &mut g[at(bx, by)];
                c.max = 255;
                c.per[p] = 255;
                c.best = owner;
            }
            for i in 1..=last {
                let (cx, cy) = (bx + spiral::dx(i), by + spiral::dy(i));
                if !in_quad(cx, cy) {
                    continue;
                }
                let c = &mut g[at(cx, cy)];
                let v = (c.per[p] as u32 + 50 - spiral::radius(i) as u32).min(254) as u8;
                c.per[p] = v;
                if v > c.max {
                    c.max = v;
                    c.best = owner;
                }
            }
        }
    }

    // Current owners within ±50.
    let (x0, x1) = ((1 - x).max(-50), (size - 2 - x).min(50));
    let (y0, y1) = ((1 - y).max(-50), (size - 2 - y).min(50));
    for dy in y0..=y1 {
        for dx in x0..=x1 {
            let node = world.grid.index((x + dx) as u32, (y + dy) as u32);
            g[at(dx + C, dy + C)].tile = world.owner_of(node) | 0x80;
        }
    }

    // Re-evaluate the radius-40 spiral.
    let mut out = OwnerChange::default();
    for m in 0..=spiral::last(40) {
        let (dx, dy) = (spiral::dx(m), spiral::dy(m));
        let c = g[at(dx + C, dy + C)];
        if c.tile & 0x80 == 0 {
            continue;
        }
        let cur = c.tile & 0x3F;
        let best = c.best & 0x3F;
        let node = world.grid.index((x + dx) as u32, (y + dy) as u32);
        if best == 0 {
            world.layers.flags[node] &= !FLAG_INFLUENCE;
            continue;
        }
        world.layers.flags[node] |= FLAG_INFLUENCE;
        let cur_has_influence = cur != 0 && c.per.get(cur as usize).copied().unwrap_or(0) != 0;
        if !cur_has_influence && cur != best {
            if world.layers.flags[node] & FLAG_BUILDING != 0 && cur != 0 {
                out.destroy.push((x + dx, y + dy));
                if destroy {
                    crate::buildings::crush::destroy_building_at(world, x + dx, y + dy, 0);
                }
            }
            // CWorldManager::SetOwnerId → CTiling::ChangeOwner. Water is blocked land for the
            // tiling and keeps owner 0 (also what every water node in the savegames shows).
            world.change_owner(x + dx, y + dy, best);
            out.changed += (world.owner_of(node) == best) as u32;
        }
    }
    out
}

/// Per node: which players' inhabited military buildings reach it (bit p-1 for player p) and
/// the player with the highest influence, computed over the whole map at once (no ±100 window).
/// Diagnostic for oracle tests; the game itself only ever evaluates `SetOwner` windows.
pub fn influence_map(world: &World) -> (Vec<u8>, Vec<u8>) {
    let n = world.grid.len();
    let mut per = vec![[0u8; 9]; n];
    let mut best = vec![0u8; n];
    let mut max = vec![0u8; n];
    for (id, e) in world.entities.iter() {
        if e.obj_type != ObjType::Building || !crate::buildings::have_inhabitant(world, id) || e.is_dead() {
            continue;
        }
        let (last, _) = influence_info(e.kind);
        let p = e.owner as usize;
        if last == 0 || !(1..=8).contains(&p) {
            continue;
        }
        for i in 0..=last {
            let Some(node) = world.grid.checked_index(e.x() + spiral::dx(i), e.y() + spiral::dy(i)) else { continue };
            let add = if i == 0 { 255 } else { 50 - spiral::radius(i) as u32 };
            let v = (per[node][p] as u32 + add).min(if i == 0 { 255 } else { 254 }) as u8;
            per[node][p] = v;
            if v > max[node] {
                max[node] = v;
                best[node] = e.owner;
            }
        }
    }
    let mask = per.iter().map(|c| (1..=8).filter(|&p| c[p] != 0).fold(0u8, |m, p| m | 1 << (p - 1))).collect();
    (mask, best)
}
