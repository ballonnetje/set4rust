//! `CAStarTiling::FindPath(startXY, endXY, waypoints, flags)` (release VA 0x5ac2f0).
//!
//! Flags: bits 0..3 player, 0x10 stay in own territory, 0x20 alliance-weighted cost, 0x40 accept
//! a partial path (to the tile nearest the goal), 0x80 sparse waypoints, bits 8..10 mode
//! (0 land, 1 water, 2 catapult tiling, 3/4 ferry transitions, 5/6 military, 7 allied land).
//!
//! Returns 1 (path; waypoints from the start side at the back), -1 (partial), 2 (modes 5/6:
//! goal only), 0 (no path).

use super::scratch::Stamped;
use super::{Waypoints, distance_fast};
use crate::grid::Y16X16;
use crate::tiling::Tiling;
use crate::warmap::WarMap;

/// What `CAlliances` provides to the path finder ([`crate::alliances`], `World::alliances`).
pub use crate::alliances::Alliances;

const TABLE_MODE3: [u32; 8] =
    [0x2c00_0000, 0x2800_0000, 0x1c00_0000, 0x3c00_0000, 0x3c00_0000, 0x3c00_0000, 0x3c00_0000, 0x3c00_0000];
const TABLE_MODE4: [u32; 8] =
    [0x2c00_0000, 0x2800_0000, 0x1c00_0000, 0x1800_0000, 0x2c00_0000, 0x2c00_0000, 0x3c00_0000, 0x3c00_0000];

#[derive(Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Cost {
    Default,
    Alliance,
    Mode5,
    Mode6,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Rebuild {
    Default,
    Sparse,
    GoalOnly,
    Mode3,
    Mode4,
}

#[derive(Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Closed {
    g: i32,
    parent: u16,
    closed: bool,
    state: u8,
}

/// The 1-based binary min-heap with slot 0 as a key-0 sentinel (sift-up 0x5ad8d0, sift-down
/// 0x5ad930). Keys are `((G + H) << 16) | G`.
struct Heap<'a> {
    e: &'a mut Vec<(u32, u16)>,
}

impl Heap<'_> {
    fn len(&self) -> usize {
        self.e.len() - 1
    }
    fn push(&mut self, key: u32, idx: u16) {
        self.e.push((key, idx));
        let mut p = self.len();
        while self.e[p >> 1].0 > key {
            self.e[p] = self.e[p >> 1];
            p >>= 1;
        }
        self.e[p] = (key, idx);
    }
    fn sift_down(&mut self, mut p: usize) {
        let x = self.e[p];
        let n = self.len();
        while p <= n >> 1 {
            let mut c = 2 * p;
            if c < n && self.e[c].0 > self.e[c + 1].0 {
                c += 1;
            }
            if x.0 <= self.e[c].0 {
                break;
            }
            self.e[p] = self.e[c];
            p = c;
        }
        self.e[p] = x;
    }
    fn pop(&mut self) -> (u32, u16) {
        let top = self.e[1];
        let last = self.e.pop().expect("non-empty");
        if self.len() > 0 {
            self.e[1] = last;
            self.sift_down(1);
        }
        top
    }
    /// Linear search from the back; replace by the last and sift down only.
    fn remove(&mut self, idx: u16) {
        let n = self.len();
        if let Some(j) = (1..=n).rev().find(|&j| self.e[j].1 == idx) {
            let last = self.e.pop().expect("non-empty");
            if j <= self.len() {
                self.e[j] = last;
                self.sift_down(j);
            }
        }
    }
}

struct Ctx<'a> {
    t: &'a Tiling,
    alliances: &'a Alliances,
    warmap: &'a WarMap,
    side: usize,
    player: u8,
    end_xy: Y16X16,

}

