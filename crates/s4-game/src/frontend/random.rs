//! The random map parameters, `CStateMDRandomMapParameters` (constructor 0x526040, events
//! 0x5265a0): dialog 7 (local) or 6 (network) of `Menu\GUISetMDStartscreens.DAT` with the
//! procedure `RandomMapMenuCallback` 0x4981b0 (init 0x497240, refresh 0x497a50 / 0x497930).
//! Decompiles in `tools/re/mapgen/` (`md_random*.c`); the generator is `s4_mapgen`.
//!
//! * The map settings' random tab (2507, both dialogs; proc 0x48e5e0 → event 0x1b69 →
//!   0x520c90) constructs it with the settings' network flag (+4): dialog 7 locally, 6 in a
//!   network game. The constructor's defaults: 2 players, 2 teams, mode 1, goods 2, the game
//!   name string 789 with the player's name, the parameter block 0x12987f8 (size 512, land 70,
//!   resources 10, no mirror, 2 players, a seed `(rand · 0x7fff + rand) % 1000000`).
//! * The init (code 0) resets the block again (a new seed, the players kept), the labels, the
//!   Trojan option rows (0x7e7 / 0x7e8 in dialog 6 with the Add-On or Mission CD 2, 0x7e9 /
//!   0x7ea with Mission CD 2) and refreshes.
//! * The refresh: teams at most the players, the counts 0x7c7 / 0x7cd, their arrows enabled by
//!   the mode (0x149c216 / 0x149c217), start 0x7d5 enabled while the key is valid
//!   (`IsCorrectState`), goods 2000, mode 0x7c3, size 0x7ac, mirror 0x7b0, land 0x7b7 (an
//!   unknown value becomes 70 but shows "50%", the release's fall-through), seed 0x7b4 (empty
//!   after an empty entry), resources 0x7bb, the key 0x7bf (`key::build`) while valid; the preview
//!   toggle 0x7d6 released (images 0x1f).
//! * The arrows (code 3 → events 0x1b6d size ±64 in 256..=1024, 0x1b6e mirror 0..=3, 0x1b6f land
//!   ±10 in 10..=90, 0x1b70 resources ±5 in 5..=15, 0x1b71 a new seed, 0x44 players 2..=8 (the
//!   teams follow down), 0x43 teams 2.. up to the players (8 in conflict, 2 in economic), 0x45
//!   mode 1 → 3 → 5 (free settle: 1 player, 1 team; local only) and back, 0x46 goods 1..=3) make
//!   the key valid again (the size .. players ones) and refresh.
//! * The edits (code 2): the seed 0x7b4 (`atoi`, the key valid), the key 0x7bf (`key::entered`:
//!   a refused key clears the block and disables start), the game name 0x7c1.
//! * The preview toggle 0x7d6: with a valid key the generator's init and preview
//!   (`CRandomMaps` vtable +0) drawn into 0x7e6 with the starts (event 0x1b74, 0x527010 /
//!   0x527090 as the lobby's DrawMap) and images 0x1e; else 0x7e6 hidden.
//! * Back 0x7d4 (event 0x1b6a, and Esc): the start screens' set again and the single player type
//!   screen (0x522110) locally, the game list (0x51b300) in a network game. The category tabs
//!   0x7e1 / 0x7e2 / 0x7e3 (0x1b6c): the map settings with that category (0x51fe70).
//! * Start 0x7d5 (0x1b6b): the GameState of the map `RD_<key>` (0x5069b0's random branch: every
//!   slot race 5 random and free, slot 0 the human, the others computers at the chosen
//!   difficulty locally and open in a network game, team = slot % teams), then the game
//!   settings; in a network game the host's lobby with the map `RD_<key>`, which every peer
//!   generates from the name (`s4_viewer::scene::resolve_map`).
//!
//! * Copy 0x7e4 / paste 0x7e5: the key field's text to the clipboard / 8 characters of it as the
//!   key (`crate::clipboard`).
//!
//! * The start's GameState extras (as the map settings' start 0x520f9b): mode 5 → +0x2dc = 1,
//!   +0x2e0 = 1, the game decided for team 1 (`PlayerSetup::decided`, set by
//!   `setup::player_setup`); mode 3 → +0x44 + 4·team = ⌈players / 2⌉ for every team, the
//!   per-team player counts, whose only other access found is the recount at 0x50413b [inf: no
//!   reader; not kept].
//!
//! Left out: the password (0x7cc / 0x7d3; s4-net's LAN handshake has none). Not ported: the
//! sounds.

