//! Reader for savegame chunk 166 (`CVehicleMgr` load 0x5967c0 / the vehicles' `Store`), used as
//! an oracle. The chunk is a text stream: version 1, the vehicle count, then per vehicle its
//! persistence class id and (`CVehicle::Store` 0x594520 after its bases):
//! * `IEntity`: version 2, id, type, packed xy, health, obj type (2 ship / 4 land), tribe/player,
//!   flags, unique id;
//! * `IAnimatedEntity`: version 1, frame, attacker, job part, prev, next, last update tick, last
//!   logic slot, event count + events (7 tokens each);
//! * `IMovingEntity`: version 1, direction, distance, displacement costs, int, has-walking flag,
//!   the `CWalkingBase` record (20 tokens: magic, version, kind, 16 ints, magic);
//! * `CVehicle`: 1, the passenger count (+0x90) and ids (+0x8c), 2, bytes +0x5c..=+0x65, int
//!   +0x68, the global bool 0x12dbc54, bytes +0x6c..=+0x6f, shorts +0x70..=+0x78, float +0x7c,
//!   byte +0x41, the task count and `CEntityTask`s (15 tokens each: class, version 1, +4..+0x15),
//!   short +0x66;
//! * the class's own fields (`CCart` 0x55b370, `CCatapult` 0x560ac0, ...), kept undecoded.

use crate::jobs::save_settler::parse_number;

#[derive(Clone, Debug, PartialEq)]
pub struct SavedVehicle {
    pub class: i64,
    pub id: u16,
    pub kind: u16,
    pub pos: (i32, i32),
    pub health: i64,
    pub obj_type: u8,
    pub race: u8,
    pub owner: u8,
    pub flags: u32,
    pub frame: i64,
    pub job_part: i64,
    pub prev: i64,
    pub next: i64,
    pub last_update_tick: i64,
    pub events: Vec<[i64; 5]>,
    pub direction: i64,
    pub displacement: i64,
    pub walking: Option<[i64; 18]>,
    pub passengers: Vec<i64>,
    /// +0x5c..=+0x65.
    pub b5c: [i64; 10],
    pub i68: i64,
    pub global_flag: i64,
    /// +0x6c..=+0x6f.
    pub b6c: [i64; 4],
    /// +0x70..=+0x78.
    pub s70: [i64; 5],
    pub f7c: i64,
    pub b41: i64,
    /// `CEntityTask` records (13 fields each after class and version).
    pub tasks: Vec<[i64; 13]>,
    pub s66: i64,
    /// The class's own fields.
    pub tail: Vec<i64>,
}

const WALK_BEGIN: i64 = 929_995_208;
const WALK_END: i64 = 1_726_811_097;

/// A vehicle header: `class, 2, id, type 1..=5, packed, health, obj type 2 / 4, tribe, flags, uid`.
fn is_header(t: &[i64], k: usize) -> bool {
    k + 10 < t.len() && t[k + 1] == 2 && (1..0xFFFF).contains(&t[k + 2]) && (1..=5).contains(&t[k + 3]) && (t[k + 6] == 2 || t[k + 6] == 4)
}

