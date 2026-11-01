//! The relay server [ours]: registrations (after a cookie round trip), rooms, the room list and
//! forwarding between members of one room. [`Server::handle`] is the whole protocol without
//! sockets (tests drive it directly); [`Server::run`] is the UDP loop.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::proto::{HELLO_LEN, MAX_PAYLOAD, MAX_REQUEST, RoomInfo, ToClient, ToServer, room_entry_len};

/// Limits and timeouts.
#[derive(Clone, Debug)]
pub struct Config {
    pub max_rooms: usize,
    pub max_clients: usize,
    pub max_clients_per_ip: usize,
    pub max_members: usize,
    /// Per source IP: packets and bytes a second (token buckets with twice that as the burst).
    pub rate_packets: u32,
    pub rate_bytes: u32,
    /// A room is listed until this long after its host's last heartbeat.
    pub room_listed: Duration,
    /// A client silent this long is dropped (and leaves its room).
    pub client_timeout: Duration,
    /// Tests only: lose and reorder forwarded data.
    pub chaos: Option<Chaos>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            max_rooms: 256,
            max_clients: 2048,
            max_clients_per_ip: 32,
            max_members: 16,
            // A peer of an 8-player game sends to 7 others about every 15 ms (~470 a second);
            // several players may share an address.
            rate_packets: 2500,
            rate_bytes: 2_000_000,
            room_listed: Duration::from_secs(15),
            client_timeout: Duration::from_secs(30),
            chaos: None,
        }
    }
}

/// Forwarded data lost and reordered (tests of the game over a bad link).
#[derive(Clone, Debug)]
pub struct Chaos {
    /// Each forwarded datagram is dropped with this probability (per mille).
    pub drop_per_mille: u32,
    /// Every other datagram is held back and sent after the next one.
    pub reorder: bool,
    pub seed: u64,
}

struct Client {
    id: u16,
    token: u64,
    last_seen: Instant,
    room: Option<u16>,
}

struct Room {
    info: RoomInfo,
    open: bool,
    heartbeat: Instant,
    members: Vec<u16>,
}

struct Bucket {
    packets: f64,
    bytes: f64,
    last: Instant,
}

/// Counters (tests and the periodic log line).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub malformed: u64,
    pub rate_limited: u64,
    pub unproven: u64,
    pub forwarded: u64,
    pub refused: u64,
}

pub struct Server {
    cfg: Config,
    secret: RandomState,
    /// The tokens' secret, apart from the cookies'.
    token_secret: RandomState,
    clients: HashMap<SocketAddr, Client>,
    ids: HashMap<u16, SocketAddr>,
    rooms: HashMap<u16, Room>,
    buckets: HashMap<IpAddr, Bucket>,
    next_id: u16,
    rng: u64,
    held: Option<(SocketAddr, Vec<u8>)>,
    /// Lines for the log (room created / closed), drained by the caller.
    pub log: Vec<String>,
    pub stats: Stats,
}

/// The cookie epoch: a cookie is valid in its 30 s epoch and the next.
fn epoch() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 30)
}

impl Server {
    pub fn new(cfg: Config) -> Server {
        let rng = cfg.chaos.as_ref().map_or(1, |c| c.seed | 1);
        Server {
            cfg,
            secret: RandomState::new(),
            token_secret: RandomState::new(),
            clients: HashMap::new(),
            ids: HashMap::new(),
            rooms: HashMap::new(),
            buckets: HashMap::new(),
            next_id: 1,
            rng,
            held: None,
            log: Vec::new(),
            stats: Stats::default(),
        }
    }

    fn cookie(&self, from: SocketAddr, epoch: u64) -> u64 {
        self.secret.hash_one((from, epoch)) | 1
    }

    fn token(&self, from: SocketAddr, id: u16) -> u64 {
        self.token_secret.hash_one((from, id, Instant::now())) | 1
    }

