//! The transport under lobbies and sessions [ours]: a plain UDP socket (LAN, direct addresses)
//! or a link through the internet relay (s4-relay). Through the relay every peer is a relay id;
//! the lobby and the session see it as a fake address [`relay_addr`] (240.0.x.y, class E, never
//! routed), so their peer tables, the 1017 roster's addresses and the sender checks stay as
//! they are.
//!
//! A [`RelayLink`] proves its address (cookie round trip), keeps its registration alive, hosts a
//! room (its heartbeat carries the room's list entry) or joins one, and turns datagrams to and
//! from fake addresses into relay data.

use std::cell::{Cell, RefCell};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use s4_relay::proto::{ToClient, ToServer};
pub use s4_relay::{RELAY_PORT, RoomInfo};

use crate::wire::{GameInfo, MAGIC, MAX_DATAGRAM};

const _: () = assert!(MAX_DATAGRAM == s4_relay::MAX_PAYLOAD, "the relay forwards what the game sends");

/// The fake address of relay id `id`.
pub fn relay_addr(id: u16) -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(240, 0, (id >> 8) as u8, id as u8), RELAY_PORT))
}

/// The relay id of a fake address.
pub fn relay_id(a: SocketAddr) -> Option<u16> {
    match a {
        SocketAddr::V4(v) if v.ip().octets()[..2] == [240, 0] && v.port() == RELAY_PORT => {
            let o = v.ip().octets();
            Some(u16::from_be_bytes([o[2], o[3]]))
        }
        _ => None,
    }
}

/// Why the relay could not be reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayError {
    /// The name does not resolve (or to no IPv4 address).
    NotFound(String),
    /// No answer within the timeout.
    NoAnswer(String),
}

impl std::fmt::Display for RelayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelayError::NotFound(s) => write!(f, "relay server {s} not found"),
            RelayError::NoAnswer(s) => write!(f, "relay server {s} does not answer"),
        }
    }
}

impl std::error::Error for RelayError {}

/// `server` with [`RELAY_PORT`] when it names none.
pub fn with_port(server: &str) -> String {
    match server.rsplit_once(':') {
        Some((_, p)) if p.parse::<u16>().is_ok() => server.to_string(),
        _ => format!("{server}:{RELAY_PORT}"),
    }
}

/// Nothing from the relay this long: it is lost ([`RelayLink::lost`]).
pub const RELAY_LOST: Duration = Duration::from_secs(10);

/// A heartbeat (keepalive, or the room's entry when hosting) at least this often.
const HEARTBEAT: Duration = Duration::from_secs(2);

/// What lobbies and sessions send and receive through.
pub enum Link {
    Udp(UdpSocket),
    Relay(Box<RelayLink>),
}

impl Link {
    /// A UDP socket on `bind` (non-blocking, broadcast allowed).
    pub fn udp(bind: &str) -> Result<Link> {
        let s = UdpSocket::bind(bind).with_context(|| format!("bind {bind}"))?;
        s.set_nonblocking(true)?;
        s.set_broadcast(true)?;
        Ok(Link::Udp(s))
    }

    /// A link through the relay at `server` (`host:port`; a name is resolved), registered within
    /// `timeout`.
    pub fn relay(server: &str, timeout: Duration) -> Result<Link> {
        Ok(Link::Relay(Box::new(RelayLink::connect(server, timeout)?)))
    }

    pub fn is_relay(&self) -> bool {
        matches!(self, Link::Relay(_))
    }

    /// Our address as the peers see it (a relay link: our fake address).
    pub fn local_addr(&self) -> Result<SocketAddr> {
        match self {
            Link::Udp(s) => Ok(s.local_addr()?),
            Link::Relay(r) => Ok(relay_addr(r.id)),
        }
    }

    pub(crate) fn send_to(&self, b: &[u8], to: SocketAddr) -> io::Result<usize> {
        match self {
            Link::Udp(s) => s.send_to(b, to),
            Link::Relay(r) => r.send_to(b, to),
        }
    }

    /// A datagram; `WouldBlock` / `TimedOut` when none (as the socket's mode says).
    pub(crate) fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        match self {
            Link::Udp(s) => s.recv_from(buf),
            Link::Relay(r) => r.recv_from(buf),
        }
    }

    /// Blocking receives that give up after `t` (the session), or non-blocking (`None`).
    pub(crate) fn set_wait(&self, t: Option<Duration>) -> io::Result<()> {
        let s = match self {
            Link::Udp(s) => s,
            Link::Relay(r) => &r.sock,
        };
        match t {
            Some(t) => {
                s.set_nonblocking(false)?;
                s.set_read_timeout(Some(t))
            }
            None => s.set_nonblocking(true),
        }
    }

    pub(crate) fn relay_link(&self) -> Option<&RelayLink> {
        match self {
            Link::Relay(r) => Some(&**r),
            Link::Udp(_) => None,
        }
    }
}

