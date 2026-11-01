//! `CSearchRoutines` (MapObjects/Building/SearchRoutines.cpp): what a working building looks for
//! in its working area, per `SEARCH_*` type (SettlerValues.xml `search/searchType` of the
//! building's inhabitant). A routine returns the entity to work on (> 0), "a position, no
//! entity" (< 0) or nothing (0); the worker then stands at the found node + the type's offset.

use crate::deco::{stone, tree};
use crate::entity::EntityData;
use crate::world::World;

/// `g_sSearchData[type]` (release .data 0x1164f70, 27 records of 16 bytes: routine, an int that
/// `CSettlerMgr::LoadInfo` overwrites with the type, and the worker's offset `dx`, `dy` from the
/// found node). Routine addresses (release): 1 Fish 0x54e920, 2/3 Grain 0x54e490 / 0x54e520,
/// 4/5 Agave 0x54e5b0 / 0x54e640, 6/7 tree seed 0x54e1c0 / 0x54e260, 8 Tree 0x54e140, 9/10 Vine
/// 0x54e390 / 0x54e410, 11/12 Beehive 0x54e8e0 / 0x54e880, 13/14 Mushroom 0x54e7d0 / 0x54e810,
/// 15 Stone 0x54e310, 16 Venison 0x54ea10, 17 Water 0x54ea70, 18..22 Gold/Iron/Coal/Granite/
/// Sulfur 0x54eaa0 / 0x54eb00 / 0x54ead0 / 0x54eb60 / 0x54eb30, 23/24 Dark/Green land 0x54eb90 / 0x54ebc0, 25/26 Sunflower 0x54e6c0 /
/// 0x54e750. Ported: see [`is_ported`].
pub const OFFSETS: [(i8, i8); 27] = [
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, -1),
    (0, -1),
    (-1, -1),
    (-1, -1),
    (1, 1),
    (-1, -1),
    (1, 1),
    (0, -1),
    (0, 1),
    (0, -1),
    (0, -1),
    (2, 2),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, -1),
    (0, -1),
];

/// `SEARCH_TREE`.
pub const TREE: u16 = 8;
/// `SEARCH_TREE_SEED_POS`.
pub const TREE_SEED_POS: u16 = 6;
/// Search type 7 (0x54e260): the Maya's and Trojans' foresters' tree seed search.
pub const TREE_SEED_POS_DRY: u16 = 7;
/// `SEARCH_STONE`.
pub const STONE: u16 = 15;
/// `SEARCH_GRAIN_SEED_POS`, `SEARCH_GRAIN`.
pub const GRAIN_SEED_POS: u16 = 2;
pub const GRAIN: u16 = 3;
/// `SEARCH_FISH`.
pub const FISH: u16 = 1;
/// `SEARCH_AGAVE_SEED_POS`, `SEARCH_AGAVE`.
pub const AGAVE_SEED_POS: u16 = 4;
pub const AGAVE: u16 = 5;
/// `SEARCH_VINE_SEED_POS`, `SEARCH_VINE`.
pub const VINE_SEED_POS: u16 = 9;
pub const VINE: u16 = 10;
/// `SEARCH_BEEHIVE_POS` (placing a hive), `SEARCH_BEEHIVE` (a hive with honey).
pub const BEEHIVE_POS: u16 = 11;
pub const BEEHIVE: u16 = 12;
/// `SEARCH_MUSHROOM_SEED_POS`, `SEARCH_MUSHROOM`.
pub const MUSHROOM_SEED_POS: u16 = 13;
pub const MUSHROOM: u16 = 14;
/// `SEARCH_SUNFLOWER_SEED_POS`, `SEARCH_SUNFLOWER`.
pub const SUNFLOWER_SEED_POS: u16 = 25;
pub const SUNFLOWER: u16 = 26;
/// `SEARCH_WATER`.
pub const WATER: u16 = 17;
/// `SEARCH_VENISON`, `SEARCH_GOLD` .. `SEARCH_SULFUR`, `SEARCH_DARK_LAND`, `SEARCH_GREEN_LAND`:
/// in the release table, but no shipped settler searches with them through the table (the
/// hunter's hut uses `CScanner::FindNearestAnimal`, the gardeners their own 0x579f80, the
/// geologist `SEARCH_RESOURCE_POS`, which the define table lacks).
pub const VENISON: u16 = 16;
pub const GOLD: u16 = 18;
pub const SULFUR: u16 = 22;
pub const DARK_LAND: u16 = 23;
pub const GREEN_LAND: u16 = 24;