use s4_mapgen::key::{self, Params};
use s4_viewer::gpu::Gpu;
use s4_viewer::gui::Gui;
use s4_viewer::guistate::GuiState;

use super::{Action, render};

pub const SET: &str = "GUISetMDStartscreens.DAT";
/// The network game's dialog and the local one (`hsrch = (flag == 0) + 6`).
pub const NETWORK: usize = 6;
pub const LOCAL: usize = 7;

pub const SIZE: u16 = 0x7ac;
pub const MIRROR: u16 = 0x7b0;
pub const SEED: u16 = 0x7b4;
pub const LAND: u16 = 0x7b7;
pub const RESOURCES: u16 = 0x7bb;
pub const KEY: u16 = 0x7bf;
pub const NAME: u16 = 0x7c1;
pub const MODE: u16 = 0x7c3;
pub const PLAYERS: u16 = 0x7c7;
pub const TEAMS: u16 = 0x7cd;
pub const GOODS: u16 = 0x7d0;
pub const BACK: u16 = 0x7d4;
pub const START: u16 = 0x7d5;
pub const PREVIEW_TOGGLE: u16 = 0x7d6;
pub const PREVIEW: u16 = 0x7e6;

/// The labels the init sets: (link, string).
const LABELS: [(u16, usize); 13] = [
    (0x7aa, 0xbaf),
    (0x7c0, 0x7e1),
    (0x7c6, 0x80e),
    (0x7ca, 0x819),
    (0x7cb, 0x805),
    (0x7c2, 0x802),
    (0x7be, 0xbb2),
    (0x7b3, 0xbb4),
    (0x7b6, 0xbae),
    (0x7af, 0xbb5),
    (0x7ba, 0xbb0),
    (0x7ab, 0xbb1),
    (0x7e8, 0x129),
];

/// What a command asks of the start screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    None,
    /// The dialog's values changed: `refresh`.
    Refresh,
    /// The preview toggle: draw the preview (true) or hide it.
    Preview(bool),
    /// Event 0x1b6a.
    Back,
    /// Event 0x1b6c: the map settings with this category.
    Category(u32),
    /// Event 0x1b6b.
    Start,
    /// 0x7e4: the key field's text to the clipboard (its object's vtable +4).
    Copy,
    /// 0x7e5: at most 8 characters of the clipboard as the key (vtable +8, then [`Random::paste`]).
    Paste,
}

/// The state's globals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Random {
    /// `hsrch`: [`LOCAL`] or [`NETWORK`].
    pub dialog: usize,
    /// The parameter block 0x12987f8.
    pub params: Params,
    /// Block +0x24 (0x129881c): the seed entry was empty.
    pub seed_blank: bool,
    /// 0x149c1e4 players, 0x149c1e8 teams, 0x149c1f0 goods, 0x149c1f4 mode.
    pub players: u32,
    pub teams: u32,
    pub goods: u32,
    pub mode: u32,
    /// 0x149c216 / 0x149c217: the players' and the teams' arrows enabled.
    pub players_on: bool,
    pub teams_on: bool,
    /// `IsCorrectState`: the key is valid (start enabled).
    pub valid: bool,
    /// `GeneratedMap`: the preview toggle is down.
    pub generated: bool,
    /// 0x149c21c: the game name.
    pub name: String,
    /// 0x149c250 bit 0 (from 0x12938b3, the 0x7e7 option, event 0x4b) and 0x149c251 (0x7e9,
    /// event 0x4c): into the GameState +0x228 / +0x599 at the start [inf: Trojans allowed].
    pub trojans: bool,
    pub option2: bool,
}

