//! The relay's datagram format [ours]. Little-endian; every datagram starts with [`MAGIC`] and a
//! kind byte. Client → server kinds are below 0x80, server → client kinds 0x80 and up.
//!
//! * A client proves its address first: [`ToServer::Hello`] (padded to [`HELLO_LEN`]) is
//!   answered with a [`ToClient::Challenge`] cookie; the Hello that carries it is answered with
//!   [`ToClient::Welcome`]: the client's relay id and a token. Until then the server answers
//!   nothing larger than the request (no amplification with a spoofed source).
//! * Every later request carries the token; the server drops requests whose token does not
//!   match the registration of their source address.
//! * A host registers its game ([`ToServer::Host`], repeated as the heartbeat); the room's id is
//!   the host's relay id. Clients list the rooms of their protocol version and join one. Data
//!   goes to a relay id and arrives with the sender's id, filled in by the server, between
//!   members of the same room only.

/// The relay protocol; the last byte is its version.
pub const MAGIC: [u8; 4] = *b"S4R1";
/// The relay's port. [ours]
pub const RELAY_PORT: u16 = 7720;
/// The largest payload forwarded (s4-net's `MAX_DATAGRAM`).
pub const MAX_PAYLOAD: usize = 1200;
/// Header of a [`ToServer::Data`]: magic, kind, token, target.
pub const DATA_HEADER: usize = 4 + 1 + 8 + 2;
/// The largest datagram the server accepts.
pub const MAX_REQUEST: usize = MAX_PAYLOAD + DATA_HEADER;
/// A Hello is padded to this size: every answer to an unproven address is smaller.
pub const HELLO_LEN: usize = 64;
/// String limits of a room.
pub const MAX_NAME: usize = 40;
pub const MAX_MAP: usize = 80;

const HELLO: u8 = 0x01;
const KEEPALIVE: u8 = 0x02;
const HOST: u8 = 0x03;
const LIST: u8 = 0x04;
const JOIN: u8 = 0x05;
const DATA: u8 = 0x06;
const LEAVE: u8 = 0x07;
const CHALLENGE: u8 = 0x81;
const WELCOME: u8 = 0x82;
const ROOMS: u8 = 0x84;
const JOINED: u8 = 0x85;
const FROM: u8 = 0x86;
const ALIVE: u8 = 0x88;

/// A game as the room list shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoomInfo {
    pub name: String,
    pub map: String,
    pub map_hash: u64,
    /// A loaded game's world hash (0: a new game).
    pub load_hash: u64,
    pub joined: u8,
    pub max: u8,
    /// The host's player id.
    pub host_player: u8,
    /// The game's network protocol (s4-net `MAGIC`): only rooms of the asker's version are
    /// listed.
    pub version: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToServer {
    /// `cookie` 0 asks for one.
    Hello { cookie: u64 },
    Keepalive { token: u64 },
    /// Register or refresh our room; `open` = listed and joinable.
    Host { token: u64, open: bool, info: RoomInfo },
    /// The rooms of `version` from `offset` on.
    List { token: u64, version: [u8; 4], offset: u16 },
    Join { token: u64, room: u16 },
    Data { token: u64, to: u16, payload: Vec<u8> },
    Leave { token: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToClient {
    Challenge { cookie: u64 },
    Welcome { id: u16, token: u64 },
    /// `total` rooms listed; these start at `offset`.
    Rooms { total: u16, offset: u16, rooms: Vec<(u16, RoomInfo)> },
    Joined { room: u16, ok: bool },
    Data { from: u16, payload: Vec<u8> },
    /// The answer to a keepalive or a room heartbeat: the relay is there.
    Alive,
}

struct W(Vec<u8>);

impl W {
    fn new(kind: u8) -> W {
        let mut v = Vec::with_capacity(32);
        v.extend_from_slice(&MAGIC);
        v.push(kind);
        W(v)
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    /// A string cut to `max` bytes (at a char boundary).
    fn str(&mut self, s: &str, max: usize) {
        let mut n = s.len().min(max);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.u8(n as u8);
        self.0.extend_from_slice(&s.as_bytes()[..n]);
    }
    fn info(&mut self, i: &RoomInfo) {
        self.str(&i.name, MAX_NAME);
        self.str(&i.map, MAX_MAP);
        self.u64(i.map_hash);
        self.u64(i.load_hash);
        self.u8(i.joined);
        self.u8(i.max);
        self.u8(i.host_player);
        self.0.extend_from_slice(&i.version);
    }
}

/// A bounds-checked reader: every read past the end is `None`.
struct R<'a>(&'a [u8]);

impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap_or_default()))
    }
    fn str(&mut self, max: usize) -> Option<String> {
        let n = self.u8()? as usize;
        if n > max {
            return None;
        }
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn info(&mut self) -> Option<RoomInfo> {
        Some(RoomInfo {
            name: self.str(MAX_NAME)?,
            map: self.str(MAX_MAP)?,
            map_hash: self.u64()?,
            load_hash: self.u64()?,
            joined: self.u8()?,
            max: self.u8()?,
            host_player: self.u8()?,
            version: self.take(4)?.try_into().ok()?,
        })
    }
    fn end(&self) -> Option<()> {
        self.0.is_empty().then_some(())
    }
}

