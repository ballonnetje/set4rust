//! The datagram format. Our own (the original's DirectPlay/Storm framing is not reproduced; its
//! game payload is: per communication tick one message per player, 1054 with the `CNet_Event`s or
//! 1055 "nothing", `CGameHost::AllSend`). Little-endian, every datagram starts with [`MAGIC`] and
//! the session id, so stray packets from another game are dropped.

use anyhow::{Result, bail};
use s4_sim::NetMessage;

/// The protocol version is its last byte, so builds of different versions never meet: 2 since a
/// command may carry up to 1000 entity ids (the selection cap, 2026-10-23); 3 since the turn
/// packets carry the departed players and the settings the AI level (2026-10-24); 4 since dumps
/// come in parts (2026-10-24).
pub const MAGIC: [u8; 4] = *b"S4N4";
/// The largest datagram we build: under the usual 1500-byte MTU so nothing is fragmented; the
/// sender splits its turns across datagrams.
pub const MAX_DATAGRAM: usize = 1200;

/// One player's input for one communication tick (`CNet_Event`s of a 1054 message, or none =
/// 1055).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub tick: u32,
    pub player: u8,
    pub messages: Vec<NetMessage>,
}

/// Lobby settings the host sends with the roster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameSettings {
    /// Map path or name, as the peers resolve it.
    pub map: String,
    /// Network delay in ms (`g_pGameType+660`); `dt` = delay / 71.
    pub delay_ms: u32,
    /// Ticks between state-hash exchanges.
    pub hash_interval: u32,
    /// [`crate::map_hash`] of the host's map file; a client with another file is rejected.
    pub map_hash: u64,
    /// The map settings' choices (`CStateLobbyMapSettings` 0x520c90 event 0x40 → GameState):
    /// team count (+0x40; 0 = the lobby's old 8), game mode (+0x344: 1 conflict, 2 cooperation,
    /// 3 economic, 4 head-to-head, 5 free settle) and goods (+0x38: 1 small .. 3 large).
    pub teams: u8,
    pub mode: u8,
    pub goods: u8,
    /// Game mode 3's seven goods (GameState +0x2fc..+0x302): the host's lobby draws them
    /// (0x51c1a0 → 0x51f5c0) and its player-info message carries them to the clients (+0x62,
    /// read by 0x5a2370).
    pub race_goods: [u8; 7],
    /// A loaded network save (0 / 0: a new game): its tick and world state hash. Every peer
    /// loads its own save of that game (`<name> - player n`) and the lockstep restarts there.
    pub load_tick: u32,
    pub load_hash: u64,
    /// The host's AI level (0x11b5948: 0 easy, else normal): a departed player becomes a
    /// computer of control `2 + (ai_level != 0)` (0x5a2da0).
    pub ai_level: u8,
}

/// A game as discovery lists it (dialog 2 row: name, map, joined / max players).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameInfo {
    pub name: String,
    pub map: String,
    pub joined: u8,
    pub max: u8,
    /// A loaded game's world hash (`GameSettings::load_hash`; 0: a new game): the joining
    /// peer looks for its own save of it.
    pub load_hash: u64,
    /// The host's player id (1 unless a loaded game makes it another).
    pub host: u8,
}

