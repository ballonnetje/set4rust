//! `CBuildingMgr::CheckForBuild` (debug BuildingMgr.cpp:2550, release 0x53e4c0) and
//! `CBuildingMgr::CheckForBuildCalc` (BuildingMgr.cpp:2205, release 0x53dc90).

use super::BuildInfo;
use crate::World;
use crate::grid::Grid;

/// `MAX_DIG_RANGE`: the leveling window is `(2·15 + 1)²` nodes around the building.
const DIG: i32 = 15;
const W: usize = 31;

/// Flags that must be clear on the building node and every footprint cell (`0x5F`: blocked land,
/// repelled, 0x04, building, being dug, 0x40 = no-build ring of a deco object).
const ILLEGAL: u8 = 0x5F;

/// The ground-type groups (`ground >> 4`) a race may build on (`CheckForBuild`'s `v54`):
/// Romans 1, 3; Vikings 1, 3, 9; Mayas 1, 3, 4; Trojans 1, 3, 4, 9. Mines: group 2 (rock).
fn ground_groups(race: u8) -> u32 {
    match race {
        1 => 522,
        2 => 26,
        4 => 538,
        _ => 10,
    }
}

/// Why a place is impossible (all are the original's `-1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Why {
    /// Within 16 nodes of the map edge.
    Edge,
    /// Port / shipyard: no orientation fits (`CheckForBuildWater` 0x53f210 returned −1).
    Water,
    /// No buildingInfo record for the race and type.
    NoInfo,
    /// The building node has flags 0x5F set.
    NodeFlags,
    /// The building node is not the player's land.
    NodeOwner,
    /// A footprint cell has flags 0x5F set (`CTiling::CheckFlagBits`).
    FootprintFlags,
    /// A footprint cell is not the player's land.
    CellOwner,
    /// A footprint cell's ground type is not allowed for the race / for mines.
    Ground,
    /// A footprint cell on group-3 ground (or, for mines, outside rock 32) has a flagged neighbour.
    NeighbourFlags,
}

/// Outcome of a placement check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CheckResult {
    /// `-1`: impossible here (position, flags, territory, ground).
    Impossible(Why),
    /// `0`: the ground cannot be leveled (slopes run into fixed nodes).
    CannotLevel,
    /// `1..=8`: possible; the class of the leveling effort (1 = nothing or almost nothing to
    /// dig, 8 = the most). The marker is helper object `10 + class − 1` (mines `74`).
    Possible { class: u8, effort: u32 },
}

impl CheckResult {
    /// The original's integer result.
    pub fn code(self) -> i32 {
        match self {
            CheckResult::Impossible(_) => -1,
            CheckResult::CannotLevel => 0,
            CheckResult::Possible { class, .. } => class as i32,
        }
    }
}

/// The leveling plan of [`calc`]: target height per window node and the summed difference.
pub(crate) struct DigPlan {
    /// `[32·dx + dy]` for window offsets `dx, dy` in 0..31 (centre 15, 15).
    pub heights: [u8; 32 * W],
    /// Σ |target − current| over the inner window (at least 1).
    pub effort: u32,
    /// The mean height of the dig mask the plan levels to.
    pub mean: u8,
}

/// The class of an effort (`CheckForBuild` after the calc): < 40 → 1, < 90 → 2, < 150 → 3,
/// < 220 → 4, < 300 → 5, < 390 → 6, < 490 → 7, else 8.
fn effort_class(e: u32) -> u8 {
    match e {
        0..40 => 1,
        40..90 => 2,
        90..150 => 3,
        150..220 => 4,
        220..300 => 5,
        300..390 => 6,
        390..490 => 7,
        _ => 8,
    }
}

/// `CheckForBuild(x, y, player, type, 0)`. A shipyard (31) or port (32) is checked as the
/// orientation [`water_variant`] picks.
pub fn check_for_build(world: &World, x: i32, y: i32, player: u8, building: u8) -> CheckResult {
    check(world, x, y, player, player, building).0
}

