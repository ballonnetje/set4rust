//! Dark land: `CDarkTribe` (release vtable 0x102dff0, the object at 0x118451c) as far as it
//! turns land dark and light again. Decompiles in `tools/re/decompiled/script/dark/`.
//!
//! Dark land is node flag 0x04. Setting or clearing it (0x4cbb60) also converts the deco object on
//! the node, or the one whose ring the node is in (flag 0x40), into its dark or light variant
//! (pairs `{light, dark}` at .rdata 0x101a430). Objects without a variant vanish with effect
//! 0x40. The AI's cell statistics follow (`PostAIEvent` 0x1d, [`crate::animals::cells`]).
//!
//! Callers: the shadow herb (`CShadowHerb`, deco 216) darkens around itself
//! ([`darken_around`], vtbl+0xc). The gardener lightens ([`lighten`], vtbl+0x10).
//!
//! Not ported: the minimap / landscape redraw (0x65b210, client), and what lightening does to a
//! building standing there (0x53ce10, only with a player).
//!
//! The rest of `CDarkTribe` ties the mushroom farms to the dark temple (decompiles
//! `dark/darkland.c`, `dark/mushfarm2.c`, `dark/misc.c`):
//! * vtbl+0 ([`order_mushroom_farmer`], 0x4cb6c0): the temple makes a farmer for a farm.
//! * vtbl+4 ([`send_to_temple`], 0x4cb830): a farmer or slave goes to the temple.
//! * vtbl+8 ([`slave_to_farm`], 0x4cb940): a slave goes to the farm with the fewest slaves.
//! * vtbl+0x18 ([`temple_of`], 0x4cb410): the player's temple.

use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::jobs::state::EntityEvent;

/// .rdata 0x101a430: `{light, dark}` deco type pairs, in table order.
const PAIRS: [(u16, u16); 59] = [
    (1, 27), (2, 28), (3, 23), (4, 24), (5, 25), (6, 26), (7, 27), (8, 28), (9, 29), (10, 29), (11, 31), (12, 31),
    (13, 23), (14, 24), (43, 114), (44, 112), (45, 111), (46, 113), (47, 113), (48, 112), (49, 111), (160, 114),
    (56, 233), (57, 234), (58, 235), (59, 236), (60, 237), (61, 238), (62, 239), (63, 240), (64, 241), (65, 242),
    (66, 243), (67, 244), (68, 245), (69, 246), (70, 247), (71, 248), (72, 249), (73, 250), (74, 251), (75, 252),
    (41, 253), (124, 137), (125, 138), (126, 139), (127, 140), (128, 141), (129, 142), (130, 143), (131, 144),
    (132, 145), (133, 146), (134, 147), (135, 148), (136, 149), (89, 150), (215, 151), (51, 51),
];
/// The shadow herb: never converted.
const SHADOW_HERB: u16 = 216;
const DARK: u8 = 0x04;
const DECO_RING: u8 = 0x40;

/// 0x5620f0: the dark variant of a light deco type (0 = none).
fn dark_of(kind: u16) -> u16 {
    PAIRS.iter().find(|p| p.0 == kind).map_or(0, |p| p.1)
}

/// 0x562120: the light variant of a dark deco type (0 = none; the first pair wins).
fn light_of(kind: u16) -> u16 {
    PAIRS.iter().find(|p| p.1 == kind).map_or(0, |p| p.0)
}

/// .data 0x11f67d8 bit 0x10 (statically initialised): the ground types that never turn dark —
/// water 0..=8, 0x50, 0x51, 0x80, 0x81.
fn no_dark_ground(g: u8) -> bool {
    matches!(g, 0..=8 | 0x50 | 0x51 | 0x80 | 0x81)
}

/// The six neighbours' flat offsets (0x1454108, filled by 0x5afa60): +1, size+1, size, −1,
/// −size−1, −size (the direction order).
pub(crate) fn neighbour(world: &World, n: usize, d: usize) -> Option<usize> {
    let size = world.grid.size() as i64;
    let off = [1, size + 1, size, -1, -size - 1, -size][d % 6];
    usize::try_from(n as i64 + off).ok().filter(|&m| m < world.grid.len())
}