/// `(rand() · 0x7fff + rand()) % 1000000` (0x526040, 0x497240, event 0x1b71) with two 15-bit
/// draws [ours: from the clock, the CRT `rand` is not seeded per game here].
pub fn new_seed() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
    let mut x = t ^ (t >> 29) ^ 0x9e37_79b9_7f4a_7c15;
    let mut rand = || {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (x >> 49) as u32 & 0x7fff
    };
    let (a, b) = (rand(), rand());
    (a * 0x7fff + b) % 1_000_000
}

impl Random {
    /// The constructor 0x526040 (the GameState is always new there).
    pub fn new(network: bool, name: String) -> Random {
        let mut r = Random {
            dialog: if network { NETWORK } else { LOCAL },
            params: Params::default(),
            seed_blank: false,
            players: 2,
            teams: 2,
            goods: 2,
            mode: 1,
            players_on: true,
            teams_on: true,
            valid: false,
            generated: false,
            name,
            trojans: false,
            option2: false,
        };
        r.reset_params();
        r
    }

    /// The block's defaults (0x526040 / 0x497240).
    fn reset_params(&mut self) {
        self.params = Params { size: 0x200, land: 0x46, resources: 10, mirror: 0, players: self.players, seed: new_seed(), ..Default::default() };
        self.seed_blank = false;
    }

    /// The init 0x497240 (code 0): `addon` / `mcd2` are the Add-On / Mission CD 2 objects'
    /// "installed" (vtable +4).
    pub fn init(&mut self, st: &mut GuiState, strings: &dyn Fn(usize) -> String, addon: bool, mcd2: bool) {
        let d = self.dialog;
        self.option2 = false;
        st.images.insert((d, PREVIEW_TOGGLE), (0x1f, 0x1f));
        self.generated = false;
        self.reset_params();
        self.valid = true;
        for (link, id) in LABELS {
            st.texts.insert((d, link), strings(id));
        }
        if d == NETWORK {
            // [not ported] the password: 0x7cc "Privat" and 0x7d3 hidden.
            st.set_hidden(d, 0x7cc, true);
            st.set_hidden(d, 0x7d3, true);
        }
        st.set_hidden(d, 0x7e8, !(addon || mcd2));
        st.set_hidden(d, 0x7e7, !(addon || mcd2));
        st.set_greyed(d, 0x7e7, false);
        st.set_hidden(d, 0x7e9, !mcd2);
        if mcd2 {
            st.texts.insert((d, 0x7ea), strings(0xf01));
        }
        self.refresh(st, strings, addon, mcd2);
    }

    /// The refresh 0x497a50 (with 0x497930).
    pub fn refresh(&mut self, st: &mut GuiState, strings: &dyn Fn(usize) -> String, addon: bool, mcd2: bool) {
        let d = self.dialog;
        st.images.insert((d, PREVIEW_TOGGLE), (0x1f, 0x1f));
        self.generated = false;
        // 0x497930.
        self.teams = self.teams.min(self.players);
        st.texts.insert((d, PLAYERS), self.players.to_string());
        st.texts.insert((d, TEAMS), self.teams.to_string());
        for (link, on) in [(0x7c8, self.players_on), (0x7c9, self.players_on), (0x7ce, self.teams_on), (0x7cf, self.teams_on)] {
            st.set_greyed(d, link, !on);
        }
        st.set_greyed(d, 0x7e1, d != LOCAL);
        st.set_greyed(d, START, !self.valid);
        // Events 0x4b / 0x4c again with the options (none without the add-ons).
        if !(addon || mcd2) {
            self.trojans = false;
        }
        if !mcd2 {
            self.option2 = false;
        }
        if let Some(id) = goods_text(self.goods) {
            st.texts.insert((d, GOODS), strings(id));
        }
        if let Some(id) = mode_text(self.mode) {
            st.texts.insert((d, MODE), strings(id));
        }
        st.set_pressed_link(d, PREVIEW_TOGGLE, self.generated);
        st.texts.insert((d, NAME), self.name.clone());
        let (size, id) = size_text(self.params.size);
        self.params.size = size;
        st.texts.insert((d, SIZE), strings(id));
        st.texts.insert((d, MIRROR), strings(mirror_text(self.params.mirror)));
        let (land, id) = land_text(self.params.land);
        self.params.land = land;
        st.texts.insert((d, LAND), strings(id));
        self.params.players = self.players;
        let seed = if self.seed_blank { String::new() } else { (self.params.seed as i32).to_string() };
        st.texts.insert((d, SEED), seed);
        let (res, id) = resources_text(self.params.resources);
        self.params.resources = res;
        st.texts.insert((d, RESOURCES), strings(id));
        if self.valid {
            st.texts.insert((d, KEY), key::build(&self.params));
        }
    }