/// The leveling plan `CheckForBuild` would hand to `SetDiggingInfos`: the effort and the target
/// height of every node of the 31×31 window (`[32·dx + dy]`, offsets + 15), or None when the place
/// is not possible. For tools and oracles.
pub fn dig_plan(world: &World, x: i32, y: i32, player: u8, building: u8) -> Option<(u32, Vec<u8>)> {
    match check(world, x, y, player, player, building) {
        (CheckResult::Possible { effort, .. }, Some(p)) => Some((effort, p.heights.to_vec())),
        _ => None,
    }
}

/// [`check_for_build`] plus the dig plan (for the build command).
/// `owner` is the territory owner the land must have (the player; 0 for the editor's "anywhere").
pub(crate) fn check(world: &World, x: i32, y: i32, player: u8, owner: u8, building: u8) -> (CheckResult, Option<DigPlan>) {
    let (result, plan, _) = check_typed(world, x, y, player, owner, building);
    (result, plan)
}

/// [`check`] plus the type the site gets: `building` itself, or for a shipyard / port the
/// orientation (`CheckForBuild` 0x53e4e0 replaces `type` by 0x15ab038, the 0x53f210 result).
pub(crate) fn check_typed(world: &World, x: i32, y: i32, player: u8, owner: u8, building: u8) -> (CheckResult, Option<DigPlan>, u8) {
    use CheckResult::*;
    let size = world.grid.size() as i32;
    // Grid::InQuadrat(x − 16, y − 16, size − 32).
    if x < 16 || y < 16 || x >= size - 16 || y >= size - 16 {
        return (Impossible(Why::Edge), None, building);
    }
    // The water check takes the player, not the owner.
    let water = building == 31 || building == 32;
    let building = if water {
        match water_variant(world, x, y, player, building, 0) {
            Some(t) => t,
            None => return (Impossible(Why::Water), None, building),
        }
    } else {
        building
    };
    let (result, plan) = check_resolved(world, x, y, player, owner, building, water);
    (result, plan, building)
}

fn check_resolved(world: &World, x: i32, y: i32, player: u8, owner: u8, building: u8, water: bool) -> (CheckResult, Option<DigPlan>) {
    use CheckResult::*;
    let race = world.race_of(player);
    if race == 3 {
        return check_dark_tribe(world, x, y, player, building);
    }
    let rules = &world.rules.build;
    let mine = rules.is_mine(building);
    let Some(info) = rules.info(race, building) else { return (Impossible(Why::NoInfo), None) };
    if !water {
        // The water check did the node, footprint, territory and ground tests itself.
        if let Some(why) = footprint(world, info, x, y, owner, race, mine) {
            return (Impossible(why), None);
        }
        if mine {
            return (Possible { class: 1, effort: 1 }, None);
        }
    }
    // Target height: the rounded mean over digPosLines (0x53e9c5).
    let (g, l) = (world.grid, &world.layers);
    let (mut n, mut sum) = (0u32, 0u32);
    for (cx, cy) in info.cells(&info.dig_lines, x, y) {
        if let Some(i) = g.checked_index(cx, cy) {
            n += 1;
            sum += l.height[i] as u32;
        }
    }
    if n == 0 {
        // v62 stays uninitialised in the original; no shipped building has an empty dig mask.
        return (CannotLevel, None);
    }
    let avg = ((sum + (n >> 1)) / n) as u8;
    match calc(world, info, x, y, owner, avg) {
        Some(plan) => (Possible { class: effort_class(plan.effort), effort: plan.effort }, Some(plan)),
        None => (CannotLevel, None),
    }
}

