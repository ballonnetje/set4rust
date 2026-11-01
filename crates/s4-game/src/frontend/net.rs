//! The multiplayer screens of `Menu\GUISetStartscreens.dat` (LAN and internet), over s4-net's
//! lobby.
//!
//! The History Edition only reaches multiplayer through Ubisoft's services: main menu event 44
//! (0x522c95) asks the online manager (`GameManager +0xa34`, vtable +0x54) whether it is logged
//! in. Not logged in: the message box (dialog 0x14, proc 0x48f1d0, via 0x671ed0) with a text
//! built into the exe by language (`[0x11b59e4]`): "The Ubisoft services are not available.Please
//! try again later." (0x10570b4; German 0x1057108, French 0x1057068); logged in but the flag
//! byte 0x149b208 clear: "Netzwerkfehler! Bitte überprüfen Sie die Netzwerkkonfiguration!"
//! (0x1057188, German only); else `CStateLobbyConnect` (0x51b300). There is no ubi.com login
//! screen and no server address anywhere in the GUI; the old lobby's texts (1930..1945,
//! 2497..2547: "Status: Connect...", "Lobby server not found! Please try again.", ...) are still
//! in the string table. The release has no LAN lobby classes. Here (release addresses of what
//! each screen mirrors):
//! * dialog 1 is our entry (in the release `CStateLobbyMultiplayerType` is never constructed and
//!   its proc 0x49beb0 is unreferenced): 2521 "LAN" (tooltip string 2028, event 53) → the game
//!   list; 2520 "Internet" (tooltip 2027) → the same game list in internet mode ([ours], below);
//!   2523 back (event 54); title 2522 = string 2057 (0x49bf0e);
//! * dialog 2 (`CStateLobbyConnect`: create 0x51b350, proc 0x47a430, events 0x51ba50): the player
//!   name 2312 (the release shows the Ubisoft account name, 0x47a0f0; here an edit field kept in
//!   settings.toml), the host IP 2313 (shown and stored, 0x47a1d0; typed addresses are queried
//!   directly here), 2274 = string 2102, 2275 = "IP" (0x47a30f); eight rows of games (0x479a20,
//!   row table at file offset 0xbfc6e0: name button 2277+i, map 2314+i, `" %d - %d"` 2285+i,
//!   quality image 2301+i (29 / 31 / 30), lock image 2293+i (87 open / 89 password)), scroll
//!   2309 / 2310 and the wheel, "Create" 2311 (string 1999, event 61) → map settings with the
//!   multiplayer maps, back 2273 (event 58: the main menu in the release, dialog 1 here); the list
//!   is re-queried every 3000 ms (0x51b8df). 2276 (a second create path, string 3839) is hidden:
//!   it needs the Ubisoft service;
//! * dialog 3 in multiplayer mode: OK 2433 (event 64, 0x51c140) opens the host's lobby;
//! * dialog 4 (`CStateLobbyGameSettings`: init 0x48b710, update 0x48a450, proc 0x48ba20, events
//!   0x51d460): eight rows of eleven links (table at file offset 0xbfc920: slot, team, name,
//!   race, colour, logo, number, ping, ready, slot type, kick), the edit arrows (team 2181 /
//!   2180, race 2183 / 2182, colour 2185 / 2184, logo 2187 / 2186; events 82 / 83 / 84 / 85,
//!   lower link = up), ready 2244 (event 86), start 2142 (host only, event 78), back 2141 (event
//!   77, disabled while ready), chat edit 2153 (event 38) with lines 2240..2243;
//! * logos (`s4_formats::logo`): row[5] and 2256 (the selected row's) are controls whose +0x1e
//!   is 0xf6 (`DrawGUIElement` 0x67294a): image `k` is the user logo `k + 1`, sprite file 1
//!   image `2k` (`CGfxManager` vt+4 0x461cd0), set to the slot's logo index (0x48aa66,
//!   0x48afb0). File 1 is composed from `Logo\Current\HerFig_<slot>.png`, which the screen
//!   fills: at the open every slot from the list in order (0x51c1a0), then `0x51dc90(0, 0)`:
//!   slot `p` ← list[logo of `p`]; in a local game after a logo change the own slot only
//!   (0x51d234 → `0x51dc90(−1, −1)`), and the other slots that had the new logo take the first
//!   unused one (0x51cec2); in a network game every slot at the start (0x51c710 when ready).
//!   The arrows are disabled for a computer's row.
//!
//! The internet mode of dialog 2 ([ours]; the lobbies, the start and the game are the LAN ones,
//! only the game list and the host's / clients' connections go through a relay server): the
//! title 2322 = string 2027 "Internet", 2275 = "Server (host:port)" and 2313 the server
//! (`relay_server` in settings.toml, default `set4.online:7720`; Enter saves it and connects),
//! Create 2311 only while connected, the empty list's first row "Status: Connect..." (1944) /
//! "Status: Not connected" (1945). Every relay registration (the list's, a host's, a joining
//! client's) runs on a worker thread; a host's or a client's under the message box "Status:
//! Connect..." (1944), whose OK / Esc cancel. A server that can't be reached opens the message
//! box with the old lobby's text and the address: 2506 (the name not resolved), else 2510
//! "Lobby server not found! Please try again."; a list whose relay stops answering
//! (`relay_lost`, 10 s) says 2509 "Connection to lobby server lost!", in the list and in a
//! lobby (which goes back to the list); a client whose host goes silent for 10 s
//! (`ClientLobby::host_lost`, LAN or relay) says 2418 "Connection to the host lost!" and goes back
//! to the list. A loaded game is hosted from the load list: a network
//! save opens dialog 1 first, whose LAN / Internet host it on the LAN (`host_saved`) or through
//! the relay (`host_saved_internet`); back returns to the load list.
//!
//! Not as in the release: the start handshake (ours 1017 / 1018 / 1019 over s4-net; the release
//! posts 0x405 into its online `CGameHost` session machine, 0x59c4c0, which sends packet 0x402),
//! the number column (row[6], "%u" of a slot field or "-": always "-" here), ping quality (not
//! measured: shown as 29), `Logo\Current` is kept in memory, not written to the install, and a
//! list shorter than eight keeps the slots past it (the release's copy fails: message 0x95e). 2245 is hidden as in the
//! release without the Ubisoft session: its flag `GameState +0x595` comes only from the session
//! string (0x59ee60).

use std::time::{Duration, Instant};

use s4_net::lobby::{self, FIELD_COLOUR, FIELD_LOGO, FIELD_RACE, FIELD_READY, FIELD_TEAM, LobbyEvent, SLOT_AI, SLOT_CLOSED, SLOT_HUMAN};
use s4_net::{ClientLobby, Discovery, GameInfo, GameSettings, HostLobby, SlotInfo};
use s4_viewer::gpu::Gpu;
use s4_viewer::guistate::UiEvent;

use super::{Action, Frontend, LOAD, LOCAL_GAME, LOCAL_MAP, MAIN, MAP_SETTINGS, render, set_enabled, set_text, set_visible, setup};

pub const MP_TYPE: usize = 1;
pub const GAME_LIST: usize = 2;
pub const LOBBY: usize = 4;

/// The per-row links of dialog 4 (release file offset 0xbfc920).
fn row(i: usize) -> [u16; 11] {
    let i = i as u16;
    [2155 + i, 2163 + i, 2265 + i, 2171 + i, 2190 + i, 2257 + i, 2198 + i, 2206 + i, 2214 + i, 2222 + i, 2230 + i]
}

/// Images of dialog 4 (its update 0x48a450, tables in the release .rdata):
/// team (file offset 0xbfc900) 115..122 by team 0..7;
fn team_image(team: u8) -> u16 {
    115 + team.min(7) as u16
}
/// colour (0xbfc8dc) 1, 3, 5, …, 15 by colour 0..7;
fn colour_image(colour: u8) -> u16 {
    1 + 2 * colour.min(7) as u16
}
/// race (0xbfc8c4) by race 0..5: Roman 33, Viking 34, Maya 32, Dark Tribe 35, Trojan 38,
/// Random 332;
fn race_image(race: u8) -> u16 {
    [33, 34, 32, 35, 38, 332][race.min(5) as usize]
}
/// ready (0xbfc898) by ready state: 0 not ready 28, 1..5 map upload 123..127, 6 ready 27;
fn ready_image(ready: bool) -> u16 {
    if ready { 27 } else { 28 }
}
/// ping (0xbfc8b4) by quality 0..2: 29, 31, 30;
const PING_GOOD: u16 = 29;
/// slot type (0xbfc868: image, pressed, tooltip) by `m_sPlayerType`: 0 locked (89, 90, 2274),
/// 1 open / human (87, 88, 2278), 2 AI easy (93, 94, 2265), 3 AI normal (97, 98, 2316).
fn slot_images(s: &SlotInfo) -> (u16, u16) {
    match (s.kind, s.level) {
        // A human's slot after a network host's clicks ([`lobby::cycle_slot`]): type 2, 3, 0.
        (SLOT_HUMAN, _) if s.human_steps == 1 => (93, 94),
        (SLOT_HUMAN, _) if s.human_steps == 2 => (97, 98),
        (SLOT_HUMAN, _) if s.human_steps == 3 => (89, 90),
        (SLOT_CLOSED, _) => (89, 90),
        (SLOT_AI, 0) => (93, 94),
        (SLOT_AI, _) => (97, 98),
        _ => (87, 88),
    }
}

/// A local game's settings (dialog 19): the slots, the map settings' choices, the human's slot.
pub struct LocalLobby {
    pub slots: Vec<SlotInfo>,
    pub settings: GameSettings,
    pub me: usize,
}

/// The network side of the start screens.
#[derive(Default)]
pub enum NetUi {
    #[default]
    None,
    Browse { disc: Box<Discovery>, last_query: Instant, direct: Vec<std::net::SocketAddr>, top: usize },
    /// [ours] The internet game list: the LAN list's discovery through the relay `server`.
    Internet { server: String, relay: Relay, last_query: Instant, top: usize },
    /// [ours] A host's or a joining client's registration at the relay `server` on a worker
    /// thread, under the message box "Status: Connect..." (its OK / Esc cancel: the game list).
    Opening { server: String, rx: std::sync::mpsc::Receiver<anyhow::Result<Opened>> },
    Host(Box<HostLobby>),
    Client(Box<ClientLobby>),
    Local(Box<LocalLobby>),
}

/// A lobby opened through the relay: the host's, or a client's with the joined game's name.
pub enum Opened {
    Host(Box<HostLobby>),
    Client(Box<ClientLobby>, String),
}

/// The internet list's link to its relay server.
pub enum Relay {
    /// No server typed, or it could not be reached (said once in the message box).
    None,
    /// Registering on a worker thread (up to `RELAY_TIMEOUT`).
    Connecting(std::sync::mpsc::Receiver<anyhow::Result<Discovery>>),
    Connected(Box<Discovery>),
}

/// The QA pass's view of the network screens (main.rs tests, which drive them by clicks).
#[cfg(test)]
impl Frontend {
    pub fn set_lan_port(&mut self, port: u16) {
        self.lan_port = port;
    }

    pub fn current_screen(&self) -> usize {
        self.screen
    }

    /// The game list keeps only the games on `port` (a LAN broadcast may find others); how many.
    pub fn keep_games_on(&mut self, port: u16) -> usize {
        match &mut self.net {
            NetUi::Browse { disc, .. } => {
                disc.games.retain(|g| g.0.port() == port);
                disc.games.len()
            }
            _ => 0,
        }
    }

    /// The host: every other slot ready. A client: its own slot.
    pub fn lobby_state(&self) -> (bool, Option<u8>) {
        match &self.net {
            NetUi::Host(h) => (h.slots.len() > 1 && h.slots.iter().enumerate().all(|(i, s)| i == h.me || s.ready), None),
            NetUi::Client(c) => (false, c.me),
            _ => (false, None),
        }
    }
}

