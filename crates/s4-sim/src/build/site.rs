//! Construction sites: command 5025 (`CheckForBuild(x, y, player, type, 1)`), `AddBuilding` with
//! a `CBuildingSiteRole`, the role's `Init` / `PrepareGround`, and its `LogicUpdate` states as
//! far as they run without workers.

use super::check::{self, CheckResult};
use super::{DigSlot, MANACOPTERHALL, MUSHROOMFARM, PEG};
use crate::World;
use crate::entity::{BuildingData, Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;

/// Digging slots per player (`CBuildingMgr`: `while (slot used && slot != 30)`).
const DIG_SLOTS: u8 = 30;
/// Ground type of a construction site (`PrepareGround`: `SetGround(x, y, 28)`).
const SITE_GROUND: u8 = 28;

/// `CBuildingSiteRole` (the fields this port uses; offsets of the debug object).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Site {
    /// +396: 2 digging, 3 digging + material, 4 waiting for the footprint to be free, 5 building,
    /// 6/7 (manakopter hall, mushroom farm), 8 done.
    pub state: u8,
    /// +380: Σ |height − target| of the nodes to dig.
    pub dig_total: i32,
    /// +384: digging slot (−1 = none).
    pub dig_slot: i32,
    /// +388: diggers wanted, `min(8, ((effort + 16) >> 5) + 1)`.
    pub diggers: i32,
    /// +392: leveling effort still to do.
    pub dig_effort: i32,
    /// +397: diggers at work.
    pub diggers_working: u8,
    /// +401: builders at work; +402: next builder spot (`GetBuilderPos`).
    pub builders_working: u8,
    pub builder_pos: u8,
    /// +403/+405/+407: boards, stones, gold needed (buildingInfo cost); +404/+406/+408 used.
    pub boards: u8,
    pub stones: u8,
    pub gold: u8,
    pub boards_used: u8,
    pub stones_used: u8,
    pub gold_used: u8,
    /// +412: work done (`AddWork`); +448: goods consumed + 1 (the ctor sets 1).
    pub work_done: i32,
    pub goods: i32,
    /// +416: work to do, `200 · (gold + stones + boards)`; +420 = `131070 / that`.
    pub work: i32,
    pub work_step: i32,
    /// +432/+434/+436: the material piles at the flag (entity index, 0 = none).
    pub piles: [u16; 3],
    /// +0x1b8..+0x1bc: the priority flags (command 5004, `build::order::prioritize`).
    pub prio: SitePrio,
    /// +0x1c4: the progress divisor `(work + dig_effort) / 100.0f` (single bits), set by the
    /// first progress query (0x543c60; 0 = not yet).
    #[cfg_attr(feature = "serde", serde(default))]
    pub progress_div: crate::sfloat::Sf,
}

/// A construction site's priority flags (release `CBuildingSiteRole` +0x1b8..+0x1bc).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SitePrio {
    /// +0x1b8: prioritised.
    pub on: bool,
    /// +0x1b9 digger, +0x1ba builder, +0x1bb board, +0x1bc stone priority.
    pub diggers: bool,
    pub builders: bool,
    pub boards: bool,
    pub stone: bool,
}

impl Site {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        h.u8(self.diggers_working);
        h.u8(self.builders_working);
        h.u8(self.builder_pos);
        h.bytes(&[self.boards_used, self.stones_used, self.gold_used]);
        h.u32(self.work_done as u32);
        h.u32(self.goods as u32);
        for v in [self.dig_total, self.dig_slot, self.diggers, self.dig_effort, self.work, self.work_step] {
            h.u32(v as u32);
        }
        h.u8(self.boards);
        h.u8(self.stones);
        h.u8(self.gold);
        h.u16s(&self.piles);
        let p = self.prio;
        h.bytes(&[p.on as u8, p.diggers as u8, p.builders as u8, p.boards as u8, p.stone as u8]);
        h.u32(self.progress_div.0);
    }
}

/// `CWorldManager` ground-adjacency lists used by `CheckField` (release .rdata 0x1027078,
/// `int16[16]` per ground type, −1-terminated): the ground types a node of type `g` may border.
pub const GROUND_NEIGHBOURS: &[(u8, &[u8])] = &[
    (0, &[8, 0, 1]),
    (1, &[0, 1, 2]),
    (2, &[1, 2, 3]),
    (3, &[2, 3, 4]),
    (4, &[3, 4, 5]),
    (5, &[4, 5, 6]),
    (6, &[5, 6, 7]),
    (7, &[6, 7]),
    (8, &[48, 8, 0]),
    (16, &[16, 20, 114, 21, 17, 48, 96, 97, 98, 99, 23, 25, 18, 28, 29]),
    (17, &[16, 17, 33]),
    (18, &[16, 18]),
    (20, &[16, 20, 65]),
    (21, &[16, 21, 81]),
    (23, &[16, 23, 145]),
    (24, &[25, 24]),
    (25, &[16, 25, 24]),
    (28, &[16, 28, 29]),
    (29, &[16, 29, 28]),
    (32, &[33, 32, 35]),
    (33, &[17, 33, 32]),
    (35, &[32, 35, 129]),
    (48, &[16, 48, 8]),
    (64, &[65, 64]),
    (65, &[20, 65, 64]),
    (80, &[81, 80]),
    (81, &[21, 81, 80]),
    (96, &[16, 96, 97, 98, 99, 0, 48, 8]),
    (97, &[16, 96, 97, 98, 99, 0, 48, 8]),
    (98, &[16, 96, 97, 98, 99, 0, 48, 8]),
    (99, &[16, 96, 97, 98, 99, 0, 48, 8]),
    (112, &[113, 112]),
    (113, &[114, 113, 112]),
    (114, &[16, 114, 113]),
    (128, &[129, 128]),
    (129, &[35, 129, 128]),
    (144, &[145, 144]),
    (145, &[23, 145, 144]),
];

