//! Landscape textures (`Gfx\2.gh5/.gh6/.gl5/.gl6`, `Gfx\41.*`) and the pure-data half of the
//! terrain renderer: ground-type → texture-piece lookup, per-triangle UVs, vertex shading and the
//! height → pixel offset. The rasteriser itself (draw loop, triangle fill) is described in
//! `docs/TERRAIN.md`; an example implementation is in `examples/landscape_dump.rs`.
//!
//! Function addresses below are from the decompiled *debug* build (`generated\...`); data tables
//! were read from the shipped *release* `S4_Main.exe` (image base 0x400000) and are quoted with
//! their release VA. Where the release code was disassembled to confirm the debug text, the
//! release VA is given as `rel 0x...`.
//!
//! ## File choice (`CGfxManager::Reload` 0x1363b20, `CGfxManager::EnableGfxFile` 0x13637e0)
//! - `2` = classic textures, `41` = "new textures" (`g_bNewTextures`, History Edition option;
//!   `CGfxManager+0xCBE`).
//! - `gh*` = HQ texture set (`CInterfaceD3D::IsHQTextureSet`, 256×256 pages), `gl*` = LQ (128×128).
//! - `*5` = pixel format RGB555, `*6` = RGB565 (`CGfxManager+0xC98` = `!(surface mode == 1)`).
//!   `.x5` and `.x6` hold the same picture in the two packings (decoded to 8 bit they differ by
//!   at most 8 per channel, i.e. quantisation; same layout and size).
//!
//! ## Container (`ReadTextureBitmapSet` 0x2f606c0, `ReadTextureBitmap` 0x2f5f510)
//! ```text
//! 0x00  20-byte blob header (u32 0x8000, 3 × u32 0, u32 1) — skipped (`Reload` returns base+20)
//! 0x14  record*: u16 flags, u8 version, u8 count, u32 size, size bytes   (size 0 = "rest of file")
//!         flags bit1 (0x2) = paletted "software renderer" pages, clear = 16-bit "hardware" pages
//!         flags bit0 (0x1) = HQ (256×256), clear = LQ (128×128)
//!       16-bit record: count × (dim × dim × u16) ; paletted: n × (dim × dim × u8 index + 768 B palette)
//! ```
//! The game requires the first record's `count` byte to be 44 and takes the **first** record
//! whose bit1 matches the renderer (hardware → 16-bit). 44 pages = 22 normal + 22 dark-land
//! pages (`s_iDarkTribeElement` = 22 is added to the page index for dark land).
//! Observed on disk (all 8 files decode):
//! - `2.gh*`: [flags 1: 44 × 256² u16] [flags 3: 44 × (256² + 768)]
//! - `2.gl*`, `41.gl*`: [flags 0: 44 × 128² u16] [flags 2: 44 × (128² + 768)]
//! - `41.gh*`: [flags 1: 44 × 256² u16] [flags 3, count byte 22, but size = 44 × (256² + 768)]
//!   [flags 1, version 2, count 22: 22 × 256² u16]. The third record is never selected by
//!   `ReadTextureBitmapSet` (the first flags-1 record wins); it is byte-identical to pages
//!   22..44 of record 0, which in turn are byte-identical to `2.gh6`'s dark pages — `41` only
//!   replaces the 22 normal pages.
//!
//! The software pages are read with a fixed stride of 66304 = 256² + 768 regardless of the
//! file, so the game only ever uses them from a `gh` file (software rendering forces HQ).
//!
//! ## Rendering summary (details + draw loop in `docs/TERRAIN.md`)
//! Node (x, y) sits at screen `(W·x − W/2·y, H·y − hoff(h))`, W = triangle size (24 px at zoom
//! 1), H = W/2 in the game's camera mode 1. Each node owns two triangles:
//! `Up` = A(x,y) B(x,y+1) C(x+1,y+1) and `Down` = A(x,y) D(x+1,y) C(x+1,y+1).
//! [`column_pieces`] picks each triangle's texture page + UVs, [`shade_index`] /
//! [`shade_colors`] give the Gouraud diffuse/specular colours, [`height_offset_fixed`] the
//! vertical displacement.

use crate::reader::Reader;
use crate::{Result, bail};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------
// Texture files
// ---------------------------------------------------------------------------------------------

/// Which texture family (`CGfxManager::Reload`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureSet {
    /// `Gfx\2.*` — original textures.
    Classic = 2,
    /// `Gfx\41.*` — History Edition "new textures".
    New = 41,
}

/// 16-bit packing of the "hardware" pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rgb16 {
    /// `.gh5` / `.gl5`: `0RRRRRGG GGGBBBBB` (bit 15 is 0 in every shipped pixel).
    Rgb555,
    /// `.gh6` / `.gl6`: `RRRRRGGG GGGBBBBB`.
    Rgb565,
}

impl Rgb16 {
    /// Expand to 8-bit RGB by bit replication.
    pub fn to_rgb(self, p: u16) -> [u8; 3] {
        let e5 = |v: u16| ((v << 3) | (v >> 2)) as u8;
        match self {
            Rgb16::Rgb555 => [e5((p >> 10) & 31), e5((p >> 5) & 31), e5(p & 31)],
            Rgb16::Rgb565 => {
                let g = (p >> 5) & 63;
                [e5(p >> 11), ((g << 2) | (g >> 4)) as u8, e5(p & 31)]
            }
        }
    }
}

/// Path of a landscape texture file, e.g. `Gfx\2.gh6`.
pub fn texture_path(game_dir: &Path, set: TextureSet, hq: bool, fmt: Rgb16) -> PathBuf {
    let ext = match (hq, fmt) {
        (true, Rgb16::Rgb555) => "gh5",
        (true, Rgb16::Rgb565) => "gh6",
        (false, Rgb16::Rgb555) => "gl5",
        (false, Rgb16::Rgb565) => "gl6",
    };
    game_dir.join("Gfx").join(format!("{}.{ext}", set as u32))
}