    /// Per source IP token buckets; false = drop.
    fn allow(&mut self, ip: IpAddr, len: usize, now: Instant) -> bool {
        let (rp, rb) = (self.cfg.rate_packets as f64, self.cfg.rate_bytes as f64);
        if !self.buckets.contains_key(&ip) && self.buckets.len() >= 65536 {
            return false;
        }
        let b = self.buckets.entry(ip).or_insert(Bucket { packets: 2.0 * rp, bytes: 2.0 * rb, last: now });
        let dt = now.saturating_duration_since(b.last).as_secs_f64();
        b.last = now;
        b.packets = (b.packets + dt * rp).min(2.0 * rp);
        b.bytes = (b.bytes + dt * rb).min(2.0 * rb);
        if b.packets < 1.0 || b.bytes < len as f64 {
            return false;
        }
        b.packets -= 1.0;
        b.bytes -= len as f64;
        true
    }

    /// The registered client at `from` with this token.
    fn proven(&self, from: SocketAddr, token: u64) -> Option<u16> {
        self.clients.get(&from).filter(|c| c.token == token).map(|c| c.id)
    }

    /// One datagram from `from`; the answers go to `out`. Never panics on any input.
    pub fn handle(&mut self, now: Instant, from: SocketAddr, data: &[u8], out: &mut Vec<(SocketAddr, Vec<u8>)>) {
        if !self.allow(from.ip(), data.len(), now) {
            self.stats.rate_limited += 1;
            return;
        }
        if data.len() > MAX_REQUEST {
            self.stats.malformed += 1;
            return;
        }
        let Some(m) = ToServer::decode(data) else {
            self.stats.malformed += 1;
            return;
        };
        let token = match &m {
            ToServer::Hello { cookie } => {
                self.hello(now, from, *cookie, data.len(), out);
                return;
            }
            ToServer::Keepalive { token } | ToServer::Host { token, .. } | ToServer::List { token, .. } | ToServer::Join { token, .. } | ToServer::Data { token, .. } | ToServer::Leave { token } => *token,
        };
        let Some(id) = self.proven(from, token) else {
            self.stats.unproven += 1;
            return;
        };
        if let Some(c) = self.clients.get_mut(&from) {
            c.last_seen = now;
        }
        match m {
            ToServer::Hello { .. } => {}
            // Heartbeats are answered (5 bytes): the client sees the relay is there.
            ToServer::Keepalive { .. } => out.push((from, ToClient::Alive.encode())),
            ToServer::Host { open, info, .. } => {
                self.host(now, from, id, open, info);
                out.push((from, ToClient::Alive.encode()));
            }
            ToServer::List { version, offset, .. } => self.list(from, version, offset, out),
            ToServer::Join { room, .. } => {
                let ok = self.join(id, room);
                out.push((from, ToClient::Joined { room, ok }.encode()));
            }
            ToServer::Data { to, payload, .. } => self.forward(id, to, payload, out),
            ToServer::Leave { .. } => self.drop_client(from),
        }
    }

    /// The cookie round trip, then the registration. Both answers are smaller than the padded
    /// Hello.
    fn hello(&mut self, now: Instant, from: SocketAddr, cookie: u64, len: usize, out: &mut Vec<(SocketAddr, Vec<u8>)>) {
        debug_assert!(len >= HELLO_LEN);
        let e = epoch();
        if cookie == 0 || (cookie != self.cookie(from, e) && cookie != self.cookie(from, e.wrapping_sub(1))) {
            out.push((from, ToClient::Challenge { cookie: self.cookie(from, e) }.encode()));
            return;
        }
        // Proven. A repeated Hello (our Welcome was lost) gets the same registration.
        if let Some(c) = self.clients.get(&from) {
            out.push((from, ToClient::Welcome { id: c.id, token: c.token }.encode()));
            return;
        }
        let per_ip = self.clients.keys().filter(|a| a.ip() == from.ip()).count();
        if self.clients.len() >= self.cfg.max_clients || per_ip >= self.cfg.max_clients_per_ip {
            self.stats.refused += 1;
            return;
        }
        let mut id = self.next_id;
        while id == 0 || self.ids.contains_key(&id) {
            id = id.wrapping_add(1);
        }
        self.next_id = id.wrapping_add(1);
        let token = self.token(from, id);
        self.clients.insert(from, Client { id, token, last_seen: now, room: None });
        self.ids.insert(id, from);
        out.push((from, ToClient::Welcome { id, token }.encode()));
    }