fn may_border(g: u8, n: u8) -> bool {
    g == n || GROUND_NEIGHBOURS.iter().find(|(t, _)| *t == g).is_some_and(|(_, l)| l.contains(&n))
}

/// `IGfxEngine::CanChangeGround(x, y, ground)` → `CheckField` (release 0x66d240): every in-world
/// hex neighbour's ground must be allowed next to `ground`. Side effect as in the original: the
/// neighbours' render byte 3 gets bit 0x40.
pub(crate) fn can_change_ground(world: &mut World, x: i32, y: i32, ground: u8) -> bool {
    let mut ok = true;
    for (dx, dy) in [(-1, -1), (0, -1), (0, 1), (1, 1), (-1, 0), (1, 0)] {
        if let Some(n) = world.grid.checked_index(x + dx, y + dy) {
            world.layers.gfx_bits[n] |= 0x40;
            ok &= may_border(ground, world.layers.ground[n]);
        }
    }
    ok
}

/// `CWorldManager::SetGround(x, y, type)`: the type, then (type 28 is land, not blocked)
/// `ClearBlockedLand` and move cost 2.
pub(crate) fn set_ground(world: &mut World, x: i32, y: i32, ground: u8) {
    let Some(n) = world.grid.checked_index(x, y) else { return };
    world.layers.ground[n] = ground;
    world.clear_blocked_land(x, y);
    world.layers.set_move_cost(n, 2);
}

/// Command 5025: `CheckForBuild(x, y, player, type, 1)` — check, take a digging slot, create the
/// site (`AddBuilding(…, true)`), record what must be dug.
pub fn command_build(world: &mut World, player: u8, building: u32, x: u16, y: u16) {
    let (Ok(building), x, y) = (u8::try_from(building), x as i32, y as i32) else { return };
    if !(1..83).contains(&building) || !(1..=8).contains(&player) {
        return;
    }
    // A shipyard / port becomes the orientation the check picked (`AddBuilding` gets it).
    let (result, plan, building) = check::check_typed(world, x, y, player, player, building);
    let CheckResult::Possible { effort, .. } = result else { return };
    if world.race_of(player) == 3 {
        // CheckForBuildDarkTribe (0x53ebc0) with mode 1: no diggers, the land is levelled at
        // once, then `AddBuilding(x, y, player, type, 1)`.
        if let Some(plan) = plan {
            level_dark_tribe(world, x, y, &plan);
        }
        add_building_site(world, x, y, player, building);
        return;
    }
    let mine = world.rules.build.is_mine(building);
    // First free digging slot (mines take none).
    let slot = if mine { 0 } else { (0..DIG_SLOTS).find(|&s| !world.sites.dig_slots.contains_key(&(player, s))).unwrap_or(DIG_SLOTS) };
    if slot == DIG_SLOTS {
        return;
    }
    let Some(id) = add_building_site(world, x, y, player, building) else { return };
    if mine || effort == 1 {
        return;
    }
    let Some(plan) = plan else { return };
    // SetDiggingInfos(slot, effort).
    let mut heights = vec![0u8; 32 * 31];
    let mut total = 0i32;
    for k in 0..31 {
        let wy = y + k - 15;
        for m in 0..31 {
            let wx = x + m - 15;
            let n = world.grid.index(wx as u32, wy as u32);
            let target = plan.heights[(32 * m + k) as usize];
            if world.layers.height[n] != target {
                world.layers.flags[n] |= 0x10;
                heights[(32 * m + k) as usize] = target;
                if target != 0 {
                    total += (world.layers.height[n] as i32 - target as i32).abs();
                }
            }
        }
    }
    world.sites.dig_slots.insert((player, slot), DigSlot { heights });
    if let Some(s) = world.sites.roles.get_mut(&id) {
        s.dig_total += total;
        s.dig_slot = slot as i32;
        s.dig_effort = effort as i32;
        s.diggers = (((effort as i32 + 16) >> 5) + 1).min(8);
    }
}