fn ground(world: &World, n: usize) -> u8 {
    world.layers.ground[n]
}

fn is_dark(world: &World, n: usize) -> bool {
    world.layers.flags[n] & DARK != 0
}

/// `CDarkTribe` vtbl+0xc (0x4cb9a0): the node and, with `many`, its six neighbours (the inner ring
/// with 75 % each, game RNG; spiral entry 6 always) turn dark — those not dark yet on ground
/// that can be, not ground 0x2X — and single light holes next to them close (0x4cba70).
pub fn darken_around(world: &mut World, x: i32, y: i32, many: bool) {
    let count = if many { 7 } else { 1 };
    for i in 0..count {
        // `(state & 0xffff0000) > 0x3fffffff`, i.e. `Rand() >= 0x4000`.
        if i >= 1 && i != 6 && world.rng.rand() < 0x4000 {
            continue;
        }
        let (px, py) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
        let Some(n) = world.grid.checked_index(px, py) else { continue };
        let g = ground(world, n);
        if !is_dark(world, n) && !no_dark_ground(g) && g & 0xf0 != 0x20 {
            set_dark(world, px, py, true, 0);
            close_holes(world, px, py, true, 0);
        }
    }
}

/// `CDarkTribe` vtbl+0x10 (0x4cbc80): the node turns light (for `player`), and single dark holes
/// next to it close.
pub fn lighten(world: &mut World, x: i32, y: i32, player: i32) {
    set_dark(world, x, y, false, player);
    close_holes(world, x, y, false, player);
}

/// 0x4cba70: a neighbour of `(x, y)` that still differs from `dark` while all six of its own
/// neighbours already match it gets the same state.
fn close_holes(world: &mut World, x: i32, y: i32, dark: bool, player: i32) {
    for i in 1..=6 {
        let (px, py) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
        let Some(n) = world.grid.checked_index(px, py) else { continue };
        if is_dark(world, n) == dark {
            continue;
        }
        let surrounded = (1..=6).all(|j| match world.grid.checked_index(px + crate::spiral::dx(j), py + crate::spiral::dy(j)) {
            Some(m) => is_dark(world, m) == dark,
            None => true,
        });
        if surrounded {
            set_dark(world, px, py, dark, player);
        }
    }
}

/// 0x4cbb60: node flag 0x04 to `dark`, and the deco object there — or, for a node in an object's
/// ring that the contour check ([`ring_closed`]) accepts, the nearest one around — to its variant.
fn set_dark(world: &mut World, x: i32, y: i32, dark: bool, player: i32) {
    let Some(n) = world.grid.checked_index(x, y) else { return };
    if is_dark(world, n) == dark {
        return;
    }
    if dark {
        world.layers.flags[n] |= DARK;
    } else {
        world.layers.flags[n] &= !DARK;
    }
    // (0x65b210: landscape / minimap redraw, client side.)
    let obj = world.layers.deco_object[n];
    let target = if obj != 0 {
        Some((obj, x, y))
    } else if !ring_closed(world, n, dark) {
        None
    } else if world.layers.flags[n] & DECO_RING == 0 {
        if !dark && player > 0 {
            // 0x4cbc15: a Dark Tribe building whose footprint holds the node is destroyed with
            // the player as its cause (`DestroyBuilding(x, y, player)` 0x53ce10).
            crate::buildings::crush::destroy_building_at(world, x, y, player);
        }
        None
    } else {
        nearby_object(world, x, y).filter(|&(_, ox, oy)| world.grid.checked_index(ox, oy).is_some_and(|m| is_dark(world, m) != dark))
    };
    if let Some((obj, ox, oy)) = target {
        convert(world, obj, ox, oy, dark);
    }
    // `PostAIEvent(0x1d, x, y, dark)` (0x4cbc6f): the land cell again, then every player AI.
    crate::ai::post_event(world, 0x1d, x, y, dark as i32);
}

