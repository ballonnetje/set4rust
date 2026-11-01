//! The AI's landscape statistics per 16×16 cell (release .bss 0x1267bc0: 42 bytes per cell, 64
//! cells per row, one zeroed margin row above and below — `memset(0x1267140, 0, 0x2b500)` in
//! 0x43c100). `CAnimalMgr` reads it to decide where animals live, so it is ported with them.
//!
//! * Built once per game by 0x43c100, which `CAIMain` vtbl+12 (0x438370) calls at the end of the
//!   map load (0x4ff460: after buildings, settlers, piles and the script chunk), whether AI
//!   players exist or not. Per cell: the resource sums (0x43b7b0), the ground bits (0x43ba20) and,
//!   for a cell that is not all water, the ground-class counts (0x43bc40).
//! * Rescanned by `CAIMain::Update` every tick, with or without AI players (0x43c280, enabled by
//!   the end of 0x43c100): the resource sums of `ceil(side² / 48)` cells, row by row, so the
//!   whole map every 48 ticks ([`rescan`]). The class counts and ground bits are not rescanned.
//! * `CAIMain::PostAIEvent` (0x438620) event 0x1d (posted by the dark-land setter 0x4cbb60)
//!   recomputes the class counts and ground bits of the node's cell (0x43c3c0). Event 2 (0x43c2e0,
//!   an incremental resource update) is posted by nobody in the release. Buildings placed later
//!   do not change the class counts (their blocked land would).
//! * The 3×3 neighbourhood sums and bits (+0x10..+0x1e, +0x28; 0x43b8e0), which only the
//!   eco-sector AI reads: every cell after the first pass of 0x43c100, and the nine cells around
//!   each rescanned one (0x43be60). Not kept: the per-row dark flags (0x15aa008).
//!
//! Indexing follows the original's flat array: a neighbour index that runs off the side of a row
//! lands in the previous / next row, one that runs off the top or bottom lands in a margin row
//! (zero). Anything further out reads as an empty cell.

use crate::hash::StateHasher;
use crate::world::World;

/// Cells per row (`MapSize >> 4` of the largest map).
pub const SIDE: i32 = 64;

/// 0x15aa050 (filled by 0x43c100 / 0x43bb70): ground type `>> 4` → ground class. 1 grass,
/// 2 ground 0x4X, 3 ground 0x9X, 4 ground 0x2X, 7 everything else (not stored).
const GROUND_CLASS: [usize; 16] = [7, 1, 4, 7, 2, 7, 7, 7, 7, 3, 7, 7, 7, 7, 7, 7];

/// One cell (the fields `CAnimalMgr` reads, plus the rest of what the same functions write).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Cell {
    /// +0x00..+0x0e: the resource amounts summed per resource type 0..7 (0x43b7b0). Type 7 (+0x0e)
    /// is the tree count (a tree node holds resource 0x71).
    pub resources: [u16; 8],
    /// +0x20..+0x26 (0x43bc40), over the nodes without flags 0x49: 0 dark land (flag 0x04), 1 grass
    /// (ground 0x1X), 2 ground 0x4X, 3 ground 0x9X, 4 ground 0x2X; 5 / 6 the grass nodes with a
    /// height step below / at least 8. Each count is stored minus one when it is 256.
    pub classes: [u8; 7],
    /// +0x27 (0x43ba20): 1 not all water, 2 some water (ground < 0x10), 8 / 0x10 / 4 / 0x20 / 0x40
    /// some ground 0x4X / 0x5X / 0x6X / 0x7X / 0x8X.
    pub bits: u8,
    /// +0x10..+0x1e (0x43b8e0): the resource sums of the 3×3 cells around (u16, wrapping);
    /// +0x28: their +0x27 bits or-ed.
    #[cfg_attr(feature = "serde", serde(default))]
    pub around: [u16; 8],
    #[cfg_attr(feature = "serde", serde(default))]
    pub around_bits: u8,
}

