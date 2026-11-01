//! Reading original savegames (`*.sav`) as test oracles. See `docs/SIM.md` §6 for the chunk map.
//!
//! The world layers are raw arrays (`CWorldManager::SaveMap`); the general chunk is binary
//! (`CGameChunkGeneral::Save`). Entity chunks are text streams of whitespace-separated numbers
//! (`operator^` / `Serial::Serialize`) whose per-class layouts are not ported yet, so entity
//! records are located by their `IEntity` header (`IEntity::IEntity(std::istream&)`): class id,
//! then `version (2), id, m_nType, m_uPackedXY, m_cHealth, m_objType, race|player<<4, m_iFlags,
//! m_iUniqueId`. A header is accepted only if every field is in range and the object type matches
//! the chunk; the count must equal the record count at the chunk's head (checked by callers).

use s4_formats::map::MapFile;

use crate::grid::Y16X16;

/// The `IEntity` part of a saved entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SavedEntity {
    pub id: u16,
    pub kind: u16,
    pub pos: Y16X16,
    pub health: u8,
    pub obj_type: u8,
    pub race: u8,
    pub owner: u8,
    pub flags: u32,
    pub uid: u32,
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SaveGame {
    pub size: u32,
    /// `CGameChunkGeneral::m_uiTickCounter`.
    pub tick: u32,
    /// Game RNG `(seed, iteration)`.
    pub rng: (u32, u32),
    /// Chunk 210.
    pub owner: Vec<u8>,
    /// Chunk 205.
    pub flags: Vec<u8>,
    /// Chunk 202.
    pub map_object: Vec<u16>,
    /// Chunk 201.
    pub deco_object: Vec<u16>,
    /// Buildings from chunk 162 (header scan), and the record count the chunk declares.
    pub buildings: Vec<SavedEntity>,
    pub building_count: u32,
    /// Settlers from chunk 161 (header scan; no declared count is checked).
    pub settlers: Vec<SavedEntity>,
    /// Eco-sectors from chunk 180 as `(owner, world index)`: each `CEcoSector` record starts with
    /// control id 0xCEE458C8, version, then +12, +4, owner (+18), …, and +8 = a world index whose
    /// tiling eco-sector becomes the record's id on load (`CEcoSector(std::istream&)`).
    pub eco_sectors: Vec<(u8, u32)>,
    /// Render map (chunk 200), 4 bytes per node.
    pub render: Vec<u8>,
    /// Chunk 50 present: written only in the Settlers United era (docs/SIM.md §10.2). Such saves
    /// were played with the mod's balance data; for rules that data affects (footprints) they
    /// are not vanilla oracles.
    pub settlers_united: bool,
    /// `m_bWorkerInHouse` per building id.
    workers_in_house: std::collections::BTreeMap<u16, Option<bool>>,
}

fn chunk(m: &MapFile, id: u16) -> s4_formats::Result<Vec<u8>> {
    match m.container.load_id(id, 0)? {
        Some((_, d)) => Ok(d),
        None => Err(s4_formats::Error::Format(format!("save: chunk {id} missing"))),
    }
}

fn u16s(d: &[u8]) -> Vec<u16> {
    d.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
}

fn le32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

