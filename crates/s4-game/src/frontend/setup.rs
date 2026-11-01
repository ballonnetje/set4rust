//! The free game's setup: what the map settings screen reads from a map and lets the player
//! choose (dialog 18 local, 3 network; `CStateLobbyMapSettings`, events 0x520c90), and the slots
//! the game settings screen starts with (dialog 19 local, 4 network). Decompiles in
//! `tools/re/viewer-2026-10-22/skirmish/`.
//!
//! **The map** (`sub_1069B0` 0x5069b0, the GameState of a map file, with the team setup the
//! screen shows): per player slot `MA_GetPlayerData(player, setup)` gives the control (0 free,
//! 1 human only, 2 computer only), the team (below 0 or above 8 → 0) and, from the player
//! chunk, the race (255 → random: race 5 and +0x3c3). The team count (+0x40) is the largest
//! team + 1. From the map's options (general info +20):
//! * +0x34f = bit 0x2, +0x350 = bit 0x4 clear and the map's mode 1, +0x348 = bit 0x10 (with
//!   more than one team), bit 0x80 forces conflict as the default;
//! * listed locally (+0x341) with one player, or +0x34f and +0x350, or no human-only slot but
//!   the first; listed in a network game (+0x340) with more than one slot a human can take;
//! * the modes the map allows (the list entry +0x40..+0x44, 0x5201e0): conflict (1) unless there
//!   is one team (+0x34b), economic (3) with +0x348, free settle (5, local only, +0x34a) with one
//!   team or +0x350 and +0x34f, cooperation (2, +0x34c) never set by what was traced, head-to-
//!   head (4) skipped by the mode buttons; the default mode (+0x344) is 1, or 5 when conflict is
//!   not allowed and free settle is (bit 0x80: 1).
//!
//! **The buttons** (events of 0x520c90): players 2441 / 2442 (0x44: + up to the map's count, −
//! down to 2 unless already 1; the teams follow down), teams 2445 / 2446 (0x43: − down to 2, to 1
//! in free settle; + up to the players, in economic up to 2), mode 2487 / 2488 (0x45: the next
//! mode the map allows, wrapping 1..5; conflict / cooperation set players = teams = the map's
//! players, economic 2 teams, free settle 1 player 1 team), goods 2489 / 2490 (0x46: 1..3),
//! team setup 2491 / 2492 (0x42: wrapping; the map is read again with it, 0x521540).
//!
//! **The start** (event 0x40): the GameState again with the players' choices; the slot count is
//! the player count, every team `% teams`. The slots: free ones are computers in a local game
//! (`sub_1069B0`'s `param_6`, the difficulty: easy 2 / normal 3) and open in a network game;
//! computer-only ones are computers; human-only ones open. A human (`sub_1A7EE0` 0x5a7ee0, with
//! GameState +0x2ac clear) takes the first open slot.

use std::path::Path;

use s4_net::SlotInfo;
use s4_net::lobby::{RACE_RANDOM, SLOT_AI, SLOT_HUMAN, SLOT_OPEN};

/// One map player slot as `sub_1069B0` reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapSlot {
    /// `MA_GetPlayerData` control: 0 free, 1 human only, 2 computer only.
    pub control: u8,
    pub team: u8,
    /// 0..=4, or [`RACE_RANDOM`].
    pub race: u8,
    /// The players chunk's name (char[33], GameState + (k + 0x28) · 0x18).
    pub name: String,
}

/// A computer's name (0x501880 fills 0x149b348 + 0x18 k from `Config\AINames.cfg` `[AINAMES]
/// Player<k + 1>`, default "AI Player"; 0x507f30 reads slot `k`'s).
pub fn ai_name(k: usize) -> String {
    let p = s4_formats::path::resolve(s4_formats::game_dir().join("Config").join("AINames.cfg"));
    let cfg = s4_audio::cfg::Cfg::parse(&std::fs::read_to_string(p).unwrap_or_default());
    cfg.get("AINAMES", &format!("Player{}", k + 1)).unwrap_or("AI Player").to_string()
}