/// One lobby slot (a row of dialog 4): the host's player data for that player id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlotInfo {
    /// `crate::lobby::SLOT_OPEN` / `SLOT_HUMAN` / `SLOT_AI` / `SLOT_CLOSED`.
    pub kind: u8,
    pub name: String,
    pub race: u8,
    pub team: u8,
    pub colour: u8,
    pub ready: bool,
    /// A computer player's level (GameState +0x6c: 2 easy → 0, 3 normal → 1).
    pub level: u8,
    /// The map's slot is not free (+0x3ba clear: control 1 human-only or 2 computer-only), so
    /// its type does not cycle (`SetLobbyState` 0x57 only toggles a computer's level).
    pub fixed: bool,
    /// The logo index (GameState +0x168: 0..8 into each machine's own logo list,
    /// `s4_formats::logo`; default the slot index, reset 0x506320).
    pub logo: u8,
    /// The map leaves the slot's race free (+0x3c3: the map's race 255).
    pub race_free: bool,
    /// A network host's slot type clicks on a slot a human is in (`crate::lobby::cycle_slot`):
    /// 0..=3 stand for the release's +0x6c 1, 2, 3, 0.
    pub human_steps: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Packet {
    /// Discovery: anyone hosting? (broadcast, session 0)
    Query,
    /// A host's answer to [`Packet::Query`].
    Game { info: GameInfo },
    /// Host → clients: the slot table (the original's player data, message 1032).
    Roster { slots: Vec<SlotInfo> },
    /// Client → host: change our slot's `field` (`ChangeData`, message 1052).
    Change { slot: u8, field: u8, up: bool },
    /// Chat: client → host (1050), host → all (1036, the line with the sender's name).
    Chat { text: String },
    /// Client → host: the game is initialised (1018).
    Inited { player: u8 },
    /// Host → all: tick (1019).
    Go,
    /// Client → host: join request, with the hash of the client's copy of the map; for a loaded
    /// game the world hash of the client's save and the player id it was saved as (0 / 0: none).
    Hello { name: String, map_hash: u64, save_hash: u64, want: u8 },
    /// Host → client: not admitted (e.g. a different map file).
    Reject { reason: String },
    /// Host → client: admitted as player `you`, with the game's settings.
    Welcome { you: u8, settings: GameSettings },
    /// Host → all: start ticking. `players`: the final roster, (player, name, address as the
    /// host sees it; empty for the host itself), so every client knows all players and can reach
    /// the others (full mesh).
    Start { players: Vec<(u8, String, String)> },
    /// Turns of `player` (each peer resends its unacknowledged turns until acked; a departed
    /// player's turns before it left are relayed), plus the sender's ack: for every player the
    /// highest tick up to which it holds all turns; and the departed players the sender knows of
    /// (player, first missing communication tick).
    Turns { from: u8, turns: Vec<Turn>, acks: Vec<(u8, u32)>, left: Vec<(u8, u32)> },
    /// The sender's state hash after `tick` (`CStateGame`: 0xFA6 with `CRCLogicUpdate`).
    Hash { from: u8, tick: u32, hash: u64 },
    /// After a mismatch: the sender's per-subsystem hashes after `tick`, entries `first ..` of
    /// `total` (split under [`MAX_DATAGRAM`], [`dump_packets`]).
    Dump { from: u8, tick: u32, first: u16, total: u16, parts: Vec<(String, u64)> },
    /// Leaving.
    Bye { from: u8 },
}

