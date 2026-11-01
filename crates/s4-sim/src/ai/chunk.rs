//! Savegame chunk 150 (0x96, `SaveGame` 0x4feef0 writes `CAIMain` there): `CAIMain::Save`
//! (vtbl+4 of its chunk object, 0x438900) / `Load` (0x438790). Little-endian words through the
//! persistence object (+0x14 an int, +0x18 a tag, +0x10 raw bytes):
//!
//! 1. tag 0xa5901002 (0xa5901000 in older saves: without the next two words), 1, 0x1266b18;
//! 2. the regions (0x438060 / 0x437fd0): 0x10, 16 × (x, y, radius), then the 9 × 16 guard
//!    flag words (0x240 bytes);
//! 3. the player records (0x43c9b0 / 0x43c910 / 0x43c7a0): tag 0xa5967720, 9, then per record
//!    level, flags, the last variable index (0x16) and 4 columns of (last + 1) values;
//! 4. tag 0xa5901030, the player count, per AI player its id and its body (`CAIPlayerAI` save,
//!    0x43b580: tag 0xa5912000 … tag 0xa5912001), then count + 1 and tag 0xa5901001.

use super::vars::{COLUMNS, Record, VARS, default_columns};
use super::{Ai, PLAYERS, REGIONS, Region};
use crate::world::World;

pub const TAG_V2: u32 = 0xa590_1002;
pub const TAG_V0: u32 = 0xa590_1000;
pub const TAG_RECORDS: u32 = 0xa596_7720;
pub const TAG_PLAYERS: u32 = 0xa590_1030;
pub const TAG_END: u32 = 0xa590_1001;

/// One saved player record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedRecord {
    pub level: i32,
    pub flags: u32,
    /// The last variable index written (0x16 in every save).
    pub last: u32,
    pub columns: Vec<Vec<i32>>,
}

/// The decoded chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk150 {
    pub version: u32,
    /// The word after the version (1) and 0x1266b18; absent in version 0xa5901000.
    pub b18: Option<(u32, i32)>,
    pub regions: [Region; REGIONS],
    pub guard: [[u32; REGIONS]; PLAYERS],
    pub records: Vec<SavedRecord>,
    pub player_count: u32,
    /// (player, the words of its `CAIPlayerAI` subclass's save).
    pub players: Vec<(u32, Vec<u32>)>,
}

struct Words<'a> {
    w: &'a [u8],
    at: usize,
}

impl Words<'_> {
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.w.get(self.at..self.at + 4).ok_or_else(|| format!("chunk 150 ends at byte {}", self.at))?;
        self.at += 4;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&mut self) -> Result<i32, String> {
        self.u32().map(|v| v as i32)
    }
    fn tag(&mut self, t: u32) -> Result<(), String> {
        let v = self.u32()?;
        if v != t { Err(format!("chunk 150: tag {t:#x} expected at byte {}, found {v:#x}", self.at - 4)) } else { Ok(()) }
    }
    fn ranged(&mut self, lo: u32, hi: u32) -> Result<u32, String> {
        let v = self.u32()?;
        if v < lo || v > hi { Err(format!("chunk 150: {v} outside {lo}..={hi} at byte {}", self.at - 4)) } else { Ok(v) }
    }
}

