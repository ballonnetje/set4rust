//! The random map generator of the release (own RE of S4_Main.exe; evidence and the unicorn
//! harness in `tools/re/mapgen/`).
//!
//! * `CRandomMaps` (vtable 0x1055984): Init 0x50b1d0 builds the host `CMapGeneratorHost`
//!   (ctor 0x50a4d0) from the 0xb8-byte parameter block and runs the generator's init 0x68c640
//!   and the preview 0x68caf0; Generate 0x50b290 runs the main pass 0x68d8c0 and writes the map
//!   (0x50ab70: chunks 1, 13 landscape, 6 objects, 220 random-map info).
//! * The map key `RD_xxxxxxxx` ([`key`]): parse 0x50ada0, build 0x50abf0.
//! * The host's object placement (0x50a8f0) uses the object records the host's static init
//!   0x564750 builds from `objectInfo.xml` ([`objects`]).
//!
//! * Ours: [`Map::map_file_bytes`] / [`map_path`]: the chunks the release keeps in memory as a
//!   map container, with an extra chunk 2 (players: race 255, the starts from chunk 220) and the
//!   preview as chunk 4, so the game's map loaders take a random map unchanged; the release's
//!   game start 0x5069b0 reads chunk 220 instead. An internal file format detail.
//!
//! Floats: the generator uses SSE scalar arithmetic only (no x87), `sqrtsd` and `pow(int, 2)`;
//! f32 / f64 in the release's operation order give the same bits.

pub mod blob;
pub mod generator;
pub mod key;
pub mod main_pass;
pub mod objects;
pub mod preview;
pub mod resources;
pub mod rivers;

/// A generated map: the host's buffers after the main pass (4 bytes per node; landscape
/// height, type, slope nibble; objects id, —, flags, resource) and the start positions
/// (players 1..=8).
#[derive(Clone, Debug)]
pub struct Map {
    pub size: i32,
    /// The host's player count (block +0x20, clamped to 1..=9 by the ctor 0x50a4d0).
    pub players: u32,
    pub land: Vec<[u8; 4]>,
    pub objs: Vec<[u8; 4]>,
    pub starts: [(i32, i32); 9],
    /// The preview (0x68caf0, 160 × 160 RGB555).
    pub preview: Vec<u16>,
}

impl Map {
    /// The chunks `CRandomMaps` stores for the game (0x50ab70 → the chunk store 0x4b1ac0):
    /// 1 (host +0x120, `SEditorGlobalMapData`: players at +4, size at +0xc), 13 (landscape),
    /// 6 (objects) and 220 (host +0xc4: 0x5c, 1, 9, size, players, then the start x of players
    /// 0..=8 and their y, player 0 unused), the game start 0x5069b0 reads 220 back.
    pub fn chunks(&self) -> Vec<(u16, Vec<u8>)> {
        let words = |w: &[u32]| w.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>();
        let mut info = vec![0x5c, 1, 9, self.size as u32, self.players];
        info.extend(self.starts.iter().map(|s| s.0 as u32));
        info.extend(self.starts.iter().map(|s| s.1 as u32));
        vec![
            (1, words(&[0, self.players, 0, self.size as u32, 0, 0])),
            (13, self.land.concat()),
            (6, self.objs.concat()),
            (220, words(&info)),
        ]
    }

    /// Ours: chunk 2 (45 bytes per player: race 255 "random", the start, no name) from chunk
    /// 220, for map readers that take the players from chunk 2. The release's game start reads
    /// chunk 220 instead for a random map (0x5069b0: race 5 with the random flag +0x3c3, the
    /// starts from +0x18 / +0x3c).
    pub fn player_chunk(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for k in 1..=self.players.min(8) as usize {
            out.extend(255u32.to_le_bytes());
            out.extend((self.starts[k].0 as u32).to_le_bytes());
            out.extend((self.starts[k].1 as u32).to_le_bytes());
            out.extend([0u8; 33]);
        }
        out
    }

    /// The chunks, [`Map::player_chunk`] and the preview as chunk 4 [ours] as a map container
    /// (`s4_formats::map::container`, version 31, uncompressed; header param: the player count
    /// for chunk 2, the side for chunk 4, else 0) [inf: the release keeps the chunks in memory,
    /// not in a file].
    pub fn map_file_bytes(&self) -> Vec<u8> {
        use s4_formats::map::crypt::{header_keystream, s4_crc};
        let ks = header_keystream();
        let mut out = vec![0u8; 4];
        out.extend(31u32.to_le_bytes());
        let mut chunks = self.chunks();
        chunks.insert(1, (2, self.player_chunk()));
        chunks.insert(2, (4, self.preview.iter().flat_map(|p| p.to_le_bytes()).collect()));
        for (kind, data) in chunks {
            let param = match kind {
                2 => self.players.min(8),
                4 => 160,
                _ => 0,
            };
            let mut h = Vec::with_capacity(24);
            h.extend(kind.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend(u32::MAX.to_le_bytes());
            h.extend((s4_crc(&data) as u32).to_le_bytes());
            h.extend(0u32.to_le_bytes());
            h.extend(param.to_le_bytes());
            out.extend(h.iter().zip(ks).map(|(b, k)| b ^ k));
            out.extend(data);
        }
        let crc = s4_crc(&out[8..]) as u32;
        out[..4].copy_from_slice(&crc.to_le_bytes());
        out
    }
}

/// `CRandomMaps` Init (0x50b1d0: host, generator init, preview) and Generate's main pass
/// (0x50b290 → 0x68d8c0), with the host's object records (`objects::records`).
pub fn generate(params: &key::Params, records: Vec<objects::Record>) -> Map {
    let mut g = generator::Gen::new(params);
    g.records = records;
    let preview = g.preview();
    g.main_pass();
    Map { size: g.size, players: (g.params.players as i32).clamp(1, 9) as u32, land: g.land, objs: g.objs, starts: g.starts, preview }
}

/// The version of the generated map files: bumped whenever `map_file_bytes` of a key changes
/// (the generator, the chunks), so no stale file of an older build is taken (a network peer
/// with one would fail the map hash).
pub const FILE_VERSION: u32 = 1;

/// The map file of a random map key (`RD_` optional), generated once into
/// `<temp>/s4rs/random/v<FILE_VERSION>/RD_<key>.map` [ours: the release generates the map from
/// its name `RD_<key>` when the game starts, 0x5069b0 → `CRandomMaps` vtable +0x20 / +0x10 /
/// +0 / +8]. Each call writes a temporary file of its own and renames it into place.
pub fn map_path(key: &str) -> s4_formats::Result<std::path::PathBuf> {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let k = key.strip_prefix("RD_").unwrap_or(key);
    let Some(p) = key::parse(k) else {
        return Err(s4_formats::Error::Format(format!("random map key {key:?}")));
    };
    let dir = std::env::temp_dir().join("s4rs").join("random").join(format!("v{FILE_VERSION}"));
    let path = dir.join(format!("RD_{k}.map"));
    if !path.exists() {
        let data = s4_formats::gamedata::GameData::load_default()?;
        let m = generate(&p, objects::records(&data));
        std::fs::create_dir_all(&dir)?;
        let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join(format!("RD_{k}.{}.{n}.tmp", std::process::id()));
        std::fs::write(&tmp, m.map_file_bytes())?;
        // Another writer of the same key may have been first: its file is the same.
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            if !path.exists() {
                return Err(e.into());
            }
        }
    }
    Ok(path)
}

/// CRC-32 (IEEE, as zlib's), for the checkpoints.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}
