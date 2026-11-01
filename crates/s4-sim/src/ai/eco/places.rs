//! The manager's place search (docs/ECOAI_MANAGER.md, "Helpers"): per building type a walk over
//! the eco-sector's 16×16 cells (0x44cd90 / 0x44ce20), the cells a type may stand in (0x451510),
//! the nodes of each where it can be built (0x450fa0) and a quadtree of those places (cells of
//! 16, 32 and 64 nodes; 0x456030..0x456ed0).

use super::manager::{Manager, World2};
use crate::World;

/// Building types the manager keeps (0..0x53).
pub const TYPES: usize = 0x53;

/// A place (the first 0x48 bytes of a quadtree node, the node list deep-copied; 0x44e6f0).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Place {
    /// +0 / +4: the cell in units of the level (64, 32 or 16 nodes).
    pub x: i32,
    pub y: i32,
    /// +8 the building type.
    pub t: i32,
    /// +0xc the level: 0x40, 0x20 or 0x10.
    pub level: i32,
    /// +0x10..+0x28: the cell's ground-class counts (leaves only).
    pub classes: [i32; 7],
    /// +0x2c..+0x3a: the cell's resource sums (leaves); a parent has its children's first sum in
    /// every slot (0x456201).
    pub res: [u16; 8],
    /// +0x3c: the nodes where the type can be built, with the `CheckForBuild` result (leaves).
    pub nodes: Vec<(u32, u8)>,
}

/// A quadtree node (100 bytes, 0x456030).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Node {
    pub place: Place,
    /// +0x48: up to four children (slots may be empty).
    pub children: [Option<Box<Node>>; 4],
    /// +0x58: 0 at a leaf, the child count at 0x20, the children's sum at 0x40.
    pub count: i32,
}

impl Node {
    fn new(level: i32, t: i32, x: i32, y: i32) -> Node {
        Node { place: Place { x, y, t, level, ..Place::default() }, ..Node::default() }
    }

    /// 0x4565b0: the child at exactly `(x, y)`.
    fn child_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..4).find(|&k| self.children[k].as_ref().is_some_and(|c| c.place.x == x && c.place.y == y))
    }

    /// 0x456160: the children first, then this node's sums.
    fn refresh(&mut self, world: &World) {
        for c in self.children.iter_mut().flatten() {
            c.refresh(world);
        }
        self.count = 0;
        let level = self.place.level;
        if level == 0x10 {
            let c = world.animals.cells.get(self.place.x, self.place.y);
            self.place.res = c.resources;
            for k in 0..7 {
                self.place.classes[k] = c.classes[k] as i32;
            }
        } else if level == 0x20 || level == 0x40 {
            self.place.res = [0; 8];
            if level == 0x20 {
                self.count = self.children.iter().filter(|c| c.is_some()).count() as i32;
            }
            for c in self.children.iter().flatten() {
                let first = c.place.res[0];
                for r in &mut self.place.res {
                    *r = r.wrapping_add(first);
                }
                if level == 0x40 {
                    self.count += c.count;
                }
            }
        }
    }

    /// 0x456340 `(cx, cy, nodes)`: into the leaf of cell `(cx, cy)`, made on the way. A node
    /// matches if `cx >> s == x` *or* `cy >> s == y` (s = 2 at 0x40, 1 at 0x20).
    fn insert(&mut self, world: &World, cx: i32, cy: i32, nodes: &[(u32, u8)]) -> bool {
        let level = self.place.level;
        if level == 0x10 {
            self.place.nodes.extend_from_slice(nodes);
            return false;
        }
        let (mx, my) = match level {
            0x20 => (cx >> 1, cy >> 1),
            0x40 => (cx >> 2, cy >> 2),
            _ => (cy, cy),
        };
        if mx != self.place.x && my != self.place.y {
            return false;
        }
        let (kx, ky) = match level {
            0x20 => (cx, cy),
            0x40 => (cx >> 1, cy >> 1),
            _ => (cy, cy),
        };
        if let Some(k) = self.child_at(kx, ky) {
            let r = self.children[k].as_mut().expect("child").insert(world, cx, cy, nodes);
            self.refresh(world);
            return r;
        }
        let Some(k) = (0..4).find(|&k| self.children[k].is_none()) else { return false };
        let mut c = Box::new(Node::new(level >> 1, self.place.t, kx, ky));
        c.insert(world, cx, cy, nodes);
        self.children[k] = Some(c);
        self.refresh(world);
        true
    }

    /// 0x456b90: `target` (a node of the tree) removed below this node.
    fn remove_below(&mut self, target: &Place) -> bool {
        for k in 0..4 {
            if self.children[k].as_ref().is_some_and(|c| same(&c.place, target)) {
                self.children[k] = None;
                return true;
            }
        }
        self.children.iter_mut().flatten().any(|c| c.remove_below(target))
    }
}