    /// Leave the current room (closed when empty; no longer listed when its host left).
    fn leave_room(&mut self, id: u16) {
        let Some(addr) = self.ids.get(&id).copied() else { return };
        let Some(r) = self.clients.get_mut(&addr).and_then(|c| c.room.take()) else { return };
        let Some(room) = self.rooms.get_mut(&r) else { return };
        room.members.retain(|&m| m != id);
        if r == id {
            room.open = false;
        }
        if room.members.is_empty() {
            let room = self.rooms.remove(&r).expect("present");
            self.log.push(format!("room {r} closed: {:?} on {:?}", room.info.name, room.info.map));
        }
    }

    fn host(&mut self, now: Instant, from: SocketAddr, id: u16, open: bool, info: RoomInfo) {
        let current = self.clients.get(&from).and_then(|c| c.room);
        if current == Some(id) {
            if let Some(r) = self.rooms.get_mut(&id) {
                r.info = info;
                r.open = open;
                r.heartbeat = now;
            }
            return;
        }
        if self.rooms.len() >= self.cfg.max_rooms {
            self.stats.refused += 1;
            return;
        }
        self.leave_room(id);
        self.log.push(format!("room {id} created by {}: {:?} on {:?}, {}/{} players", from.ip(), info.name, info.map, info.joined, info.max));
        self.rooms.insert(id, Room { info, open, heartbeat: now, members: vec![id] });
        if let Some(c) = self.clients.get_mut(&from) {
            c.room = Some(id);
        }
    }

    fn listed(&self, now: Instant) -> impl Iterator<Item = (&u16, &Room)> {
        let window = self.cfg.room_listed;
        self.rooms.iter().filter(move |(_, r)| r.open && now.saturating_duration_since(r.heartbeat) < window)
    }

    /// The rooms of `version` from `offset` on, as many as fit one datagram (sorted by id).
    fn list(&self, to: SocketAddr, version: [u8; 4], offset: u16, out: &mut Vec<(SocketAddr, Vec<u8>)>) {
        let now = Instant::now();
        let mut all: Vec<(u16, &RoomInfo)> = self.listed(now).filter(|(_, r)| r.info.version == version).map(|(&id, r)| (id, &r.info)).collect();
        all.sort_by_key(|r| r.0);
        let mut size = 4 + 1 + 5;
        let mut rooms = Vec::new();
        for (id, info) in all.iter().skip(offset as usize) {
            let n = room_entry_len(info);
            if size + n > MAX_PAYLOAD || rooms.len() == 255 {
                break;
            }
            size += n;
            rooms.push((*id, (*info).clone()));
        }
        out.push((to, ToClient::Rooms { total: all.len().min(u16::MAX as usize) as u16, offset, rooms }.encode()));
    }

    fn join(&mut self, id: u16, room: u16) -> bool {
        let Some(addr) = self.ids.get(&id).copied() else { return false };
        if self.clients.get(&addr).and_then(|c| c.room) == Some(room) {
            return true;
        }
        let max = self.cfg.max_members;
        if !self.rooms.get(&room).is_some_and(|r| r.open && r.members.len() < max) {
            return false;
        }
        self.leave_room(id);
        // The client's own room may have been the one asked for (it just closed).
        let Some(r) = self.rooms.get_mut(&room) else { return false };
        r.members.push(id);
        if let Some(c) = self.clients.get_mut(&addr) {
            c.room = Some(room);
        }
        true
    }

    fn forward(&mut self, from: u16, to: u16, payload: Vec<u8>, out: &mut Vec<(SocketAddr, Vec<u8>)>) {
        let room_of = |id: u16| self.ids.get(&id).and_then(|a| self.clients.get(a)).and_then(|c| c.room);
        let (Some(a), Some(b)) = (room_of(from), room_of(to)) else { return };
        if a != b || payload.len() > MAX_PAYLOAD {
            return;
        }
        let dst = self.ids[&to];
        let d = ToClient::Data { from, payload }.encode();
        self.stats.forwarded += 1;
        if let Some(c) = self.cfg.chaos.clone() {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            if (self.rng % 1000) < c.drop_per_mille as u64 {
                return;
            }
            if c.reorder {
                match self.held.take() {
                    Some(h) => {
                        out.push((dst, d));
                        out.push(h);
                    }
                    None => self.held = Some((dst, d)),
                }
                return;
            }
        }
        out.push((dst, d));
    }