/// The land tests of `CheckForBuild` (0x53e5d4..0x53e74a): the building node, then every
/// footprint cell's flags, owner and ground.
fn footprint(world: &World, info: &BuildInfo, x: i32, y: i32, owner: u8, race: u8, mine: bool) -> Option<Why> {
    let (g, l) = (world.grid, &world.layers);
    let node = g.index(x as u32, y as u32);
    if l.flags[node] & ILLEGAL != 0 {
        return Some(Why::NodeFlags);
    }
    if world.owner_of(node) != owner {
        return Some(Why::NodeOwner);
    }
    // g_pTiling->CheckFlagBits(x − hotX, y − hotY, rows, buildingPosLines, 0x5F).
    if !flag_bits_clear(world, info, x, y) {
        return Some(Why::FootprintFlags);
    }
    for (cx, cy) in info.cells(&info.building_lines, x, y) {
        let i = g.index(cx as u32, cy as u32);
        if world.owner_of(i) != owner {
            return Some(Why::CellOwner);
        }
        let ground = l.ground[i];
        let group = ground & 0xF0;
        if mine {
            // Mines: rock (group 2) or the grass/rock band 17; outside 32 no blocked neighbour.
            if ground != 17 && group != 32 {
                return Some(Why::Ground);
            }
            if ground != 32 && neighbours(g, cx, cy).any(|j| l.flags[j] & 1 != 0) {
                return Some(Why::NeighbourFlags);
            }
        } else {
            if ground == 21 {
                return Some(Why::Ground);
            }
            if group == 48 && neighbours(g, cx, cy).any(|j| l.flags[j] & ILLEGAL != 0) {
                return Some(Why::NeighbourFlags);
            }
            if ground_groups(race) & (1 << (group >> 4)) == 0 {
                return Some(Why::Ground);
            }
        }
    }
    None
}

/// `CTiling::CheckFlagBits(x − hotX, y − hotY, rows, buildingPosLines, 0x5F)` (tiling vtbl+0x40).
fn flag_bits_clear(world: &World, info: &BuildInfo, x: i32, y: i32) -> bool {
    let (g, l) = (world.grid, &world.layers);
    info.cells(&info.building_lines, x, y).all(|(cx, cy)| g.checked_index(cx, cy).is_some_and(|i| l.flags[i] & ILLEGAL == 0))
}

/// The shipyard orientations (.rdata 0x101a1e0, for type 31) and the port orientations
/// (0x101a1c0, type 32), in the order `CheckForBuildWater` tries them.
const SHIPYARDS: [u8; 8] = [58, 59, 60, 61, 62, 63, 76, 77];
const PORTS: [u8; 8] = [52, 53, 54, 55, 56, 57, 78, 79];
/// .rdata 0x101a1a0: per orientation, the first of the two neighbour directions
/// (`g_sNeighborPoints` 0x101bd88) at which the same orientation must not fit.
const FACING: [usize; 8] = [2, 3, 3, 5, 5, 0, 4, 1];

/// Land a water building's node or footprint cell may stand on (0x53f22c / 0x53f5f7): ground
/// group 0x10, or group 0x30 with no neighbour flagged 0x5F.
fn water_land_ok(world: &World, x: i32, y: i32, i: usize) -> bool {
    let l = &world.layers;
    match l.ground[i] & 0xF0 {
        0x10 => true,
        0x30 => neighbours(world.grid, x, y).all(|j| l.flags[j] & ILLEGAL == 0),
        _ => false,
    }
}