/// The release compares node pointers; within one type's tree a node is identified by its level
/// and cell.
fn same(a: &Place, b: &Place) -> bool {
    a.level == b.level && a.x == b.x && a.y == b.y
}

/// One record of the accepted cells (0x1c bytes: the cell `y << 8 | x` and two vectors the
/// release never fills [inf]).
pub type CellRec = u16;

/// The cell walk (0xa10 bytes; init 0x44cd90, next 0x44ce20): a breadth-first walk over the
/// squares that hold tiles of the player, from the first square holding a tile of the
/// eco-sector.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Bfs {
    /// +4 the start cell `y << 8 | x` (kept when no start is found).
    start: u16,
    /// +8 the visited bitmap (byte `y·8 + x>>3`, bit `x & 7`).
    visited: Vec<u8>,
    /// +0x208 / +0x20c / +0x210: a ring of 0x400 cells.
    read: usize,
    write: usize,
    ring: Vec<u16>,
}

impl Default for Bfs {
    fn default() -> Self {
        Bfs { start: 0, visited: vec![0; 0x200], read: 0, write: 0, ring: vec![0; 0x400] }
    }
}

/// What the cell walk reads.
pub struct BfsWorld<'a> {
    pub world: &'a World,
    pub eco: u16,
    pub player: u8,
    pub side: i32,
}

impl BfsWorld<'_> {
    fn owner_bit(&self, cx: i32, cy: i32) -> bool {
        let owner = self.world.economy.owner(self.eco);
        let bit = ((1u32 << owner) >> 1) as u8;
        self.world.tiling.owner_bits(cx, cy)[0] & bit != 0
    }

    /// 0x44d190: the square's chain has a tile of the eco-sector before a catapult tile.
    fn chain_has_eco(&self, cx: i32, cy: i32) -> bool {
        let mut t = self.world.tiling.first_tile_of_square(cx as usize, cy as usize, self.side as usize);
        while t != 0 {
            let tile = self.world.tiling.tile(t);
            if tile.catapult() {
                return false;
            }
            if tile.eco_sector == self.eco {
                return true;
            }
            t = tile.sq_next;
        }
        false
    }
}

impl Bfs {
    /// 0x44cd90 (with 0x44d070): the start is the first square, x outer and y inner, with the
    /// player's bit and a tile of the eco-sector; it is queued, *then* the visited bitmap is
    /// cleared (the start is never marked).
    pub fn init(&mut self, m: &World2<'_>) {
        let w = m.bfs_parts();
        if w.world.economy.get(w.eco).is_some() {
            'found: for x in 0..w.side {
                for y in 0..w.side {
                    if w.owner_bit(x, y) && w.chain_has_eco(x, y) {
                        self.start = ((y << 8) | x) as u16;
                        break 'found;
                    }
                }
            }
        }
        self.read = 0;
        self.write = 0;
        self.ring[self.write] = self.start;
        self.write = (self.write + 1) & 0x3ff;
        self.visited.iter_mut().for_each(|b| *b = 0);
    }

    fn seen(&self, x: i32, y: i32) -> bool {
        self.visited[(y * 8 + (x >> 3)) as usize] & (1 << (x & 7)) != 0
    }

    fn mark(&mut self, x: i32, y: i32) {
        self.visited[(y * 8 + (x >> 3)) as usize] |= 1 << (x & 7);
    }

    /// Whether cells are left (`read != write`).
    pub fn pending(&self) -> bool {
        self.read != self.write
    }

    /// 0x44ce20: the next cell `(x, y)`; its neighbours (x+1, x−1, y+1, y−1) in range are marked
    /// and, with the player's bit and not seen before, queued.
    pub fn next(&mut self, m: &World2<'_>) -> Option<(i32, i32)> {
        let w = m.bfs_parts();
        w.world.economy.get(w.eco)?;
        if self.read == self.write {
            return None;
        }
        let c = self.ring[self.read];
        self.read = (self.read + 1) & 0x3ff;
        let (x, y) = ((c & 0xff) as i32, (c >> 8) as i32);
        for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if nx < 0 || ny < 0 || nx >= w.side || ny >= w.side {
                continue;
            }
            if w.owner_bit(nx, ny) && !self.seen(nx, ny) {
                self.ring[self.write] = ((ny << 8) | nx) as u16;
                self.write = (self.write + 1) & 0x3ff;
            }
            self.mark(nx, ny);
        }
        Some((x, y))
    }
}