/// A dump as datagrams of at most [`MAX_DATAGRAM`] bytes each (names cut to 200 bytes).
pub fn dump_packets(from: u8, tick: u32, parts: &[(String, u64)]) -> Vec<Packet> {
    // Header: magic, session, kind, from, tick, first, total, count; an entry: name + hash.
    const HEAD: usize = 4 + 4 + 1 + 1 + 4 + 2 + 2 + 2;
    let total = parts.len().min(u16::MAX as usize) as u16;
    let mut out = Vec::new();
    let mut chunk: Vec<(String, u64)> = Vec::new();
    let (mut first, mut size) = (0u16, HEAD);
    for (i, (n, h)) in parts.iter().take(total as usize).enumerate() {
        let mut k = n.len().min(200);
        while !n.is_char_boundary(k) {
            k -= 1;
        }
        let e = 2 + k + 8;
        if !chunk.is_empty() && size + e > MAX_DATAGRAM {
            out.push(Packet::Dump { from, tick, first, total, parts: std::mem::take(&mut chunk) });
            (first, size) = (i as u16, HEAD);
        }
        size += e;
        chunk.push((n[..k].to_string(), *h));
    }
    out.push(Packet::Dump { from, tick, first, total, parts: chunk });
    out
}

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        let b = s.as_bytes();
        self.u16(b.len().min(u16::MAX as usize) as u16);
        self.0.extend_from_slice(&b[..b.len().min(u16::MAX as usize)]);
    }
    fn msg(&mut self, m: &NetMessage) {
        self.u32(m.kind);
        self.u8(m.player);
        self.u32(m.wparam);
        self.u32(m.lparam);
        self.u16(m.data.len() as u16);
        for &d in &m.data {
            self.u16(d);
        }
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if self.0.len() < n {
            bail!("truncated packet");
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn str(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn msg(&mut self) -> Result<NetMessage> {
        let kind = self.u32()?;
        let player = self.u8()?;
        let wparam = self.u32()?;
        let lparam = self.u32()?;
        let n = self.u16()? as usize;
        let data = (0..n).map(|_| self.u16()).collect::<Result<Vec<_>>>()?;
        Ok(NetMessage { kind, player, wparam, lparam, data })
    }
}

impl Packet {
    pub fn encode(&self, session: u32) -> Vec<u8> {
        let mut w = W(Vec::with_capacity(64));
        w.0.extend_from_slice(&MAGIC);
        w.u32(session);
        match self {
            Packet::Hello { name, map_hash, save_hash, want } => {
                w.u8(1);
                w.str(name);
                w.u64(*map_hash);
                w.u64(*save_hash);
                w.u8(*want);
            }
            Packet::Reject { reason } => {
                w.u8(9);
                w.str(reason);
            }
            Packet::Welcome { you, settings } => {
                w.u8(2);
                w.u8(*you);
                w.str(&settings.map);
                w.u32(settings.delay_ms);
                w.u32(settings.hash_interval);
                w.u64(settings.map_hash);
                w.u8(settings.teams);
                w.u8(settings.mode);
                w.u8(settings.goods);
                for &g in &settings.race_goods {
                    w.u8(g);
                }
                w.u32(settings.load_tick);
                w.u64(settings.load_hash);
                w.u8(settings.ai_level);
            }
            Packet::Start { players } => {
                w.u8(4);
                w.u8(players.len() as u8);
                for (p, n, a) in players {
                    w.u8(*p);
                    w.str(n);
                    w.str(a);
                }
            }
            Packet::Turns { from, turns, acks, left } => {
                w.u8(5);
                w.u8(*from);
                w.u8(acks.len() as u8);
                for &(p, t) in acks {
                    w.u8(p);
                    w.u32(t);
                }
                w.u8(left.len() as u8);
                for &(p, t) in left {
                    w.u8(p);
                    w.u32(t);
                }
                w.u16(turns.len() as u16);
                for t in turns {
                    w.u32(t.tick);
                    w.u8(t.player);
                    w.u16(t.messages.len() as u16);
                    for m in &t.messages {
                        w.msg(m);
                    }
                }
            }
            Packet::Hash { from, tick, hash } => {
                w.u8(6);
                w.u8(*from);
                w.u32(*tick);
                w.u64(*hash);
            }
            Packet::Dump { from, tick, first, total, parts } => {
                w.u8(7);
                w.u8(*from);
                w.u32(*tick);
                w.u16(*first);
                w.u16(*total);
                w.u16(parts.len() as u16);
                for (n, h) in parts {
                    w.str(n);
                    w.u64(*h);
                }
            }
            Packet::Bye { from } => {
                w.u8(8);
                w.u8(*from);
            }
            Packet::Query => w.u8(10),
            Packet::Game { info } => {
                w.u8(11);
                w.str(&info.name);
                w.str(&info.map);
                w.u8(info.joined);
                w.u8(info.max);
                w.u64(info.load_hash);
                w.u8(info.host);
            }
            Packet::Roster { slots } => {
                w.u8(12);
                w.u8(slots.len() as u8);
                for s in slots {
                    w.u8(s.kind);
                    w.str(&s.name);
                    w.u8(s.race);
                    w.u8(s.team);
                    w.u8(s.colour);
                    w.u8(s.ready as u8);
                    w.u8(s.level);
                    w.u8(s.fixed as u8);
                    w.u8(s.logo);
                    w.u8(s.race_free as u8);
                    w.u8(s.human_steps);
                }
            }
            Packet::Change { slot, field, up } => {
                w.u8(13);
                w.u8(*slot);
                w.u8(*field);
                w.u8(*up as u8);
            }
            Packet::Chat { text } => {
                w.u8(14);
                w.str(text);
            }
            Packet::Inited { player } => {
                w.u8(15);
                w.u8(*player);
            }
            Packet::Go => w.u8(16),
        }
        w.0
    }

    /// Decode a datagram; returns the session id and the packet.
    pub fn decode(b: &[u8]) -> Result<(u32, Packet)> {
        let mut r = R(b);
        if r.take(4)? != MAGIC {
            bail!("not an s4-net packet");
        }
        let session = r.u32()?;
        let p = match r.u8()? {
            1 => Packet::Hello { name: r.str()?, map_hash: r.u64()?, save_hash: r.u64()?, want: r.u8()? },
            9 => Packet::Reject { reason: r.str()? },
            2 => {
                let you = r.u8()?;
                let settings = GameSettings { map: r.str()?, delay_ms: r.u32()?, hash_interval: r.u32()?, map_hash: r.u64()?, teams: r.u8()?, mode: r.u8()?, goods: r.u8()?, ..Default::default() };
                let mut settings = settings;
                for g in settings.race_goods.iter_mut() {
                    *g = r.u8()?;
                }
                settings.load_tick = r.u32()?;
                settings.load_hash = r.u64()?;
                settings.ai_level = r.u8()?;
                Packet::Welcome { you, settings }
            }
            4 => {
                let n = r.u8()?;
                Packet::Start { players: (0..n).map(|_| Ok((r.u8()?, r.str()?, r.str()?))).collect::<Result<Vec<_>>>()? }
            }
            5 => {
                let from = r.u8()?;
                let na = r.u8()?;
                let acks = (0..na).map(|_| Ok((r.u8()?, r.u32()?))).collect::<Result<Vec<_>>>()?;
                let nl = r.u8()?;
                let left = (0..nl).map(|_| Ok((r.u8()?, r.u32()?))).collect::<Result<Vec<_>>>()?;
                let nt = r.u16()?;
                let mut turns = Vec::with_capacity(nt as usize);
                for _ in 0..nt {
                    let tick = r.u32()?;
                    let player = r.u8()?;
                    let nm = r.u16()?;
                    let messages = (0..nm).map(|_| r.msg()).collect::<Result<Vec<_>>>()?;
                    turns.push(Turn { tick, player, messages });
                }
                Packet::Turns { from, turns, acks, left }
            }
            6 => Packet::Hash { from: r.u8()?, tick: r.u32()?, hash: r.u64()? },
            7 => {
                let from = r.u8()?;
                let tick = r.u32()?;
                let first = r.u16()?;
                let total = r.u16()?;
                let n = r.u16()?;
                let parts = (0..n).map(|_| Ok((r.str()?, r.u64()?))).collect::<Result<Vec<_>>>()?;
                Packet::Dump { from, tick, first, total, parts }
            }
            8 => Packet::Bye { from: r.u8()? },
            10 => Packet::Query,
            11 => Packet::Game { info: GameInfo { name: r.str()?, map: r.str()?, joined: r.u8()?, max: r.u8()?, load_hash: r.u64()?, host: r.u8()? } },
            12 => {
                let n = r.u8()?;
                let slots = (0..n)
                    .map(|_| Ok(SlotInfo { kind: r.u8()?, name: r.str()?, race: r.u8()?, team: r.u8()?, colour: r.u8()?, ready: r.u8()? != 0, level: r.u8()?, fixed: r.u8()? != 0, logo: r.u8()?, race_free: r.u8()? != 0, human_steps: r.u8()? }))
                    .collect::<Result<Vec<_>>>()?;
                Packet::Roster { slots }
            }
            13 => Packet::Change { slot: r.u8()?, field: r.u8()?, up: r.u8()? != 0 },
            14 => Packet::Chat { text: r.str()? },
            15 => Packet::Inited { player: r.u8()? },
            16 => Packet::Go,
            k => bail!("unknown packet kind {k}"),
        };
        if !r.0.is_empty() {
            bail!("{} trailing bytes", r.0.len());
        }
        Ok((session, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let m = NetMessage { kind: 5025, player: 2, wparam: 7, lparam: 0x0010_0020, data: vec![3, 4] };
        let ps = vec![
            Packet::Hello { name: "a".into(), map_hash: 99, save_hash: 0x1234_5678_9abc, want: 3 },
            Packet::Reject { reason: "map".into() },
            Packet::Welcome {
                you: 2,
                settings: GameSettings { map: "Campaign/roman02".into(), delay_ms: 284, hash_interval: 64, map_hash: 5, teams: 2, mode: 3, goods: 1, race_goods: [3, 4, 7, 8, 9, 10, 11], load_tick: 504, load_hash: 0xdead_beef_0102, ai_level: 1 },
            },
            Packet::Start { players: vec![(1, "host".into(), String::new()), (2, "a".into(), "127.0.0.1:5000".into()), (3, "b".into(), "10.0.0.2:7".into())] },
            Packet::Turns { from: 2, turns: vec![Turn { tick: 8, player: 2, messages: vec![m] }, Turn { tick: 10, player: 2, messages: vec![] }], acks: vec![(1, 6), (2, 10)], left: vec![(3, 12)] },
            Packet::Hash { from: 1, tick: 64, hash: 0xdead_beef_0123_4567 },
            Packet::Dump { from: 1, tick: 64, first: 3, total: 5, parts: vec![("rng".into(), 1), ("entities".into(), 2)] },
            Packet::Bye { from: 2 },
            Packet::Query,
            Packet::Game { info: GameInfo { name: "g".into(), map: "m".into(), joined: 1, max: 4, load_hash: 77, host: 2 } },
            Packet::Roster { slots: vec![SlotInfo { kind: 2, name: "a".into(), race: 4, team: 2, colour: 3, ready: true, level: 1, fixed: true, logo: 6, race_free: true, human_steps: 2 }, SlotInfo::default()] },
            Packet::Change { slot: 1, field: 5, up: true },
            Packet::Chat { text: "hi".into() },
            Packet::Inited { player: 2 },
            Packet::Go,
        ];
        for p in ps {
            let b = p.encode(77);
            assert_eq!(Packet::decode(&b).unwrap(), (77, p));
        }
        assert!(Packet::decode(b"XXXX\0\0\0\0\x01").is_err());
        assert!(Packet::decode(&Packet::Start { players: vec![] }.encode(1)[..8]).is_err());
    }

    /// A dump of 300 long entries goes as several datagrams, each under `MAX_DATAGRAM`, that
    /// put the whole dump back together.
    #[test]
    fn dumps_come_in_parts() {
        let parts: Vec<(String, u64)> = (0..300).map(|i| (format!("subsystem {i} {}", "x".repeat(i % 40)), i as u64 * 7)).collect();
        let packets = dump_packets(2, 640, &parts);
        assert!(packets.len() > 5, "{}", packets.len());
        let mut back: Vec<Option<(String, u64)>> = vec![None; 300];
        for p in &packets {
            let b = p.encode(9);
            assert!(b.len() <= MAX_DATAGRAM, "{}", b.len());
            let Ok((9, Packet::Dump { from: 2, tick: 640, first, total: 300, parts })) = Packet::decode(&b) else { panic!() };
            for (k, e) in parts.into_iter().enumerate() {
                back[first as usize + k] = Some(e);
            }
        }
        assert_eq!(back.into_iter().collect::<Option<Vec<_>>>(), Some(parts));
        // A long name is cut, at a char boundary.
        let p = dump_packets(1, 1, &[("ä".repeat(300), 1)]);
        let Packet::Dump { parts, .. } = &p[0] else { panic!() };
        assert_eq!(parts[0].0.len(), 200);
    }
}