impl Frontend {
    pub(super) fn open_net(&mut self, gpu: &mut Gpu, dialog: usize) {
        match dialog {
            MP_TYPE => {
                // The title (0x49bf0e); "Internet" (2520) is our relay list.
                let t = self.text(2057);
                set_text(gpu, MP_TYPE, 2522, &t);
                set_enabled(gpu, MP_TYPE, 2520, true);
            }
            GAME_LIST => {
                // The texts (0x47a280 / 0x47a0f0).
                let (create, user) = (self.text(1999), self.text(2102));
                set_text(gpu, GAME_LIST, 2311, &create);
                set_visible(gpu, GAME_LIST, 2276, false); // the Ubisoft create path
                set_text(gpu, GAME_LIST, 2274, &user);
                set_visible(gpu, GAME_LIST, 2275, true);
                set_visible(gpu, GAME_LIST, 2313, true);
                let settings = crate::settings::Settings::load();
                set_text(gpu, GAME_LIST, 2312, &settings.player_name);
                if self.internet {
                    // [ours] The title "Internet" (string 2027) and the relay server's address.
                    let t = self.text(2027);
                    set_text(gpu, GAME_LIST, 2322, &t);
                    set_text(gpu, GAME_LIST, 2275, SERVER_LABEL);
                    let server = settings.relay().unwrap_or_default();
                    set_text(gpu, GAME_LIST, 2313, &server);
                    if !matches!(self.net, NetUi::Internet { .. }) {
                        self.net = internet_list(&server);
                    }
                    self.fill_games(gpu);
                    return;
                }
                set_text(gpu, GAME_LIST, 2322, "");
                set_enabled(gpu, GAME_LIST, 2311, true);
                set_text(gpu, GAME_LIST, 2275, "IP");
                set_text(gpu, GAME_LIST, 2313, "");
                if !matches!(self.net, NetUi::Browse { .. }) {
                    match Discovery::new() {
                        Ok(d) => self.net = NetUi::Browse { disc: Box::new(d), last_query: Instant::now() - Duration::from_secs(10), direct: Vec::new(), top: 0 },
                        Err(e) => tracing::warn!("LAN discovery: {e}"),
                    }
                }
                self.fill_games(gpu);
            }
            LOBBY | LOCAL_GAME => {
                // The init (0x48b710): the labels, the slot buttons' numbers; 2144 (update);
                // no row selected (`SelectedPlayerIdLobby` −1: the own one at the update).
                for (link, id) in [(2147, 2022), (2149, 2023), (2150, 2018), (2151, 2021), (2154, 2019), (2244, 2064), (2144, 3840)] {
                    let t = self.text(id);
                    set_text(gpu, dialog, link, &t);
                }
                set_text(gpu, dialog, 2148, "");
                for i in 0..8 {
                    set_text(gpu, dialog, row(i)[0], &(i + 1).to_string());
                }
                set_text(gpu, dialog, 2153, "");
                // `CStateLobbyGameSettings::DrawMap` into the owner image 2246.
                let map = match &self.net {
                    NetUi::Host(h) => Some(h.settings.map.clone()),
                    NetUi::Client(c) => c.settings.as_ref().map(|s| s.map.clone()),
                    NetUi::Local(l) => Some(l.settings.map.clone()),
                    _ => None,
                };
                let img = map.and_then(|m| super::preview(&s4_viewer::scene::resolve_map(&m)).ok());
                if let Some(ui) = gpu.ui.as_mut() {
                    match img {
                        Some(img) => ui.state.owner_images.insert((dialog, 2246), img),
                        None => ui.state.owner_images.remove(&(dialog, 2246)),
                    };
                }
                self.open_logos();
                self.row = None;
                self.fill_lobby(gpu);
            }
            _ => {}
        }
    }

    /// The game settings' open (0x51c1a0): the logo list (0x51c7e0), `Logo\Current` slot `i` ←
    /// list[i], then every slot's own logo (0x51c5ee → 0x51dc90(0, 0)).
    fn open_logos(&mut self) {
        self.logo_list = s4_formats::logo::list(&s4_formats::game_dir());
        for (i, p) in self.logo_list.iter().take(8).enumerate() {
            if let Ok(rgb) = s4_formats::logo::read_rgb(p) {
                self.logos[i] = Some(rgb);
            }
        }
        let slots = match &mut self.net {
            NetUi::Local(l) => {
                separate_logos(&mut l.slots, l.me);
                l.slots.clone()
            }
            NetUi::Host(h) => h.slots.clone(),
            NetUi::Client(c) => c.slots.clone(),
            _ => Vec::new(),
        };
        self.take_logos(&slots, None);
    }

    /// 0x51dc90: `Logo\Current` slot `p` ← list[logo of `p`], for `only` or every slot.
    fn take_logos(&mut self, slots: &[SlotInfo], only: Option<usize>) {
        take_logos(&mut self.logos, &self.logo_list, slots, only);
    }

    /// A logo control (+0x1e = 0xf6): sprite file 1 image `2 logo`, composed from
    /// `Logo\Current` slot `logo`.
    fn show_logo(&self, gpu: &mut Gpu, dlg: usize, link: u16, logo: u8) {
        let img = self.logos.get(logo as usize).and_then(|c| c.as_ref()).map(|rgb| {
            let n = s4_formats::logo::FRAME as u32;
            (n, n, s4_formats::logo::compose_rgba(rgb))
        });
        if let Some(ui) = gpu.ui.as_mut() {
            match img {
                Some(img) => ui.state.owner_images.insert((dlg, link), img),
                None => ui.state.owner_images.remove(&(dlg, link)),
            };
        }
    }

    fn player_name(&self, gpu: &Gpu) -> String {
        gpu.ui.as_ref().and_then(|u| u.state.texts.get(&(GAME_LIST, 2312)).cloned()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| crate::settings::Settings::load().player_name)
    }

    /// The game list rows (release 0x479a20).
    fn fill_games(&self, gpu: &mut Gpu) {
        let Some((games, top)) = self.listed() else { return };
        for i in 0..8 {
            let g = games.get(top + i);
            let links = [2277 + i as u16, 2314 + i as u16, 2285 + i as u16, 2301 + i as u16];
            for l in links {
                set_visible(gpu, GAME_LIST, l, g.is_some());
            }
            // Lock image 87 (no passwords here), quality image 29 (not measured).
            set_visible(gpu, GAME_LIST, 2293 + i as u16, g.is_some());
            if let Some(ui) = gpu.ui.as_mut() {
                ui.state.images.insert((GAME_LIST, 2293 + i as u16), (87, 0));
                ui.state.images.insert((GAME_LIST, 2301 + i as u16), (PING_GOOD, 0));
            }
            if let Some(info) = g {
                set_text(gpu, GAME_LIST, links[0], &info.name);
                set_text(gpu, GAME_LIST, links[1], &map_title(info));
                set_text(gpu, GAME_LIST, links[2], &format!(" {} - {}", info.joined, info.max));
                set_enabled(gpu, GAME_LIST, links[0], info.joined < info.max);
            }
        }
        // [ours] The internet list: Create (2311) needs the server; an empty list says why in its
        // first row: "Status: Connect..." (string 1944) or "Status: Not connected" (1945).
        if let NetUi::Internet { relay, .. } = &self.net {
            set_enabled(gpu, GAME_LIST, 2311, matches!(relay, Relay::Connected(_)));
            let status = match relay {
                Relay::Connected(_) => None,
                Relay::Connecting(_) => Some(1944),
                Relay::None => Some(1945),
            };
            if let Some(id) = status {
                let t = self.text(id);
                set_visible(gpu, GAME_LIST, 2277, true);
                set_text(gpu, GAME_LIST, 2277, &t);
                set_enabled(gpu, GAME_LIST, 2277, false);
            }
        }
    }

    /// The game list's games and its top row.
    fn listed(&self) -> Option<(Vec<&GameInfo>, usize)> {
        match &self.net {
            NetUi::Browse { disc, top, .. } => Some((disc.games.iter().map(|g| &g.1).collect(), *top)),
            NetUi::Internet { relay: Relay::Connected(disc), top, .. } => Some((disc.games.iter().map(|g| &g.1).collect(), *top)),
            NetUi::Internet { top, .. } => Some((Vec::new(), *top)),
            _ => None,
        }
    }

    /// Host a game with `slots` and open its lobby: on the LAN at once (its error returned); from
    /// the internet list the relay registration runs on a worker ([`Frontend::connecting`]).
    pub(super) fn host(&mut self, gpu: &mut Gpu, name: &str, game: &str, slots: Vec<SlotInfo>, settings: GameSettings) -> anyhow::Result<()> {
        let max = slots.len().max(1);
        if let NetUi::Internet { server, .. } = &self.net
            && self.internet
        {
            let server = with_port(server);
            let (to, name, game) = (server.clone(), name.to_string(), game.to_string());
            self.connecting(gpu, server, move || {
                let mut h = HostLobby::open_internet(&to, &name, &game, max, settings)?;
                h.slots = slots;
                Ok(Opened::Host(Box::new(h)))
            });
            return Ok(());
        }
        let mut h = HostLobby::open(&format!("0.0.0.0:{}", self.lan_port), name, game, max, settings)?;
        h.slots = slots;
        self.net = NetUi::Host(Box::new(h));
        self.open(gpu, LOBBY);
        Ok(())
    }

    /// [ours] Register with the relay `server` on a worker (it blocks up to `RELAY_TIMEOUT`)
    /// while the message box says "Status: Connect..." (string 1944); its OK or Esc goes back to
    /// the game list, which drops the registration. [`Frontend::net_tick`] opens the lobby.
    fn connecting(&mut self, gpu: &mut Gpu, server: String, work: impl FnOnce() -> anyhow::Result<Opened> + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.net = NetUi::Opening { server, rx };
        let t = self.text(1944);
        self.message(gpu, t, GAME_LIST);
    }

