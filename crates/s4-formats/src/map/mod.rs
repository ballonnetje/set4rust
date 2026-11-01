//! Maps (`Map\**\*.map`, `*.edm`) and the shared savegame container (`Documents\TheSettlers4\Save\*.sav`).
//!
//! Layers:
//! * [`container`] — file CRC, obfuscated 24-byte chunk headers, chunk CRCs
//!   (`S4::CMapFile::Open` 0x13da960 / `LoadChunk` 0x13dad70).
//! * [`lzhl`] — the LZH-Light decompressor every packed chunk uses (0x2f2b5e0).
//! * [`crypt`] — header keystream (`Cryptor`) and the CRC-16 variant.
//! * [`chunks`] — typed map chunks.
//!
//! Map chunk ids (type/subtype), from `MA_OpenMapFile` (0x2fbbe60), `CGameRun::LoadEditorMap`
//! (0x1484f50) and `CWorldManager::LoadGfxData` (0x16a2a20):
//! ```text
//!  0  end marker (4 bytes, unknown)       10  unknown, 83 bytes, no reader found
//!  1  GeneralInfo (24)                    11/12  description / tips text (ANSI)
//!  2  Player x param (45 each)            13  landscape, T_GFX_MAP_ELEMENT x size² (4 each)
//!  3  TeamSetup, subtype = setup 0..9     14/15  English description / tips
//!  4  preview, u16 x param², param = 256  16  Lua script (ANSI, NUL-terminated)
//!  6  map elements x size² (4 each)       64  editor-map flag (4 bytes, unknown)
//!  7  settlers x param (6 each)           65  campaign-map flag (4 bytes, unknown)
//!  8  buildings x param (20 each)         66  EditorInfo (20)
//!  9  piles x param (8 each)              256 unknown (4 bytes; 60 maps), no reader found
//! ```
//! Ids >= 0x100 other than 256 that appear in maps are decoys (see [`container`]).
//! Observed but unexplained payloads: chunks 0, 64 and 65 hold a u16 that varies per map (often
//! 0) followed by 00 00; chunk 256 is always `09 00 00 00`; chunk 10 is 83 bytes that differ only
//! in the first ~10 bytes and the last 9 (the editor exe, not S4_Main, presumably reads it).
//! Savegames use ids 50/51, 130..182, 200..210, 250 in the same container; only the container
//! layer is implemented for them.

pub mod chunks;
pub mod container;
pub mod crypt;
pub mod lzhl;

use std::path::Path;

pub use chunks::{
    Building, EditorInfo, GeneralInfo, Landscape, MapObject, Node, Pile, Player, Settler,
    TeamSetup, TeamSlot, ground_rgb,
};
pub use container::{ChunkHeader, Container, chunk_name};

use crate::{Result, bail};

/// Chunk ids used by maps.
pub mod id {
    pub const END: u16 = 0;
    pub const GENERAL: u16 = 1;
    pub const PLAYERS: u16 = 2;
    pub const TEAM_SETUP: u16 = 3;
    pub const PREVIEW: u16 = 4;
    pub const MAP_ELEMENTS: u16 = 6;
    pub const SETTLERS: u16 = 7;
    pub const BUILDINGS: u16 = 8;
    pub const PILES: u16 = 9;
    pub const UNKNOWN_10: u16 = 10;
    pub const DESCRIPTION: u16 = 11;
    pub const TIPS: u16 = 12;
    pub const LANDSCAPE: u16 = 13;
    pub const DESCRIPTION_EN: u16 = 14;
    pub const TIPS_EN: u16 = 15;
    pub const SCRIPT: u16 = 16;
    pub const EDITOR_FLAG: u16 = 64;
    pub const CAMPAIGN_FLAG: u16 = 65;
    pub const EDITOR_INFO: u16 = 66;
}

/// Which of the four map texts (`MA_GetDescriptionText` 0x2fbdf60 indices 0..3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapText {
    Description,
    Tips,
    DescriptionEnglish,
    TipsEnglish,
}

/// An opened map, editor map or savegame.
pub struct MapFile {
    pub container: Container,
}