impl SaveGame {
    pub fn open(path: impl AsRef<std::path::Path>) -> s4_formats::Result<SaveGame> {
        let m = MapFile::open(path)?;
        let owner = chunk(&m, 210)?;
        let size = (1..=1024u32).find(|s| (s * s) as usize == owner.len()).ok_or_else(|| {
            s4_formats::Error::Format(format!("save: owner layer of {} bytes", owner.len()))
        })?;
        let general = chunk(&m, 130)?;
        // Tick: 13 bytes before the end-of-base signature 761539484 (0x2D642B9C); seed/iteration:
        // the two u32 after signature 493104030, i.e. 28 / 24 bytes before the end (version 10).
        let sig = 761_539_484u32.to_le_bytes();
        let p = general.windows(4).skip(4).position(|w| w == sig).map(|p| p + 4);
        let tick = p.filter(|&p| p >= 13).map_or(0, |p| le32(&general, p - 13));
        let n = general.len();
        let rng = if n >= 28 { (le32(&general, n - 28), le32(&general, n - 24)) } else { (0, 0) };
        let text = chunk(&m, 162)?;
        let (building_count, buildings, headers) = scan_buildings(&text);
        let settlers_united = m.container.load_id(50, 0)?.is_some();
        let toks = tokens(&text);
        let workers_in_house = buildings
            .iter()
            .zip(&headers)
            .map(|(b, &h)| (b.id, worker_in_house(&toks, h, b)))
            .collect();
        let settlers = scan_entities(&chunk(&m, 161)?, 0, 1, 67);
        let eco_text = chunk(&m, 180)?;
        let toks = tokens(&eco_text);
        let mut eco_sectors = Vec::new();
        for (i, t) in toks.iter().enumerate() {
            if *t == b"-823895864"
                && let (Some(o), Some(w)) = (toks.get(i + 4).and_then(|t| num(t)), toks.get(i + 16).and_then(|t| num(t)))
            {
                eco_sectors.push((o as u8, w as u32));
            }
        }
        Ok(SaveGame {
            size,
            tick,
            rng,
            owner,
            flags: chunk(&m, 205)?,
            map_object: u16s(&chunk(&m, 202)?),
            deco_object: u16s(&chunk(&m, 201)?),
            buildings,
            building_count,
            settlers,
            eco_sectors,
            render: chunk(&m, 200)?,
            settlers_united,
            workers_in_house,
        })
    }

    /// `IBuildingRole +29` (`m_bWorkerInHouse`) of a saved building (see [`worker_in_house`]).
    pub fn worker_in_house(&self, b: &SavedEntity) -> Option<bool> {
        self.workers_in_house.get(&b.id).copied().flatten()
    }
}

/// `IBuildingRole::Store` writes `version 4, u8 +4, u8 +5, u16 +6 (building id), …, bool +28,
/// bool +29 m_bWorkerInHouse`; the record is the first `4 _ _ id` after the building's IEntity
/// header (from `examples/oracle_rom2.rs`).
fn worker_in_house(toks: &[&[u8]], header: usize, b: &SavedEntity) -> Option<bool> {
    let id = b.id.to_string();
    let k = (header + 10..header + 60).find(|&k| toks.get(k) == Some(&id.as_bytes()) && toks.get(k - 3) == Some(&b"4".as_slice()))?;
    match *toks.get(k + 10)? {
        b"0" => Some(false),
        b"1" => Some(true),
        _ => None,
    }
}

/// `CBuildingMgr::Load` (chunk 162): header `version 2`, 9×(2 + 20) ints, count, 9×83, 9×83×2,
/// 9×83 values, then the number of buildings and the persistent objects.
fn scan_buildings(text: &[u8]) -> (u32, Vec<SavedEntity>, Vec<usize>) {
    let head = 1 + 9 * 22 + 1 + 9 * 83 + 9 * 83 * 2 + 9 * 83;
    let count = tokens(text).get(head).and_then(|t| num(t)).unwrap_or(0) as u32;
    let (e, pos) = scan_entities_at(text, head + 1, 8, 83);
    (count, e, pos)
}

fn tokens(text: &[u8]) -> Vec<&[u8]> {
    text.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|t| !t.is_empty()).collect()
}

fn num(t: &[u8]) -> Option<i64> {
    std::str::from_utf8(t).ok()?.parse::<i64>().ok()
}

/// All `IEntity` headers of object type `obj_type` with `0 < m_nType < max_kind`, from token `start`.
fn scan_entities(text: &[u8], start: usize, obj_type: i64, max_kind: i64) -> Vec<SavedEntity> {
    scan_entities_at(text, start, obj_type, max_kind).0
}