/// 0x564de0: the first deco object (not a shadow herb) on a ring node (flag 0x40) among spiral
/// entries 1 .. `Last(3)`.
fn nearby_object(world: &World, x: i32, y: i32) -> Option<(u16, i32, i32)> {
    for i in 1..crate::spiral::last(3) {
        let (px, py) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
        let Some(m) = world.grid.checked_index(px, py) else { continue };
        let obj = world.layers.deco_object[m];
        if world.layers.flags[m] & DECO_RING != 0 && obj != 0 && world.entities.by_index(obj as u32).is_some_and(|e| e.kind != SHADOW_HERB) {
            return Some((obj, px, py));
        }
    }
    None
}

/// 0x4cbcc0: deco object `obj` at `(x, y)` is replaced by its dark / light variant with the same
/// amount (`IsAddDecoObjOk`, then `AddDecoObj` 0x563300), or vanishes (effect 0x40, sound 0x45);
/// the node takes the new state.
fn convert(world: &mut World, obj: u16, x: i32, y: i32, dark: bool) {
    let Some(e) = world.entities.by_index(obj as u32) else { return };
    let crate::entity::EntityData::Deco(d) = &e.data else { return };
    if e.kind == SHADOW_HERB {
        return;
    }
    let (kind, amount) = (e.kind, d.amount);
    let new = if dark { dark_of(kind) } else { light_of(kind) };
    crate::deco::remove_at(world, x, y, false);
    if new >= 1 && crate::deco::is_add_deco_obj_ok(world, x, y, new, 0) {
        let _ = crate::deco::add_deco_obj_without_flags(world, x, y, new, amount);
        crate::deco::set_flags_for_object(world, x, y, new);
    } else {
        world.events.push(crate::event::SimEvent::Effect { effect: 0x40, sound: 0x45, x, y });
    }
    if let Some(n) = world.grid.checked_index(x, y)
        && is_dark(world, n) != dark
    {
        if dark {
            world.layers.flags[n] |= DARK;
        } else {
            world.layers.flags[n] &= !DARK;
        }
    }
}

/// `CDarkTribe` vtbl+0x1c (0x4cbf40): whether node `n` (in an object's ring: flag 0x40, or 0x48
/// when lightening) lies on a closed loop of `dark`-state ring nodes around a blocked object. A
/// blocked neighbour on ground above 8 (and, for a node outside the rings, with flag 0x08) gives
/// the start direction; the walk keeps the blocked object on its right and counts its turns;
/// back at the start with a net left turn, the loop is closed.
fn ring_closed(world: &World, n: usize, dark: bool) -> bool {
    let mask = if dark { 0x40 } else { 0x48 };
    let state = if dark { DARK } else { 0 };
    let flags = &world.layers.flags;
    if flags[n] & mask == 0 {
        return false;
    }
    let outside = flags[n] & DECO_RING == 0;
    let mut start = None;
    let mut blocked = 0u32;
    for d in 0..6 {
        let Some(m) = neighbour(world, n, d) else { return false };
        let b = flags[m];
        if b & 1 != 0 {
            blocked += 1;
            if ground(world, m) > 8 && (!outside || b & 8 != 0) {
                start = Some(d);
            }
        }
    }
    let Some(s) = start.filter(|_| blocked < 6) else { return false };
    let mut turns: i32 = 1;
    let mut dir = if s == 5 { 0 } else { s + 1 };
    let mut cur = n;
    for _ in 0..0x4000 {
        let Some(m) = neighbour(world, cur, dir) else { return false };
        let f = flags[m];
        if f & 1 == 0 {
            if f & DARK != state || f & mask == 0 {
                return false;
            }
            turns -= 1;
            dir = if dir == 0 { 5 } else { dir - 1 };
            cur = m;
            if m == n {
                return turns < 0;
            }
        } else {
            if f & mask == 0 {
                return false;
            }
            turns += 1;
            dir = if dir == 5 { 0 } else { dir + 1 };
        }
    }
    false
}