/// One type's place search.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scan {
    /// +0x10 + 4t: the top nodes (`None`: no list allocated).
    pub tops: Option<Vec<Node>>,
    /// +0x15c / +0x1af / +0x202 / +0x255 / +0x2a8.
    pub active: bool,
    pub walked: bool,
    pub cell_set: bool,
    pub cell_done: bool,
    pub walk_started: bool,
    pub bfs: Bfs,
    /// +0x3462c: the accepted cells.
    pub cells: Vec<CellRec>,
    /// +0x34a10: the current cell's nodes.
    pub nodes: Vec<(u32, u8)>,
    /// +0x34df4: the queued cells.
    pub queued: Vec<u8>,
    /// +0x3f3f4 cursor, +0x3f540 nodes per step, +0x3f68c steps, +0x3f7d8 step, +0x3f924 cells
    /// inserted, +0x3fa70 cells processed.
    pub cursor: i32,
    pub per_step: i32,
    pub steps: i32,
    pub step: i32,
    pub inserted: i32,
    pub processed: i32,
}

impl Default for Scan {
    fn default() -> Self {
        Scan {
            tops: None,
            active: false,
            walked: false,
            cell_set: false,
            cell_done: false,
            walk_started: false,
            bfs: Bfs::default(),
            cells: Vec::new(),
            nodes: Vec::new(),
            queued: vec![0; 0x200],
            cursor: 0,
            per_step: 0,
            steps: 0,
            step: 0,
            inserted: 0,
            processed: 0,
        }
    }
}

impl Scan {
    fn is_queued(&self, x: i32, y: i32) -> bool {
        let i = (y * 8 + (x >> 3)) as usize;
        i < 0x200 && self.queued[i] & (1 << (x & 7)) != 0
    }

    fn set_queued(&mut self, x: i32, y: i32) {
        let i = (y * 8 + (x >> 3)) as usize;
        if i < 0x200 {
            self.queued[i] |= 1 << (x & 7);
        }
    }
}

/// All types' searches (allocated on first use; a fresh one equals the release's zeroed memory).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Scans {
    pub by_type: Vec<Option<Box<Scan>>>,
}

impl Scans {
    fn get(&self, t: i32) -> Option<&Scan> {
        self.by_type.get(usize::try_from(t).ok()?)?.as_deref()
    }

    fn get_mut(&mut self, t: i32) -> &mut Scan {
        let t = t as usize;
        if self.by_type.len() < TYPES {
            self.by_type.resize_with(TYPES, || None);
        }
        self.by_type[t].get_or_insert_with(Default::default)
    }

    fn tops(&self, t: i32) -> Option<&Vec<Node>> {
        self.get(t)?.tops.as_ref()
    }

    /// Slot 0x10 (0x44df40): bitmap, flags and counters zero, the accepted cells gone (the
    /// node list is kept); the tree freed — true if there was one.
    pub fn clear(&mut self, t: i32) -> bool {
        let s = self.get_mut(t);
        s.queued.iter_mut().for_each(|b| *b = 0);
        s.active = false;
        s.walked = false;
        s.cell_set = false;
        s.cell_done = false;
        s.walk_started = false;
        s.cursor = 0;
        s.per_step = 0;
        s.steps = 0;
        s.step = 0;
        s.inserted = 0;
        s.processed = 0;
        s.cells.clear();
        s.tops.take().is_some()
    }