/// The magic and the kind byte; the rest.
fn head(b: &[u8]) -> Option<(u8, R<'_>)> {
    if b.len() < 5 || b[..4] != MAGIC {
        return None;
    }
    Some((b[4], R(&b[5..])))
}

impl ToServer {
    pub fn encode(&self) -> Vec<u8> {
        let w = match self {
            ToServer::Hello { cookie } => {
                let mut w = W::new(HELLO);
                w.u64(*cookie);
                w.0.resize(HELLO_LEN, 0);
                w
            }
            ToServer::Keepalive { token } => {
                let mut w = W::new(KEEPALIVE);
                w.u64(*token);
                w
            }
            ToServer::Host { token, open, info } => {
                let mut w = W::new(HOST);
                w.u64(*token);
                w.u8(*open as u8);
                w.info(info);
                w
            }
            ToServer::List { token, version, offset } => {
                let mut w = W::new(LIST);
                w.u64(*token);
                w.0.extend_from_slice(version);
                w.u16(*offset);
                w
            }
            ToServer::Join { token, room } => {
                let mut w = W::new(JOIN);
                w.u64(*token);
                w.u16(*room);
                w
            }
            ToServer::Data { token, to, payload } => {
                let mut w = W::new(DATA);
                w.u64(*token);
                w.u16(*to);
                w.0.extend_from_slice(payload);
                w
            }
            ToServer::Leave { token } => {
                let mut w = W::new(LEAVE);
                w.u64(*token);
                w
            }
        };
        w.0
    }

    /// `None` for anything malformed (never panics).
    pub fn decode(b: &[u8]) -> Option<ToServer> {
        if b.len() > MAX_REQUEST {
            return None;
        }
        let (kind, mut r) = head(b)?;
        let m = match kind {
            // The padding is not checked, only the length.
            HELLO if b.len() >= HELLO_LEN => ToServer::Hello { cookie: r.u64()? },
            KEEPALIVE => ToServer::Keepalive { token: r.u64()? },
            HOST => {
                let token = r.u64()?;
                let open = r.u8()? != 0;
                let info = r.info()?;
                r.end()?;
                ToServer::Host { token, open, info }
            }
            LIST => {
                let token = r.u64()?;
                let version = r.take(4)?.try_into().ok()?;
                let offset = r.u16()?;
                ToServer::List { token, version, offset }
            }
            JOIN => ToServer::Join { token: r.u64()?, room: r.u16()? },
            DATA => {
                let token = r.u64()?;
                let to = r.u16()?;
                ToServer::Data { token, to, payload: r.0.to_vec() }
            }
            LEAVE => ToServer::Leave { token: r.u64()? },
            _ => return None,
        };
        Some(m)
    }
}

/// The size of a room entry in [`ToClient::Rooms`].
pub fn room_entry_len(i: &RoomInfo) -> usize {
    let mut w = W(Vec::new());
    w.info(i);
    2 + w.0.len()
}

