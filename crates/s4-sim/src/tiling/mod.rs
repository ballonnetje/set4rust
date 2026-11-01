//! The tiling (`Pathing/CTiling`, `CSquare`, `CTilesEx`, `CTilesAndLinks`, `CLinks`, `CSectors`,
//! `CWaterFlagsEx`, `COwnerMapEx`): the map cut into tiles — connected groups of nodes of the same
//! kind inside one 16×16 square — linked to their neighbours, grouped into sectors (walkable
//! connectivity) and eco-sectors (connected land of one owner). Path finding runs on it.
//!
//! * [`init`]: the full rebuild `CTiling::Init` (Tiling.cpp:3921), run when a map or savegame loads;
//! * [`update`]: the incremental changes `SetBlockedLand`, `ClearBlockedLand`, `ChangeOwner`,
//!   `Set/ClearBlockedWater` with tile split/merge and sector / eco-sector split/merge/join.
//!
//! Tile ids: 0..=8 are the owner pseudo tiles (`OwnerPseudoTileId(owner) = owner`) that blocked
//! nodes keep, 9 and 0x7FFF are list sentinels, real tiles start at 10 and are allocated in push
//! order during `Init`.
//!
//! Tile type bits (`CTile::m_iType`): 0..3 owner, 4..12 `16 << owner`, 13..21 size (≤ 256),
//! 22/23 square x/y parity (keeps tiles of neighbouring squares distinct), 26 walkable water
//! (water flag 0x400), 27 catapult tiling, 28 land, 29 water, 30 slow ground.

use std::collections::VecDeque;

use crate::grid::{Grid, Y16X16, flag};
use crate::hash::StateHasher;

pub mod update;
pub use update::Ctx;

pub const TILE_LAND: u32 = 0x1000_0000;
pub const TILE_WATER: u32 = 0x2000_0000;
pub const TILE_CATAPULT: u32 = 0x0800_0000;
pub const TILE_WALKABLE_WATER: u32 = 0x0400_0000;
/// First real tile id.
pub const FIRST_REAL: u16 = 10;
const SENTINEL: u16 = 0x7FFF;
const UNUSED: u32 = 0xFFFF_FFFF;
/// `PushTileUndef` refuses beyond this many used tiles.
const MAX_USED_TILES: u32 = 32757;

/// `CTile`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tile {
    /// `m_iType` (`UNUSED` = -1 free, -2 / -3 sentinels).
    pub ty: u32,
    /// `m_uCenter`: the node nearest to the centroid that belongs to the tile.
    pub center: Y16X16,
    /// `m_iSectorId` (low half) and `m_iEcoSectorId` (high half).
    pub sector: u16,
    pub eco_sector: u16,
    /// `m_pLinkList`: neighbouring tiles, in discovery order (removal swaps in the last entry).
    pub links: Vec<u16>,
    /// `m_uSquarePrev` / `m_uSquareNext`: per-square tile list (normal tiles at the front,
    /// catapult tiles at the back).
    pub sq_prev: u16,
    pub sq_next: u16,
}

impl Tile {
    fn unused() -> Tile {
        Tile { ty: UNUSED, center: Y16X16(0), sector: 0, eco_sector: 0, links: Vec::new(), sq_prev: 0, sq_next: 0 }
    }
    /// `CTile::TilingType`.
    pub fn catapult(&self) -> bool {
        self.ty & TILE_CATAPULT != 0
    }
    /// `CTile::SetSize` (clamped to 0..=256).
    fn set_size(&mut self, size: i32) {
        let s = size.clamp(0, 256) as u32;
        self.ty = (self.ty & 0xFFC0_1FFF) | s << 13;
    }
    /// `CTile::ChangeSize`.
    fn change_size(&mut self, delta: i32) {
        self.set_size(self.size() as i32 + delta);
    }
    /// `CTile::OwnerId`.
    pub fn owner(&self) -> u8 {
        (self.ty & 0xF) as u8
    }
    /// `CTile::Size`.
    pub fn size(&self) -> u32 {
        (self.ty & 0x3F_E000) >> 13
    }
    /// `CTile::Used`.
    pub fn used(&self) -> bool {
        self.ty < 0xFFFF_FFFD
    }
}

/// `CTile::TileType(owner)`.
pub fn owner_tile_type(owner: u8) -> u32 {
    owner as u32 | (16 << owner)
}

/// `CLandscapeProperties` (ctor table): per ground type a byte whose low nibble, shifted to bits
/// 28..31, gives the tile type (1 land, 2 water, 4 slow). Unlisted grounds are 0.
pub fn ground_tile_type(ground: u8) -> u32 {
    let b: u32 = match ground {
        0..=8 => 18,
        16 | 18 | 20 | 21 | 23 | 24 | 25 | 28 | 29 | 48 | 64 | 65 | 96..=99 | 144 | 145 => 1,
        17 | 32 | 33 | 35 => 5,
        80 | 81 | 128 | 129 => 17,
        _ => 0,
    };
    b.wrapping_shl(28)
}

/// `CSquare::TileTypeXY`.
/// Rows of [`Tiling::owner_map`]: 64 and a margin above and below (memset 0x2100 at 0x5aff50).
const OWNER_MAP_ROWS: usize = 66;

fn square_parity(x: i32, y: i32) -> u32 {
    (((y >> 4) & 1) as u32) << 23 | (((x >> 4) & 1) as u32) << 22
}