    /// The game settings' update (release 0x48a450, dialog 4 / 19) from the lobby's slot table.
    fn fill_lobby(&mut self, gpu: &mut Gpu) {
        let (slots, me, host, settings, chat, name): (&[SlotInfo], usize, bool, _, &[String], String) = match &self.net {
            NetUi::Host(h) => (&h.slots, h.me, true, Some(&h.settings), &h.chat, h.game_name.clone()),
            NetUi::Client(c) => (&c.slots, c.me.map_or(0, |m| m as usize - 1), false, c.settings.as_ref(), &c.chat, self.joined_name.clone()),
            NetUi::Local(l) => (&l.slots, l.me, true, Some(&l.settings), &[], String::new()),
            _ => return,
        };
        let local = matches!(self.net, NetUi::Local(_));
        let dlg = if local { LOCAL_GAME } else { LOBBY };
        // `SelectedPlayerIdLobby`: the own row unless the host selected a computer's.
        let sel = self.row.filter(|&r| host && slots.get(r).is_some_and(|s| s.kind == SLOT_AI)).unwrap_or(me);
        let map = settings.map(|s| map_title(&GameInfo { map: s.map.clone(), ..Default::default() })).unwrap_or_default();
        set_text(gpu, dlg, 2145, &name);
        set_text(gpu, dlg, 2152, &map);
        let size = settings.and_then(|s| s4_formats::map::MapFile::open(s4_viewer::scene::resolve_map(&s.map)).ok()).and_then(|m| m.general().ok()).map_or(0, |g| g.size);
        set_text(gpu, dlg, 2146, &size.to_string());
        let joined = slots.iter().filter(|s| s.kind == SLOT_HUMAN || s.kind == SLOT_AI).count();
        set_text(gpu, dlg, 2143, &format!("{joined} / {}", slots.len()));
        for i in 0..8 {
            let links = row(i);
            let Some(s) = slots.get(i) else {
                for l in links {
                    set_visible(gpu, dlg, l, false);
                }
                continue;
            };
            let occupied = matches!(s.kind, SLOT_HUMAN | SLOT_AI);
            set_visible(gpu, dlg, links[0], true);
            for (k, &l) in links.iter().enumerate().skip(1) {
                set_visible(gpu, dlg, l, occupied || k == 9);
            }
            set_visible(gpu, dlg, links[5], occupied);
            self.show_logo(gpu, dlg, links[5], s.logo);
            if let Some(ui) = gpu.ui.as_mut() {
                // [inf] Ping quality is not measured: shown as good.
                ui.state.images.insert((dlg, links[7]), (PING_GOOD, 0));
            }
            let img = |gpu: &mut Gpu, l: u16, im: (u16, u16)| {
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.state.images.insert((dlg, l), im);
                }
            };
            img(gpu, links[9], slot_images(s));
            // The slot type button is enabled for the host (0x149c13a) where the row's +0x83d is
            // set (0x51ca70: the map's slot is free, +0x3ba, or a computer is in it), human rows
            // included (0x48a450); event 0x57 then changes nothing for a human ([`lobby::cycle_slot`];
            // in a network game a free slot with a free race, +0x3c3, still counts up). Kick for
            // other humans.
            // [inf] Off in a loaded game (see `host_saved`).
            let loaded = settings.is_some_and(|x| x.load_hash != 0);
            set_enabled(gpu, dlg, links[9], host && !loaded && (!s.fixed || s.kind == SLOT_AI));
            set_visible(gpu, dlg, links[10], host && i != me && s.kind == SLOT_HUMAN);
            if !occupied && loaded {
                // A loaded game: the name the player had, until its peer joins.
                set_text(gpu, dlg, links[2], &s.name);
            }
            if occupied {
                img(gpu, links[1], (team_image(s.team), 0));
                set_text(gpu, dlg, links[2], &s.name);
                img(gpu, links[3], (race_image(s.race), 0));
                img(gpu, links[4], (colour_image(s.colour), 0));
                set_text(gpu, dlg, links[6], "-");
                img(gpu, links[8], (ready_image(s.ready), 0));
                if let Some(ui) = gpu.ui.as_mut() {
                    // Font 9 for a row the player can edit that is not the selected one (the
                    // host's AI rows here), else 7.
                    ui.state.fonts.insert((dlg, links[2]), if host && s.kind == SLOT_AI && i != sel { 9 } else { 7 });
                }
            }
        }
        // The selected row's edit arrows and the images between them.
        if let Some(s) = slots.get(sel) {
            let img = |gpu: &mut Gpu, l: u16, im: u16| {
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.state.images.insert((dlg, l), (im, 0));
                }
            };
            img(gpu, 2188, team_image(s.team));
            img(gpu, 2179, race_image(s.race));
            img(gpu, 2189, colour_image(s.colour));
            // The logo arrows: not for a computer's row (0x48a450).
            self.show_logo(gpu, dlg, 2256, s.logo);
            for l in [2186, 2187] {
                set_enabled(gpu, dlg, l, s.kind != SLOT_AI);
            }
            // Team / race / colour arrows: the release keeps them enabled while ready (only its
            // race- / team-locked map settings disable them); back is disabled while ready.
            for l in [2180, 2181, 2182, 2183, 2184, 2185] {
                set_enabled(gpu, dlg, l, true);
            }
            let mine = &slots[me];
            set_enabled(gpu, dlg, 2141, !mine.ready || host);
            // Ready: hidden for the host of a network game; enabled by CheckReady.
            set_visible(gpu, dlg, 2244, !host);
            set_enabled(gpu, dlg, 2244, mine.ready || lobby::can_be_ready(slots, me));
            if let Some(ui) = gpu.ui.as_mut()
                && let Some(i) = ui.state.set.dialog(dlg).and_then(|d| d.controls.iter().position(|c| c.link == 2244))
                && let Some(e) = ui.state.effects.get_mut(&(dlg, i))
            {
                *e = if mine.ready { *e | 1 } else { *e & !1 };
            }
        }
        self.row = Some(sel);
        set_visible(gpu, dlg, 2142, host);
        set_enabled(gpu, dlg, 2142, host && lobby::can_start(slots));
        set_visible(gpu, dlg, 2245, false);
        let start = chat.len().saturating_sub(4);
        for k in 0..4 {
            set_text(gpu, dlg, 2240 + k as u16, chat.get(start + k).map_or("", |s| s.as_str()));
        }
    }

    /// Mouse commands of the network screens.
    pub(super) fn net_command(&mut self, gpu: &mut Gpu, dialog: usize, link: u16) -> Option<Action> {
        match (dialog, link) {
            // [ours] A network save from the load list: hosted on the LAN or through the relay.
            (MP_TYPE, 2520 | 2521) if self.pending_save.is_some() => {
                let path = self.pending_save.take()?;
                let hosted = if link == 2520 { self.host_saved_internet(gpu, &path) } else { self.host_saved(gpu, &path) };
                if let Err(e) = hosted {
                    tracing::warn!("host {}: {e}", path.display());
                    self.message(gpu, format!("{e}"), LOAD);
                }
            }
            (MP_TYPE, 2520 | 2521) => {
                self.internet = link == 2520;
                self.open(gpu, GAME_LIST);
            }
            (MP_TYPE, 2523) if self.pending_save.take().is_some() => self.open(gpu, LOAD),
            (MP_TYPE, 2523) => self.open(gpu, MAIN),
            (GAME_LIST, 2273) => {
                self.net = NetUi::None;
                self.open(gpu, MP_TYPE);
            }
            (GAME_LIST, 2311) => {
                // Create: CStateLobbyMapSettings with the multiplayer maps.
                self.save_name(gpu);
                self.multiplayer = true;
                self.category = 1;
                self.open(gpu, MAP_SETTINGS);
            }
            (GAME_LIST, 2309 | 2310) => {
                let n = self.listed().map_or(0, |l| l.0.len());
                if let NetUi::Browse { top, .. } | NetUi::Internet { top, .. } = &mut self.net {
                    let was = *top;
                    *top = if link == 2309 { top.saturating_sub(1) } else { (*top + 1).min(n.saturating_sub(8)) };
                    if *top != was {
                        self.play(gpu, crate::sound::gui::SUB_BUTTON_UP);
                    }
                }
                self.fill_games(gpu);
                render(gpu);
            }
            (GAME_LIST, 2277..=2284) => {
                // Join row i (0x479970 → event 60).
                self.save_name(gpu);
                let i = (link - 2277) as usize;
                let (disc, top) = match &self.net {
                    NetUi::Browse { disc, top, .. } | NetUi::Internet { relay: Relay::Connected(disc), top, .. } => (disc, *top),
                    _ => return None,
                };
                let (addr, info, _) = disc.games.get(top + i)?.clone();
                let path = s4_viewer::scene::resolve_map(&info.map);
                let hash = match s4_net::map_hash(&path) {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::warn!("join: the map {} is not here: {e}", info.map);
                        return None;
                    }
                };
                let name = self.player_name(gpu);
                // A loaded game: our own save of it (the player we were saved as).
                let save = if info.load_hash == 0 {
                    (0, 0)
                } else {
                    let saves = match &self.save_dir {
                        Some(d) => crate::save::network_saves_in(d, info.load_hash),
                        None => crate::save::network_saves(info.load_hash),
                    };
                    match saves.into_iter().find(|s| s.1 != info.host) {
                        Some((_, me)) => (info.load_hash, me),
                        None => {
                            tracing::warn!("join: no save of the loaded game \"{}\" here", info.name);
                            return None;
                        }
                    }
                };
                if let NetUi::Internet { server, .. } = &self.net {
                    let server = with_port(server);
                    let to = server.clone();
                    self.connecting(gpu, server, move || {
                        let c = ClientLobby::connect_internet(&to, addr, &name, hash)?;
                        Ok(Opened::Client(Box::new(c.with_save(save.0, save.1)), info.name))
                    });
                    return None;
                }
                match ClientLobby::connect("0.0.0.0:0", addr, &name, hash).map(|c| c.with_save(save.0, save.1)) {
                    Ok(c) => {
                        self.joined_name = info.name.clone();
                        self.net = NetUi::Client(Box::new(c));
                        self.open(gpu, LOBBY);
                    }
                    Err(e) => tracing::warn!("join {addr}: {e}"),
                }
            }
            (LOCAL_MAP, 2433) => {
                // Event 0x40 (local): the GameState with the choices, then the game settings
                // (dialog 19) with the computers at the chosen difficulty.
                let path = self.selected.map(|i| self.maps[i].path.clone())?;
                let m = self.map_read.as_ref()?;
                let name = crate::settings::Settings::load().player_name;
                let (mut slots, me) = setup::start_slots(m, &self.choice, true, self.difficulty, &name);
                // [inf] A map without an open slot leaves the local player out in the release
                // (0x5a7ee0 finds no slot); here the player takes the first slot.
                let me = me.unwrap_or_else(|| {
                    if let Some(s) = slots.first_mut() {
                        *s = SlotInfo { kind: SLOT_HUMAN, name: name.clone(), ready: false, level: 0, human_steps: 0, ..s.clone() };
                    }
                    0
                });
                let c = self.choice;
                let settings = GameSettings { map: path.to_string_lossy().into_owned(), teams: c.teams as u8, mode: c.mode as u8, goods: c.goods as u8, race_goods: race_goods(c.mode), ..Default::default() };
                self.net = NetUi::Local(Box::new(LocalLobby { slots, settings, me }));
                self.open(gpu, LOCAL_GAME);
            }
            (MAP_SETTINGS, 2433) if self.multiplayer => {
                // CStateLobbyMapSettings::OnEvent 64: host the selected map with the choices.
                let path = self.selected.map(|i| self.maps[i].path.clone())?;
                let m = self.map_read.clone()?;
                let c = self.choice;
                let name = crate::settings::Settings::load().player_name;
                let game = self.text(789).replace("%s", &name);
                let map = match net_map_name(&path) {
                    Ok(m) => m,
                    Err(e) => {
                        self.message(gpu, format!("{e}"), MAP_SETTINGS);
                        return None;
                    }
                };
                let settings = s4_net::GameSettings {
                    load_tick: 0,
                    load_hash: 0,
                    map,
                    delay_ms: crate::network_time_delta(),
                    hash_interval: 64,
                    map_hash: s4_net::map_hash(&path).ok()?,
                    teams: c.teams as u8,
                    mode: c.mode as u8,
                    goods: c.goods as u8,
                    race_goods: race_goods(c.mode),
                    // The host's difficulty (0x12987ec): a player who leaves becomes a computer at it.
                    ai_level: self.difficulty,
                };
                let (mut slots, _) = setup::start_slots(&m, &c, false, self.difficulty, &name);
                // [inf] The host is player 1 here (s4-net): slot 0 is the host's even when the
                // map's first open slot is another.
                if let Some(s) = slots.first_mut() {
                    *s = SlotInfo { kind: SLOT_HUMAN, name: name.clone(), ready: false, level: 0, human_steps: 0, ..s.clone() };
                }
                for s in slots.iter_mut().skip(1).filter(|s| s.kind == SLOT_HUMAN) {
                    s.kind = s4_net::lobby::SLOT_OPEN;
                    s.name.clear();
                }
                if let Err(e) = self.host(gpu, &name, &game, slots, settings) {
                    tracing::warn!("host: {e}");
                }
            }
            (LOCAL_GAME, 2141) => {
                self.net = NetUi::None;
                self.open(gpu, LOCAL_MAP);
            }
            (LOCAL_GAME, 2142) => {
                let NetUi::Local(l) = &mut self.net else { return None };
                if !lobby::can_start(&l.slots) {
                    return None;
                }
                // `SetRandomRaces` at the start.
                let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
                lobby::resolve_random_races(&mut l.slots, seed);
                let mut players = setup::player_setup(&l.slots, l.settings.mode as u32, l.me as u8 + 1);
                players.race_goods = l.settings.race_goods;
                players.start_resources = l.settings.goods;
                let map = std::path::PathBuf::from(&l.settings.map);
                return Some(Action::Start(map, None, Some(players)));
            }
            (LOCAL_GAME | LOBBY, 2265..=2272) => {
                // A row's name (0x8d9..0x8e0): `SelectedPlayerIdLobby` (kept by the update only for
                // the host's computer rows).
                self.row = Some((link - 2265) as usize);
                self.fill_lobby(gpu);
                render(gpu);
            }
            (MAP_SETTINGS, 2415) if self.multiplayer => {
                self.multiplayer = false;
                self.open(gpu, GAME_LIST);
            }
            (LOCAL_GAME, 2180..=2185) => {
                let field = match link {
                    2180 | 2181 => FIELD_TEAM,
                    2182 | 2183 => FIELD_RACE,
                    _ => FIELD_COLOUR,
                };
                let row = self.row;
                if let NetUi::Local(l) = &mut self.net {
                    lobby::apply_change(&mut l.slots, row.unwrap_or(l.me), field, matches!(link, 2180 | 2182 | 2184), l.settings.teams);
                }
                self.fill_lobby(gpu);
                render(gpu);
            }
            (LOCAL_GAME, 2186 | 2187) => {
                // Event 0x55 → field 4 (2186 up); then 0x51d234: the own logo goes to
                // `Logo\Current` (0x51dc90(−1, −1)), and the others give way (0x51cec2).
                let row = self.row;
                if let NetUi::Local(l) = &mut self.net {
                    let sel = row.unwrap_or(l.me);
                    if l.slots.get(sel).is_some_and(|s| s.kind != SLOT_AI) {
                        lobby::apply_change(&mut l.slots, sel, FIELD_LOGO, link == 2186, l.settings.teams);
                    }
                    separate_logos(&mut l.slots, l.me);
                }
                if let NetUi::Local(l) = &self.net {
                    let (slots, me) = (l.slots.clone(), l.me);
                    self.take_logos(&slots, Some(me));
                }
                self.fill_lobby(gpu);
                render(gpu);
            }
            (LOCAL_GAME, 2222..=2229) => {
                if let NetUi::Local(l) = &mut self.net
                    && (link - 2222) as usize != l.me
                {
                    lobby::cycle_slot(&mut l.slots, (link - 2222) as usize, false);
                }
                self.fill_lobby(gpu);
                render(gpu);
            }
            (LOBBY, 2141) => {
                if let NetUi::Client(c) = &self.net {
                    c.leave();
                }
                self.net = NetUi::None;
                self.open(gpu, GAME_LIST);
            }
            (LOBBY, 2180..=2187 | 2244) => {
                let (field, up) = match link {
                    2186 => (FIELD_LOGO, true),
                    2187 => (FIELD_LOGO, false),
                    2181 => (FIELD_TEAM, false),
                    2180 => (FIELD_TEAM, true),
                    2183 => (FIELD_RACE, false),
                    2182 => (FIELD_RACE, true),
                    2185 => (FIELD_COLOUR, false),
                    2184 => (FIELD_COLOUR, true),
                    _ => (FIELD_READY, true),
                };
                let row = self.row.unwrap_or(0);
                match &mut self.net {
                    NetUi::Host(h) => {
                        let me = h.me;
                        h.change(if field == FIELD_READY { me } else { row }, field, up)
                    }
                    NetUi::Client(c) => c.change(field, up),
                    _ => {}
                }
                self.fill_lobby(gpu);
                render(gpu);
            }
            (LOBBY, 2222..=2229) => {
                if let NetUi::Host(h) = &mut self.net {
                    h.cycle_slot((link - 2222) as usize);
                }
                self.fill_lobby(gpu);
                render(gpu);
            }
            (LOBBY, 2230..=2237) => {
                if let NetUi::Host(h) = &mut self.net {
                    h.kick((link - 2230) as usize);
                }
                self.fill_lobby(gpu);
                render(gpu);
            }
            (LOBBY, 2142) => return self.host_start(gpu),
            _ => {}
        }
        None
    }

    fn save_name(&self, gpu: &Gpu) {
        let name = self.player_name(gpu);
        let mut s = crate::settings::Settings::load();
        if s.player_name != name {
            s.player_name = name;
            s.save();
        }
    }

    /// Host a loaded network game (the save menu's multiplayer branch): the save's map and tick,
    /// its players as the slots — the host takes the slot of the player whose save it is, the
    /// computers stay computers (map-fixed), every other player's slot is open for the peer that
    /// holds that player's save (s4-net checks the save hash and the slot). The slot type button
    /// is off in a loaded game ([inf] the release's +0xab / +0x58e branch of +0x83d enables it
    /// for free map slots, where a computer could only join a saved human's place).
    pub fn host_saved(&mut self, gpu: &mut Gpu, path: &std::path::Path) -> anyhow::Result<()> {
        let (slots, me, name, game, settings) = self.saved_game(path)?;
        let mut h = HostLobby::open(&format!("0.0.0.0:{}", self.lan_port), &name, &game, slots.len().max(1), settings)?;
        h.slots = slots;
        h.me = me;
        self.multiplayer = true;
        self.internet = false;
        self.net = NetUi::Host(Box::new(h));
        self.open(gpu, LOBBY);
        Ok(())
    }

    /// [ours] [`Frontend::host_saved`] through the relay (`Settings::relay`): the same slots and
    /// save checks (the room lists the save's hash and the host's player), registered on a
    /// worker ([`Frontend::connecting`]).
    pub fn host_saved_internet(&mut self, gpu: &mut Gpu, path: &std::path::Path) -> anyhow::Result<()> {
        let (slots, me, name, game, settings) = self.saved_game(path)?;
        let server = with_port(&crate::settings::Settings::load().relay().ok_or_else(|| anyhow::anyhow!(self.text(1945)))?);
        self.multiplayer = true;
        self.internet = true;
        let to = server.clone();
        self.connecting(gpu, server, move || {
            let mut h = HostLobby::open_internet(&to, &name, &game, slots.len().max(1), settings)?;
            h.slots = slots;
            h.me = me;
            Ok(Opened::Host(Box::new(h)))
        });
        Ok(())
    }

    /// A network save as a hosted game: its slots, the host's slot, the host's and the game's
    /// names, the settings.
    fn saved_game(&self, path: &std::path::Path) -> anyhow::Result<(Vec<SlotInfo>, usize, String, String, GameSettings)> {
        let rules = s4_sim::Rules::load_default()?;
        let l = crate::save::read(path, rules)?;
        anyhow::ensure!(l.client.net.is_some(), "{} is not a network save", path.display());
        let me = l.client.local;
        anyhow::ensure!((1..=l.world.player_count()).contains(&me), "{}: player {me} is not in the game", path.display());
        let map = std::path::PathBuf::from(&l.header.map);
        let name = crate::settings::Settings::load().player_name;
        let w = &l.world;
        let saved_name = |p: u8| l.client.net.as_ref().and_then(|n| n.names.iter().find(|e| e.0 == p)).map(|e| e.1.clone()).unwrap_or_default();
        let slots: Vec<SlotInfo> = (1..=w.player_count())
            .map(|p| {
                let pl = w.players.get(p as usize).and_then(|x| x.as_ref());
                let ai = matches!(w.setup.control.get(p as usize).copied().unwrap_or(0), 2 | 3);
                SlotInfo {
                    kind: if p == me { SLOT_HUMAN } else if ai { SLOT_AI } else { s4_net::lobby::SLOT_OPEN },
                    name: if p == me { name.clone() } else if ai { "AI".into() } else { saved_name(p) },
                    race: pl.map_or(0, |x| x.race),
                    team: w.alliances.alliance_id.get(p as usize).copied().unwrap_or(0),
                    colour: p - 1,
                    ready: ai,
                    level: (w.setup.control.get(p as usize).copied() == Some(3)) as u8,
                    fixed: true,
                    logo: p - 1,
                    ..Default::default()
                }
            })
            .collect();
        // The save names the map as the game ran it (an absolute path after a network start):
        // the peers get its name under the Map folder.
        let map_path = s4_viewer::scene::resolve_map(&l.header.map);
        let settings = s4_net::GameSettings {
            map: net_map_name(&map_path)?,
            delay_ms: crate::network_time_delta(),
            hash_interval: 64,
            map_hash: s4_net::map_hash(&map_path)?,
            load_tick: l.header.tick,
            load_hash: l.header.world_hash,
            ai_level: self.difficulty,
            ..Default::default()
        };
        let game = self.text(789).replace("%s", &name);
        let _ = map;
        Ok((slots, me as usize - 1, name, game, settings))
    }

    /// Start (host): the 1017 / 1018 / 1019 handshake, then the game.
    fn host_start(&mut self, gpu: &mut Gpu) -> Option<Action> {
        let NetUi::Host(h) = std::mem::take(&mut self.net) else { return None };
        if !h.can_start() {
            self.net = NetUi::Host(h);
            return None;
        }
        let map = s4_viewer::scene::resolve_map(&h.settings.map);
        let h_mode = h.settings.mode as u32;
        let race_goods = h.settings.race_goods;
        let goods = h.settings.goods;
        match h.start(Duration::from_secs(70)) {
            Ok((session, slots)) => {
                self.take_logos(&slots, None);
                let mut players = setup::player_setup(&slots, h_mode, session.local);
                players.race_goods = race_goods;
                players.start_resources = goods;
                Some(Action::StartNet(Box::new(session), map, players))
            }
            Err(e) => {
                tracing::warn!("start: {e}");
                self.open(gpu, GAME_LIST);
                None
            }
        }
    }

    /// Typed text and per-frame work of the network screens (pump the lobby, refresh). Returns
    /// a game to start.
    pub(super) fn net_tick(&mut self, gpu: &mut Gpu, events: &[UiEvent]) -> Option<Action> {
        for e in events {
            if let UiEvent::EditDone { dialog, link } = *e {
                let text = gpu.ui.as_ref().and_then(|u| u.state.texts.get(&(dialog, link)).cloned()).unwrap_or_default();
                match (dialog, link) {
                    (GAME_LIST, 2312) => self.save_name(gpu),
                    (GAME_LIST, 2313) if self.internet => {
                        // [ours] The relay server: kept in settings.toml, the list from it.
                        let server = text.trim().to_string();
                        set_text(gpu, GAME_LIST, 2313, &server);
                        let mut s = crate::settings::Settings::load();
                        if s.relay_server != server {
                            s.relay_server = server.clone();
                            s.save();
                        }
                        self.net = internet_list(&server);
                        self.fill_games(gpu);
                        render(gpu);
                    }
                    (GAME_LIST, 2313) => {
                        // The typed host is queried directly (the release stores it, 0x149c080).
                        let a = if text.contains(':') { text.clone() } else { format!("{text}:{}", self.lan_port) };
                        match a.parse() {
                            Ok(addr) => {
                                if let NetUi::Browse { direct, last_query, .. } = &mut self.net
                                    && !direct.contains(&addr)
                                {
                                    direct.push(addr);
                                    *last_query = Instant::now() - Duration::from_secs(10);
                                }
                            }
                            Err(_) => tracing::warn!("not an address: {text}"),
                        }
                    }
                    (LOBBY, 2153) if !text.is_empty() => {
                        match &mut self.net {
                            NetUi::Host(h) => h.say(&text),
                            NetUi::Client(c) => c.say(&text),
                            _ => {}
                        }
                        set_text(gpu, LOBBY, 2153, "");
                    }
                    _ => {}
                }
            }
        }
        let mut changed = false;
        let mut action = None;
        let mut failed = None;
        let mut opened = None;
        let mut lobby_lost = false;
        let mut host_lost = false;
        match &mut self.net {
            NetUi::Browse { disc, last_query, direct, .. } => {
                if last_query.elapsed() >= Duration::from_millis(3000) {
                    disc.query(direct);
                    *last_query = Instant::now();
                }
                if disc.pump() && self.screen == GAME_LIST {
                    changed = true;
                }
            }
            NetUi::Internet { relay, server, last_query, .. } => match relay {
                Relay::Connecting(rx) => match rx.try_recv() {
                    Ok(Ok(d)) => {
                        *relay = Relay::Connected(Box::new(d));
                        *last_query = Instant::now() - Duration::from_secs(10);
                        changed = true;
                    }
                    Ok(Err(e)) => failed = Some((Some(e), server.clone())),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => failed = Some((Some(anyhow::anyhow!("relay worker ended")), server.clone())),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                },
                Relay::Connected(disc) => {
                    // Every 3 s as the LAN list (0x51b8df; the relay unlists a game 15 s after its
                    // host stops).
                    if last_query.elapsed() >= Duration::from_millis(3000) {
                        disc.query(&[]);
                        *last_query = Instant::now();
                    }
                    if disc.pump() && self.screen == GAME_LIST {
                        changed = true;
                    }
                    // No list answer for `RELAY_LOST` (10 s): the relay is gone.
                    if disc.relay_lost() {
                        failed = Some((None, server.clone()));
                    }
                }
                Relay::None => {}
            },
            NetUi::Opening { server, rx } => match rx.try_recv() {
                Ok(r) => opened = Some((r, server.clone())),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => opened = Some((Err(anyhow::anyhow!("relay worker ended")), server.clone())),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            },
            NetUi::Host(h) => {
                changed = !h.pump().is_empty();
                lobby_lost = h.relay_lost();
            }
            NetUi::Client(c) => {
                for e in c.pump() {
                    if let LobbyEvent::Rejected(r) = e {
                        tracing::warn!("{r}");
                        self.net = NetUi::None;
                        self.open(gpu, GAME_LIST);
                        return None;
                    }
                    changed = true;
                }
                if c.relay_lost() {
                    lobby_lost = true;
                } else if c.host_lost() {
                    host_lost = true;
                } else if c.started() {
                    let NetUi::Client(c) = std::mem::take(&mut self.net) else { unreachable!() };
                    let map = c.settings.as_ref().map(|s| s4_viewer::scene::resolve_map(&s.map)).unwrap_or_default();
                    let mode = c.settings.as_ref().map_or(0, |s| s.mode as u32);
                    let race_goods = c.settings.as_ref().map_or([0; 7], |s| s.race_goods);
                    let goods = c.settings.as_ref().map_or(0, |s| s.goods);
                    let slots = c.slots.clone();
                    match c.into_session() {
                        Ok(s) => {
                            take_logos(&mut self.logos, &self.logo_list, &slots, None);
                            let mut players = setup::player_setup(&slots, mode, s.local);
                            players.race_goods = race_goods;
                            players.start_resources = goods;
                            action = Some(Action::StartNet(Box::new(s), map, players));
                        }
                        Err(e) => {
                            tracing::warn!("start: {e}");
                            self.open(gpu, GAME_LIST);
                        }
                    }
                }
            }
            NetUi::None | NetUi::Local(_) => {}
        }
        // A lobby whose relay stopped answering: 2509 with the address, then the game list.
        if lobby_lost {
            self.net = NetUi::None;
            let server = crate::settings::Settings::load().relay().unwrap_or_default();
            let text = format!("{} ({})", self.text(2509), with_port(&server));
            self.message(gpu, text, GAME_LIST);
            return None;
        }
        // A client whose host went silent (`HOST_LOST`, LAN or relay): 2418 "Connection to the
        // host lost!", then the game list.
        if host_lost {
            self.net = NetUi::None;
            let t = self.text(2418);
            self.message(gpu, t, GAME_LIST);
            return None;
        }
        // The registration done: the lobby, or why not (the message box, then the game list).
        if let Some((r, server)) = opened {
            match r {
                Ok(Opened::Host(h)) => self.net = NetUi::Host(h),
                Ok(Opened::Client(c, name)) => {
                    self.joined_name = name;
                    self.net = NetUi::Client(c);
                }
                Err(e) => {
                    self.net = NetUi::None;
                    let text = self.relay_error(&e, &server);
                    self.message(gpu, text, GAME_LIST);
                    return None;
                }
            }
            self.open(gpu, LOBBY);
            return None;
        }
        // A relay that can't be reached (or is lost: no error): the old lobby's message in the
        // message box; the list stays "not connected" until the address is entered again.
        if let Some((e, server)) = failed {
            if let NetUi::Internet { relay, .. } = &mut self.net {
                *relay = Relay::None;
            }
            if self.screen == GAME_LIST {
                let server = with_port(&server);
                let text = match e {
                    Some(e) => self.relay_error(&e, &server),
                    None => format!("{} ({server})", self.text(2509)),
                };
                self.message(gpu, text, GAME_LIST);
                return None;
            }
        }
        if changed {
            match self.screen {
                GAME_LIST => self.fill_games(gpu),
                LOBBY => self.fill_lobby(gpu),
                _ => {}
            }
            render(gpu);
        }
        action
    }
}