/// One square texture page decoded to RGBA (alpha always 255).
#[derive(Clone)]
pub struct TexturePage {
    pub size: u32,
    pub rgba: Vec<u8>,
}

impl TexturePage {
    /// Point-sample at texel (u, v) in `0..size`, wrapping (the device uses `D3DTADDRESS_WRAP`).
    pub fn texel(&self, x: i32, y: i32) -> [u8; 4] {
        let s = self.size as i32;
        let i = ((y.rem_euclid(s) * s + x.rem_euclid(s)) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

/// One record of a texture file.
#[derive(Clone)]
pub struct TextureRecord {
    pub flags: u16,
    pub version: u8,
    /// The record's count byte as stored (not always the real page count, see module doc).
    pub count_byte: u8,
    /// File offset of the record header.
    pub offset: usize,
    pub pages: Vec<TexturePage>,
    /// Per-page 256-colour palettes (paletted records only), RGB.
    pub palettes: Vec<Vec<[u8; 3]>>,
}

impl TextureRecord {
    pub fn is_paletted(&self) -> bool {
        self.flags & 2 != 0
    }
    pub fn is_hq(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// A whole `2.gh5`-style file.
pub struct LandscapeTextures {
    pub records: Vec<TextureRecord>,
}

impl LandscapeTextures {
    pub fn open(path: &Path, fmt: Rgb16) -> Result<Self> {
        Self::parse(&std::fs::read(crate::path::resolve(path))?, fmt)
    }

    /// Parse every record. `fmt` selects the 16-bit packing (from the extension).
    pub fn parse(data: &[u8], fmt: Rgb16) -> Result<Self> {
        let mut r = Reader::new(data, "landscape textures");
        if r.u32()? != 0x8000 {
            bail!("landscape textures: bad magic");
        }
        r.seek(20);
        let mut records = Vec::new();
        while r.remaining() >= 8 {
            let offset = r.pos();
            let flags = r.u16()?;
            let version = r.u8()?;
            let count_byte = r.u8()?;
            let mut size = r.u32()? as usize;
            if size == 0 {
                size = r.remaining();
            }
            let body = r.bytes(size)?;
            let dim = if flags & 1 != 0 { 256 } else { 128 };
            let px = dim * dim;
            let mut rec = TextureRecord {
                flags,
                version,
                count_byte,
                offset,
                pages: Vec::new(),
                palettes: Vec::new(),
            };
            if flags & 2 == 0 {
                if !size.is_multiple_of(px * 2) {
                    bail!("landscape textures: record @{offset:#x} size {size} not a multiple of {}", px * 2);
                }
                for chunk in body.chunks_exact(px * 2) {
                    let mut rgba = Vec::with_capacity(px * 4);
                    for p in chunk.as_chunks::<2>().0 {
                        let [r8, g8, b8] = fmt.to_rgb(u16::from_le_bytes(*p));
                        rgba.extend_from_slice(&[r8, g8, b8, 255]);
                    }
                    rec.pages.push(TexturePage { size: dim as u32, rgba });
                }
            } else {
                let stride = px + 768;
                if !size.is_multiple_of(stride) {
                    bail!("landscape textures: paletted record @{offset:#x} size {size} not a multiple of {stride}");
                }
                for chunk in body.chunks_exact(stride) {
                    let pal: Vec<[u8; 3]> = chunk[px..].as_chunks::<3>().0.to_vec();
                    let mut rgba = Vec::with_capacity(px * 4);
                    for &i in &chunk[..px] {
                        let [r8, g8, b8] = pal[i as usize];
                        rgba.extend_from_slice(&[r8, g8, b8, 255]);
                    }
                    rec.pages.push(TexturePage { size: dim as u32, rgba });
                    rec.palettes.push(pal);
                }
            }
            records.push(rec);
        }
        if records.is_empty() {
            bail!("landscape textures: no records");
        }
        Ok(Self { records })
    }

    /// The record the game's hardware renderer uses: the first 16-bit one
    /// (`ReadTextureBitmapSet` with `a1 = hardware`). Its 44 pages are indexed by
    /// [`Piece::page`].
    pub fn hardware_pages(&self) -> Option<&[TexturePage]> {
        self.records.iter().find(|r| !r.is_paletted()).map(|r| r.pages.as_slice())
    }
}

// ---------------------------------------------------------------------------------------------
// Ground types
// ---------------------------------------------------------------------------------------------

/// Ground (terrain) type ids as stored in byte 1 of the render map element
/// (`T_GFX_MAP_ELEMENT::m_uGroundType`).
///
/// The release exe carries **no** names for these ids. The ids and their relations come from the
/// binary (render tables + transition table below); the names are inferred from that structure
/// together with the map editor's terrain menu (`Editor\LngSat00.dll`: Grass, Desert, Rock,
/// Swamp, Snow, River (Width 1..4), Beach, Water Level 1..8, Dark Grass, Grassy Spot, River
/// Mouth, Dusty Road, Stony Road). E.g. 17/33 are the two halves of the grass↔rock band because
/// the only transition entries for them are 16↔17, 17↔33, 33↔32, all on texture page 9.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ground {
    /// 0..=7: water, 0 = shallowest (borders beach/grass) … 7 = deepest (the only tiled water).
    Water1 = 0,
    Water2 = 1,
    Water3 = 2,
    Water4 = 3,
    Water5 = 4,
    Water6 = 5,
    Water7 = 6,
    Water8 = 7,
    /// Rendered as [`Ground::Beach`] (the type remap table maps 8 → 48).
    BeachAlt = 8,
    Grass = 16,
    /// Grass side of the grass↔rock band.
    GrassToRock = 17,
    /// Tiled on its own page (1), blends with grass on page 3 ("Grassy Spot"?).
    GrassSpot = 18,
    GrassToDesert = 20,
    GrassToSwamp = 21,
    GrassToMud = 23,
    DarkGrass = 24,
    GrassToDarkGrass = 25,
    DustyRoad = 28,
    StonyRoad = 29,
    Rock = 32,
    RockToGrass = 33,
    RockToSnow = 35,
    Beach = 48,
    Desert = 64,
    DesertToGrass = 65,
    Swamp = 80,
    SwampToGrass = 81,
    River1 = 96,
    River2 = 97,
    River3 = 98,
    River4 = 99,
    Snow = 128,
    SnowToRock = 129,
    /// Community name; the editor menu has no matching entry.
    Mud = 144,
    MudToGrass = 145,
}

impl Ground {
    pub const ALL: [Ground; 35] = {
        use Ground::*;
        [
            Water1, Water2, Water3, Water4, Water5, Water6, Water7, Water8, BeachAlt, Grass, GrassToRock,
            GrassSpot, GrassToDesert, GrassToSwamp, GrassToMud, DarkGrass, GrassToDarkGrass, DustyRoad,
            StonyRoad, Rock, RockToGrass, RockToSnow, Beach, Desert, DesertToGrass, Swamp, SwampToGrass,
            River1, River2, River3, River4, Snow, SnowToRock, Mud, MudToGrass,
        ]
    };

    pub fn from_id(id: u8) -> Option<Ground> {
        Self::ALL.iter().copied().find(|g| *g as u8 == id)
    }
}

/// Ground-type remap applied before everything else (`byte_3ACD340`, release VA 0x1026e18):
/// identity except 8 → 48. Verified byte-for-byte against the exe.
pub fn render_type(raw: u8) -> u8 {
    if raw == 8 { 48 } else { raw }
}

/// Transition class of a (remapped) ground type, 0..39 (`byte_3ACD510`, release VA 0x1026d18).
/// Unlisted types are class 0 (= grass's class).
pub const TRANSITION_CLASS: [u8; 256] = [
    9, 10, 11, 12, 13, 14, 15, 16, 29, 0, 0, 0, 0, 0, 0, 0,
    0, 19, 32, 0, 17, 18, 0, 22, 30, 31, 0, 0, 33, 34, 0, 0,
    3, 25, 0, 21, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 23, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    2, 24, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    35, 36, 37, 38, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    4, 26, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    5, 27, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    7, 28, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
];

/// Page of the seamless tiling texture used when all three corners have this type
/// (`byte_3ACD240`, release VA 0x1026f18); −1 = no tiling texture (use the transition table
/// entry `T[c][c]`). Note unlisted types are 0 (they tile with the grass page).
pub const TILE_PAGE: [i8; 256] = [
    -1, -1, -1, -1, -1, -1, -1, 4, 0, 0, 0, 0, 0, 0, 0, 0,
    0, -1, 1, 0, -1, -1, 0, -1, 2, -1, 0, 0, 20, 21, 0, 0,
    8, -1, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    10, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    15, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    -1, -1, -1, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    17, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    13, -1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0
];

/// One entry of the 39×39 transition table (`g_cHeightAndTypeTable + 0x80`, `byte_4697610`;
/// release VA 0x1568380 in .bss).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransitionEntry {
    /// Pattern cell column (0..4) on the page.
    pub cell_x: u8,
    /// Pattern cell row (0..4) on the page.
    pub cell_y: u8,
    /// Texture page 0..22 (before the dark-land +22).
    pub page: u8,
    /// Bit 0 of byte 3: on odd node columns use the cell one row below (`cell_y + 1`).
    /// Byte 3 is 0x81 for land transitions, 0x80 for water/river (no alternation).
    pub alternate: bool,
}

/// `(major class, minor class, [cell_x, cell_y, page, flags])` in the order the release
/// `CHeightAndTypeTable::CHeightAndTypeTable` (rel 0x66ab70) writes them after zeroing the table
/// (it is the same list as the debug constructor 0x2f7c6a0, with the class lookups folded).
const TRANSITION_WRITES: &[(u8, u8, [u8; 4])] = &[
    (0, 32, [3, 2, 3, 0x81]),
    (32, 0, [2, 2, 3, 0x81]),
    (30, 31, [1, 2, 3, 0x81]),
    (31, 30, [0, 2, 3, 0x81]),
    (0, 31, [1, 0, 3, 0x81]),
    (31, 0, [0, 0, 3, 0x81]),
    (0, 33, [1, 0, 19, 0x81]),
    (33, 0, [0, 0, 19, 0x81]),
    (0, 34, [3, 0, 19, 0x81]),
    (34, 0, [2, 0, 19, 0x81]),
    (0, 9, [3, 0, 18, 0x81]),
    (9, 0, [3, 2, 18, 0x81]),
    (9, 38, [0, 0, 5, 0x80]),
    (38, 9, [0, 0, 5, 0x80]),
    (0, 35, [0, 0, 18, 0x81]),
    (35, 0, [0, 2, 18, 0x81]),
    (0, 36, [1, 0, 18, 0x81]),
    (36, 0, [1, 2, 18, 0x81]),
    (0, 37, [2, 0, 18, 0x81]),
    (37, 0, [2, 2, 18, 0x81]),
    (0, 38, [3, 0, 18, 0x81]),
    (38, 0, [3, 2, 18, 0x81]),
    (0, 19, [2, 0, 9, 0x81]),
    (19, 0, [3, 0, 9, 0x81]),
    (19, 25, [0, 2, 9, 0x81]),
    (25, 19, [1, 2, 9, 0x81]),
    (25, 3, [2, 2, 9, 0x81]),
    (3, 25, [3, 2, 9, 0x81]),
    (3, 21, [2, 0, 16, 0x81]),
    (21, 3, [3, 0, 16, 0x81]),
    (21, 27, [0, 2, 16, 0x81]),
    (27, 21, [1, 2, 16, 0x81]),
    (27, 5, [2, 2, 16, 0x81]),
    (5, 27, [3, 2, 16, 0x81]),
    (0, 18, [2, 0, 14, 0x81]),
    (18, 0, [3, 0, 14, 0x81]),
    (18, 24, [0, 2, 14, 0x81]),
    (24, 18, [1, 2, 14, 0x81]),
    (24, 2, [2, 2, 14, 0x81]),
    (2, 24, [3, 2, 14, 0x81]),
    (0, 22, [2, 2, 12, 0x81]),
    (22, 0, [3, 2, 12, 0x81]),
    (22, 28, [0, 2, 12, 0x81]),
    (28, 22, [1, 2, 12, 0x81]),
    (28, 7, [2, 0, 12, 0x81]),
    (7, 28, [3, 0, 12, 0x81]),
    (0, 17, [2, 2, 11, 0x81]),
    (17, 0, [3, 2, 11, 0x81]),
    (17, 23, [0, 2, 11, 0x81]),
    (23, 17, [1, 2, 11, 0x81]),
    (23, 1, [2, 0, 11, 0x81]),
    (1, 23, [3, 0, 11, 0x81]),
    (0, 6, [3, 0, 3, 0x81]),
    (6, 0, [2, 0, 3, 0x81]),
    (6, 9, [0, 2, 5, 0x81]),
    (9, 6, [1, 2, 5, 0x81]),
    (9, 10, [0, 0, 6, 0x80]),
    (10, 9, [0, 1, 6, 0x80]),
    (10, 11, [1, 0, 6, 0x80]),
    (11, 10, [1, 1, 6, 0x80]),
    (11, 12, [2, 0, 6, 0x80]),
    (12, 11, [2, 1, 6, 0x80]),
    (12, 13, [3, 0, 6, 0x80]),
    (13, 12, [3, 1, 6, 0x80]),
    (13, 14, [0, 2, 6, 0x80]),
    (14, 13, [0, 3, 6, 0x80]),
    (14, 15, [1, 2, 6, 0x80]),
    (15, 14, [1, 3, 6, 0x80]),
    (15, 16, [2, 2, 6, 0x80]),
    (16, 15, [2, 3, 6, 0x80]),
    (31, 31, [0, 1, 9, 0x80]),
    (19, 19, [0, 0, 9, 0x80]),
    (25, 25, [1, 0, 9, 0x80]),
    (21, 21, [0, 0, 16, 0x80]),
    (27, 27, [1, 0, 16, 0x80]),
    (18, 18, [0, 0, 14, 0x80]),
    (24, 24, [1, 0, 14, 0x80]),
    (22, 22, [0, 0, 12, 0x80]),
    (28, 28, [1, 0, 12, 0x80]),
    (17, 17, [0, 0, 11, 0x80]),
    (23, 23, [1, 0, 11, 0x80]),
    (9, 9, [0, 0, 5, 0x80]),
    (10, 10, [1, 0, 5, 0x80]),
    (11, 11, [2, 0, 5, 0x80]),
    (12, 12, [3, 0, 5, 0x80]),
    (13, 13, [0, 1, 5, 0x80]),
    (14, 14, [1, 1, 5, 0x80]),
    (15, 15, [2, 1, 5, 0x80]),
    (35, 35, [0, 0, 5, 0x80]),
    (35, 36, [0, 0, 5, 0x80]),
    (35, 37, [0, 0, 5, 0x80]),
    (35, 38, [0, 0, 5, 0x80]),
    (36, 35, [0, 0, 5, 0x80]),
    (36, 36, [0, 0, 5, 0x80]),
    (36, 37, [0, 0, 5, 0x80]),
    (36, 38, [0, 0, 5, 0x80]),
    (37, 35, [0, 0, 5, 0x80]),
    (37, 36, [0, 0, 5, 0x80]),
    (37, 37, [0, 0, 5, 0x80]),
    (37, 38, [0, 0, 5, 0x80]),
    (38, 35, [0, 0, 5, 0x80]),
    (38, 36, [0, 0, 5, 0x80]),
    (38, 37, [0, 0, 5, 0x80]),
    (38, 38, [0, 0, 5, 0x80]),
    (16, 14, [2, 2, 5, 0x80]),
    (16, 13, [2, 3, 5, 0x80]),
];

/// The 39×39 transition table, `table[major][minor]`.
pub fn transition_table() -> [[TransitionEntry; 39]; 39] {
    let mut t = [[TransitionEntry::default(); 39]; 39];
    for &(a, b, e) in TRANSITION_WRITES {
        t[a as usize][b as usize] =
            TransitionEntry { cell_x: e[0], cell_y: e[1], page: e[2], alternate: e[3] & 1 != 0 };
    }
    t
}

/// Look up `T[class(major)][class(minor)]` for two (remapped) ground types.
pub fn transition(major: u8, minor: u8) -> TransitionEntry {
    let a = TRANSITION_CLASS[major as usize] as usize;
    let b = TRANSITION_CLASS[minor as usize] as usize;
    // Cheap enough for tests/tools; a renderer should cache `transition_table()`.
    let mut e = TransitionEntry::default();
    for &(x, y, v) in TRANSITION_WRITES {
        if x as usize == a && y as usize == b {
            e = TransitionEntry { cell_x: v[0], cell_y: v[1], page: v[2], alternate: v[3] & 1 != 0 };
        }
    }
    e
}

// ---------------------------------------------------------------------------------------------
// Per-triangle texture piece
// ---------------------------------------------------------------------------------------------

/// Which of the two triangles of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    /// A(x,y) top, B(x,y+1) bottom-left, C(x+1,y+1) bottom-right. Drawn first.
    Up,
    /// A(x,y) top-left, D(x+1,y) top-right, C(x+1,y+1) bottom.
    Down,
}

/// How a triangle is textured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    /// Seamless tiling page, UVs planar in map space (`CalcTilingVerticesType1/2` 0x2f822a0/0x2f823f0).
    Tiled,
    /// One triangle `k` (0..6) of pattern cell (`cell_x`, `cell_y`) (`PatternTripleVertices`).
    Pattern { cell_x: u8, cell_y: u8, k: u8 },
}

/// Texture choice for one triangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    /// Index into the 44 hardware pages (dark land already added).
    pub page: u8,
    pub kind: PieceKind,
}