/// `g_sSurroundingHexPoints8`: hex rings around a node, ring r starting at (r, 0). Copied by
/// content check (`tests/sim_spiral.rs`: 721 entries at .rdata file offset 0xc1b1b8).
pub fn hex_points(max_radius: i32) -> Vec<(i32, i32)> {
    let mut v = vec![(0, 0)];
    for r in 1..=max_radius {
        let (mut x, mut y) = (r, 0);
        v.push((x, y));
        for (dx, dy, n) in [(0, 1, r), (-1, 0, r), (-1, -1, r), (0, -1, r), (1, 0, r), (1, 1, r - 1)] {
            for _ in 0..n {
                x += dx;
                y += dy;
                v.push((x, y));
            }
        }
    }
    v
}

/// `g_sNeighborPoints`.
pub const NEIGHBORS: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];

/// A `CTiling` → `CEcoSectorMgr` notification, queued during a tiling operation and handed to
/// the economy (`eco::apply_tiling_notes`) right after it. The handlers only use the ids and
/// positions they are given, so running them after the operation is equivalent.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EcoNote {
    /// `NotifyCreateEcoSector(owner)` → `CEcoSectorMgr::NewEcoSector`.
    Create { id: u16, owner: u8 },
    /// `NotifyDeleteEcoSector`.
    Delete { id: u16 },
    /// `NotifyChangeEcoSector(node, old, new)` for a node holding a pile or a map object (the
    /// other nodes only change sizes, which the tiling keeps).
    Node { node: u32, old: u16, new: u16 },
    /// `MergeEcoSectors`: `NotifyEcoSectorMerge(keep, gone)` before the walk.
    MergeStart { keep: u16, gone: u16 },
    /// `NotifyEcoSectorsMerged(keep, gone)` → `ConnectEconomy` after the walk.
    Merged { keep: u16, gone: u16 },
    /// `NotifyEcoSectorSplit(old, new)` → `SeparateEconomy`.
    Split { old: u16, new: u16 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tiling {
    /// `m_pTileIds[0]`: normal tile id per node.
    pub normal: Vec<u16>,
    /// `m_pTileIds[1]`: catapult tile id per node (0 where none).
    pub catapult: Vec<u16>,
    /// `CWaterFlags` layer (shares the catapult buffer in the original; values ≥ 0xC000 on water).
    pub water_flags: Vec<u16>,
    pub tiles: Vec<Tile>,
    /// `CTilesEx::m_iLastUsedTileId`.
    pub last_used: u16,
    /// `CSectors`: used flags, allocation cursor, sizes.
    pub sector_used: Vec<u8>,
    pub sector_cursor: u16,
    pub sector_count: u32,
    pub sector_sizes: Vec<u32>,
    /// `m_iEcoSectorSizes` (nodes per eco-sector, -1 = unused).
    pub eco_sizes: Vec<i32>,
    /// Number of eco-sectors created by the last [`init`].
    pub last_eco: u16,
    /// `CEcoSectorMgr` slot use (`GetFreeSlot`: lowest free id >= `m_iMinFreeId`).
    pub eco_used: Vec<bool>,
    pub eco_min_free: u16,
    /// `CTilesEx` free list (head = tile 9's `m_uCenter`; deleted tiles go to the front).
    pub free: VecDeque<u16>,
    pub num_used: u32,
    /// `CTiles::m_sSquareFirstLastTiles`, per 16x16 square `(first, last)`.
    pub square_lists: Vec<(u16, u16)>,
    /// `COwnerMapEx` (.bss 0x12ef918: 66 rows of 64 squares, two bytes each; row 0 and row 65
    /// are margins). Byte 0 of a 16×16 square: the owners (bit `owner − 1`) of the tiles whose
    /// centre lies in it; byte 1: the owners of byte 0 of the square and its eight neighbours.
    /// See [`Tiling::owner_bits`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub owner_map: Vec<[u8; 2]>,
    /// .bss 0x141ecb4: nodes per owner 0..=8 — at `Init` (0x5b4a00) the nodes whose tile is land
    /// (type bit 28) by the tile's owner; `ChangeOwner` (0x5b79a0) moves one for every owner change
    /// it is called with, even on a blocked water node it then leaves alone. Read by the statistic
    /// (`CTiling` vtbl+0x3c, 0x5b7d40).
    #[cfg_attr(feature = "serde", serde(default))]
    pub owner_nodes: [i32; 9],
    /// 0x142ece0: the water nodes `Init` counted (the squares' normal-tiling water counts);
    /// `CTiling` vtbl+0x34 (0x5b7d20) is `size² −` it.
    #[cfg_attr(feature = "serde", serde(default))]
    pub water_nodes: i32,
    /// Pending economy notifications (transient: drained after every operation, not state).
    pub eco_notes: Vec<EcoNote>,
    /// Pending `g_pFogging->UpdateOwnerFogging(x, y, owner)` calls (transient, like
    /// `eco_notes`).
    pub fog_notes: Vec<(i32, i32, u8)>,
}

impl Tiling {
    pub fn new(grid: Grid) -> Tiling {
        let n = grid.len();
        Tiling {
            normal: vec![0; n],
            catapult: vec![0; n],
            water_flags: vec![0; n],
            tiles: Vec::new(),
            last_used: 9,
            sector_used: vec![0; 0x8000],
            sector_cursor: 1,
            sector_count: 0,
            sector_sizes: vec![0; 0x8000],
            eco_sizes: vec![-1; 0x4000],
            last_eco: 0,
            eco_used: vec![false; 0x4000],
            eco_min_free: 1,
            free: VecDeque::new(),
            num_used: 0,
            square_lists: vec![(0, 0); (grid.size() as usize / 16).pow(2)],
            owner_map: vec![[0; 2]; OWNER_MAP_ROWS * 64],
            owner_nodes: [0; 9],
            water_nodes: 0,
            eco_notes: Vec::new(),
            fog_notes: Vec::new(),
        }
    }

    pub fn ids(&self, catapult: bool) -> &[u16] {
        if catapult { &self.catapult } else { &self.normal }
    }

    fn ids_mut(&mut self, catapult: bool) -> &mut Vec<u16> {
        if catapult { &mut self.catapult } else { &mut self.normal }
    }

    /// `ITiling::FirstTileOfSquareVW` (`side` = squares per row).
    pub fn first_tile_of_square(&self, v: usize, w: usize, side: usize) -> u16 {
        self.square_lists[w * side + v].0
    }

    pub fn tile(&self, id: u16) -> &Tile {
        &self.tiles[id as usize]
    }

    /// `ITiling::SectorId`.
    pub fn sector_of(&self, node: usize) -> u16 {
        self.tiles[self.normal[node] as usize].sector
    }

    /// `ITiling::EcoSectorId`.
    pub fn eco_sector_of(&self, node: usize) -> u16 {
        self.tiles[self.normal[node] as usize].eco_sector
    }

    /// `ITiling::OwnerId`.
    pub fn owner_of(&self, node: usize) -> u8 {
        self.tiles[self.normal[node] as usize].owner()
    }

    fn is_valid_real(&self, id: u16) -> bool {
        (FIRST_REAL..SENTINEL).contains(&id) && self.tiles[id as usize].used()
    }

    /// `CEcoSectorMgr::GetFreeSlot` + `CTiling::NotifyCreateEcoSector(owner)`.
    fn create_eco(&mut self, owner: u8) -> u16 {
        let mut i = self.eco_min_free.max(1);
        while (i as usize) < self.eco_used.len() && self.eco_used[i as usize] {
            i += 1;
        }
        self.eco_min_free = i;
        self.eco_used[i as usize] = true;
        self.eco_sizes[i as usize] = 0;
        self.eco_notes.push(EcoNote::Create { id: i, owner });
        i
    }

    /// `CSectors::NewSector(2)`.
    fn create_sector(&mut self) -> u16 {
        self.sector_count += 1;
        while self.sector_used[self.sector_cursor as usize] != 0 {
            self.sector_cursor = (self.sector_cursor + 1) & 0x7FFF;
        }
        let id = self.sector_cursor;
        self.sector_used[id as usize] = 3;
        self.sector_sizes[id as usize] = 0;
        id
    }

    /// `COwnerMapEx` of the 16×16 square `(sx, sy)`: `[owners of its tiles, owners of it and its
    /// eight neighbours]` (bit `owner − 1`). Byte 1 only grows while the game runs: a tile's
    /// deletion clears byte 0 alone (0x5b0130), so byte 1 remembers owners until the next full
    /// calculation at load (0x5aff50).
    pub fn owner_bits(&self, sx: i32, sy: i32) -> [u8; 2] {
        if !(0..64).contains(&sx) || !(-1..65).contains(&sy) {
            return [0; 2];
        }
        self.owner_map.get(((sy + 1) * 64 + sx) as usize).copied().unwrap_or_default()
    }

    fn owner_index(c: Y16X16) -> usize {
        ((c.y() as usize >> 4) + 1) * 64 + (c.x() as usize >> 4)
    }

    /// 0x5b00a0 `(centre, owner)`, from `PushTile` (0x5b2440 / 0x5b6ff0) for an owned tile: when
    /// the owner's bit is new in the square's byte 0, it is set there and in byte 1 of the
    /// square and its eight neighbours (columns 0..=63 only, rows into the margins).
    fn owner_map_add(&mut self, c: Y16X16, owner: u8) {
        if owner == 0 || self.owner_map.is_empty() {
            return;
        }
        let bit = ((1u32 << owner) >> 1) as u8;
        let i = Self::owner_index(c);
        if self.owner_map[i][0] & bit != 0 {
            return;
        }
        self.owner_map[i][0] |= bit;
        let cx = c.x() as usize >> 4;
        for row in [i - 64, i, i + 64] {
            self.owner_map[row][1] |= bit;
            if cx != 0 {
                self.owner_map[row - 1][1] |= bit;
            }
            if cx < 0x3f {
                self.owner_map[row + 1][1] |= bit;
            }
        }
    }

    /// 0x5b0130 `(centre, owner)`, from `DeleteTile` (0x5b24f0) after the tile left its square
    /// list: unless another tile of the square has the owner's type bit (`16 << owner`), the
    /// owner's bit goes from byte 0 (byte 1 keeps it).
    fn owner_map_remove(&mut self, c: Y16X16, owner: u8, side: usize) {
        if owner == 0 || self.owner_map.is_empty() {
            return;
        }
        let sq = (c.y() as usize >> 4) * side + (c.x() as usize >> 4);
        let mut id = self.square_lists[sq].0;
        while id != 0 {
            if self.tiles[id as usize].ty & (16 << owner) != 0 {
                return;
            }
            id = self.tiles[id as usize].sq_next;
        }
        let bit = ((1u32 << owner) >> 1) as u8;
        self.owner_map[Self::owner_index(c)][0] &= !bit;
    }

    /// 0x5aff50 (at load, 0x5b7400): byte 0 of every square from its tile list, then byte 1 from
    /// the 3×3 squares around (columns 0..=63, rows into the margins); all else cleared.
    pub fn calc_owner_map(&mut self, side: usize) {
        self.owner_map = vec![[0; 2]; OWNER_MAP_ROWS * 64];
        for sy in 0..side {
            for sx in 0..side {
                let mut bits = 0u8;
                let mut id = self.square_lists[sy * side + sx].0;
                while id != 0 {
                    bits |= ((1u32 << self.tiles[id as usize].owner()) >> 1) as u8;
                    id = self.tiles[id as usize].sq_next;
                }
                self.owner_map[(sy + 1) * 64 + sx][0] = bits;
            }
        }
        for sy in 0..side {
            for sx in 0..side {
                let i = (sy + 1) * 64 + sx;
                let mut b = 0;
                for row in [i - 64, i, i + 64] {
                    b |= self.owner_map[row][0];
                    if sx > 0 {
                        b |= self.owner_map[row - 1][0];
                    }
                    if sx < 0x3f {
                        b |= self.owner_map[row + 1][0];
                    }
                }
                self.owner_map[i][1] = b;
            }
        }
    }

    /// `CTiling` vtbl+0x3c (0x5b7d40): the nodes of `owner` (0 beyond 8).
    pub fn nodes_of(&self, owner: u8) -> i32 {
        self.owner_nodes.get(owner as usize).copied().unwrap_or(0)
    }

    /// `CTiling` vtbl+0x34 (0x5b7d20): the map's nodes less the water nodes counted at `Init`.
    pub fn land_nodes(&self) -> i32 {
        (self.normal.len() as i32).wrapping_sub(self.water_nodes)
    }

    /// `CTilesEx::InsertTileIntoSquareList`.
    fn insert_square_list(&mut self, id: u16, side: usize) {
        let c = self.tiles[id as usize].center;
        let sq = (c.y() as usize >> 4) * side + (c.x() as usize >> 4);
        let (first, last) = self.square_lists[sq];
        if first == 0 {
            self.square_lists[sq] = (id, id);
            self.tiles[id as usize].sq_prev = 0;
            self.tiles[id as usize].sq_next = 0;
        } else if self.tiles[id as usize].catapult() {
            self.tiles[last as usize].sq_next = id;
            self.tiles[id as usize].sq_prev = last;
            self.tiles[id as usize].sq_next = 0;
            self.square_lists[sq].1 = id;
        } else {
            self.tiles[first as usize].sq_prev = id;
            self.tiles[id as usize].sq_prev = 0;
            self.tiles[id as usize].sq_next = first;
            self.square_lists[sq].0 = id;
        }
    }

    /// `CTilesEx::DeleteTileFromSquareList`.
    fn delete_square_list(&mut self, id: u16, side: usize) {
        let (c, prev, next) = {
            let t = &self.tiles[id as usize];
            (t.center, t.sq_prev, t.sq_next)
        };
        let sq = (c.y() as usize >> 4) * side + (c.x() as usize >> 4);
        if prev != 0 {
            self.tiles[prev as usize].sq_next = next;
        } else {
            self.square_lists[sq].0 = next;
        }
        if next != 0 {
            self.tiles[next as usize].sq_prev = prev;
        } else {
            self.square_lists[sq].1 = prev;
        }
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u16s(&self.normal);
        h.u16s(&self.catapult);
        h.u16s(&self.water_flags);
        h.u16(self.last_used);
        // A `Tiling::new` that was never built has no tile table.
        for t in self.tiles.iter().take(self.last_used as usize + 1) {
            h.u32(t.ty);
            h.u32(t.center.0);
            h.u16(t.sector);
            h.u16(t.eco_sector);
            h.u16s(&t.links);
        }
        h.u16(self.sector_cursor);
        h.u32(self.sector_count);
        h.u16(self.last_eco);
        h.u16(self.eco_min_free);
        h.u32(self.free.len() as u32);
        for &f in self.free.iter().take(16) {
            h.u16(f);
        }
        for b in &self.owner_map {
            h.u8(b[0]);
            h.u8(b[1]);
        }
        self.owner_nodes.iter().for_each(|&n| h.u32(n as u32));
        h.u32(self.water_nodes as u32);
    }
}

/// Inputs `CTiling::Init` reads and writes.
pub struct InitLayers<'a> {
    pub grid: Grid,
    pub ground: &'a [u8],
    pub flags: &'a mut [u8],
    pub gfx_bits: &'a mut [u8],
    /// Owner per node (what `StartUpOwner` wrote as pseudo tile ids).
    pub owner: &'a [u8],
}

