//! `CWalkingBase` (Pathing/Walking.cpp): the walking state machine of a moving entity, with the
//! subclasses `CWalkingNormal` (type 0) and `CWalkingWorker` (type 1).
//!
//! Read from the shipped exe and checked against the debug decompilation (identical logic, the
//! debug build only adds traces/asserts): vtables by RTTI (`CWalkingBase` .rdata 0x1060f4c,
//! `CWalkingNormal` 0x1060eb4, `CWalkingWorker` 0x1060e68), `Init` 0x5b8d70, `Walk` 0x5b91f0,
//! `IdleWalk` 0x5b98b0, `GoalCheck` 0x5b8fa0, `NextCoarseWaypoint` 0x5b9110, Worker
//! `IsNotBlocked` 0x5ba190 / `SectorId` 0x5ba1c0 / `FindPathAStar64` 0x5ba1e0 (docs/SIM.md §11.2).
//!
//! Per call `Walk(currentXY)` returns the step word (see `jobs::walking`): a direction 0..5 or ≥ 6
//! (no step), or-ed with 0x10 (no path wear), 0x20 (arrived), 0x40 (failed). Coarse waypoints come
//! from [`astar_tiling`] (tile centres), the fine moves between them from [`astar64`] (up to 16
//! directions per search).
//!
//! `CWalkingCatapult` (type 3, vtable 0x1060f00; the land vehicles') overrides: `IsNotBlocked`
//! 0x5ba2a0 (a catapult-tiling node), `IsNotOccupied` 0x5ba2c0 (the node and its six neighbours
//! free: the vehicle's seven-node footprint), `SectorId` 0x5ba360 / `TileId` 0x5ba390 on the
//! catapult tiling, `FindPathAStar64` 0x5ba3b0 (`g_cAStar64Catapult` with the walker's own
//! footprint marks lifted), and `CWalking::Create` 0x5badc0 gives it the base flags 0x200 (coarse
//! search mode 2, no owner bits); `CWalkingShip` gets 0x100.
//!
//! `CWalkingShip` (type 4, vtable 0x1060e1c) dispatches to `vehicles::ship`: `IsNotBlocked`
//! 0x5ba7d0, `IsNotOccupied` 0x5ba800, `IdleWalk` 0x5ba8a0, `FindPathAStar64` 0x5ba820 (its own
//! mark lifted) and the ship branch of `Walk` (0x5b96c6); `SectorId` / `TileId` are the base's.
//!
//! Not ported: walking groups (`AttachWalking` / `GroupLeaderWalking`, the `+0xb4..0xbc` links;
//! every walk here is its own leader). `CWalkingCatapult::IdleWalk` is 0x5ba480. The coarse search gets
//! [`crate::World::alliances`].

use std::sync::OnceLock;

use super::{DirCache, Variant, Waypoints, astar_tiling, astar64, distance_fast};
use crate::entity::EntityData;
use crate::grid::{Y16X16, flag};
use crate::hash::StateHasher;
use crate::world::World;

/// `T_WALKING_TYPE` (`CWalking::Create`).
pub mod walking_type {
    pub const NORMAL: u8 = 0;
    pub const WORKER: u8 = 1;
    pub const CATAPULT: u8 = 3;
    pub const SHIP: u8 = 4;
}

/// Walk flag bits (`m_iFlags`, +0x30). Bits 0..3 are the owner (from `+0x34`); the names are
/// inferred from their use.
pub mod walk_flag {
    /// `CWalkingWorker` ctor: stay in own territory (the `CAStarTiling` bit).
    pub const OWN_TERRITORY: u32 = 0x10;
    /// Accept a partial coarse path (`CAStarTiling` 0x40); the walk retargets to its end.
    pub const PARTIAL: u32 = 0x40;
    /// `GoalCheck`: arrived when within 1; aim at a same-sector neighbour of the target.
    pub const NEAR: u32 = 0x1000;
    /// Fine path stuck (3 tries) within 3 of the target counts as arrived.
    pub const CLOSE_ENOUGH: u32 = 0x2000;
    /// After arriving stay in state 4 (returns arrived without an idle step).
    pub const HOLD: u32 = 0x4000;
    /// `IdleWalk`: the last idle direction / target is kept.
    pub const IDLE_KEEP: u32 = 0x20000;
    /// Group follower (`InitIfLeader`): arriving returns to idle (state 0).
    pub const FOLLOWER: u32 = 0x40000;
    /// `Init` keeps only these caller bits (`WALK_FLAG_STRICT_INTERNAL_USE_MASK` = 0xBC000).
    pub const INIT_MASK: u32 = 0xFFF4_3060;
}

