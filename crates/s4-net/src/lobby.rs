//! The LAN lobby, non-blocking (the start screens poll it every frame): discovery by UDP
//! broadcast, the host's slot table, client requests, chat, and the start handshake.
//!
//! The original (`CStateLobbyConnect`, `CStateLobbyGameSettings`, `CGameHost`):
//! * the host owns the player data; a client's edit is a request (`ChangeData{slot, field, ∓}`,
//!   message 1052, applied in `CGameHost::UserDataChange`) and the host sends the new player data
//!   to everyone (`SendPlayerData`, message 1032);
//! * chat goes to the host (1050), which relays it to all (1036);
//! * start (host, `CheckContinue`: no slot left open, every occupied slot ready): the final player
//!   data, then 1017 "init" → every client initialises the game and answers 1018 → 1019 "start" →
//!   everyone ticks (`HostInitGame`, `ClientStartingGame`, `HostGameInited`).
//!
//! Here: [`Packet::Roster`] is the player data, [`Packet::Start`] carries 1017 (with every
//! client's address for the mesh), [`Packet::Inited`] is 1018 and [`Packet::Go`] 1019.
//!
//! [ours] Internet games run the same lobby over the relay ([`Link::Relay`]): the host's room
//! entry is its [`GameInfo`] ([`HostLobby::open_internet`]), [`Discovery::internet`] lists the
//! rooms, [`ClientLobby::connect_internet`] joins one.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::relay::{Link, relay_id, room_info};
use crate::session::Session;
use crate::wire::{GameInfo, GameSettings, Packet, SlotInfo};

/// The LAN port (host lobby and discovery). [inf] Our choice; the original used DirectPlay/Storm.
pub const LAN_PORT: u16 = 7710;

pub const SLOT_OPEN: u8 = 0;
pub const SLOT_HUMAN: u8 = 1;
pub const SLOT_AI: u8 = 2;
pub const SLOT_CLOSED: u8 = 3;

/// `ChangeData` fields (`GuiDlgMainGameSettingsProc`: colour 2185/2184, race 2183/2182, team
/// 2181/2180, logo 2187/2186 (events 0x55, `SetLobbyState` 0x51d460 → field 4), ready 2244).
pub const FIELD_COLOUR: u8 = 0;
pub const FIELD_RACE: u8 = 1;
pub const FIELD_TEAM: u8 = 2;
pub const FIELD_LOGO: u8 = 4;
pub const FIELD_READY: u8 = 5;

/// `CGameHost::UserDataChange` ranges: colours 0..=7 (wrapping, skipping colours other occupied
/// slots have); races in the cycle 0 Roman, 1 Viking, 2 Maya, (4 Trojan with
/// `g_uiExtrasAllowed & 1`), 5 Random, never 3 (Dark Tribe); teams 0..alliances−1 (wrapping),
/// alliances = the map settings' team count (GameState +0x40, [`GameSettings::teams`]).
/// [inf] The History Edition has the add-ons, so the Trojans are offered.
const COLOURS: u8 = 8;
const RACES: [u8; 5] = [0, 1, 2, 4, RACE_RANDOM];
/// The team count of a lobby whose settings carry none.
const TEAMS: u8 = 8;
/// Logo indices (0x5a5270 wraps at 8; `s4_formats::logo::CHOICES`).
const LOGOS: u8 = 8;
/// Race 5: resolved at the start (`SetRandomRaces`).
pub const RACE_RANDOM: u8 = 5;

/// What the lobby reports to the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LobbyEvent {
    /// The slot table changed.
    Roster,
    Chat(String),
    /// Client: the host turned us away.
    Rejected(String),
    /// Client: admitted as this player.
    Joined(u8),
    /// A player left.
    Left(u8),
}

fn session_id() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
    (t as u32) | 1
}

/// `CGameHost::UserDataChange` on one slot: `up` = the + arrow (direction 7, − = 6); `teams`
/// the team count (0: 8).
pub fn apply_change(slots: &mut [SlotInfo], i: usize, field: u8, up: bool, teams: u8) {
    if i >= slots.len() {
        return;
    }
    let step = |v: u8, n: u8| if up { (v + 1) % n } else { (v + n - 1) % n };
    match field {
        FIELD_COLOUR => {
            // Repeated until no other occupied slot has the colour.
            let taken = |c: u8| slots.iter().enumerate().any(|(j, o)| j != i && matches!(o.kind, SLOT_HUMAN | SLOT_AI) && o.colour == c);
            let mut c = step(slots[i].colour, COLOURS);
            for _ in 0..COLOURS {
                if !taken(c) {
                    break;
                }
                c = step(c, COLOURS);
            }
            slots[i].colour = c;
        }
        FIELD_RACE => {
            let k = RACES.iter().position(|&r| r == slots[i].race).unwrap_or(0) as u8;
            slots[i].race = RACES[step(k, RACES.len() as u8) as usize];
        }
        FIELD_TEAM => {
            let n = if teams == 0 { TEAMS } else { teams };
            slots[i].team = step(slots[i].team.min(n - 1), n);
        }
        // 0x5a5270 case 4: 0..8, wrapping; nothing keeps logos apart on the host.
        FIELD_LOGO => slots[i].logo = step(slots[i].logo % LOGOS, LOGOS),
        FIELD_READY => slots[i].ready = !slots[i].ready,
        _ => {}
    }
}