    /// 0x456a00 `(level, x, y)` on type `t`'s tree.
    fn node(&self, t: i32, level: i32, x: i32, y: i32) -> Option<&Node> {
        node_in(self.tops(t)?, level, x, y)
    }

    /// Slot 0x1c (0x44e180).
    pub fn places_around(&self, t: i32, c: &Place, out: &mut Vec<Place>, r: i32, side: i32) -> i32 {
        if t == 0 || self.tops(t).is_none() {
            return 0;
        }
        for dx in -r..=r {
            for dy in -r..=r {
                if dx == 0 && dy == 0 && c.t != 0 {
                    continue;
                }
                let (x, y) = (c.x + dx, c.y + dy);
                if x < 0 || x > side || y < 0 || y > side {
                    continue;
                }
                if let Some(n) = self.node(t, c.level, x, y) {
                    out.push(n.place.clone());
                }
            }
        }
        out.len() as i32
    }

    /// Slot 0x20 (0x44e070): every top (0x40), child (0x20) or grandchild (other levels).
    pub fn places_of(&self, level: i32, t: i32, out: &mut Vec<Place>) -> i32 {
        if t == 0 {
            return 0;
        }
        let Some(tops) = self.tops(t) else { return 0 };
        for top in tops {
            if level == 0x40 {
                out.push(top.place.clone());
                continue;
            }
            for c in top.children.iter().flatten() {
                if level == 0x20 {
                    out.push(c.place.clone());
                } else {
                    for g in c.children.iter().flatten() {
                        out.push(g.place.clone());
                    }
                }
            }
        }
        out.len() as i32
    }

    /// Slot 0x108 (0x450920): a coarser level gives the parent cell's node in `out[0]` (and 0);
    /// a finer one the children (0x40 → 0x10: the grandchildren), with `level` set, and their
    /// count; the same level gives 0.
    pub fn place_family(&self, t: i32, level: i32, p: &Place, out: &mut Vec<Place>) -> i32 {
        if self.tops(t).is_none() || p.level == level {
            return 0;
        }
        if p.level < level {
            if let Some(n) = self.node(t, level, p.x >> 1, p.y >> 1) {
                let mut q = n.place.clone();
                q.level = level;
                out.push(q);
            }
            return 0;
        }
        let Some(n) = self.node(t, p.level, p.x, p.y) else { return 0 };
        let mut count = 0;
        if p.level == 0x40 && level == 0x10 {
            for c in n.children.iter().flatten() {
                for g in c.children.iter().flatten() {
                    let mut q = g.place.clone();
                    q.level = 0x10;
                    out.push(q);
                    count += 1;
                }
            }
        } else {
            for c in n.children.iter().flatten() {
                let mut q = c.place.clone();
                q.level = level;
                out.push(q);
                count += 1;
            }
        }
        count
    }

    /// Slot 0x14 (0x44e050 → 0x4568e0): `p`'s node removed (a top swap-removed, else from its
    /// parent).
    pub fn remove_place(&mut self, t: i32, p: &Place) -> bool {
        let Some(tops) = self.get_mut(t).tops.as_mut() else { return true };
        let Some(target) = node_in(tops, p.level, p.x, p.y).map(|n| n.place.clone()) else { return false };
        for k in 0..tops.len() {
            if same(&tops[k].place, &target) {
                tops.swap_remove(k);
                return true;
            }
            if tops[k].remove_below(&target) {
                return true;
            }
        }
        false
    }
}

/// 0x456a00 `(level, x, y)`: the top at `(x, y)` halved per level, then the child and grandchild
/// by exact coordinates.
fn node_in(tops: &[Node], level: i32, x: i32, y: i32) -> Option<&Node> {
    let (mut tx, mut ty) = (x, y);
    if level < 0x40 {
        tx >>= 1;
        ty >>= 1;
    }
    if level == 0x10 {
        tx >>= 1;
        ty >>= 1;
    }
    let top = tops.iter().find(|n| n.place.x == tx && n.place.y == ty)?;
    if level == 0x40 {
        return Some(top);
    }
    let (cx, cy) = if level == 0x10 { (x >> 1, y >> 1) } else { (x, y) };
    let child = top.children[top.child_at(cx, cy)?].as_deref()?;
    if level == 0x20 {
        return Some(child);
    }
    child.children[child.child_at(x, y)?].as_deref()
}