    /// A button (code 3, `effects` the control's new state) and the event it sends, handled by
    /// 0x5265a0.
    pub fn command(&mut self, link: u16, effects: u8) -> Step {
        let local = self.dialog == LOCAL;
        // The parameter arrows set `IsCorrectState` before their event.
        if matches!(link, 0x7ad | 0x7ae | 0x7b1 | 0x7b2 | 0x7b5 | 0x7b8 | 0x7b9 | 0x7bc | 0x7bd | 0x7c4 | 0x7c5 | 0x7c8 | 0x7c9) {
            self.valid = true;
        }
        let p = &mut self.params;
        match link {
            // 0x1b6d.
            0x7ad => p.size = if p.size > 0x100 { p.size - 0x40 } else { p.size },
            0x7ae => p.size = if p.size < 0x400 { p.size + 0x40 } else { p.size },
            // 0x1b6e.
            0x7b1 => p.mirror = if p.mirror & 3 != 0 { p.mirror - 1 } else { p.mirror },
            0x7b2 => p.mirror = if p.mirror & 3 < 3 { p.mirror + 1 } else { p.mirror },
            // 0x1b71.
            0x7b5 => {
                p.seed = new_seed();
                self.seed_blank = false;
            }
            // 0x1b6f.
            0x7b8 => p.land = if p.land < 0x5a { p.land + 10 } else { p.land },
            0x7b9 => p.land = if p.land > 10 { p.land - 10 } else { p.land },
            // 0x1b70.
            0x7bc => p.resources = if p.resources > 5 { p.resources - 5 } else { p.resources },
            0x7bd => p.resources = if p.resources < 0xf { p.resources + 5 } else { p.resources },
            // 0x45.
            0x7c4 | 0x7c5 => loop {
                if link == 0x7c4 {
                    self.mode -= 1;
                    if self.mode == 0 {
                        self.mode = 5;
                    }
                } else {
                    self.mode += 1;
                    if self.mode > 6 {
                        self.mode = 1;
                    }
                }
                match self.mode {
                    1 => {
                        (self.players_on, self.teams_on) = (true, true);
                        (self.players, self.teams) = (2, 2);
                        break;
                    }
                    3 => {
                        (self.players_on, self.teams_on) = (true, false);
                        (self.players, self.teams) = (2, 2);
                        break;
                    }
                    5 if local => {
                        (self.players_on, self.teams_on) = (false, false);
                        (self.players, self.teams) = (1, 1);
                        break;
                    }
                    _ => {}
                }
            },
            // 0x44.
            0x7c8 => {
                if self.players > 2 {
                    self.players -= 1;
                    if self.players < self.teams {
                        self.teams -= 1;
                    }
                }
            }
            0x7c9 => self.players = if self.players < 8 { self.players + 1 } else { self.players },
            // 0x43: nothing at all (not even the refresh) at the limits below.
            0x7ce => {
                if self.teams == 2 {
                    return Step::None;
                }
                self.teams -= 1;
            }
            0x7cf => {
                if self.teams == self.players {
                    return Step::None;
                }
                if (self.mode == 1 && self.teams < 8) || (self.mode == 3 && self.teams < 2) {
                    self.teams += 1;
                }
            }
            // 0x46.
            0x7d1 => {
                if self.goods == 1 {
                    return Step::None;
                }
                self.goods -= 1;
            }
            0x7d2 => {
                if self.goods == 3 {
                    return Step::None;
                }
                self.goods += 1;
            }
            BACK => return Step::Back,
            START => return Step::Start,
            PREVIEW_TOGGLE => {
                self.generated = self.valid;
                return Step::Preview(self.valid);
            }
            0x7e1..=0x7e3 => return Step::Category((link - 0x7e1) as u32),
            0x7e4 => return Step::Copy,
            0x7e5 => return Step::Paste,
            0x7e7 => {
                self.trojans = effects & 1 != 0;
                return Step::None;
            }
            0x7e9 => {
                self.option2 = effects & 1 != 0;
                return Step::None;
            }
            _ => return Step::None,
        }
        Step::Refresh
    }