impl Ctx<'_> {
    fn centre(&self, id: u16) -> Y16X16 {
        self.t.tiles[id as usize].center
    }
    fn ty(&self, id: u16) -> u32 {
        self.t.tiles[id as usize].ty
    }
    /// The 16×16 square of a tile's centre (`((c >> 20) << 6) + ((c >> 4) & 0xfff)`).
    fn square(&self, id: u16) -> usize {
        let c = self.centre(id);
        (c.y() as usize >> 4) * self.side + (c.x() as usize >> 4)
    }
    /// `COwnerMap::OwnerBits9` (0x12ef999, byte 1 of [`Tiling::owner_bits`]): owners with a tile
    /// in the square or its 8 neighbours, as the owner map has kept them.
    fn owner_bits9(&self, sq: usize) -> u16 {
        let (sx, sy) = ((sq % self.side) as i32, (sq / self.side) as i32);
        self.t.owner_bits(sx, sy)[1] as u16
    }
    /// `cost_default` (0x5abd00): hex distance of the centres, doubled on slow ground (type
    /// bit 30), at least 1.
    fn cost_default(&self, a: u16, b: u16) -> i32 {
        let d = distance_fast(self.centre(a), self.centre(b)) << (((self.ty(b) >> 30) & 1) + 1);
        if d <= 1 { 1 } else { d >> 1 }
    }
    fn cost(&self, kind: Cost, a: u16, b: u16) -> i32 {
        let c = self.cost_default(a, b);
        match kind {
            Cost::Default => c,
            // 0x5abd90: weighted by the foreign strength (war map 0x12dd498: total − the
            // player's team, 0x15ab0e8 = 0x12dcd0c[player]) of the target tile's centre cell.
            Cost::Alliance => {
                let p = self.centre(b);
                let s = self.warmap.cell_strength(p.x() >> 4, p.y() >> 4);
                let diff = s[0] as i32 - s.get(self.alliances.team(self.player)).copied().unwrap_or(0) as i32;
                let r = if diff >= 0x30 { 0x40 } else { diff + 0x10 };
                (r * c) >> 4
            }
            // 0x5abe00.
            Cost::Mode5 => {
                if self.ty(b) & 0xF == self.player as u32 {
                    c >> 1
                } else if self.owner_bits9(self.square(b)) & self.alliances.enemy_bits[self.player as usize] != 0 {
                    c * 2
                } else {
                    c
                }
            }
            // 0x5abe60 (can be 0).
            Cost::Mode6 => {
                if self.owner_bits9(self.square(b)) & self.alliances.enemy_bits[self.player as usize] == 0 {
                    c >> 2
                } else {
                    c
                }
            }
        }
    }
}

/// `CAStarTiling::FindPath`.
#[allow(clippy::too_many_arguments)]
pub fn astar_tiling(t: &Tiling, alliances: &Alliances, warmap: &WarMap, side: usize, start: Y16X16, end: Y16X16, flags: u32, wp: &mut Waypoints) -> i32 {
    let r = SCRATCH.with(|s| match s.try_borrow_mut() {
        Ok(mut s) => search(t, alliances, warmap, side, start, end, flags, wp, &mut s),
        Err(_) => search(t, alliances, warmap, side, start, end, flags, wp, &mut Scratch::new()),
    });
    crate::debug::coarse_path(r, wp.total);
    r
}

/// The search's tables, kept between searches ([`super::scratch`]).
struct Scratch {
    closed: Stamped<Closed>,
    seen: Stamped<bool>,
    heap: Vec<(u32, u16)>,
}

impl Scratch {
    const fn new() -> Self {
        Scratch { closed: Stamped::new(), seen: Stamped::new(), heap: Vec::new() }
    }
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = const { std::cell::RefCell::new(Scratch::new()) };
}