/// `CTiling::Init`: rebuild everything from the node layers.
pub fn init(l: InitLayers) -> Tiling {
    init_with(l, None)
}

/// [`init`] with given water flags instead of `CalcWaterFlags` (the game computes water flags only
/// in `Init`; tests compare incremental updates against a rebuild that keeps them).
pub fn init_with(l: InitLayers, water_flags: Option<&[u16]>) -> Tiling {
    let InitLayers { grid, ground, flags, gfx_bits, owner } = l;
    let w = grid.size() as i32;
    let mut t = Tiling::new(grid);
    for (i, &o) in owner.iter().enumerate() {
        t.normal[i] = o as u16; // StartUpOwner(i, OwnerPseudoTileId(owner))
    }
    // Water and the map border are blocked land for the tiling.
    for (i, &g) in ground.iter().enumerate() {
        if g <= 8 {
            flags[i] |= flag::BLOCKED_LAND;
        }
    }
    for k in 0..w {
        for (x, y) in [(k, 0), (k, w - 1), (0, k), (w - 1, k)] {
            flags[grid.index(x as u32, y as u32)] |= flag::BLOCKED_LAND;
        }
    }
    // TODO(port): g_pFogging init.
    crate::debug::load_step("t: water flags");
    match water_flags {
        Some(wf) => t.water_flags.copy_from_slice(wf),
        None => calc_water_flags(grid, ground, flags, &mut t.water_flags),
    }
    crate::debug::load_step("t: init_tiles");
    init_tiles(&mut t);
    crate::debug::load_step("t: squares");
    let hex15 = hex_points(15);
    // Each square's type maps and components read only its own nodes (the owner pseudo tiles
    // `StartUpOwner` wrote, the flags), which no square's tiles change: they are worked out first,
    // on several threads, and the tiles then made one square after the other in the original
    // order (the ids are handed out in that order, and a tile centre looks into the neighbours).
    let origins: Vec<(i32, i32)> = (0..w).step_by(16).flat_map(|sy| (0..w).step_by(16).map(move |sx| (sx, sy))).collect();
    let squares = square_components(&t, grid, ground, flags, &origins);
    for (mut normal, catapult) in squares {
        normal.make_tiles(&mut t, grid, false, &hex15);
        t.water_nodes += normal.water as i32;
        if let Some(mut c) = catapult {
            c.make_tiles(&mut t, grid, true, &hex15);
        }
    }
    // CTilesEx::DeactivateTilesPushBackMode: the free list continues after the last pushed tile;
    // CalculateSquareTileLists.
    crate::debug::load_step("t: lists");
    t.num_used = (t.last_used - 9) as u32;
    t.free = (t.last_used + 1..SENTINEL).collect();
    let side = (w / 16) as usize;
    for id in FIRST_REAL..=t.last_used {
        if t.tiles[id as usize].used() {
            t.insert_square_list(id, side);
        }
    }
    crate::debug::load_step("t: owner map");
    t.calc_owner_map(side);
    crate::debug::load_step("t: links");
    calc_links(&mut t, grid);
    crate::debug::load_step("t: sectors");
    calc_sectors(&mut t);
    crate::debug::load_step("t: sizes + borderstones");
    // CalcNotifyAll: sector sizes (eco-sector bookkeeping lives in CEcoSectorMgr, TODO(port)).
    for i in 0..grid.len() {
        let tile = &t.tiles[t.normal[i] as usize];
        let (s, e) = (tile.sector, tile.eco_sector);
        if s != 0 {
            t.sector_sizes[s as usize] += 1;
            if e != 0 {
                t.eco_sizes[e as usize] += 1;
            }
        }
    }
    borderstones(&t, grid, ground, gfx_bits);
    for i in 0..grid.len() {
        let tile = &t.tiles[t.normal[i] as usize];
        if tile.ty & (1 << 28) != 0 {
            t.owner_nodes[tile.owner() as usize] += 1;
        }
    }
    t
}