impl Cell {
    /// +0x0e.
    pub fn trees(&self) -> u16 {
        self.resources[7]
    }
    /// +0x20.
    pub fn dark(&self) -> u8 {
        self.classes[0]
    }
    /// +0x21.
    pub fn grass(&self) -> u8 {
        self.classes[1]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LandCells {
    /// `(SIDE + 2)` rows of `SIDE` cells, row 0 and the last row the margins.
    cells: Vec<Cell>,
    /// 0x1266e24: the rescan runs (set at the end of a successful 0x43c100).
    #[cfg_attr(feature = "serde", serde(default))]
    enabled: bool,
    /// 0x1589b44 / 0x15a9b4c: the next cell the rescan visits.
    #[cfg_attr(feature = "serde", serde(default))]
    cursor: (i32, i32),
}

impl Default for LandCells {
    fn default() -> Self {
        LandCells { cells: vec![Cell::default(); (SIDE * (SIDE + 2)) as usize], enabled: false, cursor: (0, 0) }
    }
}

impl LandCells {
    /// The cell at flat index `cy * 64 + cx` (the original's addressing; see the module doc).
    pub fn at(&self, flat: i32) -> Cell {
        self.at_ref(flat).copied().unwrap_or_default()
    }

    /// [`Self::at`] without the copy; `None` reads as the default cell.
    fn at_ref(&self, flat: i32) -> Option<&Cell> {
        usize::try_from(flat + SIDE).ok().and_then(|i| self.cells.get(i))
    }

    pub fn get(&self, cx: i32, cy: i32) -> Cell {
        self.at(cy * SIDE + cx)
    }

    fn get_mut(&mut self, cx: i32, cy: i32) -> Option<&mut Cell> {
        self.cells.get_mut(usize::try_from((cy + 1) * SIDE + cx).ok()?)
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.bool(self.enabled);
        h.u32(self.cursor.0 as u32);
        h.u32(self.cursor.1 as u32);
        for c in &self.cells {
            c.resources.iter().for_each(|&v| h.u16(v));
            h.bytes(&c.classes);
            h.u8(c.bits);
            c.around.iter().for_each(|&v| h.u16(v));
            h.u8(c.around_bits);
        }
    }
}

/// 0x43c100 (the parts kept here): every cell of a map whose size is a multiple of 16; the
/// rescan starts at the first cell.
pub fn init(world: &mut World) {
    world.animals.cells = LandCells::default();
    let size = world.grid.size() as i32;
    if size > 1024 || size & 0xf != 0 {
        return; // "CInit(): Invalid map width or height!"
    }
    let side = size >> 4;
    for cy in 0..side {
        for cx in 0..side {
            resource_sums(world, cx, cy);
            if ground_bits(world, cx, cy) {
                ground_classes(world, cx, cy);
            }
        }
    }
    for cy in 0..side {
        for cx in 0..side {
            around(world, cx, cy);
        }
    }
    world.animals.cells.enabled = true;
}

/// 0x43c280 (from `CAIMain::Update`): `ceil(side² / 48)` cells (0x1563afc) get their resource
/// sums again (0x43be60 → 0x43b7b0; the 3×3 sums it also refreshes are not kept), the cursor
/// running along the rows and wrapping to the first cell.
pub fn rescan(world: &mut World) {
    if !world.animals.cells.enabled {
        return;
    }
    let side = world.grid.size() as i32 >> 4;
    let per_tick = (side * side + 0x2f) / 0x30;
    for _ in 0..per_tick {
        let (x, y) = world.animals.cells.cursor;
        // 0x43be60: the sums, then the 3×3 sums of the nine cells around.
        resource_sums(world, x, y);
        for dy in [-1, 0, 1] {
            around(world, x, y + dy);
        }
        if x > 0 {
            for dy in [-1, 0, 1] {
                around(world, x - 1, y + dy);
            }
        }
        if x < 0x3f {
            for dy in [-1, 0, 1] {
                around(world, x + 1, y + dy);
            }
        }
        let (mut x, mut y) = (x + 1, y);
        if x >= side {
            x = 0;
            y += 1;
            if y >= side {
                y = 0;
            }
        }
        world.animals.cells.cursor = (x, y);
    }
}

/// 0x43b8e0 (+0x10..+0x1e of cell `(cx, cy)`): the resource sums of the 3×3 cells around, each
/// sum cut to 16 bits. The release stores them; it recomputes them for the nine cells around a
/// cell whenever that cell's sums are rescanned (0x43be60) and once for all cells after building
/// them (0x43c100), so they always equal this. Neighbours left or right of the 64 columns are
/// left out, rows above or below read the margin (0).
pub fn neighbour_sums(cells: &LandCells, cx: i32, cy: i32) -> [u16; 8] {
    let mut sum = [0u32; 8];
    if !(0..SIDE).contains(&cx) || !(0..SIDE).contains(&cy) {
        return [0; 8];
    }
    for dy in -1..=1 {
        for dx in -1..=1 {
            if !(0..SIDE).contains(&(cx + dx)) {
                continue;
            }
            let c = cells.at((cy + dy) * SIDE + cx + dx);
            for (o, &r) in sum.iter_mut().zip(&c.resources) {
                *o += r as u32;
            }
        }
    }
    sum.map(|v| v as u16)
}

/// 0x43c3c0 (`PostAIEvent` 0x1d, posted by the dark-land setter 0x4cbb60 after it changed node
/// flag 0x04 at `(x, y)`): the class counts and ground bits of that cell again.
pub fn dark_land_changed(world: &mut World, x: i32, y: i32) {
    let (cx, cy) = (x >> 4, y >> 4);
    ground_classes(world, cx, cy);
    ground_bits(world, cx, cy);
}

/// Nodes of the cell, row by row.
fn nodes(world: &World, cx: i32, cy: i32) -> impl Iterator<Item = usize> + use<> {
    let size = world.grid.size() as usize;
    let (x0, y0) = (cx as usize * 16, cy as usize * 16);
    (0..16).flat_map(move |j| (0..16).map(move |i| (y0 + j) * size + x0 + i))
}

/// 0x43b7b0.
fn resource_sums(world: &mut World, cx: i32, cy: i32) {
    let mut sums = [0u32; 16];
    // The cell's nodes row by row, as [`nodes`] (a row a slice: no bounds check a node).
    let size = world.grid.size() as usize;
    let (x0, y0) = (cx as usize * 16, cy as usize * 16);
    for j in 0..16 {
        let row = (y0 + j) * size + x0;
        for &r in &world.layers.resource[row..row + 16] {
            sums[(r >> 4) as usize] += (r & 0xf) as u32;
        }
    }
    if let Some(c) = world.animals.cells.get_mut(cx, cy) {
        for (o, &s) in c.resources.iter_mut().zip(&sums) {
            *o = s as u16;
        }
    }
}

/// 0x43b8e0 `(cx, cy)` (cx, cy < 64): the 3×3 sums of the resource sums and the or of the bits,
/// over the neighbours whose x is in 0..64 (y runs on through the flat array: a margin row reads
/// as empty).
fn around(world: &mut World, cx: i32, cy: i32) {
    if !(0..64).contains(&cx) || !(0..64).contains(&cy) {
        return;
    }
    let mut sums = [0u32; 8];
    let mut bits = 0u8;
    for dy in -1..=1 {
        for dx in -1..=1 {
            if !(0..64).contains(&(cx + dx)) {
                continue;
            }
            let Some(c) = world.animals.cells.at_ref((cy + dy) * SIDE + cx + dx) else { continue };
            for (s, &r) in sums.iter_mut().zip(&c.resources) {
                *s += r as u32;
            }
            bits |= c.bits;
        }
    }
    if let Some(c) = world.animals.cells.get_mut(cx, cy) {
        for (o, &s) in c.around.iter_mut().zip(&sums) {
            *o = s as u16;
        }
        c.around_bits = bits;
    }
}

/// 0x43ba20: the ground bits; returns whether the cell is not all water.
fn ground_bits(world: &mut World, cx: i32, cy: i32) -> bool {
    let mut count = [0u32; 16];
    for n in nodes(world, cx, cy) {
        count[(world.layers.ground[n] >> 4) as usize] += 1;
    }
    let water = count[0];
    let any = |k: usize, bit: u8| if count[k] != 0 { bit } else { 0 };
    let bits = any(8, 0x40) | any(7, 0x20) | any(6, 4) | any(5, 0x10) | any(4, 8) | any(0, 2) | (water != 256) as u8;
    if let Some(c) = world.animals.cells.get_mut(cx, cy) {
        c.bits = bits;
    }
    water != 256
}

/// 0x43bc40: the ground-class counts.
fn ground_classes(world: &mut World, cx: i32, cy: i32) {
    let size = world.grid.size() as i64;
    let l = &world.layers;
    let h = |i: i64| usize::try_from(i).ok().and_then(|i| l.height.get(i)).copied().unwrap_or(0) as i32;
    let mut count = [0u32; 8];
    for n in nodes(world, cx, cy) {
        let f = l.flags[n];
        if f & 0x49 != 0 {
            continue;
        }
        let class = if f & 0x04 != 0 { 0 } else { GROUND_CLASS[(l.ground[n] >> 4) as usize] };
        count[class] += 1;
        if class == 1 {
            let i = n as i64;
            let slope = (h(i - 1) - h(i + 1))
                .abs()
                .max((h(i - size) - h(i + size)).abs())
                .max((h(i - size - 1) - h(i + size + 1)).abs());
            count[if slope < 8 { 5 } else { 6 }] += 1;
        }
    }
    if let Some(c) = world.animals.cells.get_mut(cx, cy) {
        for (o, &v) in c.classes.iter_mut().zip(&count) {
            *o = (v - (v == 256) as u32) as u8;
        }
    }
}