/// `Y16X16::g_iNeighborY16X16Modifiers` (.rdata 0x101bd58): direction → packed delta.
const MODIFIER: [i32; 6] = [1, 0x10001, 0x10000, -1, -0x10001, -0x10000];
const NEIGHBOR: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];
/// `Y16X16::DirectionFast` table (.rdata 0x101c8fc).
const DIRECTION: [u8; 8] = [0, 1, 2, 2, 5, 3, 4, 3];

/// The `CWalkingBase::SData` block (`+4 .. +0xc0`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WalkState {
    /// `GetWalkingType` (`CWalking::Create(type, owner)`), see [`walking_type`].
    pub kind: u8,
    /// +8 walk state 0..9 (0 idle, 1/2 failed, 3..5 arrived, 6 fine step, 7 goal check, 8 coarse
    /// search, 9 fine search).
    pub state: i32,
    /// +0xc last idle direction.
    pub idle_dir: i32,
    /// +0x10 `m_iWalkToXY` (-1 none).
    pub walk_to: i32,
    /// +0x14 `m_iWalkToXY2`: what the coarse search aims at.
    pub walk_to2: i32,
    /// +0x18 the target last given to `Init` (re-applied by `Walk` when it differs).
    pub requested: i32,
    /// +0x1c `IdleWalk`'s target when standing on a cost-7 node.
    pub idle_target: i32,
    /// +0x20 world index of `walk_to`.
    pub walk_to_index: i32,
    /// +0x24 tries on the current fine waypoint.
    pub fine_tries: i32,
    /// +0x28 `m_iFineWaypointXY`.
    pub fine: i32,
    /// +0x2c `m_iCoarseWaypointXY`.
    pub coarse: i32,
    /// +0x30 flags ([`walk_flag`]).
    pub flags: u32,
    /// +0x34 owner (bits 0..3) and the subclass bits.
    pub base_flags: u32,
    /// +0x38 ship wait counter (the ship branch of `Walk`).
    pub ship_wait: i32,
    /// +0x44 group target (groups not ported: only ever reset).
    pub group_target: i32,
    /// +0x48 `m_cCoarseWaypoints`.
    pub coarse_waypoints: Waypoints,
    /// +0xa0 `m_cFineWaypoints`.
    pub fine_dirs: DirCache,
}

impl Default for WalkState {
    fn default() -> Self {
        WalkState::new(walking_type::NORMAL, 0)
    }
}

impl WalkState {
    /// `CWalking::Create(type, owner)`: the ctor zeroes the data (the caches included) and sets
    /// state 1; `CWalkingWorker` adds 0x10 to the owner bits.
    pub fn new(kind: u8, owner: u8) -> WalkState {
        let base = match kind {
            walking_type::WORKER => (owner & 0xF) as u32 | walk_flag::OWN_TERRITORY,
            walking_type::CATAPULT => 0x200,
            walking_type::SHIP => 0x100,
            _ => (owner & 0xF) as u32,
        };
        WalkState {
            kind,
            state: 1,
            idle_dir: 0,
            walk_to: 0,
            walk_to2: 0,
            requested: 0,
            idle_target: 0,
            walk_to_index: 0,
            fine_tries: 0,
            fine: 0,
            coarse: 0,
            flags: 0,
            base_flags: base,
            ship_wait: 0,
            group_target: 0,
            coarse_waypoints: Waypoints {
                goal: 0,
                goal_value: 0,
                total: 0,
                count: 0,
                head: 0,
                ring: [0; 16],
                fully_cached: false,
            },
            fine_dirs: DirCache { count: 0, head: 0, ring: [0; 16] },
        }
    }

    fn owner(&self) -> u8 {
        (self.base_flags & 0xF) as u8
    }