/// `SetRandomRaces` at the start (host): race 5 becomes `rand() & 3` with 3 → 4 (Trojans
/// allowed), [inf] from a seed the host picks; the result goes to everyone in the roster.
pub fn resolve_random_races(slots: &mut [SlotInfo], seed: u32) {
    let mut r = seed | 1;
    for s in slots.iter_mut().filter(|s| s.race == RACE_RANDOM) {
        r = r.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let v = ((r >> 16) & 3) as u8;
        s.race = if v == 3 { 4 } else { v };
    }
}

/// The slot type button (`SetLobbyState` event 0x57, 0x51d460) on slot `i` (GameState +0x6c:
/// 1 open, 2 computer easy, 3 computer normal; +0x1b0 the slot is empty; +0x3ba the map's slot
/// is free):
/// * a map-fixed slot (control 1 or 2) only toggles a computer between easy and normal;
/// * a free slot that is empty or a computer counts up: open → easy (a computer joins, 0x5a7ee0:
///   ready) → normal → after normal (4) easy again in a local game, in a network game type 0,
///   which 0x5a8160 turns back into an empty open slot;
/// * a slot a human is in does not change in a local game. In a network game the release counts
///   such a slot's type up anyway when the slot is free and its race is free (+0x3c3, the map's
///   race 255): 1 → 2 asks 0x5a7ee0 to put a computer into the occupied slot (refused with
///   GameState +0x2ac clear: "could not be connected", the type stays 2), then 3, then 4 → 0,
///   where 0x5a8160 finds no computer to take out, then 1 ([`SlotInfo::human_steps`]; the button
///   shows the type). The human stays; what the start makes of a human slot of type 2 / 3 was
///   not traced, so the start ignores it (`tools/re/viewer-2026-10-22/slots/`).
pub fn cycle_slot(slots: &mut [SlotInfo], i: usize, network: bool) {
    let Some(s) = slots.get_mut(i) else { return };
    if s.fixed {
        if s.kind == SLOT_AI {
            s.level ^= 1;
        }
        return;
    }
    match (s.kind, s.level) {
        (SLOT_HUMAN, _) if network && s.race_free => s.human_steps = (s.human_steps + 1) % 4,
        (SLOT_OPEN, _) => {
            s.kind = SLOT_AI;
            s.level = 0;
            s.name = "AI".into();
            s.ready = true;
        }
        (SLOT_AI, 0) => s.level = 1,
        (SLOT_AI, _) if !network => s.level = 0,
        (SLOT_AI, _) => {
            s.kind = SLOT_OPEN;
            s.level = 0;
            s.name.clear();
            s.ready = false;
        }
        _ => {}
    }
}

/// `CheckReady` (map loaded; already ready → true): no other occupied slot has the colour.
/// [inf] Game mode 3 (every alliance needs a player) is not offered.
pub fn can_be_ready(slots: &[SlotInfo], i: usize) -> bool {
    let Some(me) = slots.get(i) else { return false };
    !slots.iter().enumerate().any(|(j, s)| j != i && matches!(s.kind, SLOT_HUMAN | SLOT_AI) && s.colour == me.colour)
}

/// `CheckContinue`: every empty slot locked (not open), every occupied slot ready (AI slots come
/// in ready; the host, who has no ready button in a network game, counts as ready).
pub fn can_start(slots: &[SlotInfo]) -> bool {
    can_start_hosted(slots, 0)
}

/// [`can_start`] with the host in slot `host`.
pub fn can_start_hosted(slots: &[SlotInfo], host: usize) -> bool {
    slots.iter().all(|s| s.kind != SLOT_OPEN) && slots.iter().enumerate().filter(|(_, s)| s.kind == SLOT_HUMAN).all(|(i, s)| i == host || s.ready)
}

/// The host's lobby. Player id = slot index + 1; the host is in slot [`HostLobby::me`] (0, i.e.
/// player 1, unless a loaded game makes it another player).
pub struct HostLobby {
    sock: Link,
    session: u32,
    pub game_name: String,
    pub settings: GameSettings,
    pub slots: Vec<SlotInfo>,
    addrs: HashMap<u8, SocketAddr>,
    pub chat: Vec<String>,
    /// The host's own slot.
    pub me: usize,
    last_roster: Instant,
    buf: Vec<u8>,
}

impl HostLobby {
    /// Open a lobby for `max` players on `bind` (e.g. `0.0.0.0:7710`).
    pub fn open(bind: &str, host_name: &str, game_name: &str, max: usize, settings: GameSettings) -> Result<HostLobby> {
        HostLobby::open_on(Link::udp(bind)?, host_name, game_name, max, settings)
    }

    /// [ours] Host through the relay at `server` (`host:port`): the game is listed there while
    /// the lobby is pumped.
    pub fn open_internet(server: &str, host_name: &str, game_name: &str, max: usize, settings: GameSettings) -> Result<HostLobby> {
        let mut l = HostLobby::open_on(Link::relay(server, RELAY_TIMEOUT)?, host_name, game_name, max, settings)?;
        l.list_room(true);
        Ok(l)
    }