/// `f(row)` for rows `0..rows`, in order, on up to as many threads as the machine has (8 at most).
fn par_rows<T: Send>(rows: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
    if threads < 2 || rows < 64 {
        return (0..rows).map(&f).collect();
    }
    let chunk = rows.div_ceil(threads);
    let f = &f;
    std::thread::scope(|s| {
        let parts: Vec<_> = (0..rows).step_by(chunk).map(|a| s.spawn(move || (a..(a + chunk).min(rows)).map(f).collect::<Vec<T>>())).collect();
        parts.into_iter().flat_map(|h| h.join().expect("rows")).collect()
    })
}

/// The normal and (unless the square is all water) the catapult components of each square at
/// `origins`, in that order, worked out on up to as many threads as the machine has.
fn square_components(t: &Tiling, grid: Grid, ground: &[u8], flags: &[u8], origins: &[(i32, i32)]) -> Vec<(Square, Option<Square>)> {
    let one = |&(sx, sy): &(i32, i32)| {
        let mut normal = Square::default();
        normal.fill_normal(t, grid, ground, flags, sx, sy);
        normal.components();
        let catapult = (normal.water != 256).then(|| {
            let mut c = Square::default();
            c.fill_catapult(grid, flags, sx, sy);
            c.components();
            c
        });
        (normal, catapult)
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8);
    if threads < 2 || origins.len() < 64 {
        return origins.iter().map(one).collect();
    }
    let chunk = origins.len().div_ceil(threads);
    std::thread::scope(|s| {
        let parts: Vec<_> = origins.chunks(chunk).map(|c| s.spawn(move || c.iter().map(one).collect::<Vec<_>>())).collect();
        parts.into_iter().flat_map(|h| h.join().expect("square components")).collect()
    })
}