/// One corner as the renderer sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Corner {
    /// Raw ground type (byte 1 of the render map element).
    pub ground: u8,
    /// Dark-land flag (byte 2 bit 0x40).
    pub dark: bool,
}

const DARK_PAGE_OFFSET: u8 = 22;

fn is_river(t: u8) -> bool {
    (96..=99).contains(&t)
}

/// `sub_2F7C100` (rel 0x66b5c0): called when the three corner types are pairwise different.
/// May rewrite the types (river widths) and returns `Some((dx, k))` for a river-mouth piece
/// (page 5, cell (2 + dx, 2), triangle k).
fn river_mouth(p: &mut [u8; 3], down: bool) -> Option<(u8, u8)> {
    let count = |v: u8| p.iter().filter(|&&t| t == v).count();
    let (n96, n97, n98, n99) = (count(96), count(97), count(98), count(99));
    if n96 + n97 + n98 + n99 == 2 {
        let r = if n99 > 0 {
            99
        } else if n98 > 0 {
            98
        } else if n97 > 0 {
            97
        } else {
            96
        };
        if is_river(p[0]) {
            if is_river(p[1]) {
                p[0] = r;
                p[1] = r;
            } else {
                p[0] = r;
                p[2] = r;
            }
        } else {
            p[1] = r;
            p[2] = r;
        }
        return None;
    }
    let widen = |t: u8| if (96..99).contains(&t) || t == 0 { 99 } else { t };
    let q = [widen(p[0]), widen(p[1]), widen(p[2])];
    if q[0] == q[1] && q[1] == q[2] {
        // All became 99: nothing is written back (faithful to rel 0x66b716).
        return None;
    }
    if q[0] == q[1] || q[1] == q[2] || q[0] == q[2] {
        *p = q;
        return None;
    }
    let (dx, k) = match (down, q) {
        (false, [16, 48, 99]) => (0, 2),
        (false, [16, 99, 48]) => (1, 4),
        (false, [48, 99, 16]) => (0, 4),
        (false, [48, 16, 99]) => (1, 2),
        (false, [99, 48, 16]) => (1, 0),
        (false, _) => (0, 0),
        (true, [16, 48, 99]) => (0, 1),
        (true, [16, 99, 48]) => (1, 3),
        (true, [48, 99, 16]) => (0, 3),
        (true, [48, 16, 99]) => (1, 1),
        (true, [99, 48, 16]) => (1, 5),
        (true, _) => (0, 5),
    };
    Some((dx, k))
}