fn parse_one(t: &[i64], k: usize) -> Option<(SavedVehicle, usize)> {
    let g = |i: usize| t.get(i).copied();
    let mut p = k + 10;
    let (class, id, kind, packed, health, obj, tribe, flags) = (g(k)?, g(k + 2)?, g(k + 3)?, g(k + 4)?, g(k + 5)?, g(k + 6)?, g(k + 7)?, g(k + 8)?);
    if g(p)? != 1 {
        return None;
    }
    let (frame, job_part, prev, next, last, nev) = (g(p + 1)?, g(p + 3)?, g(p + 4)?, g(p + 5)?, g(p + 6)?, g(p + 8)?);
    p += 9;
    let mut events = Vec::new();
    for e in 0..nev.max(0) as usize {
        let b = p + 7 * e;
        events.push([g(b + 2)?, g(b + 3)?, g(b + 4)?, g(b + 5)?, g(b + 6)?]);
    }
    p += 7 * nev.max(0) as usize;
    if g(p)? != 1 {
        return None;
    }
    let (direction, displacement, has_walk) = (g(p + 1)?, g(p + 3)?, g(p + 5)?);
    p += 6;
    let mut walking = None;
    if has_walk != 0 {
        if g(p)? != WALK_BEGIN || g(p + 19)? != WALK_END {
            return None;
        }
        let mut w = [0; 18];
        for (i, v) in w.iter_mut().enumerate() {
            *v = g(p + 1 + i)?;
        }
        walking = Some(w);
        p += 20;
    }
    // CVehicle.
    if g(p)? != 1 {
        return None;
    }
    let n = g(p + 1)?;
    if !(0..=200).contains(&n) {
        return None;
    }
    let passengers = t.get(p + 2..p + 2 + n as usize)?.to_vec();
    p += 2 + n as usize;
    if g(p)? != 2 {
        return None;
    }
    p += 1;
    let mut b5c = [0; 10];
    for (i, v) in b5c.iter_mut().enumerate() {
        *v = g(p + i)?;
    }
    p += 10;
    let (i68, global_flag) = (g(p)?, g(p + 1)?);
    p += 2;
    let mut b6c = [0; 4];
    for (i, v) in b6c.iter_mut().enumerate() {
        *v = g(p + i)?;
    }
    p += 4;
    let mut s70 = [0; 5];
    for (i, v) in s70.iter_mut().enumerate() {
        *v = g(p + i)?;
    }
    p += 5;
    let (f7c, b41, nt) = (g(p)?, g(p + 1)?, g(p + 2)?);
    p += 3;
    if !(0..=1000).contains(&nt) {
        return None;
    }
    let mut tasks = Vec::new();
    for _ in 0..nt {
        if g(p + 1)? != 1 {
            return None;
        }
        let mut f = [0; 13];
        for (i, v) in f.iter_mut().enumerate() {
            *v = g(p + 2 + i)?;
        }
        tasks.push(f);
        p += 15;
    }
    let s66 = g(p)?;
    p += 1;
    let pos = packed as u32;
    Some((
        SavedVehicle {
            class,
            id: id as u16,
            kind: kind as u16,
            pos: ((pos & 0xFFFF) as i32, (pos >> 16) as i32),
            health,
            obj_type: obj as u8,
            race: (tribe & 15) as u8,
            owner: (tribe >> 4) as u8,
            flags: flags as u32,
            frame,
            job_part,
            prev,
            next,
            last_update_tick: last,
            events,
            direction,
            displacement,
            walking,
            passengers,
            b5c,
            i68,
            global_flag,
            b6c,
            s70,
            f7c,
            b41,
            tasks,
            s66,
            tail: Vec::new(),
        },
        p,
    ))
}

/// The `CWheeler` and `CCart` fields of a cart record's tail (load ctors 0x599fb0 / 0x55afd0):
/// `CWheeler` version 1, +0x94, +0x98, +0x9c; `CCart` version 1 or 2, bytes +0xb8..=+0xbb,
/// bools +0xbc / +0xbd, bytes +0xbe / +0xbf, int +0xc4, the build record again (bytes
/// +0x6c..=+0x6f, shorts +0x70..=+0x78, float +0x7c), bools +0x110..=+0x112, ints
/// +0x114..=+0x120, byte +0x124, the settlers (byte count, `{int, bool, int, int}`), the goods
/// (byte count, 4 ints), the slots +0xc8 (0x55f3d0: element size 12, count, `{good, amount,
/// int}`), and
/// with version 2 the bool +0x125.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedCart {
    pub wheeler: [i64; 3],
    pub version: i64,
    /// +0xb8..=+0xbf.
    pub b8: [i64; 8],
    pub c4: i64,
    pub b6c: [i64; 4],
    pub s70: [i64; 5],
    pub f7c: i64,
    /// +0x110..=+0x112.
    pub b110: [i64; 3],
    /// +0x114..=+0x120.
    pub i114: [i64; 4],
    pub b124: i64,
    pub settlers: Vec<[i64; 4]>,
    pub goods: Vec<[i64; 4]>,
    /// The slots' element size (12).
    pub slots_version: i64,
    pub slots: Vec<[i64; 3]>,
    pub b125: Option<i64>,
}