/// `CTilesEx::InitTiles`.
fn init_tiles(t: &mut Tiling) {
    t.tiles = vec![Tile::unused(); 0x8000];
    for o in 0..=8u8 {
        t.tiles[o as usize] = Tile { ty: owner_tile_type(o), ..Tile::unused() };
    }
    t.tiles[9] = Tile { ty: 0xFFFF_FFFD, ..Tile::unused() };
    t.tiles[SENTINEL as usize] = Tile { ty: 0xFFFF_FFFE, ..Tile::unused() };
    t.last_used = 9;
}

/// `CWaterFlagsEx::CalcWaterFlags` (release 0x5bbad0): classify water nodes for ships and walkers.
fn calc_water_flags(grid: Grid, ground: &[u8], flags: &[u8], out: &mut [u16]) {
    let w = grid.size() as usize;
    let pw = w + 4;
    let hex = hex_points(4);
    let rel: Vec<isize> = hex.iter().take(61).map(|&(x, y)| y as isize * pw as isize + x as isize).collect();
    let mut b = vec![0x81u8; pw * pw];
    for y in 0..w {
        for x in 0..w {
            let i = y * w + x;
            b[(y + 2) * pw + x + 2] = if ground[i] <= 8 {
                if flags[i] & flag::BLOCKED_OBJECT != 0 { 0xC5 } else { 0x81 }
            } else if flags[i] & flag::BLOCKED_LAND != 0 {
                0xC2
            } else {
                0x42
            };
        }
    }
    let at = |p: usize, r: isize| (p as isize + r) as usize;
    // Neither pass reads the bits it sets (0x48 / 0x50 against the masks 6 / 0xE and the node's
    // own 0xE), so each pass's decisions come from the map as it was before it: worked out by
    // rows on several threads, then applied.
    for (mask, set) in [(6u8, 0x48u8), (0xE, 0x50)] {
        let bm = &b;
        let hits = par_rows(w, |y| {
            (0..w)
                .filter(|&x| {
                    let p = (y + 2) * pw + x + 2;
                    bm[p] & 0xE == 0 && rel[1..19].iter().any(|&r| bm[at(p, r)] & mask != 0)
                })
                .map(|x| (y + 2) * pw + x + 2)
                .collect::<Vec<usize>>()
        });
        for p in hits.into_iter().flatten() {
            b[p] |= set;
        }
    }
    let block = |v: u8| if v & 1 != 0 { 0xC5 } else { 0xC2 };
    for row in [2, 3, pw - 4, pw - 3] {
        for x in 0..pw {
            b[row * pw + x] = block(b[row * pw + x]);
        }
    }
    for col in [2, 3, pw - 4, pw - 3] {
        for y in 0..pw {
            b[y * pw + col] = block(b[y * pw + col]);
        }
    }
    for y in 0..w {
        for x in 0..w {
            let p = (y + 2) * pw + x + 2;
            let v = b[p];
            if v & 1 == 0 {
                continue;
            }
            let f = if v & 0x40 == 0 {
                0xC000
            } else if v & 4 != 0 {
                0xC300
            } else if v & 0x18 != 0 && rel[1..61].iter().fold(0xFFu8, |a, &r| a & b[at(p, r)]) & 0xC0 == 0 {
                0xC500
            } else {
                0xC100
            };
            out[y * w + x] = f;
        }
    }
}