fn pattern(e: TransitionEntry, k: u8, odd: bool) -> PieceKind {
    PieceKind::Pattern { cell_x: e.cell_x, cell_y: e.cell_y + (e.alternate && odd) as u8, k }
}

/// Choose the texture pieces of both triangles of node A = (x, y), exactly as the inner loop of
/// `DrawTexturedLandscape` (0x2f77130) does: `a`=(x,y), `b`=(x,y+1), `c`=(x+1,y+1), `d`=(x+1,y).
/// `odd_x` = `x & 1` (the loop's `v119`).
///
/// Not modelled: the fog rule that turns roads (28/29) into grass when the node's fog value is
/// ≤ 59 (`CalcCurrentFog`); apply it to `ground` beforehand if you render fog.
pub fn column_pieces(a: Corner, b: Corner, c: Corner, d: Corner, odd_x: bool) -> [Piece; 2] {
    let mut ta = render_type(a.ground);
    let tb = render_type(b.ground);
    let mut tc = render_type(c.ground);
    let td = render_type(d.ground);

    // --- Up triangle (A, B, C) ---
    let dark = if a.dark || b.dark || c.dark { DARK_PAGE_OFFSET } else { 0 };
    let mut tri = [ta, tb, tc];
    let up = select(&mut tri, false, odd_x, dark);
    // The loop writes the possibly rewritten types back into its variables: A and C carry over.
    ta = tri[0];
    tc = tri[2];

    // --- Down triangle (A, D, C) ---
    let dark = if a.dark || d.dark || c.dark { DARK_PAGE_OFFSET } else { 0 };
    let mut tri = [ta, td, tc];
    let down = select(&mut tri, true, odd_x, dark);
    [up, down]
}

