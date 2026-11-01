//! Reader for the settler records of savegame chunk 161 (`CSettlerMgr::Save` → the persistence
//! stream of every `CSettler`), as far as the job code needs them. Used as an oracle: the saved
//! to-do list id, iterator position, role task and animation values must agree with the lists
//! [`super::JobData`] builds.
//!
//! The chunk is a whitespace-separated text stream (`operator^` writes numbers as text, floats
//! with a decimal point). One settler is (`CSettler::Store` release vtbl+4 0x580a70 and its bases):
//! * `CPersistence::Store`: class id (2 for `CSettler`);
//! * `IEntity::Store`: version 2, id, type, packed xy, health, obj type (1), tribe/player, flags,
//!   unique id;
//! * `IAnimatedEntity::Store`: version 1, frame, attacker player, job part, prev, next, last
//!   update tick, last logic update slot, event count + events (7 tokens each, class id 66);
//! * `IMovingEntity::Store`: version 1, direction, distance (+69), displacement costs, int +76,
//!   has-walking flag, then `CWalkingBase::Store` (magic 929995208, short version, short kind,
//!   16 ints, magic 1726811097);
//! * `CSettler`: version 1, list id (short, −1 = role-built list), the iterator position (only
//!   when the id is not −1), role id (`GetSettlerRole`, 23 → 24 for one subcase);
//! * the role: class id, `ISettlerRole::Store` version 2 with +4, +5, +6, +7, +8, +10, +11, +14,
//!   +16, +18, +24, +28, +32, +34, float +36, float +40, +20, +12; then the role's own fields
//!   (not decoded here: the reader skips to the next settler header).

/// The decoded part of one saved settler.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SavedSettler {
    pub id: u16,
    pub kind: u16,
    pub race: u8,
    pub owner: u8,
    pub pos: (i32, i32),
    pub flags: u32,
    /// `IAnimatedEntity`.
    pub frame: u8,
    pub job_part: u16,
    pub last_update_tick: u32,
    pub events: u32,
    /// `IMovingEntity`.
    pub direction: i8,
    pub distance: u8,
    pub displacement_costs: u8,
    /// `CWalkingBase` kind, if the settler has a walking object.
    pub walking_kind: Option<i32>,
    /// `CSettler`.
    pub list_id: i16,
    pub cursor: Option<u16>,
    pub role: u8,
    pub role_class: i64,
    /// `ISettlerRole` in store order (+4, +5, +6, +7, +8, +10, +11, +14, +16, +18, +24, +28, +32,
    /// +34, +36·10⁶, +40·10⁶, +20, +12). The two floats are kept as integers in millionths.
    pub role_fields: [i64; 18],
    /// The role's own fields after `ISettlerRole` (undecoded tokens up to the next settler).
    pub role_tail: Vec<i64>,
    /// `IAnimatedEntity::m_iEventQueue`: (event, type, data0, data1, data2).
    pub event_list: Vec<[i64; 5]>,
}

impl SavedSettler {
    pub fn task(&self) -> u8 {
        self.role_fields[0] as u8
    }
    pub fn counter(&self) -> i8 {
        self.role_fields[2] as i8
    }
    pub fn cycle_frames(&self) -> u8 {
        self.role_fields[3] as u8
    }
    pub fn home(&self) -> u16 {
        self.role_fields[12] as u16
    }
}

/// A token as an integer; decimals (floats) in millionths, truncated.
pub fn parse_number(t: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(t).ok()?;
    if let Ok(v) = s.parse::<i64>() {
        return Some(v);
    }
    let (neg, s) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
    let (int, frac) = s.split_once('.')?;
    let int: i64 = if int.is_empty() { 0 } else { int.parse().ok()? };
    let mut micro = 0i64;
    for (k, c) in frac.bytes().take(6).enumerate() {
        if !c.is_ascii_digit() {
            return None;
        }
        micro += (c - b'0') as i64 * 10i64.pow(5 - k as u32);
    }
    let v = int * 1_000_000 + micro;
    Some(if neg { -v } else { v })
}

const CLASS_SETTLER: i64 = 2;
const EVENT_CLASS: i64 = 66;
const WALK_BEGIN: i64 = 929_995_208;
const WALK_END: i64 = 1_726_811_097;

fn is_header(t: &[i64], k: usize) -> bool {
    k + 10 < t.len()
        && t[k] == CLASS_SETTLER
        && t[k + 1] == 2
        && (1..0xFFFF).contains(&t[k + 2])
        && (1..67).contains(&t[k + 3])
        && t[k + 6] == 1
}