/// What the map settings screen knows of a map (the GameState of `sub_1069B0` and the list
/// entry of 0x5201e0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRead {
    pub slots: Vec<MapSlot>,
    /// +0x40: the largest team + 1.
    pub teams: u32,
    /// The list entry: listed locally (+0x341) / in a network game (+0x340).
    pub local: bool,
    pub network: bool,
    /// Modes the map allows: [conflict, cooperation, economic, head-to-head, free settle].
    pub modes: [bool; 5],
    /// +0x344 and +0x38 (the map's start resources).
    pub mode: u32,
    pub goods: u32,
    /// +0x354 and the current setup's name (+0x2e4).
    pub setups: usize,
    pub setup_name: String,
    /// The buttons the map allows (0x521540 from the list entry, 0x5201e0): players +0x47 clear
    /// (options bit 0x2, and bit 0x4 clear in mode 1), teams +0x48 clear (bit 0x2), goods +0x49
    /// clear (bit 0x1 in mode 1). 0x48d600 enables 2441/2442, 2445/2446 and 2489/2490 by them
    /// (`IGuiEngine::EnableControl` 0x676cd0). None of the six multiplayer maps with preplaced
    /// settlers and buildings sets bit 0x2: their player count is the map's.
    pub can_players: bool,
    pub can_teams: bool,
    pub can_goods: bool,
}

impl MapRead {
    /// `sub_1069B0` for map `path` with team setup `setup` (the index into the map's list of
    /// setups), as a local or network game.
    pub fn open(path: &Path, setup: usize, local: bool) -> anyhow::Result<MapRead> {
        let m = s4_formats::map::MapFile::open(path)?;
        let g = m.general()?;
        let players = m.players().unwrap_or_default();
        let setups = m.team_setups().unwrap_or_default();
        let chosen = setups.get(setup);
        let count = g.player_count as usize;
        let mut slots = Vec::with_capacity(count);
        for k in 0..count {
            let s = chosen.and_then(|s| s.slots.get(k));
            let team = s.map_or(0, |s| if s.team > 8 { 0 } else { s.team });
            let race = players.get(k).map_or(RACE_RANDOM, |p| if p.race <= 4 { p.race as u8 } else { RACE_RANDOM });
            let name = players.get(k).map(|p| p.name.clone()).unwrap_or_default();
            slots.push(MapSlot { control: s.map_or(0, |s| s.control), team, race, name });
        }
        let options = g.options;
        let map_mode = g.mode();
        let teams = slots.iter().map(|s| s.team as u32).max().unwrap_or(0) + 1;
        let b34f = options & 2 != 0;
        let b350 = options & 4 == 0 && map_mode == 1;
        // A human-only slot other than the first (sub_1069B0's bVar23).
        let more_humans = slots.iter().skip(1).any(|s| s.control != 0 && s.control != 2);
        let human_capable = slots.iter().filter(|s| s.control != 2).count();
        let conflict = teams != 1;
        let economic = teams != 1 && options & 0x10 != 0;
        let free_settle = local && (teams == 1 || (b350 && b34f));
        let mut mode = 1;
        if !conflict && free_settle && options & 0x80 == 0 {
            mode = 5;
        }
        Ok(MapRead {
            teams,
            local: count == 1 || (b34f && b350) || !more_humans,
            network: human_capable > 1,
            modes: [conflict, false, economic, false, free_settle],
            mode,
            goods: g.start_resources.clamp(1, 3),
            setups: setups.len(),
            can_players: b350 && b34f,
            can_teams: b34f,
            can_goods: options & 1 != 0 && map_mode == 1,
            setup_name: chosen.map(|s| s.name.clone()).unwrap_or_default(),
            slots,
        })
    }
}

/// The map settings' choices (0x149c1e4 players, 0x149c1e8 teams, 0x149c1f4 mode, 0x149c1f0
/// goods, 0x149c1f8 team setup).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub players: u32,
    pub teams: u32,
    pub mode: u32,
    pub goods: u32,
    pub setup: usize,
}

impl Choice {
    /// After reading a map (0x521540): its players, teams, mode and goods.
    pub fn of(m: &MapRead, setup: usize) -> Choice {
        Choice { players: m.slots.len() as u32, teams: m.teams, mode: m.mode, goods: m.goods, setup }
    }