/// The room list as the relay's answers gave it.
#[derive(Default)]
struct RoomList {
    /// (host id, entry).
    rooms: Vec<(u16, RoomInfo)>,
    total: u16,
    /// Pages still coming.
    asking: bool,
    /// Handed out by `take_rooms` already.
    taken: bool,
}

/// A registered client of the relay.
pub struct RelayLink {
    sock: UdpSocket,
    server: SocketAddr,
    /// Our relay id and token.
    pub id: u16,
    token: u64,
    /// The last heartbeat sent.
    beat: Cell<Instant>,
    /// Hosting: our room's entry and whether it is listed (open).
    room: RefCell<Option<(RoomInfo, bool)>>,
    /// The room we want to be in (a host's id), and whether the relay confirmed it.
    want: Cell<Option<u16>>,
    joined: Cell<Option<u16>>,
    rooms: RefCell<RoomList>,
    /// The last datagram from the relay.
    heard: Cell<Instant>,
}

impl RelayLink {
    /// Register with the relay at `server`: Hello, the challenge's cookie, Welcome.
    /// `server` is `host:port` or `host` ([`RELAY_PORT`]); a name is resolved now (IPv4 only:
    /// the server is reached over IPv4). Errors: [`RelayError`] (`anyhow` downcast).
    pub fn connect(server: &str, timeout: Duration) -> Result<RelayLink> {
        let server = &with_port(server.trim());
        let to = match server.to_socket_addrs() {
            Ok(mut a) => a.find(|a| a.is_ipv4()),
            Err(e) => {
                tracing::info!("relay {server}: {e}");
                None
            }
        };
        let Some(to) = to else { return Err(RelayError::NotFound(server.clone()).into()) };
        let sock = UdpSocket::bind("0.0.0.0:0").context("bind")?;
        sock.set_read_timeout(Some(Duration::from_millis(100)))?;
        let deadline = Instant::now() + timeout;
        let mut cookie = 0;
        let mut buf = [0u8; 2048];
        let mut sent = Instant::now() - Duration::from_secs(1);
        loop {
            if Instant::now() > deadline {
                return Err(RelayError::NoAnswer(server.clone()).into());
            }
            if sent.elapsed() > Duration::from_millis(250) {
                let _ = sock.send_to(&ToServer::Hello { cookie }.encode(), to);
                sent = Instant::now();
            }
            let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
            if from != to {
                continue;
            }
            match ToClient::decode(&buf[..n]) {
                Some(ToClient::Challenge { cookie: c }) => {
                    cookie = c;
                    let _ = sock.send_to(&ToServer::Hello { cookie }.encode(), to);
                    sent = Instant::now();
                }
                Some(ToClient::Welcome { id, token }) => {
                    sock.set_nonblocking(true)?;
                    tracing::info!("relay {server} ({to}): registered as {id}");
                    return Ok(RelayLink {
                        sock,
                        server: to,
                        id,
                        token,
                        beat: Cell::new(Instant::now()),
                        heard: Cell::new(Instant::now()),
                        room: RefCell::new(None),
                        want: Cell::new(None),
                        joined: Cell::new(None),
                        rooms: RefCell::new(RoomList { taken: true, ..Default::default() }),
                    });
                }
                _ => {}
            }
        }
    }

    pub fn server(&self) -> SocketAddr {
        self.server
    }

    fn send(&self, m: &ToServer) {
        let _ = self.sock.send_to(&m.encode(), self.server);
    }

    /// Host: our room's list entry (`open` = listed and joinable), sent now and with every
    /// heartbeat.
    pub fn set_room(&self, info: RoomInfo, open: bool) {
        let changed = self.room.borrow().as_ref() != Some(&(info.clone(), open));
        if changed {
            self.send(&ToServer::Host { token: self.token, open, info: info.clone() });
            *self.room.borrow_mut() = Some((info, open));
        }
    }

    /// Client: be in the room of host `room` (asked again with each send until confirmed).
    pub fn join(&self, room: u16) {
        self.want.set(Some(room));
        if self.joined.get() != Some(room) {
            self.send(&ToServer::Join { token: self.token, room });
        }
    }

    /// Ask for the room list of our game version (the answers arrive with receives).
    pub fn list(&self) {
        let mut r = self.rooms.borrow_mut();
        (r.asking, r.taken) = (true, false);
        drop(r);
        self.send(&ToServer::List { token: self.token, version: MAGIC, offset: 0 });
    }

    /// Nothing from the relay for [`RELAY_LOST`]: it is gone. (It answers every heartbeat, so a
    /// link that is received on hears from it at least every [`HEARTBEAT`].)
    pub fn lost(&self) -> bool {
        self.heard.get().elapsed() > RELAY_LOST
    }

    /// The room list, once the answers to the last [`RelayLink::list`] are all in (once).
    pub(crate) fn take_rooms(&self) -> Option<Vec<(u16, RoomInfo)>> {
        let mut r = self.rooms.borrow_mut();
        if r.asking || r.taken {
            return None;
        }
        r.taken = true;
        Some(r.rooms.clone())
    }