impl ToClient {
    pub fn encode(&self) -> Vec<u8> {
        let w = match self {
            ToClient::Challenge { cookie } => {
                let mut w = W::new(CHALLENGE);
                w.u64(*cookie);
                w
            }
            ToClient::Welcome { id, token } => {
                let mut w = W::new(WELCOME);
                w.u16(*id);
                w.u64(*token);
                w
            }
            ToClient::Rooms { total, offset, rooms } => {
                let mut w = W::new(ROOMS);
                w.u16(*total);
                w.u16(*offset);
                w.u8(rooms.len() as u8);
                for (id, info) in rooms {
                    w.u16(*id);
                    w.info(info);
                }
                w
            }
            ToClient::Joined { room, ok } => {
                let mut w = W::new(JOINED);
                w.u16(*room);
                w.u8(*ok as u8);
                w
            }
            ToClient::Data { from, payload } => {
                let mut w = W::new(FROM);
                w.u16(*from);
                w.0.extend_from_slice(payload);
                w
            }
            ToClient::Alive => W::new(ALIVE),
        };
        w.0
    }

    /// `None` for anything malformed (never panics).
    pub fn decode(b: &[u8]) -> Option<ToClient> {
        let (kind, mut r) = head(b)?;
        let m = match kind {
            CHALLENGE => ToClient::Challenge { cookie: r.u64()? },
            WELCOME => ToClient::Welcome { id: r.u16()?, token: r.u64()? },
            ROOMS => {
                let total = r.u16()?;
                let offset = r.u16()?;
                let n = r.u8()?;
                let mut rooms = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let id = r.u16()?;
                    rooms.push((id, r.info()?));
                }
                ToClient::Rooms { total, offset, rooms }
            }
            JOINED => ToClient::Joined { room: r.u16()?, ok: r.u8()? != 0 },
            FROM => {
                let from = r.u16()?;
                ToClient::Data { from, payload: r.0.to_vec() }
            }
            ALIVE => ToClient::Alive,
            _ => return None,
        };
        Some(m)
    }

    /// The header length of a forwarded datagram (the payload follows it).
    pub const DATA_HEADER: usize = 4 + 1 + 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let info = RoomInfo { name: "Gäme".into(), map: "Multiplayer/Arena".into(), map_hash: 7, load_hash: 9, joined: 2, max: 4, host_player: 1, version: *b"S4N3" };
        for m in [
            ToServer::Hello { cookie: 5 },
            ToServer::Keepalive { token: 1 },
            ToServer::Host { token: 2, open: true, info: info.clone() },
            ToServer::List { token: 3, version: *b"S4N3", offset: 8 },
            ToServer::Join { token: 4, room: 12 },
            ToServer::Data { token: 5, to: 13, payload: vec![1, 2, 3] },
            ToServer::Leave { token: 6 },
        ] {
            assert_eq!(ToServer::decode(&m.encode()), Some(m));
        }
        for m in [
            ToClient::Challenge { cookie: 5 },
            ToClient::Welcome { id: 3, token: 99 },
            ToClient::Rooms { total: 3, offset: 1, rooms: vec![(4, info.clone()), (5, RoomInfo::default())] },
            ToClient::Joined { room: 4, ok: true },
            ToClient::Data { from: 6, payload: vec![9; 1200] },
            ToClient::Alive,
        ] {
            assert_eq!(ToClient::decode(&m.encode()), Some(m));
        }
        assert_eq!(ToServer::Hello { cookie: 0 }.encode().len(), HELLO_LEN);
        // A short Hello is not one.
        assert_eq!(ToServer::decode(&ToServer::Hello { cookie: 1 }.encode()[..20]), None);
        // Long strings are cut at a char boundary.
        let long = RoomInfo { name: "é".repeat(30), ..Default::default() };
        let Some(ToServer::Host { info, .. }) = ToServer::decode(&ToServer::Host { token: 0, open: true, info: long }.encode()) else { panic!() };
        assert_eq!(info.name.len(), MAX_NAME);
    }
}