    /// Enter in an edit field (code 2): the seed, the key, the game name.
    pub fn edit_done(&mut self, st: &mut GuiState, link: u16) -> Step {
        let text = st.texts.get(&(self.dialog, link)).cloned().unwrap_or_default();
        match link {
            SEED => {
                self.seed_blank = text.is_empty();
                self.params.seed = atoi(&text) as u32;
                self.valid = true;
                Step::Refresh
            }
            KEY => match key::entered(&text) {
                Some(p) => {
                    self.params = p;
                    self.valid = true;
                    self.players = p.players;
                    Step::Refresh
                }
                None => {
                    // 0x50b2c0 cleared the block before it refused (a short key leaves it).
                    if text.encode_utf16().count() >= 8 {
                        self.params = Params::default();
                    }
                    self.valid = false;
                    st.set_greyed(self.dialog, START, true);
                    Step::None
                }
            },
            NAME => {
                self.name = text;
                Step::None
            }
            _ => Step::None,
        }
    }

    /// 0x7e5 with the clipboard's text: fewer than 8 characters or a refused key (the block
    /// cleared) make the key invalid, without greying start; a key is taken as typed.
    pub fn paste(&mut self, text: Option<String>) -> Step {
        let Some(t) = text else { return Step::None };
        if t.encode_utf16().count() < 8 {
            self.valid = false;
            return Step::None;
        }
        match key::entered(&t) {
            Some(p) => {
                self.params = p;
                self.valid = true;
                self.players = p.players;
                Step::Refresh
            }
            None => {
                self.params = Params::default();
                self.valid = false;
                Step::None
            }
        }
    }

    /// The map's name as the release gives it to the game start (`RD_` and the key).
    pub fn map_name(&self) -> String {
        format!("RD_{}", key::build(&self.params))
    }

    /// The slots of 0x5069b0's random branch and 0x1b6b's teams: (kind, level) per slot, the
    /// human in slot 0, computers at `level` locally, open slots in a network game.
    pub fn slots(&self, level: u8, me: &str) -> Vec<s4_net::SlotInfo> {
        use s4_net::lobby::{RACE_RANDOM, SLOT_AI, SLOT_HUMAN, SLOT_OPEN};
        let teams = self.teams.max(1);
        (0..self.players.clamp(1, 8) as usize)
            .map(|k| {
                let (kind, name, ready, lv) = match k {
                    0 => (SLOT_HUMAN, me.to_string(), false, 0),
                    // 0x506cf1: a computer always gets its AINames entry.
                    _ if self.dialog == LOCAL => (SLOT_AI, super::setup::ai_name(k), true, level),
                    _ => (SLOT_OPEN, String::new(), false, 0),
                };
                s4_net::SlotInfo {
                    kind,
                    name,
                    race: RACE_RANDOM,
                    team: (k as u32 % teams) as u8,
                    colour: k as u8,
                    ready,
                    level: lv,
                    fixed: false,
                    logo: k as u8,
                    race_free: true,
                    human_steps: 0,
                }
            })
            .collect()
    }
}