    /// `CWalkingBase::Init(targetXY, flags)` (vtbl+8, `WalkToXY`): a negative target stops the
    /// walk (-1 → idle state 0, other → state 1).
    pub fn init(&mut self, w: &World, target: i32, flags: u32) {
        let f = flags & walk_flag::INIT_MASK;
        if target < 0 {
            self.walk_to = -1;
            self.walk_to_index = 0;
            self.state = (target != -1) as i32;
        } else {
            self.walk_to = target;
            self.walk_to_index = raw_index(w, target);
            self.state = 8;
        }
        self.group_target = 0;
        self.walk_to2 = self.walk_to;
        self.flags = self.base_flags | f;
        self.fine_tries = 0;
        if f & walk_flag::FOLLOWER == 0 {
            self.requested = self.walk_to;
        }
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.kind);
        for v in [
            self.state,
            self.idle_dir,
            self.walk_to,
            self.walk_to2,
            self.requested,
            self.idle_target,
            self.walk_to_index,
            self.fine_tries,
            self.fine,
            self.coarse,
            self.flags as i32,
            self.base_flags as i32,
            self.ship_wait,
            self.group_target,
        ] {
            h.u32(v as u32);
        }
        let c = &self.coarse_waypoints;
        h.u32(c.goal as u32);
        h.u32(c.goal_value as u32);
        h.u32(c.total);
        h.u32(c.count);
        h.u8(c.head);
        c.ring.iter().for_each(|&p| h.u32(p));
        h.bool(c.fully_cached);
        let d = &self.fine_dirs;
        h.u8(d.count);
        h.u8(d.head);
        d.ring.iter().for_each(|&x| h.u8(x));
    }
}

/// `CWorldManager::Index(packedXY)` without a range check.
fn raw_index(w: &World, p: i32) -> i32 {
    let p = Y16X16(p as u32);
    p.y().wrapping_mul(w.grid.size() as i32).wrapping_add(p.x())
}

/// `InWorldPackedXY` + `Index`.
fn node(w: &World, p: i32) -> Option<usize> {
    let p = Y16X16(p as u32);
    w.grid.checked_index(p.x(), p.y())
}

/// A raw index (`Index + rel`), `None` outside the map array.
fn at(w: &World, i: i64) -> Option<usize> {
    (0..w.grid.len() as i64).contains(&i).then_some(i as usize)
}

/// `CWorldManager::NeighborRelIndex(d)`.
fn neighbor_rel(w: &World, d: usize) -> i64 {
    let (dx, dy) = NEIGHBOR[d];
    dy as i64 * w.grid.size() as i64 + dx as i64
}

fn distance(a: i32, b: i32) -> i32 {
    distance_fast(Y16X16(a as u32), Y16X16(b as u32))
}

/// `Y16X16::DirectionFast(from, to)` (0x575370).
pub(crate) fn direction(from: i32, to: i32) -> usize {
    let (a, b) = (Y16X16(from as u32), Y16X16(to as u32));
    let (dx, dy) = (b.x() - a.x(), b.y() - a.y());
    let i = (dx <= 2 * dy) as usize | ((2 * dx <= dy) as usize) << 1 | ((dx < -dy) as usize) << 2;
    DIRECTION[i] as usize
}

fn move_cost(w: &World, n: Option<usize>) -> u8 {
    n.map_or(7, |n| w.layers.move_cost(n))
}

fn hex15() -> &'static [(i32, i32)] {
    static HEX: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    HEX.get_or_init(|| crate::tiling::hex_points(15))
}

fn tile_id(w: &World, n: Option<usize>) -> u16 {
    n.and_then(|n| w.tiling.normal.get(n).copied()).unwrap_or(0)
}

/// `CWalkingCatapult` `TileId` (0x5ba390): the catapult tiling's id, ids from 0x8000 count as 0.
fn catapult_tile(w: &World, n: Option<usize>) -> u16 {
    n.and_then(|n| w.tiling.catapult.get(n).copied()).filter(|&t| t < 0x8000).unwrap_or(0)
}

/// The walker's tile at `n` (vtbl+0x40).
fn walker_tile(w: &World, s: &WalkState, n: Option<usize>) -> u16 {
    if s.kind == walking_type::CATAPULT { catapult_tile(w, n) } else { tile_id(w, n) }
}

/// vtbl+20 `IsNotBlocked`: base not blocked land; Worker: a real tile of the walker's owner;
/// Catapult: a node of the catapult tiling (id 1..=0xbfff, 0x5ba2a0).
pub fn is_not_blocked(w: &World, s: &WalkState, n: Option<usize>) -> bool {
    let Some(n) = n else { return false };
    if s.kind == walking_type::CATAPULT {
        return w.tiling.catapult.get(n).is_some_and(|&t| t.wrapping_sub(1) < 0xbfff);
    }
    if s.kind == walking_type::SHIP {
        return crate::vehicles::ship::not_blocked(w, Some(n));
    }
    if s.kind == walking_type::WORKER {
        let t = tile_id(w, Some(n));
        t >= 10 && w.tiling.tiles.get(t as usize).is_some_and(|t| t.owner() == s.owner())
    } else {
        w.layers.flags[n] & flag::BLOCKED_LAND == 0
    }
}