/// [`scan_entities`] plus the token index of each header.
fn scan_entities_at(text: &[u8], start: usize, obj_type: i64, max_kind: i64) -> (Vec<SavedEntity>, Vec<usize>) {
    let mut at = Vec::new();
    let toks = tokens(text);
    let mut out = Vec::new();
    let mut k = start;
    while k + 9 < toks.len() {
        let v: Option<Vec<i64>> = toks[k..k + 10].iter().map(|t| num(t)).collect();
        if let Some(v) = v {
            let (ver, id, kind, pk, hp, ot, tp, fl, uid) = (v[1], v[2], v[3], v[4], v[5], v[6], v[7], v[8], v[9]);
            let pos = Y16X16(pk as u32);
            if ver == 2
                && ot == obj_type
                && (1..0xFFFF).contains(&id)
                && (1..max_kind).contains(&kind)
                && (0..1024).contains(&pos.x())
                && (0..1024).contains(&pos.y())
                && (1..=8).contains(&(tp >> 4))
                && (0..=255).contains(&hp)
            {
                out.push(SavedEntity {
                    id: id as u16,
                    kind: kind as u16,
                    pos,
                    health: hp as u8,
                    obj_type: ot as u8,
                    race: (tp & 15) as u8,
                    owner: (tp >> 4) as u8,
                    flags: fl as u32,
                    uid: uid as u32,
                });
                at.push(k);
                k += 10;
                continue;
            }
        }
        k += 1;
    }
    (out, at)
}

/// One player of chunk 140 (`CPlayerManager` store 0x50a190): `CPlayerGameData` (0x4f70f0:
/// u32 0x76234526, u32 0x70, the 0x70 bytes of the mana block, u32 8, 8 more bytes), then after
/// all players each player's race and colour (u32 each).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedPlayer {
    pub race: u32,
    pub color: u32,
    /// The block decoded (see [`crate::magic`] for the layout and keys).
    pub magic: crate::magic::ManaPool,
    /// +0: bit 0 = the GUI's cast cache is stale.
    pub flags: u32,
    /// +0xc decoded (never read by the exe).
    pub unknown_c: u32,
    /// +0x50: the GUI's affordable-cast cache (−1 = not computed).
    pub cache: [i32; 8],
    /// The 8 bytes after the block (`CPlayerGameData` +0x78).
    pub tail: [u32; 2],
}

/// Chunk 140 of a savegame (`CPlayerManager::Load` 0x50a100): the player count, the local
/// player, then the players (see [`SavedPlayer`]).
pub fn players(path: impl AsRef<std::path::Path>) -> s4_formats::Result<Vec<SavedPlayer>> {
    use crate::magic::keys;
    let m = MapFile::open(path)?;
    let d = chunk(&m, 140)?;
    let bad = || s4_formats::Error::Format("save: chunk 140 too short".into());
    let rd = |at: usize| d.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(bad);
    let n = rd(0)? as usize;
    let mut out = Vec::new();
    let mut o = 8;
    for _ in 0..n.min(8) {
        if rd(o)? != 0x7623_4526 || rd(o + 4)? != 0x70 {
            return Err(s4_formats::Error::Format("save: chunk 140 player record".into()));
        }
        o += 8;
        let w: Vec<u32> = (0..28).map(|k| rd(o + 4 * k)).collect::<s4_formats::Result<_>>()?;
        o += 0x70 + 4;
        let tail = [rd(o)?, rd(o + 4)?];
        o += 8;
        let magic = crate::magic::ManaPool {
            mana: (w[1] ^ keys::MANA) as i32,
            gained: (w[2] ^ keys::GAINED) as i32,
            costs: std::array::from_fn(|i| (w[4 + i] ^ keys::cost(i)) as i32),
            casts: std::array::from_fn(|i| w[12 + i] ^ keys::casts(i)),
        };
        let cache = std::array::from_fn(|i| w[20 + i] as i32);
        out.push(SavedPlayer { race: 0, color: 0, magic, flags: w[0], unknown_c: w[3] ^ keys::UNKNOWN_C, cache, tail });
    }
    for p in out.iter_mut() {
        p.race = rd(o)?;
        p.color = rd(o + 4)?;
        o += 8;
    }
    Ok(out)
}

