//! `CAIDarkTribe` (release vtable 0x102b9e8, 0x406c bytes): the Dark Tribe AI's plan of its
//! land, one per Dark Tribe sector AI (made by 0x44b1c0 from `CAIDarkTribeSectorAI`'s ctor
//! 0x440600 into its +0x40c; the last one made is also .bss 0x12933a0, which only the debug
//! overlay 0x458d80 reads). Decompiles in `tools/re/decompiled/script/ai/darktribe*.c`.
//!
//! It sees the map in 16×16 cells, packed `cy << 16 | cx` (the AI land cells of
//! [`crate::animals::cells`]: `dark` = the cell's dark-land count, "light" = its grass, 0x4X and
//! 0x9X counts, "convertible" = not all water and some light land), and keeps
//! * the cells with dark land (+0x2028, `{cell, state}`; +0x28 a 64×64 grid of index + 1).
//!   State 0: dark land and light land to convert, or all dark below a state 1 / 2 cell;
//!   1: all dark and nothing above or below in state 1 / 2 — room for a mushroom farm;
//!   2: the temple's cell or a farm's; 4: all dark below a state 1 / 2 cell, or the cell under a
//!   farm;
//! * the frontier (+0x2034): cells with light land at dark land, each with a count (+0x2040)
//!   of the dark-land changes seen there;
//! * the candidates (+0x204c): the same cells, searched from the temple outwards (cursor +0x1c,
//!   radius +0x20);
//! * the targets (+0x2058, a list of at most 20 nodes): light, unblocked nodes at dark land —
//!   where the dark gardeners go next (vtbl+8 / vtbl+0xc);
//! * the farm cells (+0x2060; +0x206c a grid of index + 1): the state 1 cells in the order they
//!   became so, where a mushroom farm site is looked for (vtbl+0x14).
//!
//! `Execute` (vtbl+4, 0x4490e0) runs at every update of the player AI, from the sector AI's update
//! (0x4411c0) after its event queue: the manakopter hall order (0x44b260), the targets (0x44a410),
//! the cell states (0x44ac70).
//!
//! The release does not save it: every load builds it anew from the map (the sector AI's ctor).
//! s4rs keeps it in its own saves, so a loaded game goes on with the plan it had [deviation]; a
//! chunk 150 load builds it anew as the release does.

use crate::World;
use crate::entity::{EntityData, ObjType};

/// Cells per row of the index grids (+0x28, +0x206c).
const GRID: usize = 64;
/// .rdata 0x101bd88: the six neighbours `(dx, dy)` of a cell or node.
const DIRS: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];
/// The mushroom farm (0x31) and the manakopter hall (0x50).
const FARM: u8 = 0x31;
const HALL: u8 = 0x50;
/// `READY` or the flag next to it (+0x14 & 0x3000000), as the release tests buildings here.
const STANDING: u32 = 0x0300_0000;
/// The config variable `AI` / `DTMushroomFarmDelay` (.data 0x1182970, set to 300 by 0x415440).
const FARM_DELAY: i32 = 300;
/// 50 400 = 14 · 3600: ticks per hour.
const TICKS_PER_HOUR: u32 = 0xc4e0;

fn pack(x: i32, y: i32) -> u32 {
    ((y as u32) << 16).wrapping_add(x as u32 & 0xffff)
}

fn lo(v: u32) -> i32 {
    (v & 0xffff) as i32
}

fn hi(v: u32) -> i32 {
    (v as i32) >> 16
}

/// The cell of a node.
fn cell_of(node: u32) -> u32 {
    pack(lo(node) >> 4, hi(node) >> 4)
}

fn dist(a: u32, b: u32) -> i32 {
    crate::spiral::grid_distance(lo(a) - lo(b), hi(a) - hi(b))
}

/// `(dx, dy)` of [`DIRS`] such that `node = (x + dx, y + dy)`.
fn beside(node: u32, x: i32, y: i32) -> bool {
    DIRS.iter().any(|&(dx, dy)| lo(node) == x + dx && hi(node) == y + dy)
}

fn land(world: &World, c: u32) -> crate::animals::cells::Cell {
    world.animals.cells.get(lo(c), hi(c))
}