    /// The buttons enabled (0x521540, the mode cases of 0x520c90; 0x48d600 applies them):
    /// players (0x149c216), teams (0x149c217), goods (0x149c215), team setup (0x149c218: the
    /// map's setup count +0x354 is not 1). Free settle (0x52115b) clears the first two.
    pub fn enabled(&self, m: &MapRead) -> [bool; 4] {
        let settle = self.mode == 5;
        [m.can_players && !settle, m.can_teams && !settle, m.can_goods, m.setups != 1]
    }

    /// Event 0x44.
    pub fn players(&mut self, up: bool, max: u32) {
        if up {
            if self.players != max {
                self.players += 1;
            }
        } else if self.players != 1 && self.players > 2 {
            self.players -= 1;
            if self.teams > self.players {
                self.teams -= 1;
            }
        }
    }

    /// Event 0x43.
    pub fn teams(&mut self, up: bool) {
        if up {
            if self.teams != self.players && (self.mode != 3 || self.teams < 2) {
                self.teams += 1;
            }
        } else if self.teams != 1 && (self.mode == 5 || self.teams > 2) {
            self.teams -= 1;
        }
    }

    /// Event 0x45: step until a mode the map allows; `max` = the map's player count.
    pub fn mode(&mut self, up: bool, m: &MapRead) {
        if !m.modes.iter().any(|&a| a) {
            return;
        }
        let max = m.slots.len() as u32;
        loop {
            self.mode = if up {
                if self.mode + 1 > 6 { 1 } else { self.mode + 1 }
            } else if self.mode <= 1 {
                5
            } else {
                self.mode - 1
            };
            let ok = (1..=5).contains(&self.mode) && self.mode != 4 && m.modes[self.mode as usize - 1];
            if !ok {
                continue;
            }
            match self.mode {
                1 | 2 => {
                    self.players = max;
                    self.teams = max;
                }
                3 => {
                    self.teams = 2;
                    self.players = max;
                }
                _ => {
                    self.players = 1;
                    self.teams = 1;
                }
            }
            return;
        }
    }

    /// Event 0x46.
    pub fn goods(&mut self, up: bool) {
        if up {
            if self.goods != 3 {
                self.goods += 1;
            }
        } else if self.goods != 1 {
            self.goods -= 1;
        }
    }

    /// Event 0x42: the next team setup (wrapping); the caller reads the map again with it.
    pub fn setup(&mut self, up: bool, count: usize) {
        let n = count.max(1);
        self.setup = if up { (self.setup + 1) % n } else { (self.setup + n - 1) % n };
    }
}

/// The mode's text (0x48dda0 into 2484): conflict 2040, cooperation 2041, economic 2042,
/// head-to-head 2309, free settle 2043.
pub fn mode_text(mode: u32) -> Option<usize> {
    match mode {
        1 => Some(2040),
        2 => Some(2041),
        3 => Some(2042),
        4 => Some(2309),
        5 => Some(2043),
        _ => None,
    }
}

/// The goods' text (into 2485): small 2056, medium 2049, large 2046.
pub fn goods_text(goods: u32) -> Option<usize> {
    match goods {
        1 => Some(2056),
        2 => Some(2049),
        3 => Some(2046),
        _ => None,
    }
}

/// The game settings' slots at the start (event 0x40): the first `choice.players` map slots,
/// teams `% choice.teams`, colours by slot, computers at `level` (0 easy, 1 normal); the human
/// `name` in the first open slot. Returns the slots and the human's slot.
pub fn start_slots(m: &MapRead, choice: &Choice, local: bool, level: u8, name: &str) -> (Vec<SlotInfo>, Option<usize>) {
    let teams = choice.teams.max(1);
    let mut slots: Vec<SlotInfo> = m
        .slots
        .iter()
        .take(choice.players as usize)
        .enumerate()
        .map(|(k, s)| {
            let ai = s.control == 2 || (s.control == 0 && local);
            SlotInfo {
                kind: if ai { SLOT_AI } else { SLOT_OPEN },
                // 0x5069b0 (0x507659): a computer's name is the map's, or its AINames entry.
                name: if !ai {
                    String::new()
                } else if s.name.is_empty() {
                    ai_name(k)
                } else {
                    s.name.clone()
                },
                race: s.race,
                team: (s.team as u32 % teams) as u8,
                colour: k as u8,
                ready: ai,
                level: if ai { level } else { 0 },
                fixed: s.control != 0,
                logo: k as u8,
                race_free: s.race == RACE_RANDOM,
                human_steps: 0,
            }
        })
        .collect();
    let me = slots.iter().position(|s| s.kind == SLOT_OPEN);
    if let Some(i) = me {
        slots[i].kind = SLOT_HUMAN;
        slots[i].name = name.to_string();
    }
    (slots, me)
}