impl MapFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(std::fs::read(crate::path::resolve(path))?)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Ok(Self { container: Container::parse(data)? })
    }

    /// File version: 31 base game, 40 add-on.
    pub fn version(&self) -> u32 {
        self.container.version
    }

    fn required(&self, kind: u16) -> Result<(ChunkHeader, Vec<u8>)> {
        match self.container.load_id(kind, 0)? {
            Some(c) => Ok(c),
            None => bail!("map: chunk {kind} ({}) missing", chunk_name(kind).unwrap_or("?")),
        }
    }

    pub fn general(&self) -> Result<GeneralInfo> {
        GeneralInfo::parse(&self.required(id::GENERAL)?.1)
    }

    pub fn editor_info(&self) -> Result<Option<EditorInfo>> {
        self.container.load_id(id::EDITOR_INFO, 0)?.map(|(_, d)| EditorInfo::parse(&d)).transpose()
    }

    pub fn is_campaign(&self) -> bool {
        self.container.find(id::CAMPAIGN_FLAG, 0).is_some()
    }

    pub fn is_editor_map(&self) -> bool {
        self.container.find(id::EDITOR_FLAG, 0).is_some()
    }

    /// Players from chunk 2 (count = header param).
    pub fn players(&self) -> Result<Vec<Player>> {
        let (h, d) = self.required(id::PLAYERS)?;
        chunks::parse_players(&d, h.param)
    }

    /// All team setups (chunk 3, subtypes 0..9), in subtype order.
    pub fn team_setups(&self) -> Result<Vec<TeamSetup>> {
        let n = self.players()?.len() as u32;
        let mut out = Vec::new();
        for sub in 0..10 {
            if let Some((_, d)) = self.container.load_id(id::TEAM_SETUP, sub)? {
                out.push(chunks::parse_team_setup(sub, &d, n)?);
            }
        }
        Ok(out)
    }

    /// Landscape (chunk 13) merged with map elements (chunk 6).
    pub fn landscape(&self) -> Result<Landscape> {
        let (h, gfx) = self.required(id::LANDSCAPE)?;
        let size = self.general().map(|g| g.size).unwrap_or(h.param);
        if h.param != 0 && h.param != size {
            bail!("landscape: header side {} != general size {size}", h.param);
        }
        let elements = self.container.load_id(id::MAP_ELEMENTS, 0)?;
        Landscape::parse(size, &gfx, elements.as_ref().map(|e| e.1.as_slice()))
    }

    /// Non-zero chunk-6 objects (trees, stones, decoration), row-major.
    pub fn objects(&self) -> Result<Vec<MapObject>> {
        let l = self.landscape()?;
        Ok(l.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.object != 0)
            .map(|(i, n)| MapObject { x: i as u32 % l.width, y: i as u32 / l.width, id: n.object })
            .collect())
    }

    pub fn settlers(&self) -> Result<Vec<Settler>> {
        match self.container.load_id(id::SETTLERS, 0)? {
            Some((h, d)) => Settler::parse_all(&d, h.param),
            None => Ok(Vec::new()),
        }
    }

    pub fn buildings(&self) -> Result<Vec<Building>> {
        match self.container.load_id(id::BUILDINGS, 0)? {
            Some((h, d)) => Building::parse_all(&d, h.param),
            None => Ok(Vec::new()),
        }
    }

    pub fn piles(&self) -> Result<Vec<Pile>> {
        match self.container.load_id(id::PILES, 0)? {
            Some((h, d)) => Pile::parse_all(&d, h.param),
            None => Ok(Vec::new()),
        }
    }

    pub fn text(&self, which: MapText) -> Result<Option<String>> {
        let kind = match which {
            MapText::Description => id::DESCRIPTION,
            MapText::Tips => id::TIPS,
            MapText::DescriptionEnglish => id::DESCRIPTION_EN,
            MapText::TipsEnglish => id::TIPS_EN,
        };
        Ok(self.container.load_id(kind, 0)?.map(|(_, d)| chunks::cstr(&d)))
    }

    /// Lua source from chunk 16. `None` if absent or shorter than 4 bytes (the game then runs no
    /// script, `CGameRun::LoadEditorMap`).
    pub fn script(&self) -> Result<Option<String>> {
        Ok(self.container.load_id(id::SCRIPT, 0)?.map(|(_, d)| chunks::cstr(&d)).filter(|s| s.len() >= 4))
    }

    /// Chunk 4 as (side, raw 16-bit pixels). `MA_OpenMapFile` accepts sides 128..=1024.
    pub fn preview_raw(&self) -> Result<(u32, Vec<u16>)> {
        let (h, d) = self.required(id::PREVIEW)?;
        let side = h.param;
        if !(128..=1024).contains(&side) || d.len() != 2 * (side * side) as usize {
            bail!("preview: side {side}, {} bytes", d.len());
        }
        Ok((side, d.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect()))
    }

    /// Preview as RGBA8 (side × side).
    pub fn preview_rgba(&self) -> Result<(u32, Vec<u8>)> {
        let (side, px) = self.preview_raw()?;
        let mut out = Vec::with_capacity(px.len() * 4);
        for p in px {
            out.extend_from_slice(&preview_pixel(p));
        }
        Ok((side, out))
    }
}

/// Preview pixel → RGBA. The pixels are RGB555 (`r << 10 | g << 5 | b`), the same "HiCol"
/// encoding `CLandscapeColorProperties` (ctor 0x2f8dcb0) builds for the random-map generator's
/// `CreatePreview` (0x2fc09f0). Checked visually: decoded as 555 the preview matches the
/// landscape's ground types; as 565 greens turn purple.
pub fn preview_pixel(p: u16) -> [u8; 4] {
    let c = |v: u16| ((v & 0x1F) as u32 * 255 / 31) as u8;
    [c(p >> 10), c(p >> 5), c(p), 255]
}
