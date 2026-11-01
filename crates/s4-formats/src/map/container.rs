//! The chunk container shared by maps (`.map`, `.edm`) and savegames (`.sav`).
//!
//! Mirrors `S4::CMapFile::Open` (0x13da960), `LoadChunk` (0x13dad70), `SaveChunk` (0x13db820),
//! `FileCRC` (0x13dbd60) and the MapAccess reader `MA_OpenMapFile` (0x2fbbe60) /
//! `CalcChecksumOfFile` (0x2fbbdb0) / `ReadChunk` (0x2fc0150).
//!
//! ```text
//! file:   u32 file_crc      s4_crc(file[8..])            (Open: compared when a4 is set)
//!         u32 version       31 = base game, 40 = add-on ("g_bAddOnMap"); anything else rejected
//!         chunk*            until fewer than 24 bytes remain
//! chunk:  [24-byte header, XOR keystream, see crypt.rs]
//!         u16 type          chunk id (see `chunk_name`)
//!         u16 subtype       e.g. setup index for type 3
//!         u32 packed_size   bytes of payload following the header
//!         u32 unpacked_size 0xFFFFFFFF = stored uncompressed, else LZHL (lzhl.rs)
//!         u32 crc           s4_crc(payload as stored) in the low 16 bits
//!         u32 reserved      SaveChunk writes 0; 0 in every file checked
//!         u32 param         LoadChunk's out-value `a4` (player count, map size, record count...)
//!                           — meaningful in maps; uninitialised stack garbage in savegames
//!         u8  payload[packed_size]
//! ```
//! `LoadChunk` scans from the start and takes the *first* header whose (type, subtype) matches, so
//! later duplicates are ignored. Map files also contain decoy chunks with random large type ids
//! (e.g. type 1233 subtype 1792): they have valid sizes and CRCs and are skipped by id.

use super::crypt::{header_keystream, s4_crc};
use super::lzhl;
use crate::reader::Reader;
use crate::{Result, bail};

/// One decoded chunk header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkHeader {
    pub kind: u16,
    pub subtype: u16,
    pub packed_size: u32,
    /// `None` when stored uncompressed (0xFFFFFFFF on disk).
    pub unpacked_size: Option<u32>,
    /// Stored checksum (low 16 bits of the on-disk u32).
    pub crc: u32,
    pub reserved: u32,
    pub param: u32,
    /// File offset of the 24-byte header.
    pub header_offset: usize,
    /// File offset of the payload.
    pub data_offset: usize,
}

impl ChunkHeader {
    pub fn size(&self) -> usize {
        self.unpacked_size.unwrap_or(self.packed_size) as usize
    }
}

/// A parsed container (whole file kept in memory).
pub struct Container {
    data: Vec<u8>,
    pub file_crc: u32,
    pub version: u32,
    pub chunks: Vec<ChunkHeader>,
    /// Bytes after the last complete chunk (< 24; ignored by the game). Seen: 0..=3.
    pub trailing: usize,
}