/// The players the game takes from the slots (GameState +0x6c / +0x90 / +0x18c → the game's
/// player data): computers control 2 / 3, every slot its team.
///
/// Mode 5 (free settle) starts decided for team 1 (0x520fa5 / 0x526b45: GameState +0x2dc, +0x2e0).
pub fn player_setup(slots: &[SlotInfo], mode: u32, local: u8) -> crate::sim::PlayerSetup {
    let mut p = crate::sim::PlayerSetup { game_type: mode, local, players: Some(slots.len().min(8) as u8), decided: mode == 5, ..Default::default() };
    for (i, s) in slots.iter().enumerate().take(8) {
        if matches!(s.kind, SLOT_HUMAN | SLOT_AI) {
            p.races[i] = Some(s.race);
            p.teams[i] = Some(s.team);
            // `GlobalGameState +0x6c`: 1 a human, 2 / 3 a computer (0, an open slot, stays).
            p.control[i] = if s.kind == SLOT_AI { 2 + s.level.min(1) } else { 1 };
        }
    }
    p
}

/// A campaign mission's players (the briefing's start, e.g. `CStateMD2Briefing` 0x523710:
/// `sub_1069B0(1, …, difficulty, type)`): the map's own setup as a local game, its free and
/// computer slots computers at `level`, the races the map's.
pub fn campaign_setup(path: &Path, level: u8) -> Option<crate::sim::PlayerSetup> {
    let m = MapRead::open(path, 0, true).inspect_err(|e| tracing::warn!("{}: {e}", path.display())).ok()?;
    let c = Choice::of(&m, 0);
    let (slots, me) = start_slots(&m, &c, true, level, "");
    let mut p = player_setup(&slots, m.mode, me.map_or(1, |i| i as u8 + 1));
    p.races = [None; 8];
    // A mission plays every slot of its map; the briefing's start leaves GameState +0x2dc /
    // +0x2e0 as the map read set them (0).
    p.players = None;
    p.decided = false;
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The computers' names from `Config\AINames.cfg` (the install's: "BB Techs", ...).
    #[test]
    fn computer_names() {
        if s4_formats::game_dir().join("Config").join("AINames.cfg").exists() {
            assert_eq!((ai_name(0).as_str(), ai_name(1).as_str()), ("BB Techs", "BB Wolf-Ram"));
        }
        assert_eq!(ai_name(20), "AI Player");
    }

    fn read(slots: &[(u8, u8)], teams: u32, local: bool) -> MapRead {
        MapRead {
            slots: slots.iter().map(|&(control, team)| MapSlot { control, team, race: 0, name: String::new() }).collect(),
            teams,
            local,
            network: true,
            modes: [teams != 1, false, true, false, local],
            mode: 1,
            goods: 2,
            setups: 3,
            setup_name: String::new(),
            can_players: true,
            can_teams: true,
            can_goods: true,
        }
    }

    /// The real maps: the six multiplayer maps with preplaced settlers and buildings have their
    /// player count locked (options bit 0x2 clear), e.g. Alien's is free; free settle locks it
    /// too.
    #[test]
    fn player_count_buttons_follow_the_map_options() {
        let dir = s4_formats::game_dir().join("Map").join("Multiplayer");
        if !dir.exists() {
            return;
        }
        for (name, players) in [("InTrouble", false), ("Issos Brawl", false), ("Marathon", false), ("Massada", false), ("Roemerberg", false), ("UnitedNations", false), ("Alien", true), ("Arena", true)] {
            let m = MapRead::open(&dir.join(format!("{name}.map")), 0, true).unwrap();
            let c = Choice::of(&m, 0);
            assert_eq!(c.enabled(&m)[0], players, "{name}");
        }
        let m = MapRead::open(&dir.join("Alien.map"), 0, true).unwrap();
        let c = Choice { mode: 5, ..Choice::of(&m, 0) };
        assert_eq!(&c.enabled(&m)[..2], &[false, false], "free settle");
    }

    #[test]
    fn map_settings_buttons() {
        let m = read(&[(1, 0), (0, 0), (0, 1), (0, 1)], 2, true);
        let mut c = Choice::of(&m, 0);
        assert_eq!((c.players, c.teams, c.mode, c.goods), (4, 2, 1, 2));
        c.players(true, 4);
        assert_eq!(c.players, 4, "not above the map's");
        c.players(false, 4);
        c.players(false, 4);
        c.players(false, 4);
        assert_eq!((c.players, c.teams), (2, 2), "down to 2");
        c.teams(false);
        assert_eq!(c.teams, 2, "teams down to 2 outside free settle");
        c.players(true, 4);
        c.teams(true);
        assert_eq!(c.teams, 3);
        c.teams(true);
        assert_eq!(c.teams, 3, "not above the players");
        c.mode(true, &m);
        assert_eq!((c.mode, c.players, c.teams), (3, 4, 2), "economic: 2 teams");
        c.teams(true);
        assert_eq!(c.teams, 2, "economic caps the teams at 2");
        c.mode(true, &m);
        assert_eq!((c.mode, c.players, c.teams), (5, 1, 1), "free settle: alone");
        c.mode(true, &m);
        assert_eq!((c.mode, c.players, c.teams), (1, 4, 4), "conflict: everyone, own teams");
        c.mode(false, &m);
        assert_eq!(c.mode, 5, "down from conflict wraps to free settle");
        c.goods(true);
        c.goods(true);
        assert_eq!(c.goods, 3);
        c.setup(false, 3);
        assert_eq!(c.setup, 2);
    }

    #[test]
    fn start_slots_local_and_network() {
        let m = read(&[(1, 0), (0, 0), (2, 1), (0, 1)], 2, true);
        let c = Choice { players: 3, teams: 2, mode: 1, goods: 2, setup: 0 };
        let (s, me) = start_slots(&m, &c, true, 1, "me");
        assert_eq!(me, Some(0));
        assert_eq!(s.iter().map(|s| (s.kind, s.level, s.fixed)).collect::<Vec<_>>(), [(SLOT_HUMAN, 0, true), (SLOT_AI, 1, false), (SLOT_AI, 1, true)]);
        let p = player_setup(&s, 1, 1);
        assert_eq!(p.control, [1, 3, 3, 0, 0, 0, 0, 0]);
        assert_eq!(p.teams[..3], [Some(0), Some(0), Some(1)]);
        assert_eq!(p.players, Some(3), "three of the map's four slots play (NewGame::players)");
        let (n, me) = start_slots(&m, &c, false, 0, "host");
        assert_eq!(me, Some(0));
        assert_eq!(n.iter().map(|s| s.kind).collect::<Vec<_>>(), [SLOT_HUMAN, SLOT_OPEN, SLOT_AI], "network: the free slot open");
    }

    /// The shipped maps: every single-player map is listed locally, and the team setups give
    /// teams (e.g. AncientGlory's four, "1 vs 1 vs 1 vs 1": 4 teams).
    #[test]
    fn shipped_maps_read() {
        let dir = s4_formats::game_dir().join("Map");
        let p = dir.join("Multiplayer").join("AncientGlory.map");
        if !p.exists() {
            return;
        }
        let m = MapRead::open(&p, 3, true).unwrap();
        assert_eq!((m.slots.len(), m.teams, m.setups, m.setup_name.as_str()), (4, 4, 4, "1 vs 1 vs 1 vs 1"));
        assert!(m.local && m.network);
        let iso = MapRead::open(&dir.join("Multiplayer").join("4 Isles.map"), 0, false).unwrap();
        assert!(!iso.network, "one human slot: not a network map");
    }
}