/// The Dark Tribe's levelling (0x53ebc0, mode 1, 0x53ed7d..0x53ef3a): passes over the 31×31
/// window (row by row, column by column) until nothing changes. A node whose plan height is set
/// (not 0) and differs from its height moves one step towards the dig mask's mean — down when it
/// is above the mean, up otherwise — unless a neighbour would make the slope too steep: going down
/// is refused when (−1,−1) or (0,−1) is 7 higher or (0,+1) or (+1,+1) is 5 higher than the node;
/// going up when (−1,−1) or (0,−1) is 5 lower or (0,+1) or (+1,+1) is 7 lower. Every step
/// refreshes the node's shading (`UpdateWorldPosition` 0x65a4b0).
fn level_dark_tribe(world: &mut World, x: i32, y: i32, plan: &check::DigPlan) {
    let mean = plan.mean as i32;
    let h = |w: &World, cx: i32, cy: i32| w.grid.checked_index(cx, cy).map_or(-1000, |n| w.layers.height[n] as i32);
    loop {
        let mut changed = false;
        for row in 0..31 {
            for col in 0..31 {
                let (cx, cy) = (x - 15 + col, y - 15 + row);
                let Some(n) = world.grid.checked_index(cx, cy) else { continue };
                let target = plan.heights[(32 * col + row) as usize];
                let cur = world.layers.height[n] as i32;
                if target == 0 || target as i32 == cur {
                    continue;
                }
                let (ul, u, d, dr) = (h(world, cx - 1, cy - 1), h(world, cx, cy - 1), h(world, cx, cy + 1), h(world, cx + 1, cy + 1));
                let step = if mean < cur {
                    (ul != cur + 7 && u != cur + 7 && d != cur + 5 && dr != cur + 5).then_some(-1)
                } else {
                    (ul != cur - 5 && u != cur - 5 && d != cur - 7 && dr != cur - 7).then_some(1)
                };
                if let Some(s) = step {
                    world.layers.height[n] = (cur + s) as u8;
                    world.update_world_position(cx, cy);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// `CBuildingMgr::AddBuilding(x, y, player, type, true)` (release 0x53c350) + `CBuilding` ctor +
/// `CBuildingSiteRole::Init`: a building entity that is not stamped into the world yet.
pub fn add_building_site(world: &mut World, x: i32, y: i32, player: u8, type_ex: u8) -> Option<u16> {
    if world.counters.buildings >= 4000 {
        return None;
    }
    // Type 0x50: the position joins the player's hall list (0x53f810) first.
    let second_hall = type_ex == 80 && super::halls::before_add(world, player, x, y);
    world.counters.buildings += 1;
    let race = world.race_of(player);
    let slot = world.entities.free_slot()?;
    let rule = world.rules.building(race, type_ex);
    let door = Y16X16::pack(x + rule.door.0 as i32, y + rule.door.1 as i32);
    let ensign = Y16X16::pack(x + rule.flag.0 as i32, y + rule.flag.1 as i32);
    let data = BuildingData { type_ex, door, ensign, working_area: ensign, health: 0, events: Vec::new() };
    let kind = crate::create::building_kind(type_ex);
    let mut e = Entity::new(ObjType::Building, kind, player, race, Y16X16::pack(x, y), EntityData::Building(data));
    e.flags |= 0x1100;
    if world.rules.is_military(kind) {
        e.flags |= 0xC;
    }
    world.entities.insert(slot, e);
    // The `CBuilding` ctor puts every building, a site too, into the war map (0x535d1a / 0x535d24,
    // category 2).
    crate::warmap::readd(world, slot);
    // `AddBuilding` (0x53c4ab..0x53c4e9): the head of its (player, type) list.
    crate::buildings::list_add(world, slot);
    // Unlike a finished building, a site does not hold its node in the map-object layer: the saves
    // show map object 0 (or a settler passing) at every site node under digging, and the site id
    // only in the deco-object layer (`PrepareGround`); state 4 → 5 sets the map object.
    init(world, slot);
    if second_hall {
        super::halls::destroy_second(world, player, x, y);
        return None;
    }
    Some(slot)
}

/// `CBuildingSiteRole::Init` (release 0x541a50).
fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags = (e.flags & !flag::READY) | 0x100_1000;
    let (race, type_ex) = (e.race, match &e.data {
        EntityData::Building(d) => d.type_ex,
        _ => return,
    });
    let info = world.rules.build.info(race, type_ex).cloned().unwrap_or_default();
    let work = 200 * (info.gold as i32 + info.stone as i32 + info.boards as i32);
    let site = Site {
        dig_slot: -1,
        boards: info.boards,
        stones: info.stone,
        gold: info.gold,
        work,
        // 131070 / work; the original divides by zero for a building without cost.
        work_step: 131_070i32.checked_div(work).unwrap_or(0),
        goods: 1,
        ..Default::default()
    };
    // Not kept: +424 = GetSettlerJobFrameCount(race, 0x35, 2) (0x541af8). Only the ctor
    // (0x54145c), save and load (0x54166f, 0x5418f3) touch +0x1a8 in the site and settler
    // role code (0x540000..0x545000, 0x56c000..0x590000) [inf: a gfx-side value].
    world.sites.roles.insert(id, site);
    prepare_ground(world, id, &info);
    world.register_for_logic_update(id, 16);
    // The AI notice (5, owner, site, 0), release 0x541b71.
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    crate::ai::post_event(world, 5, owner as i32, id as i32, 0);
}

/// `CBuildingSiteRole::PrepareGround` (release 0x543680).
fn prepare_ground(world: &mut World, id: u16, info: &super::BuildInfo) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, kind) = (e.x(), e.y(), e.kind);
    let cells: Vec<(i32, i32)> = info.cells(&info.building_lines, x, y).collect();
    let pos = world.grid.index(x as u32, y as u32);
    if kind == MUSHROOMFARM || kind == MANACOPTERHALL {
        for &(cx, cy) in &cells {
            if let Some(n) = world.grid.checked_index(cx, cy) {
                world.layers.deco_object[n] = 0;
                world.layers.flags[n] |= 8;
            }
        }
        world.layers.deco_object[pos] = id;
        set_state(world, id, 6);
        return;
    }
    // Per spiral direction, the footprint cell farthest from the building gets a stake.
    let mut stakes = [(0u8, 0i32, 0i32); 6];
    for &(cx, cy) in &cells {
        if can_change_ground(world, cx, cy, SITE_GROUND) {
            set_ground(world, cx, cy, SITE_GROUND);
            world.update_world_position(cx, cy); // 0x54379e
        }
        let Some(n) = world.grid.checked_index(cx, cy) else { continue };
        world.layers.deco_object[n] = 0;
        world.layers.flags[n] |= 8;
        if cx != 0 || cy != 0 {
            let k = (0..crate::spiral::LEN).find(|&k| crate::spiral::dx(k) + x == cx && crate::spiral::dy(k) + y == cy);
            if let Some(k) = k {
                let (_, _, r, dir) = crate::spiral::entry(k);
                let d = dir as usize % 6;
                if r >= stakes[d].0 {
                    stakes[d] = (r, cx, cy);
                }
            }
        }
    }
    for (r, sx, sy) in stakes {
        if r >= 1 {
            add_deco_obj(world, sx, sy, PEG);
        }
    }
    world.layers.deco_object[pos] = id;
    // Material piles at the flag: boards right of it, stones left, gold on it.
    let (fx, fy) = (x + info.flag.0 as i32, y + info.flag.1 as i32);
    let mut piles = [0u16; 3];
    for (k, (need, dx, good)) in [(info.boards, 1, 7u8), (info.stone, -1, 32), (info.gold, 0, 14)].into_iter().enumerate() {
        if need > 0 {
            piles[k] = crate::create::add_pile(world, fx + dx, fy, good, 0, 5).unwrap_or(0);
            // The site's vtbl+0x74 (CBuilding 0x5370e0): the pile joins the site's observers and
            // gets it as its target (pile vtbl+0x44), so its arrivals reach the script
            // (GOODARRIVE, 0x56ec10) and a destroyed site leaves free piles.
            if piles[k] != 0 {
                crate::buildings::attach(world, id, piles[k]);
            }
        }
    }
    let state = if world.rules.build.is_mine(kind as u8) { 4 } else { 2 };
    if let Some(s) = world.sites.roles.get_mut(&id) {
        s.piles = piles;
        s.state = state;
    }
    // Ports / shipyards: PrepareGround reads no water mask (0x543680 never reads info +6); the
    // water cells are taken in state 4 (`block_water`).
}

/// `CDecoObjMgr::AddDecoObj(x, y, type, 0, 1)`: create, then `SetFlagsForObject(x, y, type, false)`
/// (release 0x563510: the rings run up to `CSpiralOffsets::First(r)`, the word at
/// `[r·4 + 0x117cf38]`).
fn add_deco_obj(world: &mut World, x: i32, y: i32, kind: u16) {
    if crate::create::add_deco_obj_without_flags(world, x, y, kind, 0).is_err() {
        return;
    }
    let f = world.rules.build.deco_flags.get(kind as usize).copied().unwrap_or_default();
    let first = crate::spiral::first;
    if f.repelling {
        if let Some(n) = world.grid.checked_index(x, y) {
            world.layers.flags[n] |= 2 | 0x40;
        }
    } else {
        for i in 0..first(f.rings) {
            let (px, py) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
            world.set_blocked_land(px, py);
            if let Some(n) = world.grid.checked_index(px, py) {
                world.layers.flags[n] |= 0x40;
            }
        }
    }
    for i in first(f.rings)..first(f.total) {
        if let Some(n) = world.grid.checked_index(x + crate::spiral::dx(i), y + crate::spiral::dy(i)) {
            world.layers.flags[n] |= 0x40;
        }
    }
}

fn set_state(world: &mut World, id: u16, state: u8) {
    if let Some(s) = world.sites.roles.get_mut(&id) {
        s.state = state;
    }
}

/// `CWorldManager::CheckBlockable(x, y)`: nobody stands on the node, and none of its six
/// neighbours holds map object id 1..=6.
fn check_blockable(world: &World, x: i32, y: i32) -> bool {
    let Some(n) = world.grid.checked_index(x, y) else { return false };
    if world.layers.map_object[n] != 0 {
        return false;
    }
    crate::tiling::NEIGHBORS.iter().all(|&(dx, dy)| {
        world.grid.checked_index(x + dx, y + dy).is_none_or(|j| !(1..=6).contains(&world.layers.map_object[j]))
    })
}

/// `CBuildingSiteRole::Switch` (vtbl+0x20, 0x542bf0; command 5003 on a site): flag 0x1000 off
/// stops the site — its priorities go (0x543120: boards, stones, diggers, builders at the
/// ensign's eco-sector), its observers get event 7 (data0 = the site; builders and diggers are
/// set free) and every one but the piles is detached (0x56e1a0), diggers and builders at work and
/// the next builder spot are 0, the eco-sector at the ensign drops the site's requests
/// (0x4d5b70); flag 0x1000 on starts it again — the observers get event 8 (building vtbl+0x7c)
/// and the site updates next tick. The observers are the stand-in of
/// [`crate::buildings::site_observers`].
pub fn switch(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    let working = e.flags & 0x1000 != 0;
    let send = |world: &mut World, o: u16, ev: crate::jobs::state::EntityEvent| match world.entities.by_index(o as u32).map(|e| e.obj_type) {
        Some(ObjType::Pile) => crate::pile::set_event(world, o, ev),
        Some(ObjType::Settler) => crate::jobs::core::set_event(world, o, ev),
        _ => {}
    };
    if !working {
        let ev = crate::jobs::state::EntityEvent { event: 8, kind: 0, data: [id as i32, 0, 0] };
        for o in crate::buildings::site_observers(world, id) {
            send(world, o, ev);
        }
        if let Some(e) = world.entities.by_index_mut(id as u32) {
            e.flags |= 0x1000;
        }
        world.register_for_logic_update(id, 1);
        return;
    }
    e.flags &= !0x1000;
    super::order::deactivate_all_here(world, id);
    let ev = crate::jobs::state::EntityEvent { event: 7, kind: 0, data: [id as i32, 0, 0] };
    let observers = crate::buildings::site_observers(world, id);
    for &o in &observers {
        send(world, o, ev);
    }
    for &o in &observers {
        if world.entities.by_index(o as u32).is_some_and(|e| e.obj_type != ObjType::Pile) {
            crate::buildings::military::set_home(world, o, 0);
        }
    }
    if let Some(s) = world.sites.roles.get_mut(&id) {
        s.diggers_working = 0;
        s.builders_working = 0;
    }
    let eco = match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => world.grid.checked_index(d.ensign.x(), d.ensign.y()).map_or(0, |n| world.tiling.eco_sector_of(n)),
        _ => 0,
    };
    if eco != 0 {
        crate::eco::clean_up_building_need(world, eco, id);
    }
    if let Some(s) = world.sites.roles.get_mut(&id) {
        s.builder_pos = 0;
    }
}

/// `CBuilding::LogicUpdate` for a building with a site role (`CBuildingSiteRole::LogicUpdate`,
/// 0x1501570). Other buildings: TODO(port) (their roles are not ported).
pub fn building_logic_update(world: &mut World, id: u16) {
    if world.sites.roles.contains_key(&id) {
        site_logic_update(world, id);
    }
}

fn site_logic_update(world: &mut World, id: u16) {
    world.register_for_logic_update(id, 15);
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.flags & 0x1000 == 0 {
        return;
    }
    let (x, y, kind, owner, race) = (e.x(), e.y(), e.kind, e.owner, e.race);
    let EntityData::Building(d) = &e.data else { return };
    let type_ex = d.type_ex;
    let ensign = d.ensign;
    let eco = world.grid.checked_index(ensign.x(), ensign.y()).map_or(0, |n| world.tiling.eco_sector_of(n));
    if kind != MUSHROOMFARM && kind != MANACOPTERHALL && eco == 0 {
        return; // "iEcoSectorId != 0"
    }
    let Some(site) = world.sites.roles.get(&id).cloned() else { return };
    match site.state {
        2 => {
            if site.dig_effort <= 0 {
                digging_is_ready(world, id, owner);
                return;
            }
            // Release 0x541cc0: OrderDigger, then `effort / +397 < 25` → state 3.
            super::order::order_digger(world, id);
            let Some(site) = world.sites.roles.get(&id) else { return };
            if site.diggers_working > 0 && site.dig_effort / site.diggers_working as i32 <= 24 {
                set_state(world, id, 3);
            }
        }
        3 => {
            if site.dig_effort <= 0 {
                digging_is_ready(world, id, owner);
                return;
            }
            super::order::order_digger(world, id);
            super::order::order_material(world, id);
        }
        4 => {
            super::order::order_material(world, id);
            let info = world.rules.build.info(race, type_ex).cloned().unwrap_or_default();
            let pos = world.grid.index(x as u32, y as u32);
            let mut busy = false;
            let mut last = None;
            for (cx, cy) in info.cells(&info.block_lines, x, y) {
                if let Some(n) = world.grid.checked_index(cx, cy) {
                    world.layers.set_move_cost(n, 7);
                }
                // The original sets 0x02 on the building's own node here, not on the cell.
                world.layers.flags[pos] |= 2;
                busy |= !check_blockable(world, cx, cy);
                last = Some((cx, cy));
            }
            let rule = world.rules.building(race, type_ex);
            if rule.has_water_masks {
                let (cells, working) = (rule.water_block_cells.clone(), rule.working);
                busy |= block_water(world, x, y, type_ex, &cells, working, last);
            }
            let military = world.rules.is_military(kind);
            let (dx, dy) = (x + info.door.0 as i32, y + info.door.1 as i32);
            if military {
                if let Some(n) = world.grid.checked_index(dx, dy) {
                    world.layers.set_move_cost(n, 7);
                }
                world.layers.flags[pos] |= 2;
                busy |= !check_blockable(world, dx, dy);
            }
            if !busy {
                if military {
                    world.set_blocked_land(dx, dy);
                }
                set_state(world, id, 5);
                world.layers.map_object[pos] = id;
                crate::create::set_to_world(world, id);
                world.layers.deco_object[pos] = 0;
            }
        }
        5 => {
            super::order::order_builder(world, id);
            super::order::order_material(world, id);
            if world.sites.roles.get(&id).is_some_and(|s| s.work_done >= s.work) {
                delete_stakes(world, id);
                building_done(world, id);
            }
        }
        // 6 (`LogicUpdate` 0x541cc0): the mushroom farm / manakopter hall (no diggers, builders or material,
        // `PrepareGround` set it): every `blockPosLines` cell gets move cost 7 and node flag 0x02,
        // then on to 7 at once.
        6 | 7 => {
            let info = world.rules.build.info(race, type_ex).cloned().unwrap_or_default();
            let cells: Vec<(i32, i32)> = info.cells(&info.block_lines, x, y).collect();
            if site.state == 6 {
                for &(cx, cy) in &cells {
                    if let Some(n) = world.grid.checked_index(cx, cy) {
                        world.layers.set_move_cost(n, 7);
                        world.layers.flags[n] |= 2;
                    }
                }
                set_state(world, id, 7);
            }
            // 7: wait (the site updates every 15 ticks) until nobody blocks a cell
            // (`CheckBlockable`, 0x60d870); then a mushroom farm's effect 0x3a, `SetToWorld`
            // (0x536470), the next update in 1 tick and state 8.
            if cells.iter().any(|&(cx, cy)| !check_blockable(world, cx, cy)) {
                return;
            }
            if kind == MUSHROOMFARM {
                world.events.push(crate::event::SimEvent::Effect { effect: 0x3a, sound: 0, x, y });
            }
            crate::create::set_to_world(world, id);
            world.register_for_logic_update(id, 1);
            set_state(world, id, 8);
        }
        // 8: the building holds its node (map object, not the deco layer), loses BIRTH, gets
        // READY and is finished (`CBuilding::BuildingDone` 0x5362f0, not the site's 0x5429f0:
        // there are no piles or workers to let go).
        8 => {
            let pos = world.grid.index(x as u32, y as u32);
            world.layers.map_object[pos] = id;
            world.layers.deco_object[pos] = 0;
            if let Some(e) = world.entities.by_index_mut(id as u32) {
                e.flags = (e.flags & !flag::BIRTH) | flag::READY;
            }
            world.sites.roles.remove(&id);
            crate::buildings::building_done(world, id);
        }
        _ => {}
    }
}

/// The port / shipyard part of the site's state 4 (release 0x541f1b..0x54205f, info byte +6):
/// over the +0x314 cells (`waterPosLines | waterBlockPosLines`), a land cell (flag 0x01 clear)
/// repeats the block-mask step on the release's stale coordinates — the last `blockPosLines`
/// cell (`last`; [inf] none when the mask is empty, the release then reads its argument): move
/// cost 7 there, flag 0x02 on the building node, busy unless `CheckBlockable`; a water cell gets
/// water flag 0x1000 and is busy while a map object stands on it. Every cell loses its deco
/// object and gets flag 0x08. A shipyard (58..=63, 76, 77) also sets water flag 0x1000 at its
/// `workingpos` (+0x2a / +0x2b). Returns whether anything was busy.
fn block_water(world: &mut World, x: i32, y: i32, type_ex: u8, cells: &[(i8, i8)], working: (i8, i8), last: Option<(i32, i32)>) -> bool {
    let pos = world.grid.index(x as u32, y as u32);
    let mut busy = false;
    for &(dx, dy) in cells {
        let Some(n) = world.grid.checked_index(x + dx as i32, y + dy as i32) else { continue };
        if world.layers.flags[n] & 1 == 0 {
            if let Some((sx, sy)) = last {
                if let Some(m) = world.grid.checked_index(sx, sy) {
                    world.layers.set_move_cost(m, 7);
                }
                world.layers.flags[pos] |= 2;
                busy |= !check_blockable(world, sx, sy);
            }
        } else {
            world.tiling.water_flags[n] |= 0x1000;
            busy |= world.layers.map_object[n] != 0;
        }
        world.layers.deco_object[n] = 0;
        world.layers.flags[n] |= 8;
    }
    if matches!(type_ex, 58..=63 | 76 | 77)
        && let Some(n) = world.grid.checked_index(x + working.0 as i32, y + working.1 as i32)
    {
        world.tiling.water_flags[n] |= 0x1000;
    }
    busy
}

/// `CBuildingSiteRole::DeleteStakes` (release 0x5424c0): every `buildingPosLines` cell's object
/// (deco layer) is cleared unless a pile stands there; this removes the stakes and the site's own
/// id at its node.
fn delete_stakes(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, race) = (e.x(), e.y(), e.race);
    let EntityData::Building(d) = &e.data else { return };
    let Some(info) = world.rules.build.info(race, d.type_ex).cloned() else { return };
    for (cx, cy) in info.cells(&info.building_lines, x, y) {
        let Some(n) = world.grid.checked_index(cx, cy) else { continue };
        let o = world.layers.deco_object[n];
        let pile = o != 0 && world.entities.by_index(o as u32).is_some_and(|e| e.obj_type == ObjType::Pile);
        if !pile {
            world.layers.deco_object[n] = 0;
        }
    }
}