/// vtbl+24 `IsNotOccupied`: `OccupyingEntityId == 0` (blocked land has none).
pub fn is_not_occupied(w: &World, n: Option<usize>) -> bool {
    let Some(n) = n else { return false };
    w.layers.flags[n] & flag::BLOCKED_LAND != 0 || w.layers.map_object[n] == 0
}

/// vtbl+24 for the walker: Catapult (0x5ba2c0) wants the node and its six neighbours free.
pub fn walker_not_occupied(w: &World, s: &WalkState, n: Option<usize>) -> bool {
    if s.kind == walking_type::SHIP {
        return crate::vehicles::ship::not_occupied(w, n);
    }
    if s.kind != walking_type::CATAPULT {
        return is_not_occupied(w, n);
    }
    let Some(n) = n else { return false };
    is_not_occupied(w, Some(n)) && (0..6).all(|d| is_not_occupied(w, at(w, n as i64 + neighbor_rel(w, d))))
}

/// vtbl+28 `SectorId`: base the tile's sector, Worker its eco-sector, Catapult the sector of its
/// catapult tile (0x5ba360).
fn sector_id(w: &World, s: &WalkState, n: Option<usize>) -> i32 {
    let t = w.tiling.tiles.get(walker_tile(w, s, n) as usize);
    t.map_or(0, |t| if s.kind == walking_type::WORKER { t.eco_sector } else { t.sector }) as i32
}

/// vtbl+68 `FindPathAStar64` on `g_cAStar64Normal` / `Worker` / `Catapult`. The catapult walker
/// (0x5ba3b0) lifts the footprint marks 1..=6 around `from` for the search.
fn find_path64(w: &mut World, s: &mut WalkState, from: i32, to: i32) -> bool {
    let v = match s.kind {
        walking_type::WORKER => Variant::Worker,
        walking_type::CATAPULT => Variant::Catapult,
        // CWalkingShip 0x5ba820: the ship's own mark is lifted for the search.
        walking_type::SHIP => {
            return crate::vehicles::ship::with_own_mark_lifted(w, from, |w| astar64(w, Variant::Ship, Y16X16(from as u32), Y16X16(to as u32), &mut s.fine_dirs));
        }
        _ => Variant::Normal,
    };
    let mut lifted = [None; 6];
    if s.kind == walking_type::CATAPULT
        && let Some(c) = node(w, from)
    {
        for (d, l) in lifted.iter_mut().enumerate() {
            if let Some(m) = at(w, c as i64 + neighbor_rel(w, d))
                && w.layers.map_object[m] == d as u16 + 1
            {
                w.layers.map_object[m] = 0;
                *l = Some(m);
            }
        }
    }
    let r = astar64(w, v, Y16X16(from as u32), Y16X16(to as u32), &mut s.fine_dirs);
    for (d, l) in lifted.into_iter().enumerate() {
        if let Some(m) = l {
            w.layers.map_object[m] = d as u16 + 1;
        }
    }
    r
}

/// `CWalkingBase::GoalCheck(currentXY)`: arrival and sector checks before a (re)search.
fn goal_check(w: &World, s: &mut WalkState, cur: i32) {
    let sec = sector_id(w, s, node(w, cur));
    if sec == 0 {
        s.state = 1; // "Current sector id is 0!"
        return;
    }
    let target_sec = sector_id(w, s, at(w, s.walk_to_index as i64));
    if s.flags & walk_flag::NEAR != 0 {
        if distance(cur, s.walk_to) <= 1 {
            s.state = 3;
        } else if sec == target_sec {
            s.walk_to2 = s.walk_to;
        } else if sec != sector_id(w, s, node(w, s.walk_to2)) {
            let found =
                (0..6).find(|&d| sec == sector_id(w, s, at(w, s.walk_to_index as i64 + neighbor_rel(w, d))));
            match found {
                Some(d) => s.walk_to2 = s.walk_to.wrapping_add(MODIFIER[d]),
                None => s.state = 1,
            }
        }
    } else if cur == s.walk_to {
        s.state = 3;
    } else if s.flags & walk_flag::PARTIAL == 0 && sec != target_sec {
        s.state = 1;
    }
}