    fn drop_client(&mut self, addr: SocketAddr) {
        let Some(id) = self.clients.get(&addr).map(|c| c.id) else { return };
        self.leave_room(id);
        self.clients.remove(&addr);
        self.ids.remove(&id);
    }

    /// Timeouts: silent clients leave, idle rate buckets go; a held (reordered) datagram goes
    /// out. Call about once a second.
    pub fn sweep(&mut self, now: Instant, out: &mut Vec<(SocketAddr, Vec<u8>)>) {
        let t = self.cfg.client_timeout;
        let silent: Vec<SocketAddr> = self.clients.iter().filter(|(_, c)| now.saturating_duration_since(c.last_seen) > t).map(|(a, _)| *a).collect();
        for a in silent {
            self.drop_client(a);
        }
        self.buckets.retain(|_, b| now.saturating_duration_since(b.last) < Duration::from_secs(10));
        out.extend(self.held.take());
    }

    pub fn rooms(&self) -> usize {
        self.rooms.len()
    }

    pub fn clients(&self) -> usize {
        self.clients.len()
    }

    /// Serve on `sock` until `stop` is set; the log lines go to stdout.
    pub fn run(mut self, sock: UdpSocket, stop: Arc<AtomicBool>) -> anyhow::Result<()> {
        sock.set_read_timeout(Some(Duration::from_millis(200)))?;
        let mut buf = vec![0u8; 2048];
        let mut out = Vec::new();
        let mut last_sweep = Instant::now();
        let mut last_status = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            // Errors: timeouts, and on Windows the ICMP "port unreachable" of an earlier send.
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                self.handle(Instant::now(), from, &buf[..n], &mut out);
            }
            if last_sweep.elapsed() > Duration::from_secs(1) {
                self.sweep(Instant::now(), &mut out);
                last_sweep = Instant::now();
            }
            if last_status.elapsed() > Duration::from_secs(600) {
                println!("status: {} rooms, {} clients, {:?}", self.rooms.len(), self.clients.len(), self.stats);
                last_status = Instant::now();
            }
            for (to, d) in out.drain(..) {
                let _ = sock.send_to(&d, to);
            }
            for l in self.log.drain(..) {
                println!("{l}");
            }
        }
        Ok(())
    }
}