#[allow(clippy::too_many_arguments)]
fn search(t: &Tiling, alliances: &Alliances, warmap: &WarMap, side: usize, start: Y16X16, end: Y16X16, flags: u32, wp: &mut Waypoints, scratch: &mut Scratch) -> i32 {
    wp.init();
    let mode = (flags & 0x700) >> 8;
    let catapult = mode == 2;
    let ids = t.ids(catapult);
    let w = side * 16;
    let idx = |p: Y16X16| p.y() as usize * w + p.x() as usize;
    let start_tile = ids[idx(start)];
    let mut end_tile = ids[idx(end)];
    if start_tile == 0 || (end_tile == 0 && flags & 0x40 == 0) {
        return 0; // "Invalid start or end tile."
    }
    if start_tile == end_tile {
        wp.push_back(end);
        return 1;
    }
    let player = (flags & 0xF) as u8;
    let own = if flags & 0x10 != 0 { (0x10u32 << player) ^ 0x1ff0 } else { 0 };
    let mut cost = if flags & 0x20 != 0 { Cost::Alliance } else { Cost::Default };
    let mut rebuild = Rebuild::Default;
    let state_mode = mode;
    let single: [u32; 8];
    let table: &[u32; 8] = match mode {
        0 => {
            single = [own | 0x2c00_0000; 8];
            &single
        }
        1 => {
            single = [own | 0x1c00_0000; 8];
            &single
        }
        2 => {
            single = [own | 0x3400_0000; 8];
            &single
        }
        3 => {
            rebuild = Rebuild::Mode3;
            &TABLE_MODE3
        }
        4 => {
            rebuild = Rebuild::Mode4;
            &TABLE_MODE4
        }
        5 | 6 => {
            cost = if mode == 5 { Cost::Mode5 } else { Cost::Mode6 };
            rebuild = Rebuild::GoalOnly;
            single = [0x2c00_0000; 8];
            &single
        }
        _ => {
            single = [((alliances.ally_bits[(player as usize).min(8)] as u32) << 5) ^ 0x1ff0 | 0x2c00_0000; 8];
            &single
        }
    };
    if rebuild == Rebuild::Default && flags & 0x80 != 0 {
        rebuild = Rebuild::Sparse;
    }
    let state_fn = |cur: u8, n: u16| -> u8 {
        let bit26 = ((t.tiles[n as usize].ty >> 26) & 1) as u8;
        match state_mode {
            3 => match cur {
                0 => 1,
                1 => 1 + bit26,
                2 => 2,
                _ => 7,
            },
            4 => match cur {
                0 => 1,
                1 => 1 + bit26,
                2 => 3,
                3 => 3 + bit26,
                4 => {
                    if t.tiles[end_tile as usize].sector != t.tiles[n as usize].sector { 7 } else { 5 }
                }
                5 => 5,
                _ => 7,
            },
            _ => 0,
        }
    };
    let cx = Ctx { t, alliances, warmap, side, player, end_xy: end };
    if cx.ty(start_tile) & table[0] != 0 {
        return 0; // "Start tile failed type check!"
    }
    let Scratch { closed, seen, heap } = scratch;
    closed.reset(0x8000);
    seen.reset(0x8000);
    heap.clear();
    heap.push((0, 0));
    let mut heap = Heap { e: heap };
    let mut best = if flags & 0x40 != 0 { 0x50 } else { 0 };
    let mut best_tile = 0u16;
    seen.set(start_tile as usize, true);
    let h0 = distance_fast(cx.centre(start_tile), end) as u32;
    heap.push(h0 << 16, start_tile);
    while heap.len() > 0 {
        let (key, cur) = heap.pop();
        let g = (key & 0xFFFF) as i32; // G truncated to 16 bits on pop (0x5ac6a8)
        closed.get_mut(cur as usize).closed = true;
        let cur_state = closed.get(cur as usize).state;
        let mask = table[cur_state as usize];
        for &n in &t.tiles[cur as usize].links {
            if cx.ty(n) & mask != 0 {
                continue;
            }
            let ng = cx.cost(cost, cur, n) + g;
            if n == end_tile {
                closed.set(n as usize, Closed { g: ng, parent: cur, closed: false, state: 0 });
                return do_rebuild(&cx, rebuild, closed, start_tile, &mut end_tile, wp);
            }
            let s = state_fn(cur_state, n);
            if s == 7 {
                continue;
            }
            if seen.get(n as usize) {
                if ng >= closed.get(n as usize).g {
                    continue;
                }
                if !closed.get(n as usize).closed {
                    heap.remove(n);
                }
            }
            let h = distance_fast(end, cx.centre(n));
            if h < best {
                best_tile = n;
            }
            best = best.min(h);
            heap.push(((ng as u32).wrapping_add(h as u32) << 16) | ng as u32 & 0xFFFF, n);
            seen.set(n as usize, true);
            closed.set(n as usize, Closed { g: ng, parent: cur, closed: false, state: s });
        }
    }
    if best_tile != 0 && best_tile != start_tile {
        wp.push_goal(cx.centre(best_tile), 0);
        let mut k = best_tile;
        while k != start_tile {
            wp.push_back(cx.centre(k));
            k = closed.get(k as usize).parent;
        }
        return -1;
    }
    0 // "No path found!"
}