/// `CWalkingBase::NextCoarseWaypoint(currentXY)`: skip waypoints on the current tile or a tile
/// linked to it; false when the next one is in another sector or the cache runs out.
fn next_coarse_waypoint(w: &World, s: &mut WalkState, cur: i32) -> bool {
    let cn = node(w, cur);
    let (sec, tile) = (sector_id(w, s, cn), walker_tile(w, s, cn));
    loop {
        let c = node(w, s.coarse);
        let ct = walker_tile(w, s, c);
        if sector_id(w, s, c) != sec {
            return false;
        }
        let linked = w.tiling.tiles.get(ct as usize).is_some_and(|t| t.links.contains(&tile));
        if ct != tile && !linked {
            return true;
        }
        s.coarse = s.coarse_waypoints.back().0 as i32;
        s.coarse_waypoints.pop_back();
        if s.coarse == s.walk_to2 {
            return true;
        }
        if s.coarse_waypoints.count == 0 {
            return false;
        }
    }
}

/// Does the entity on `fine` walk its own fine step onto `cur` (a head-on swap)?
fn head_on(w: &World, fine: i32, cur: i32) -> bool {
    let Some(n) = node(w, fine) else { return false };
    if w.layers.flags[n] & flag::BLOCKED_LAND != 0 {
        return false;
    }
    let id = w.layers.map_object[n];
    if id == 0 {
        return false;
    }
    match w.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Settler(o)) => o.job.walking.as_ref().is_some_and(|o| o.state == 6 && o.fine == cur),
        _ => false,
    }
}

/// `CWalkingBase::IdleWalk(currentXY, flags)` (vtbl+16): a random step to a free neighbour,
/// weighted by the free nodes around it, or — standing on a move-cost-7 node (inside a building
/// footprint) — the way out to the nearest cost < 7 node of the same sector. 8 = no step.
pub(crate) fn idle_walk(w: &mut World, s: &mut WalkState, cur: i32, a3: u32) -> u32 {
    if s.kind == walking_type::CATAPULT {
        return catapult_idle_walk(w, s, cur);
    }
    if s.kind == walking_type::SHIP {
        return crate::vehicles::ship::idle_walk(w, s, cur);
    }
    let Some(cn) = node(w, cur) else { return 8 };
    let mut v = [0i32; 7];
    let (mut free, mut cheap, mut blocked, mut occupied) = (0, 0, 0, 0);
    for (d, vd) in v.iter_mut().take(6).enumerate() {
        let n = at(w, cn as i64 + neighbor_rel(w, d));
        if !is_not_blocked(w, s, n) {
            blocked += 1;
        } else if !is_not_occupied(w, n) {
            occupied += 1;
        } else {
            free += 1;
            if move_cost(w, n) >= 7 {
                *vd = 1;
            } else {
                *vd = 4;
                cheap += 1;
            }
        }
    }
    v[6] = v[0];
    if (occupied != 0 && (blocked != 0 || occupied > 2)) || move_cost(w, Some(cn)) != 7 {
        s.idle_target = -1;
        if s.flags & walk_flag::IDLE_KEEP != 0 && (0..6).contains(&s.idle_dir) {
            // Dead in the original too: v holds 0, 1 or 4, never 2.
            let d = s.idle_dir as usize;
            if v[(d + 5) % 6] == 2 && v[d] == 2 && v[d + 1] == 2 {
                s.flags &= !walk_flag::IDLE_KEEP;
                return d as u32;
            }
        }
        if free >= 1 && (a3 & 0x1000_0000 != 0 || occupied >= 1) {
            // Ring 2 (`SurroundingHexPointRelIndex(7..19)`): 2 = free and cheap, 1 = free.
            let size = w.grid.size() as i64;
            let mut r2 = [0i32; 12];
            for (k, &(dx, dy)) in hex15()[7..19].iter().enumerate() {
                let n = at(w, cn as i64 + dy as i64 * size + dx as i64);
                if is_not_blocked(w, s, n) && is_not_occupied(w, n) {
                    r2[k] = if move_cost(w, n) < 7 { 2 } else { 1 };
                }
            }
            if cheap >= 3 && cheap < free {
                v.iter_mut().filter(|x| **x == 1).for_each(|x| *x = 0);
            }
            let o = v;
            for m in 0..6 {
                let left = if m == 0 { o[5] } else { o[m - 1] };
                v[m] = o[m] * (r2[2 * m + 1] + r2[2 * m] + r2[(2 * m + 11) % 12] + o[m + 1] + left + 2 * o[m]);
            }
            let mut total = 0i32;
            for x in v.iter_mut().take(6) {
                if *x > 0 {
                    total += *x;
                    *x = total;
                }
            }
            if total > 0 {
                total = (w.rng.rand() % total as u32) as i32;
            }
            if let Some(d) = (0..6).find(|&d| total < v[d]) {
                s.flags |= walk_flag::IDLE_KEEP;
                s.idle_dir = d as i32;
                return d as u32;
            }
        }
        s.flags &= !walk_flag::IDLE_KEEP;
        return 8;
    }
    // On a cost-7 node: head for the nearest way out.
    let sec = sector_id(w, s, Some(cn));
    if s.flags & walk_flag::IDLE_KEEP == 0 {
        s.idle_target = -1;
    }
    s.flags &= !walk_flag::IDLE_KEEP;
    let t = node(w, s.idle_target);
    if t.is_none() || move_cost(w, t) == 7 || {
        let ts = sector_id(w, s, t);
        ts <= 0 || ts != sec
    } {
        s.idle_target = -1;
    }
    if s.idle_target < 0 {
        let (x, y) = (Y16X16(cur as u32).x(), Y16X16(cur as u32).y());
        let found = hex15().iter().find_map(|&(dx, dy)| {
            let n = w.grid.checked_index(x + dx, y + dy)?;
            (sector_id(w, s, Some(n)) == sec && is_not_occupied(w, Some(n)) && move_cost(w, Some(n)) < 7)
                .then_some((x + dx, y + dy))
        });
        let Some((nx, ny)) = found else { return 8 };
        s.idle_target = Y16X16::pack(nx, ny).0 as i32;
    }
    let mut d = direction(cur, s.idle_target);
    let left = (d + 5) % 6;
    if v[d] < v[left] {
        // The original compares both turns against the same (left) neighbour and rolls the
        // game RNG to pick between them; either way it turns left.
        w.rng.rand();
        d = left;
    }
    if v[d] <= 0 {
        s.idle_target = -1;
        return 8;
    }
    d as u32
}