/// [ours] The label of the internet list's server field (2275; the LAN list's says "IP").
const SERVER_LABEL: &str = "Server (host:port)";

/// The internet list of `server` (registering with it on a worker thread), or an unconnected
/// one for no server.
fn internet_list(server: &str) -> NetUi {
    let relay = if server.is_empty() {
        Relay::None
    } else {
        let (tx, rx) = std::sync::mpsc::channel();
        let to = with_port(server);
        std::thread::spawn(move || {
            let _ = tx.send(Discovery::internet(&to));
        });
        Relay::Connecting(rx)
    };
    NetUi::Internet { server: server.to_string(), relay, last_query: Instant::now(), top: 0 }
}

/// [ours] A map as network games name it (`GameSettings::map`, the relay's room entry): its path
/// under the install's `Map` folder with `/` (`Multiplayer/Arena.map`), which every peer's
/// `resolve_map` finds under its own install; a random map by its key `RD_<key>` (every peer
/// generates it). Never an absolute path: the peers' installs differ, and the relay keeps 80
/// bytes of it (s4-relay `MAX_MAP`). Every map list is under `Map` (`map_dir`); a map elsewhere
/// is refused.
pub(super) fn net_map_name(path: &std::path::Path) -> anyhow::Result<String> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if stem.starts_with("RD_") && s4_mapgen::key::parse(&stem).is_some() {
        return Ok(stem);
    }
    let base = s4_formats::path::resolve(s4_formats::game_dir().join("Map"));
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).ok();
    let rel = path.strip_prefix(&base).ok().map(std::path::Path::to_path_buf).or_else(|| {
        let (p, b) = (canon(path)?, canon(&base)?);
        p.strip_prefix(b).ok().map(std::path::Path::to_path_buf)
    });
    match rel {
        Some(r) if r.components().all(|c| matches!(c, std::path::Component::Normal(_))) => {
            Ok(r.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        }
        _ => anyhow::bail!("{}: the map is not in the game's Map folder ({}), so the other players cannot find it", path.display(), base.display()),
    }
}