/// `t` = [A, B, C] for `Up`, [A, D, C] for `Down`.
fn select(t: &mut [u8; 3], down: bool, odd: bool, dark: u8) -> Piece {
    let tiled = |ty: u8| Piece { page: dark + TILE_PAGE[ty as usize] as u8, kind: PieceKind::Tiled };
    if t[0] == t[1] && t[1] == t[2] && TILE_PAGE[t[0] as usize] >= 0 {
        return tiled(t[0]);
    }
    let n28 = t.iter().filter(|&&v| v == 28).count();
    let n29 = t.iter().filter(|&&v| v == 29).count();
    if n28 + n29 == 3 {
        return tiled(29);
    }
    if n28 > 0 && n29 > 0 {
        for v in t.iter_mut() {
            if *v == 28 {
                *v = 29;
            }
        }
    }
    // Pair rules. For Up: (B==C: T[B][A] k0) (B==A: T[B][C] k2) (A==C: T[A][B] k4).
    // For Down (t = A, D, C): (A==D: T[A][C] k3) (D==C: T[D][A] k5) (A==C: T[A][D] k1), as the
    // release loop picks its vertex arrays (0x668f70: 0x14643d0 = k3, 0x1464400 = k5, 0x14643a0
    // = k1): the odd corner always lands on the hexagon's centre (2, 2).
    let pair = |t: &[u8; 3], last_resort: bool| -> Option<(TransitionEntry, u8)> {
        let [p0, p1, p2] = *t;
        if !down {
            if p1 == p2 {
                Some((transition(p1, p0), 0))
            } else if p1 == p0 {
                Some((transition(p1, p2), 2))
            } else if p0 == p2 || last_resort {
                Some((transition(p0, p1), 4))
            } else {
                None
            }
        } else if p0 == p1 {
            Some((transition(p0, p2), 3))
        } else if p1 == p2 {
            Some((transition(p1, p0), 5))
        } else if p0 == p2 || last_resort {
            Some((transition(p0, p1), 1))
        } else {
            None
        }
    };
    if let Some((e, k)) = pair(t, false) {
        return Piece { page: dark + e.page, kind: pattern(e, k, odd) };
    }
    // Three different types. Up passes (A, B, C), Down passes (A, C, D) to sub_2F7C100.
    let mut p = if down { [t[0], t[2], t[1]] } else { *t };
    let mouth = river_mouth(&mut p, down);
    *t = if down { [p[0], p[2], p[1]] } else { p };
    if let Some((dx, k)) = mouth {
        return Piece { page: dark + 5, kind: PieceKind::Pattern { cell_x: 2 + dx, cell_y: 2, k } };
    }
    let (e, k) = pair(t, true).expect("last_resort always matches");
    Piece { page: dark + e.page, kind: pattern(e, k, odd) }
}