/// `CWalkingCatapult::IdleWalk(currentXY)` (0x5ba480): no step unless the vehicle stands on a
/// cost-7 node or next to a building's cost-7 node (flag 0x08); then it heads for the kept way
/// out (+0x1c, while [`walk_flag::IDLE_KEEP`] holds and it is still a free node of the sector), or
/// the first node of the radius-15 hex spiral in its sector, under cost 7, with room for the
/// footprint and no building cost-7 node within two rings — one step towards it if that
/// neighbour is catapult ground. 8 = no step. Map-edge nodes never step.
fn catapult_idle_walk(w: &mut World, s: &mut WalkState, cur: i32) -> u32 {
    let p = Y16X16(cur as u32);
    let (x, y, size) = (p.x(), p.y(), w.grid.size() as i32);
    if !(1..size - 1).contains(&x) || !(1..size - 1).contains(&y) {
        return 8;
    }
    let Some(cn) = node(w, cur) else { return 8 };
    let near_building = |w: &World, n: usize| (0..6).any(|d| at(w, n as i64 + neighbor_rel(w, d)).is_some_and(|m| w.layers.flags[m] & 0x08 != 0 && move_cost(w, Some(m)) > 6));
    let cost = move_cost(w, Some(cn));
    if cost < 7 && !near_building(w, cn) {
        s.idle_target = -1;
        return 8;
    }
    let sec = sector_id(w, s, Some(cn));
    if s.flags & walk_flag::IDLE_KEEP == 0 {
        s.idle_target = -1;
    }
    s.flags &= !walk_flag::IDLE_KEEP;
    let t = node(w, s.idle_target);
    let ok = t.is_some_and(|t| move_cost(w, Some(t)) != 7 && {
        let ts = sector_id(w, s, Some(t));
        ts >= 1 && ts == sec
    });
    if !ok {
        s.idle_target = -1;
    }
    if s.idle_target < 0 {
        let ring2 = &hex15()[1..19];
        let found = hex15().iter().find_map(|&(dx, dy)| {
            let n = w.grid.checked_index(x + dx, y + dy)?;
            let fits = sector_id(w, s, Some(n)) == sec && move_cost(w, Some(n)) < 7 && walker_not_occupied(w, s, Some(n));
            let clear = || {
                ring2.iter().all(|&(ex, ey)| w.grid.checked_index(x + dx + ex, y + dy + ey).is_none_or(|m| w.layers.flags[m] & 0x08 == 0 || move_cost(w, Some(m)) < 7))
            };
            (fits && clear()).then_some((x + dx, y + dy))
        });
        let Some((nx, ny)) = found else { return 8 };
        s.idle_target = Y16X16::pack(nx, ny).0 as i32;
    }
    let d = direction(cur, s.idle_target);
    if !is_not_blocked(w, s, at(w, cn as i64 + neighbor_rel(w, d))) {
        return 8;
    }
    d as u32
}