/// `CBuildingSiteRole::BuildingDone` (release 0x5429f0): the material piles go (`DeletePile`, which
/// clears their 0x02 — the flag ± 1 misses of SIM.md §10.4), the workers are told event 9 and
/// detached, BIRTH → READY, and the building gets its real role (`CBuilding::Ready`).
fn building_done(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let EntityData::Building(d) = &e.data else { return };
    let ensign = d.ensign;
    if world.grid.checked_index(ensign.x(), ensign.y()).is_none_or(|n| world.tiling.eco_sector_of(n) == 0) {
        return;
    }
    // 0x5429f0 opens with CheckDeactivateUrgent{Boards,Stones,Digger,Builder}(eco-sector).
    super::order::deactivate_all_here(world, id);
    let piles = world.sites.roles.get(&id).map(|s| s.piles).unwrap_or_default();
    for p in piles {
        if p != 0 {
            // vtbl+0x40 (CBuilding 0x537100): out of the observers, the pile's target 0 (its
            // role's SubjectDie), the role's vtbl+0x7c (0x534450: nothing); then DeletePile.
            crate::buildings::detach(world, id, p);
            crate::pile::set_building(world, p, 0);
            crate::pile::delete_pile(world, p);
        }
    }
    // NotifyAndDetachAllObservers(CEntityEvent(9)). [inf] A site has no `buildings` role, so no
    // observer list: the settlers whose home is the site, in entity order, stand in for it.
    let workers: Vec<u16> = world
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Settler && matches!(&e.data, EntityData::Settler(s) if s.job.home == id))
        .map(|(i, _)| i)
        .collect();
    for &w in &workers {
        crate::jobs::core::set_event(world, w, crate::jobs::state::EntityEvent { event: 9, kind: 0, data: [0, 0, 0] });
    }
    // 0x537430 then detaches them all (0x56e130: `SetObserverTarget(0, 0)`, home 0), so their
    // `SetFree` on event 9 finds no home.
    for w in workers {
        crate::buildings::military::set_home(world, w, 0);
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags = (e.flags & !flag::BIRTH) | flag::READY;
    }
    world.sites.roles.remove(&id);
    // CBuilding::Ready: the building's real role (created and initialised by `buildings`).
    crate::buildings::building_done(world, id);
}