/// `CheckForBuildWater(x, y, player, type, mode)` (release 0x53f210; type 31 shipyard, 32
/// port): the orientation of the building to place at (x, y), or None (the release's −1; its
/// result goes to 0x15ab038).
///
/// The node must be free (flags & 0x5F), the player's, and on [`water_land_ok`] land. Then each
/// orientation (all eight for mode 0, only `mode − 10` for mode ≥ 10) is scored:
/// * the bounding rect (+0x34..+0x37) stays one node inside the map, the water flags at
///   `workingpos` are open water (`& 0xc300 == 0xc000`), the footprint has no flag 0x5F;
/// * score = the water cells (ground group 0) of `waterPosLines`; a water cell with flag 0x08 or
///   water flag 0x200, or a land cell that is not free group 0x10 / 0x30 land, scores −1;
/// * only a score above the best so far (starting at 0) is tested further: every
///   `waterBlockPosLines` cell without flag 0x08 and water flags `& 0xc200 == 0xc000`, every
///   `waterFreePosLines` cell without flag 0x08 and water flags `& 0xcb00 == 0xc000`, every
///   footprint cell the player's and on [`water_land_ok`] land.
///
/// Mode 0 then rejects the place if the chosen orientation also fits at either of the two
/// neighbours `FACING`, `FACING + 1` (mod 6) — a recursive call with mode 10 + orientation —
/// so of a row of fitting nodes only the one farthest that way is offered.
pub fn water_variant(world: &World, x: i32, y: i32, player: u8, building: u8, mode: i32) -> Option<u8> {
    let (g, l) = (world.grid, &world.layers);
    let wf = &world.tiling.water_flags;
    let n = g.checked_index(x, y)?;
    if l.flags[n] & ILLEGAL != 0 || world.owner_of(n) != player || !water_land_ok(world, x, y, n) {
        return None;
    }
    let table = match building {
        31 => &SHIPYARDS,
        32 => &PORTS,
        _ => return None,
    };
    let race = world.race_of(player);
    let lim = g.size() as i32 - 1;
    let range = if mode > 9 { (mode - 10) as usize..=(mode - 10) as usize } else { 0..=7 };
    let (mut best, mut chosen) = (0i32, None);
    for v in range {
        let Some(info) = world.rules.build.info(race, table[v]) else { continue };
        let [x0, x1, y0, y1] = info.bounds.map(i32::from);
        if !(0 < x + x0 && x + x1 < lim && 0 < y + y0 && y + y1 < lim) {
            continue;
        }
        let working = g.checked_index(x + info.working.0 as i32, y + info.working.1 as i32);
        if working.is_none_or(|i| wf[i] & 0xC300 != 0xC000) || !flag_bits_clear(world, info, x, y) {
            continue;
        }
        let mut score = 0i32;
        for (cx, cy) in info.cells(&info.water_lines, x, y) {
            let Some(i) = g.checked_index(cx, cy) else {
                score = -1;
                break;
            };
            let group = l.ground[i] & 0xF0;
            let ok = if group == 0 {
                l.flags[i] & 8 == 0 && wf[i] & 0x200 == 0
            } else {
                (group == 0x30 || group == 0x10) && l.flags[i] & ILLEGAL == 0
            };
            if !ok {
                score = -1;
                break;
            }
            score += (group == 0) as i32;
        }
        if score <= best {
            continue;
        }
        let water_ok = |lines: &[u32], mask: u16| {
            info.cells(lines, x, y).all(|(cx, cy)| g.checked_index(cx, cy).is_some_and(|i| l.flags[i] & 8 == 0 && wf[i] & mask == 0xC000))
        };
        if !water_ok(&info.water_block_lines, 0xC200) || !water_ok(&info.water_free_lines, 0xCB00) {
            continue;
        }
        let land_ok = info.cells(&info.building_lines, x, y).all(|(cx, cy)| {
            g.checked_index(cx, cy).is_some_and(|i| world.owner_of(i) == player && water_land_ok(world, cx, cy, i))
        });
        if land_ok {
            best = score;
            chosen = Some(v);
        }
    }
    let v = chosen?;
    if mode < 10 {
        let mut d = FACING[v];
        for _ in 0..2 {
            let (dx, dy) = crate::tiling::NEIGHBORS[d];
            if water_variant(world, x + dx, y + dy, player, building, v as i32 + 10).is_some() {
                return None;
            }
            d = if d < 5 { d + 1 } else { 0 };
        }
    }
    Some(table[v])
}

