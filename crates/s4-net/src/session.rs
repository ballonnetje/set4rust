//! UDP transport and lobby: a host waits for the players, assigns ids and starts the game,
//! telling every client the others' addresses; then every peer sends its turns to every other
//! peer (full mesh, like `CGameHost::SendToAll`), resending them until acknowledged, and
//! exchanges state hashes.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::lockstep::{Desync, Lockstep, dump_report};
use crate::relay::Link;
use crate::wire::{GameSettings, MAX_DATAGRAM, Packet, Turn};

/// What happened on the network during a [`Session::pump`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetEvent {
    /// The state hashes differ.
    Desync(Desync),
    /// Both peers' per-subsystem hashes for a desync are in: the report.
    DesyncReport(String),
    /// A player left (its Bye's departure tick, or the deciding peer's kick); its people go to
    /// the AI at that tick ([`crate::lockstep::PLAYER_LEFT`]).
    Left(u8),
    /// A player's turn is overdue (0x5a2a90 → 0x8d5).
    Waiting(u8),
    /// [ours] Through the relay: nothing from it for [`crate::relay::RELAY_LOST`]. Reported once
    /// per loss; the game itself goes on as with silent peers until [`NetEvent::Offline`].
    RelayLost,
    /// [ours] The relay stayed lost past the kick delay: every other player was dropped (each
    /// with its [`NetEvent::Left`], its people the AI's), and this peer plays on alone, sending and
    /// receiving nothing more (2509, shown once). docs/NETWORK.md.
    Offline,
}

/// Resend unacknowledged turns at most this often.
const RESEND: Duration = Duration::from_millis(15);

/// A subsystem's name and hash in a dump.
type Part = (String, u64);

/// Our last four hashes go out again this often.
const HASH_RESEND: Duration = Duration::from_millis(100);

/// Our dump after a desync goes out again this often, for this long.
const DUMP_RESEND: Duration = Duration::from_millis(500);
const DUMP_LINGER: Duration = Duration::from_secs(5);

/// A leaver waits at most this long for its turns to be acknowledged.
pub const LEAVE_LINGER: Duration = Duration::from_millis(1500);

pub struct Session {
    sock: Link,
    session: u32,
    pub local: u8,
    /// Other players' addresses.
    peers: HashMap<u8, SocketAddr>,
    /// The host: `Go` (1019), answered again to a client that repeats its `Inited`.
    start: Option<Packet>,
    pub roster: Vec<(u8, String)>,
    pub settings: GameSettings,
    pub lock: Lockstep,
    last_send: Option<Instant>,
    /// Dumps received for desync ticks: (tick, player) → its entries as their parts came in.
    dumps: HashMap<(u32, u8), Vec<Option<Part>>>,
    /// Our dumps out: (tick, sent first, sent last); resent every [`DUMP_RESEND`] for
    /// [`DUMP_LINGER`] (a lost part would leave the peer's report incomplete).
    dumps_out: Vec<(u32, Instant, Instant)>,
    last_hash_send: Instant,
    /// Desyncs whose dump we sent, waiting for the peer's.
    open_desyncs: Vec<(u32, u8)>,
    buf: Vec<u8>,
    /// The relay loss was reported ([`NetEvent::RelayLost`]).
    relay_lost: bool,
    /// Gone offline ([`NetEvent::Offline`]): nothing is sent or received any more.
    pub offline: bool,
}