/// The worker's offset from the found node for search type `t` (`GetSearchData`: out-of-range
/// types read record 0).
pub fn offset(t: u16) -> (i32, i32) {
    let (dx, dy) = OFFSETS.get(t as usize).copied().unwrap_or((0, 0));
    (dx as i32, dy as i32)
}

/// Whether type `t` has a routine (all of the table's 1..=26 are ported; 0 has none).
pub fn is_ported(t: u16) -> bool {
    (1..=26).contains(&t)
}

/// The routine of search type `t` at (x, y) (`SSearchData::m_pSearchFkt(x, y, 0)`).
pub fn run(world: &mut World, t: u16, x: i32, y: i32) -> i32 {
    match t {
        TREE => search_tree(world, x, y),
        STONE => search_stone(world, x, y),
        TREE_SEED_POS => search_tree_seed_pos(world, x, y, 0),
        TREE_SEED_POS_DRY => search_tree_seed_pos_dry(world, x, y, 0),
        GRAIN => search_grain(world, x, y),
        GRAIN_SEED_POS => search_grain_seed_pos(world, x, y),
        FISH => search_fish(world, x, y),
        WATER => search_water(world, x, y),
        // 0x54e5b0 / 0x54e640, 0x54e390 / 0x54e410, 0x54e6c0 / 0x54e750.
        AGAVE_SEED_POS => seed_pos(world, x, y, |g| g & 0xF0 == 0x40 && g != 0x1d, |s, _| s < 8),
        AGAVE => ripe_plant(world, x, y, &[211]),
        VINE_SEED_POS => seed_pos(world, x, y, |g| g & 0xF0 == 0x10, |s, gradient| s > 7 && gradient > 7),
        VINE => ripe_plant(world, x, y, &[162]),
        SUNFLOWER_SEED_POS => seed_pos(world, x, y, |g| g & 0xF0 == 0x10 && g != 0x1d, |s, _| s < 8),
        SUNFLOWER => ripe_plant(world, x, y, &[255]),
        BEEHIVE_POS => search_beehive_pos(world, x, y),
        BEEHIVE => search_beehive(world, x, y),
        MUSHROOM_SEED_POS => search_mushroom_seed_pos(world, x, y),
        MUSHROOM => search_mushroom(world, x, y),
        VENISON => search_venison(world, x, y),
        GOLD..=SULFUR => search_resource(world, x, y, t),
        // 0x54eb90 / 0x54ebc0: node flag 0x04 (dark land) set / clear; a position, no entity.
        DARK_LAND => -i32::from(world.grid.checked_index(x, y).is_some_and(|n| world.layers.flags[n] & 0x04 != 0)),
        GREEN_LAND => -i32::from(world.grid.checked_index(x, y).is_some_and(|n| world.layers.flags[n] & 0x04 == 0)),
        _ => 0,
    }
}

/// `SearchVenison` (release 0x54ea10): the alive animal on the node (map object of type 0x80,
/// kind < 18) whose type is huntable (0x1298a04).
fn search_venison(world: &World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let id = world.layers.map_object[n];
    let hit = id != 0
        && world.entities.by_index(id as u32).is_some_and(|e| {
            e.obj_type == crate::entity::ObjType::Animal
                && (e.kind as usize) < crate::animals::TYPES
                && world.animals.types.get(e.kind as usize).is_some_and(|t| t.huntable)
                && e.flags & crate::entity::flag::ALIVE_MASK != 0
        });
    if hit { id as i32 } else { 0 }
}

/// The resource searches 18..22 (release 0x54eaa0, 0x54eb00, 0x54ead0, 0x54eb60, 0x54eb30): the
/// node's resource type is 0x10, 0x20, 0x10, 0x50, 0x40 — type 18 (`SEARCH_GOLD`) tests the same
/// 0x10 as type 20 (copied as is). A position, no entity.
fn search_resource(world: &World, x: i32, y: i32, t: u16) -> i32 {
    const KIND: [u8; 5] = [0x10, 0x20, 0x10, 0x50, 0x40];
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    -i32::from(world.layers.resource_type(n) == KIND[(t - GOLD) as usize])
}