/// Parse a chunk-161 stream. Records that do not decode are skipped (the reader resyncs on the
/// next settler header); the second value counts them.
pub fn parse(text: &[u8]) -> (Vec<SavedSettler>, usize) {
    let t: Vec<i64> = text
        .split(|c| c.is_ascii_whitespace() || *c == 0)
        .filter(|w| !w.is_empty())
        .map(|w| parse_number(w).unwrap_or(i64::MIN))
        .collect();
    let mut out = Vec::new();
    let mut failed = 0;
    let mut k = 0;
    while k < t.len() {
        if !is_header(&t, k) {
            k += 1;
            continue;
        }
        match parse_one(&t, k) {
            Some((mut s, end)) => {
                let mut q = end;
                while q < t.len() && !is_header(&t, q) {
                    q += 1;
                }
                s.role_tail = t[end..q].to_vec();
                out.push(s);
                k = q;
            }
            None => {
                failed += 1;
                k += 1;
            }
        }
        while k < t.len() && !is_header(&t, k) {
            k += 1;
        }
    }
    (out, failed)
}

fn parse_one(t: &[i64], k: usize) -> Option<(SavedSettler, usize)> {
    let g = |i: usize| t.get(i).copied();
    let (id, kind, packed, tribe, flags) = (g(k + 2)?, g(k + 3)?, g(k + 4)?, g(k + 7)?, g(k + 8)?);
    let mut p = k + 10;
    if g(p)? != 1 {
        return None;
    }
    let (frame, job_part, last_update_tick, nev) = (g(p + 1)?, g(p + 3)?, g(p + 6)?, g(p + 8)?);
    p += 9;
    let mut event_list = Vec::new();
    for e in 0..nev.max(0) as usize {
        let b = p + 7 * e;
        if g(b)? != EVENT_CLASS || g(b + 1)? != 1 {
            return None;
        }
        event_list.push([g(b + 2)?, g(b + 3)?, g(b + 4)?, g(b + 5)?, g(b + 6)?]);
    }
    p += 7 * nev.max(0) as usize;
    if g(p)? != 1 {
        return None;
    }
    let (direction, distance, displacement, has_walk) = (g(p + 1)?, g(p + 2)?, g(p + 3)?, g(p + 5)?);
    p += 6;
    let mut walking_kind = None;
    if has_walk != 0 {
        if g(p)? != WALK_BEGIN || g(p + 19)? != WALK_END {
            return None;
        }
        walking_kind = Some(g(p + 2)? as i32);
        p += 20;
    }
    if g(p)? != 1 {
        return None;
    }
    let list_id = g(p + 1)?;
    p += 2;
    let mut cursor = None;
    if list_id != -1 {
        cursor = Some(g(p)? as u16);
        p += 1;
    }
    let role = g(p)?;
    let role_class = g(p + 1)?;
    if g(p + 2)? != 2 {
        return None;
    }
    p += 3;
    let mut role_fields = [0i64; 18];
    for (n, f) in role_fields.iter_mut().enumerate() {
        *f = g(p + n)?;
    }
    p += 18;
    let pos = packed as u32;
    Some((
        SavedSettler {
            id: id as u16,
            kind: kind as u16,
            race: (tribe & 15) as u8,
            owner: (tribe >> 4) as u8,
            pos: ((pos & 0xFFFF) as i32, (pos >> 16) as i32),
            flags: flags as u32,
            frame: frame as u8,
            job_part: job_part as u16,
            last_update_tick: last_update_tick as u32,
            events: nev as u32,
            direction: direction as i8,
            distance: distance as u8,
            displacement_costs: displacement as u8,
            walking_kind,
            list_id: list_id as i16,
            cursor,
            role: role as u8,
            role_class,
            role_fields,
            role_tail: Vec::new(),
            event_list,
        },
        p,
    ))
}

/// Chunk 161 of a savegame file.
pub fn load(path: impl AsRef<std::path::Path>) -> s4_formats::Result<(Vec<SavedSettler>, usize)> {
    let m = s4_formats::map::MapFile::open(path)?;
    match m.container.load_id(161, 0)? {
        Some((_, d)) => Ok(parse(&d)),
        None => Err(s4_formats::Error::Format("save: chunk 161 missing".into())),
    }
}