/// `CBuildingSiteRole::DiggingIsReady(player)`: state 4, the digging slot is freed.
pub(crate) fn digging_is_ready(world: &mut World, id: u16, player: u8) {
    let Some(s) = world.sites.roles.get_mut(&id) else { return };
    s.state = 4;
    if s.dig_slot != -1 {
        world.sites.dig_slots.remove(&(player, s.dig_slot as u8));
        s.dig_slot = -1;
    }
    // 0x543190: CheckDeactivateUrgentDigger(eco-sector at the ensign).
    super::order::deactivate_diggers_here(world, id);
}

// ------------------------------------------------------------------------------------------------
// Calls from the digger and builder roles (jobs/digger.rs, jobs/builder.rs).

/// The material test `HaveBuildingMaterial` (release 0x542ee0) and `AddWork` (0x5423cd..0x5423d3)
/// inline: a pile whose amount (+0x41) is above the goods carriers are about to take (+0x42).
fn pile_good_available(world: &World, pile: u16) -> bool {
    pile != 0 && matches!(world.entities.by_index(pile as u32).map(|e| &e.data), Some(EntityData::Pile(p)) if p.amount > p.leaving)
}

/// `CPile::DecreaseUnforeseen(n)`, inlined in `AddWork` (release 0x5423e9..0x54242f): amount −
/// n, `CPile::AdjustStatistic(−n, 0)` (0x542412 → 0x56fe80: the eco-sector's and the player's
/// goods count), role vt+0xc, and on an empty pile leaving = 0 and the role's empty notice.
fn pile_decrease(world: &mut World, pile: u16, n: u8) {
    crate::pile::decrease_unforeseen(world, pile, n as i32);
}