impl Container {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let mut r = Reader::new(&data, "map container");
        let file_crc = r.u32()?;
        let version = r.u32()?;
        if version != 31 && version != 40 {
            bail!("map container: version {version} (expected 31 or 40)");
        }
        let ks = header_keystream();
        let mut chunks = Vec::new();
        let mut pos = 8;
        while data.len() - pos >= 24 {
            let mut h = [0u8; 24];
            for (i, b) in h.iter_mut().enumerate() {
                *b = data[pos + i] ^ ks[i];
            }
            let mut hr = Reader::new(&h, "chunk header");
            let kind = hr.u16()?;
            let subtype = hr.u16()?;
            let packed_size = hr.u32()?;
            let unpacked = hr.u32()?;
            let crc = hr.u32()?;
            let reserved = hr.u32()?;
            let param = hr.u32()?;
            let data_offset = pos + 24;
            let end = data_offset.checked_add(packed_size as usize).filter(|&e| e <= data.len());
            let Some(end) = end else {
                bail!("chunk {kind}/{subtype} at {pos:#x}: size {packed_size} past end of file");
            };
            chunks.push(ChunkHeader {
                kind,
                subtype,
                packed_size,
                unpacked_size: (unpacked != u32::MAX).then_some(unpacked),
                crc,
                reserved,
                param,
                header_offset: pos,
                data_offset,
            });
            pos = end;
        }
        let trailing = data.len() - pos;
        Ok(Self { data, file_crc, version, chunks, trailing })
    }

    /// Whole-file checksum as `CMapFile::FileCRC` computes it.
    pub fn computed_file_crc(&self) -> u32 {
        s4_crc(&self.data[8..]) as u32
    }

    pub fn file_crc_ok(&self) -> bool {
        self.computed_file_crc() == self.file_crc
    }

    /// First chunk with this id, the way `LoadChunk` finds it.
    pub fn find(&self, kind: u16, subtype: u16) -> Option<&ChunkHeader> {
        self.chunks.iter().find(|c| c.kind == kind && c.subtype == subtype)
    }

    /// Payload bytes as stored (possibly compressed).
    pub fn raw(&self, h: &ChunkHeader) -> &[u8] {
        &self.data[h.data_offset..h.data_offset + h.packed_size as usize]
    }

    pub fn chunk_crc_ok(&self, h: &ChunkHeader) -> bool {
        s4_crc(self.raw(h)) as u32 == h.crc
    }

    /// Verify the CRC and decompress, like `LoadChunk`.
    pub fn load(&self, h: &ChunkHeader) -> Result<Vec<u8>> {
        if !self.chunk_crc_ok(h) {
            bail!("chunk {}/{}: CRC mismatch", h.kind, h.subtype);
        }
        let raw = self.raw(h);
        match h.unpacked_size {
            None => Ok(raw.to_vec()),
            Some(n) => lzhl::decompress(raw, n as usize)
                .map_err(|e| crate::Error::Format(format!("chunk {}/{}: {e}", h.kind, h.subtype))),
        }
    }

    /// `load` of the first chunk with this id; `Ok(None)` if absent.
    pub fn load_id(&self, kind: u16, subtype: u16) -> Result<Option<(ChunkHeader, Vec<u8>)>> {
        match self.find(kind, subtype) {
            None => Ok(None),
            Some(h) => Ok(Some((*h, self.load(h)?))),
        }
    }
}

/// Name for a chunk id, from the code that reads or writes it. `None` for ids no reader handles
/// (in maps these are the decoy chunks).
pub fn chunk_name(kind: u16) -> Option<&'static str> {
    Some(match kind {
        // Map chunks (MA_OpenMapFile 0x2fbbe60 switch, CGameRun::LoadEditorMap 0x1484f50).
        0 => "end",                         // MA reader stops here; 4-byte payload, unknown
        1 => "general",                     // SEditorGlobalMapData, 24 bytes
        2 => "players",                     // CPlayerData::Load, 45 bytes x param
        3 => "team_setup",                  // CPlayerData::LoadTeamData, subtype = setup 0..9
        4 => "preview",                     // u16 pixels, param = side (128..1024)
        5 => "end_alt",                     // MA reader also stops here
        6 => "map_elements",                // objects/owner/flags/resources per node
        7 => "settlers",                    // CSettlerMgr::LoadSettlerData, 6 bytes each
        8 => "buildings",                   // CBuildingMgr::LoadBuildingData, 20 bytes each
        9 => "piles",                       // CPileMgr::LoadPileData, 8 bytes each
        10 => "unknown_10",
        11 => "description",                // g_pTextDescription
        12 => "tips",                       // g_pTextTandT
        13 => "landscape",                  // T_GFX_MAP_ELEMENT per node
        14 => "description_en",             // g_pTextEnglishDescription
        15 => "tips_en",                    // g_pTextEnglishTandT
        16 => "script",                     // Lua source for CGameScriptManager::NewGameEx
        64 => "editor_flag",                // sets g_bEditorMap
        65 => "campaign_flag",              // sets g_bCampaign
        66 => "editor_info",                // EDITOR_INFO, 20 bytes
        // Savegame chunks (CGameRun save/load, CWorldManager::SaveMap 0x16a28b0, ...).
        50 | 51 => "save_unknown",
        130..=182 => "save_object",
        200 => "save_render_map",           // TMap<T_GFX_MAP_ELEMENT>
        201 => "save_deco_object_map",
        202 => "save_map_object_map",
        204 => "save_help_object_map",
        205 => "save_flag_map",
        207 => "save_resource_map",
        209 => "save_walk_move_cost_map",
        210 => "save_owner_map",
        220 => "random_map_info",           // CMapGeneratorHost, 0x5C bytes
        250 => "debug_info",                // CMapFile::SaveDbgInfoChunk, 32 bytes
        256 => "unknown_256",
        _ => return None,
    })
}