    /// The heartbeat when due (every [`HEARTBEAT`], whatever else goes out): the room's entry
    /// when hosting, else a keepalive. The relay answers both.
    fn heartbeat(&self) {
        if self.beat.get().elapsed() < HEARTBEAT {
            return;
        }
        self.beat.set(Instant::now());
        let room = self.room.borrow().clone();
        match room {
            Some((info, open)) => self.send(&ToServer::Host { token: self.token, open, info }),
            None => self.send(&ToServer::Keepalive { token: self.token }),
        }
    }

    fn send_to(&self, b: &[u8], to: SocketAddr) -> io::Result<usize> {
        let Some(id) = relay_id(to) else { return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{to} is no relay peer"))) };
        if b.len() > s4_relay::MAX_PAYLOAD {
            tracing::debug!("relay: {} bytes to {id} dropped (over {})", b.len(), s4_relay::MAX_PAYLOAD);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "datagram too large for the relay"));
        }
        if let Some(r) = self.want.get().filter(|r| self.joined.get() != Some(*r)) {
            self.send(&ToServer::Join { token: self.token, room: r });
        }
        self.send(&ToServer::Data { token: self.token, to: id, payload: b.to_vec() });
        Ok(b.len())
    }

    /// The next game datagram (the relay's own answers are taken in on the way).
    fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.heartbeat();
        loop {
            let (n, from) = self.sock.recv_from(buf)?;
            if from != self.server {
                continue;
            }
            let m = ToClient::decode(&buf[..n]);
            if m.is_some() {
                self.heard.set(Instant::now());
            }
            match m {
                Some(ToClient::Data { from, payload }) => {
                    let k = payload.len();
                    buf[..k].copy_from_slice(&payload);
                    return Ok((k, relay_addr(from)));
                }
                Some(ToClient::Joined { room, ok }) => {
                    if ok {
                        self.joined.set(Some(room));
                    } else {
                        tracing::debug!("relay: room {room} refused us");
                    }
                }
                Some(ToClient::Rooms { total, offset, rooms }) => {
                    let mut r = self.rooms.borrow_mut();
                    if offset == 0 {
                        r.rooms.clear();
                    }
                    // A page that does not follow the ones we have is stale.
                    if offset as usize != r.rooms.len() {
                        continue;
                    }
                    let added = !rooms.is_empty();
                    r.rooms.extend(rooms);
                    r.total = total;
                    let got = r.rooms.len();
                    // More to fetch: the next page (a list that shrank meanwhile ends here).
                    if got < total as usize && added {
                        drop(r);
                        self.send(&ToServer::List { token: self.token, version: MAGIC, offset: got as u16 });
                    } else {
                        r.asking = false;
                    }
                }
                _ => {}
            }
        }
    }
}

impl Drop for RelayLink {
    fn drop(&mut self) {
        let _ = self.sock.send_to(&ToServer::Leave { token: self.token }.encode(), self.server);
    }
}

/// A room's list entry from a host's [`GameInfo`] and map hash.
pub fn room_info(info: &GameInfo, map_hash: u64) -> RoomInfo {
    RoomInfo { name: info.name.clone(), map: info.map.clone(), map_hash, load_hash: info.load_hash, joined: info.joined, max: info.max, host_player: info.host, version: MAGIC }
}

/// The [`GameInfo`] of a room's entry.
pub fn game_info(r: &RoomInfo) -> GameInfo {
    GameInfo { name: r.name.clone(), map: r.map.clone(), joined: r.joined, max: r.max, load_hash: r.load_hash, host: r.host_player }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_addresses() {
        for id in [1u16, 255, 256, 0xfffe] {
            assert_eq!(relay_id(relay_addr(id)), Some(id));
            let s = relay_addr(id).to_string();
            assert_eq!(s.parse::<SocketAddr>().ok().and_then(relay_id), Some(id), "{s}");
        }
        assert_eq!(relay_id("192.168.1.2:7720".parse().unwrap()), None);
    }

    #[test]
    fn default_port_and_errors() {
        assert_eq!(with_port("set4.online"), "set4.online:7720");
        assert_eq!(with_port("set4.online:7000"), "set4.online:7000");
        assert_eq!(with_port("62.166.245.29"), "62.166.245.29:7720");
        let e = RelayLink::connect("no-such-host.invalid", Duration::from_millis(300)).err().unwrap();
        assert_eq!(e.downcast_ref::<RelayError>(), Some(&RelayError::NotFound("no-such-host.invalid:7720".into())), "{e}");
        let dead = UdpSocket::bind("127.0.0.1:0").unwrap();
        let a = dead.local_addr().unwrap().to_string();
        let e = RelayLink::connect(&a, Duration::from_millis(300)).err().unwrap();
        assert_eq!(e.downcast_ref::<RelayError>(), Some(&RelayError::NoAnswer(a)), "{e}");
    }
}