/// `CheckForBuildDarkTribe` (release 0x53ebc0, from `CheckForBuild` 0x53e4e0 for race 3): every
/// footprint cell (`buildingPosLines`) must be dark land (node flags & 0x5F = 4, whoever owns
/// it) of ground group 0x10 / 0x40 / 0x90 or ground 0x30, not 0x15; then the rounded mean height
/// of `digPosLines` goes through [`calc`] (owner = the player). The release returns 1 (possible),
/// 0 (cannot level) or −column of the first bad cell; here [`CheckResult::Possible`] with class 1.
fn check_dark_tribe(world: &World, x: i32, y: i32, player: u8, building: u8) -> (CheckResult, Option<DigPlan>) {
    use CheckResult::*;
    let size = world.grid.size() as i32;
    if x < 15 || y < 15 || x > size - 16 || y > size - 16 {
        return (Impossible(Why::Edge), None);
    }
    let Some(info) = world.rules.build.info(3, building) else { return (Impossible(Why::NoInfo), None) };
    let (g, l) = (world.grid, &world.layers);
    for (cx, cy) in info.cells(&info.building_lines, x, y) {
        let Some(i) = g.checked_index(cx, cy) else { return (Impossible(Why::FootprintFlags), None) };
        let ground = l.ground[i];
        if l.flags[i] & 0x5F != 4 {
            return (Impossible(Why::FootprintFlags), None);
        }
        if ground == 0x15 || (!matches!(ground & 0xF0, 0x10 | 0x40 | 0x90) && ground != 0x30) {
            return (Impossible(Why::Ground), None);
        }
    }
    let (mut n, mut sum) = (0u32, 0u32);
    for (cx, cy) in info.cells(&info.dig_lines, x, y) {
        if let Some(i) = g.checked_index(cx, cy) {
            n += 1;
            sum += l.height[i] as u32;
        }
    }
    if n == 0 {
        // No dig mask (no shipped building): the release levels from its last plan; not here.
        return (Possible { class: 1, effort: 1 }, None);
    }
    let mean = ((sum + (n >> 1)) / n) as u8;
    match calc(world, info, x, y, player, mean) {
        Some(plan) => (Possible { class: 1, effort: plan.effort }, Some(plan)),
        None => (CannotLevel, None),
    }
}

/// The six hex neighbours (`CWorldManager::NeighborRelIndex(0..6)`) that are in the world.
fn neighbours(g: Grid, x: i32, y: i32) -> impl Iterator<Item = usize> {
    crate::tiling::NEIGHBORS.iter().filter_map(move |&(dx, dy)| g.checked_index(x + dx, y + dy))
}

/// `CheckForBuildCalc(x, y, player, owner, type, height)`: level the dig mask to `height` and let
/// the change run down the slopes of the 31×31 window. Allowed steps between a node and its
/// neighbours (−1,−1) and (0,−1): at most 5 down, 7 up; to (+1,+1) and (0,+1): 7 down, 5 up.
/// A node whose height must change but is fixed (window border, blocked/building/no-build flags
/// 0x49, someone else's land, group-3 ground, being dug) or already queued makes the place
/// impossible.
///
/// As the release code does it, the queue of nodes still to propagate is cleared at the end of
/// every pass over the window, so a node queued behind the scan position is adjusted but never
/// propagates further.
pub(crate) fn calc(world: &World, info: &BuildInfo, x: i32, y: i32, owner: u8, height: u8) -> Option<DigPlan> {
    const FIXED: u8 = 100;
    let idx = |j: usize, i: usize| 32 * j + i;
    let g = world.grid;
    let l = &world.layers;
    let mut h = [0u8; 32 * W];
    let mut s = [0u8; 32 * W];
    let si = idx;
    for i in 0..W {
        let wy = y + i as i32 - DIG;
        for j in 0..W {
            let wx = x + j as i32 - DIG;
            s[si(j, i)] = 0;
            let n = g.index(wx as u32, wy as u32);
            h[idx(j, i)] = l.height[n];
            let interior = j != 0 && i != 0 && j != W - 1 && i != W - 1;
            if !interior {
                s[si(j, i)] = FIXED;
            } else if l.flags[n] & 0x10 != 0 {
                for (dj, di) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
                    s[si((j as i32 + dj) as usize, (i as i32 + di) as usize)] = FIXED;
                }
            } else if l.flags[n] & 0x49 == 0 && world.owner_of(n) == owner {
                if l.ground[n] & 0xF0 == 48 {
                    s[si(j, i)] = FIXED;
                }
            } else {
                s[si(j, i)] = FIXED;
            }
        }
    }
    // Stamp the dig mask: target height, queued.
    for (r, &m) in info.dig_lines.iter().enumerate() {
        if m == 0 {
            continue;
        }
        let dy = r as i32 - info.hotspot.1 as i32;
        for c in 0..32 {
            if m & (0x8000_0000u32 >> c) != 0 {
                let dx = c - info.hotspot.0 as i32;
                let (j, i) = (dx + DIG, dy + DIG);
                if (0..W as i32).contains(&j) && (0..W as i32).contains(&i) {
                    h[idx(j as usize, i as usize)] = height;
                    s[si(j as usize, i as usize)] = 1;
                }
            }
        }
    }
    // Neighbour (dj, di), "down" limit, "up" limit.
    const STEPS: [(i32, i32, i32, i32); 4] = [(-1, -1, 5, 7), (0, -1, 5, 7), (1, 1, 7, 5), (0, 1, 7, 5)];
    let mut changed = true;
    while changed {
        changed = false;
        for i in 1..W - 1 {
            for j in 1..W - 1 {
                if s[si(j, i)] != 1 {
                    continue;
                }
                s[si(j, i)] = FIXED;
                let v = h[idx(j, i)] as i32;
                for (dj, di, down, up) in STEPS {
                    let (nj, ni) = ((j as i32 + dj) as usize, (i as i32 + di) as usize);
                    for lower in [true, false] {
                        let hn = h[idx(nj, ni)] as i32;
                        let off = if lower { hn + down < v } else { hn - up > v };
                        if !off {
                            continue;
                        }
                        match s[si(nj, ni)] {
                            0 => {
                                s[si(nj, ni)] = 1;
                                h[idx(nj, ni)] = (if lower { v - down } else { v + up }) as u8;
                                changed = true;
                            }
                            1 | 2 | FIXED => return None,
                            _ => {}
                        }
                    }
                }
            }
        }
        for i in 1..W - 1 {
            for j in 1..W - 1 {
                if s[si(j, i)] == 1 {
                    s[si(j, i)] = 0;
                }
            }
        }
    }
    let mut effort = 0u32;
    for i in 1..W - 1 {
        for j in 1..W - 1 {
            let cur = l.height[g.index((x + j as i32 - DIG) as u32, (y + i as i32 - DIG) as u32)] as i32;
            effort += (h[idx(j, i)] as i32 - cur).unsigned_abs();
        }
    }
    Some(DigPlan { heights: h, effort: effort.max(1), mean: height })
}