/// `CWalkingBase::Walk(currentXY)` (vtbl+12): the next step word.
pub fn walk(w: &mut World, s: &mut WalkState, cur: Y16X16) -> u32 {
    let cur = cur.0 as i32;
    // (The debug build traces "Invalid current position!" here.)
    if s.requested != s.walk_to {
        let f = s.flags & walk_flag::INIT_MASK;
        s.init(w, s.requested, f);
    }
    if !(0..=5).contains(&s.state) {
        s.flags &= !walk_flag::IDLE_KEEP;
        goal_check(w, s, cur);
    }
    let (mut blocked_once, mut search_failed_once, mut fine_checked) = (false, false, false);
    for _ in 0..32 {
        match s.state {
            0 => return idle_walk(w, s, cur, 0) | 0x10,
            1 => {
                s.group_target = 0;
                s.state = 2;
                return 72;
            }
            2 => return idle_walk(w, s, cur, 0) | 0x50,
            3 => {
                s.group_target = 0;
                if s.flags & walk_flag::FOLLOWER != 0 {
                    s.state = 0;
                    return 24;
                }
                s.state = if s.flags & walk_flag::HOLD != 0 { 4 } else { 5 };
                return 40;
            }
            4 => return 40,
            5 => return idle_walk(w, s, cur, 0) | 0x30,
            6 => {
                if !fine_checked {
                    s.fine_tries += 1;
                    if s.fine_tries > 16 {
                        s.fine_tries = 0;
                        s.state = 7;
                        return idle_walk(w, s, cur, 0);
                    }
                    if s.fine_tries > 2 {
                        let close = distance(cur, s.walk_to) <= 3 && s.flags & walk_flag::CLOSE_ENOUGH != 0;
                        s.state = if close { 3 } else { 9 };
                        continue;
                    }
                    if distance(cur, s.fine) == 1 && head_on(w, s.fine, cur) {
                        s.state = 9;
                        continue;
                    }
                    fine_checked = true;
                }
                let dir;
                if s.fine == cur {
                    s.fine_tries = 0;
                    if s.fine_dirs.count == 0 {
                        s.state = 9;
                        continue;
                    }
                    dir = s.fine_dirs.back() as usize % 6;
                    s.fine = cur.wrapping_add(MODIFIER[dir]);
                    s.fine_dirs.pop_back();
                } else {
                    if distance(cur, s.fine) != 1 {
                        s.state = 9;
                        continue;
                    }
                    dir = direction(cur, s.fine);
                }
                // The ship branch (0x5b96c6): other ships near the next node.
                if s.kind == walking_type::SHIP {
                    return crate::vehicles::ship::walk_step(w, s, cur, dir as u32);
                }
                if is_not_blocked(w, s, node(w, s.fine)) {
                    return dir as u32;
                }
                if blocked_once {
                    s.state = 7; // "Waypoint blocked!"
                } else {
                    blocked_once = true;
                    s.state = 9;
                }
            }
            7 => {
                s.state = 8;
                goal_check(w, s, cur);
            }
            8 => {
                let res = match node(w, s.walk_to2) {
                    Some(_) => {
                        let side = w.grid.size() as usize / 16;
                        let to = Y16X16(s.walk_to2 as u32);
                        astar_tiling(&w.tiling, &w.alliances, &w.warmap, side, Y16X16(cur as u32), to, s.flags, &mut s.coarse_waypoints)
                    }
                    None => 0,
                };
                if res == 0 {
                    s.state = 1;
                    continue;
                }
                if res < 0 && s.flags & walk_flag::PARTIAL != 0 {
                    s.walk_to = s.coarse_waypoints.goal;
                    s.walk_to2 = s.walk_to;
                    s.walk_to_index = raw_index(w, s.walk_to);
                }
                s.coarse = s.coarse_waypoints.back().0 as i32;
                s.coarse_waypoints.pop_back();
                s.state = 9;
            }
            9 => {
                s.group_target = 0; // (a group leader would wait for its followers here)
                if s.coarse != s.walk_to2 {
                    let c = &s.coarse_waypoints;
                    if c.count == 0 || (c.count < 2 && !c.goal_cached()) || !next_coarse_waypoint(w, s, cur) {
                        s.state = 8;
                        continue;
                    }
                }
                let target = if s.coarse == s.walk_to2 { s.walk_to } else { s.coarse };
                if find_path64(w, s, cur, target) {
                    let d = s.fine_dirs.back() as usize % 6;
                    s.fine = cur.wrapping_add(MODIFIER[d]);
                    s.fine_dirs.pop_back();
                    s.state = 6;
                } else if search_failed_once {
                    s.state = 1;
                } else {
                    search_failed_once = true; // "FindPathAStar64 failed once [may be ignored]"
                    s.state = 7;
                }
            }
            _ => s.state = 1, // "Walk(): Invalid walk state!"
        }
    }
    s.state = 7; // "LoopCount >= 32!"
    8
}