/// Decode a cart record's tail; `None` unless it is consumed exactly.
pub fn parse_cart(t: &[i64]) -> Option<SavedCart> {
    let g = |i: usize| t.get(i).copied();
    if g(0)? != 1 {
        return None;
    }
    let mut c = SavedCart { wheeler: [g(1)?, g(2)?, g(3)?], version: g(4)?, ..SavedCart::default() };
    if !(1..=2).contains(&c.version) {
        return None;
    }
    let mut p = 5;
    let mut take = |n: usize| -> Option<Vec<i64>> {
        let v = t.get(p..p + n)?.to_vec();
        p += n;
        Some(v)
    };
    c.b8.copy_from_slice(&take(8)?);
    c.c4 = take(1)?[0];
    c.b6c.copy_from_slice(&take(4)?);
    c.s70.copy_from_slice(&take(5)?);
    c.f7c = take(1)?[0];
    c.b110.copy_from_slice(&take(3)?);
    c.i114.copy_from_slice(&take(4)?);
    c.b124 = take(1)?[0];
    let n = take(1)?[0];
    for _ in 0..n {
        let v = take(4)?;
        c.settlers.push([v[0], v[1], v[2], v[3]]);
    }
    let n = take(1)?[0];
    for _ in 0..n {
        let v = take(4)?;
        c.goods.push([v[0], v[1], v[2], v[3]]);
    }
    c.slots_version = take(1)?[0];
    let n = take(1)?[0];
    if !(0..=64).contains(&n) {
        return None;
    }
    for _ in 0..n {
        let v = take(3)?;
        c.slots.push([v[0], v[1], v[2]]);
    }
    if c.version == 2 {
        c.b125 = Some(take(1)?[0]);
    }
    (p == t.len()).then_some(c)
}

/// The `CWheeler` and `CCatapult` fields of a catapult record's tail (load ctors 0x599fb0 /
/// 0x560980, the race class's 0x561820 / ...): `CWheeler` version 1, +0x94, +0x98, +0x9c;
/// `CCatapult` version 1, bool +0xb8, ints +0xbc / +0xc0; the race class's version 1. The
/// `CWarriorBehavior` part (+0xa0..+0xb4) is not stored.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedCatapult {
    pub wheeler: [i64; 3],
    /// +0xb8 it takes ammunition from piles next to it, +0xbc its ammunition carrier / donkey,
    /// +0xc0 the tick of the last ammunition / repair step.
    pub may_load: i64,
    pub carrier: i64,
    pub last_tick: i64,
}

/// Decode a catapult record's tail; `None` unless it is consumed exactly.
pub fn parse_catapult(t: &[i64]) -> Option<SavedCatapult> {
    if t.len() != 9 || t[0] != 1 || t[4] != 1 || t[8] != 1 {
        return None;
    }
    Some(SavedCatapult { wheeler: [t[1], t[2], t[3]], may_load: t[5], carrier: t[6], last_tick: t[7] })
}

/// Parse a chunk-166 stream: the vehicles and the count the chunk announces. Records that do not
/// decode are counted in the third value (the reader resyncs on the next header).
pub fn parse(text: &[u8]) -> (Vec<SavedVehicle>, i64, usize) {
    let t: Vec<i64> = text.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|w| !w.is_empty()).map(|w| parse_number(w).unwrap_or(i64::MIN)).collect();
    let announced = t.get(1).copied().unwrap_or(0);
    let (mut out, mut failed) = (Vec::new(), 0);
    let mut k = 2;
    while k < t.len() {
        if !is_header(&t, k) {
            k += 1;
            continue;
        }
        match parse_one(&t, k) {
            Some((mut v, end)) => {
                let mut q = end;
                while q < t.len() && !is_header(&t, q) {
                    q += 1;
                }
                v.tail = t[end..q].to_vec();
                out.push(v);
                k = q;
            }
            None => {
                failed += 1;
                k += 1;
            }
        }
    }
    (out, announced, failed)
}

/// Chunk 166 of a savegame file (`None` without one).
pub fn load(path: impl AsRef<std::path::Path>) -> s4_formats::Result<Option<(Vec<SavedVehicle>, i64, usize)>> {
    let m = s4_formats::map::MapFile::open(path)?;
    Ok(m.container.load_id(166, 0)?.map(|(_, d)| parse(&d)))
}