/// `CSquare`: per-square scratch for the component labelling.
struct Square {
    x: i32,
    y: i32,
    water: u32,
    /// Cell types (-1 blocked), `dword_42F7C68`.
    ty: [i64; 256],
    /// Cell → area, `dword_42F8068`.
    area: [i32; 256],
    /// Area → component, area → type.
    area_comp: Vec<i32>,
    area_ty: Vec<i64>,
    /// Component → member areas (empty = merged away).
    comps: Vec<Vec<i32>>,
}

impl Default for Square {
    fn default() -> Self {
        Square { x: 0, y: 0, water: 0, ty: [-1; 256], area: [-1; 256], area_comp: Vec::new(), area_ty: Vec::new(), comps: Vec::new() }
    }
}

const fn sidx(x: usize, y: usize) -> usize {
    x + 16 * y
}

impl Square {
    fn reset(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.water = 0;
        self.area_comp.clear();
        self.area_ty.clear();
        self.comps.clear();
    }

    /// `CSquare::InitAndFillTypeMapNormal`.
    fn fill_normal(&mut self, t: &Tiling, grid: Grid, ground: &[u8], flags: &[u8], x0: i32, y0: i32) {
        self.reset(x0, y0);
        let parity = square_parity(x0, y0);
        for j in 0..16 {
            for i in 0..16 {
                let n = grid.index((x0 + i) as u32, (y0 + j) as u32);
                let owner = t.tiles[t.normal[n] as usize].owner();
                let mut v = parity | ground_tile_type(ground[n]) | owner_tile_type(owner);
                let blocked = if v & TILE_LAND != 0 {
                    flags[n] & flag::BLOCKED_LAND != 0
                } else {
                    self.water += 1;
                    let wf = t.water_flags[n];
                    if wf & 0x400 != 0 {
                        v = (v & !TILE_WATER) | TILE_WALKABLE_WATER;
                        false
                    } else {
                        wf & 0x300 != 0
                    }
                };
                self.ty[sidx(i as usize, j as usize)] = if blocked { -1 } else { v as i64 };
            }
        }
    }

    /// `CSquare::InitAndFillTypeMapCatapult`: free = no blocked node within hex radius 2.
    fn fill_catapult(&mut self, grid: Grid, flags: &[u8], x0: i32, y0: i32) {
        self.reset(x0, y0);
        let ty = (square_parity(x0, y0) | TILE_CATAPULT) as i64;
        let near: Vec<(i32, i32)> = hex_points(2);
        for j in 0..16 {
            for i in 0..16 {
                let (x, y) = (x0 + i, y0 + j);
                let blocked = |px: i32, py: i32| {
                    grid.checked_index(px, py).is_none_or(|n| flags[n] & flag::BLOCKED_LAND != 0)
                };
                let v = if near.iter().any(|&(dx, dy)| blocked(x + dx, y + dy)) { -1 } else { ty };
                self.ty[sidx(i as usize, j as usize)] = v;
            }
        }
    }

    fn new_area(&mut self, ty: i64) -> i32 {
        self.area_ty.push(ty);
        self.area_comp.push(-1);
        self.area_ty.len() as i32 - 1
    }

    fn add_component(&mut self, a: i32) {
        self.area_comp[a as usize] = self.comps.len() as i32;
        self.comps.push(vec![a]);
    }

    fn add_component_if_necessary(&mut self, a: i32) {
        if self.area_comp[a as usize] < 0 {
            self.add_component(a);
        }
    }

