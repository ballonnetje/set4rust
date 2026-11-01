//! `CAStar64::FindPath(startXY, goalXY, CDirCache&)` (release VA 0x5acd90): node-level search in
//! a 64×64 window, with the variants `CAStar64Normal`, `Worker`, `Ship`, `Catapult`. Read from
//! the shipped exe (helpers: Init 0x5aca30, reconstruct 0x5acbb0, heuristic 0x5acc10,
//! open/closed check 0x5acc60, heap 0x5ad9a0/0x5ada10; docs/SIM.md §11.1).
//!
//! Window cells are packed `y * 128 + x` (`CY6R1X6`); bit 6 guards x overflow, so a cell is valid
//! iff `idx & 0xFFFFE040 == 0`. Only the left, straight-on and right neighbours of the arrival
//! direction are expanded; G = 8 × (move + turn costs) + a step counter in the low 3 bits that
//! saturates at 7; H = 128 per hex step.

use crate::entity::{EntityData, ObjType};
use crate::grid::{Y16X16, flag};
use crate::world::World;
use super::scratch::Stamped;

/// `CDirCache`: the first 16 moves of a path (directions 0..5), filled from the goal backwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DirCache {
    pub count: u8,
    pub head: u8,
    pub ring: [u8; 16],
}

impl Default for DirCache {
    fn default() -> Self {
        DirCache { count: 0, head: 15, ring: [0; 16] }
    }
}

impl DirCache {
    pub fn init(&mut self) {
        self.count = 0;
        self.head = 15;
    }
    fn push_back(&mut self, d: u8) {
        self.count += (self.count < 16) as u8;
        self.head = (self.head + 1) & 15;
        self.ring[self.head as usize] = d;
    }
    /// `CDirCache::Back`: the next move. [inferred: not called in the search code]
    pub fn back(&self) -> u8 {
        self.ring[self.head as usize]
    }
    /// `CDirCache::PopBack`. [inferred]
    pub fn pop_back(&mut self) {
        if self.count > 0 {
            self.count -= 1;
            self.head = self.head.wrapping_sub(1) & 15;
        }
    }
    /// `CDirCache::operator[]` from the next move on. [inferred]
    pub fn get(&self, i: usize) -> u8 {
        self.ring[(self.head as usize + 16 - i) & 15]
    }
}

/// Which `CAStar64` subclass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Variant {
    Normal,
    Worker,
    Ship,
    Catapult,
}

const REL: [i32; 6] = [1, 129, 128, -1, -129, -128];
const TURN: [i32; 6] = [0, 1, 2, 3, 3, 2];

fn valid(idx: i32) -> bool {
    idx as u32 & 0xFFFF_E040 == 0
}

fn hexdist(a: i32, b: i32) -> i32 {
    crate::spiral::grid_distance((b & 63) - (a & 63), (b >> 7) - (a >> 7))
}

struct Search<'a> {
    w: &'a World,
    variant: Variant,
    origin: i64,
    width: i64,
    eco: u16,
}

impl Search<'_> {
    /// `origin + (n >> 7) * width + (n & 63)`: cells past the high map edge wrap (not clamped in
    /// the original); indices beyond the map count as blocked here.
    fn world(&self, n: i32) -> Option<usize> {
        let i = self.origin + (n >> 7) as i64 * self.width + (n & 63) as i64;
        (0..self.w.grid.len() as i64).contains(&i).then_some(i as usize)
    }

    fn not_blocked(&self, i: usize) -> bool {
        let l = &self.w.layers;
        match self.variant {
            Variant::Normal => l.flags[i] & flag::BLOCKED_LAND == 0,
            Variant::Worker => {
                let t = self.w.tiling.normal[i];
                t >= 10 && self.w.tiling.tile(t).eco_sector == self.eco
            }
            Variant::Ship => self.w.tiling.water_flags[i] & 0xC300 == 0xC000,
            // Catapult tile id in 1..0xBFFF (water flags share that buffer in the original).
            Variant::Catapult => self.w.tiling.catapult[i] != 0 && self.w.tiling.water_flags[i] < 0xC000,
        }
    }

    /// `CAStar64::SettlerDisplacementCosts`: `min(256, (1 << displacement) >> (steps >> 1))` for a
    /// settler on the node.
    fn displacement(&self, i: usize, steps: i32) -> i32 {
        let id = self.w.layers.map_object[i];
        match self.w.entities.by_index(id as u32) {
            Some(e) if id != 0 && e.obj_type == ObjType::Settler => {
                let d = match &e.data {
                    EntityData::Settler(s) => s.job.displacement_costs as u32,
                    _ => 0,
                };
                ((1i32 << d.min(30)) >> (steps >> 1)).min(256)
            }
            _ => 0,
        }
    }

    fn move_cost(&self, i: usize, steps: i32) -> i32 {
        let b = self.w.layers.walk_move[i] as i32;
        match self.variant {
            Variant::Normal | Variant::Worker => {
                let bits = if self.variant == Variant::Normal { (b & 5) | 2 } else { b & 7 };
                let base = bits * 4 + 8;
                if steps < 7 { base + self.displacement(i, steps) } else { base }
            }
            Variant::Ship => {
                let wf = self.w.tiling.water_flags[i] as i32;
                ((4 << (wf & 0xF)) >> (steps >= 7) as i32) + 16
            }
            Variant::Catapult => {
                if steps >= 7 {
                    return 16;
                }
                let (x, y) = (self.w.grid.x(i) as i32, self.w.grid.y(i) as i32);
                let mut c = 0;
                for (dx, dy) in [(0, 0)].into_iter().chain(crate::tiling::NEIGHBORS) {
                    if let Some(j) = self.w.grid.checked_index(x + dx, y + dy) {
                        let v = self.w.layers.map_object[j];
                        c += (1..=7).contains(&v) as i32;
                    }
                }
                if c == 0 {
                    return 16;
                }
                let v = if c < 3 { 8 * c + 24 } else { 48 };
                if steps < 5 { v + 16 } else { v }
            }
        }
    }
}