/// `BUILDING_MUSHROOMFARM`, `BUILDING_DARKTEMPLE`, `BUILDING_FORTRESS`.
pub const MUSHROOM_FARM: u16 = 49;
pub const DARK_TEMPLE: u16 = 50;
pub const FORTRESS: u16 = 51;
/// `SETTLER_DARKGARDENER`, `SETTLER_MUSHROOMFARMER`, `SETTLER_SLAVED_SETTLER`.
pub const DARK_GARDENER: u16 = 52;
pub const MUSHROOM_FARMER: u16 = 53;
pub const SLAVE: u16 = 55;

/// The Dark Tribe's values from the game data: frame counts the farm and its pull effect take
/// their timing from, and the mana a farmer brings.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DarkRules {
    /// `GetJobFrameCount(6, 0x14, 0)`: the effect at a mushroom the farm pulls from (0x4e2190;
    /// file 6, not 4, because the hardware-rendering byte 0x116928e is always 0: 0x505e20
    /// returns 0). At least 1.
    pub pull_frames: u8,
    /// Per race, the mushroom farm's patch slots 0 OPEN, 1 FLOW, 2 GROW: `GetJobFrameCount(10 +
    /// race, job, 0)` of their buildingInfo patch jobs, i.e. the images of that `.dil` entry of
    /// the race's building file (0 where the race has no farm).
    pub farm_patch_frames: Vec<[u8; 3]>,
    /// AI config `DTManaPerSphere` (0x415fa0 → 0x11842d4: 2 / 6 / 8 per difficulty column, not in
    /// `Balancing.cfg`) in the column every player record points to (0x43c8c0: `+8` = record +
    /// 0xc4, the second column): 6.
    pub mana_per_sphere: i32,
    /// Per race, the manakopter hall's (type 80) patch slots 0..10: `GetJobFrameCount(10 + race,
    /// job, 0)` of their buildingInfo patch jobs; a slot without a patch counts job 0, as the
    /// role's zeroed slot does (`InitHousePatches` 0x540bc0). The hall reads slots 3 and 5.
    #[cfg_attr(feature = "serde", serde(default))]
    pub hall_patch_frames: Vec<[u8; 10]>,
}

impl Default for DarkRules {
    fn default() -> Self {
        DarkRules { pull_frames: 1, farm_patch_frames: Vec::new(), mana_per_sphere: 6, hall_patch_frames: Vec::new() }
    }
}

impl DarkRules {
    pub fn new(data: &s4_formats::gamedata::GameData) -> DarkRules {
        let frames = |g: &s4_formats::gfx::GfxFile, job: usize| g.job_frames(job, 0).map_or(0, |r| r.len().min(255) as u8);
        let pull_frames = s4_formats::gfx::GfxFile::open_default(6).map_or(1, |g| frames(&g, 0x14).max(1));
        let mut farm_patch_frames = vec![[0u8; 3]; crate::rules::RACES];
        for b in data.buildings.iter().filter(|b| b.building as u16 == MUSHROOM_FARM) {
            let race = b.race as usize;
            let Some(slot) = farm_patch_frames.get_mut(race) else { continue };
            let Ok(g) = s4_formats::gfx::GfxFile::open_default(10 + race as u32) else { continue };
            for p in &b.patches {
                // Building files count per `.dil` entry (0x461fe0, files 10..=19).
                if let (Some(job), Some(f)) = (p.job, usize::try_from(p.slot).ok().and_then(|s| slot.get_mut(s))) {
                    *f = g.dil_frames(job as usize).map_or(1, |r| r.len().min(255) as u8);
                }
            }
        }
        let mut hall_patch_frames = vec![[0u8; 10]; crate::rules::RACES];
        for b in data.buildings.iter().filter(|b| b.building as u16 == crate::build::MANACOPTERHALL) {
            let race = b.race as usize;
            let Some(slot) = hall_patch_frames.get_mut(race) else { continue };
            let Ok(g) = s4_formats::gfx::GfxFile::open_default(10 + race as u32) else { continue };
            let count = |job: usize| g.dil_frames(job).map_or(0, |r| r.len().min(255) as u8);
            *slot = [count(0); 10];
            for p in &b.patches {
                if let (Some(job), Some(f)) = (p.job, usize::try_from(p.slot).ok().and_then(|s| slot.get_mut(s))) {
                    *f = count(job as usize);
                }
            }
        }
        DarkRules { pull_frames, farm_patch_frames, mana_per_sphere: 6, hall_patch_frames }
    }
}