    /// [`HostLobby::open`] on a given link.
    pub fn open_on(sock: Link, host_name: &str, game_name: &str, max: usize, settings: GameSettings) -> Result<HostLobby> {
        let mut slots = vec![SlotInfo { kind: SLOT_OPEN, ..Default::default() }; max.clamp(1, 8)];
        for (i, s) in slots.iter_mut().enumerate() {
            s.colour = i as u8;
            s.team = i as u8;
            s.logo = i as u8;
        }
        slots[0].kind = SLOT_HUMAN;
        slots[0].name = host_name.to_string();
        Ok(HostLobby { sock, session: session_id(), game_name: game_name.to_string(), settings, slots, addrs: HashMap::new(), chat: Vec::new(), me: 0, last_roster: Instant::now(), buf: vec![0; 65536] })
    }

    /// Our address: the LAN socket's, or the relay id's fake address (the room to join).
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.sock.local_addr()
    }

    /// Whether the lobby runs through the relay.
    pub fn is_internet(&self) -> bool {
        self.sock.is_relay()
    }

    /// Through the relay: nothing from it for [`crate::relay::RELAY_LOST`] (10 s) while
    /// pumped, so it is gone (2509); never on the LAN.
    pub fn relay_lost(&self) -> bool {
        self.sock.relay_link().is_some_and(|r| r.lost())
    }

    /// The game as discovery lists it.
    pub fn info(&self) -> GameInfo {
        let joined = self.slots.iter().filter(|s| s.kind == SLOT_HUMAN).count() as u8;
        GameInfo { name: self.game_name.clone(), map: self.settings.map.clone(), joined, max: self.slots.len() as u8, load_hash: self.settings.load_hash, host: self.me as u8 + 1 }
    }

    /// Through the relay: our room's entry, listed (`open`) or not.
    fn list_room(&mut self, open: bool) {
        if let Some(r) = self.sock.relay_link() {
            r.set_room(room_info(&self.info(), self.settings.map_hash), open);
        }
    }

    fn send(&self, p: &Packet, to: SocketAddr) {
        let _ = self.sock.send_to(&p.encode(self.session), to);
    }

    fn broadcast_roster(&mut self) {
        let p = Packet::Roster { slots: self.slots.clone() };
        for &a in self.addrs.values() {
            self.send(&p, a);
        }
        self.last_roster = Instant::now();
    }

    /// The host's own edit of slot `i` (its own row, or the slot type / kick of another).
    pub fn change(&mut self, i: usize, field: u8, up: bool) {
        if field == FIELD_READY && !self.slots.get(i).is_some_and(|s| s.ready) && !can_be_ready(&self.slots, i) {
            return;
        }
        apply_change(&mut self.slots, i, field, up, self.settings.teams);
        self.broadcast_roster();
    }

    /// The slot type button (event 0x57, [`cycle_slot`] in a network game) of another slot.
    pub fn cycle_slot(&mut self, i: usize) {
        if i == self.me {
            return;
        }
        cycle_slot(&mut self.slots, i, true);
        self.broadcast_roster();
    }

    /// Kick (event 88): the player's slot is opened again.
    pub fn kick(&mut self, i: usize) {
        let id = i as u8 + 1;
        if let Some(a) = self.addrs.remove(&id) {
            self.send(&Packet::Reject { reason: "kicked by the host".into() }, a);
        }
        if let Some(s) = self.slots.get_mut(i).filter(|_| i != self.me) {
            *s = SlotInfo { kind: SLOT_OPEN, colour: s.colour, team: s.team, race: s.race, fixed: s.fixed, logo: s.logo, race_free: s.race_free, ..Default::default() };
        }
        self.broadcast_roster();
    }

    pub fn say(&mut self, text: &str) {
        let line = format!("{}: {text}", self.slots[self.me].name);
        self.chat.push(line.clone());
        for &a in self.addrs.values() {
            self.send(&Packet::Chat { text: line.clone() }, a);
        }
    }

    pub fn can_start(&self) -> bool {
        can_start_hosted(&self.slots, self.me)
    }

    /// Answer discovery, joins, requests and chat. Call every frame.
    pub fn pump(&mut self) -> Vec<LobbyEvent> {
        let mut out = Vec::new();
        let mut roster = false;
        while let Ok((n, from)) = self.sock.recv_from(&mut self.buf) {
            let Ok((sid, p)) = Packet::decode(&self.buf[..n]) else { continue };
            let player = self.addrs.iter().find(|(_, a)| **a == from).map(|(&p, _)| p);
            match p {
                Packet::Query if sid == 0 => self.send(&Packet::Game { info: self.info() }, from),
                Packet::Hello { name, map_hash, save_hash, want } if sid == 0 => {
                    if map_hash != self.settings.map_hash {
                        let reason = format!("{name}: your copy of the map {} differs from the host's (hash {map_hash:016x}, host {:016x})", self.settings.map, self.settings.map_hash);
                        self.send(&Packet::Reject { reason }, from);
                        continue;
                    }
                    // A loaded game: the client's own save of it, as the player it was saved as.
                    let loaded = self.settings.load_hash != 0;
                    if loaded && player.is_none() {
                        let reason = if save_hash != self.settings.load_hash {
                            Some(format!("{name}: you do not have this saved game (tick {}, hash {:016x})", self.settings.load_tick, self.settings.load_hash))
                        } else if !(1..=self.slots.len() as u8).contains(&want) || want as usize - 1 == self.me || self.slots[want as usize - 1].kind != SLOT_OPEN {
                            Some(format!("{name}: player {want} of the saved game is not open"))
                        } else {
                            None
                        };
                        if let Some(reason) = reason {
                            self.send(&Packet::Reject { reason }, from);
                            continue;
                        }
                    }
                    let open = if loaded { player.is_none().then_some(want as usize - 1) } else { self.slots.iter().position(|s| s.kind == SLOT_OPEN) };
                    let id = match player {
                        Some(id) => id,
                        None => match open {
                            Some(i) => {
                                self.slots[i].kind = SLOT_HUMAN;
                                self.slots[i].name = name;
                                self.slots[i].ready = false;
                                self.addrs.insert(i as u8 + 1, from);
                                roster = true;
                                i as u8 + 1
                            }
                            None => {
                                self.send(&Packet::Reject { reason: "the game is full".into() }, from);
                                continue;
                            }
                        },
                    };
                    self.send(&Packet::Welcome { you: id, settings: self.settings.clone() }, from);
                }
                Packet::Change { slot, field, up } if sid == self.session => {
                    // A client may only edit its own slot.
                    if let Some(id) = player
                        && slot + 1 == id
                    {
                        let i = slot as usize;
                        if field != FIELD_READY || self.slots[i].ready || can_be_ready(&self.slots, i) {
                            apply_change(&mut self.slots, i, field, up, self.settings.teams);
                            roster = true;
                        }
                    }
                }
                Packet::Chat { text } if sid == self.session => {
                    if let Some(id) = player {
                        let line = format!("{}: {text}", self.slots[id as usize - 1].name);
                        self.chat.push(line.clone());
                        for &a in self.addrs.values() {
                            self.send(&Packet::Chat { text: line.clone() }, a);
                        }
                        out.push(LobbyEvent::Chat(line));
                    }
                }
                Packet::Bye { .. } if sid == self.session => {
                    if let Some(id) = player {
                        self.addrs.remove(&id);
                        let i = id as usize - 1;
                        self.slots[i] = SlotInfo { kind: SLOT_OPEN, colour: self.slots[i].colour, team: self.slots[i].team, race: self.slots[i].race, fixed: self.slots[i].fixed, logo: self.slots[i].logo, race_free: self.slots[i].race_free, ..Default::default() };
                        roster = true;
                        out.push(LobbyEvent::Left(id));
                    }
                }
                _ => {}
            }
        }
        if roster {
            out.push(LobbyEvent::Roster);
        }
        if roster || self.last_roster.elapsed() > Duration::from_millis(500) {
            self.broadcast_roster();
        }
        // The room list shows the players as they come and go.
        self.list_room(true);
        out
    }

    /// Start (host, when [`HostLobby::can_start`]): 1017 with the final roster and addresses,
    /// wait for every client's 1018, then 1019. Returns the game session and the slot table.
    pub fn start(mut self, timeout: Duration) -> Result<(Session, Vec<SlotInfo>)> {
        if !self.can_start() {
            bail!("not every player is ready");
        }
        resolve_random_races(&mut self.slots, session_id());
        // Started: no longer listed (the room stays for its members).
        self.list_room(false);
        let humans: Vec<u8> = self.slots.iter().enumerate().filter(|(_, s)| s.kind == SLOT_HUMAN).map(|(i, _)| i as u8 + 1).collect();
        let start = Packet::Start {
            players: humans.iter().map(|&p| (p, self.slots[p as usize - 1].name.clone(), self.addrs.get(&p).map(|a| a.to_string()).unwrap_or_default())).collect(),
        };
        let deadline = Instant::now() + timeout;
        let mut inited: Vec<u8> = vec![self.me as u8 + 1];
        let mut last = Instant::now() - Duration::from_secs(1);
        while inited.len() < humans.len() {
            if Instant::now() > deadline {
                bail!("start: players {:?} did not initialise", humans.iter().filter(|p| !inited.contains(p)).collect::<Vec<_>>());
            }
            if last.elapsed() > Duration::from_millis(100) {
                self.broadcast_roster();
                for &a in self.addrs.values() {
                    self.send(&start, a);
                }
                last = Instant::now();
            }
            match self.sock.recv_from(&mut self.buf) {
                Ok((n, from)) => {
                    if let Ok((sid, Packet::Inited { player })) = Packet::decode(&self.buf[..n])
                        && sid == self.session
                        && self.addrs.get(&player) == Some(&from)
                        && !inited.contains(&player)
                    {
                        inited.push(player);
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        for _ in 0..3 {
            for &a in self.addrs.values() {
                self.send(&Packet::Go, a);
            }
        }
        let roster: Vec<(u8, String)> = humans.iter().map(|&p| (p, self.slots[p as usize - 1].name.clone())).collect();
        let s = Session::new_host(self.sock, self.session, self.me as u8 + 1, self.addrs, roster, self.settings, Packet::Go)?;
        Ok((s, self.slots))
    }
}

/// A client in a host's lobby.
pub struct ClientLobby {
    sock: Link,
    host: SocketAddr,
    session: u32,
    pub name: String,
    map_hash: u64,
    /// A loaded game: our save's world hash and the player id we were saved as (0 / 0: none).
    save: (u64, u8),
    /// Our player id once admitted.
    pub me: Option<u8>,
    pub settings: Option<GameSettings>,
    pub slots: Vec<SlotInfo>,
    pub chat: Vec<String>,
    /// The host's 1017: the final roster with addresses.
    start: Option<Vec<(u8, String, String)>>,
    go: bool,
    last_hello: Instant,
    /// The last datagram from the host (or the connect).
    heard: Instant,
    /// [`ClientLobby::host_lost`] after this long without a word from the host.
    pub host_timeout: Duration,
    buf: Vec<u8>,
}

/// A host that says nothing for this long is gone ([`ClientLobby::host_lost`]): it sends the roster
/// every 500 ms, and answers a Hello within its next pump.
pub const HOST_LOST: Duration = Duration::from_secs(10);

impl ClientLobby {
    /// Ask to join `host` (the request is repeated by [`ClientLobby::pump`] until answered).
    pub fn connect(bind: &str, host: SocketAddr, name: &str, map_hash: u64) -> Result<ClientLobby> {
        ClientLobby::connect_on(Link::udp(bind)?, host, name, map_hash)
    }

    /// [ours] Join the game at `host` (an address from [`Discovery::internet`]) through the
    /// relay at `server`.
    pub fn connect_internet(server: &str, host: SocketAddr, name: &str, map_hash: u64) -> Result<ClientLobby> {
        ClientLobby::connect_on(Link::relay(server, RELAY_TIMEOUT)?, host, name, map_hash)
    }

    /// [`ClientLobby::connect`] on a given link (through the relay: `host` is the host's fake
    /// address, whose room is joined).
    pub fn connect_on(sock: Link, host: SocketAddr, name: &str, map_hash: u64) -> Result<ClientLobby> {
        if let Some(r) = sock.relay_link() {
            r.join(relay_id(host).with_context(|| format!("{host} is no game on the relay"))?);
        }
        Ok(ClientLobby {
            sock,
            host,
            session: 0,
            name: name.to_string(),
            map_hash,
            save: (0, 0),
            me: None,
            settings: None,
            slots: Vec::new(),
            chat: Vec::new(),
            start: None,
            go: false,
            last_hello: Instant::now() - Duration::from_secs(1),
            heard: Instant::now(),
            host_timeout: HOST_LOST,
            buf: vec![0; 65536],
        })
    }

    /// Join a loaded game with our save of it: its world hash and the player we were saved as.
    pub fn with_save(mut self, hash: u64, player: u8) -> ClientLobby {
        self.save = (hash, player);
        self
    }

    fn send(&self, p: &Packet) {
        let _ = self.sock.send_to(&p.encode(self.session), self.host);
    }

    /// Request a change of our slot (`ChangeData`, message 1052).
    pub fn change(&self, field: u8, up: bool) {
        if let Some(me) = self.me {
            self.send(&Packet::Change { slot: me - 1, field, up });
        }
    }

    pub fn say(&self, text: &str) {
        self.send(&Packet::Chat { text: text.to_string() });
    }

    pub fn leave(&self) {
        if let Some(me) = self.me {
            self.send(&Packet::Bye { from: me });
        }
    }

    /// Through the relay: nothing from it for [`crate::relay::RELAY_LOST`] (10 s) while
    /// pumped, so it is gone (2509); never on the LAN.
    pub fn relay_lost(&self) -> bool {
        self.sock.relay_link().is_some_and(|r| r.lost())
    }

    /// The host has said nothing for [`HOST_LOST`] (10 s) while pumped: it is gone (closed, crashed,
    /// unreachable), on the LAN or through a relay that still answers. Not after the start.
    pub fn host_lost(&self) -> bool {
        !self.go && self.heard.elapsed() > self.host_timeout
    }

    /// Whether the host started the game (after [`ClientLobby::pump`] saw 1019).
    pub fn started(&self) -> bool {
        self.go
    }

    pub fn pump(&mut self) -> Vec<LobbyEvent> {
        let mut out = Vec::new();
        if self.last_hello.elapsed() > Duration::from_millis(200) {
            match self.me {
                None => {
                    let _ = self.sock.send_to(&Packet::Hello { name: self.name.clone(), map_hash: self.map_hash, save_hash: self.save.0, want: self.save.1 }.encode(0), self.host);
                }
                // Initialised, waiting for 1019: repeat 1018 (the host answers with `Go`).
                Some(me) if self.start.is_some() && !self.go => self.send(&Packet::Inited { player: me }),
                _ => {}
            }
            self.last_hello = Instant::now();
        }
        while let Ok((n, from)) = self.sock.recv_from(&mut self.buf) {
            if from != self.host {
                continue;
            }
            let Ok((sid, p)) = Packet::decode(&self.buf[..n]) else { continue };
            self.heard = Instant::now();
            match p {
                Packet::Reject { reason } => out.push(LobbyEvent::Rejected(reason)),
                Packet::Welcome { you, settings, .. } if self.me.is_none() => {
                    self.session = sid;
                    self.me = Some(you);
                    self.settings = Some(settings);
                    out.push(LobbyEvent::Joined(you));
                }
                _ if sid != self.session || self.session == 0 => {}
                Packet::Roster { slots } => {
                    if slots != self.slots {
                        self.slots = slots;
                        out.push(LobbyEvent::Roster);
                    }
                }
                Packet::Chat { text } => {
                    self.chat.push(text.clone());
                    out.push(LobbyEvent::Chat(text));
                }
                Packet::Start { players } => {
                    // 1017: initialise, answer 1018.
                    self.start = Some(players);
                    if let Some(me) = self.me {
                        self.send(&Packet::Inited { player: me });
                    }
                }
                Packet::Go if self.start.is_some() => self.go = true,
                _ => {}
            }
        }
        out
    }

    /// The game session, once [`ClientLobby::started`].
    pub fn into_session(self) -> Result<Session> {
        let (Some(me), Some(settings), Some(players)) = (self.me, self.settings, self.start) else { bail!("the game has not started") };
        Session::new_client(self.sock, self.session, self.host, me, players, settings)
    }
}

/// LAN game discovery: broadcast queries (and direct queries to typed addresses), collect the
/// hosts' answers. [ours] Or the internet's games: the relay's room list
/// ([`Discovery::internet`]), each game's address the fake address to join.
pub struct Discovery {
    sock: Link,
    pub games: Vec<(SocketAddr, GameInfo, Instant)>,
    buf: Vec<u8>,
}

/// How long registering with the relay may take.
pub const RELAY_TIMEOUT: Duration = Duration::from_secs(5);

impl Discovery {
    pub fn new() -> Result<Discovery> {
        Ok(Discovery { sock: Link::udp("0.0.0.0:0")?, games: Vec::new(), buf: vec![0; 4096] })
    }

    /// [ours] The games listed on the relay at `server` (`host:port`; blocks while registering,
    /// at most [`RELAY_TIMEOUT`]).
    pub fn internet(server: &str) -> Result<Discovery> {
        Ok(Discovery { sock: Link::relay(server, RELAY_TIMEOUT)?, games: Vec::new(), buf: vec![0; 4096] })
    }

    /// Whether this lists the relay's games.
    pub fn is_internet(&self) -> bool {
        self.sock.is_relay()
    }

    /// Through the relay: nothing from it for [`crate::relay::RELAY_LOST`] (10 s) while
    /// pumped, so it is gone (2509); never on the LAN.
    pub fn relay_lost(&self) -> bool {
        self.sock.relay_link().is_some_and(|r| r.lost())
    }

    /// Query the LAN (broadcast to [`LAN_PORT`]) and `direct` addresses; through the relay: ask
    /// for its room list (`direct` is ignored).
    pub fn query(&self, direct: &[SocketAddr]) {
        if let Some(r) = self.sock.relay_link() {
            r.list();
            return;
        }
        let q = Packet::Query.encode(0);
        let _ = self.sock.send_to(&q, SocketAddr::from(([255, 255, 255, 255], LAN_PORT)));
        for a in direct {
            let _ = self.sock.send_to(&q, *a);
        }
    }

    /// Collect answers; games not heard from for 10 s are dropped (through the relay: the list
    /// is replaced by each complete answer). Returns whether the list changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        if self.sock.is_relay() {
            // The relay's answers are taken in by the receive; no game datagrams come here.
            while self.sock.recv_from(&mut self.buf).is_ok() {}
            if let Some(rooms) = self.sock.relay_link().and_then(|r| r.take_rooms()) {
                let now = Instant::now();
                let games: Vec<(SocketAddr, GameInfo, Instant)> = rooms.iter().map(|(id, r)| (crate::relay::relay_addr(*id), crate::relay::game_info(r), now)).collect();
                let same = games.len() == self.games.len() && games.iter().zip(&self.games).all(|(a, b)| (a.0, &a.1) == (b.0, &b.1));
                self.games = games;
                return !same;
            }
            return false;
        }
        while let Ok((n, from)) = self.sock.recv_from(&mut self.buf) {
            if let Ok((_, Packet::Game { info })) = Packet::decode(&self.buf[..n]) {
                match self.games.iter_mut().find(|g| g.0 == from) {
                    Some(g) => {
                        changed |= g.1 != info;
                        g.1 = info;
                        g.2 = Instant::now();
                    }
                    None => {
                        self.games.push((from, info, Instant::now()));
                        changed = true;
                    }
                }
            }
        }
        let before = self.games.len();
        self.games.retain(|g| g.2.elapsed() < Duration::from_secs(10));
        changed || self.games.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots() -> Vec<SlotInfo> {
        let mut s = vec![SlotInfo { kind: SLOT_HUMAN, ..Default::default() }; 3];
        for (i, x) in s.iter_mut().enumerate() {
            x.colour = i as u8;
            x.team = i as u8;
        }
        s
    }

    #[test]
    fn edits_and_rules() {
        let mut s = slots();
        apply_change(&mut s, 1, FIELD_RACE, true, 8);
        assert_eq!(s[1].race, 1);
        apply_change(&mut s, 1, FIELD_RACE, false, 8);
        apply_change(&mut s, 1, FIELD_RACE, false, 8);
        assert_eq!(s[1].race, RACE_RANDOM, "Roman − = Random");
        apply_change(&mut s, 1, FIELD_RACE, false, 8);
        assert_eq!(s[1].race, 4, "then the Trojans, never the Dark Tribe");
        apply_change(&mut s, 1, FIELD_TEAM, false, 8);
        assert_eq!(s[1].team, 0);
        apply_change(&mut s, 1, FIELD_TEAM, false, 8);
        assert_eq!(s[1].team, 7, "teams wrap");
        // Colour − from 1: 0 is taken by slot 0, so it goes on to 7.
        apply_change(&mut s, 1, FIELD_COLOUR, false, 8);
        assert_eq!(s[1].colour, 7);
        // Logos wrap in 0..8 and may be shared (0x5a5270 case 4).
        apply_change(&mut s, 1, FIELD_LOGO, false, 8);
        assert_eq!(s[1].logo, 7);
        apply_change(&mut s, 1, FIELD_LOGO, true, 8);
        apply_change(&mut s, 1, FIELD_LOGO, true, 8);
        assert_eq!(s[1].logo, 1);
        assert!(can_be_ready(&s, 1));
        s[2].colour = 7;
        assert!(!can_be_ready(&s, 1), "colour shared with slot 2");
        s[2].colour = 2;
        assert!(!can_start(&s));
        for i in 1..3 {
            apply_change(&mut s, i, FIELD_READY, true, 8);
        }
        assert!(can_start(&s), "the host needs no ready");
        s[2].kind = SLOT_OPEN;
        assert!(!can_start(&s), "an open slot blocks the start");
        s[2].kind = SLOT_CLOSED;
        assert!(can_start(&s));
        let mut r = vec![SlotInfo { race: RACE_RANDOM, ..Default::default() }; 4];
        resolve_random_races(&mut r, 9);
        assert!(r.iter().all(|s| matches!(s.race, 0 | 1 | 2 | 4)));
    }

    /// `SetLobbyState` 0x57 and the team range: a free slot counts open → easy → normal → (local)
    /// easy / (network) open; a map-fixed computer only toggles its level; a human slot stays;
    /// teams wrap within the map settings' team count.
    #[test]
    fn slot_type_cycle_and_team_count() {
        let mut s = slots();
        s[1].kind = SLOT_OPEN;
        let kinds = |s: &[SlotInfo]| (s[1].kind, s[1].level);
        cycle_slot(&mut s, 1, false);
        assert_eq!(kinds(&s), (SLOT_AI, 0));
        assert!(s[1].ready, "a computer comes in ready");
        cycle_slot(&mut s, 1, false);
        assert_eq!(kinds(&s), (SLOT_AI, 1));
        cycle_slot(&mut s, 1, false);
        assert_eq!(kinds(&s), (SLOT_AI, 0), "local: normal → easy");
        cycle_slot(&mut s, 1, true);
        cycle_slot(&mut s, 1, true);
        assert_eq!(kinds(&s), (SLOT_OPEN, 0), "network: normal → open");
        s[2] = SlotInfo { kind: SLOT_AI, fixed: true, ..s[2].clone() };
        cycle_slot(&mut s, 2, true);
        assert_eq!((s[2].kind, s[2].level), (SLOT_AI, 1));
        cycle_slot(&mut s, 2, true);
        assert_eq!((s[2].kind, s[2].level), (SLOT_AI, 0), "fixed: the level only");
        cycle_slot(&mut s, 0, false);
        assert_eq!(s[0].kind, SLOT_HUMAN, "a human's slot stays");
        // A network host on a human's free slot with a free race: the type counts 1 → 2 → 3 →
        // 0 → 1 (`human_steps` 1, 2, 3, 0), the human stays; locally or with a map race nothing.
        s[0].race_free = true;
        cycle_slot(&mut s, 0, false);
        assert_eq!(s[0].human_steps, 0, "local");
        let steps: Vec<u8> = (0..4).map(|_| { cycle_slot(&mut s, 0, true); s[0].human_steps }).collect();
        assert_eq!((steps, s[0].kind), (vec![1, 2, 3, 0], SLOT_HUMAN));
        s[0].race_free = false;
        cycle_slot(&mut s, 0, true);
        assert_eq!(s[0].human_steps, 0, "the map's race");
        apply_change(&mut s, 2, FIELD_TEAM, true, 3);
        assert_eq!(s[2].team, 0, "slot 2's team 2 + 1 wraps in 3 teams");
        apply_change(&mut s, 2, FIELD_TEAM, false, 3);
        assert_eq!(s[2].team, 2);
    }

    /// A loaded game's lobby: a client without the save (or with another one) is refused, one
    /// asking for a slot that is not open too; one with the save gets the player it was saved
    /// as, and the session starts at the saved tick.
    #[test]
    fn loaded_game_takes_the_saved_player() {
        let settings = GameSettings { map: "m".into(), delay_ms: 0, hash_interval: 64, map_hash: 7, load_tick: 504, load_hash: 0xabcd, ..Default::default() };
        let mut host = HostLobby::open("127.0.0.1:0", "host", "game", 3, settings).unwrap();
        let addr = host.local_addr().unwrap();
        let mut try_join = |save: (u64, u8)| {
            let mut c = ClientLobby::connect("127.0.0.1:0", addr, "client", 7).unwrap().with_save(save.0, save.1);
            let t = Instant::now();
            let mut rejected = None;
            while c.me.is_none() && rejected.is_none() && t.elapsed() < Duration::from_secs(5) {
                host.pump();
                for e in c.pump() {
                    if let LobbyEvent::Rejected(r) = e {
                        rejected = Some(r);
                    }
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            (c, rejected)
        };
        let (_, r) = try_join((0, 0));
        assert!(r.is_some_and(|r| r.contains("do not have this saved game")));
        let (_, r) = try_join((0x1234, 3));
        assert!(r.is_some());
        let (_, r) = try_join((0xabcd, 1));
        assert!(r.is_some_and(|r| r.contains("player 1")), "the host's own slot");
        let (c, r) = try_join((0xabcd, 3));
        assert_eq!((r, c.me), (None, Some(3)));
        assert_eq!(c.settings.as_ref().map(|s| (s.load_tick, s.load_hash)), Some((504, 0xabcd)));
        let (_, r) = try_join((0xabcd, 3));
        assert!(r.is_some_and(|r| r.contains("not open")), "taken");
    }

    /// A host that goes away: its client reports it lost once it has heard nothing for the
    /// timeout (1 s here), not while the host is there.
    #[test]
    fn a_vanished_host_is_lost() {
        let settings = GameSettings { map: "m".into(), delay_ms: 0, hash_interval: 64, map_hash: 7, ..Default::default() };
        let mut host = HostLobby::open("127.0.0.1:0", "host", "game", 2, settings).unwrap();
        let mut c = ClientLobby::connect("127.0.0.1:0", host.local_addr().unwrap(), "client", 7).unwrap();
        c.host_timeout = Duration::from_secs(1);
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(1500) {
            host.pump();
            c.pump();
            assert!(!c.host_lost());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.me, Some(2));
        drop(host);
        let t = Instant::now();
        while !c.host_lost() {
            assert!(t.elapsed() < Duration::from_secs(3));
            c.pump();
            std::thread::sleep(Duration::from_millis(20));
        }
        // (The host's last roster came up to 500 ms before it went.)
        assert!(t.elapsed() >= Duration::from_millis(400), "{:?}", t.elapsed());
    }

    #[test]
    fn lobby_to_game_on_localhost() {
        let settings = GameSettings { map: "m".into(), delay_ms: 0, hash_interval: 64, map_hash: 7, ..Default::default() };
        let mut host = HostLobby::open("127.0.0.1:0", "host", "game", 2, settings).unwrap();
        let addr = host.local_addr().unwrap();
        // Discovery by direct query.
        let mut d = Discovery::new().unwrap();
        d.query(&[addr]);
        let mut c = ClientLobby::connect("127.0.0.1:0", addr, "client", 7).unwrap();
        let t = Instant::now();
        while (c.me.is_none() || d.games.is_empty() || c.slots.len() != 2) && t.elapsed() < Duration::from_secs(5) {
            host.pump();
            c.pump();
            d.pump();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(c.me, Some(2));
        // Asked again after the join, the host reports two players.
        d.query(&[addr]);
        let t = Instant::now();
        while d.games[0].1.joined < 2 && t.elapsed() < Duration::from_secs(5) {
            host.pump();
            d.pump();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(d.games[0].1, GameInfo { name: "game".into(), map: "m".into(), joined: 2, max: 2, load_hash: 0, host: 1 });
        // The client sets its race and declares itself ready; chat goes round.
        c.change(FIELD_RACE, true);
        c.change(FIELD_READY, true);
        c.say("hi");
        let t = Instant::now();
        while !(host.can_start() && c.chat.len() == 1) && t.elapsed() < Duration::from_secs(5) {
            host.pump();
            c.pump();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(host.can_start());
        assert_eq!(host.slots[1].race, 1);
        assert_eq!(c.chat, vec!["client: hi".to_string()]);
        // Start handshake: the client initialises while the host waits.
        let h = std::thread::spawn(move || host.start(Duration::from_secs(5)).map(|(s, slots)| (s.local, slots.len())));
        let t = Instant::now();
        while !c.started() && t.elapsed() < Duration::from_secs(5) {
            c.pump();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(c.started());
        let s = c.into_session().unwrap();
        assert_eq!(s.local, 2);
        assert_eq!(s.lock.players(), &[1, 2]);
        assert_eq!(h.join().unwrap().unwrap(), (1, 2));
    }
}