/// 0x456750 `(t, cx, cy, nodes)`: the top `(cx >> 2, cy >> 2)` found or appended, then the
/// insert.
fn add_cell(world: &World, tops: &mut Vec<Node>, t: i32, cx: i32, cy: i32, nodes: &[(u32, u8)]) {
    let k = match tops.iter().position(|n| n.place.x == cx >> 2 && n.place.y == cy >> 2) {
        Some(k) => k,
        None => {
            tops.push(Node::new(0x40, t, cx >> 2, cy >> 2));
            tops.len() - 1
        }
    };
    tops[k].insert(world, cx, cy, nodes);
}

/// The race's land check of 0x451510 (skipped for types 0xe..0x12): base class 0 for the Dark
/// Tribe, 1 otherwise, plus 3 (Vikings), 2 (Maya), 3 and 2 (Trojans).
fn race_land(m: &mut World2<'_>, cx: i32, cy: i32) -> bool {
    let base = if m.race() == 3 { 0 } else { 1 };
    let n = match m.race() {
        2 => m.cell_class(2, cx, cy) + m.cell_class(base, cx, cy),
        1 => m.cell_class(3, cx, cy) + m.cell_class(base, cx, cy),
        4 => m.cell_class(base, cx, cy) + m.cell_class(3, cx, cy) + m.cell_class(2, cx, cy),
        _ => m.cell_class(base, cx, cy),
    };
    n > 0
}

/// 0x451b00 `(cx, cy)`: a tile of the eco-sector in the square links to a walkable-water tile.
fn water_link(m: &World2<'_>, cx: i32, cy: i32) -> bool {
    let w = m.bfs_parts();
    let mut t = w.world.tiling.first_tile_of_square(cx as usize, cy as usize, w.side as usize);
    while t != 0 {
        let tile = w.world.tiling.tile(t);
        if tile.eco_sector == w.eco && tile.links.iter().any(|&l| w.world.tiling.tile(l).ty & 0x400_0000 != 0) {
            return true;
        }
        t = tile.sq_next;
    }
    false
}

/// 0x451510 `(t, cx, cy)`: whether type `t` may stand in the cell.
fn cell_ok(m: &mut World2<'_>, t: i32, cx: i32, cy: i32) -> bool {
    if !m.cell_bit(0xd4, cx, cy) {
        return false;
    }
    if !(0xe..=0x12).contains(&t) && !race_land(m, cx, cy) {
        return false;
    }
    let side = m.bfs_parts().side;
    match t {
        1 | 7 => m.resource_near(0x70, cx, cy, 2) || m.cell_resource(0x70, cx, cy) != 0,
        4 => m.resource_near(0x60, cx, cy, 2) || m.cell_resource(0x60, cx, cy) != 0,
        5 => m.cell_bit(0xe8, cx, cy),
        6 => {
            if !m.resource_near(0, cx, cy, 2) {
                return false;
            }
            if water_link(m, cx, cy) {
                return true;
            }
            for dx in -2..=2 {
                for dy in -2..=2 {
                    if dy * dy + dx * dx >= 5 || (dx == 0 && dy == 0) {
                        continue;
                    }
                    let (x, y) = (cx + dx, cy + dy);
                    if x < 0 || x > side || y < 0 || y > side {
                        continue;
                    }
                    if m.cell_bit(0xd4, x, y) && m.cell_bit(0xdc, x, y) && water_link(m, x, y) {
                        return true;
                    }
                }
            }
            false
        }
        0xe..=0x12 => {
            let res = match t {
                0xe => 0x50,
                0xf => 0x20,
                0x10 => 0x30,
                0x11 => 0x10,
                _ => 0x40,
            };
            m.cell_resource(res, cx, cy) != 0
        }
        0x1f | 0x20 => m.cell_bit(0xe0, cx, cy),
        0x23 => m.class_near(6, cx, cy, 1),
        0x24 => m.class_near(2, cx, cy, 1) || m.cell_class(2, cx, cy) != 0,
        0x52 => m.class_near(1, cx, cy, 1) || m.cell_class(1, cx, cy) != 0,
        _ => true,
    }
}