/// The player's newest building of `kind` (any state): the head of `CBuildingMgr`'s per-player,
/// per-type list (0x14eea74 + (player · 83 + type) · 4), which `CheckInBuilding` (0x53f050 shows
/// the insert) fills at the head. [inf] Newest = highest unique id, i.e. the list is filled when
/// the building entity is created.
fn newest_building(world: &World, player: u8, kind: u16) -> Option<u16> {
    world
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == player && e.kind == kind && !e.is_dead())
        .max_by_key(|(_, e)| e.uid)
        .map(|(i, _)| i)
}

/// `CDarkTribe` vtbl+0x18 (0x4cb410; 0x4cb460 is the same with the role): the head of the
/// player's dark temple list, or of the fortress list when there is no temple, if it is READY.
pub fn temple_of(world: &World, player: u8) -> Option<u16> {
    let t = newest_building(world, player, DARK_TEMPLE).or_else(|| newest_building(world, player, FORTRESS))?;
    world.entities.by_index(t as u32).filter(|e| e.flags & flag::READY != 0).map(|_| t)
}

fn sector_at(world: &World, p: crate::grid::Y16X16) -> u16 {
    world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.tiling.sector_of(n))
}

fn ensign(world: &World, b: u16) -> crate::grid::Y16X16 {
    match world.entities.by_index(b as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => d.ensign,
        _ => crate::grid::Y16X16(0),
    }
}

/// `CBuilding::Attach(settler)` for a settler (vtbl+0x74, 0x5370e0 → `CObserverList::Attach`
/// 0x56e0a0): on the building's observer list, then `CSettler::SetObserverTarget(0, building)`
/// (0x581710): the role's home and entity flag ATTACHED.
pub fn attach_settler(world: &mut World, building: u16, settler: u16) {
    crate::buildings::attach(world, building, settler);
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.home = building;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags |= flag::ATTACHED;
    }
}

/// `CDarkTribe` vtbl+0 (0x4cb6c0): mushroom farm `farm` wants a farmer. Its player's temple
/// ([`temple_of`]) on the same sector (both ensigns) makes one ([`crate::buildings::dark_temple::make_settler`]),
/// who is attached to the farm and sent there (event 1). Returns the farmer, 0 when none was
/// made (another sector, no room), −1 when `farm` is no mushroom farm or there is no temple.
pub fn order_mushroom_farmer(world: &mut World, farm: u16) -> i32 {
    let Some(e) = world.entities.by_index(farm as u32) else { return -1 };
    if e.obj_type != ObjType::Building || e.kind != MUSHROOM_FARM {
        return -1;
    }
    let player = e.owner;
    let Some(temple) = temple_of(world, player) else { return -1 };
    if sector_at(world, ensign(world, temple)) != sector_at(world, ensign(world, farm)) {
        return 0;
    }
    let farmer = crate::buildings::dark_temple::make_settler(world, temple, player, MUSHROOM_FARMER, 0);
    if farmer == 0 {
        return 0;
    }
    // 0x5816f0: `farm->vtbl+0x74(farmer)`.
    attach_settler(world, farm, farmer);
    crate::jobs::core::set_event(world, farmer, EntityEvent { event: 1, kind: 0, data: [farm as i32, 0, 0] });
    farmer as i32
}