/// Ground types whose high nibble is 6: river (`SearchWater`; `SearchFish` also takes 0x0?, water).
const GROUND_RIVER: u8 = 0x60;

/// The linear node offsets at 0x1454148 that `SearchFish` (19 entries) and the tree
/// seed-position searches (7; 0x54e1c0 / 0x54e260) walk: 0x5afb90, called from `SetMapSize` 0x60c5b0, fills 721 entries with
/// `dy · MapSize + dx` from `g_sSurroundingHexPoints8` (.rdata 0x101bdb8, = `tiling::hex_points`:
/// rings starting at (r, 0)). Its radius-2 part, as (dx, dy). Not the `CSpiralOffsets` order:
/// ring 2 starts at (2, 0) here, at (1, -1) there.
pub const HEX_RADIUS2: [(i8, i8); 19] = [
    (0, 0),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (2, 0),
    (2, 1),
    (2, 2),
    (1, 2),
    (0, 2),
    (-1, 1),
    (-2, 0),
    (-2, -1),
    (-2, -2),
    (-1, -2),
    (0, -2),
    (1, -1),
];

/// `CSearchRoutines::SearchFish(x, y)` (release 0x54e920): away from the map's 2-node margin, the
/// first of the 19 nodes of radius 2 around (x, y) in [`HEX_RADIUS2`] order that holds fish (resource type 0, amount 1..=15), has no flag
/// 0x04 and is water or river (ground `0x0?` / `0x6?`). Then the game RNG decides: above
/// 0x5479ffff (67 %) one fish is caught (the amount drops by one, returns 1, "an entity": the
/// fisher's catch list), else -1 (a position: the empty-net list). No such node: 0.
pub fn search_fish(world: &mut World, x: i32, y: i32) -> i32 {
    let size = world.grid.size() as i32;
    if !(2..size - 2).contains(&x) || !(2..size - 2).contains(&y) {
        return 0;
    }
    for &(dx, dy) in &HEX_RADIUS2 {
        let Some(n) = world.grid.checked_index(x + dx as i32, y + dy as i32) else { continue };
        let fish = world.layers.resource[n];
        let ground = world.layers.ground[n] & 0xF0;
        if !(1..=15).contains(&fish) || world.layers.flags[n] & 0x04 != 0 || (ground != GROUND_RIVER && ground != 0) {
            continue;
        }
        if world.rng.rand() << 16 > 0x5479_ffff {
            world.layers.resource[n] = fish - 1;
            return 1;
        }
        return -1;
    }
    0
}

/// `CSearchRoutines::SearchWater(x, y)` (release 0x54ea70): river ground (`0x6?`) at the node,
/// 1 (the waterworker's work list), else 0.
pub fn search_water(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    i32::from(world.layers.ground[n] & 0xF0 == GROUND_RIVER)
}

/// `CSearchRoutines::SearchTree(x, y)` (release 0x54e140): a grown tree nobody works on (node
/// flags 0x24 clear, resource type 0x70) whose deco entity is still the shared static instance.
/// It gets a dynamic `CTree` of its own, whose id is returned.
pub fn search_tree(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    if world.layers.flags[n] & 0x24 != 0 || world.layers.resource_type(n) != tree::RESOURCE_TREE {
        return 0;
    }
    let id = world.layers.deco_object[n];
    let is_static = world.entities.by_index(id as u32).is_some_and(|e| matches!(&e.data, EntityData::Deco(d) if d.static_instance));
    if id == 0 || !is_static {
        return 0;
    }
    tree::make_dynamic(world, id, x, y).map_or(0, |t| t as i32)
}

/// `CSearchRoutines::SearchStone(x, y)` (release 0x54e310): a stone heap (resource type 0x60) on
/// a node without flag 0x04 whose deco entity is the shared static instance; it gets a dynamic
/// `CStone`. Unlike trees, the work flag 0x20 is not checked, so several stonecutters may pick
/// the same heap.
pub fn search_stone(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    if world.layers.flags[n] & 0x04 != 0 || world.layers.resource_type(n) != stone::RESOURCE_STONE {
        return 0;
    }
    let id = world.layers.deco_object[n];
    let is_static = world.entities.by_index(id as u32).is_some_and(|e| matches!(&e.data, EntityData::Deco(d) if d.static_instance));
    if !is_static {
        return 0;
    }
    stone::make_dynamic(world, id, x, y).map_or(0, |s| s as i32)
}