/// Slot 0x0c (0x44dbb0).
pub fn scan(m: &mut World2<'_>, t: i32) -> i32 {
    if !m.state.scans.get_mut(t).active {
        m.state.scans.clear(t);
        let s = m.state.scans.get_mut(t);
        s.active = true;
        s.walk_started = false;
        s.cell_set = false;
        s.walked = false;
        s.cursor = 0;
    }
    match step(m, t) {
        2 => 2,
        1 => {
            m.state.scans.clear(t);
            1
        }
        _ => {
            if m.state.scans.tops(t).is_some_and(|v| !v.is_empty()) {
                m.state.scans.get_mut(t).active = false;
                0
            } else {
                m.state.scans.clear(t);
                1
            }
        }
    }
}

/// Slot 0x08 (0x44dc60) `(t, cx, cy)`: the 3×3 cells around into the accepted cells (no walk:
/// a fresh search starts with the walk marked done). Always 2.
pub fn rescan(m: &mut World2<'_>, t: i32, cx: i32, cy: i32) -> i32 {
    if !(0..0x40).contains(&cx) || !(0..0x40).contains(&cy) {
        return 2;
    }
    if !m.state.scans.get_mut(t).active {
        m.state.scans.clear(t);
        let s = m.state.scans.get_mut(t);
        s.active = true;
        s.walk_started = false;
        s.cell_set = false;
        s.walked = true;
        s.cursor = 0;
    }
    for y in cy - 1..=cy + 1 {
        for x in cx - 1..=cx + 1 {
            if x < 0 || y < 0 || y * 8 + (x >> 3) >= 0x200 {
                continue;
            }
            if m.state.scans.get_mut(t).is_queued(x, y) {
                continue;
            }
            let w = m.bfs_parts();
            if x >= w.side || y >= w.side || !w.chain_has_eco(x, y) {
                continue;
            }
            if cell_ok(m, t, x, y) {
                let s = m.state.scans.get_mut(t);
                s.cells.push(((y << 8) | x) as u16);
                s.set_queued(x, y);
            }
        }
    }
    2
}