/// `CDarkTribe` vtbl+4 (0x4cb830): settler `settler` goes to its player's temple (attached to
/// it, event 2 with the temple and `full`: a farmer whose farm is full takes his second anim
/// list). Without a temple it gets its default list (`NewToDoList` of list `type`). Returns 1,
/// 0 without a temple, −1 when `settler` is no settler.
pub fn send_to_temple(world: &mut World, settler: u16, full: bool) -> i32 {
    let Some(e) = world.entities.by_index(settler as u32) else { return -1 };
    if e.obj_type != ObjType::Settler {
        return -1;
    }
    match temple_of(world, e.owner) {
        Some(t) => {
            attach_settler(world, t, settler);
            crate::jobs::core::set_event(world, settler, EntityEvent { event: 2, kind: 0, data: [t as i32, full as i32, 0] });
            1
        }
        None => {
            crate::jobs::core::take_wait_list(world, settler);
            0
        }
    }
}

/// `CDarkTribe` vtbl+8 (0x4cb940): a living slave goes to a mushroom farm ([`choose_farm`];
/// 0x4cb610: attached, the farm's slave count +1, event 1), or without one to the temple.
pub fn slave_to_farm(world: &mut World, slave: u16) {
    let Some(e) = world.entities.by_index(slave as u32) else { return };
    if e.obj_type != ObjType::Settler || e.flags & flag::DIED != 0 || e.kind != SLAVE {
        return;
    }
    let farm = choose_farm(world, slave);
    if farm == 0 {
        send_to_temple(world, slave, false);
        return;
    }
    attach_settler(world, farm, slave);
    crate::buildings::mushfarm::add_slave(world, farm);
    crate::jobs::core::set_event(world, slave, EntityEvent { event: 1, kind: 0, data: [farm as i32, 0, 0] });
}

/// 0x4cb4c0: the slave's farm — of the player's READY mushroom farms on the slave's sector
/// (at the ensign), newest first, one with fewer than 5 slaves; a farm with `c` slaves at grid
/// distance `d` (ensign − slave) beats the best so far (`bc`, `bd`; start 5 and 0x4000) when
/// `c − bc < −1`, or `c − bc < 2` and `(c − bc) · 50 + d < bd`. 0 = none.
fn choose_farm(world: &World, slave: u16) -> u16 {
    let Some(e) = world.entities.by_index(slave as u32) else { return 0 };
    let (player, pos) = (e.owner, e.pos);
    let sector = sector_at(world, pos);
    let mut farms: Vec<(u32, u16)> = world
        .entities
        .iter()
        .filter(|(_, b)| b.obj_type == ObjType::Building && b.owner == player && b.kind == MUSHROOM_FARM && !b.is_dead())
        .map(|(i, b)| (b.uid, i))
        .collect();
    farms.sort_by_key(|f| std::cmp::Reverse(f.0));
    let (mut best, mut bc, mut bd) = (0u16, 5i32, 0x4000i32);
    for (_, f) in farms {
        if world.entities.by_index(f as u32).is_none_or(|b| b.flags & flag::READY == 0) {
            continue;
        }
        let en = ensign(world, f);
        if sector_at(world, en) != sector {
            continue;
        }
        let d = crate::spiral::grid_distance(en.x() - pos.x(), en.y() - pos.y());
        let c = crate::buildings::mushfarm::slaves(world, f);
        let dc = c - bc;
        if c < 5 && (dc < -1 || (dc < 2 && dc * 50 + d < bd)) {
            (best, bc, bd) = (f, c, d);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_follow_the_pair_table() {
        assert_eq!(dark_of(1), 27);
        assert_eq!(dark_of(7), 27);
        assert_eq!(light_of(27), 1, "the first pair wins");
        assert_eq!(dark_of(51), 51);
        assert_eq!(dark_of(216), 0);
        assert_eq!(light_of(253), 41);
    }
}