/// Chunk 133, `CEndStatistic` (store 0x4f7bc0, after a `Statistic::Update`; load 0x4f7b20): u32
/// 0x2d6ebb9b, version 4, 0x1128, the nine players' `CStatistic` blocks (0x1128 bytes each, the
/// layout in [`crate::statistic`]), u32 0x2d6ebbfb. Returns the blocks as dwords, players 0..=8.
pub fn statistic(path: impl AsRef<std::path::Path>) -> s4_formats::Result<Vec<Vec<i32>>> {
    let m = MapFile::open(path)?;
    let d = chunk(&m, 133)?;
    let rd = |at: usize| d.get(at..at + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
    if rd(0) != Some(0x2d6e_bb9b) || rd(4) != Some(4) || rd(8) != Some(0x1128) || d.len() != 12 + 9 * 0x1128 + 4 {
        return Err(s4_formats::Error::Format("save: chunk 133 header".into()));
    }
    Ok((0..9).map(|p| (0..0x1128 / 4).map(|k| rd(12 + p * 0x1128 + 4 * k).unwrap_or(0)).collect()).collect())
}

/// Chunk 137, `CStatisticArchive` (store 0x4f9240): u32 0x2d6ecc9b, version 1, the dimensions
/// 9, 0x31, 16, 16, 16, then the three rings (`[player][category][slot]` dwords) and 0xc4 bytes.
pub fn statistic_archive(path: impl AsRef<std::path::Path>) -> s4_formats::Result<[Vec<i32>; 3]> {
    let m = MapFile::open(path)?;
    let d = chunk(&m, 137)?;
    let rd = |at: usize| d.get(at..at + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
    let head: Vec<Option<i32>> = (0..7).map(|k| rd(4 * k)).collect();
    let want = [0x2d6e_cc9b, 1, 9, 0x31, 16, 16, 16].map(Some);
    let ring = 9 * 0x31 * 16;
    if head != want || d.len() != 28 + 3 * 4 * ring + 0xc4 {
        return Err(s4_formats::Error::Format("save: chunk 137 header".into()));
    }
    Ok(std::array::from_fn(|r| (0..ring).map(|k| rd(28 + 4 * (r * ring + k)).unwrap_or(0)).collect()))
}

/// The three `9 × 67` tables `CSettlerMgr::Store` (0x582040) writes after the settlers into the
/// text stream of chunk 161, in stream order (the last `3 · 603` numbers of the chunk).
pub fn settler_mgr_tables(path: impl AsRef<std::path::Path>) -> s4_formats::Result<[Vec<i32>; 3]> {
    let m = MapFile::open(path)?;
    let d = chunk(&m, 161)?;
    let text = String::from_utf8_lossy(&d);
    let nums: Vec<i32> = text.split_ascii_whitespace().filter_map(|t| t.parse().ok()).collect();
    let n = 9 * 67;
    if nums.len() < 3 * n {
        return Err(s4_formats::Error::Format("save: chunk 161 too short".into()));
    }
    let tail = &nums[nums.len() - 3 * n..];
    Ok(std::array::from_fn(|k| tail[k * n..(k + 1) * n].to_vec()))
}

/// `CBuildingMgr::Load` (chunk 162): the first `9 × 83` table after the header and the count —
/// [inf] the list heads (0x14eea74 `[player · 83 + type]`).
pub fn building_list_heads(path: impl AsRef<std::path::Path>) -> s4_formats::Result<Vec<i64>> {
    let m = MapFile::open(path)?;
    let d = chunk(&m, 162)?;
    let toks = tokens(&d);
    let start = 1 + 9 * 22 + 1;
    if toks.len() < start + 9 * 83 {
        return Err(s4_formats::Error::Format("save: chunk 162 too short".into()));
    }
    Ok(toks[start..start + 9 * 83].iter().map(|t| num(t).unwrap_or(-1)).collect())
}