/// 0x450fa0 `(t)`: one step of type `t`'s search; 0 done, 1 no eco-sector, 2 working.
fn step(m: &mut World2<'_>, t: i32) -> i32 {
    if m.world.economy.get(m.state.eco).is_none() {
        return 1;
    }
    {
        let s = m.state.scans.get_mut(t);
        if s.tops.is_none() {
            s.tops = Some(Vec::new());
        }
    }
    let player = m.player();
    if !m.state.scans.get_mut(t).walked {
        if !m.state.scans.get_mut(t).walk_started {
            let mut bfs = std::mem::take(&mut m.state.scans.get_mut(t).bfs);
            bfs.init(m);
            let s = m.state.scans.get_mut(t);
            s.bfs = bfs;
            s.walk_started = true;
            s.cell_set = false;
            s.inserted = 0;
            s.processed = 0;
            s.cursor = 0;
            return 2;
        }
        let mut counted = 0;
        loop {
            if !m.state.scans.get_mut(t).bfs.pending() {
                break;
            }
            let mut bfs = std::mem::take(&mut m.state.scans.get_mut(t).bfs);
            let c = bfs.next(m);
            m.state.scans.get_mut(t).bfs = bfs;
            let Some((x, y)) = c else { continue };
            if y * 8 + (x >> 3) >= 0x200 {
                continue;
            }
            let queued = m.state.scans.get_mut(t).is_queued(x, y);
            if queued || !m.bfs_parts().chain_has_eco(x, y) {
                continue;
            }
            if cell_ok(m, t, x, y) {
                let s = m.state.scans.get_mut(t);
                s.cells.push(((y << 8) | x) as u16);
                s.set_queued(x, y);
            }
            counted += 1;
            if counted == 2 {
                return 2;
            }
        }
        m.state.scans.get_mut(t).walked = true;
        return 2;
    }
    let (cursor, n) = {
        let s = m.state.scans.get_mut(t);
        (s.cursor, s.cells.len() as i32)
    };
    if cursor >= n {
        return 0;
    }
    let rec = m.state.scans.get_mut(t).cells[cursor as usize] as i32;
    let (x, y) = (rec & 0xff, rec >> 8);
    if !m.state.scans.get_mut(t).cell_set {
        {
            let s = m.state.scans.get_mut(t);
            s.nodes.clear();
            s.cell_set = true;
            s.cell_done = false;
        }
        let mut cls = if m.race() == 3 { 0 } else { 1 };
        if (0xe..=0x12).contains(&t) {
            cls = 4;
        }
        let n = m.cell_class(cls, x, y);
        let race = m.race();
        let c = if race < 5 { m.world.rules.building(race as u8, t as u8).building_cells.len() as i32 } else { 0 };
        // (float)(n / c) >= 1.0f (0x1070de4); c is never 0 for a building type [inf: the
        // release would fault].
        let q = if c == 0 { 0 } else { n / c };
        let s = m.state.scans.get_mut(t);
        if q >= 1 {
            s.steps = q;
            s.per_step = 0x100 / q;
        } else {
            s.per_step = 0x100;
            s.steps = 1;
        }
        s.step = 0;
    }
    if !m.state.scans.get_mut(t).cell_done {
        let (step, per) = {
            let s = m.state.scans.get_mut(t);
            (s.step, s.per_step)
        };
        for k in step * per..step * per + per {
            let nx = (k % 16) + (x << 4);
            let ny = (k >> 4) + (y << 4);
            let node = (ny << 16) + nx;
            // The release calls slot 0xec as (nx, ny, 0x70) / (nx, ny, 0x60): resource nx at
            // node (ny, 0x70) / (ny, 0x60). Kept.
            if m.resource_at(nx, ny, 0x70) == 0 && m.resource_at(nx, ny, 0x60) == 0 {
                let b = m.overlapping_building(t, node);
                if b == 0 {
                    let r = crate::build::check::check_for_build(m.world, nx, ny, player as u8, t as u8).code();
                    if r > 0 {
                        m.state.scans.get_mut(t).nodes.push((node as u32, r as u8));
                    }
                }
            }
        }
        let s = m.state.scans.get_mut(t);
        s.step += 1;
        if s.step >= s.steps {
            s.cell_done = true;
        }
        return 2;
    }
    let nodes = std::mem::take(&mut m.state.scans.get_mut(t).nodes);
    if !nodes.is_empty() {
        let mut tops = m.state.scans.get_mut(t).tops.take().unwrap_or_default();
        add_cell(m.world, &mut tops, t, x, y, &nodes);
        let s = m.state.scans.get_mut(t);
        s.tops = Some(tops);
        s.inserted += 1;
    }
    let s = m.state.scans.get_mut(t);
    s.nodes = nodes;
    s.cell_set = false;
    s.processed += 1;
    s.cursor += 1;
    2
}

/// Slot 0x18 (0x44e270) `(level, t, b1, b2, b3)`.
pub fn place_near(m: &mut World2<'_>, level: i32, t: i32, b: [i32; 3]) -> Place {
    if b.iter().all(|&v| v == -1) || m.state.scans.tops(t).is_none() {
        return Place::default();
    }
    let n = b.iter().filter(|&&v| v != -1).count() as i32;
    let (mut sx, mut sy) = (0, 0);
    for &id in &b {
        if id == -1 {
            continue;
        }
        let Some((x, y)) = m.building_pos(id) else { return Place::default() };
        sx += x >> 4;
        sy += y >> 4;
    }
    let (cx, cy) = (sx / n, sy / n);
    let scans = &m.state.scans;
    let found = scans.node(t, 0x10, cx, cy).or_else(|| {
        for x in cx - 1..cx + 1 {
            for y in cy - 1..cy + 1 {
                if let Some(n) = scans.node(t, 0x10, x, y) {
                    return Some(n);
                }
            }
        }
        None
    });
    let Some(node) = found else { return Place::default() };
    let mut p = node.place.clone();
    p.level = 0x10;
    if level != 0x10 {
        let mut out = Vec::new();
        m.state.scans.place_family(t, level, &p, &mut out);
        return out.into_iter().next().unwrap_or_default();
    }
    p
}
