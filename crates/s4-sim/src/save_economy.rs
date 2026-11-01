//! Savegame readers for the economy: pile records (chunk 163, `CPileMgr::Store`) and eco-sector
//! records (chunk 180, `CEcoSectorMgr::Store`). Text streams of numbers like the other entity
//! chunks (see `save`).
//!
//! A pile (`CPile::Store` release vtbl+4 0x56f3d0 and its bases): class id 3; `IEntity` version 2, id, type,
//! packed xy, health, obj type 16, tribe, flags, unique id; `IAnimatedEntity` version 1, frame,
//! attacker, job part, two list links, last update tick, logic slot, event count + 7 tokens per
//! event; `CPileObserverList` version 1, count, (entity, delta, target) per observer; `CPile`
//! version 1, good, amount, leaving, coming, role, patch, variant, offset x, offset y, building,
//! offer flag; then the role (class id, `IPileRole` version 1 + pile id, the role's own fields).
//!
//! An eco-sector (`CEcoSector::Store`): the two priority objects, control id -823895864, version
//! 7, unique id, size, owner, +19, +20, +21, +22, +24, +26, +28, +29, +30, min carriers,
//! carriers with job order, a world index (its eco-sector is the record's id); arrays are written
//! as (element size, count, values): settler counts (67), settler offer heads (67), good offer
//! heads (43); 43 request queues (count, ids); supply counters (42), specialist wishes (5), worker
//! deltas (2); the full-pile vectors (count, then per vector its count and ids); goods on piles
//! (43), goods carried (43), soldier orders (10); the order vectors and counters (not read).

/// One saved pile.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SavedPile {
    pub id: u16,
    pub pos: (i32, i32),
    pub flags: u32,
    /// The two `IAnimatedEntity` list links in store order.
    pub links: (u16, u16),
    pub events: u32,
    /// (entity, delta, target).
    pub observers: Vec<(u16, i8, u8)>,
    pub good: u8,
    pub amount: u8,
    pub leaving: u8,
    pub coming: u8,
    pub role: u8,
    pub patch: u8,
    pub variant: u8,
    pub offset: (i16, i16),
    pub building: u16,
    pub offer: bool,
    pub role_class: i64,
}

/// One saved eco-sector (the fields the oracle uses).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SavedEco {
    pub unique_id: u32,
    pub size: u32,
    pub owner: u8,
    pub min_carrier: u16,
    pub carriers_with_job_order: u16,
    /// +8: a node of the eco-sector (`CalculateStoreWorldIndices`: centre of its first tile).
    pub world_index: i64,
    pub settlers: Vec<u16>,
    pub settler_offer: Vec<u16>,
    pub good_offer: Vec<u16>,
    pub requests: Vec<Vec<u16>>,
    pub goods_on_piles: Vec<u16>,
    pub goods_on_carriers: Vec<u16>,
    /// `CGoodTransportPriority`: rank per good (43), rank count.
    pub transport_prio: Vec<u16>,
    pub transport_prio_count: i64,
    /// `CBuildingSupplyPriority`: (building type, [(good, share)]) in map order (empty when the
    /// record's start could not be found).
    pub supply: SavedSupply,
}

/// A saved supply map: (building type, [(good, share)]) in map order.
pub type SavedSupply = Vec<(i64, Vec<(i64, i64)>)>;

fn numbers(text: &[u8]) -> Vec<i64> {
    text.split(|c| c.is_ascii_whitespace() || *c == 0)
        .filter(|w| !w.is_empty())
        .map(|w| std::str::from_utf8(w).ok().and_then(|s| s.parse::<i64>().ok()).unwrap_or(i64::MIN))
        .collect()
}

const PILE_CLASS: i64 = 3;
const EVENT_CLASS: i64 = 66;
const ECO_CONTROL: i64 = -823_895_864;

fn is_pile_header(t: &[i64], k: usize) -> bool {
    t.get(k) == Some(&PILE_CLASS) && t.get(k + 1) == Some(&2) && t.get(k + 6) == Some(&16) && t.get(k + 3) == Some(&0)
}

/// Chunk 163: (piles, declared count, records that failed to decode).
pub fn parse_piles(text: &[u8]) -> PileRecords {
    let t = numbers(text);
    let declared = t.get(1).copied().unwrap_or(0).max(0) as u32;
    let (mut out, mut failed) = (Vec::new(), 0);
    let mut k = 2;
    while k < t.len() {
        if !is_pile_header(&t, k) {
            k += 1;
            continue;
        }
        match parse_pile(&t, k) {
            Some(p) => out.push(p),
            None => failed += 1,
        }
        k += 1;
    }
    (out, declared, failed)
}