/// `host[:port]` with the relay's default port.
fn with_port(server: &str) -> String {
    if server.contains(':') { server.to_string() } else { format!("{server}:{}", s4_net::RELAY_PORT) }
}

impl Frontend {
    /// The message for a relay that can't be reached ([ours], the old lobby's texts): the name
    /// not resolved (`RelayError::NotFound`, or an I/O error) → 2506 "Lobby server not found due
    /// to network problems (DNS)...", no answer (`NoAnswer`, anything else) → 2510 "Lobby server
    /// not found! Please try again."; then the address. A lost relay says 2509 (`net_tick`).
    fn relay_error(&self, e: &anyhow::Error, server: &str) -> String {
        tracing::warn!("relay {server}: {e:#}");
        let id = match e.downcast_ref::<s4_net::RelayError>() {
            Some(s4_net::RelayError::NotFound(_)) => 2506,
            Some(s4_net::RelayError::NoAnswer(_)) => 2510,
            None if e.root_cause().downcast_ref::<std::io::Error>().is_some() => 2506,
            None => 2510,
        };
        format!("{} ({server})", self.text(id))
    }
}

/// The map's name as the list shows it (file stem).
/// The lobby's construction (0x51c1a0, single player and host; not for a savegame): game mode 3
/// draws its seven goods (0x51f5c0) with the CRT `rand`, [inf] seeded from the clock here. A
/// client takes the host's from the settings (0x5a2370).
pub(super) fn race_goods(mode: u32) -> [u8; 7] {
    if mode != 3 {
        return [0; 7];
    }
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos());
    s4_viewer::goods_race::draw([0; 7], &mut s4_viewer::goods_race::crt_rand(seed))
}

fn map_title(info: &GameInfo) -> String {
    std::path::Path::new(&info.map.replace('\\', "/")).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| info.map.clone())
}


/// 0x51dc90: `Logo\Current` slot `p` ← list[logo of `p`] (GameState +0x168; a slot past the
/// table keeps its default, `p`), for `only` or all eight. A missing list entry changes nothing.
fn take_logos(current: &mut [Option<Vec<u8>>; 8], list: &[std::path::PathBuf], slots: &[SlotInfo], only: Option<usize>) {
    for (p, cur) in current.iter_mut().enumerate() {
        if only.is_some_and(|o| o != p) {
            continue;
        }
        let logo = slots.get(p).map_or(p, |s| s.logo as usize);
        if let Some(rgb) = list.get(logo).and_then(|f| s4_formats::logo::read_rgb(f).ok()) {
            *cur = Some(rgb);
        }
    }
}