fn do_rebuild(cx: &Ctx, kind: Rebuild, closed: &Stamped<Closed>, start: u16, end_tile: &mut u16, wp: &mut Waypoints) -> i32 {
    let parent = |t: u16| closed.get(t as usize).parent;
    match kind {
        Rebuild::GoalOnly => {
            wp.push_goal(cx.end_xy, closed.get(*end_tile as usize).g);
            2
        }
        Rebuild::Mode3 => {
            // 0x5ac180: back from the end to the first tile with bit 26 (no start check).
            let mut k = *end_tile;
            for _ in 0..0x8000 {
                if cx.ty(k) & 0x0400_0000 != 0 {
                    wp.push_goal(cx.centre(k), 0);
                    return 1;
                }
                k = parent(k);
            }
            0
        }
        Rebuild::Mode4 => {
            // 0x5ac120: move the end back over land tiles, then the default rebuild.
            let mut e = *end_tile;
            let mut p = parent(e);
            while cx.ty(p) & 0x1000_0000 != 0 {
                e = p;
                p = parent(p);
            }
            *end_tile = e;
            let end_xy = cx.centre(e);
            default_rebuild(cx, closed, start, e, end_xy, wp)
        }
        Rebuild::Sparse => {
            // 0x5abf80: a waypoint whenever the summed distance exceeds 40; drop the last one.
            wp.push_goal(cx.end_xy, closed.get(*end_tile as usize).g);
            let (mut prev, mut sum) = (cx.end_xy, 0);
            let mut k = parent(*end_tile);
            let mut limit = 0x8000;
            while k != start {
                limit -= 1;
                if limit == 0 {
                    wp.init();
                    return 0;
                }
                let c = cx.centre(k);
                sum += distance_fast(prev, c);
                prev = c;
                if sum > 40 {
                    wp.push_back(c);
                    sum = 0;
                }
                k = parent(k);
            }
            if wp.count > 2 {
                wp.pop_back();
            }
            1
        }
        Rebuild::Default => default_rebuild(cx, closed, start, *end_tile, cx.end_xy, wp),
    }
}

/// 0x5abeb0: goal, then every tile centre on the parent chain except the start.
fn default_rebuild(cx: &Ctx, closed: &Stamped<Closed>, start: u16, end: u16, end_xy: Y16X16, wp: &mut Waypoints) -> i32 {
    wp.push_goal(end_xy, closed.get(end as usize).g);
    let mut k = closed.get(end as usize).parent;
    for _ in 0..0x8000 {
        if k == start {
            return 1;
        }
        wp.push_back(cx.centre(k));
        k = closed.get(k as usize).parent;
    }
    wp.init();
    0
}