/// `SearchTreeSeedPos` extra free rings: `table[Rand() & 7]` (release .rdata 0x101a3a0).
const SEED_EXTRA: [u8; 8] = [0, 0, 1, 1, 1, 1, 1, 2];

/// `CSearchRoutines::SearchTreeSeedPos(x, y, a)` (release 0x54e1c0): a place to plant a tree: no
/// settler on the node, room for a tree object (`IsAddDecoObjOk(x, y, 1, _, extra)`, the extra
/// free rings drawn from the game RNG), and green land (ground `0x1?`, not 29) on the node and its
/// six neighbours. Returns −1 (a position, no entity).
pub fn search_tree_seed_pos(world: &mut World, x: i32, y: i32, a: i32) -> i32 {
    tree_seed_pos(world, x, y, a, |g| g & 0xF0 == 0x10)
}

/// Search type 7 (release 0x54e260), the Maya's and Trojans' foresters' (`SettlerInfo` search 1
/// of type 6): as [`search_tree_seed_pos`] on green land or on ground `0x4?` (not 29).
pub fn search_tree_seed_pos_dry(world: &mut World, x: i32, y: i32, a: i32) -> i32 {
    tree_seed_pos(world, x, y, a, |g| matches!(g & 0xF0, 0x10 | 0x40))
}

fn tree_seed_pos(world: &mut World, x: i32, y: i32, a: i32, land: impl Fn(u8) -> bool) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    if world.layers.map_object[n] != 0 {
        return 0;
    }
    let r = world.rng.rand();
    let extra = if a & 0xF != 0 { (r & 3 == 0) as u8 } else { SEED_EXTRA[(r & 7) as usize] };
    if !crate::deco::is_add_deco_obj_ok(world, x, y, 1, extra) {
        return 0;
    }
    for (dx, dy) in [(0, 0), (1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)] {
        let Some(m) = world.grid.checked_index(x + dx, y + dy) else { return 0 };
        let g = world.layers.ground[m];
        if !land(g) || g == 29 {
            return 0;
        }
    }
    -1
}

/// The steepest height difference across node `n` (release 0x54e0c0): the largest of
/// |h(x−1, y) − h(x+1, y)|, |h(x, y−1) − h(x, y+1)| and |h(x−1, y−1) − h(x+1, y+1)|.
fn steepness(world: &World, x: i32, y: i32) -> i32 {
    let h = |dx: i32, dy: i32| world.grid.checked_index(x + dx, y + dy).map_or(0, |m| world.layers.height[m] as i32);
    let a = (h(-1, 0) - h(1, 0)).abs();
    let b = (h(0, -1) - h(0, 1)).abs();
    let c = (h(-1, -1) - h(1, 1)).abs();
    a.max(b).max(c)
}

/// `CSearchRoutines::SearchGrainSeedPos(x, y)` (release 0x54e490): a place to sow grain:
/// green land (ground `0x1?`), not steeper than 7, and none of node flags 0x7F on the node and
/// the six around it (spiral entries 0..6, 0x1169968). Returns −1 (a position, no entity).
pub fn search_grain_seed_pos(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let g = world.layers.ground[n];
    // (The exe also rejects ground 0x40, which the 0x1? test has already excluded.)
    if g & 0xF0 != 0x10 || steepness(world, x, y) > 7 {
        return 0;
    }
    for k in 0..7 {
        let Some(m) = world.grid.checked_index(x + crate::spiral::dx(k), y + crate::spiral::dy(k)) else { return 0 };
        if world.layers.flags[m] & 0x7F != 0 {
            return 0;
        }
    }
    -1
}