/// `CBuildingSiteRole::HaveBuildingMaterial` (release 0x542ee0): any of the three material piles has a
/// good available.
pub(crate) fn have_building_material(world: &World, site: u16) -> bool {
    let Some(s) = world.sites.roles.get(&site) else { return false };
    s.piles.iter().any(|&p| pile_good_available(world, p))
}

/// `CBuildingSiteRole::AddWork(amount)` (release 0x542390): +412 += amount; once it reaches
/// `200 · goods` one more good is used from the first pile (boards, stones, gold) that has one.
pub fn add_work(world: &mut World, site: u16, amount: i32) {
    let Some(s) = world.sites.roles.get_mut(&site) else { return };
    s.work_done += amount;
    if s.work_done < 200 * s.goods {
        return;
    }
    let piles = s.piles;
    for (k, &p) in piles.iter().enumerate() {
        if pile_good_available(world, p) {
            let s = world.sites.roles.get_mut(&site).expect("site");
            s.goods += 1;
            match k {
                0 => s.boards_used += 1,
                1 => s.stones_used += 1,
                _ => s.gold_used += 1,
            }
            pile_decrease(world, p, 1);
            return;
        }
    }
}

/// `CBuildingSiteRole::BuilderLeft(settler)`: one digger (settler type 2) or builder (type 3) less.
pub(crate) fn builder_left(world: &mut World, site: u16, settler: u16) {
    let kind = world.entities.by_index(settler as u32).map(|e| e.kind);
    let Some(s) = world.sites.roles.get_mut(&site) else { return };
    match kind {
        Some(2) => {
            debug_assert!(s.diggers_working > 0, "m_uDiggerAct");
            s.diggers_working = s.diggers_working.saturating_sub(1);
        }
        Some(3) => s.builders_working = s.builders_working.saturating_sub(1),
        _ => {} // "Wrong Builder left."
    }
}