/// A relay on a background thread (tests, a local game): its address and a stop flag.
pub fn spawn(bind: &str, cfg: Config) -> anyhow::Result<(SocketAddr, Arc<AtomicBool>, std::thread::JoinHandle<anyhow::Result<()>>)> {
    let sock = UdpSocket::bind(bind)?;
    let addr = sock.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let h = std::thread::spawn(move || Server::new(cfg).run(sock, s));
    Ok((addr, stop, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(i: u8) -> SocketAddr {
        SocketAddr::from(([10, 0, 0, i], 4000 + i as u16))
    }

    /// Registers `a`; its (id, token).
    fn register(s: &mut Server, a: SocketAddr) -> (u16, u64) {
        let mut out = Vec::new();
        s.handle(Instant::now(), a, &ToServer::Hello { cookie: 0 }.encode(), &mut out);
        let Some(ToClient::Challenge { cookie }) = ToClient::decode(&out[0].1) else { panic!("{out:?}") };
        out.clear();
        s.handle(Instant::now(), a, &ToServer::Hello { cookie }.encode(), &mut out);
        let Some(ToClient::Welcome { id, token }) = ToClient::decode(&out[0].1) else { panic!("{out:?}") };
        (id, token)
    }

    fn info(name: &str) -> RoomInfo {
        RoomInfo { name: name.into(), map: "m".into(), max: 4, joined: 1, host_player: 1, version: *b"S4N3", ..Default::default() }
    }

    /// The cookie: nothing but a small challenge for an unproven address, no registration
    /// without the cookie of that address, every answer to it smaller than the request.
    #[test]
    fn cookie_round_trip_and_no_amplification() {
        let mut s = Server::new(Config::default());
        let mut out = Vec::new();
        let hello = ToServer::Hello { cookie: 0 }.encode();
        s.handle(Instant::now(), addr(1), &hello, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].1.len() < hello.len());
        let Some(ToClient::Challenge { cookie }) = ToClient::decode(&out[0].1) else { panic!() };
        // Another address with that cookie (a spoofer that saw it) gets a challenge, no id.
        out.clear();
        s.handle(Instant::now(), addr(2), &ToServer::Hello { cookie }.encode(), &mut out);
        assert!(matches!(ToClient::decode(&out[0].1), Some(ToClient::Challenge { .. })));
        assert_eq!(s.clients(), 0);
        // A wrong cookie: challenged again.
        out.clear();
        s.handle(Instant::now(), addr(1), &ToServer::Hello { cookie: cookie ^ 2 }.encode(), &mut out);
        assert!(matches!(ToClient::decode(&out[0].1), Some(ToClient::Challenge { .. })));
        // The right one: registered.
        out.clear();
        s.handle(Instant::now(), addr(1), &ToServer::Hello { cookie }.encode(), &mut out);
        assert!(matches!(ToClient::decode(&out[0].1), Some(ToClient::Welcome { .. })));
        assert!(out[0].1.len() < hello.len());
        assert_eq!(s.clients(), 1);
        // Unproven requests (no registration, or a wrong token) get nothing, even a list.
        out.clear();
        for a in [addr(1), addr(3)] {
            s.handle(Instant::now(), a, &ToServer::List { token: 12345, version: *b"S4N3", offset: 0 }.encode(), &mut out);
        }
        assert!(out.is_empty());
        assert_eq!(s.stats.unproven, 2);
        // A Hello too short to be one is ignored.
        s.handle(Instant::now(), addr(4), &hello[..20], &mut out);
        assert!(out.is_empty());
    }

    /// Rooms: listed by version, joined, data only between members of one room with the
    /// server's `from`; the host's departure unlists the room, the last member's closes it.
    #[test]
    fn rooms_and_forwarding() {
        let mut s = Server::new(Config::default());
        let (h, ht) = register(&mut s, addr(1));
        let (c, ct) = register(&mut s, addr(2));
        let (x, xt) = register(&mut s, addr(3));
        let mut out = Vec::new();
        let now = Instant::now();
        s.handle(now, addr(1), &ToServer::Host { token: ht, open: true, info: info("game") }.encode(), &mut out);
        s.handle(now, addr(3), &ToServer::Host { token: xt, open: true, info: RoomInfo { version: *b"S4N2", ..info("old") } }.encode(), &mut out);
        assert_eq!(s.log.len(), 2);
        assert!(out.iter().all(|o| ToClient::decode(&o.1) == Some(ToClient::Alive)), "heartbeats are answered");
        out.clear();
        s.handle(now, addr(2), &ToServer::List { token: ct, version: *b"S4N3", offset: 0 }.encode(), &mut out);
        let Some(ToClient::Rooms { total, rooms, .. }) = ToClient::decode(&out[0].1) else { panic!() };
        assert_eq!((total, rooms), (1, vec![(h, info("game"))]));
        // Not a member yet: nothing forwarded either way.
        out.clear();
        s.handle(now, addr(2), &ToServer::Data { token: ct, to: h, payload: vec![1] }.encode(), &mut out);
        assert!(out.is_empty());
        s.handle(now, addr(2), &ToServer::Join { token: ct, room: h }.encode(), &mut out);
        assert_eq!(ToClient::decode(&out[0].1), Some(ToClient::Joined { room: h, ok: true }));
        out.clear();
        s.handle(now, addr(2), &ToServer::Data { token: ct, to: h, payload: vec![1, 2] }.encode(), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, addr(1));
        assert_eq!(ToClient::decode(&out[0].1), Some(ToClient::Data { from: c, payload: vec![1, 2] }));
        // Another room's client reaches neither.
        out.clear();
        s.handle(now, addr(3), &ToServer::Data { token: xt, to: h, payload: vec![3] }.encode(), &mut out);
        s.handle(now, addr(3), &ToServer::Data { token: xt, to: c, payload: vec![3] }.encode(), &mut out);
        assert!(out.is_empty());
        // The host unlists (game started), the room stays for its members.
        s.handle(now, addr(1), &ToServer::Host { token: ht, open: false, info: info("game") }.encode(), &mut out);
        out.clear();
        s.handle(now, addr(2), &ToServer::List { token: ct, version: *b"S4N3", offset: 0 }.encode(), &mut out);
        assert!(matches!(ToClient::decode(&out[0].1), Some(ToClient::Rooms { total: 0, .. })));
        out.clear();
        s.handle(now, addr(1), &ToServer::Data { token: ht, to: c, payload: vec![4] }.encode(), &mut out);
        assert_eq!(out.len(), 1, "members still reach each other");
        // A third client cannot join a closed (unlisted) room.
        out.clear();
        s.handle(now, addr(3), &ToServer::Join { token: xt, room: h }.encode(), &mut out);
        assert_eq!(ToClient::decode(&out[0].1), Some(ToClient::Joined { room: h, ok: false }));
        let _ = x;
        // The host leaves: the client keeps the room; then it times out: closed.
        s.handle(now, addr(1), &ToServer::Leave { token: ht }.encode(), &mut out);
        assert_eq!(s.rooms(), 2);
        s.sweep(now + Duration::from_secs(31), &mut out);
        assert_eq!((s.rooms(), s.clients()), (0, 0));
        assert!(s.log.iter().filter(|l| l.contains("closed")).count() == 2, "{:?}", s.log);
    }

    /// A room is listed only up to 15 s after its host's last heartbeat.
    #[test]
    fn listing_expires_without_heartbeat() {
        let mut s = Server::new(Config::default());
        let (_, ht) = register(&mut s, addr(1));
        let (_, ct) = register(&mut s, addr(2));
        let mut out = Vec::new();
        let t0 = Instant::now();
        s.handle(t0, addr(1), &ToServer::Host { token: ht, open: true, info: info("game") }.encode(), &mut out);
        let n = |s: &Server, t| s.listed(t).count();
        assert_eq!(n(&s, t0 + Duration::from_secs(14)), 1);
        assert_eq!(n(&s, t0 + Duration::from_secs(16)), 0);
        let _ = ct;
    }

    /// Per-IP rate limit: a burst past twice the rate is dropped, other addresses are not
    /// affected, the bucket refills.
    #[test]
    fn rate_limit_per_ip() {
        let mut s = Server::new(Config { rate_packets: 100, rate_bytes: 1_000_000, ..Config::default() });
        let t0 = Instant::now();
        let mut out = Vec::new();
        let hello = ToServer::Hello { cookie: 0 }.encode();
        for _ in 0..300 {
            s.handle(t0, addr(1), &hello, &mut out);
        }
        assert_eq!(out.len(), 200, "the burst");
        assert_eq!(s.stats.rate_limited, 100);
        out.clear();
        s.handle(t0, addr(2), &hello, &mut out);
        assert_eq!(out.len(), 1, "another IP");
        out.clear();
        for _ in 0..60 {
            s.handle(t0 + Duration::from_millis(500), addr(1), &hello, &mut out);
        }
        assert_eq!(out.len(), 50, "refilled at 100 a second");
        // Bytes: 1 kB/s allows ~30 Hellos of 64 bytes in the burst.
        let mut s = Server::new(Config { rate_packets: 10_000, rate_bytes: 1000, ..Config::default() });
        out.clear();
        for _ in 0..100 {
            s.handle(t0, addr(1), &hello, &mut out);
        }
        assert_eq!(out.len(), 2000 / 64);
    }

    /// Caps on rooms, clients (and per IP), members; oversize data is dropped.
    #[test]
    fn caps() {
        let mut s = Server::new(Config { max_clients: 4, max_clients_per_ip: 3, max_rooms: 1, max_members: 2, ..Config::default() });
        let same_ip = |p: u16| SocketAddr::from(([10, 0, 0, 1], p));
        let a = register(&mut s, same_ip(1));
        let b = register(&mut s, same_ip(2));
        let _ = register(&mut s, same_ip(3));
        let mut out = Vec::new();
        let hello = |s: &mut Server, at: SocketAddr, out: &mut Vec<_>| {
            out.clear();
            s.handle(Instant::now(), at, &ToServer::Hello { cookie: 0 }.encode(), out);
            let Some(ToClient::Challenge { cookie }) = ToClient::decode(&out[0].1) else { panic!() };
            out.clear();
            s.handle(Instant::now(), at, &ToServer::Hello { cookie }.encode(), out);
        };
        hello(&mut s, same_ip(4), &mut out);
        assert!(out.is_empty(), "fourth from one IP");
        let c = register(&mut s, addr(9));
        hello(&mut s, addr(10), &mut out);
        assert!(out.is_empty(), "server full");
        let now = Instant::now();
        s.handle(now, same_ip(1), &ToServer::Host { token: a.1, open: true, info: info("one") }.encode(), &mut out);
        s.handle(now, same_ip(2), &ToServer::Host { token: b.1, open: true, info: info("two") }.encode(), &mut out);
        assert_eq!(s.rooms(), 1, "one room at most");
        out.clear();
        s.handle(now, same_ip(2), &ToServer::Join { token: b.1, room: a.0 }.encode(), &mut out);
        s.handle(now, addr(9), &ToServer::Join { token: c.1, room: a.0 }.encode(), &mut out);
        assert_eq!(ToClient::decode(&out[1].1), Some(ToClient::Joined { room: a.0, ok: false }), "two members at most");
        out.clear();
        let big = ToServer::Data { token: b.1, to: a.0, payload: vec![0; MAX_PAYLOAD + 1] }.encode();
        s.handle(now, same_ip(2), &big, &mut out);
        assert!(out.is_empty());
        let ok = ToServer::Data { token: b.1, to: a.0, payload: vec![0; MAX_PAYLOAD] }.encode();
        s.handle(now, same_ip(2), &ok, &mut out);
        assert_eq!(out.len(), 1);
    }

    /// Random bytes, random mutations of valid datagrams and truncations: no panic, and nothing
    /// answered to an unregistered source but challenges no larger than the request.
    #[test]
    fn fuzz_never_panics() {
        let mut s = Server::new(Config { rate_packets: 1_000_000_000, rate_bytes: u32::MAX, ..Config::default() });
        let (h, ht) = register(&mut s, addr(1));
        let (_, ct) = register(&mut s, addr(2));
        let valid: Vec<Vec<u8>> = vec![
            ToServer::Host { token: ht, open: true, info: info("g") }.encode(),
            ToServer::List { token: ct, version: *b"S4N3", offset: 0 }.encode(),
            ToServer::Join { token: ct, room: h }.encode(),
            ToServer::Data { token: ct, to: h, payload: vec![7; 40] }.encode(),
            ToServer::Hello { cookie: 0 }.encode(),
            ToServer::Keepalive { token: ct }.encode(),
        ];
        let mut r: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            r
        };
        let mut out = Vec::new();
        for i in 0..200_000u32 {
            let mut d = if i % 3 == 0 {
                let n = (next() % 1400) as usize;
                (0..n).map(|_| next() as u8).collect::<Vec<u8>>()
            } else {
                valid[(next() % valid.len() as u64) as usize].clone()
            };
            if !d.is_empty() {
                for _ in 0..(next() % 4) {
                    let k = (next() % d.len() as u64) as usize;
                    d[k] = next() as u8;
                }
                if next() % 4 == 0 {
                    d.truncate((next() % (d.len() as u64 + 1)) as usize);
                }
            }
            let from = if next() % 2 == 0 { addr(1 + (next() % 2) as u8) } else { SocketAddr::from(([192, 168, 1, (next() % 200) as u8], (next() % 65535) as u16)) };
            let registered = s.clients.contains_key(&from);
            out.clear();
            s.handle(Instant::now(), from, &d, &mut out);
            if !registered {
                assert!(out.iter().all(|(to, b)| *to != from || b.len() < d.len().max(HELLO_LEN)), "amplification to {from}");
            }
            // Decoding the server's answers never panics either.
            for (_, b) in &out {
                let _ = ToClient::decode(b);
            }
            let _ = ToClient::decode(&d);
        }
        s.sweep(Instant::now() + Duration::from_secs(60), &mut out);
        assert!(s.stats.malformed > 0);
    }
}