    /// `CSquare::ConnectAreas(a, b)`.
    fn connect(&mut self, a: i32, b: i32) {
        let ca = self.area_comp[a as usize];
        let cb = self.area_comp[b as usize];
        if ca < 0 {
            self.area_comp[a as usize] = cb;
            self.comps[cb as usize].push(a);
        } else if ca != cb {
            let (lo, hi) = if ca < cb { (ca, cb) } else { (cb, ca) };
            let moved = std::mem::take(&mut self.comps[hi as usize]);
            for &m in &moved {
                self.area_comp[m as usize] = lo;
                self.comps[lo as usize].push(m);
            }
            if hi as usize == self.comps.len() - 1 {
                self.comps.pop();
            }
        }
    }

    /// `CTiling::CalcSquareComponents` (release 0x5b44a0): hex-connected labelling, neighbours left,
    /// up and up-left.
    fn components(&mut self) {
        let ty = self.ty;
        let mut cur = -1i32;
        for i in 0..16 {
            let v = ty[sidx(i, 0)];
            if v >= 0 {
                if cur < 0 {
                    cur = self.new_area(v);
                } else if v != ty[sidx(i - 1, 0)] {
                    self.add_component(cur);
                    cur = self.new_area(v);
                }
                self.area[sidx(i, 0)] = cur;
            } else {
                if cur >= 0 {
                    self.add_component(cur);
                    cur = -1;
                }
                self.area[sidx(i, 0)] = -1;
            }
        }
        if cur >= 0 {
            self.add_component(cur);
        }
        for j in 1..16 {
            let v = ty[sidx(0, j)];
            if v >= 0 {
                cur = if v == ty[sidx(0, j - 1)] { self.area[sidx(0, j - 1)] } else { self.new_area(v) };
            } else {
                cur = -1;
            }
            self.area[sidx(0, j)] = cur;
            for k in 1..16 {
                let v = ty[sidx(k, j)];
                if v >= 0 {
                    let a_ul = self.area[sidx(k - 1, j - 1)];
                    let a_u = self.area[sidx(k, j - 1)];
                    let t_ul = ty[sidx(k - 1, j - 1)];
                    let t_u = ty[sidx(k, j - 1)];
                    if cur >= 0 {
                        if v != ty[sidx(k - 1, j)] {
                            self.add_component_if_necessary(cur);
                            cur = self.new_area(v);
                        }
                        if cur != a_ul && v == t_ul {
                            self.connect(cur, a_ul);
                        } else if cur != a_u && v == t_u {
                            self.connect(cur, a_u);
                        }
                    } else if v == t_ul {
                        cur = a_ul;
                    } else if v == t_u {
                        cur = a_u;
                    } else {
                        cur = self.new_area(v);
                    }
                    self.area[sidx(k, j)] = cur;
                } else {
                    if cur >= 0 {
                        self.add_component_if_necessary(cur);
                        cur = -1;
                    }
                    self.area[sidx(k, j)] = -1;
                }
            }
            if cur >= 0 {
                self.add_component_if_necessary(cur);
            }
        }
    }

    /// `CTiling::CalcSquareTiles(type)` (release 0x5b47d0).
    fn make_tiles(&mut self, t: &mut Tiling, grid: Grid, catapult: bool, hex15: &[(i32, i32)]) {
        // CSquare::CalcComponentTileIndices + CTilesEx::PushTilesBackUndef.
        let base = t.last_used + 1;
        let mut comp_tile = vec![0u16; self.comps.len()];
        let mut n = 0;
        for (c, members) in self.comps.iter().enumerate() {
            if !members.is_empty() {
                comp_tile[c] = base + n;
                n += 1;
            }
        }
        t.last_used += n;
        let mut sums = vec![(0i32, 0i32, 0i32); self.comps.len()];
        for j in 0..16 {
            for i in 0..16 {
                let a = self.area[sidx(i, j)];
                if a >= 0 && self.ty[sidx(i, j)] >= 0 {
                    let c = self.area_comp[a as usize] as usize;
                    let node = grid.index((self.x + i as i32) as u32, (self.y + j as i32) as u32);
                    let ids = if catapult { &mut t.catapult } else { &mut t.normal };
                    ids[node] = comp_tile[c];
                    sums[c].0 += i as i32;
                    sums[c].1 += j as i32;
                    sums[c].2 += 1;
                }
            }
        }
        for (c, members) in self.comps.iter().enumerate() {
            if members.is_empty() {
                continue;
            }
            let (sx, sy, cnt) = sums[c];
            let id = comp_tile[c];
            let ty = self.area_ty[members[0] as usize] as u32;
            let (cx, cy) = (self.x + sx / cnt, self.y + sy / cnt);
            let ids = if catapult { &t.catapult } else { &t.normal };
            let center = nearest_element(grid, ids, cx, cy, id, hex15);
            let mut tile = Tile { ty: ty & 0xFFC0_1FFF, center, ..Tile::unused() };
            tile.set_size(cnt);
            t.tiles[id as usize] = tile;
        }
    }
}

/// `CTiling::SearchForNearestTileElement`: the centroid if it belongs to the tile, else the first
/// hex-ring point (radius ≤ 15) that does.
fn nearest_element(grid: Grid, ids: &[u16], x: i32, y: i32, tile: u16, hex15: &[(i32, i32)]) -> Y16X16 {
    for &(dx, dy) in hex15 {
        if let Some(n) = grid.checked_index(x + dx, y + dy)
            && ids[n] == tile
        {
            return Y16X16::pack(x + dx, y + dy);
        }
    }
    Y16X16::pack(x, y) // "SearchForNearestTileElement failed!"
}