/// Decode chunk 150.
pub fn parse(bytes: &[u8]) -> Result<Chunk150, String> {
    let mut r = Words { w: bytes, at: 0 };
    let version = r.u32()?;
    let b18 = match version {
        TAG_V2 => {
            let one = r.u32()?;
            Some((one, if one != 0 { r.i32()? } else { 0 }))
        }
        TAG_V0 => None,
        v => return Err(format!("chunk 150: version tag {v:#x}")),
    };
    r.ranged(0x10, 0x10)?;
    let mut regions = [Region::default(); REGIONS];
    for g in regions.iter_mut() {
        *g = Region { x: r.i32()?, y: r.i32()?, radius: r.i32()? };
    }
    let mut guard = [[0u32; REGIONS]; PLAYERS];
    for g in guard.iter_mut().flatten() {
        *g = r.u32()?;
    }
    r.tag(TAG_RECORDS)?;
    let n = r.ranged(0, 9)?;
    let mut records = Vec::new();
    for _ in 0..n {
        let level = r.ranged(0, 3)? as i32;
        let flags = r.u32()?;
        let last = r.ranged(0, 0x16)?;
        let mut columns = Vec::new();
        for _ in 0..COLUMNS {
            columns.push((0..=last).map(|_| r.i32()).collect::<Result<Vec<_>, _>>()?);
        }
        records.push(SavedRecord { level, flags, last, columns });
    }
    r.tag(TAG_PLAYERS)?;
    let player_count = r.u32()?;
    let mut players = Vec::new();
    loop {
        let p = r.u32()?;
        if p > player_count {
            break;
        }
        // The body's extent: decode it (`CAINormalPlayerAI` / `CAIDarkTribePlayerAI` load).
        let words: Vec<u32> = bytes[r.at..].as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
        let mut m = super::model::Rd { w: &words, at: 0 };
        super::model::PlayerBody::load(&mut m).map_err(|e| format!("chunk 150 player {p}: {e}"))?;
        players.push((p, words[..m.at].to_vec()));
        r.at += 4 * m.at;
    }
    r.tag(TAG_END)?;
    if r.at != bytes.len() {
        return Err(format!("chunk 150: {} bytes after the end tag", bytes.len() - r.at));
    }
    Ok(Chunk150 { version, b18, regions, guard, records, player_count, players })
}