/// `CInputProcessor::GetPossibleBuildingSites` / `UpdateScreenPossibleBuildingSites`: the marker
/// of every node in `x0..x1 × y0..y1` (clamped to 4..size−5) where `player` may build `building`,
/// as `(x, y, helper object)`: `10 + class − 1` (classes 1..=8), mines `74 + class − 1`. Nodes in
/// 16×16 squares without any of the player's land are skipped, as the original does.
pub fn possible_sites(world: &World, player: u8, building: u8, (x0, y0): (i32, i32), (x1, y1): (i32, i32)) -> Vec<(u16, u16, u8)> {
    let size = world.grid.size() as i32;
    let lim = size - 5;
    let (x0, x1, y0, y1) = (x0.clamp(4, lim), x1.clamp(4, lim), y0.clamp(4, lim), y1.clamp(4, lim));
    let base = if world.rules.build.is_mine(building) { 74 } else { 10 };
    let squares = owned_squares(world, player);
    let sq = (size as usize).div_ceil(16);
    let mut out = Vec::new();
    for y in y0..=y1 {
        let mut x = x0;
        while x < x1 {
            if !squares[(y as usize >> 4) * sq + (x as usize >> 4)] {
                // `j = (j + 16) & ~15` inside the loop body, then the loop's `++j`: the first
                // node of the next square is skipped too.
                x = ((x + 16) & !15) + 1;
                continue;
            }
            if let CheckResult::Possible { class, .. } = check_for_build(world, x, y, player, building) {
                out.push((x as u16, y as u16, base + class - 1));
            }
            x += 1;
        }
    }
    out
}

/// `COwnerMap::OwnerBits1XY`: the 16×16 squares holding any node of `player`.
fn owned_squares(world: &World, player: u8) -> Vec<bool> {
    let size = world.grid.size() as usize;
    let sq = size.div_ceil(16);
    let mut v = vec![false; sq * sq];
    for i in 0..size * size {
        if world.owner_of(i) == player {
            v[((i / size) >> 4) * sq + ((i % size) >> 4)] = true;
        }
    }
    v
}