/// `CTiling::CalcAllTileLinks` (release 0x5b4b60): for every node (not the last row/column) and its
/// neighbours (1,0), (1,1), (0,1): link differing real tiles, both directions, first seen first.
fn calc_links(t: &mut Tiling, grid: Grid) {
    let w = grid.size() as usize;
    let rel = [1usize, w + 1, w];
    let mut lists: Vec<Vec<u16>> = vec![Vec::new(); t.last_used as usize + 1];
    for y in 0..w - 1 {
        for x in 0..w - 1 {
            let n = y * w + x;
            for ids in [&t.normal, &t.catapult] {
                let a = ids[n];
                if !t.is_valid_real(a) {
                    continue;
                }
                for &r in &rel {
                    let b = ids[n + r];
                    if b != a && t.is_valid_real(b) && !lists[a as usize].contains(&b) {
                        lists[a as usize].push(b);
                        lists[b as usize].push(a);
                    }
                }
            }
        }
    }
    for (id, l) in lists.into_iter().enumerate().skip(FIRST_REAL as usize) {
        t.tiles[id].links = l;
    }
}

/// `CTiling::CalcSectorIds` (release 0x5b4f90) with sequential eco-sector ids (the load path; a new game
/// takes them from `CEcoSectorMgr::NewEcoSector`, whose free slots start at 1 too — inferred).
fn calc_sectors(t: &mut Tiling) {
    t.sector_used.fill(0);
    t.sector_used[0] = 1;
    t.sector_cursor = 1;
    t.sector_count = 0;
    t.sector_sizes.fill(0);
    t.eco_sizes.fill(-1);
    t.eco_used.fill(false);
    t.eco_min_free = 1;
    t.last_eco = 0;
    let last = t.last_used as usize;
    for tile in &mut t.tiles[FIRST_REAL as usize..=last] {
        let v = if tile.used() { 0 } else { 0xFFFF };
        tile.sector = v;
        tile.eco_sector = v;
    }
    for j in FIRST_REAL as usize..=last {
        let tile = &t.tiles[j];
        if tile.sector == 0 && tile.ty & TILE_WALKABLE_WATER == 0 {
            // NotifyCreateCatapultSector / NotifyCreateSector → CSectors::NewSector.
            let id = t.create_sector();
            flood(t, j as u16, |tl| tl.sector, |tl, v| tl.sector = v, id, true);
        }
    }
    for k in FIRST_REAL as usize..=last {
        let tile = &t.tiles[k];
        if tile.eco_sector == 0 && tile.owner() != 0 {
            let id = t.create_eco(tile.owner());
            t.last_eco += 1;
            flood(t, k as u16, |tl| tl.eco_sector, |tl, v| tl.eco_sector = v, id, false);
        }
    }
}

/// `WalkTilesChangeSector` (link to tiles sharing a bit of `type & 0x3C000000`) or
/// `WalkTilesChangeEcoSector` (link to tiles of the same owner): breadth first from `start`.
fn flood(t: &mut Tiling, start: u16, get: fn(&Tile) -> u16, set: fn(&mut Tile, u16), new: u16, sector: bool) {
    let old = get(&t.tiles[start as usize]);
    let key = if sector { t.tiles[start as usize].ty & 0x3C00_0000 } else { t.tiles[start as usize].owner() as u32 };
    let mut q = std::collections::VecDeque::from([start]);
    while let Some(id) = q.pop_front() {
        if get(&t.tiles[id as usize]) != old {
            continue;
        }
        set(&mut t.tiles[id as usize], new);
        for i in 0..t.tiles[id as usize].links.len() {
            let n = t.tiles[id as usize].links[i];
            let nt = &t.tiles[n as usize];
            let ok = if sector { key & nt.ty != 0 } else { nt.owner() as u32 == key };
            if ok {
                q.push_back(n);
            }
        }
    }
}

/// `COwnerMapEx::CalculateOwnerBits` + `CTiling::CalculateBorderstoneBits`: in every square that
/// holds an owned real tile, gfx bit 0x80 marks owned inner nodes with a differently owned
/// land neighbour (`CalculateBorderstoneBit`); squares without owned tiles are left alone.
/// `CTiling::CalculateBorderstoneBit` (release 0x5b37b0).
fn borderstone_bit(t: &Tiling, grid: Grid, ground: &[u8], gfx_bits: &mut [u8], x: i32, y: i32) {
    let n = grid.index(x as u32, y as u32);
    let sorted = [(-1, -1), (0, -1), (-1, 0), (1, 0), (0, 1), (1, 1)];
    let mut set = false;
    if grid.in_inner_world(x, y, 1) {
        let o = t.owner_of(n);
        if o > 0 {
            set = sorted.iter().any(|&(dx, dy)| {
                let m = grid.index((x + dx) as u32, (y + dy) as u32);
                t.owner_of(m) != o && ground[m] > 8
            });
        }
    }
    if set {
        gfx_bits[n] |= 0x80;
    } else {
        gfx_bits[n] &= !0x80;
    }
}

fn borderstones(t: &Tiling, grid: Grid, ground: &[u8], gfx_bits: &mut [u8]) {
    let w = grid.size() as i32;
    for sy in (0..w).step_by(16) {
        for sx in (0..w).step_by(16) {
            let owned = (0..16).any(|j| {
                (0..16).any(|i| {
                    let id = t.normal[grid.index((sx + i) as u32, (sy + j) as u32)];
                    id >= FIRST_REAL && t.tiles[id as usize].owner() != 0
                })
            });
            if !owned {
                continue;
            }
            for y in sy..sy + 16 {
                for x in sx..sx + 16 {
                    borderstone_bit(t, grid, ground, gfx_bits, x, y);
                }
            }
        }
    }
}