/// The walking a settler's job code uses (`jobs::walking::Pathing`).
pub struct OriginalPathing;

fn walk_state(world: &mut World, settler: u16) -> Option<&mut Option<Box<WalkState>>> {
    match &mut world.entities.by_index_mut(settler as u32)?.data {
        EntityData::Settler(s) => Some(&mut s.job.walking),
        _ => None,
    }
}

fn walk_state_ref(world: &World, settler: u16) -> Option<&WalkState> {
    match &world.entities.by_index(settler as u32)?.data {
        EntityData::Settler(s) => s.job.walking.as_deref(),
        _ => None,
    }
}

impl crate::jobs::walking::Pathing for OriginalPathing {
    fn create(world: &mut World, settler: u16, kind: u8) {
        let Some(owner) = world.entities.by_index(settler as u32).map(|e| e.owner) else { return };
        if !matches!(kind, walking_type::NORMAL | walking_type::WORKER) {
            // CWalkingCatapult / CWalkingShip walk as Normal for now.
            world.events.push(crate::event::SimEvent::Unported { what: "CWalking::Create(catapult/ship)", entity: settler });
        }
        if let Some(w) = walk_state(world, settler) {
            *w = Some(Box::new(WalkState::new(kind, owner)));
        }
    }

    fn walk_to_xy(world: &mut World, settler: u16, target: Y16X16, flags: i32) {
        let Some(mut s) = walk_state(world, settler).and_then(|w| w.take()) else { return };
        s.init(world, target.0 as i32, flags as u32);
        if let Some(w) = walk_state(world, settler) {
            *w = Some(s);
        }
    }

    fn next_step(world: &mut World, settler: u16) -> u32 {
        let Some(pos) = world.entities.by_index(settler as u32).map(|e| e.pos) else {
            return crate::jobs::walking::WALK_NONE;
        };
        let Some(mut s) = walk_state(world, settler).and_then(|w| w.take()) else {
            return crate::jobs::walking::WALK_NONE;
        };
        let r = walk(world, &mut s, pos);
        if let Some(w) = walk_state(world, settler) {
            *w = Some(s);
        }
        r
    }

    fn idle_step(world: &mut World, settler: u16, flags: u32) -> u32 {
        let Some(pos) = world.entities.by_index(settler as u32).map(|e| e.pos) else {
            return crate::jobs::walking::WALK_NONE;
        };
        let Some(mut s) = walk_state(world, settler).and_then(|w| w.take()) else {
            return crate::jobs::walking::WALK_NONE;
        };
        let r = idle_walk(world, &mut s, pos.0 as i32, flags);
        if let Some(w) = walk_state(world, settler) {
            *w = Some(s);
        }
        r
    }

    fn is_not_blocked(world: &World, settler: u16, node: usize) -> bool {
        let base = WalkState::default();
        is_not_blocked(world, walk_state_ref(world, settler).unwrap_or(&base), at(world, node as i64))
    }

    fn is_not_occupied(world: &World, _settler: u16, node: usize) -> bool {
        is_not_occupied(world, at(world, node as i64))
    }
}