impl super::Frontend {
    /// Event 0x1b69 of the map settings (0x520c90 → the state's factory 0x525ff0 with the
    /// settings' network flag): the Mission CD's set and dialog 7 / 6 with its init.
    pub(super) fn open_random(&mut self, gpu: &mut Gpu) {
        let name = crate::settings::Settings::load().player_name;
        let game = self.text(789).replace("%s", &name);
        let r = Random::new(self.multiplayer, game);
        self.flush_calls(gpu);
        match Gui::from_set(SET, super::SCREEN.1, &[]) {
            Ok(ui) => gpu.ui = Some(ui),
            Err(e) => {
                tracing::warn!("{SET}: {e}");
                return;
            }
        }
        let d = r.dialog;
        self.random = Some(r);
        self.screen = d;
        let texts = &self.texts;
        let strings = |id: usize| texts.as_ref().and_then(|t| t.get(id)).unwrap_or("").to_string();
        if let (Some(r), Some(ui)) = (self.random.as_mut(), gpu.ui.as_mut()) {
            let mut ev = Vec::new();
            for o in ui.state.open.clone() {
                ui.state.close(o, &mut ev);
            }
            ui.state.open(d, &mut ev);
            // [inf] The History Edition has the Add-On and Mission CD 2.
            r.init(&mut ui.state, &strings, true, true);
        }
        render(gpu);
    }

    /// Back to the start screens' set (the Mission CD object's vtable +0xc) without the state.
    fn leave_random(&mut self, gpu: &mut Gpu) {
        self.flush_calls(gpu);
        self.random = None;
        match Gui::from_set(super::SET, super::SCREEN.1, &[]) {
            Ok(ui) => gpu.ui = Some(ui),
            Err(e) => tracing::warn!("{}: {e}", super::SET),
        }
    }

    fn random_refresh(&mut self, gpu: &mut Gpu) {
        let texts = &self.texts;
        let strings = |id: usize| texts.as_ref().and_then(|t| t.get(id)).unwrap_or("").to_string();
        if let (Some(r), Some(ui)) = (self.random.as_mut(), gpu.ui.as_mut()) {
            r.refresh(&mut ui.state, &strings, true, true);
        }
    }

    /// The procedure's code 3 and the state's events.
    pub(super) fn random_command(&mut self, gpu: &mut Gpu, dialog: usize, link: u16, effects: u8) -> Option<Action> {
        let r = self.random.as_mut()?;
        if dialog != r.dialog {
            return None;
        }
        match r.command(link, effects) {
            Step::None => {}
            Step::Refresh => self.random_refresh(gpu),
            Step::Copy => {
                let d = r.dialog;
                let key = gpu.ui.as_ref().and_then(|u| u.state.texts.get(&(d, KEY)).cloned()).unwrap_or_default();
                crate::clipboard::set_text(&key);
            }
            Step::Paste => {
                if r.paste(crate::clipboard::get_text(8)) == Step::Refresh {
                    self.random_refresh(gpu);
                }
            }
            Step::Preview(on) => {
                let d = r.dialog;
                let img = on.then(|| preview_image(&r.params)).flatten();
                if let Some(ui) = gpu.ui.as_mut() {
                    match img {
                        Some(img) => {
                            ui.state.owner_images.insert((d, PREVIEW), img);
                            ui.state.set_hidden(d, PREVIEW, false);
                            ui.state.images.insert((d, PREVIEW_TOGGLE), (0x1e, 0x1e));
                        }
                        None => {
                            ui.state.set_hidden(d, PREVIEW, true);
                            ui.state.images.insert((d, PREVIEW_TOGGLE), (0x1f, 0x1e));
                        }
                    }
                }
            }
            Step::Back => {
                self.leave_random(gpu);
                let to = if self.multiplayer { super::net::GAME_LIST } else { super::LOCAL_TYPE };
                self.open(gpu, to);
                return None;
            }
            Step::Category(c) => {
                self.leave_random(gpu);
                self.category = c;
                let to = if self.multiplayer { super::MAP_SETTINGS } else { super::LOCAL_MAP };
                self.open(gpu, to);
                return None;
            }
            Step::Start if r.dialog == NETWORK => {
                // The host's lobby with the map's name `RD_<key>`: every peer generates it
                // (`resolve_map`), the hash of the generated file checks them.
                let name = r.map_name();
                let path = match s4_mapgen::map_path(&name) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("{name}: {e}");
                        return None;
                    }
                };
                let me = crate::settings::Settings::load().player_name;
                let settings = s4_net::GameSettings {
                    load_tick: 0,
                    load_hash: 0,
                    map: name,
                    delay_ms: crate::network_time_delta(),
                    hash_interval: 64,
                    map_hash: s4_net::map_hash(&path).ok()?,
                    teams: r.teams as u8,
                    mode: r.mode as u8,
                    goods: r.goods as u8,
                    race_goods: super::net::race_goods(r.mode),
                    ai_level: self.difficulty,
                };
                let slots = r.slots(self.difficulty, &me);
                let game = r.name.clone();
                self.leave_random(gpu);
                if let Err(e) = self.host(gpu, &me, &game, slots, settings) {
                    tracing::warn!("host: {e}");
                    self.open(gpu, super::MAP_SETTINGS);
                }
                return None;
            }
            Step::Start => {
                let name = r.map_name();
                let path = match s4_mapgen::map_path(&name) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("{name}: {e}");
                        return None;
                    }
                };
                let me = crate::settings::Settings::load().player_name;
                let slots = r.slots(self.difficulty, &me);
                let settings = s4_net::GameSettings {
                    map: path.to_string_lossy().into_owned(),
                    teams: r.teams as u8,
                    mode: r.mode as u8,
                    goods: r.goods as u8,
                    race_goods: super::net::race_goods(r.mode),
                    ..Default::default()
                };
                self.leave_random(gpu);
                self.net = super::net::NetUi::Local(Box::new(super::net::LocalLobby { slots, settings, me: 0 }));
                self.open(gpu, super::LOCAL_GAME);
                return None;
            }
        }
        render(gpu);
        None
    }

    /// Code 2 (Enter in an edit field).
    pub(super) fn random_edit(&mut self, gpu: &mut Gpu, link: u16) {
        let (Some(r), Some(ui)) = (self.random.as_mut(), gpu.ui.as_mut()) else { return };
        if r.edit_done(&mut ui.state, link) == Step::Refresh {
            self.random_refresh(gpu);
        }
        render(gpu);
    }
}