/// `CBuildingSiteRole::GetBuilderPos` (release 0x542770): the spot of the newest builder,
/// `builderInfo[+401 − 1]` relative to the building, as absolute (x, y, dir). Sets +402 to that
/// index + 1 as the original does.
pub(crate) fn get_builder_pos(world: &mut World, site: u16) -> (i32, i32, i8) {
    let Some(e) = world.entities.by_index(site as u32) else { return (0, 0, 0) };
    let (x, y, race) = (e.x(), e.y(), e.race);
    let type_ex = match &e.data {
        EntityData::Building(d) => d.type_ex,
        _ => return (x, y, 0),
    };
    let spot = {
        let Some(s) = world.sites.roles.get(&site) else { return (x, y, 0) };
        debug_assert!(s.builders_working > 0, "m_uBuilderAct");
        s.builders_working.saturating_sub(1)
    };
    let b = world.rules.build.info(race, type_ex).and_then(|i| i.builders.get(spot as usize).copied()).unwrap_or((0, 0, 0));
    if let Some(s) = world.sites.roles.get_mut(&site) {
        s.builder_pos = spot + 1;
    }
    (x + b.0 as i32, y + b.1 as i32, b.2)
}

// ------------------------------------------------------------------------------------------------
// `CBuilding::CrushBuilding` on a construction site (`buildings::crush`), release exe.