fn parse_pile(t: &[i64], k: usize) -> Option<SavedPile> {
    let g = |i: usize| t.get(i).copied();
    let (id, packed, flags) = (g(k + 2)?, g(k + 4)?, g(k + 8)?);
    let mut p = k + 10;
    if g(p)? != 1 {
        return None;
    }
    let links = (g(p + 4)? as u16, g(p + 5)? as u16);
    let nev = g(p + 8)?.max(0) as usize;
    p += 9;
    for e in 0..nev {
        if g(p + 7 * e)? != EVENT_CLASS {
            return None;
        }
    }
    p += 7 * nev;
    if g(p)? != 1 {
        return None;
    }
    let nobs = g(p + 1)?.clamp(0, 8) as usize;
    p += 2;
    let mut observers = Vec::new();
    for o in 0..nobs {
        observers.push((g(p + 3 * o)? as u16, g(p + 3 * o + 1)? as i8, g(p + 3 * o + 2)? as u8));
    }
    p += 3 * nobs;
    if g(p)? != 1 {
        return None;
    }
    let f = |i: usize| g(p + 1 + i);
    let pile = SavedPile {
        id: id as u16,
        pos: ((packed & 0xFFFF) as i32, (packed >> 16) as i32),
        flags: flags as u32,
        links,
        events: nev as u32,
        observers,
        good: f(0)? as u8,
        amount: f(1)? as u8,
        leaving: f(2)? as u8,
        coming: f(3)? as u8,
        role: f(4)? as u8,
        patch: f(5)? as u8,
        variant: f(6)? as u8,
        offset: (f(7)? as i16, f(8)? as i16),
        building: f(9)? as u16,
        offer: f(10)? != 0,
        role_class: f(11)?,
    };
    // IPileRole: version 1, the pile id.
    (f(12)? == 1 && f(13)? == id).then_some(pile)
}

/// Chunk 180: the eco-sector records that decoded.
pub fn parse_eco_sectors(text: &[u8]) -> Vec<SavedEco> {
    const TRAILER: i64 = 1_073_196_093;
    let t = numbers(text);
    let mut out = Vec::new();
    // Manager header: version 3, five ints, record count; records follow back to back, each
    // ending with the trailer id and four counters.
    let mut start = 7;
    for c in 0..t.len() {
        if t[c] == ECO_CONTROL
            && t.get(c + 1) == Some(&7)
            && let Some(mut e) = parse_eco(&t, c)
        {
            e.supply = parse_supply(&t, start, c).unwrap_or_default();
            out.push(e);
            if let Some(k) = (c..t.len()).find(|&k| t[k] == TRAILER) {
                start = k + 5;
            }
        }
    }
    out
}

/// The supply map at the record start `s` (eco class, supply class, version 1, count, entries),
/// which must end where the transport priority object (class, version 1, …) begins.
fn parse_supply(t: &[i64], s: usize, control: usize) -> Option<SavedSupply> {
    let g = |i: usize| t.get(i).copied();
    if g(s + 2)? != 1 {
        return None;
    }
    let n = g(s + 3)?.clamp(0, 100) as usize;
    let mut p = s + 4;
    let mut out = Vec::new();
    for _ in 0..n {
        let (key, m) = (g(p)?, g(p + 1)?.clamp(0, 100) as usize);
        let pairs = (0..m).map(|j| Some((g(p + 2 + 2 * j)?, g(p + 3 + 2 * j)?))).collect::<Option<Vec<_>>>()?;
        out.push((key, pairs));
        p += 2 + 2 * m;
    }
    (p + 49 == control).then_some(out)
}

fn parse_eco(t: &[i64], c: usize) -> Option<SavedEco> {
    if c < 49 {
        return None;
    }
    let g = |i: usize| t.get(i).copied();
    let mut p = c + 17;
    let arr = |p: &mut usize, n: usize| -> Option<Vec<u16>> {
        let (_size, count) = (g(*p)?, g(*p + 1)?);
        if count != n as i64 {
            return None;
        }
        let v = (0..n).map(|i| g(*p + 2 + i).map(|x| x as u16)).collect::<Option<Vec<_>>>()?;
        *p += 2 + n;
        Some(v)
    };
    let settlers = arr(&mut p, 67)?;
    let settler_offer = arr(&mut p, 67)?;
    let good_offer = arr(&mut p, 43)?;
    let mut requests = Vec::new();
    for _ in 0..43 {
        let n = g(p)?.max(0) as usize;
        requests.push((0..n).map(|i| g(p + 1 + i).map(|x| x as u16)).collect::<Option<Vec<_>>>()?);
        p += 1 + n;
    }
    arr(&mut p, 42)?;
    arr(&mut p, 5)?;
    arr(&mut p, 2)?;
    let full = g(p)?.max(0) as usize;
    p += 1;
    for _ in 0..full {
        let n = g(p)?.max(0) as usize;
        p += 1 + n;
    }
    let goods_on_piles = arr(&mut p, 43)?;
    let goods_on_carriers = arr(&mut p, 43)?;
    Some(SavedEco {
        unique_id: g(c + 2)? as u32,
        size: g(c + 3)? as u32,
        owner: g(c + 4)? as u8,
        min_carrier: g(c + 14)? as u16,
        carriers_with_job_order: g(c + 15)? as u16,
        world_index: g(c + 16)?,
        settlers,
        settler_offer,
        good_offer,
        requests,
        goods_on_piles,
        goods_on_carriers,
        transport_prio: (0..43).map(|i| g(c - 43 + i).map(|x| x as u16)).collect::<Option<Vec<_>>>()?,
        transport_prio_count: g(c - 46)?,
        supply: Vec::new(),
    })
}

/// [`parse_piles`]' result: piles, declared count, failed records.
pub type PileRecords = (Vec<SavedPile>, u32, u32);

/// Piles and eco-sectors of a savegame.
pub fn load(path: impl AsRef<std::path::Path>) -> s4_formats::Result<(PileRecords, Vec<SavedEco>)> {
    let m = s4_formats::map::MapFile::open(path)?;
    let get = |id| -> s4_formats::Result<Vec<u8>> {
        Ok(m.container.load_id(id, 0)?.map(|(_, d)| d).unwrap_or_default())
    };
    Ok((parse_piles(&get(163)?), parse_eco_sectors(&get(180)?)))
}