/// Event 0x1b74: the generator's init and preview (`CRandomMaps` vtable +0: host, 0x68c640,
/// 0x68caf0) drawn as the lobby draws a map (0x527090) with the starts (+0xc4 info).
fn preview_image(p: &Params) -> Option<(u32, u32, Vec<u8>)> {
    let mut g = s4_mapgen::generator::Gen::new(p);
    let px = g.preview();
    let rgba: Vec<u8> = px.iter().flat_map(|&c| s4_formats::map::preview_pixel(c)).collect();
    let starts: Vec<(u32, u32)> = g.starts[1..=(p.players as usize).clamp(1, 8)].iter().map(|s| (s.0 as u32, s.1 as u32)).collect();
    super::draw_map(160, &rgba, g.size as u32, &starts).inspect_err(|e| tracing::warn!("random preview: {e}")).ok()
}

/// The CRT `atoi` (`sub_999BBA`): leading blanks, a sign, digits; 0 without digits.
fn atoi(s: &str) -> i32 {
    let s = s.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut v: i32 = 0;
    for c in digits.bytes().take_while(u8::is_ascii_digit) {
        v = v.wrapping_mul(10).wrapping_add((c - b'0') as i32);
    }
    if neg { v.wrapping_neg() } else { v }
}

/// Goods (and the resources' 5 / 10 / 15): 0x808 / 0x801 / 0x7fe.
fn goods_text(goods: u32) -> Option<usize> {
    match goods {
        1 => Some(0x808),
        2 => Some(0x801),
        3 => Some(0x7fe),
        _ => None,
    }
}

fn mode_text(mode: u32) -> Option<usize> {
    match mode {
        1 => Some(0x7f8),
        2 => Some(0x7f9),
        3 => Some(0x7fa),
        4 => Some(0x905),
        5 => Some(0x7fb),
        _ => None,
    }
}