// ---------------------------------------------------------------------------------------------
// Texture coordinates
// ---------------------------------------------------------------------------------------------

/// Half-texel bias the release build adds to every pattern UV (rel 0x66a4fd, constant 1/512).
pub const PATTERN_UV_BIAS: f32 = 1.0 / 512.0;

/// UVs (0..1 page space, before any bias) of pattern triangle `k` in cell (`cx`, `cy`):
/// `sub_2F7BC20` (rel 0x66b390, jump table rel 0x66b5a4). A cell is 0.25 × 0.25 of the page
/// (64 × 64 texels at 256²); every triangle is 0.125 wide and 0.125 high.
/// Returned in **vertex order** `Up: [A, B, C]`, `Down: [A, D, C]` (k even = Up, k odd = Down).
pub fn pattern_uv(cx: u8, cy: u8, k: u8) -> [[f32; 2]; 3] {
    let (u0, v0) = (cx as f32 * 0.25, cy as f32 * 0.25);
    const Q: f32 = 0.0625;
    // Units of Q inside the cell, from the disassembly (engine order: Up = C,B,A; Down = D,C,A).
    let q: [[f32; 2]; 3] = match k {
        0 => [[2.0, 2.0], [1.0, 4.0], [3.0, 4.0]], // A top, B, C on the bottom edge
        1 => [[0.0, 2.0], [2.0, 2.0], [1.0, 4.0]], // A, D top edge, C bottom
        2 => [[1.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
        3 => [[1.0, 0.0], [3.0, 0.0], [2.0, 2.0]],
        4 => [[3.0, 0.0], [2.0, 2.0], [4.0, 2.0]],
        _ => [[2.0, 2.0], [4.0, 2.0], [3.0, 4.0]],
    };
    q.map(|[a, b]| [u0 + a * Q, v0 + b * Q])
}

/// UVs of a tiled triangle of node (x, y), vertex order as [`pattern_uv`]. Equivalent to the
/// planar mapping `u = (x − y/2)/8 + 1/16`, `v = y/8` (page repeats every 8 nodes in each
/// direction), which is what `g_fPatternSuboffsetX/Y` + `CalcTilingVerticesType1/2` compute
/// (the engine keeps u in 0..1 by splitting triangles at the seam; with wrap addressing the
/// unbounded value samples identically). No bias included (the engine adds 1/512 for HQ,
/// 1/256 for LQ: `dword_3E2E708`).
pub fn tiled_uv(x: i32, y: i32, tri: Tri) -> [[f32; 2]; 3] {
    let u = |nx: i32, ny: i32| (nx as f32 - ny as f32 / 2.0) / 8.0 + 1.0 / 16.0;
    let v = |ny: i32| ny.rem_euclid(8) as f32 / 8.0;
    let vy = v(y);
    let ua = u(x, y).rem_euclid(1.0);
    match tri {
        // B = (x, y+1) is W/2 left of A, C = (x+1, y+1) W/2 right; one row down.
        Tri::Up => [[ua, vy], [ua - 0.0625, vy + 0.125], [ua + 0.0625, vy + 0.125]],
        Tri::Down => [[ua, vy], [ua + 0.125, vy], [ua + 0.0625, vy + 0.125]],
    }
}

impl Piece {
    /// UVs in vertex order `Up: [A, B, C]`, `Down: [A, D, C]` for node (x, y), without bias.
    pub fn uv(&self, x: i32, y: i32, tri: Tri) -> [[f32; 2]; 3] {
        match self.kind {
            PieceKind::Tiled => tiled_uv(x, y, tri),
            PieceKind::Pattern { cell_x, cell_y, k } => pattern_uv(cell_x, cell_y, k),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Height and shading
// ---------------------------------------------------------------------------------------------

/// Default triangle size (16.16): `CGameStateHandler` calls `SetTriangleSize(1572864)` = 24 px.
pub const DEFAULT_TRIANGLE_SIZE: i32 = 24 << 16;

/// `CalcFinalHeightOffset` (0x2f5f2f0): vertical displacement of a node of height `h` in 16.16,
/// `(int)(h · 0.051562499f · W)` with W the triangle size in 16.16 (all in f32).
/// Terrain vertices and objects floor the same way: `y_px = (row_y16 − this) >> 16`
/// (arithmetic, i.e. floor). Objects get it from `RenderObjectLayer` → `BlitObject` /
/// `BlitObjectHardware`, which shift the difference. `ConvertMapIndexToScreenPosition` floors this
/// value first, but it is not on the draw path.
pub fn height_offset_fixed(h: u8, triangle_size16: i32) -> i32 {
    // 0x3D533333 = the exe's float constant (prints as 0.051562499, rel VA 0x1070d84).
    (h as f32 * f32::from_bits(0x3D53_3333) * triangle_size16 as f32) as i32
}

/// `CalcStaticHeightOffsetInt` (0x2f5f350): `(81024 · h) >> 16` — the integer zoom-1 offset used
/// by game logic (not by the renderer).
pub fn static_height_offset(h: u8) -> i32 {
    (81024 * h as i32) >> 16
}

/// Shading index of a node (`RefreshShading` 0x2f8eb10, rel 0x66d7b0):
/// `g = h(x, y−1) − h(x, y+1)` (a missing neighbour at the map edge counts as the node itself),
/// result `((g >> 1) + 8) & 15`. There is **no clamp**: |g| ≥ 16 wraps around (g = 16 → 0).
/// Stored in the low nibble of byte 2 of the render map element.
pub fn shade_index(h_up: u8, h_down: u8) -> u8 {
    let g = h_up as i32 - h_down as i32;
    (((g >> 1) + 8) & 15) as u8
}

/// Diffuse grey per shading index (`byte_3ACD720`, release VA 0x106ebec).
pub const SHADE_DIFFUSE: [u8; 16] = [25, 51, 76, 102, 127, 153, 178, 204, 255, 255, 255, 255, 255, 255, 255, 255];

/// Diffuse colour used for shading index 0 instead of the grey (0xFF472449). Index 0 is also
/// forced when byte 2 bit 7 ("dark-land crossing", set by `EnsureDarkCrossingBit`) is set.
pub const SHADE0_DIFFUSE: [u8; 3] = [0x47, 0x24, 0x49];

/// Unfogged vertex colours for a shading index: `(diffuse, specular)` RGB.
/// Diffuse multiplies the texel (D3D default `MODULATE`), specular is added
/// (`D3DRENDERSTATE_SPECULARENABLE` = 1, set as "SetLandscapeLighting"); both Gouraud-interpolated
/// (`SHADEMODE` = 2). From the `CHeightAndTypeTable` constructor (0x2f7c6a0, rel 0x66ab70) and
/// `InitShadeTables` (0x2f81cd0, rel 0x66a7b0): specular for index 9..15 starts at
/// (71, 48, 23)/256·255 for 15 and drops by 71/8 per step (clamped at 0), rounded to nearest
/// (`cvtss2si`); index ≤ 8 has no specular.
pub fn shade_colors(shade: u8) -> ([u8; 3], [u8; 3]) {
    let s = (shade & 15) as usize;
    let diffuse = if s == 0 { SHADE0_DIFFUSE } else { [SHADE_DIFFUSE[s]; 3] };
    (diffuse, shade_specular(s))
}

fn shade_specular(s: usize) -> [u8; 3] {
    if s <= 8 {
        return [0; 3];
    }
    let step = 71.0f32 / 8.0;
    let mut c = [71.0f32, 48.0, 23.0];
    for _ in 0..(15 - s) {
        for v in &mut c {
            *v = (*v - step).max(0.0);
        }
    }
    c.map(|v| (v * (1.0 / 256.0) * 255.0).round_ties_even() as u8)
}

/// Per-element shade as the renderer reads it: byte 2 bit 7 set → 0, else low nibble.
pub fn element_shade(byte2: u8) -> u8 {
    if byte2 & 0x80 != 0 { 0 } else { byte2 & 15 }
}

/// Grey ramp `CHeightAndTypeTable[0..9]` (low byte), used by the fog darkening.
const FOG_RAMP: [u8; 9] = [0x49, 0x00, 0x0a, 0x16, 0x23, 0x3f, 0x66, 0xa5, 0xff];

/// Fogged vertex colours (`CHeightAndTypeTable::CalcFogging` 0x2f85c90, rel 0x66a960):
/// `fade` = fog fade step 0..15, `old`/`new` = the element's old/new fog level 0..7 (byte 3
/// bits 0..2 / 3..5). Level 0 = fully visible.
pub fn fogged_colors(shade: u8, fade: u8, old: u8, new: u8) -> ([u8; 3], [u8; 3]) {
    let (mut d, mut s) = shade_colors(shade);
    let (fade, old, new) = (fade as i32, old as i32, new as i32);
    let mix = old * (15 - fade) + new * fade;
    let f = 16 * mix / 15;
    if f == 0 {
        return (d, s);
    }
    let idx = (f >> 4) as usize;
    let frac = (f & 15) as u32;
    let a = FOG_RAMP[8 - idx] as u32;
    let b = if idx != 8 { FOG_RAMP[8 - idx - 1] as u32 } else { a };
    let lum = ((a * (15 - frac) + b * frac) >> 4).min(d[2] as u32);
    let k = lum as f32 / 255.0;
    d = d.map(|v| (v as f32 * k).round_ties_even() as u8);
    let k2 = 1.0 - mix as f32 / 105.0;
    s = s.map(|v| (v as f32 * k2).round_ties_even() as u8);
    (d, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb16() {
        assert_eq!(Rgb16::Rgb555.to_rgb(0x7fff), [255, 255, 255]);
        assert_eq!(Rgb16::Rgb565.to_rgb(0xf800), [255, 0, 0]);
        assert_eq!(Rgb16::Rgb565.to_rgb(0x07e0), [0, 255, 0]);
        // Same pixel in 2.gh5 / 2.gh6 (first texel of page 0).
        assert_eq!(Rgb16::Rgb555.to_rgb(0x2142)[0], Rgb16::Rgb565.to_rgb(0x42a2)[0]);
    }

    #[test]
    fn table_shape() {
        let t = transition_table();
        let c = |g: u8| TRANSITION_CLASS[g as usize] as usize;
        assert_eq!(t[c(16)][c(48)], TransitionEntry { cell_x: 3, cell_y: 0, page: 3, alternate: true });
        assert_eq!(t[c(48)][c(0)], TransitionEntry { cell_x: 0, cell_y: 2, page: 5, alternate: true });
        assert_eq!(t[c(7)][c(4)], TransitionEntry { cell_x: 2, cell_y: 3, page: 5, alternate: false });
        assert_eq!(TILE_PAGE[16], 0);
        assert_eq!(TILE_PAGE[7], 4);
        assert_eq!(TILE_PAGE[0], -1);
        assert_eq!(render_type(8), 48);
        for g in Ground::ALL {
            assert!(TRANSITION_CLASS[g as usize] < 39);
        }
    }

    fn c(g: u8) -> Corner {
        Corner { ground: g, dark: false }
    }

    #[test]
    fn uniform_grass_tiles() {
        let [u, d] = column_pieces(c(16), c(16), c(16), c(16), false);
        assert_eq!(u, Piece { page: 0, kind: PieceKind::Tiled });
        assert_eq!(d, Piece { page: 0, kind: PieceKind::Tiled });
        let dark = Corner { ground: 16, dark: true };
        let [u, _] = column_pieces(dark, c(16), c(16), c(16), false);
        assert_eq!(u.page, 22);
    }

    #[test]
    fn grass_beach_edge() {
        // A grass, B/C beach: Up uses T[beach][grass] k0 = cell (2,0) page 3, +1 row on odd x.
        let [u, d] = column_pieces(c(16), c(48), c(48), c(16), true);
        assert_eq!(u, Piece { page: 3, kind: PieceKind::Pattern { cell_x: 2, cell_y: 1, k: 0 } });
        // Down: A grass, D grass, C beach -> A==D: T[grass][beach] k3 (C on the hexagon centre).
        assert_eq!(d, Piece { page: 3, kind: PieceKind::Pattern { cell_x: 3, cell_y: 1, k: 3 } });
    }

    #[test]
    fn river_mouth_piece() {
        let [u, _] = column_pieces(c(16), c(48), c(0), c(16), false);
        assert_eq!(u, Piece { page: 5, kind: PieceKind::Pattern { cell_x: 2, cell_y: 2, k: 2 } });
    }

    #[test]
    fn roads_merge() {
        let [u, _] = column_pieces(c(28), c(29), c(28), c(28), false);
        assert_eq!(u, Piece { page: 21, kind: PieceKind::Tiled });
    }

    #[test]
    fn uv_patterns_are_triangles_in_cell() {
        for k in 0..6 {
            let uv = pattern_uv(1, 2, k);
            for [u, v] in uv {
                assert!((0.25..=0.5).contains(&u) && (0.5..=0.75).contains(&v), "k={k} {u} {v}");
            }
            // Width and height are both 0.125.
            let us: Vec<f32> = uv.iter().map(|p| p[0]).collect();
            let vs: Vec<f32> = uv.iter().map(|p| p[1]).collect();
            let span = |s: &[f32]| s.iter().cloned().fold(f32::MIN, f32::max) - s.iter().cloned().fold(f32::MAX, f32::min);
            assert_eq!(span(&us), 0.125);
            assert_eq!(span(&vs), 0.125);
        }
    }

    #[test]
    fn tiled_matches_engine_suboffsets() {
        // Engine: sx = ((x - ceil(y/2)) mod 8)/8 + (y odd)/16; A.u = sx + 1/16, B.u = sx.
        for y in 0..16i32 {
            for x in 0..16 {
                let sx = ((x - (y + 1) / 2).rem_euclid(8i32)) as f32 / 8.0 + if y & 1 == 1 { 0.0625 } else { 0.0 };
                let uv = tiled_uv(x, y, Tri::Up);
                assert!(((uv[1][0] - sx).rem_euclid(1.0)).abs() < 1e-6);
                assert!(((uv[0][0] - (sx + 0.0625)).rem_euclid(1.0)).abs() < 1e-6);
                assert_eq!(uv[0][1], (y % 8) as f32 * 0.125);
            }
        }
    }

    #[test]
    fn height_and_shade() {
        assert_eq!(height_offset_fixed(0, DEFAULT_TRIANGLE_SIZE), 0);
        // 100 · 0.0515625 · 24 = 123.75 px
        assert_eq!(height_offset_fixed(100, DEFAULT_TRIANGLE_SIZE) >> 16, 123);
        assert_eq!(static_height_offset(100), 123);
        assert_eq!(shade_index(10, 10), 8);
        assert_eq!(shade_index(20, 10), 13);
        assert_eq!(shade_index(10, 20), 3);
        assert_eq!(shade_index(26, 10), 0); // wraps, no clamp
        assert_eq!(shade_colors(8), ([255; 3], [0; 3]));
        assert_eq!(shade_colors(0).0, SHADE0_DIFFUSE);
        assert_eq!(shade_colors(15).1, [71, 48, 23]);
        assert_eq!(shade_colors(9).1, [18, 0, 0]);
        assert_eq!(fogged_colors(8, 0, 0, 0), shade_colors(8));
        assert_eq!(fogged_colors(8, 0, 7, 7).0, [0; 3]);
    }
}