impl Session {
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.sock.local_addr()
    }

    /// The game session of a host whose lobby started (`HostLobby::start`): `addrs` are the
    /// clients, `go` is answered again to a client that missed it.
    pub(crate) fn new_host(sock: Link, session: u32, local: u8, addrs: HashMap<u8, SocketAddr>, roster: Vec<(u8, String)>, settings: GameSettings, go: Packet) -> Result<Session> {
        sock.set_wait(Some(Duration::from_millis(1)))?;
        let ids: Vec<u8> = roster.iter().map(|r| r.0).collect();
        let mut lock = Lockstep::new(local, &ids, crate::commands::dt_for_delay(settings.delay_ms), settings.hash_interval, settings.load_tick);
        lock.ai_level = settings.ai_level;
        Ok(Session { sock, session, local, peers: addrs, start: Some(go), roster, settings, lock, last_send: None, dumps: HashMap::new(), open_desyncs: Vec::new(), dumps_out: Vec::new(), last_hash_send: Instant::now(), buf: vec![0; 65536], relay_lost: false, offline: false })
    }

    /// The game session of a client whose lobby started: `players` is the host's 1017 roster
    /// with every client's address (full mesh).
    pub(crate) fn new_client(sock: Link, session: u32, host: SocketAddr, local: u8, players: Vec<(u8, String, String)>, settings: GameSettings) -> Result<Session> {
        sock.set_wait(Some(Duration::from_millis(1)))?;
        let roster: Vec<(u8, String)> = players.iter().map(|(p, n, _)| (*p, n.clone())).collect();
        let ids: Vec<u8> = roster.iter().map(|r| r.0).collect();
        // The host is the player without an address (player 1 unless a loaded game says other).
        let host_id = players.iter().find(|(p, _, a)| *p != local && a.is_empty()).map_or(1, |e| e.0);
        let mut peers = HashMap::from([(host_id, host)]);
        for (p, _, a) in &players {
            if *p != local && *p != host_id {
                peers.insert(*p, a.parse().with_context(|| format!("player {p} address {a}"))?);
            }
        }
        let missing: Vec<u8> = ids.iter().copied().filter(|p| *p != local && !peers.contains_key(p)).collect();
        if !missing.is_empty() {
            bail!("join: no address for player(s) {missing:?}");
        }
        let mut lock = Lockstep::new(local, &ids, crate::commands::dt_for_delay(settings.delay_ms), settings.hash_interval, settings.load_tick);
        lock.ai_level = settings.ai_level;
        Ok(Session { sock, session, local, peers, start: None, roster, settings, lock, last_send: None, dumps: HashMap::new(), open_desyncs: Vec::new(), dumps_out: Vec::new(), last_hash_send: Instant::now(), buf: vec![0; 65536], relay_lost: false, offline: false })
    }

    /// Host a game for `players` players (the host is player 1) without the lobby screen: wait
    /// until the slots are taken, mark everyone ready and start. `on_bound` gets the bound
    /// address (for tests binding port 0).
    pub fn host(bind: &str, name: &str, players: usize, settings: GameSettings, timeout: Duration, on_bound: impl FnOnce(SocketAddr)) -> Result<Session> {
        Session::host_as(bind, name, players, 1, settings, timeout, on_bound)
    }

    /// [`Session::host`] as player `me` (a loaded game hosted by another player than 1).
    pub fn host_as(bind: &str, name: &str, players: usize, me: u8, settings: GameSettings, timeout: Duration, on_bound: impl FnOnce(SocketAddr)) -> Result<Session> {
        Session::host_on(Link::udp(bind)?, name, players, me, settings, timeout, on_bound)
    }

    /// [`Session::host_as`] on a given link ([ours] through the relay: listed there, `on_bound`
    /// gets the fake address clients join).
    pub fn host_on(link: Link, name: &str, players: usize, me: u8, settings: GameSettings, timeout: Duration, on_bound: impl FnOnce(SocketAddr)) -> Result<Session> {
        let mut lobby = crate::lobby::HostLobby::open_on(link, name, &format!("{name}'s game"), players, settings)?;
        let me = (me.max(1) as usize - 1).min(lobby.slots.len() - 1);
        if me != 0 {
            let host = std::mem::take(&mut lobby.slots[0].name);
            lobby.slots[0].kind = crate::lobby::SLOT_OPEN;
            lobby.slots[me].kind = crate::lobby::SLOT_HUMAN;
            lobby.slots[me].name = host;
            lobby.me = me;
        }
        on_bound(lobby.local_addr()?);
        let deadline = Instant::now() + timeout;
        while lobby.slots.iter().any(|s| s.kind == crate::lobby::SLOT_OPEN) {
            if Instant::now() > deadline {
                let n = lobby.slots.iter().filter(|s| s.kind == crate::lobby::SLOT_HUMAN).count();
                bail!("lobby: {n} of {players} players after {timeout:?}");
            }
            lobby.pump();
            std::thread::sleep(Duration::from_millis(1));
        }
        for s in &mut lobby.slots {
            s.ready = true;
        }
        Ok(lobby.start(timeout)?.0)
    }

    /// Join the game hosted at `host` without the lobby screen; blocks until it starts.
    /// `map_hash`: [`crate::map_hash`] of the client's copy of the map (the host compares).
    pub fn join(bind: &str, host: SocketAddr, name: &str, map_hash: u64, timeout: Duration) -> Result<Session> {
        Session::join_loaded(bind, host, name, map_hash, (0, 0), timeout)
    }

    /// [`Session::join`] a loaded game with our save of it: (its world hash, the player we were
    /// saved as); `(0, 0)` for a new game.
    pub fn join_loaded(bind: &str, host: SocketAddr, name: &str, map_hash: u64, save: (u64, u8), timeout: Duration) -> Result<Session> {
        Session::join_on(Link::udp(bind)?, host, name, map_hash, save, timeout)
    }

    /// [`Session::join_loaded`] on a given link ([ours] through the relay: `host` is the fake
    /// address from its room list).
    pub fn join_on(link: Link, host: SocketAddr, name: &str, map_hash: u64, save: (u64, u8), timeout: Duration) -> Result<Session> {
        let mut lobby = crate::lobby::ClientLobby::connect_on(link, host, name, map_hash)?.with_save(save.0, save.1);
        let deadline = Instant::now() + timeout;
        while !lobby.started() {
            if Instant::now() > deadline {
                bail!("join {host}: no start after {timeout:?}");
            }
            for e in lobby.pump() {
                if let crate::lobby::LobbyEvent::Rejected(reason) = e {
                    bail!("the host rejected us: {reason}");
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        lobby.into_session()
    }

    fn send_all(&self, p: &Packet) {
        if self.offline {
            return;
        }
        let b = p.encode(self.session);
        for a in self.peers.values() {
            if let Err(e) = self.sock.send_to(&b, *a) {
                tracing::debug!("send {a}: {e}");
            }
        }
    }

    /// Send our turns (new ones now, unacknowledged ones again every [`RESEND`]) with our acks,
    /// split into datagrams under [`MAX_DATAGRAM`].
    fn send_turns(&mut self, force: bool) {
        if !force && self.last_send.is_some_and(|t| t.elapsed() < RESEND) {
            return;
        }
        let turns: Vec<Turn> = self.lock.unacked().cloned().chain(self.lock.relay()).collect();
        let acks = self.lock.acks();
        let left = self.lock.departed();
        let head = 17 + acks.len() * 5 + left.len() * 5;
        let mut chunk: Vec<Turn> = Vec::new();
        let mut size = head;
        let flush = |chunk: &mut Vec<Turn>, me: &Session| {
            me.send_all(&Packet::Turns { from: me.local, turns: std::mem::take(chunk), acks: acks.clone(), left: left.clone() });
        };
        for t in turns {
            let s = 7 + t.messages.iter().map(|m| 15 + 2 * m.data.len()).sum::<usize>();
            if !chunk.is_empty() && size + s > MAX_DATAGRAM {
                flush(&mut chunk, self);
                size = head;
            }
            size += s;
            chunk.push(t);
        }
        flush(&mut chunk, self);
        self.last_send = Some(Instant::now());
        // The last four hashes again (a lost one would hide a desync for an interval; the
        // lockstep compares each (tick, player) once).
        if self.last_hash_send.elapsed() >= HASH_RESEND {
            self.last_hash_send = Instant::now();
            for (tick, hash) in self.lock.recent_hashes(4) {
                self.send_all(&Packet::Hash { from: self.local, tick, hash });
            }
        }
    }

    /// Produce our turns up to the virtual tick (`last` = the last tick that ran) and send them.
    pub fn produce(&mut self, last: u32) {
        let new = self.lock.produce(last);
        self.send_turns(!new.is_empty());
    }

    /// Send our hash after `tick` (call when [`Lockstep::hash_due`]).
    pub fn send_hash(&mut self, tick: u32, hash: u64, parts: Vec<(String, u64)>) {
        self.lock.record_hash(tick, hash, parts);
        self.send_all(&Packet::Hash { from: self.local, tick, hash });
    }

    /// Receive whatever arrived; returns desyncs, desync reports and departures.
    pub fn pump(&mut self) -> Vec<NetEvent> {
        let mut out = Vec::new();
        // Offline: whatever still arrives (the relay may come back) belongs to another world.
        if self.offline {
            return out;
        }
        self.send_turns(false);
        let known = self.lock.desyncs.len();
        let lost = self.sock.relay_link().is_some_and(|r| r.lost());
        if lost && !self.relay_lost {
            tracing::warn!("the relay has not answered for {:?}", crate::relay::RELAY_LOST);
            out.push(NetEvent::RelayLost);
        }
        self.relay_lost = lost;
        while let Ok((n, from)) = self.sock.recv_from(&mut self.buf) {
            let Ok((sid, p)) = Packet::decode(&self.buf[..n]) else { continue };
            if sid != self.session {
                continue;
            }
            match p {
                Packet::Turns { from: f, turns, acks, left } if self.peers.get(&f) == Some(&from) => {
                    for (p, t) in left {
                        if self.lock.mark_left(p, t) {
                            out.push(NetEvent::Left(p));
                        }
                    }
                    for t in &turns {
                        self.lock.receive_from(t, f);
                    }
                    self.lock.on_acks(f, &acks);
                }
                Packet::Hash { from: f, tick, hash } if self.peers.contains_key(&f) => self.lock.remote_hash(f, tick, hash),
                Packet::Dump { from: f, tick, first, total, parts } if self.peers.contains_key(&f) => {
                    let d = self.dumps.entry((tick, f)).or_default();
                    if d.len() != total as usize {
                        *d = vec![None; total as usize];
                    }
                    for (k, p) in parts.into_iter().enumerate() {
                        if let Some(slot) = d.get_mut(first as usize + k) {
                            *slot = Some(p);
                        }
                    }
                }
                Packet::Inited { player } if self.peers.get(&player) == Some(&from) => {
                    if let Some(s) = &self.start {
                        let _ = self.sock.send_to(&s.encode(self.session), from);
                    }
                }
                // The leaver's departure came with its turns (it lingered until they were
                // acknowledged); the Bye only ends our sending to it.
                Packet::Bye { from: f } if self.peers.contains_key(&f) => {
                    self.peers.remove(&f);
                }
                _ => {}
            }
        }
        // New desyncs: report them and send our dump for that tick.
        let new: Vec<Desync> = self.lock.desyncs[known..].to_vec();
        for d in new {
            if self.lock.parts(d.tick).is_some() {
                self.send_dump(d.tick);
                self.dumps_out.push((d.tick, Instant::now(), Instant::now()));
                self.open_desyncs.push((d.tick, d.player));
            }
            out.push(NetEvent::Desync(d));
        }
        // Our dumps again until the peers surely have them.
        self.dumps_out.retain(|d| d.1.elapsed() < DUMP_LINGER);
        for i in 0..self.dumps_out.len() {
            if self.dumps_out[i].2.elapsed() >= DUMP_RESEND {
                self.dumps_out[i].2 = Instant::now();
                self.send_dump(self.dumps_out[i].0);
            }
        }
        // Reports for desyncs whose remote dump arrived whole.
        let mut i = 0;
        while i < self.open_desyncs.len() {
            let (tick, p) = self.open_desyncs[i];
            let remote: Option<Vec<(String, u64)>> = self.dumps.get(&(tick, p)).and_then(|d| d.iter().cloned().collect());
            if let (Some(remote), Some(local)) = (remote, self.lock.parts(tick)) {
                out.push(NetEvent::DesyncReport(dump_report(tick, local, p, &remote)));
                self.open_desyncs.remove(i);
            } else {
                i += 1;
            }
        }
        out
    }

    /// Our per-subsystem hashes after `tick`, in parts under [`MAX_DATAGRAM`].
    fn send_dump(&self, tick: u32) {
        let Some(parts) = self.lock.parts(tick) else { return };
        for p in crate::wire::dump_packets(self.local, tick, parts) {
            self.send_all(&p);
        }
    }

    /// Pump until tick `next` may run (or `timeout` passes): its commands, plus the events seen.
    pub fn wait_ready(&mut self, next: u32, timeout: Duration) -> (Option<Vec<s4_sim::PlayerCommand>>, Vec<NetEvent>) {
        let deadline = Instant::now() + timeout;
        let mut events = Vec::new();
        loop {
            if let Some(c) = self.lock.ready(next) {
                events.extend(self.stalled(next, true));
                return (Some(c), events);
            }
            events.extend(self.pump());
            events.extend(self.stalled(next, false));
            if Instant::now() > deadline {
                return (None, events);
            }
        }
    }

    /// Keep answering (acks, hashes, dumps) for `d`, e.g. after the last tick so the peer can
    /// finish.
    pub fn linger(&mut self, d: Duration) -> Vec<NetEvent> {
        let end = Instant::now() + d;
        let mut ev = Vec::new();
        while Instant::now() < end {
            self.send_turns(true);
            ev.extend(self.pump());
        }
        ev
    }

    /// While tick `next` waits for turns (0x5a2970 → 0x5a2a90): players overdue are warned about
    /// ([`NetEvent::Waiting`]), players silent past the kick delay removed if we decide
    /// ([`Session::kick`], [`NetEvent::Left`]). Ready: the timer starts again.
    fn stalled(&mut self, next: u32, ready: bool) -> Vec<NetEvent> {
        let missing = if ready { Vec::new() } else { self.lock.missing(next) };
        let nr = self.lock.not_ready.check(Instant::now(), &missing, self.local);
        let mut out: Vec<NetEvent> = nr.warn.into_iter().map(NetEvent::Waiting).collect();
        // [ours] The relay lost and a player past the kick delay (or nobody else left): every peer (not only the
        // deciding one) drops all the others, each after the last of its turns we hold (no
        // relaying is possible any more), and goes offline. The worlds may differ from then on.
        let others: Vec<u8> = self.lock.players().iter().copied().filter(|&p| p != self.local && !self.lock.departed().iter().any(|d| d.0 == p)).collect();
        // (The deciding peer may have kicked them all already: it goes offline too.)
        if self.relay_lost && (!nr.kick.is_empty() || others.is_empty()) && !self.offline {
            for p in others {
                let t = self.lock.own_kick_tick(p);
                if self.lock.mark_left(p, t) {
                    tracing::warn!("relay lost: player {p} dropped, departure at tick {t}");
                    out.push(NetEvent::Left(p));
                }
            }
            self.offline = true;
            out.push(NetEvent::Offline);
            return out;
        }
        for p in nr.kick {
            if self.kick(p).is_some() {
                out.push(NetEvent::Left(p));
            }
        }
        out
    }

    /// Without waiting: receive, then tick `next`'s commands if it may run, plus the events.
    pub fn poll_ready(&mut self, next: u32) -> (Option<Vec<s4_sim::PlayerCommand>>, Vec<NetEvent>) {
        let mut events = self.pump();
        let cmds = self.lock.ready(next);
        events.extend(self.stalled(next, cmds.is_some()));
        (cmds, events)
    }

    /// Leave the game: our departure is our first turn not produced; we keep sending our turns
    /// (with the departure) until every peer has acknowledged them, at most [`LEAVE_LINGER`],
    /// then say goodbye. The others run our [`crate::lockstep::PLAYER_LEFT`] at that tick.
    pub fn leave(&mut self) {
        if self.offline {
            self.lock.leave();
            return;
        }
        let t = self.lock.leave();
        tracing::info!("leaving: departure at tick {t}");
        let end = Instant::now() + LEAVE_LINGER;
        while Instant::now() < end && !self.lock.all_acked() {
            self.send_turns(true);
            self.pump();
        }
        // A few more rounds so the peers' acks of the last resend and our departure settle.
        let settle = Instant::now() + Duration::from_millis(60);
        while Instant::now() < settle {
            self.send_turns(true);
            self.pump();
        }
        self.send_all(&Packet::Bye { from: self.local });
    }

    /// The not-ready kick (0x5a2b87 → 0x5a2da0): if we decide (the lowest player still in),
    /// `player` leaves at the tick after the most of its turns any peer holds; the departure goes
    /// out with our turns. Returns it.
    pub fn kick(&mut self, player: u8) -> Option<u32> {
        if self.lock.decider() != Some(self.local) || player == self.local {
            return None;
        }
        let t = self.lock.kick_tick(player);
        self.lock.mark_left(player, t).then(|| {
            tracing::warn!("player {player} kicked: silent past the kick delay, departure at tick {t}");
            self.send_turns(true);
            t
        })
    }
}