/// The seed-position searches of the planters other than the grain farmer (agave 0x54e5b0, vine
/// 0x54e390, sunflower 0x54e6c0), the grain one's shape: the node's ground passes `ground`, its
/// steepness (0x54e0c0) and shading gradient (byte 2, low nibble) pass `slope`, and none of the
/// six around it (spiral entries 0..6) has a node flag of 0x7F. Returns -1 (a position).
/// (The vine wants a slope steeper than 7 and a gradient above 7, the others a steepness below 8.)
fn seed_pos(world: &mut World, x: i32, y: i32, ground: impl Fn(u8) -> bool, slope: impl Fn(i32, u8) -> bool) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    if !ground(world.layers.ground[n]) || !slope(steepness(world, x, y), world.layers.gradient[n] & 0x0F) {
        return 0;
    }
    for k in 0..7 {
        let Some(m) = world.grid.checked_index(x + crate::spiral::dx(k), y + crate::spiral::dy(k)) else { return 0 };
        if world.layers.flags[m] & 0x7F != 0 {
            return 0;
        }
    }
    -1
}

/// The harvest searches of vine (0x54e410, type 162), agave (0x54e640, 211) and sunflower
/// (0x54e750, 255), as `SearchGrain`: a ripe (READY), still shared static plant of one of `kinds`
/// gets a dynamic `CPlant` of its own, whose id is returned.
fn ripe_plant(world: &mut World, x: i32, y: i32, kinds: &[u16]) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let id = world.layers.deco_object[n];
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let ok = e.obj_type == crate::entity::ObjType::Deco
        && kinds.contains(&e.kind)
        && e.flags & crate::entity::flag::READY != 0
        && matches!(&e.data, EntityData::Deco(d) if d.static_instance);
    if !ok {
        return 0;
    }
    crate::deco::plant::make_dynamic(world, id, x, y).map_or(0, |p| p as i32)
}

/// `CSearchRoutines::SearchBeehivePos(x, y)` (0x54e8e0): a hive fits there
/// (`IsAddDecoObjOk(x, y, 217, _, Rand() % 3)`, the extra free rings from the game RNG, drawn
/// first). Returns -1 (a position).
fn search_beehive_pos(world: &mut World, x: i32, y: i32) -> i32 {
    let extra = (world.rng.rand() % 3) as u8;
    if crate::deco::is_add_deco_obj_ok(world, x, y, crate::deco::hive::HIVE, extra) { -1 } else { 0 }
}

/// `CSearchRoutines::SearchBeehive(x, y)` (0x54e880): a READY hive (type 217) on a node without
/// flag 0x20 (nobody works on it); its id.
fn search_beehive(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let id = world.layers.deco_object[n];
    if id == 0 || world.layers.flags[n] & 0x20 != 0 {
        return 0;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let ok = e.obj_type == crate::entity::ObjType::Deco && e.kind == crate::deco::hive::HIVE && e.flags & crate::entity::flag::READY != 0;
    if ok { id as i32 } else { 0 }
}

/// `CSearchRoutines::SearchMushroomSeedPos(x, y)` (0x54e7d0): a node with flag 0x04 where a
/// mushroom (type 212) fits (`IsAddDecoObjOk(x, y, 212, _, 0)`). Returns -1 (a position).
fn search_mushroom_seed_pos(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    if world.layers.flags[n] & 0x04 == 0 {
        return 0;
    }
    if crate::deco::is_add_deco_obj_ok(world, x, y, 212, 0) { -1 } else { 0 }
}

/// `CSearchRoutines::SearchMushroom(x, y)` (0x54e810): a ripe (READY), static mushroom (types
/// 212..=214); its id (no dynamic copy is made here).
fn search_mushroom(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let id = world.layers.deco_object[n];
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let ok = e.obj_type == crate::entity::ObjType::Deco
        && (212..=214).contains(&e.kind)
        && e.flags & crate::entity::flag::READY != 0
        && matches!(&e.data, EntityData::Deco(d) if d.static_instance);
    if ok { id as i32 } else { 0 }
}

/// `CSearchRoutines::SearchGrain(x, y)` (release 0x54e520): ripe grain (deco object 209 / 210,
/// READY) still a shared static instance; it gets a dynamic `CPlant`, whose id is returned.
pub fn search_grain(world: &mut World, x: i32, y: i32) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let id = world.layers.deco_object[n];
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let ok = e.obj_type == crate::entity::ObjType::Deco
        && (e.kind == 209 || e.kind == 210)
        && e.flags & crate::entity::flag::READY != 0
        && matches!(&e.data, EntityData::Deco(d) if d.static_instance);
    if !ok {
        return 0;
    }
    crate::deco::plant::make_dynamic(world, id, x, y).map_or(0, |p| p as i32)
}