/// 0x44b020: not all water and some light land.
fn convertible(l: &crate::animals::cells::Cell) -> bool {
    l.bits & 1 != 0 && (l.classes[1] != 0 || l.classes[2] != 0 || l.classes[3] != 0)
}

/// The light land of a cell: grass + 0x4X + 0x9X counts.
fn light(l: &crate::animals::cells::Cell) -> u32 {
    l.classes[1] as u32 + l.classes[2] as u32 + l.classes[3] as u32
}

/// Index of `v` in `list`, as the release's linear searches (0x44a1d0, 0x44a280, 0x44a320).
fn find(list: &[u32], v: u32) -> Option<usize> {
    list.iter().position(|&x| x == v)
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DarkTribe {
    /// +4.
    pub player: u8,
    /// +8 / +0xc: cells per row and column (`MapSize >> 4`).
    pub side: i32,
    /// +0x10: the temple's cell.
    pub home: u32,
    /// +0x14: the temple's position.
    pub temple: u32,
    /// +0x18: the node index of the temple's ensign.
    pub ensign: i32,
    /// +0x1c / +0x20: the candidate search.
    pub cursor: i32,
    pub radius: i32,
    /// +0x24: the targets want refilling (0x44a410).
    pub refill: bool,
    /// +0x25: the cell states want a pass (0x44ac70).
    pub recheck: bool,
    /// +0x28: per cell (64 per row) its index in `cells` + 1.
    cell_index: Vec<u16>,
    /// +0x2028.
    pub cells: Vec<(u32, u16)>,
    /// +0x2034 / +0x2040.
    pub frontier: Vec<u32>,
    pub hits: Vec<i32>,
    /// +0x204c.
    pub candidates: Vec<u32>,
    /// +0x2058: nodes, `y << 16 | x`.
    pub targets: Vec<u32>,
    /// +0x2060 / +0x206c.
    pub farm_cells: Vec<u32>,
    farm_index: Vec<u16>,
}

impl DarkTribe {
    /// 0x44b1c0: `player`'s plan, built from the map (0x449840).
    pub fn new(world: &World, player: u8) -> DarkTribe {
        let side = (world.grid.size() >> 4) as i32;
        let mut d = DarkTribe {
            player,
            side,
            home: 0,
            temple: 0,
            ensign: 0,
            cursor: 0,
            radius: 1,
            refill: false,
            recheck: false,
            cell_index: vec![0; GRID * GRID],
            cells: Vec::new(),
            frontier: Vec::new(),
            hits: Vec::new(),
            candidates: Vec::new(),
            targets: Vec::new(),
            farm_cells: Vec::new(),
            farm_index: vec![0; GRID * GRID],
        };
        d.rebuild(world);
        d
    }

    /// Consistency of the lists and grids (for tests): every entry indexed at its cell and
    /// nothing else indexed, the farm cells likewise and once each, one count per frontier
    /// cell, at most 20 targets.
    pub fn check(&self) -> Result<(), String> {
        for (k, &(c, _)) in self.cells.iter().enumerate() {
            if self.index_of(c) != k + 1 {
                return Err(format!("cell {c:#x} (entry {k}) indexed as {}", self.index_of(c)));
            }
        }
        if self.cell_index.iter().filter(|&&v| v != 0).count() != self.cells.len() {
            return Err("stale cell index".into());
        }
        for (k, &c) in self.farm_cells.iter().enumerate() {
            if Self::slot(c).map(|i| self.farm_index[i] as usize) != Some(k + 1) {
                return Err(format!("farm cell {c:#x} (entry {k}) not indexed"));
            }
        }
        if self.farm_index.iter().filter(|&&v| v != 0).count() != self.farm_cells.len() {
            return Err("stale farm index".into());
        }
        if self.hits.len() != self.frontier.len() || self.targets.len() > 20 {
            return Err(format!("{} counts for {} frontier cells, {} targets", self.hits.len(), self.frontier.len(), self.targets.len()));
        }
        Ok(())
    }

    pub(crate) fn hash(&self, h: &mut crate::hash::StateHasher) {
        h.u8(self.player);
        [self.side, self.home as i32, self.temple as i32, self.ensign, self.cursor, self.radius].iter().for_each(|&v| h.u32(v as u32));
        h.bool(self.refill);
        h.bool(self.recheck);
        h.u32(self.cells.len() as u32);
        for &(c, s) in &self.cells {
            h.u32(c);
            h.u16(s);
        }
        for list in [&self.frontier, &self.candidates, &self.targets, &self.farm_cells] {
            h.u32(list.len() as u32);
            list.iter().for_each(|&v| h.u32(v));
        }
        self.hits.iter().for_each(|&v| h.u32(v as u32));
    }

    /// The grid slot of a cell; rows outside the grid read as empty (the release reads past its
    /// grid at row 64 — bytes of the vectors behind it).
    fn slot(c: u32) -> Option<usize> {
        let (cx, cy) = (lo(c), hi(c));
        (0..GRID as i32).contains(&cy).then(|| cy as usize * GRID + cx as usize).filter(|&i| i < GRID * GRID)
    }

    fn index_of(&self, c: u32) -> usize {
        Self::slot(c).map_or(0, |i| self.cell_index[i] as usize)
    }

    fn set_index(&mut self, c: u32, v: usize) {
        if let Some(i) = Self::slot(c) {
            self.cell_index[i] = v as u16;
        }
    }

    /// 0x448b20: entry `k` of `cells`, `(0, 0)` outside.
    fn entry(&self, k: usize) -> (u32, u16) {
        self.cells.get(k).copied().unwrap_or((0, 0))
    }

    /// 0x44a020: entry `k` becomes `e` (the grid follows).
    fn set_entry(&mut self, k: usize, e: (u32, u16)) {
        let Some(&(old, _)) = self.cells.get(k) else { return };
        self.set_index(old, 0);
        self.cells[k] = e;
        self.set_index(e.0, k + 1);
    }

    /// 0x449fe0: a new entry at the end.
    fn push_cell(&mut self, e: (u32, u16)) {
        self.cells.push(e);
        self.set_index(e.0, self.cells.len());
    }

    /// 0x44a0a0: entry `k` goes, the last takes its place.
    fn remove_cell(&mut self, k: usize) {
        if k >= self.cells.len() {
            return;
        }
        let last = self.cells.pop().expect("not empty");
        self.set_index(last.0, 0);
        if k < self.cells.len() {
            self.set_entry(k, last);
        }
    }

    /// 0x4488f0 (and inline in 0x44ac70): a farm cell, once.
    fn add_farm_cell(&mut self, c: u32) {
        let Some(i) = Self::slot(c) else { return };
        if self.farm_index[i] == 0 {
            self.farm_cells.push(c);
            self.farm_index[i] = self.farm_cells.len() as u16;
        }
    }

    /// The state of the cell's entry, if it has one.
    fn state_at(&self, c: u32) -> Option<u16> {
        let k = self.index_of(c);
        (k != 0).then(|| self.entry(k - 1).1)
    }

    /// Whether the cell's entry is in state 1 or 2 (none: no).
    fn taken(&self, c: u32) -> bool {
        matches!(self.state_at(c), Some(1 | 2))
    }

    /// An all-dark state-0 entry `k` of cell `c` (0x449da0 / 0x44ac70): with a state 1 / 2 cell
    /// above it becomes 4; with none above or below, 1 and a farm cell; otherwise it stays.
    fn settle(&mut self, k: usize, c: u32) {
        let (cx, cy) = (lo(c), hi(c));
        let above = cy > 0 && self.taken(pack(cx, cy - 1));
        let below = cy < self.side && self.taken(pack(cx, cy + 1));
        if above {
            self.set_entry(k, (c, 4));
        } else if !below {
            self.set_entry(k, (c, 1));
            self.add_farm_cell(c);
        }
    }

    /// 0x449840: everything from the map: every cell (row by row, 0x449da0), the temple's cell
    /// (the player's first dark temple, else fortress) in state 2, every mushroom farm's cell in
    /// state 2 and the one below it in state 4. The candidates and targets are kept.
    fn rebuild(&mut self, world: &World) {
        self.cell_index.iter_mut().for_each(|v| *v = 0);
        self.cells.clear();
        self.hits.clear();
        self.frontier.clear();
        self.farm_cells.clear();
        self.farm_index.iter_mut().for_each(|v| *v = 0);
        let p = self.player as i32;
        let t = world.building_list(p, 50).first().copied().or_else(|| world.building_list(p, 51).first().copied());
        let Some(t) = t.and_then(|t| world.entities.by_index(t as u32)).filter(|e| e.obj_type == ObjType::Building) else { return };
        let (tx, ty) = (t.x(), t.y());
        let en = match &t.data {
            EntityData::Building(b) => b.ensign,
            _ => crate::grid::Y16X16(0),
        };
        for cy in 0..self.side {
            for cx in 0..self.side {
                self.classify(world, pack(cx, cy));
            }
        }
        self.temple = pack(tx, ty);
        self.home = pack(tx >> 4, ty >> 4);
        self.ensign = en.y() * world.grid.size() as i32 + en.x();
        self.mark(self.home, 2);
        for f in world.building_list(p, FARM as i32) {
            let Some(e) = world.entities.by_index(f as u32) else { continue };
            let c = pack(e.x() >> 4, e.y() >> 4);
            self.mark(c, 2);
            self.mark(c.wrapping_add(0x10000), 4);
        }
        self.refill = true;
    }

    /// The cell's entry, if any, gets `state`.
    fn mark(&mut self, c: u32, state: u16) {
        let k = self.index_of(c);
        if k != 0 && k <= self.cells.len() {
            self.set_entry(k - 1, (self.cells[k - 1].0, state));
        }
    }

    /// 0x449da0: a cell with dark land gets its entry (once); one with light land at dark land
    /// (or, without dark land, next to an all-dark cell, 0x44afa0) joins the frontier and the
    /// candidates; an all-dark one in state 0 is settled.
    fn classify(&mut self, world: &World, c: u32) {
        let (cx, cy) = (lo(c), hi(c));
        if cx >= self.side || !(0..self.side).contains(&cy) {
            return;
        }
        let l = land(world, c);
        if l.dark() != 0 {
            if self.index_of(c) == 0 {
                self.push_cell((c, 0));
            }
            if !convertible(&l) {
                let k = self.index_of(c) - 1;
                if self.entry(k).1 == 0 {
                    self.settle(k, c);
                }
                return;
            }
        } else if !self.near_dark(world, cx, cy) || !convertible(&l) {
            return;
        }
        if find(&self.frontier, c).is_none() {
            self.frontier.push(c);
            self.hits.push(0);
        }
        if find(&self.candidates, c).is_none() {
            self.candidates.push(c);
        }
    }

    /// 0x44afa0: a neighbour cell (`0..=side` both ways) with dark land and nothing to convert.
    fn near_dark(&self, world: &World, cx: i32, cy: i32) -> bool {
        DIRS.iter().any(|&(dx, dy)| {
            let (nx, ny) = (cx + dx, cy + dy);
            if !(0..=self.side).contains(&ny) || !(0..=self.side).contains(&nx) {
                return false;
            }
            let l = world.animals.cells.at(ny * crate::animals::cells::SIDE + nx);
            !convertible(&l) && l.dark() != 0
        })
    }

    /// 0x44a150: frontier entry `i` goes (its targets with it, 0x44a380), the last takes its place.
    fn remove_frontier(&mut self, i: usize) {
        if i >= self.frontier.len() {
            return;
        }
        let c = self.frontier[i];
        let before = self.targets.len();
        self.targets.retain(|&t| cell_of(t) != c);
        if self.targets.len() != before && self.targets.len() < 10 {
            self.refill = true;
        }
        self.frontier.swap_remove(i);
        self.hits.swap_remove(i);
    }

    /// 0x44a230.
    fn remove_candidate(&mut self, i: usize) {
        if i < self.candidates.len() {
            self.candidates.swap_remove(i);
        }
    }

    /// 0x44b360 on the targets: every copy of `node` goes; under 10 left asks for a refill.
    fn drop_target(&mut self, node: u32) {
        self.targets.retain(|&t| t != node);
        if self.targets.len() < 10 {
            self.refill = true;
        }
    }

    /// vtbl+0x18 (0x449110), the sector AI's event 0x1d: node `(x, y)` turned dark (`dark`) or
    /// light. It stops being a target; a frontier cell counts the change and is looked at again
    /// only after 17 changes or once it has nothing left to convert (0x449bd0).
    pub fn land_changed(&mut self, world: &World, x: i32, y: i32, dark: bool) {
        let c = pack(x >> 4, y >> 4);
        let node = pack(x, y);
        if find(&self.targets, node).is_some() && !self.targets.is_empty() {
            self.drop_target(node);
        }
        if let Some(i) = find(&self.frontier, c) {
            self.hits[i] += 1;
            if self.hits[i] < 0x11 && convertible(&land(world, c)) {
                return;
            }
        }
        self.reclassify(world, c, dark);
    }

    /// 0x449bd0. Light: a cell left without dark land loses its entry, frontier place and
    /// candidacy and is classified again. Dark: the cell is classified (when new), is a candidate
    /// and on the frontier; once it has nothing left to convert, its neighbours that its tiles
    /// link to (`CTiling` vtbl+0x20) are classified and it leaves both lists.
    fn reclassify(&mut self, world: &World, c: u32, dark: bool) {
        let (cx, cy) = (lo(c), hi(c));
        if cx >= self.side || !(0..self.side).contains(&cy) {
            return;
        }
        let l = land(world, c);
        if !dark {
            let k = self.index_of(c);
            if k != 0 && l.dark() == 0 {
                self.remove_cell(k - 1);
                if let Some(i) = find(&self.frontier, c) {
                    self.remove_frontier(i);
                }
                if let Some(i) = find(&self.candidates, c) {
                    self.remove_candidate(i);
                }
                self.classify(world, c);
                self.recheck = true;
            }
            return;
        }
        if self.index_of(c) == 0 {
            self.classify(world, c);
        }
        if find(&self.candidates, c).is_none() {
            self.candidates.push(c);
        }
        if find(&self.frontier, c).is_none() {
            self.frontier.push(c);
            self.hits.push(0);
        }
        if convertible(&l) {
            return;
        }
        self.recheck = true;
        for (dx, dy) in DIRS {
            let (nx, ny) = (cx + dx, cy + dy);
            if (0..=self.side).contains(&ny) && (0..=self.side).contains(&nx) && tiles_linked(world, cx, cy, nx, ny) {
                self.classify(world, pack(nx, ny));
            }
        }
        if let Some(i) = find(&self.candidates, c) {
            self.remove_candidate(i);
        }
        if let Some(i) = find(&self.frontier, c) {
            self.remove_frontier(i);
        }
    }

    /// 0x44a5c0: the next candidate from the cursor within `radius` cells of the temple's; a full
    /// round without one widens the radius, and past 99 the nearest candidate is taken. None
    /// without candidates.
    fn next_candidate(&mut self) -> Option<usize> {
        let n = self.candidates.len() as i32;
        if n == 0 {
            return None;
        }
        let start = self.cursor;
        self.cursor += 1;
        if self.cursor >= n {
            self.cursor = 0;
        }
        // The release loops for ever when the cursor was left past the end of a shrunken list
        // and no candidate is in range (it never meets `start` again); s4rs widens the radius
        // after a full round instead [deviation].
        let mut steps = 0;
        loop {
            if dist(self.home, self.candidates[self.cursor as usize]) <= self.radius {
                return Some(self.cursor as usize);
            }
            self.cursor += 1;
            if self.cursor >= n {
                self.cursor = 0;
            }
            steps += 1;
            if self.cursor == start || (start >= n && steps % n == 0) {
                self.radius += 1;
                if self.radius > 99 {
                    let mut best = (0xffff, None);
                    for (k, &c) in self.candidates.iter().enumerate() {
                        let d = dist(self.home, c);
                        if d < best.0 {
                            best = (d, Some(k));
                        }
                    }
                    return best.1;
                }
            }
        }
    }

    /// 0x44a410: when asked to, the targets are filled up to 20: per candidate (from
    /// [`Self::next_candidate`]) the first of its border nodes (0x44a790) not yet a target; a
    /// candidate without light land leaves the list, and the first candidate that gives no new
    /// node ends the round.
    fn refill_targets(&mut self, world: &World) {
        if !self.refill {
            return;
        }
        self.refill = false;
        if self.targets.len() >= 20 {
            return;
        }
        let mut k = self.next_candidate();
        while self.targets.len() < 20 {
            let Some(i) = k else { break };
            let c = self.candidates.get(i).copied().unwrap_or(0);
            if light(&land(world, c)) == 0 {
                self.remove_candidate(i);
                break;
            }
            let new = border_nodes(world, lo(c), hi(c)).into_iter().find(|&u| find(&self.targets, u).is_none());
            let Some(u) = new else { break };
            self.targets.push(u);
            k = self.next_candidate();
        }
    }

    /// 0x44ac70: when asked to, every entry (as many as there were at the start): an all-dark
    /// state-0 cell off the frontier is settled; a state-1 cell is a farm cell (once).
    fn update_cells(&mut self) {
        if !self.recheck {
            return;
        }
        self.recheck = false;
        let n = self.cells.len();
        for k in 0..n {
            let (c, state) = self.entry(k);
            match state {
                0 if find(&self.frontier, c).is_none() => self.settle(k, c),
                1 => self.add_farm_cell(c),
                _ => {}
            }
        }
    }

    /// vtbl+0xc (0x449510): the first target, taken off the list (every copy).
    pub fn take_target(&mut self) -> Option<u32> {
        let v = *self.targets.first()?;
        self.drop_target(v);
        Some(v)
    }

    /// vtbl+8 (0x4491f0) `(node)`: a node beside `node` to darken next — a target in its cell,
    /// else a border node of its cell, else from the frontier cells around its cell: the last
    /// border node beside `node`, or the first border node of the first such cell without one.
    /// 0 when nothing is found.
    pub fn target_near(&self, world: &World, node: u32) -> u32 {
        let (x, y) = (lo(node), hi(node));
        let c = pack(x >> 4, y >> 4);
        if find(&self.frontier, c).is_some()
            && let Some(&t) = self.targets.iter().find(|&&t| cell_of(t) == c && beside(t, x, y))
        {
            return t;
        }
        if light(&land(world, c)) != 0
            && let Some(u) = border_nodes(world, x >> 4, y >> 4).into_iter().find(|&u| beside(u, x, y))
        {
            return u;
        }
        let mut found = None;
        for &f in &self.frontier {
            if !beside(f, x >> 4, y >> 4) || light(&land(world, f)) == 0 {
                continue;
            }
            let nodes = border_nodes(world, lo(f), hi(f));
            if nodes.is_empty() {
                continue;
            }
            match nodes.iter().find(|&&u| beside(u, x, y)) {
                Some(&u) => found = Some(u),
                None if found.is_none() => return nodes[0],
                None => {}
            }
        }
        found.unwrap_or(0)
    }

    /// vtbl+0x10 (0x449570): whether a site of `building` may be looked for (mushroom farms
    /// only, while there are farm cells).
    pub fn has_site(&self, building: u8) -> bool {
        building == FARM && !self.farm_cells.is_empty()
    }

    /// 0x44a960: a mushroom farm site. The farm cell nearest the temple's (not within 24 of a
    /// listed hall position, 0x53f860, and with nothing left to convert) leaves the list; its
    /// nodes, row by row, give the first one in the temple ensign's sector where the farm may be
    /// built (`CheckForBuild`), far enough from every Dark Tribe temple / fortress (0x4cbdb0) and
    /// from every building's ensign (0x44aeb0). A node with trees or stone (resource type 7 / 6
    /// with an amount) skips ahead by the farm's bounding box (−minX). None: no site (the cell is
    /// gone all the same).
    fn farm_site(&mut self, world: &World) -> Option<(i32, i32)> {
        if !self.has_site(FARM) {
            return None;
        }
        let mut best = (0xffff, None);
        for (k, &c) in self.farm_cells.iter().enumerate() {
            let corner = crate::grid::Y16X16::pack(lo(c) << 4, hi(c) << 4);
            if crate::build::halls::near_position(world, self.player, corner) || convertible(&land(world, c)) {
                continue;
            }
            let d = dist(self.home, c);
            if d < best.0 {
                best = (d, Some(k));
            }
        }
        let k = best.1?;
        let size = world.grid.size() as i32;
        let sector = usize::try_from(self.ensign).ok().filter(|&n| n < world.grid.len()).map_or(0, |n| world.tiling.sector_of(n));
        let c = self.farm_cells[k];
        let last = self.farm_cells.len() - 1;
        if let Some(i) = Self::slot(c) {
            self.farm_index[i] = 0;
        }
        if k < last {
            let m = self.farm_cells[last];
            self.farm_cells[k] = m;
            if let Some(i) = Self::slot(m) {
                self.farm_index[i] = k as u16 + 1;
            }
        }
        self.farm_cells.pop();
        let min_x = world.rules.building(3, FARM).bounding_rect[0] as i32;
        let (x0, y0) = (lo(c) << 4, hi(c) << 4);
        for y in y0..y0 + 16 {
            let mut x = x0;
            while x < x0 + 16 {
                let n = (y * size + x) as usize;
                let r = world.layers.resource.get(n).copied().unwrap_or(0);
                if matches!(r & 0xf0, 0x60 | 0x70) && r & 0xf != 0 {
                    x -= min_x;
                } else if world.tiling.sector_of(n) == sector
                    && crate::build::check::check_for_build(world, x, y, self.player, FARM).code() > 0
                    && far_from_temples(world, self.player, x, y)
                    && no_building_near(world, x, y)
                {
                    return Some((x, y));
                }
                x += 1;
            }
        }
        None
    }
}

/// 0x44a790 `(cx, cy)`: the cell's nodes without flags 1 / 4 that have dark neighbours (0x44b070)
/// — first those with at least five, then all of them (so those twice), row by row.
fn border_nodes(world: &World, cx: i32, cy: i32) -> Vec<u32> {
    let size = world.grid.size() as i32;
    let flags = &world.layers.flags;
    let dark = |x: i32, y: i32| (0..size).contains(&x) && (0..size).contains(&y) && flags[(y * size + x) as usize] & 4 != 0;
    let count = |x: i32, y: i32| DIRS.iter().filter(|&&(dx, dy)| dark(x + dx, y + dy)).count();
    let mut out = Vec::new();
    for pass in 0..2 {
        for y in cy << 4..(cy << 4) + 16 {
            for x in cx << 4..(cx << 4) + 16 {
                let Some(n) = world.grid.checked_index(x, y) else { continue };
                if flags[n] & 5 != 0 {
                    continue;
                }
                let k = count(x, y);
                if (pass == 0 && k > 4) || (pass == 1 && k != 0) {
                    out.push(pack(x, y));
                }
            }
        }
    }
    out
}

/// `CTiling` vtbl+0x20 (0x5b7d60) `(cx, cy, nx, ny)`: a land tile of square `(cx, cy)` links to
/// a land tile whose centre lies in square `(nx, ny)`.
fn tiles_linked(world: &World, cx: i32, cy: i32, nx: i32, ny: i32) -> bool {
    let side = (world.grid.size() >> 4) as usize;
    let t = &world.tiling;
    let mut id = t.first_tile_of_square(cx as usize, cy as usize, side);
    while id != 0 {
        let tile = t.tile(id);
        if tile.ty & crate::tiling::TILE_LAND != 0
            && tile.links.iter().any(|&l| {
                let o = t.tile(l);
                o.ty & crate::tiling::TILE_LAND != 0 && o.center.x() >> 4 == nx && o.center.y() >> 4 == ny
            })
        {
            return true;
        }
        id = tile.sq_next;
    }
    false
}

/// `CDarkTribe` vtbl+0x14 (0x4cbdb0) `(player, x, y)`: on the map 16 nodes in from the edge and
/// not within 40 of the player's own standing dark temples and fortresses, nor within 60 of
/// another Dark Tribe player's.
fn far_from_temples(world: &World, player: u8, x: i32, y: i32) -> bool {
    let size = world.grid.size() as i32;
    if !(16..size - 16).contains(&x) || !(16..size - 16).contains(&y) {
        return false;
    }
    for p in 1..=world.player_count() {
        if world.race_of(p) != 3 {
            continue;
        }
        let r = if p == player { 40 } else { 60 };
        for kind in [50, 51] {
            for b in world.building_list(p as i32, kind) {
                let Some(e) = world.entities.by_index(b as u32) else { break };
                if e.flags & STANDING != 0 && crate::spiral::grid_distance(x - e.x(), y - e.y()) < r {
                    return false;
                }
            }
        }
    }
    true
}

/// 0x44aeb0 `(x, y)`: no building of the war map's cells within 22 (0x5b0310) has its ensign
/// within 22 of `(x, y)`.
fn no_building_near(world: &World, x: i32, y: i32) -> bool {
    for (cx, cy) in crate::warmap::cells_within(world, x, y, 0x16) {
        for b in world.warmap.cell(cx as usize, cy as usize, 2) {
            let Some(EntityData::Building(d)) = world.entities.by_index(b as u32).map(|e| &e.data) else { continue };
            if crate::spiral::grid_distance(d.ensign.x() - x, d.ensign.y() - y) <= 0x16 {
                return false;
            }
        }
    }
    true
}

/// The player's plan, taken out of the world while it works (put back by [`put`]).
fn take(world: &mut World, player: u8) -> Option<Box<DarkTribe>> {
    world.ai.players.get_mut(player as usize)?.as_mut()?.dark_ai.take()
}

fn put(world: &mut World, player: u8, d: Box<DarkTribe>) {
    if let Some(Some(a)) = world.ai.players.get_mut(player as usize) {
        a.dark_ai = Some(d);
    }
}

/// `Execute` (vtbl+4, 0x4490e0).
pub fn execute(world: &mut World, player: u8) {
    hall_order(world, player);
    let Some(mut d) = take(world, player) else { return };
    d.refill_targets(world);
    d.update_cells();
    put(world, player, d);
}

/// The sector AI's event 0x1d (0x4411e0 → vtbl+0x18): see [`DarkTribe::land_changed`].
pub fn land_changed(world: &mut World, player: u8, x: i32, y: i32, dark: bool) {
    let Some(mut d) = take(world, player) else { return };
    d.land_changed(world, x, y, dark);
    put(world, player, d);
}

/// 0x44b260: with hall positions listed, no hall (built or a site) and more than
/// `DTManakopterHallDelay` · 14 ticks since a hall was last destroyed, the first listed position
/// a hall may be built on (0x53f750) gets the build order (5025, type 0x50).
fn hall_order(world: &mut World, player: u8) {
    let p = player as usize;
    let listed = world.sites.halls.positions.get(p).is_some_and(|l| !l.is_empty());
    if !listed || crate::build::halls::has_hall(world, player) {
        return;
    }
    let since = world.tick.wrapping_sub(world.sites.halls.tick.get(p).copied().unwrap_or(0)) / 14;
    if world.ai.vars.get(player, 19) >= since as i32 {
        return;
    }
    if let Some(pos) = crate::build::halls::build_position(world, player) {
        super::send(world, 5025, player, HALL as u32, pos.0);
    }
}

/// `CAIAgentDarkTribeMushroomFarms::Execute` (0x430b00) of a sector AI whose anchor building
/// stands: with no mushroom farm at all it waits 1000 ticks; while the farms (sites included) are
/// fewer than `DTMaxFarmsBase + DTMaxFarmsPerHour · hours` (no limit with no hourly count, none
/// at all with a sum of 0), it orders one at a site from the plan (vtbl+0x10 / vtbl+0x14,
/// 0x4495a0) and waits `DTMushroomFarmDelay · 14` ticks plus 0..127 (one game RNG draw).
pub fn mushroom_farms(world: &mut World, player: u8, anchor: u16, now: u32) -> u32 {
    if now == 0 {
        return 0;
    }
    if !world.entities.by_index(anchor as u32).is_some_and(|e| e.flags & STANDING != 0) {
        return 0;
    }
    let farms = world.buildings_amount(player as i32, FARM as i32, crate::query::ALL);
    if farms < 1 {
        return 0x3e8000;
    }
    let per_hour = world.ai.vars.get(player, 13);
    if per_hour >= 1 {
        let limit = ((per_hour as u32).wrapping_mul(now) / TICKS_PER_HOUR) as i32 + world.ai.vars.get(player, 12);
        if limit != 0 && farms >= limit {
            return 0;
        }
    }
    let Some(mut d) = take(world, player) else { return 0 };
    let site = if d.has_site(FARM) { d.farm_site(world) } else { None };
    put(world, player, d);
    let Some((x, y)) = site else { return 0 };
    super::send(world, 5025, player, FARM as u32, pack(x, y));
    let r = world.rng.rand();
    (((r & 0x7f) as i32 + FARM_DELAY.clamp(1, 0xe10) * 14) * 0x1000) as u32
}