/// Encode (`CAIMain::Save` order).
pub fn write(c: &Chunk150) -> Vec<u8> {
    let mut o: Vec<u32> = vec![c.version];
    if let Some((one, b18)) = c.b18 {
        o.push(one);
        if one != 0 {
            o.push(b18 as u32);
        }
    }
    o.push(0x10);
    for g in &c.regions {
        o.extend([g.x as u32, g.y as u32, g.radius as u32]);
    }
    o.extend(c.guard.iter().flatten());
    o.push(TAG_RECORDS);
    o.push(c.records.len() as u32);
    for r in &c.records {
        o.extend([r.level as u32, r.flags, r.last]);
        for col in &r.columns {
            o.extend(col.iter().map(|&v| v as u32));
        }
    }
    o.push(TAG_PLAYERS);
    o.push(c.player_count);
    for (p, body) in &c.players {
        o.push(*p);
        o.extend(body);
    }
    o.push(c.player_count + 1);
    o.push(TAG_END);
    o.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// The record as `Load` (0x43c7a0) makes it: missing values from the defaults.
pub fn record_of(s: &SavedRecord) -> Record {
    let mut columns = default_columns();
    for (k, col) in s.columns.iter().enumerate().take(COLUMNS) {
        for (i, &v) in col.iter().enumerate().take(VARS) {
            columns[k][i] = v;
        }
    }
    Record { level: s.level, flags: s.flags, columns }
}

/// `CAIMain::Load` (0x438790) into `world.ai`. A player body that does not decode is an error.
pub fn load(world: &mut World, c: &Chunk150) -> Result<(), String> {
    // `Load` runs `Init` first (vtbl+0xc).
    let mut ai = Ai { active: true, ..Ai::default() };
    ai.register_evaluation();
    // Players whose save holds no AI (version 0, or a Dark Tribe AI without its sector AI): the
    // load ends with their `Init` (vtbl+8, 0x43b4a0 / 0x43b0c0).
    let mut init = Vec::new();
    ai.b18 = c.b18.map_or(0, |b| b.1);
    ai.regions = c.regions;
    // 0x437fd0: the highest region with a radius.
    ai.top_region = (1..REGIONS).rev().find(|&i| c.regions[i].radius > 0).unwrap_or(0) as i32;
    ai.guard = c.guard;
    for (k, s) in c.records.iter().enumerate().take(PLAYERS) {
        ai.vars.records[k] = record_of(s);
    }
    for (p, body) in &c.players {
        let mut r = super::model::Rd { w: body, at: 0 };
        let decoded = super::model::PlayerBody::load(&mut r).map_err(|e| format!("player {p}: {e}"))?;
        if r.at != body.len() {
            return Err(format!("player {p}: {} words after the body", body.len() - r.at));
        }
        // A saved "no AI" (version 0) makes the AI anew (vtbl+8).
        let dark = world.race_of(*p as u8) == 3;
        let mut a = super::PlayerAi::new(*p as u8, dark);
        match decoded {
            Some((queue, b)) => {
                a.dark = b.dark.is_some();
                if a.dark && b.sectors.is_empty() {
                    init.push(*p as u8);
                }
                a.queue = queue;
                a.body = b;
                super::sched::rebuild(&mut a.sched, &mut a.body);
            }
            None => init.push(*p as u8),
        }
        if let Some(slot) = ai.players.get_mut(*p as usize) {
            *slot = Some(a);
            ai.mask |= 1 << p;
        }
    }
    ai.tick = world.ai.tick;
    // The infos, from the task forces' member lists (the release's load re-adds each member,
    // 0x444dc0).
    for (p, a) in ai.players.iter().enumerate() {
        let Some(a) = a else { continue };
        let p = p as u8;
        use super::player::TfRef;
        let mut add = |m: &[u16], r: TfRef| m.iter().for_each(|&id| {
            ai.infos.insert(id, r);
        });
        for (k, t) in a.body.reservoirs.iter().enumerate() {
            add(&t.members, TfRef::Reservoir(p, k as u8));
        }
        add(&a.body.squad.members, TfRef::Squad(p));
        for (s, sec) in a.body.sectors.iter().enumerate() {
            for (k, t) in sec.slots.iter().enumerate() {
                if let Some((_, t)) = t {
                    add(&t.members, TfRef::Slot(p, s as u8, k as u8));
                }
            }
            if let super::model::SectorKind::Normal { forces, .. } = &sec.kind {
                for (k, t) in forces.iter().enumerate() {
                    add(&t.members, TfRef::Force(p, s as u8, k as u8));
                }
            }
            for it in &sec.group.items {
                add(&it.tf.members, TfRef::Group(p, s as u8, it.uid));
            }
        }
    }
    world.ai = ai;
    super::taskforce::rebuild_timers(world);
    // The Dark Tribe sector AI's ctor makes its `CAIDarkTribe` from the map (0x440600 → 0x44b1c0).
    for p in 1..PLAYERS as u8 {
        if world.ai.players[p as usize].as_ref().is_some_and(|a| a.dark && !a.body.sectors.is_empty()) {
            let d = Box::new(super::dark::DarkTribe::new(world, p));
            if let Some(a) = world.ai.players[p as usize].as_mut() {
                a.dark_ai = Some(d);
            }
        }
    }
    // The end of each normal sector AI's load (0x443060): its eco-sector AIs anew (0x4439f0).
    for p in 1..PLAYERS as u8 {
        let n = world.ai.players[p as usize].as_ref().filter(|a| !a.dark).map_or(0, |a| a.body.sectors.len());
        for s in 0..n {
            super::sector::make_eco(world, p, s as u8);
        }
    }
    for p in init {
        super::player::init(world, p);
    }
    Ok(())
}

/// `CAIMain::Save` (0x438900) of `world.ai`.
pub fn save(world: &World) -> Chunk150 {
    let ai = &world.ai;
    let records = ai
        .vars
        .records
        .iter()
        .map(|r| SavedRecord { level: r.level, flags: r.flags, last: (VARS - 1) as u32, columns: r.columns.iter().map(|c| c.to_vec()).collect() })
        .collect();
    let player_count = world.player_count() as u32;
    let players = (1..=player_count)
        .filter_map(|p| {
            let a = ai.players.get(p as usize)?.as_ref()?;
            let mut w = Vec::new();
            a.body.save(&a.queue, &mut w);
            Some((p, w))
        })
        .collect();
    Chunk150 { version: TAG_V2, b18: Some((1, ai.b18)), regions: ai.regions, guard: ai.guard, records, player_count, players }
}