/// A local game's settings refresh (0x51cec2..0x51cf64): every other slot with the own slot's
/// logo takes the first index in 0..8 no slot has.
fn separate_logos(slots: &mut [SlotInfo], me: usize) {
    let Some(mine) = slots.get(me).map(|s| s.logo) else { return };
    for j in 0..slots.len() {
        if j == me || slots[j].logo != mine {
            continue;
        }
        if let Some(free) = (0..s4_formats::logo::CHOICES).find(|&k| slots.iter().all(|s| s.logo != k)) {
            slots[j].logo = free;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick_until(f: &mut Frontend, gpu: &mut Gpu, secs: u64, mut done: impl FnMut(&Frontend) -> bool) -> Option<Action> {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(secs) {
            if let Some(a) = f.tick(gpu) {
                return Some(a);
            }
            if done(f) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out");
    }

    /// Two start-screen frontends on localhost: the host creates a game on a multiplayer map,
    /// the client finds it by a typed address, joins, both declare themselves ready, chat goes
    /// round, the host starts, and both get the network game (session + races). Needs the
    /// install and a free LAN port.
    /// A random map in a network game: the host's random tab (dialog 6), start → its lobby with
    /// the map `RD_<key>`; the client generates the map from the name (its file removed first)
    /// and both start on the same file. Needs the install and a free LAN port.
    #[test]
    fn random_map_network_game() {
        if !s4_formats::game_dir().join("Menu").join(super::super::random::SET).exists() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            random_run();
            let _ = tx.send(());
        });
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("random lobby test: no result in 120 s"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("random lobby test failed (see above)"),
        }
    }

    fn random_run() {
        use super::super::random;
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut gh = Gpu::headless(800, 600).unwrap();
        let mut host = Frontend::new(&mut gh, 0).unwrap();
        host.lan_port = port;
        for (d, l) in [(MAIN, 2536), (MP_TYPE, 2521), (GAME_LIST, 2311), (MAP_SETTINGS, 2507)] {
            host.command(&mut gh, d, l, 0);
        }
        assert_eq!(host.screen, random::NETWORK);
        // Size 256 and a seed of this test's own (its file is removed below).
        for _ in 0..4 {
            host.command(&mut gh, random::NETWORK, 0x7ad, 0);
        }
        gh.ui.as_mut().unwrap().state.texts.insert((random::NETWORK, random::SEED), format!("{}", 400_000 + std::process::id() % 500_000));
        host.random_edit(&mut gh, random::SEED);
        host.command(&mut gh, random::NETWORK, random::START, 0);
        assert_eq!(host.screen, LOBBY);
        let NetUi::Host(h) = &host.net else { panic!("hosting") };
        let name = h.settings.map.clone();
        let game = h.game_name.clone();
        assert!(name.starts_with("RD_"), "{name}");
        let file = s4_mapgen::map_path(&name).unwrap();
        std::fs::remove_file(&file).unwrap();

        let client = std::thread::spawn(move || {
            let mut gc = Gpu::headless(800, 600).unwrap();
            let mut c = Frontend::new(&mut gc, 0).unwrap();
            c.lan_port = port;
            c.command(&mut gc, MAIN, 2536, 0);
            c.command(&mut gc, MP_TYPE, 2521, 0);
            gc.ui.as_mut().unwrap().state.focus = Some((GAME_LIST, 2313));
            c.key_text(&mut gc, "127.0.0.1\r");
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Browse { disc, .. } if disc.games.iter().any(|g| g.0.port() == port)));
            if let NetUi::Browse { disc, .. } = &mut c.net {
                disc.games.retain(|g| g.0.port() == port);
            }
            c.command(&mut gc, GAME_LIST, 2277, 0);
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Client(c) if c.slots.len() >= 2 && c.me == Some(2)));
            // The client's lobby names the game as the host's does.
            c.fill_lobby(&mut gc);
            assert_eq!(gc.ui.as_ref().unwrap().state.texts.get(&(LOBBY, 2145)), Some(&game));
            c.command(&mut gc, LOBBY, 2244, 0); // ready
            match tick_until(&mut c, &mut gc, 30, |_| false) {
                Some(Action::StartNet(s, map, _)) => (s.local, map),
                _ => panic!("no start"),
            }
        });
        tick_until(&mut host, &mut gh, 20, |f| matches!(&f.net, NetUi::Host(h) if h.slots[1].ready));
        let Some(Action::StartNet(s, map, players)) = host.command(&mut gh, LOBBY, 2142, 0) else { panic!("host did not start") };
        assert_eq!(s.local, 1);
        let (local, cmap) = client.join().unwrap();
        assert_eq!((local, &cmap), (2, &map));
        assert!(map.ends_with(format!("{name}.map")));
        assert_eq!(players.players, Some(2));
        let m = s4_formats::map::MapFile::open(&map).unwrap();
        assert_eq!((m.general().unwrap().size, m.players().unwrap().len()), (256, 2));
    }

    #[test]
    fn lobby_screens_from_create_to_start() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        // A hard limit: a stuck peer fails the test instead of blocking the suite.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            lobby_run();
            let _ = tx.send(());
        });
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("lobby test: no result in 120 s"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("lobby test failed (see above)"),
        }
    }

    /// A loaded network game through the start screens: two network saves of one 2-player game
    /// (player 1's and player 2's); player 2's peer hosts (`host_saved`: the save's map and tick,
    /// the host in slot 2, slot 1 open), the client finds the game, picks its own save (not the
    /// host's player) and joins as player 1; both sessions start at the saved tick. A client without the save is turned
    /// away. Needs the install and a free LAN port.
    #[test]
    fn loaded_network_game_through_the_lobby() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        loaded_with_limit(false);
    }

    /// The same through a relay of the test's own: the host picks the save, then dialog 1's
    /// Internet (`host_saved_internet`); the clients find the game in the internet list (its save
    /// hash and host player listed by the relay). Needs the install.
    #[test]
    fn loaded_network_game_through_a_relay() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        loaded_with_limit(true);
    }

    fn loaded_with_limit(internet: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            loaded_run(internet);
            let _ = tx.send(());
        });
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("loaded lobby test: no result in 120 s"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("loaded lobby test failed (see above)"),
        }
    }

    fn loaded_run(internet: bool) {
        // The saves: a 2-player game on a multiplayer map, 30 ticks in.
        let map_name = "Multiplayer/AncientGlory";
        let map_path = s4_viewer::scene::resolve_map(map_name);
        let map = s4_formats::map::MapFile::open(&map_path).unwrap();
        let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default().unwrap(), &s4_sim::NewGame { players: Some(2), ..Default::default() }).unwrap();
        for _ in 0..30 {
            s4_sim::tick(&mut w, &[]);
        }
        let dir = std::env::temp_dir().join(format!("s4rs-test-loadlobby-{}-{internet}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let other = dir.join("other");
        std::fs::create_dir_all(&other).unwrap();
        for p in 1..=2u8 {
            let name = format!("mp - player {p}");
            let input = crate::save::SaveInput {
                name: &name,
                thumb: Vec::new(),
                map: std::path::Path::new(map_name),
                world: &w,
                script: None,
                host: Default::default(),
                commands: Vec::new(),
                client: crate::save::ClientState { local: p, net: Some(crate::save::NetResume { names: vec![(1, "Ann".into()), (2, "Bob".into())], ..Default::default() }), ..Default::default() },
            };
            crate::save::write(&dir.join(format!("{name}.s4rs")), input).unwrap();
        }
        let hash = w.state_hash();
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let relay = internet.then(|| s4_relay::spawn("127.0.0.1:0", s4_relay::Config::default()).unwrap());
        // 'static so the join closure stays Copy (it runs in two threads).
        let server: &'static str = Box::leak(relay.as_ref().map(|r| r.0.to_string()).unwrap_or_default().into_boxed_str());
        let file = internet.then(|| relay_settings(server, "loadhost"));
        let mut gh = Gpu::headless(800, 600).unwrap();
        let mut host = Frontend::new(&mut gh, 0).unwrap();
        host.lan_port = port;
        // Player 2's peer hosts (its slot), player 1's joins.
        let save = dir.join("mp - player 2.s4rs");
        if internet {
            // The load list's pick, then dialog 1: back returns to the load list.
            host.pending_save = Some(save.clone());
            host.open(&mut gh, MP_TYPE);
            host.command(&mut gh, MP_TYPE, 2523, 0);
            assert_eq!((host.screen, host.pending_save.is_none()), (LOAD, true));
            // Again, then Internet.
            host.pending_save = Some(save);
            host.open(&mut gh, MP_TYPE);
            host.command(&mut gh, MP_TYPE, 2520, 0);
            assert!(matches!(host.net, NetUi::Opening { .. }));
            tick_until(&mut host, &mut gh, 10, |f| f.screen == LOBBY);
            let NetUi::Host(h) = &host.net else { panic!("hosting") };
            assert!(h.is_internet());
        } else {
            host.host_saved(&mut gh, &save).unwrap();
        }
        assert_eq!(host.screen, LOBBY);
        let NetUi::Host(h) = &host.net else { panic!("hosting") };
        assert_eq!((h.settings.load_tick, h.settings.load_hash), (30, hash));
        assert_eq!(h.slots.iter().map(|s| s.kind).collect::<Vec<_>>(), [s4_net::lobby::SLOT_OPEN, SLOT_HUMAN]);
        assert_eq!(h.me, 1);
        assert_eq!(h.slots[0].name, "Ann", "the open slot keeps its player's saved name");
        assert_eq!(gh.ui.as_ref().unwrap().state.texts.get(&(LOBBY, 2265)).map(String::as_str), Some("Ann"));
        // A client: finds the game by address, joins with the save found in `save_dir`.
        let join = move |save_dir: std::path::PathBuf, expect_start: bool| {
            let file = internet.then(|| relay_settings(server, if expect_start { "loadjoin" } else { "loadnosave" }));
            let mut gc = Gpu::headless(800, 600).unwrap();
            let mut c = Frontend::new(&mut gc, 0).unwrap();
            c.lan_port = port;
            c.save_dir = Some(save_dir);
            c.command(&mut gc, MAIN, 2536, 0);
            if internet {
                c.command(&mut gc, MP_TYPE, 2520, 0);
                tick_until(&mut c, &mut gc, 15, |f| f.listed().is_some_and(|l| !l.0.is_empty()));
                let (games, _) = c.listed().unwrap();
                assert_eq!((games.len(), games[0].load_hash, games[0].host), (1, hash, 2), "the relay lists the loaded game");
            } else {
                c.command(&mut gc, MP_TYPE, 2521, 0);
                gc.ui.as_mut().unwrap().state.focus = Some((GAME_LIST, 2313));
                c.key_text(&mut gc, "127.0.0.1\r");
                tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Browse { disc, .. } if disc.games.iter().any(|g| g.0.port() == port)));
                if let NetUi::Browse { disc, .. } = &mut c.net {
                    disc.games.retain(|g| g.0.port() == port);
                    assert_eq!(disc.games[0].1.load_hash, hash, "discovery tells the loaded game");
                }
            }
            c.fill_games(&mut gc);
            c.command(&mut gc, GAME_LIST, 2277, 0);
            if let Some(f) = file {
                crate::settings::use_test_file(None);
                let _ = std::fs::remove_file(f);
            }
            if !expect_start {
                // No save of it here: the join is not even tried.
                return (matches!(c.net, NetUi::Browse { .. } | NetUi::Internet { .. }), 0, 0);
            }
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Client(c) if c.me == Some(1)));
            c.command(&mut gc, LOBBY, 2244, 0); // ready
            match tick_until(&mut c, &mut gc, 30, |_| false) {
                Some(Action::StartNet(s, _, _)) => (true, s.local, s.settings.load_tick),
                _ => panic!("no start"),
            }
        };
        let first = std::thread::spawn({
            let other = other.clone();
            move || join(other, false)
        });
        // The host answers discovery meanwhile.
        while !first.is_finished() {
            let _ = host.tick(&mut gh);
            std::thread::sleep(Duration::from_millis(5));
        }
        let (stayed, _, _) = first.join().unwrap();
        assert!(stayed, "a peer without the save stays in the game list");
        let client = std::thread::spawn({
            let dir = dir.clone();
            move || join(dir, true)
        });
        tick_until(&mut host, &mut gh, 20, |f| matches!(&f.net, NetUi::Host(h) if h.slots[0].ready));
        let Some(Action::StartNet(s, _, _)) = host.command(&mut gh, LOBBY, 2142, 0) else { panic!("host did not start") };
        assert_eq!((s.local, s.settings.load_tick, s.settings.load_hash), (2, 30, hash));
        assert_eq!(client.join().unwrap(), (true, 1, 30));
        assert_eq!(crate::save::network_saves_in(&dir, hash).iter().map(|s| s.1).collect::<std::collections::BTreeSet<_>>(), [1, 2].into());
        let _ = std::fs::remove_dir_all(&dir);
        if let Some(f) = file {
            crate::settings::use_test_file(None);
            let _ = std::fs::remove_file(f);
        }
        if let Some((_, stop, _)) = relay {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn lobby_run() {
        // A free port of our own: parallel runs (other agents' test processes) never meet.
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut gh = Gpu::headless(800, 600).unwrap();
        let mut host = Frontend::new(&mut gh, 0).unwrap();
        host.lan_port = port;
        for (d, l) in [(MAIN, 2536), (MP_TYPE, 2521), (GAME_LIST, 2311)] {
            host.command(&mut gh, d, l, 0);
        }
        assert_eq!(host.screen, MAP_SETTINGS);
        assert!(host.multiplayer && !host.maps.is_empty());
        // Two players (event 0x44 down): every slot must be taken to start.
        for _ in 0..8 {
            host.command(&mut gh, MAP_SETTINGS, 2442, 0);
        }
        assert_eq!(host.choice.players, 2);
        // The client's race + goes from the map's race of slot 2 to the next of the cycle.
        let cycle = [0u8, 1, 2, 4, 5];
        let r = host.map_read.as_ref().unwrap().slots[1].race;
        let next = cycle[(cycle.iter().position(|&c| c == r).unwrap() + 1) % 5];
        host.command(&mut gh, MAP_SETTINGS, 2433, 0);
        assert_eq!(host.screen, LOBBY);
        assert!(matches!(host.net, NetUi::Host(_)));

        let client = std::thread::spawn(move || {
            let mut gc = Gpu::headless(800, 600).unwrap();
            let mut c = Frontend::new(&mut gc, 0).unwrap();
            c.lan_port = port;
            c.command(&mut gc, MAIN, 2536, 0);
            c.command(&mut gc, MP_TYPE, 2521, 0);
            // Type the host's address into 2313.
            gc.ui.as_mut().unwrap().state.focus = Some((GAME_LIST, 2313));
            c.key_text(&mut gc, "127.0.0.1\r");
            // Only our host (a LAN broadcast may find others).
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Browse { disc, .. } if disc.games.iter().any(|g| g.0.port() == port)));
            if let NetUi::Browse { disc, .. } = &mut c.net {
                disc.games.retain(|g| g.0.port() == port);
            }
            c.command(&mut gc, GAME_LIST, 2277, 0);
            assert_eq!(c.screen, LOBBY);
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Client(c) if c.slots.len() >= 2 && c.me == Some(2)));
            c.command(&mut gc, LOBBY, 2182, 0); // race +
            c.command(&mut gc, LOBBY, 2186, 0); // logo + (1 → 2)
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Client(c) if c.slots[1].logo == 2));
            let list = s4_formats::logo::list(&s4_formats::game_dir());
            let shown = gc.ui.as_ref().unwrap().state.owner_images.get(&(LOBBY, 2258)).map(|i| i.2.clone());
            assert_eq!(shown, s4_formats::logo::read_rgb(&list[2]).ok().map(|c| s4_formats::logo::compose_rgba(&c)), "row 2 shows logo 2");
            c.command(&mut gc, LOBBY, 2244, 0); // ready
            gc.ui.as_mut().unwrap().state.focus = Some((LOBBY, 2153));
            c.key_text(&mut gc, "hello\r");
            let started = match tick_until(&mut c, &mut gc, 30, |_| false) {
                Some(Action::StartNet(s, map, players)) => (s.local, map, players),
                _ => panic!("no start"),
            };
            // At the start every slot takes its logo into `Logo\Current` (0x51c710).
            assert_eq!(c.logos[1], s4_formats::logo::read_rgb(&list[2]).ok());
            started
        });

        // Host: wait for the client's ready and chat, then ready and start.
        tick_until(&mut host, &mut gh, 20, |f| matches!(&f.net, NetUi::Host(h) if h.slots[1].ready && !h.chat.is_empty()));
        let NetUi::Host(h) = &host.net else { panic!() };
        assert!(h.can_start());
        assert_eq!(h.chat, vec![format!("{}: hello", h.slots[1].name)]);
        let Some(Action::StartNet(s, map, players)) = host.command(&mut gh, LOBBY, 2142, 0) else { panic!("host did not start") };
        assert_eq!(s.local, 1);
        if next != 5 {
            assert_eq!(players.races[1], Some(next), "the client's race change");
        }
        let (local, cmap, cplayers) = client.join().unwrap();
        assert_eq!((local, cmap, cplayers.races, cplayers.teams, cplayers.control), (2, map, players.races, players.teams, players.control));
        assert_eq!((players.local, cplayers.local), (1, 2));
    }

    fn capture(gpu: &mut Gpu, name: &str) {
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui").join(name);
            let _ = s4_viewer::client::write_png(&out, img.width, img.height, &img.rgba);
        }
    }

    fn text_of(gpu: &Gpu, dialog: usize, link: u16) -> String {
        gpu.ui.as_ref().unwrap().state.texts.get(&(dialog, link)).cloned().unwrap_or_default()
    }

    /// The internet screens: dialog 1's "Internet" (2520) opens the game list in internet mode
    /// (title string 2027, the server field 2313 from settings.toml, the status row); an address
    /// typed there is saved and connected to; a server that can't be reached shows the lobby
    /// message 2510 in the message box once, whose OK returns to the list; the LAN list (2521)
    /// is as before. Captures `out/gui/internet_*.png`.
    #[test]
    fn internet_list_and_unreachable_server() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let file = std::env::temp_dir().join(format!("s4rs_internet_{}.toml", std::process::id()));
        // A fresh file names the project's relay; this test starts without one (no traffic to
        // the real server).
        assert_eq!(crate::settings::Settings::default().relay_server, "set4.online:7720");
        let _ = std::fs::remove_file(&file);
        crate::settings::use_test_file(Some(file.clone()));
        crate::settings::Settings { relay_server: String::new(), ..Default::default() }.save();
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2537, 0);
        assert_eq!(f.screen, MP_TYPE);
        let st = &gpu.ui.as_ref().unwrap().state;
        assert!(st.index_of(MP_TYPE, 2520).is_some_and(|k| st.effects.get(&(MP_TYPE, k)).is_none_or(|e| e & 4 == 0)), "Internet offered");
        capture(&mut gpu, "internet_type.png");
        f.command(&mut gpu, MP_TYPE, 2520, 0);
        assert_eq!(f.screen, GAME_LIST);
        assert!(matches!(&f.net, NetUi::Internet { relay: Relay::None, .. }));
        assert_eq!(text_of(&gpu, GAME_LIST, 2322), f.text(2027));
        assert_eq!(text_of(&gpu, GAME_LIST, 2275), SERVER_LABEL);
        assert_eq!(text_of(&gpu, GAME_LIST, 2313), "");
        assert_eq!(text_of(&gpu, GAME_LIST, 2277), f.text(1945), "no server: not connected");
        let create_on = |gpu: &Gpu| {
            let st = &gpu.ui.as_ref().unwrap().state;
            st.index_of(GAME_LIST, 2311).is_some_and(|k| st.effects.get(&(GAME_LIST, k)).is_none_or(|e| e & 4 == 0))
        };
        assert!(!create_on(&gpu), "Create needs the server");
        capture(&mut gpu, "internet_list.png");

        // A closed port on this machine: refused at once.
        gpu.ui.as_mut().unwrap().state.focus = Some((GAME_LIST, 2313));
        f.key_text(&mut gpu, "127.0.0.1:1\r");
        assert_eq!(crate::settings::Settings::load().relay_server, "127.0.0.1:1");
        assert_eq!(text_of(&gpu, GAME_LIST, 2277), f.text(1944), "connecting");
        capture(&mut gpu, "internet_connecting.png");
        tick_until(&mut f, &mut gpu, 10, |f| f.screen == super::super::msgbox::DIALOG);
        let msg = text_of(&gpu, super::super::msgbox::DIALOG, super::super::msgbox::TEXT);
        assert!(msg.starts_with(&f.text(2510)) && msg.ends_with("(127.0.0.1:1)"), "{msg}");
        capture(&mut gpu, "internet_unreachable.png");
        // A name that does not resolve says DNS.
        let dns = anyhow::Error::new(s4_net::RelayError::NotFound("nowhere.invalid:7720".into()));
        assert!(f.relay_error(&dns, "nowhere.invalid:7720").starts_with(&f.text(2506)));
        f.command(&mut gpu, super::super::msgbox::DIALOG, super::super::msgbox::OK, 0);
        assert_eq!(f.screen, GAME_LIST);
        assert_eq!(text_of(&gpu, GAME_LIST, 2277), f.text(1945));
        assert_eq!(text_of(&gpu, GAME_LIST, 2313), "127.0.0.1:1");
        // Reported once: the list stays "not connected".
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(300) {
            f.tick(&mut gpu);
        }
        assert_eq!(f.screen, GAME_LIST);
        // Hosting (or joining) registers on a worker under "Status: Connect..."; Esc cancels.
        f.host(&mut gpu, "me", "game", vec![SlotInfo::default()], GameSettings::default()).unwrap();
        assert!(matches!(&f.net, NetUi::Opening { .. }));
        assert_eq!((f.screen, text_of(&gpu, super::super::msgbox::DIALOG, super::super::msgbox::TEXT)), (super::super::msgbox::DIALOG, f.text(1944)));
        capture(&mut gpu, "internet_opening.png");
        assert!(f.escape(&mut gpu));
        assert_eq!(f.screen, GAME_LIST);
        assert!(matches!(&f.net, NetUi::Internet { .. }));

        // Back and in again: the saved server; the LAN list is the LAN one.
        f.command(&mut gpu, GAME_LIST, 2273, 0);
        f.command(&mut gpu, MP_TYPE, 2520, 0);
        assert_eq!(text_of(&gpu, GAME_LIST, 2313), "127.0.0.1:1");
        assert!(matches!(&f.net, NetUi::Internet { relay: Relay::Connecting(_), .. }));
        f.command(&mut gpu, GAME_LIST, 2273, 0);
        f.command(&mut gpu, MP_TYPE, 2521, 0);
        assert!(matches!(&f.net, NetUi::Browse { .. }));
        assert!(create_on(&gpu));
        assert_eq!([text_of(&gpu, GAME_LIST, 2275), text_of(&gpu, GAME_LIST, 2313), text_of(&gpu, GAME_LIST, 2322)], ["IP", "", ""]);
        crate::settings::use_test_file(None);
        let _ = std::fs::remove_file(&file);
    }

    /// An internet game through a relay of the test's own (`s4_relay::spawn` on localhost, put
    /// into each side's test settings): the host's list connects, Create → map settings → its
    /// lobby is listed at the relay; the client's list shows it (captured), the client joins,
    /// declares itself ready, the host starts, and both get the network game. Needs the install.
    #[test]
    fn internet_game_through_a_relay() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            internet_run();
            let _ = tx.send(());
        });
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("internet test: no result in 120 s"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("internet test failed (see above)"),
        }
    }

    /// This thread's settings: a file of its own naming `relay`.
    fn relay_settings(relay: &str, who: &str) -> std::path::PathBuf {
        let file = std::env::temp_dir().join(format!("s4rs_relay_{who}_{}.toml", std::process::id()));
        crate::settings::use_test_file(Some(file.clone()));
        crate::settings::Settings { relay_server: relay.to_string(), player_name: who.to_string(), ..Default::default() }.save();
        file
    }

    fn internet_run() {
        let (relay, stop, server) = s4_relay::spawn("127.0.0.1:0", s4_relay::Config::default()).unwrap();
        let relay = relay.to_string();
        let file = relay_settings(&relay, "host");
        let mut gh = Gpu::headless(800, 600).unwrap();
        let mut host = Frontend::new(&mut gh, 0).unwrap();
        host.command(&mut gh, MAIN, 2537, 0);
        host.command(&mut gh, MP_TYPE, 2520, 0);
        assert_eq!(text_of(&gh, GAME_LIST, 2313), relay);
        tick_until(&mut host, &mut gh, 10, |f| matches!(&f.net, NetUi::Internet { relay: Relay::Connected(_), .. }));
        host.command(&mut gh, GAME_LIST, 2311, 0);
        assert_eq!(host.screen, MAP_SETTINGS);
        for _ in 0..8 {
            host.command(&mut gh, MAP_SETTINGS, 2442, 0);
        }
        host.command(&mut gh, MAP_SETTINGS, 2433, 0);
        assert_eq!(host.screen, super::super::msgbox::DIALOG, "registering");
        tick_until(&mut host, &mut gh, 10, |f| f.screen == LOBBY);
        let NetUi::Host(h) = &host.net else { panic!("hosting") };
        assert!(h.is_internet());
        let game = h.game_name.clone();

        let client = std::thread::spawn(move || {
            let file = relay_settings(&relay, "client");
            let mut gc = Gpu::headless(800, 600).unwrap();
            let mut c = Frontend::new(&mut gc, 0).unwrap();
            c.command(&mut gc, MAIN, 2537, 0);
            c.command(&mut gc, MP_TYPE, 2520, 0);
            tick_until(&mut c, &mut gc, 15, |f| f.listed().is_some_and(|l| l.0.iter().any(|g| g.name == game)));
            c.fill_games(&mut gc);
            assert_eq!(text_of(&gc, GAME_LIST, 2277), game);
            capture(&mut gc, "internet_list_game.png");
            c.command(&mut gc, GAME_LIST, 2277, 0);
            assert_eq!(c.screen, super::super::msgbox::DIALOG, "registering");
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Client(c) if c.slots.len() >= 2 && c.me == Some(2)));
            assert_eq!(c.screen, LOBBY);
            c.command(&mut gc, LOBBY, 2244, 0); // ready
            let started = match tick_until(&mut c, &mut gc, 30, |_| false) {
                Some(Action::StartNet(s, _, _)) => s.local,
                _ => panic!("no start"),
            };
            crate::settings::use_test_file(None);
            let _ = std::fs::remove_file(file);
            started
        });

        tick_until(&mut host, &mut gh, 20, |f| matches!(&f.net, NetUi::Host(h) if h.slots[1].ready));
        capture(&mut gh, "internet_lobby.png");
        let Some(Action::StartNet(s, _, _)) = host.command(&mut gh, LOBBY, 2142, 0) else { panic!("host did not start") };
        assert_eq!((s.local, client.join().unwrap()), (1, 2));
        crate::settings::use_test_file(None);
        let _ = std::fs::remove_file(file);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        drop(s);
        let _ = server.join();
    }

    /// The relay goes away under an open internet list: after `RELAY_LOST` (10 s) without a list
    /// answer the message box says 2509 "Connection to lobby server lost!", then the list is "not
    /// connected". Needs the install.
    #[test]
    fn internet_list_lost_relay() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let (relay, stop, server) = s4_relay::spawn("127.0.0.1:0", s4_relay::Config::default()).unwrap();
        let relay = relay.to_string();
        let file = relay_settings(&relay, "lost");
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2537, 0);
        f.command(&mut gpu, MP_TYPE, 2520, 0);
        tick_until(&mut f, &mut gpu, 10, |f| matches!(&f.net, NetUi::Internet { relay: Relay::Connected(_), .. }));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server.join();
        tick_until(&mut f, &mut gpu, 30, |f| f.screen == super::super::msgbox::DIALOG);
        let msg = text_of(&gpu, super::super::msgbox::DIALOG, super::super::msgbox::TEXT);
        assert_eq!(msg, format!("{} ({relay})", f.text(2509)));
        capture(&mut gpu, "internet_lost.png");
        f.command(&mut gpu, super::super::msgbox::DIALOG, super::super::msgbox::OK, 0);
        assert_eq!(f.screen, GAME_LIST);
        assert_eq!(text_of(&gpu, GAME_LIST, 2277), f.text(1945));
        crate::settings::use_test_file(None);
        let _ = std::fs::remove_file(file);
    }

    /// The relay goes away under a hosted internet lobby: after `RELAY_LOST` the message box says
    /// 2509 with the address, its OK opens the game list. Needs the install.
    #[test]
    fn internet_lobby_lost_relay() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let (relay, stop, server) = s4_relay::spawn("127.0.0.1:0", s4_relay::Config::default()).unwrap();
        let relay = relay.to_string();
        let file = relay_settings(&relay, "lobbylost");
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.command(&mut gpu, MAIN, 2537, 0);
        f.command(&mut gpu, MP_TYPE, 2520, 0);
        tick_until(&mut f, &mut gpu, 10, |f| matches!(&f.net, NetUi::Internet { relay: Relay::Connected(_), .. }));
        f.command(&mut gpu, GAME_LIST, 2311, 0);
        f.command(&mut gpu, MAP_SETTINGS, 2433, 0);
        tick_until(&mut f, &mut gpu, 10, |f| f.screen == LOBBY);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = server.join();
        tick_until(&mut f, &mut gpu, 30, |f| f.screen == super::super::msgbox::DIALOG);
        let msg = text_of(&gpu, super::super::msgbox::DIALOG, super::super::msgbox::TEXT);
        assert_eq!(msg, format!("{} ({relay})", f.text(2509)));
        assert!(matches!(f.net, NetUi::None));
        f.command(&mut gpu, super::super::msgbox::DIALOG, super::super::msgbox::OK, 0);
        assert_eq!(f.screen, GAME_LIST);
        crate::settings::use_test_file(None);
        let _ = std::fs::remove_file(file);
    }

    /// Maps are named under the install's `Map` folder, never by an absolute path.
    #[test]
    fn network_map_names() {
        let base = s4_formats::game_dir().join("Map");
        assert_eq!(net_map_name(&base.join("Multiplayer").join("Arena.map")).unwrap(), "Multiplayer/Arena.map");
        assert_eq!(net_map_name(&base.join("User").join("a b.map")).unwrap(), "User/a b.map");
        assert!(net_map_name(std::path::Path::new(r"D:\elsewhere\Arena.map")).is_err());
        assert!(net_map_name(&base.join("..").join("x.map")).is_err());
    }

    /// A host whose install is somewhere else than the client's: the host runs in a child
    /// process of this test binary with `S4_DIR` = a junction to the install under a long path
    /// (its maps' absolute paths are longer than the relay's 80 bytes, and unknown to the
    /// client), the client here with the normal install. Through the LAN and through a relay
    /// the game list names the map relative to `Map`, the client joins and both start. Needs
    /// the install (and junctions: Windows).
    #[test]
    fn hosts_with_another_install_folder() {
        if !cfg!(windows) || !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join(format!("s4rs-test-roots-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let root = dir.join("an install folder with a rather long name, as under Program Files (x86) and deeper");
            if !root.exists() {
                let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&root).arg(s4_formats::game_dir()).output().is_ok_and(|o| o.status.success());
                assert!(ok, "mklink /J {}", root.display());
            }
            let r = std::panic::catch_unwind(|| {
                other_root_run(&root, false);
                other_root_run(&root, true);
            });
            // The junction only (remove_dir never follows it into the install).
            let _ = std::fs::remove_dir(&root);
            let _ = std::fs::remove_dir(&dir);
            if r.is_ok() {
                let _ = tx.send(());
            }
        });
        match rx.recv_timeout(Duration::from_secs(240)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("other install test: no result in 240 s"),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("other install test failed (see above)"),
        }
    }

    fn other_root_run(root: &std::path::Path, internet: bool) {
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let relay = internet.then(|| s4_relay::spawn("127.0.0.1:0", s4_relay::Config::default()).unwrap());
        let server = relay.as_ref().map(|r| r.0.to_string()).unwrap_or_default();
        // Killed if this side fails (it would wait for its client a minute).
        struct Host(std::process::Child);
        impl Drop for Host {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Host(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["frontend::net::tests::other_root_host", "--exact", "--ignored", "--nocapture"])
                .env("S4_DIR", root)
                .env("S4RS_TEST_HOST", format!("{port};{server}"))
                .spawn()
                .unwrap(),
        );
        let file = internet.then(|| relay_settings(&server, "rootclient"));
        let mut gc = Gpu::headless(800, 600).unwrap();
        let mut c = Frontend::new(&mut gc, 0).unwrap();
        c.lan_port = port;
        c.command(&mut gc, MAIN, 2537, 0);
        if internet {
            c.command(&mut gc, MP_TYPE, 2520, 0);
        } else {
            c.command(&mut gc, MP_TYPE, 2521, 0);
            gc.ui.as_mut().unwrap().state.focus = Some((GAME_LIST, 2313));
            c.key_text(&mut gc, "127.0.0.1\r");
        }
        tick_until(&mut c, &mut gc, 60, |f| match &f.net {
            NetUi::Browse { disc, .. } => disc.games.iter().any(|g| g.0.port() == port),
            _ => f.listed().is_some_and(|l| !l.0.is_empty()),
        });
        if let NetUi::Browse { disc, .. } = &mut c.net {
            disc.games.retain(|g| g.0.port() == port);
        }
        let map = c.listed().unwrap().0[0].map.clone();
        assert!(map.starts_with("Multiplayer/") && map.ends_with(".map") && !map.contains(':'), "the map under Map, not the host's {}: {map}", root.display());
        c.fill_games(&mut gc);
        c.command(&mut gc, GAME_LIST, 2277, 0);
        tick_until(&mut c, &mut gc, 15, |f| matches!(&f.net, NetUi::Client(c) if c.me == Some(2)));
        c.command(&mut gc, LOBBY, 2244, 0); // ready
        let Some(Action::StartNet(s, path, _)) = tick_until(&mut c, &mut gc, 60, |_| false) else { panic!("no start") };
        assert_eq!((s.local, path), (2, s4_formats::game_dir().join("Map").join(&map)));
        assert!(child.0.wait().unwrap().success(), "the host ({})", if internet { "relay" } else { "LAN" });
        if let Some(f) = file {
            crate::settings::use_test_file(None);
            let _ = std::fs::remove_file(f);
        }
        if let Some((_, stop, _)) = relay {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// The host of [`hosts_with_another_install_folder`], run by it in a child process
    /// (`S4RS_TEST_HOST` = `<LAN port>;<relay or empty>`): the first multiplayer map for two, the
    /// game started once the client is ready.
    #[test]
    #[ignore]
    fn other_root_host() {
        let Ok(arg) = std::env::var("S4RS_TEST_HOST") else { return };
        let (port, server) = arg.split_once(';').unwrap();
        let internet = !server.is_empty();
        let file = relay_settings(server, "roothost");
        let mut gpu = Gpu::headless(800, 600).unwrap();
        let mut f = Frontend::new(&mut gpu, 0).unwrap();
        f.lan_port = port.parse().unwrap();
        f.command(&mut gpu, MAIN, 2537, 0);
        f.command(&mut gpu, MP_TYPE, if internet { 2520 } else { 2521 }, 0);
        if internet {
            tick_until(&mut f, &mut gpu, 10, |f| matches!(&f.net, NetUi::Internet { relay: Relay::Connected(_), .. }));
        }
        f.command(&mut gpu, GAME_LIST, 2311, 0);
        let path = f.selected.map(|i| f.maps[i].path.clone()).unwrap();
        assert!(path.starts_with(s4_formats::game_dir()) && path.to_string_lossy().len() > 80, "{}", path.display());
        for _ in 0..8 {
            f.command(&mut gpu, MAP_SETTINGS, 2442, 0);
        }
        assert_eq!(f.choice.players, 2);
        f.command(&mut gpu, MAP_SETTINGS, 2433, 0);
        tick_until(&mut f, &mut gpu, 10, |f| f.screen == LOBBY);
        tick_until(&mut f, &mut gpu, 60, |f| matches!(&f.net, NetUi::Host(h) if h.slots.get(1).is_some_and(|s| s.ready)));
        let Some(Action::StartNet(s, _, _)) = f.command(&mut gpu, LOBBY, 2142, 0) else { panic!("host did not start") };
        assert_eq!(s.local, 1);
        crate::settings::use_test_file(None);
        let _ = std::fs::remove_file(file);
    }

    /// A client whose host goes silent (not pumped: closed, crashed, cut off) leaves the lobby
    /// with 2418 "Connection to the host lost!" after `host_timeout` (10 s; 1 s here). LAN, needs
    /// the install.
    #[test]
    fn client_sees_host_lost() {
        if !s4_formats::game_dir().join("Menu").exists() {
            return;
        }
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut gh = Gpu::headless(800, 600).unwrap();
        let mut host = Frontend::new(&mut gh, 0).unwrap();
        host.lan_port = port;
        for (d, l) in [(MAIN, 2537), (MP_TYPE, 2521), (GAME_LIST, 2311)] {
            host.command(&mut gh, d, l, 0);
        }
        for _ in 0..8 {
            host.command(&mut gh, MAP_SETTINGS, 2442, 0);
        }
        host.command(&mut gh, MAP_SETTINGS, 2433, 0);
        assert_eq!(host.screen, LOBBY);
        let client = std::thread::spawn(move || {
            let mut gc = Gpu::headless(800, 600).unwrap();
            let mut c = Frontend::new(&mut gc, 0).unwrap();
            c.lan_port = port;
            c.command(&mut gc, MAIN, 2537, 0);
            c.command(&mut gc, MP_TYPE, 2521, 0);
            gc.ui.as_mut().unwrap().state.focus = Some((GAME_LIST, 2313));
            c.key_text(&mut gc, "127.0.0.1\r");
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Browse { disc, .. } if disc.games.iter().any(|g| g.0.port() == port)));
            if let NetUi::Browse { disc, .. } = &mut c.net {
                disc.games.retain(|g| g.0.port() == port);
            }
            c.command(&mut gc, GAME_LIST, 2277, 0);
            tick_until(&mut c, &mut gc, 10, |f| matches!(&f.net, NetUi::Client(c) if c.me == Some(2)));
            if let NetUi::Client(cl) = &mut c.net {
                cl.host_timeout = Duration::from_secs(1);
            }
            tick_until(&mut c, &mut gc, 10, |f| f.screen == super::super::msgbox::DIALOG);
            let msg = text_of(&gc, super::super::msgbox::DIALOG, super::super::msgbox::TEXT);
            assert_eq!(msg, c.text(2418));
            assert!(matches!(c.net, NetUi::None));
            capture(&mut gc, "host_lost.png");
            c.command(&mut gc, super::super::msgbox::DIALOG, super::super::msgbox::OK, 0);
            c.screen
        });
        // The host answers until the client is in, then says nothing.
        tick_until(&mut host, &mut gh, 10, |f| matches!(&f.net, NetUi::Host(h) if h.slots[1].kind == SLOT_HUMAN));
        assert_eq!(client.join().unwrap(), GAME_LIST);
    }
}