/// Open list: entries `(A, B)`, `A = ((G + H) << 8) | (idx >> 8)`, `B = ((idx & 0xff) << 24) | G`;
/// ordered by `A` only (unsigned). 1-based binary heap, slot 0 a key-0 sentinel.
///
/// [ours] `pos` remembers each cell's slot, so `remove` need not search the heap (the original
/// searches from the back; a cell is in the heap at most once, so it finds the same slot). A slot
/// that does not hold the cell (stale from an earlier search) falls back to that search.
struct Heap<'a> {
    e: &'a mut Vec<(u32, u32)>,
    pos: &'a mut [u16],
}

fn cell(e: (u32, u32)) -> usize {
    (((e.0 & 0xff) << 8) | (e.1 >> 24)) as usize
}

impl Heap<'_> {
    #[inline]
    fn put(&mut self, p: usize, x: (u32, u32)) {
        self.e[p] = x;
        self.pos[cell(x)] = p as u16;
    }
    fn push(&mut self, idx: i32, g: i32, h: i32) {
        let a = ((g.wrapping_add(h) as u32) << 8) | (idx as u32 >> 8);
        let b = ((idx as u32 & 0xff) << 24) | g as u32;
        self.e.push((a, b));
        let mut p = self.e.len() - 1;
        while self.e[p >> 1].0 > a {
            let up = self.e[p >> 1];
            self.put(p, up);
            p >>= 1;
        }
        self.put(p, (a, b));
    }
    fn sift_down(&mut self, mut p: usize) {
        let x = self.e[p];
        let n = self.e.len() - 1;
        while p <= n >> 1 {
            let mut c = 2 * p;
            if c < n && self.e[c].0 > self.e[c + 1].0 {
                c += 1;
            }
            if x.0 <= self.e[c].0 {
                break;
            }
            let down = self.e[c];
            self.put(p, down);
            p = c;
        }
        self.put(p, x);
    }
    fn pop(&mut self) -> Option<(i32, i32)> {
        if self.e.len() <= 1 {
            return None;
        }
        let top = self.e[1];
        let last = self.e.pop().expect("non-empty");
        if self.e.len() > 1 {
            self.put(1, last);
            self.sift_down(1);
        }
        Some((cell(top) as i32, (top.1 & 0xFF_FFFF) as i32))
    }
    fn remove(&mut self, idx: i32) {
        let n = self.e.len() - 1;
        let p = self.pos[idx as usize] as usize;
        let slot = if (1..=n).contains(&p) && cell(self.e[p]) == idx as usize { Some(p) } else { (1..=n).rev().find(|&j| cell(self.e[j]) == idx as usize) };
        if let Some(j) = slot {
            let last = self.e.pop().expect("non-empty");
            if j < self.e.len() {
                self.put(j, last);
                self.sift_down(j);
            }
        }
    }
}

/// `CAStar64::FindPath` with the given variant. Fills `dirs` with the first moves; returns
/// whether a path was found.
pub fn astar64(w: &World, variant: Variant, start: Y16X16, goal: Y16X16, dirs: &mut DirCache) -> bool {
    let found = SCRATCH.with(|s| match s.try_borrow_mut() {
        Ok(mut s) => search(w, variant, start, goal, dirs, &mut s),
        Err(_) => search(w, variant, start, goal, dirs, &mut Scratch::new()),
    });
    crate::debug::fine_path(found, dirs.count);
    found
}