/// The size's text; another size becomes 512.
fn size_text(size: u32) -> (u32, usize) {
    match size {
        0x100 => (size, 0xccf),
        0x140 => (size, 0xcd0),
        0x180 => (size, 0xcd1),
        0x1c0 => (size, 0xcd2),
        0x240 => (size, 0xcd4),
        0x280 => (size, 0xcd5),
        0x2c0 => (size, 0xcd6),
        0x300 => (size, 0xcd7),
        0x340 => (size, 0xcd8),
        0x380 => (size, 0xcd9),
        0x3c0 => (size, 0xcda),
        0x400 => (size, 0xcce),
        _ => (0x200, 0xcd3),
    }
}

fn mirror_text(mirror: u32) -> usize {
    [0xccc, 0xccd, 0xccb, 0xcca][(mirror & 3) as usize]
}

/// The land's text; another value becomes 70 and shows 50 % (the default falls into case 50).
fn land_text(land: u32) -> (u32, usize) {
    match land {
        10 => (land, 0xcc1),
        20 => (land, 0xcc2),
        30 => (land, 0xcc3),
        40 => (land, 0xcc4),
        50 => (land, 0xcc5),
        60 => (land, 0xcc6),
        70 => (land, 0xcc7),
        80 => (land, 0xcc8),
        90 => (land, 0xcc9),
        _ => (0x46, 0xcc5),
    }
}

/// The resources' text; another value becomes 10.
fn resources_text(res: u32) -> (u32, usize) {
    match res {
        5 => (res, 0x808),
        15 => (res, 0x7fe),
        10 => (res, 0x801),
        _ => (10, 0x801),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_and_limits() {
        let mut r = Random::new(false, "me".into());
        assert_eq!((r.params.size, r.params.land, r.params.resources, r.players, r.teams), (0x200, 70, 10, 2, 2));
        assert!(r.params.seed < 1_000_000);
        for _ in 0..20 {
            r.command(0x7ae, 0);
        }
        assert_eq!(r.params.size, 0x400);
        for _ in 0..20 {
            r.command(0x7b8, 0);
        }
        assert_eq!(r.params.land, 90);
        r.command(0x7bd, 0);
        r.command(0x7bd, 0);
        assert_eq!(r.params.resources, 15);
        // Teams: + up to the players; the players' − takes them along.
        assert_eq!(r.command(0x7cf, 0), Step::None);
        r.command(0x7c9, 0);
        r.command(0x7c9, 0);
        r.command(0x7cf, 0);
        assert_eq!((r.players, r.teams), (4, 3));
        r.command(0x7c8, 0);
        r.command(0x7c8, 0);
        assert_eq!((r.players, r.teams), (2, 2));
        // Modes: 1 → 3 → 5 → 1 locally, 5 skipped in a network game.
        r.command(0x7c5, 0);
        assert_eq!((r.mode, r.players_on, r.teams_on), (3, true, false));
        r.command(0x7c5, 0);
        assert_eq!((r.mode, r.players, r.teams), (5, 1, 1));
        r.command(0x7c5, 0);
        assert_eq!(r.mode, 1);
        let mut n = Random::new(true, "me".into());
        n.command(0x7c4, 0);
        assert_eq!(n.mode, 3);
        // The key of the parameters, and slots.
        r.params.seed = 12345;
        let k = key::build(&r.params);
        assert_eq!(key::entered(&k).map(|p| p.seed), Some(12345));
        assert_eq!(r.map_name(), format!("RD_{k}"));
        let s = r.slots(1, "me");
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].kind, s[1].kind, s[1].level, s[1].team), (s4_net::lobby::SLOT_HUMAN, s4_net::lobby::SLOT_AI, 1, 1));
        assert_eq!(atoi("  -42x"), -42);
        // Paste: short or refused keys invalidate (a refused one clears the block).
        assert_eq!(r.paste(None), Step::None);
        assert_eq!(r.paste(Some("H8GQ".into())), Step::None);
        assert!(!r.valid);
        assert_eq!(r.paste(Some("H8GQE2M0".into())), Step::Refresh);
        assert_eq!((r.valid, r.params), (true, key::parse("RD_H8GQE2M0").unwrap()));
        assert_eq!(r.paste(Some("ZZZZZZZZ".into())), Step::None);
        assert_eq!((r.valid, r.params), (false, Params::default()));
        assert_eq!(land_text(100), (70, 0xcc5));
    }
}