/// `CBuildingSiteRole` vtable +0x60 (release 0x543030): half (rounded down) of the boards,
/// stones and gold already used (+0x194 / +0x196 / +0x198) goes into `SearchSpaceForGoods` at the
/// site, in portions of at most 8, for each material whose pile (+0x1b0 / +0x1b2 / +0x1b4)
/// still exists.
pub fn site_drop_materials(world: &mut World, id: u16) {
    let Some(s) = world.sites.roles.get(&id) else { return };
    let used = [(s.piles[0], s.boards_used, 7u8), (s.piles[1], s.stones_used, 32), (s.piles[2], s.gold_used, 14)];
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y) = (e.x(), e.y());
    for (pile, n, good) in used {
        if pile == 0 {
            continue;
        }
        let mut left = n as i32 / 2;
        while left > 0 {
            let k = left.min(8);
            crate::pile::search_space_for_goods(world, x, y, good, k as u8);
            left -= k;
        }
    }
}

/// `CBuildingSiteRole` vtable +0x2c (release 0x542d50). While digging (state 2) the leveling flag
/// 0x10 leaves the nodes of the digging slot's mask and the slot is freed; then the site's id
/// leaves the deco-object layer over the building cells (0x5424c0: entries that are not piles).
/// First, 0x543120 drops the site's priorities (`build::order::deactivate_all`).
pub fn site_crush(world: &mut World, id: u16) {
    super::order::deactivate_all_here(world, id);
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, owner) = (e.x(), e.y(), e.owner);
    let rule_cells = match &e.data {
        EntityData::Building(b) => world.rules.building(e.race, b.type_ex).building_cells.clone(),
        _ => return,
    };
    if let Some(s) = world.sites.roles.get(&id)
        && s.state == 2
        && s.dig_slot != -1
    {
        let slot = s.dig_slot as u8;
        if let Some(d) = world.sites.dig_slots.remove(&(owner, slot)) {
            for k in 0..31 {
                for m in 0..31 {
                    if d.heights.get(32 * m + k).is_some_and(|&h| h != 0)
                        && let Some(n) = world.grid.checked_index(x + m as i32 - 15, y + k as i32 - 15)
                    {
                        world.layers.flags[n] &= !0x10;
                    }
                }
            }
        }
        if let Some(s) = world.sites.roles.get_mut(&id) {
            s.dig_slot = -1;
        }
    }
    for (dx, dy) in rule_cells {
        let Some(n) = world.grid.checked_index(x + dx as i32, y + dy as i32) else { continue };
        let o = world.layers.deco_object[n];
        if world.entities.by_index(o as u32).is_none_or(|e| e.obj_type != ObjType::Pile) {
            world.layers.deco_object[n] = 0;
        }
    }
}

/// `CBuildingSiteRole` progress 0x543c60 (SSE singles; the eco-sector AI's manager slot 0x44):
/// the divisor is fixed by the first call (`(work + dig_effort) / 100.0f`, 1.0 when that is 0),
/// then `clamp(trunc(100.0f − (work − work_done + dig_effort) / divisor), 0, 100)`. [inf] The
/// release's GUI calls it too and may fix the divisor first; s4rs leaves that to the sim's
/// callers.
pub fn progress(world: &mut World, id: u16) -> i32 {
    use crate::sfloat::Sf;
    let Some(s) = world.sites.roles.get_mut(&id) else { return 100 };
    let hundred = Sf::from_i32(100);
    if s.progress_div.is_zero() {
        s.progress_div = Sf::from_i32(s.work.wrapping_add(s.dig_effort)) / hundred;
        if s.progress_div.is_zero() {
            s.progress_div = Sf::ONE;
        }
    }
    let left = Sf::from_i32(s.work.wrapping_sub(s.work_done).wrapping_add(s.dig_effort));
    let v = (hundred - left / s.progress_div).to_i32_trunc();
    v.clamp(0, 100)
}