/// The search's tables, kept between searches ([`super::scratch`]).
struct Scratch {
    seen: Stamped<bool>,
    closed: Stamped<u32>,
    heap: Vec<(u32, u32)>,
    pos: Vec<u16>,
}

impl Scratch {
    const fn new() -> Self {
        Scratch { seen: Stamped::new(), closed: Stamped::new(), heap: Vec::new(), pos: Vec::new() }
    }
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = const { std::cell::RefCell::new(Scratch::new()) };
}

fn search(w: &World, variant: Variant, start: Y16X16, goal: Y16X16, dirs: &mut DirCache, scratch: &mut Scratch) -> bool {
    dirs.init();
    // Init (0x5aca30): a window centred on the midpoint, clamped on the low side only.
    let (sx, sy, gx, gy) = (start.x(), start.y(), goal.x(), goal.y());
    let (dx, dy) = ((gx - sx).abs(), (gy - sy).abs());
    if (dx | dy) >= 64 {
        return false;
    }
    let ox = (sx.min(gx) + (dx >> 1) - 32).max(0);
    let oy = (sy.min(gy) + (dy >> 1) - 32).max(0);
    let width = w.grid.size() as i64;
    let origin = oy as i64 * width + ox as i64;
    let s_idx = (sy - oy) * 128 + (sx - ox);
    let g_idx = (gy - oy) * 128 + (gx - ox);
    if !valid(s_idx) {
        return false;
    }
    let Scratch { seen, closed, heap, pos } = scratch;
    seen.reset(8192);
    closed.reset(8192);
    heap.clear();
    heap.push((0, 0));
    pos.resize(8192, 0);
    let mut heap = Heap { e: heap, pos };
    closed.set(s_idx as usize, 8);
    seen.set(s_idx as usize, true);
    heap.push(s_idx, 0, hexdist(s_idx, g_idx) << 7);
    let mut cx = Search { w, variant, origin, width, eco: 0 };
    // InitFindPath.
    if variant == Variant::Worker {
        let si = w.grid.index(sx as u32, sy as u32);
        let t = w.tiling.normal[si];
        cx.eco = w.tiling.tile(t).eco_sector;
        if cx.eco == 0 {
            return false;
        }
    }
    let rebuild = |closed: &Stamped<u32>, dirs: &mut DirCache| {
        let mut i = g_idx;
        let mut d = closed.get(i as usize) & 0x7f;
        while d < 6 {
            dirs.push_back(d as u8);
            i -= REL[d as usize];
            d = closed.get(i as usize) & 0x7f;
        }
    };
    // First expansion: the start node, all six neighbours, no open/closed check.
    let (idx, g) = heap.pop().expect("start");
    for (i, &r) in REL.iter().enumerate() {
        let n = idx + r;
        if !valid(n) {
            continue;
        }
        if n == g_idx {
            closed.set(n as usize, (g as u32) << 8 | i as u32);
            rebuild(closed, dirs);
            return true;
        }
        let Some(wi) = cx.world(n) else { continue };
        if !cx.not_blocked(wi) {
            continue;
        }
        let ng = g + 1 + 8 * cx.move_cost(wi, 0);
        seen.set(n as usize, true);
        closed.set(n as usize, (ng as u32) << 8 | i as u32);
        heap.push(n, ng, hexdist(n, g_idx) << 7);
    }
    while let Some((idx, g)) = heap.pop() {
        *closed.get_mut(idx as usize) |= 0x80;
        let pd = (closed.get(idx as usize) & 0x7f) as i32;
        let mut dir = if pd & 7 == 0 { 5 } else { (pd & 7) - 1 };
        for _ in 0..3 {
            let n = idx + REL[dir as usize];
            if valid(n) {
                if n == g_idx {
                    closed.set(n as usize, (g as u32) << 8 | dir as u32);
                    rebuild(closed, dirs);
                    return true;
                }
                if let Some(wi) = cx.world(n)
                    && cx.not_blocked(wi)
                {
                    let mut t = dir - pd;
                    if t < 0 {
                        t += 6;
                    }
                    let s = g & 7;
                    let ng = g + (s != 7) as i32 + 8 * (cx.move_cost(wi, s) + TURN[t as usize]);
                    // check (0x5acc60).
                    let ok = if !seen.get(n as usize) {
                        true
                    } else if ng as u32 >= closed.get(n as usize) >> 8 {
                        false
                    } else {
                        if closed.get(n as usize) & 0x80 == 0 {
                            heap.remove(n);
                        }
                        true
                    };
                    if ok {
                        seen.set(n as usize, true);
                        closed.set(n as usize, (ng as u32) << 8 | dir as u32);
                        heap.push(n, ng, hexdist(n, g_idx) << 7);
                    }
                }
            }
            dir = if dir == 5 { 0 } else { dir + 1 };
        }
    }
    false
}
