//! The map script (s4-script) in the tick order.
//!
//! The original calls `CGameScriptManager::Update(tick)` inside `CGameRun::Run`, after the tiling
//! update and before the animal update (docs/SIM.md §2.2). [`ScriptRunner`] is the sim's
//! [`ScriptDriver`], so `s4_sim::tick_with_script` calls it at exactly that point. The host answers
//! the local player, race, game time and random, the world queries `s4_sim::query` provides
//! (buildings, areas, settlers, land explored) and the "player lost" rule; every other call is
//! logged as unhandled.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use s4_script::{Arg, ScriptEngine, ScriptHost, ScriptSource, resolve_map_script};

use crate::tutorial::Request;
use s4_sim::{ScriptDriver, World};

/// How the game ended for the local player, shared with the client: 0 running, 1 won, 2 lost.
pub type Outcome = Arc<AtomicU8>;
pub const WON: u8 = 1;
pub const LOST: u8 = 2;

/// Answers for the script.
pub struct GameHost {
    pub local_player: i32,
    pub race: i32,
    pub tick: u32,
    /// [inf] Deterministic stand-in for the game RNG `Game.Random` uses (it should be the sim's
    /// `World::rng`; the host does not reach the world yet).
    rng: u32,
    pub unhandled: u64,
    /// Players on the map (`Game.NumberOfPlayers`).
    pub players: i32,
    /// `Game.GetDifficulty` (`CStateLocalType`: `s_uAIDifficulty`, 0 / 1).
    pub difficulty: i32,
    /// Players declared lost (`Game.PlayerLost`, `Game.EnemyPlayersLost`), bit per player id.
    lost: u32,
    outcome: Outcome,
    /// The default checks were asked for (logged once: not evaluated).
    default_check_logged: bool,
    /// Calls the host could not answer during the current tick (their results were defaults).
    unanswered: u32,
    /// A decision was refused because of unanswered calls (logged once).
    refused_logged: bool,
    /// The world, while the script runs (`ScriptRunner::update`); null otherwise.
    world: *mut World,
    /// What the script shares with the client: the tutorial's window, marker, controls and the
    /// camera requests (tutorial.rs).
    tutorial: crate::tutorial::Shared,
}

impl GameHost {
    /// The world the script runs in. `None` outside `ScriptRunner::update` (and in tests).
    fn world(&self) -> Option<&World> {
        // SAFETY: `world` is set from the `&mut World` that `ScriptDriver::update` holds for the
        // whole `ScriptEngine::update` call and is reset to null afterwards; nothing else
        // touches the world during that call, and the host is not used from another thread.
        unsafe { self.world.as_ref() }
    }

    fn world_mut(&mut self) -> Option<&mut World> {
        // SAFETY: as in `world`; `&mut self` keeps the two from overlapping.
        unsafe { self.world.as_mut() }
    }

    /// `GameDefaultPlayerLostCheck` for one player (`World::default_player_lost`): a player is
    /// marked lost once (the original posts message 0x38).
    fn default_lost(&mut self, p: i32) {
        if !(1..=31).contains(&p) || self.lost & (1 << p) != 0 {
            return;
        }
        if self.world().is_some_and(|w| w.is_used_player(p) && w.default_player_lost(p)) {
            tracing::info!("player {p} lost (default check)");
            self.set_lost(p);
        }
    }

    /// Marks `p` lost; the first time, event 0x38 for the client's messages (every lost path of
    /// the original tests and sets the flag 0x149b7f4 + 0x8c · p before sending it). None of
    /// them decides the game (only 0x5c8260 sends 0x37); the client ends it for the local
    /// player at its own 0x38 (0x5172f0 case 0x38 → 0x52a1a0).
    fn set_lost(&mut self, p: i32) {
        if self.lost & (1 << p) == 0 {
            self.lost |= 1 << p;
            self.tut().requests.push(Request::PlayerLost(p as u8));
            if p == self.local_player && self.outcome.load(Ordering::Relaxed) == 0 {
                tracing::info!("game decided for player {p}: lost");
                self.outcome.store(LOST, Ordering::Relaxed);
            }
        }
    }

    /// `GameDefaultPlayersLostCheck` (0x5c8100): every player; marks only.
    fn default_lost_all(&mut self) {
        let n = self.world().map_or(0, |w| w.number_of_players());
        for p in 1..=n {
            self.default_lost(p);
        }
    }

    /// The team of `p` (0x12dcd0c + 4 · p); without a world (tests) every player is its own.
    fn team_of(&self, p: i32) -> u8 {
        self.world().map_or(p as u8, |w| w.alliances.alliance_id.get(p as usize).copied().unwrap_or(0))
    }
}

impl GameHost {
    fn add_building(&mut self, x: i32, y: i32, player: i32, building: i32, radius: i32) -> i32 {
        let Some(w) = self.world_mut() else { return self.unhandled_int("Buildings.AddBuilding") };
        let size = w.grid.size() as i32;
        if !(0..size).contains(&x) || !(0..size).contains(&y) || !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 || !(1..=0x52).contains(&building) {
            return 0;
        }
        s4_sim::build::scripted::add_building_scripted(w, x, y, player as u8, building as u8, radius) as i32
    }

    /// The selection walk of `Settlers.IsSelected` / `Vehicles.IsSelected`: entities of `types`
    /// count `count` down when of `kind`; 1 when the count is 0 after an entity of `types`.
    fn selection_count(&mut self, types: &[s4_sim::entity::ObjType], kind: i32, count: i32) -> i32 {
        let sel = self.tut().selection.clone();
        let Some(w) = self.world() else { return 0 };
        let mut left = count;
        for id in sel {
            let Some(e) = w.entities.by_index(id as u32) else { continue };
            if !types.contains(&e.obj_type) {
                continue;
            }
            if e.kind as i32 == kind {
                left -= 1;
            }
            if left == 0 {
                return 1;
            }
        }
        0
    }

    fn tut(&self) -> std::sync::MutexGuard<'_, crate::tutorial::TutorialUi> {
        self.tutorial.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A query the host cannot answer (no world): counted as unanswered.
    fn unhandled_int(&mut self, func: &'static str) -> i32 {
        self.unhandled(func, &[]);
        0
    }

    /// The game end of `Game.DefaultGameEndCheck` (0x5c8260, non-tutorial part, decompile
    /// `tools/re/audio/gameend.c`): over the teams 1..=0x12dcd04 (the highest team with members;
    /// members 0x12dcde0) a team counts as lost when it has no members or all its players are;
    /// when one team stands and the lost ones are the teams with members (0x12dccfc) − 1, event
    /// 0x37 names it the winner (a lone team wins at once; a gap in the team ids keeps anyone
    /// from winning). Else, all players lost: 0x37 with team 0. The local player wins with the
    /// winner's team (0x5172f0 case 0x37), else loses. Without a world (tests): every player is
    /// its own team.
    fn decide(&mut self) {
        if self.outcome.load(Ordering::Relaxed) != 0 {
            return;
        }
        // A decision that rests on calls the host could not answer (their 0 results, e.g.
        // `Buildings.ExistsBuildingInArea` in roman02's check) is not trusted: the check is a
        // stub until the sim answers them.
        if self.unanswered > 0 {
            if !self.refused_logged {
                tracing::warn!("victory check at tick {} decided on {} unanswered script calls: ignored (stub)", self.tick, self.unanswered);
                self.refused_logged = true;
            }
            return;
        }
        let players: Vec<(i32, u8)> = (1..=self.players).map(|p| (p, self.team_of(p))).filter(|&(_, t)| t != 0).collect();
        let lost = |p: i32| self.lost & (1 << p) != 0;
        let highest = players.iter().map(|&(_, t)| t).max().unwrap_or(0);
        let teams = (1..=highest).filter(|&t| players.iter().any(|&(_, pt)| pt == t)).count();
        let (mut lost_teams, mut standing, mut lost_players) = (0, 0u8, 0);
        for t in 1..=highest {
            let members: Vec<i32> = players.iter().filter(|&&(_, pt)| pt == t).map(|&(p, _)| p).collect();
            let down = members.iter().filter(|&&p| lost(p)).count();
            lost_players += down;
            if down == members.len() {
                lost_teams += 1;
            } else {
                standing = t;
            }
        }
        let winner = if standing != 0 && lost_teams + 1 == teams {
            standing
        } else if lost_players == players.len() {
            0
        } else {
            return;
        };
        self.announce(winner);
    }

    /// Event 0x37 for `winner` (0 = nobody): the local player wins with the winner's team
    /// (0x5172f0 case 0x37), else loses.
    fn announce(&mut self, winner: u8) {
        let me = self.local_player;
        let result = if winner != 0 && winner == self.team_of(me) { WON } else { LOST };
        tracing::info!("game decided for player {me}: team {winner} won, {}", if result == WON { "won" } else { "lost" });
        {
            let mut t = self.tut();
            t.winner = winner;
            t.end_event = true;
        }
        self.outcome.store(result, Ordering::Relaxed);
    }

    /// The timed part of the goods race's victory hook 0x5c8580 (below the two default checks):
    /// once `(int)(tick · 0.071f) > 0xe0f` (an hour of game time), alliance 1 against all the
    /// others over the seven goods. For each good the sums of `CStatistic`'s goods on piles
    /// (0x11ac838 + 0x1128·p + 4·good, `AdjustStatistic`'s array, not D77's +0x1078): more goods
    /// won decides, then the larger total, then the game RNG (`(rand & 1) + 1`). Event 0x37 names
    /// team 1 or 2.
    fn goods_race_timeout(&mut self) {
        if (self.tick as f32 * 0.071_f32) as i32 <= 0xe0f {
            return;
        }
        let Some(w) = self.world() else { return };
        let (mut won, mut lost, mut team, mut rest) = (0, 0, 0i64, 0i64);
        for good in w.setup.race_goods {
            let (mut mine, mut others) = (0i64, 0i64);
            for p in 1..=w.number_of_players() as usize {
                let amount = w.statistic.players[p].goods[good as usize] as i64;
                if w.alliances.alliance_id[p] == 1 {
                    mine += amount;
                } else {
                    others += amount;
                }
            }
            team += mine;
            rest += others;
            match mine.cmp(&others) {
                std::cmp::Ordering::Greater => won += 1,
                std::cmp::Ordering::Less => lost += 1,
                std::cmp::Ordering::Equal => {}
            }
        }
        let winner = match (won.cmp(&lost), team.cmp(&rest)) {
            (std::cmp::Ordering::Greater, _) => 1,
            (std::cmp::Ordering::Less, _) => 2,
            (_, std::cmp::Ordering::Greater) => 1,
            (_, std::cmp::Ordering::Less) => 2,
            _ => {
                let Some(w) = self.world_mut() else { return };
                (w.rng.rand() & 1) as u8 + 1
            }
        };
        self.announce(winner);
    }

    fn default_check(&mut self, what: &'static str) {
        if self.world.is_null() {
            if !self.default_check_logged {
                tracing::warn!("{what}: no world");
                self.default_check_logged = true;
            }
            return;
        }
        self.default_lost_all();
    }
}

/// The free test of `Map.AddDecoObject` / `Map.AddReef` (0x5c98c0 / 0x5c9c80): every node of the
/// spiral up to radius 6 (0x117cf46) on the map, without a deco object and none of `flags`.
fn area_free(w: &World, x: i32, y: i32, flags: u8) -> bool {
    let size = w.grid.size() as i32;
    (0..=s4_sim::spiral::last(6)).all(|i| {
        let (px, py) = (x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i));
        (0..size).contains(&px) && (0..size).contains(&py) && {
            let n = w.grid.index(px as u32, py as u32);
            w.layers.deco_object[n] == 0 && w.layers.flags[n] & flags == 0
        }
    })
}

impl ScriptHost for GameHost {
    fn unhandled(&mut self, _func: &'static str, _args: &[Arg]) {
        self.unhandled += 1;
        self.unanswered += 1;
    }
    fn trace(&mut self, msg: &str) {
        tracing::debug!("script: {msg}");
    }
    fn game_local_player(&mut self) -> i32 {
        self.local_player
    }
    /// `Game.PlayerRace(player)` (0x5c6d00): the race of the given player's record
    /// (0x149b5d4 + 0x3c·player, unchecked); [inf] 0 for a player without one. Without a world
    /// (tests) the local race.
    fn game_player_race(&mut self, player: i32) -> i32 {
        let race = self.race;
        let Some(w) = self.world() else { return race };
        usize::try_from(player).ok().and_then(|p| w.players.get(p)).and_then(|p| p.as_ref()).map_or(0, |p| p.race as i32)
    }
    /// `Game.Time`: minutes, 840 ticks each.
    fn game_time(&mut self) -> i32 {
        (self.tick / 840) as i32
    }
    fn game_random(&mut self, n: i32) -> i32 {
        self.rng = self.rng.wrapping_mul(214_013).wrapping_add(2_531_011);
        if n > 0 { ((self.rng >> 16) & 0x7fff) as i32 % n } else { 0 }
    }
    /// `GlobalGameStateObject +0x34`: decided by the script, or for a team (event 0x37's, or free
    /// settle's from the start, `PlayerSetup::decided`).
    fn is_game_won(&mut self) -> bool {
        self.outcome.load(Ordering::Relaxed) != 0 || self.tut().winner != 0
    }
    /// 0x5c8560: `DefaultPlayersLostCheck` 0x5c8100, then `DefaultGameEndCheck` 0x5c8260.
    fn default_victory_check(&mut self) {
        self.default_check("ScriptDefaultVictoryConditionCheck");
        self.decide();
    }
    /// 0x5c8580, installed for game type 3 (0x4fda70) instead of the script's and the default
    /// check: those two checks, then the hour's limit.
    fn victory_hook(&mut self) -> bool {
        if self.world().is_none_or(|w| w.setup.game_type != 3) {
            return false;
        }
        self.default_victory_check();
        if self.outcome.load(Ordering::Relaxed) == 0 {
            self.goods_race_timeout();
        }
        true
    }
    fn game_default_game_end_check(&mut self, win_text: &str, lose_text: &str) {
        // 0x5c8260: the keys' ids (0xefe → 0), sent with event 0x37.
        self.tut().end_texts = (s4_script::text_key_id(win_text).unwrap_or(0), s4_script::text_key_id(lose_text).unwrap_or(0));
        // Only the decision over the teams: 0x5c8260 marks nobody lost (the scripts call
        // `DefaultPlayersLostCheck` / `DefaultPlayerLostCheck` for that).
        self.decide();
    }
    /// 0x5c7ff0: marks only.
    fn game_default_player_lost_check(&mut self, player: i32) {
        self.default_lost(player);
    }
    /// 0x5c8100: marks only; the scripts decide with `DefaultGameEndCheck`.
    fn game_default_players_lost_check(&mut self) {
        self.default_check("Game.DefaultPlayersLostCheck");
    }
    fn game_number_of_players(&mut self) -> i32 {
        self.world().map_or(self.players, |w| w.number_of_players())
    }
    fn buildings_amount(&mut self, player: i32, building: i32, status: i32) -> i32 {
        match self.world() {
            Some(w) => w.buildings_amount(player, building, status),
            None => self.unhandled_int("Buildings.Amount"),
        }
    }
    fn buildings_exists_building_in_area(&mut self, player: i32, building: i32, x: i32, y: i32, radius: i32, status: i32) -> i32 {
        match self.world() {
            Some(w) => w.exists_building_in_area(player, building, x, y, radius, status) as i32,
            None => self.unhandled_int("Buildings.ExistsBuildingInArea"),
        }
    }
    fn game_is_area_owned(&mut self, player: i32, x: i32, y: i32, radius: i32) -> i32 {
        match self.world() {
            Some(w) => w.is_area_owned(player, x, y, radius) as i32,
            None => self.unhandled_int("Game.IsAreaOwned"),
        }
    }
    fn game_is_area_green(&mut self, x: i32, y: i32, radius: i32) -> i32 {
        match self.world() {
            Some(w) => w.is_area_green(x, y, radius) as i32,
            None => self.unhandled_int("Game.IsAreaGreen"),
        }
    }
    fn game_are_player_areas_connected(&mut self, a: i32, xa: i32, ya: i32, b: i32, xb: i32, yb: i32) -> i32 {
        match self.world() {
            Some(w) => w.are_player_areas_connected(a, xa, ya, b, xb, yb) as i32,
            None => self.unhandled_int("Game.ArePlayerAreasConnected"),
        }
    }
    fn settlers_amount_in_area(&mut self, player: i32, settler: i32, x: i32, y: i32, radius: i32) -> i32 {
        match self.world() {
            Some(w) => w.settlers_amount_in_area(player, settler, x, y, radius),
            None => self.unhandled_int("Settlers.AmountInArea"),
        }
    }
    fn game_is_almost_all_land_explored(&mut self) -> i32 {
        match self.world() {
            Some(w) => w.is_almost_all_land_explored() as i32,
            None => self.unhandled_int("Game.IsAlmostAllLandExplored"),
        }
    }
    fn game_enable_land_explored_check(&mut self, enable: i32) {
        match self.world_mut() {
            Some(w) => w.enable_land_explored_check(enable != 0),
            None => {
                self.unhandled_int("Game.EnableLandExploredCheck");
            }
        }
    }
    fn game_get_difficulty(&mut self) -> i32 {
        self.difficulty
    }
    /// 0x5c7c60: a player 1..=8 with a team; marks only.
    fn game_player_lost(&mut self, player: i32) {
        if (1..=8).contains(&player) && self.team_of(player) != 0 {
            self.set_lost(player);
        }
    }
    /// 0x5c7d90: every player of another team than `player`'s (1..=8, with a team); marks only.
    fn game_enemy_players_lost(&mut self, player: i32) {
        if !(1..=8).contains(&player) {
            return;
        }
        let team = self.team_of(player);
        if team == 0 {
            return;
        }
        for p in 1..=self.players {
            if self.team_of(p) != team {
                self.set_lost(p);
            }
        }
    }
    /// `Game.ShowText(key, x, y)` (0x5c7a30): a known key → the message list (0x52a320).
    fn game_show_text(&mut self, text_key: &str, x: i32, y: i32) {
        if let Some(text) = s4_script::text_key_id(text_key) {
            self.tut().requests.push(Request::ShowText { text, x, y });
        }
    }
    /// `Debug.ShowTextMessage(text)` (0x5c4940): a string (or a number, converted) is shown as
    /// the console line 0x52a100(text, player 0); nil, tables and no argument show nothing.
    /// [simplification] The host sees those as "", so an empty string shows nothing either.
    fn debug_show_text_message(&mut self, text: &str) {
        if !text.is_empty() {
            self.tut().requests.push(Request::DebugText(text.to_string()));
        }
    }
    /// `Game.FindAnyUnit(player, x, y, radius)` (0x5c6660 → 0x5b0bb0): a player with a team, the
    /// node on the map, radius 1..=74: whether an entity of the player in war-map categories 0
    /// or 1 of the cells within the radius (0x5b0310) is within it (hex distance).
    fn game_find_any_unit(&mut self, player: i32, x: i32, y: i32, radius: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Game.FindAnyUnit") };
        let size = w.grid.size();
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 || (x as u32) >= size || (y as u32) >= size || !(1..=0x4a).contains(&radius) {
            return 0;
        }
        let found = s4_sim::warmap::cells_within(w, x, y, radius).into_iter().any(|(cx, cy)| {
            (0..2).any(|cat| {
                w.warmap.cell(cx as usize, cy as usize, cat).any(|id| {
                    w.entities.by_index(id as u32).is_some_and(|e| e.owner as i32 == player && s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= radius)
                })
            })
        });
        found as i32
    }
    /// `Game.SetFightingStrength(player, percent)` (0x5c7830): a player with a team and a
    /// percent other than 0 → 0x4f8380.
    fn game_set_fighting_strength(&mut self, player: i32, percent: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Game.SetFightingStrength", &[]) };
        if (1..=8).contains(&player) && w.alliances.team(player as u8) != 0 && percent != 0 {
            s4_sim::statistic::set_override(w, player as u8, percent);
        }
    }
    /// `Game.GetOffenceFightingStrength(player)` (0x5c6bb0): the foreign-land strength (+0xbc0)
    /// in percent (`· 100 >> 8`) for a player with a team, else 0.
    fn game_get_offence_fighting_strength(&mut self, player: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Game.GetOffenceFightingStrength") };
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 {
            return 0;
        }
        w.statistic.players.get(player as usize).map_or(0, |p| (p.foreign * 100) >> 8)
    }
    /// `Settlers.KillSelectableSettlers(player, type, x, y, radius, effects)` (0x5c0220): player
    /// -1 or one with a team, type -1 or 1..=0x42, the node on the map, radius 1..=74: every
    /// settler of warrior type 2..=7 on a node of the circle (the map-object layer; spiral order
    /// [inf], the original walks 0x4312e0) of that player and type dies (`Kill(id, 0)`), made
    /// invisible first with `effects` 0.
    fn settlers_kill_selectable_settlers(&mut self, player: i32, settler: i32, x: i32, y: i32, radius: i32, a6: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Settlers.KillSelectableSettlers", &[]) };
        let size = w.grid.size();
        let player_ok = player == -1 || ((1..=8).contains(&player) && w.alliances.team(player as u8) != 0);
        if !player_ok || !(settler == -1 || (1..0x43).contains(&settler)) || (x as u32) >= size || (y as u32) >= size || !(1..=0x4a).contains(&radius) {
            return;
        }
        let nodes: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, x, y, radius as u8).collect();
        for (px, py) in nodes {
            let id = w.layers.map_object[w.grid.index(px as u32, py as u32)];
            let Some(e) = w.entities.by_index_mut(id as u32) else { continue };
            let wt = e.flags & s4_sim::entity::flag::WARRIOR_TYPE_MASK;
            if id == 0 || e.obj_type != s4_sim::entity::ObjType::Settler || !(2..=7).contains(&wt) {
                continue;
            }
            if (settler != -1 && e.kind as i32 != settler) || (player != -1 && e.owner as i32 != player) {
                continue;
            }
            if a6 == 0 {
                e.flags &= !s4_sim::entity::flag::VISIBLE;
            }
            w.kill(id, 0);
        }
    }
    /// `Magic.CastSpell(player, race, spell, x, y)` (0x5c8ff0): a player with a team, the race
    /// (-1: the player's) below 5, spell 0..=7, the node on the map: `CastSpell(player, spell,
    /// x, y, (race | 8) << 24, 0)`, free, without range, no caster. Returns its result.
    fn magic_cast_spell(&mut self, player: i32, a2: i32, spell: i32, x: i32, y: i32) -> i32 {
        let Some(w) = self.world_mut() else { return self.unhandled_int("Magic.CastSpell") };
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 {
            return 0;
        }
        let race = if a2 >= 0 { a2 } else { w.race_of(player as u8) as i32 };
        let size = w.grid.size();
        if race >= 5 || !(0..8).contains(&spell) || (x as u32) >= size || (y as u32) >= size {
            return 0;
        }
        s4_sim::magic::cast_spell(w, player as u8, spell as u8, x, y, ((race as u32) | 8) << 24, 0)
    }
    /// `Map.AddDecoObject(x, y, object)` (0x5c98c0): the node on the map, object 1..=0x122: when
    /// every node of the spiral up to radius 6 is on the map, without a deco object and none of
    /// node flags 0x79, `AddDecoObjWithoutFlags(x, y, object, 0, grown)` (0x563940) and
    /// `SetFlagsForObject` (0x563510).
    fn map_add_deco_object(&mut self, x: i32, y: i32, object: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Map.AddDecoObject", &[]) };
        let size = w.grid.size();
        if (x as u32) >= size || (y as u32) >= size || !(1..0x123).contains(&object) {
            return;
        }
        if !area_free(w, x, y, 0x79) {
            return;
        }
        if s4_sim::deco::add_deco_obj_without_flags(w, x, y, object as u16, 0).is_ok() {
            s4_sim::deco::set_flags_for_object(w, x, y, object as u16);
        }
    }
    /// `Map.DeleteDecoObject(x, y, radius)` (0x5c9ac0): the node on the map, radius 1..=74: every
    /// node of the circle with node flag 0x40 (a deco object's ring) loses its deco object
    /// (0x564cc0 with `keep`).
    fn map_delete_deco_object(&mut self, x: i32, y: i32, radius: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Map.DeleteDecoObject", &[]) };
        let size = w.grid.size();
        if (x as u32) >= size || (y as u32) >= size || !(1..=0x4a).contains(&radius) {
            return;
        }
        let nodes: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, x, y, radius as u8).collect();
        for (px, py) in nodes {
            if w.layers.flags[w.grid.index(px as u32, py as u32)] & 0x40 != 0 {
                s4_sim::deco::remove_at(w, px, py, true);
            }
        }
    }
    /// `Game.GetAmountOfTreesInArea(x, y, radius)` (0x5c6930): the node on the map, radius
    /// 1..=74: the deco objects (obj type 0x20) of type 1..=22 (trees) on the circle's nodes.
    fn game_get_amount_of_trees_in_area(&mut self, x: i32, y: i32, radius: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Game.GetAmountOfTreesInArea") };
        let size = w.grid.size();
        if (x as u32) >= size || (y as u32) >= size || !(1..=0x4a).contains(&radius) {
            return 0;
        }
        s4_sim::spiral::walk(w.grid, x, y, radius as u8)
            .filter(|&(px, py)| {
                let id = w.layers.deco_object[w.grid.index(px as u32, py as u32)];
                w.entities.by_index(id as u32).is_some_and(|e| id != 0 && e.obj_type == s4_sim::entity::ObjType::Deco && (1..=22).contains(&e.kind))
            })
            .count() as i32
    }
    /// `Game.ResetFogging()` (0x5c7560): `CFogging` vtbl+0x30 (0x5add30).
    fn game_reset_fogging(&mut self) {
        let Some(w) = self.world_mut() else { return self.unhandled("Game.ResetFogging", &[]) };
        if let Some(mut f) = w.fog.take() {
            f.reset(w);
            w.fog = Some(f);
        }
    }
    /// `Game.SetAlliesDontRevealFog(on)` (0x5c7570): `CFogging` vtbl+0x2c (0x5add10) with
    /// `on != 0`.
    fn game_set_allies_dont_reveal_fog(&mut self, enable: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Game.SetAlliesDontRevealFog", &[]) };
        if let Some(f) = w.fog.as_mut() {
            f.set_allies_dont_reveal_fog(enable != 0);
        }
    }
    /// `Buildings.Delete(id, mode)` (0x5c5e10): a building: mode 0 its `Delete` (vtbl+0x10, [inf]
    /// as `Kill(id, 0)`), 1 `CrushBuilding` (0x536960), 2 destroyed (0x536a80, cause 0).
    fn buildings_delete(&mut self, building_id: i32, mode: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Buildings.Delete", &[]) };
        let Ok(id) = u16::try_from(building_id) else { return };
        if !w.entities.by_index(id as u32).is_some_and(|e| e.obj_type == s4_sim::entity::ObjType::Building) {
            return;
        }
        match mode {
            0 => w.kill(id, 0),
            1 => s4_sim::buildings::crush::crush_building(w, id),
            2 => s4_sim::buildings::crush::destroy(w, id, 0),
            _ => {}
        }
    }
    /// `Magic.DecreaseMana(player, amount)` (0x5c9460): a player in the game (`p − 1 <
    /// PlayerCount`) and `amount` > 0: the mana goes down by it, to 0 at least (unsigned
    /// compare). [inf] The pool's +0 bit 0 the original also sets is not kept.
    fn magic_decrease_mana(&mut self, player: i32, amount: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Magic.DecreaseMana", &[]) };
        if !(1..=w.player_count() as i32).contains(&player) || amount <= 0 {
            return;
        }
        if let Some(p) = w.magic.players.get_mut(player as usize) {
            p.mana = if (p.mana as u32) < amount as u32 { 0 } else { p.mana - amount };
        }
    }
    /// `Goods.Delete(x, y, radius, good)` (0x5c8d80): good -1 or 1..=0x2a, the node on the map,
    /// radius 1..=74: every pile (deco layer) of exactly that good on the circle loses its free
    /// goods (amount − leaving, `DecreaseUnforeseen` 0x56f8d0). Good -1 matches no pile.
    fn goods_delete(&mut self, x: i32, y: i32, radius: i32, good: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Goods.Delete", &[]) };
        let size = w.grid.size();
        if !(good == -1 || (1..0x2b).contains(&good)) || (x as u32) >= size || (y as u32) >= size || !(1..=0x4a).contains(&radius) {
            return;
        }
        let nodes: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, x, y, radius as u8).collect();
        for (px, py) in nodes {
            let id = w.layers.deco_object[w.grid.index(px as u32, py as u32)];
            let Some(p) = s4_sim::pile::get(w, id) else { continue };
            if id != 0 && p.good as i32 == good {
                let n = p.amount as i32 - p.leaving as i32;
                s4_sim::pile::decrease_unforeseen(w, id, n);
            }
        }
    }
    /// `Game.GetNumberOfSquaresWithDarkLand(n)` (0x5c6870 → 0x43c430): `n` 1..=255, else 0: the
    /// AI land cells whose dark-land count (+0x20) is at least `n`.
    fn game_get_number_of_squares_with_dark_land(&mut self, a1: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Game.GetNumberOfSquaresWithDarkLand") };
        if !(1..=0xff).contains(&a1) {
            return 0;
        }
        let side = (w.grid.size() >> 4) as i32;
        (0..side).flat_map(|cy| (0..side).map(move |cx| (cx, cy))).filter(|&(cx, cy)| w.animals.cells.get(cx, cy).dark() as i32 >= a1).count() as i32
    }
    /// `Game.IsAreaDarkLand(x, y, radius)` (0x5c6dc0 → 0x5b1af0): the node on the map, radius
    /// 1..=74: every node of the circle is dark land (node flag 0x04).
    fn game_is_area_dark_land(&mut self, x: i32, y: i32, radius: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Game.IsAreaDarkLand") };
        let size = w.grid.size();
        if (x as u32) >= size || (y as u32) >= size || !(1..=0x4a).contains(&radius) {
            return 0;
        }
        s4_sim::spiral::walk(w.grid, x, y, radius as u8).all(|(px, py)| w.layers.flags[w.grid.index(px as u32, py as u32)] & 4 != 0) as i32
    }
    /// `Map.AddReef(x, y, type)` (0x5c9c80): type 0x6c counts as 0x6b and 0x6e as 0x6d; the node
    /// on the map and the spiral up to radius 6 free (none of node flags 0x78): the object
    /// (0x563940), then, if it stands there, its flags (0x563510 with 1; [inf] as with 0).
    fn map_add_reef(&mut self, x: i32, y: i32, reef_type: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Map.AddReef", &[]) };
        let kind = match reef_type {
            0x6c => 0x6b,
            0x6e => 0x6d,
            k => k,
        };
        let size = w.grid.size();
        if (x as u32) >= size || (y as u32) >= size || !area_free(w, x, y, 0x78) {
            return;
        }
        let _ = s4_sim::deco::add_deco_obj_without_flags(w, x, y, kind as u16, 0);
        let id = w.layers.deco_object[w.grid.index(x as u32, y as u32)];
        if w.entities.by_index(id as u32).is_some_and(|e| id != 0 && e.obj_type == s4_sim::entity::ObjType::Deco && e.kind as i32 == kind) {
            s4_sim::deco::set_flags_for_object(w, x, y, kind as u16);
        }
    }
    /// `Map.DeleteReef(x, y)` (0x5c9eb0): the node on the map holding a deco object of type
    /// 0x6b..=0x6e: it goes (0x564d60: a static instance leaves the node with its flags, the
    /// resource is cleared).
    fn map_delete_reef(&mut self, x: i32, y: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Map.DeleteReef", &[]) };
        let size = w.grid.size();
        if (x as u32) >= size || (y as u32) >= size {
            return;
        }
        let id = w.layers.deco_object[w.grid.index(x as u32, y as u32)];
        if w.entities.by_index(id as u32).is_some_and(|e| id != 0 && (0x6b..=0x6e).contains(&e.kind)) {
            s4_sim::deco::remove_at(w, x, y, true);
        }
    }
    /// `Statistic.MushroomFarmsDestroyed(player)` (0x5ca350): a player with a team, the record
    /// 0x149b868 + 0x8c · p; else 0.
    fn statistic_mushroom_farms_destroyed(&mut self, player: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Statistic.MushroomFarmsDestroyed") };
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 {
            return 0;
        }
        w.statistic.players.get(player as usize).map_or(0, |p| p.mushroom_farms_destroyed)
    }
    /// `Statistic.UnitsDestroyed(player)` (0x5ca650): a player with a team, `CStatistic` +0xd28
    /// (0x11acb5c + 0x1128 · p: the settlers it killed); else 0.
    fn statistic_units_destroyed(&mut self, player: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Statistic.UnitsDestroyed") };
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 {
            return 0;
        }
        w.statistic.players.get(player as usize).map_or(0, |p| p.kills_total)
    }
    /// `Vehicles.AddVehicle(x, y, player, type, dir, ammo, ai)` (0x5c1280).
    #[allow(clippy::too_many_arguments)]
    fn vehicles_add_vehicle(&mut self, x: i32, y: i32, player: i32, vehicle: i32, direction: i32, a6: i32, a7: i32) -> i32 {
        let Some(w) = self.world_mut() else { return self.unhandled_int("Vehicles.AddVehicle") };
        s4_sim::vehicles::scripted::add_vehicle(w, x, y, player, vehicle, direction, a6, a7) as i32
    }
    /// `Vehicles.KillVehicles(player, type, x, y, radius, effects)` (0x5c22a0).
    fn vehicles_kill_vehicles(&mut self, player: i32, vehicle: i32, x: i32, y: i32, radius: i32, a6: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Vehicles.KillVehicles", &[]) };
        s4_sim::vehicles::scripted::kill_vehicles(w, player, vehicle, x, y, radius, a6);
    }
    /// `Settlers.AddSettlersToFerry(ferry, type, amount)` (0x5bffc0).
    fn settlers_add_settlers_to_ferry(&mut self, ferry_id: i32, settler: i32, amount: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Settlers.AddSettlersToFerry", &[]) };
        s4_sim::vehicles::scripted::add_settlers_to_ferry(w, ferry_id, settler, amount);
    }
    /// `Vehicles.AddWheelerToFerry(ferry, type)` (0x5c1600).
    fn vehicles_add_wheeler_to_ferry(&mut self, ferry_id: i32, vehicle: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Vehicles.AddWheelerToFerry", &[]) };
        s4_sim::vehicles::scripted::add_wheeler_to_ferry(w, ferry_id, vehicle);
    }
    /// `Vehicles.GetFerryCargoInArea(player, x, y, radius)` (0x5c1e80): six numbers.
    fn vehicles_get_ferry_cargo_in_area(&mut self, player: i32, x: i32, y: i32, radius: i32) -> [i32; 6] {
        let Some(w) = self.world() else {
            self.unhandled("Vehicles.GetFerryCargoInArea", &[]);
            return [0; 6];
        };
        s4_sim::vehicles::scripted::ferry_cargo_in_area(w, player, x, y, radius)
    }
    /// `Vehicles.GetHealth(x, y)` (0x5c27b0): −1 without a vehicle there.
    fn vehicles_get_health(&mut self, x: i32, y: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Vehicles.GetHealth") };
        s4_sim::vehicles::scripted::health_at(w, x, y)
    }
    /// `Sounds.PlaySound(id)` (0x5c9fb0): the argument truncated (`cvttsd2si`), then
    /// `PlaySound(id, 100, centre)` unless it is 0 or not below 0x6d (unsigned).
    fn sounds_play_sound(&mut self, sound: i32) {
        if (1..0x6d).contains(&sound) {
            self.tut().requests.push(Request::PlaySound(sound as u16));
        }
    }
    fn game_has_player_lost(&mut self, player: i32) -> i32 {
        ((1..=31).contains(&player) && self.lost & (1 << player) != 0) as i32
    }
    /// `Tutorial.Won` (0x5ca7a0): event 0x37 with team 1. `CStateGame::OnEvent` records the
    /// winner; in a tutorial (type 4) it shows no win screen (0x5172f0).
    fn tutorial_won(&mut self) {
        let mut t = self.tut();
        t.won = true;
        t.winner = 1;
        drop(t);
        self.outcome.store(WON, Ordering::Relaxed);
    }
    /// `Tutorial.Exit` (0x5ca830): event 0x17, the game ends (`CStateGame` +0x103).
    fn tutorial_exit(&mut self) {
        self.tut().requests.push(Request::Exit);
    }
    /// `Tutorial.GetCurrentZoomFactor` (0x5ca8c0): `CTutorial +4`.
    fn tutorial_get_current_zoom_factor(&mut self) -> i32 {
        self.tut().zoom
    }
    /// `Tutorial.SetMarker(x, y)` (0x5ca920): UI event 0x26e (`x << 16 | y`, 1); no range check.
    fn tutorial_set_marker(&mut self, x: i32, y: i32) {
        self.tut().marker = Some((x as u16, y as u16));
    }
    /// `Tutorial.ClearMarker` (0x5caa80): UI event 0x26e (0x10001, 0).
    fn tutorial_clear_marker(&mut self) {
        self.tut().marker = None;
    }
    /// `Tutorial.ShowText(key, line)` (0x5cab10): line 0 or 1 and a known key → UI event 0x263
    /// (string id, line).
    fn tutorial_show_text(&mut self, text_key: &str, mode: i32) {
        if !(0..2).contains(&mode) {
            return;
        }
        let Some(id) = s4_script::text_key_id(text_key) else { return };
        let mut t = self.tut();
        if t.text[mode as usize] != Some(id) {
            tracing::info!("tick {}: tutorial line {mode}: {text_key}", self.tick);
        }
        t.text[mode as usize] = Some(id);
        t.text_seq = t.text_seq.wrapping_add(1);
    }
    /// `Tutorial.DisableExcept(controls…)` (0x5cade0).
    fn tutorial_disable_except(&mut self, controls: &[i32]) {
        self.tut().controls.disable_except(controls);
    }
    /// `Tutorial.EnableControls(controls…)` (0x5caec0).
    fn tutorial_enable_controls(&mut self, controls: &[i32]) {
        self.tut().controls.enable(controls);
    }
    /// `Tutorial.DisableControls(controls…)` (0x5caf90): clears the bits like `EnableControls`
    /// (the release's code; see `tutorial::Controls::enable`).
    fn tutorial_disable_controls(&mut self, controls: &[i32]) {
        self.tut().controls.enable(controls);
    }
    /// `Tutorial.EnableAllControls` (0x5cada0).
    fn tutorial_enable_all_controls(&mut self) {
        self.tut().controls.enable_all();
    }
    /// `Tutorial.PressButton(dialog, control)` (0x5cacc0): dialog ≤ 0x4d, control 1..=0xb8b.
    fn tutorial_press_button(&mut self, dialog: i32, control: i32) {
        if (0..=0x4d).contains(&dialog) && (1..=0xb8b).contains(&control) {
            self.tut().requests.push(Request::PressButton { dialog: dialog as u8, control: control as u16 });
        }
    }
    /// `Tutorial.SelectNextBuilding(type)` (0x5cb060): type 1..=0x52.
    fn tutorial_select_next_building(&mut self, building: i32) {
        if (1..=0x52).contains(&building) {
            self.tut().requests.push(Request::SelectNextBuilding(building as u8));
        }
    }
    /// `Tutorial.RWM(on)` (0x5ca720): `CFogging` vtbl+0x28 (`SetRWM`) with `on == 1`, on the
    /// local player's fog (the client's [`World::fog`]; none headless).
    fn tutorial_rwm(&mut self, a1: i32) {
        if let Some(f) = self.world_mut().and_then(|w| w.fog.as_mut()) {
            f.set_reveal_map(a1 == 1);
        }
    }
    /// `Tutorial.SetZoom(level)` (0x5cb140): level 1..=99 → event 0x12.
    fn tutorial_set_zoom(&mut self, level: i32) {
        if (1..=99).contains(&level) {
            self.tut().requests.push(Request::SetZoom(level));
        }
    }
    /// `Tutorial.SetWorldCursor(x, y)` (0x5cb230): on the map → effect 0x39 there (the old one
    /// removed).
    fn tutorial_set_world_cursor(&mut self, x: i32, y: i32) {
        let size = self.world().map_or(0, |w| w.grid.size() as i32);
        if (0..size).contains(&x) && (0..size).contains(&y) {
            self.tut().arrow = Some((x as u16, y as u16));
        }
    }
    /// `Tutorial.DeleteWorldCursor` (0x5cb340).
    fn tutorial_delete_world_cursor(&mut self) {
        self.tut().arrow = None;
    }

    /// `Goods.AddPileEx(x, y, good, amount)` (0x5c8730): (x, y) on the map, good 1..=42, amount
    /// 1..=8 → 0x570c60, the goods spread over the free piles of that good around (x, y) and
    /// the rest dropped 50 spiral steps behind (`CPileMgr::SearchSpaceForGoods`).
    fn goods_add_pile_ex(&mut self, x: i32, y: i32, good: i32, amount: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Goods.AddPileEx", &[]) };
        let size = w.grid.size() as i32;
        if (0..size).contains(&x) && (0..size).contains(&y) && (1..=0x2a).contains(&good) && (1..=8).contains(&amount) {
            s4_sim::pile::search_space_for_goods(w, x, y, good as u8, amount as u8);
        }
    }
    /// `Map.PointIsOnScreen(x, y)` (0x5c9660 → 0x65a2e0): the node, with its height, projected on
    /// the screen inside the window. (Off the map the release returns nothing, i.e. nil; 0 here.)
    fn map_point_is_on_screen(&mut self, x: i32, y: i32) -> i32 {
        let view = self.tut().view;
        let (Some(w), Some(v)) = (self.world(), view) else { return 0 };
        let size = w.grid.size() as i32;
        if !(0..size).contains(&x) || !(0..size).contains(&y) {
            return 0;
        }
        let h = w.grid.checked_index(x, y).map_or(0, |n| w.layers.height[n]);
        let [wx, wy] = s4_viewer::terrain::node_pos(x as usize, y as usize, h);
        let (sx, sy) = ((wx - v.x) * v.zoom, (wy - v.y) * v.zoom);
        (sx >= 0.0 && sy >= 0.0 && sx < v.w && sy < v.h) as i32
    }
    /// `Settlers.AddSettlers(x, y, player, type, amount)` (0x5bfd90): (x, y) on the map, a
    /// player 1..8 in the game, type 1..=0x42, amount 1..=999 → 0x582770 with `ai` 1 (the AI's
    /// notice 0x14 for warriors).
    fn settlers_add_settlers(&mut self, x: i32, y: i32, player: i32, settler: i32, amount: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Settlers.AddSettlers", &[]) };
        let size = w.grid.size() as i32;
        if (0..size).contains(&x) && (0..size).contains(&y) && (1..=8).contains(&player) && w.alliances.team(player as u8) > 0 && (1..=0x42).contains(&settler) && (1..=999).contains(&amount) {
            s4_sim::create::add_settlers_around_ex(w, x, y, player as u8, settler as u16, amount as u32, 1);
        }
    }
    /// `Settlers.IsSelected(type, count)` (0x5c1100): along the selection, each settler (entity
    /// type 1) of `type` counts `count` down; 1 when it reaches 0. A count of 0 or less is 1 at
    /// the first selected settler of any type (as the release).
    fn settlers_is_selected(&mut self, settler: i32, count: i32) -> i32 {
        self.selection_count(&[s4_sim::entity::ObjType::Settler], settler, count)
    }
    /// `Vehicles.IsSelected(type, count)` (0x5c2610): the same over vehicles and ships (entity
    /// types 4 and 2).
    fn vehicles_is_selected(&mut self, vehicle: i32, count: i32) -> i32 {
        self.selection_count(&[s4_sim::entity::ObjType::LandVehicle, s4_sim::entity::ObjType::Ship], vehicle, count)
    }
    /// `Buildings.IsSelected(type)` (0x5c5820): a selected building (entity type 8) of the type.
    fn buildings_is_selected(&mut self, building: i32) -> i32 {
        let sel = self.tut().selection.clone();
        let Some(w) = self.world() else { return 0 };
        sel.iter().any(|&id| w.entities.by_index(id as u32).is_some_and(|e| e.obj_type == s4_sim::entity::ObjType::Building && e.kind as i32 == building)) as i32
    }
    /// `Buildings.AddBuilding(x, y, player, type)` (0x5c4ae0): (x, y) on the map, a player 1..8
    /// in the game, type 1..=0x52 → 0x53c580 with radius 4. Returns the building (0 if none).
    fn buildings_add_building(&mut self, x: i32, y: i32, player: i32, building: i32) -> i32 {
        self.add_building(x, y, player, building, 4)
    }
    /// `Buildings.AddBuildingEx(x, y, player, type)` (0x5c4d70): the same with radius −1 (0x18).
    fn buildings_add_building_ex(&mut self, x: i32, y: i32, player: i32, building: i32) -> i32 {
        self.add_building(x, y, player, building, -1)
    }
    /// `Buildings.GetTarget(id)` (0x5c5d00): a marketplace (33) or port (32) → its trading role's
    /// target (+0x17c), else 0.
    fn buildings_get_target(&mut self, building_id: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Buildings.GetTarget") };
        let Ok(id) = u16::try_from(building_id) else { return 0 };
        let trading = w.entities.by_index(id as u32).is_some_and(|e| e.obj_type == s4_sim::entity::ObjType::Building && matches!(e.kind, 32 | 33));
        if !trading {
            return 0;
        }
        s4_sim::buildings::trading::state(w, id).map_or(0, |s| s.target as i32)
    }
    /// `Vehicles.Amount(player, type, status)` (0x5c1780): a player 1..8 in the game, type 1..=5
    /// → 0x4368d0 with the status (0..=2, else 0): 1 under construction, 2 finished, 0 both
    /// (`CVehicleMgr` 0x1549a8c).
    fn vehicles_amount(&mut self, player: i32, vehicle: i32, a3: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Vehicles.Amount") };
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 || !(1..=5).contains(&vehicle) {
            return 0;
        }
        let i = player as usize * 6 + vehicle as usize;
        let (b, f) = (w.vehicles.building.get(i).copied().unwrap_or(0) as i32, w.vehicles.finished.get(i).copied().unwrap_or(0) as i32);
        match a3 {
            1 => b,
            2 => f,
            _ => b + f,
        }
    }
    /// `Vehicles.AmountInArea(player, type, x, y, radius)` (0x5c1930): the player's vehicles of
    /// the type (list 0x1549bd0) within grid distance `radius` (< 0x33) of (x, y).
    fn vehicles_amount_in_area(&mut self, player: i32, vehicle: i32, x: i32, y: i32, radius: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Vehicles.AmountInArea") };
        let size = w.grid.size() as i32;
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 || !(1..=5).contains(&vehicle) || !(0..size).contains(&x) || !(0..size).contains(&y) || !(0..0x33).contains(&radius) {
            return 0;
        }
        s4_sim::vehicles::iter(w, player as u8, vehicle as u16)
            .filter(|&id| w.entities.by_index(id as u32).is_some_and(|e| s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= radius))
            .count() as i32
    }
    /// `Goods.Amount(player, good)` (0x5c88f0): a player 1..8 in the game and good 1..=0x2a →
    /// the statistic's goods on the player's piles (0x11ac838, `CStatistic` +0xa04), else 0.
    fn goods_amount(&mut self, player: i32, good: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Goods.Amount") };
        if !(1..=8).contains(&player) || w.alliances.team(player as u8) == 0 || !(1..=0x2a).contains(&good) {
            return 0;
        }
        w.statistic.players.get(player as usize).and_then(|p| p.goods.get(good as usize)).copied().unwrap_or(0)
    }
    /// `Settlers.Amount(player, type)` (0x5c05a0): player 1..=player count →
    /// `CSettlerMgr::GetNumberOfSettlers` (0x1545658), else 0.
    fn settlers_amount(&mut self, player: i32, settler: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Settlers.Amount") };
        if !(1..=w.player_count() as i32).contains(&player) || settler < 0 {
            return 0;
        }
        w.settler_lists.settlers(player as u8, settler as u16)
    }
    /// `DarkTribe.AddManakopter(x, y, player)` (0x5cb370): a manakopter of `player` at the node
    /// (0x569ca0, unchecked; [`s4_sim::flying::add_manakopter`]). [inf] The player is clamped to
    /// 0..=8: the release indexes its per-player tables with it unchecked (0x569ca0's list heads
    /// `2 · player + type`, the entity's owner nibble) past their end; ours are sized for 9 players.
    fn dark_tribe_add_manakopter(&mut self, x: i32, y: i32, player: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("DarkTribe.AddManakopter", &[]) };
        s4_sim::flying::add_manakopter(w, x, y, player.clamp(0, 8) as u8);
    }
    /// `DarkTribe.FlyTo(x, y)` (0x5cb4a0): the last flying object created (0x153fc98) gets the
    /// goal, order bit 1 and an update next tick ([`s4_sim::flying::fly_to`]; the release reads
    /// it unchecked).
    fn dark_tribe_fly_to(&mut self, x: i32, y: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("DarkTribe.FlyTo", &[]) };
        let last = w.flying.last;
        s4_sim::flying::fly_to(w, last, x, y);
    }
    /// `DarkTribe.LeaveMk()` (0x5cb7a0): the last flying object created unloads (0x56b990).
    fn dark_tribe_leave_mk(&mut self) {
        let Some(w) = self.world_mut() else { return self.unhandled("DarkTribe.LeaveMk", &[]) };
        let last = w.flying.last;
        s4_sim::flying::unload(w, last);
    }
    /// `Game.DisableLastNPlayersInStatistic(n)` (0x5c81f0): the statistics leave out the last n
    /// players (0x11b589c = (int) n; the release stores it unless n ≥ `PlayerCount` and n ≤ 0).
    fn game_disable_last_n_players_in_statistic(&mut self, n: i32) {
        self.tut().requests.push(Request::DisableLastN(n));
    }
    /// `Game.ShowClock(seconds)` (0x5c7920): the clock, a countdown to `seconds` of game time
    /// unless 0 (s4_viewer::clock).
    fn game_show_clock(&mut self, seconds: i32) {
        self.tut().requests.push(Request::ShowClock(seconds));
    }
    /// `Map.SetScreenPos(x, y)` (0x5c9770): event 0x13, the camera to the node.
    fn map_set_screen_pos(&mut self, x: i32, y: i32) {
        self.tut().requests.push(Request::SetScreenPos(x, y));
    }
    /// `Buildings.GetFirstBuilding(player, type)` (0x5c5be0).
    fn buildings_get_first_building(&mut self, player: i32, building: i32) -> i32 {
        match self.world() {
            Some(w) => w.first_building(player, building) as i32,
            None => self.unhandled_int("Buildings.GetFirstBuilding"),
        }
    }
    /// `Buildings.CheckBuildingInhabitants(player, type)` (0x5c5930).
    fn buildings_check_building_inhabitants(&mut self, player: i32, building: i32) -> i32 {
        match self.world() {
            Some(w) => w.check_building_inhabitants(player, building) as i32,
            None => self.unhandled_int("Buildings.CheckBuildingInhabitants"),
        }
    }
    /// `Buildings.CrushBuilding(id)` (0x5c4cf0): a building entity not DIED (flag 0x4000000) →
    /// `CBuilding::CrushBuilding` (0x536960).
    fn buildings_crush_building(&mut self, building_id: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Buildings.CrushBuilding", &[]) };
        let ok = building_id > 0
            && w.entities.by_index(building_id as u32).is_some_and(|e| e.obj_type == s4_sim::entity::ObjType::Building && e.flags & s4_sim::entity::flag::DIED == 0);
        if ok {
            s4_sim::buildings::crush::crush_building(w, building_id as u16);
        }
    }
    /// `Buildings.IsWorkingArea(player, type, x, y)` (0x5c5530).
    fn buildings_is_working_area(&mut self, player: i32, building: i32, x: i32, y: i32) -> i32 {
        match self.world() {
            Some(w) => w.is_working_area(player, building, x, y) as i32,
            None => self.unhandled_int("Buildings.IsWorkingArea"),
        }
    }
    /// `Buildings.GetInhabitantAmount(player, type)` (0x5c5a70).
    fn buildings_get_inhabitant_amount(&mut self, player: i32, building: i32) -> i32 {
        match self.world() {
            Some(w) => w.inhabitant_amount(player, building),
            None => self.unhandled_int("Buildings.GetInhabitantAmount"),
        }
    }
    /// `Goods.GetAmountInArea(player, good, x, y, radius)` (0x5c8a30).
    fn goods_get_amount_in_area(&mut self, player: i32, good: i32, x: i32, y: i32, radius: i32) -> i32 {
        match self.world() {
            Some(w) => w.goods_amount_in_area(player, good, x, y, radius),
            None => self.unhandled_int("Goods.GetAmountInArea"),
        }
    }
    /// `Magic.CurrentManaAmount(player)` (0x5c9290): a player 1..8 in the game → the pool's
    /// mana (0x149b7f8 + 0x8c · player, +4 `^ 0x27356334`), else 0.
    fn magic_current_mana_amount(&mut self, player: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Magic.CurrentManaAmount") };
        if !w.is_used_player(player) {
            return 0;
        }
        s4_sim::magic::mana(w, player as u8)
    }
    /// `Magic.IncreaseMana(player, amount)` (0x5c9370): `player − 1 < PlayerCount` (signed: the
    /// release also writes player 0's pool; below 0 it writes outside the pools, skipped here)
    /// and amount > 0 → mana += amount. The total gained (+8) is not touched; the pool's GUI
    /// cache bit (+0) is set.
    fn magic_increase_mana(&mut self, player: i32, amount: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Magic.IncreaseMana", &[]) };
        if player > w.player_count() as i32 || amount <= 0 || player < 0 {
            return;
        }
        if let Some(p) = w.magic.players.get_mut(player as usize) {
            p.mana = p.mana.wrapping_add(amount);
        }
    }
    /// `Statistic.SpellsCast(player, spell)` (0x5ca4f0): a player 1..8 in the game and up to
    /// `PlayerCount`, spell 0..8 → the pool's casts of that spell (+0x30 + 4 · spell), else 0.
    fn statistic_spells_cast(&mut self, player: i32, spell_category: i32) -> i32 {
        let Some(w) = self.world() else { return self.unhandled_int("Statistic.SpellsCast") };
        if !w.is_used_player(player) || !(0..8).contains(&spell_category) {
            return 0;
        }
        s4_sim::magic::casts(w, player as u8, spell_category as u8) as i32
    }
    /// `Settlers.SetHealthInArea(player, type, x, y, radius, health)` (0x5c0900): (x, y) on the
    /// map and a player 1..=`PlayerCount`; each READY settler of the player and type (0x446 =
    /// any) on the spiral walk (settler layer, 0x547f20). `CSettler` vtbl+0x18 (0x5817d0) adds
    /// to the health up to the type's maximum, a negative amount sets the maximum; with
    /// `cur = vtbl+0x18(0)`: a lower `health` only goes through the role's vtbl+0x44
    /// (`cur − health`), whose result the release drops, so the health stays; a higher one calls
    /// vtbl+0x18(`cur − health`) < 0, which heals to the maximum. (The release returns 0 out
    /// of range and nothing otherwise; always 0 here.)
    fn settlers_set_health_in_area(&mut self, player: i32, settler: i32, x: i32, y: i32, radius: i32, health: i32) -> i32 {
        use s4_sim::entity::{EntityData, ObjType, flag};
        let Some(w) = self.world_mut() else { return self.unhandled_int("Settlers.SetHealthInArea") };
        if !w.grid.in_world(x, y) || !(1..=w.player_count() as i32).contains(&player) {
            return 0;
        }
        let r = radius.clamp(0, s4_sim::spiral::MAX_RADIUS as i32) as u8;
        let ids: Vec<u16> = s4_sim::spiral::walk(w.grid, x, y, r).map(|(px, py)| w.layers.map_object[w.grid.index(px as u32, py as u32)]).collect();
        for id in ids {
            let Some(e) = w.entities.by_index(id as u32) else { continue };
            if id == 0 || e.obj_type != ObjType::Settler || e.flags & flag::READY == 0 || e.owner as i32 != player || !(settler == s4_sim::query::ANY_SETTLER || e.kind as i32 == settler) {
                continue;
            }
            let max = w.rules.settler_health(e.race, e.kind);
            let Some(EntityData::Settler(s)) = w.entities.by_index_mut(id as u32).map(|e| &mut e.data) else { continue };
            s.health = s.health.min(max);
            let cur = s.health as i32;
            if health < cur {
                s4_sim::combat::role_hit(w, id, cur - health);
            } else if health > cur {
                s.health = max;
            }
        }
        0
    }
    /// `Effects.AddEffect(effect, sound, x, y, delay)` (0x5c5f00): effect 0..=0x55, (x, y) on the
    /// map, sound 0 unless below 0x6d → the effects manager's vtbl+0x10. TODO(port): the delay
    /// (the sim's effect event has none yet).
    fn effects_add_effect(&mut self, effect: i32, sound: i32, x: i32, y: i32, _delay: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Effects.AddEffect", &[]) };
        let size = w.grid.size() as i32;
        if (0..0x56).contains(&effect) && (0..size).contains(&x) && (0..size).contains(&y) {
            let sound = if (0..0x6d).contains(&sound) { sound as u16 } else { 0 };
            w.events.push(s4_sim::event::SimEvent::Effect { effect: effect as u8, sound, x, y });
        }
    }

    /// `dbg.aa(x1, y1, x2, y2)` (0x5c43f0): all four in 1..=size−2 → `CEffects` vtbl+0x18
    /// (kind 0, (x1, y1), (x2, y2), delay 0, offset 0): an arrow flies; nothing is returned.
    fn dbg_aa(&mut self, a1: i32, a2: i32, a3: i32, a4: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("dbg.aa", &[]) };
        let inner = |v: i32| (v as u32).wrapping_sub(1) < w.grid.size().wrapping_sub(2);
        if [a1, a2, a3, a4].into_iter().all(inner) {
            let (from, to) = (s4_sim::grid::Y16X16::pack(a1, a2), s4_sim::grid::Y16X16::pack(a3, a4));
            s4_sim::projectile::launch(w, 0, from, to, 0, (0, 0));
        }
    }

    // The `AI` table (release 0x43c… bindings, see `s4_sim::ai`).
    fn ai_set_player_var(&mut self, player: i32, var: &str, easy: i32, normal: i32, hard: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("AI.SetPlayerVar", &[]) };
        let teams = w.alliances.clone();
        w.ai.vars.set_player_var(player, var, easy, normal, hard, |p| teams.team(p) > 0);
    }
    fn ai_attack_now(&mut self, player: i32, a2: i32, a3: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("AI.AttackNow", &[]) };
        if is_ai(w, player) {
            s4_sim::ai::post_event(w, 0x1b, player, a2, a3);
        }
    }
    fn ai_dark_tribe_use_shamans(&mut self, player: i32, enable: i32) {
        self.ai_flags("AI.DarkTribeUseShamans", player, s4_sim::ai::vars::flag::DT_SHAMANS, enable != 0);
    }
    fn ai_dark_tribe_use_dark_gardeners(&mut self, player: i32, enable: i32) {
        self.ai_flags("AI.DarkTribeUseDarkGardeners", player, s4_sim::ai::vars::flag::DT_DARK_GARDENERS, enable != 0);
    }
    fn ai_dark_tribe_dbg_ignore_fighters(&mut self, player: i32, enable: i32) {
        self.ai_flags("AI.DarkTribeDbgIgnoreFighters", player, s4_sim::ai::vars::flag::DT_IGNORE_FIGHTERS, enable != 0);
    }
    fn ai_set_flag_bits(&mut self, player: i32, bits: i32) {
        self.ai_flags("AI.SetFlagBits", player, bits as u32, true);
    }
    fn ai_clear_flag_bits(&mut self, player: i32, bits: i32) {
        self.ai_flags("AI.ClearFlagBits", player, bits as u32, false);
    }
    fn ai_guard_region(&mut self, player: i32, region: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("AI.GuardRegion", &[]) };
        s4_sim::ai::guard_region(w, player, region);
    }
    fn ai_define_region(&mut self, region: i32, x: i32, y: i32, radius: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("AI.DefineRegion", &[]) };
        s4_sim::ai::define_region(w, region, x, y, radius);
    }
    /// 0x43cd40: player 1..8 in the game; command 7 / 9, or 8 / 13 with `(x, y)` on the map.
    fn ai_new_squad(&mut self, player: i32, command: i32, x: i32, y: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("AI.NewSquad", &[]) };
        let size = w.grid.size() as i32;
        let on_map = (0..size).contains(&x) && (0..size).contains(&y);
        if (1..=8).contains(&player) && w.alliances.team(player as u8) > 0 && matches!(command, 7 | 9 | 8 | 13) && (!matches!(command, 8 | 13) || on_map) {
            s4_sim::ai::post_event(w, 0x1c, player, command, (y << 16) | x);
        }
    }
    fn ai_deactivate_all_player_ais(&mut self) {
        let Some(w) = self.world_mut() else { return self.unhandled("AI.DeactivateAllPlayerAIs", &[]) };
        s4_sim::ai::remove_all(w);
    }
    /// 0x43d0f0: an AI player casts for free (flag 1, no caster).
    fn ai_cast_gratis_spell(&mut self, player: i32, spell: i32, x: i32, y: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("AI.CastGratisSpell", &[]) };
        if is_ai(w, player) {
            s4_sim::magic::cast_spell(w, player as u8, spell as u8, x, y, 1, 0);
        }
    }
    fn ai_dbg_prt(&mut self, _player: i32) {}
    fn ai_dbg_tst(&mut self) {}
    // The release's empty debug functions (each a lone `ret`): answered, so a check that calls
    // them (AO_trojan01's `dbg.pi`) still decides.
    /// 0x5c48e0.
    fn debug_assert_failed(&mut self) {}
    /// 0x5c4ad0.
    fn debug_print_int(&mut self) {}
    /// 0x5c49d0.
    fn dbg_tpd(&mut self) {}
    /// 0x5c49c0.
    fn debug_trace_print(&mut self) {}
    /// 0x5c48f0.
    fn debug_error(&mut self) {}
    fn ai_dbg_save_cfg_vars(&mut self) {}
    /// 0x5c3b90: player 1..8.
    fn debug_activate_ai(&mut self, player: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Debug.ActivateAI", &[]) };
        if (1..=8).contains(&player) {
            s4_sim::ai::add_player(w, player as u8);
        }
    }
    /// 0x5c4770.
    fn debug_deactivate_ai(&mut self, player: i32) {
        let Some(w) = self.world_mut() else { return self.unhandled("Debug.DeactivateAI", &[]) };
        if (1..=8).contains(&player) {
            s4_sim::ai::remove_player(w, player as u8);
        }
    }
}

/// `IsAIPlayer` for a script player number.
fn is_ai(w: &World, player: i32) -> bool {
    u8::try_from(player).is_ok_and(|p| w.ai.is_ai(p))
}

impl GameHost {
    /// The script-flag bindings (0x43d3e0 / 0x43d4b0 / 0x43d580 / 0x43d670 / 0x43d760): an AI
    /// player 1..8.
    fn ai_flags(&mut self, func: &'static str, player: i32, bits: u32, on: bool) {
        let Some(w) = self.world_mut() else { return self.unhandled(func, &[]) };
        if is_ai(w, player) {
            w.ai.vars.set_flags(player as u8, bits, on);
        }
    }
}

pub struct ScriptRunner {
    engine: Option<ScriptEngine<GameHost>>,
}

/// The game start's start resources ([`s4_sim::StartResources`], 0x4f26c0 → 0x5bd7b0): each
/// call runs `Script\Internal\StartResources.txt` and its `CreateStartResourcesFew` / `Medium` /
/// `Many(x, y, player, ai)` with the world reachable. [inf] The release runs it in the game's
/// script manager before the map script starts; here it gets a script engine of its own (made
/// at the first call), which the file cannot tell apart: it only defines the three functions,
/// and they only call `Game.PlayerRace`, `Buildings.AddBuildingEx`, `Goods.AddPileEx` and
/// `Settlers.AddSettlers`. A missing file or a failing engine makes no start resources.
pub fn start_resources(tutorial: crate::tutorial::Shared, outcome: Outcome) -> impl FnMut(&mut World, &s4_sim::StartResources) {
    let mut state: Option<Option<(Vec<u8>, ScriptEngine<GameHost>)>> = None;
    move |world, call| {
        let ready = state.get_or_insert_with(|| {
            let file = std::fs::read(s4_formats::path::resolve(s4_formats::game_dir().join("Script").join("Internal").join("StartResources.txt")))
                .inspect_err(|e| tracing::warn!(r"Script\Internal\StartResources.txt: {e}"))
                .ok()?;
            let host = GameHost {
                local_player: 0,
                race: 0,
                tick: 0,
                rng: 290_671,
                unhandled: 0,
                players: world.players.iter().skip(1).filter(|p| p.is_some()).count() as i32,
                difficulty: 0,
                lost: 0,
                outcome: outcome.clone(),
                default_check_logged: false,
                unanswered: 0,
                refused_logged: false,
                world: std::ptr::null_mut(),
                tutorial: tutorial.clone(),
            };
            let engine = ScriptEngine::new(host).inspect_err(|e| tracing::warn!("start resources: {e}")).ok()?;
            Some((file, engine))
        });
        let Some((file, engine)) = ready else { return };
        engine.host_mut().world = world;
        if let Err(e) = engine.create_start_resources(file, call.amount as u32, call.x, call.y, call.player as i32, call.ai) {
            tracing::warn!("start resources of player {}: {e}", call.player);
        }
        engine.host_mut().world = std::ptr::null_mut();
    }
}

impl ScriptRunner {
    /// Resolve and start the map's script (`CGameScriptManager::NewGameEx` + `StartScript`).
    /// `prefer_file` = campaign or tutorial.
    /// The host's own state for a savegame.
    pub fn host_state(&self) -> crate::save::HostState {
        match &self.engine {
            Some(e) => {
                let h = e.host_mut();
                crate::save::HostState { rng: h.rng, lost: h.lost, outcome: h.outcome.load(Ordering::Relaxed), winner: h.tut().winner }
            }
            None => Default::default(),
        }
    }

    /// The script's part of a savegame (`CGameScriptManager::Save`).
    pub fn saved_state(&self) -> Option<s4_script::SavedState> {
        self.engine.as_ref().and_then(|e| e.save_state().ok())
    }

    /// A saved game's script (`CGameScriptManager::Load` + `StartScript`: the chunk runs again,
    /// `register_functions`, no `new_game`).
    pub fn restore(saved: &crate::save::ScriptState, host: &crate::save::HostState, race: i32, players: i32, outcome: Outcome, local_player: i32, tutorial: crate::tutorial::Shared) -> Self {
        outcome.store(host.outcome, Ordering::Relaxed);
        {
            let mut t = tutorial.lock().unwrap_or_else(|e| e.into_inner());
            t.winner = host.winner;
            t.decided_on_load = host.outcome != 0;
        }
        let h = GameHost {
            local_player,
            race,
            tick: 0,
            rng: host.rng,
            unhandled: 0,
            players,
            difficulty: 0,
            lost: host.lost,
            outcome,
            default_check_logged: false,
            unanswered: 0,
            refused_logged: false,
            world: std::ptr::null_mut(),
            tutorial: tutorial.clone(),
        };
        let engine = (|| -> Option<ScriptEngine<GameHost>> {
            let mut e = ScriptEngine::new(h).ok()?;
            e.load_saved(&saved.to_saved()).ok()?;
            e.start().ok()?;
            Some(e)
        })();
        if engine.is_none() {
            tracing::warn!("savegame: the script could not be restored");
        }
        ScriptRunner { engine }
    }

    /// A cheat console line (0x515420 → 0x67f2a0) run in the script's state with `world`
    /// reachable; a game without a script ignores it.
    pub fn console(&mut self, code: &str, world: &mut World) {
        let Some(e) = &mut self.engine else { return };
        e.host_mut().world = world;
        if !e.console(code.as_bytes()) {
            tracing::info!("console: '{code}' failed");
        }
        e.host_mut().world = std::ptr::null_mut();
    }

    /// The script's `new_game` runs with `world` reachable: in the original it starts after the
    /// map load and the AI setup (CGameRunInitEntry, after 0x4ff5f0), so its `AI.SetPlayerVar`
    /// calls land in the AI records.
    #[allow(clippy::too_many_arguments)]
    pub fn new(map: &s4_formats::map::MapFile, map_name: &str, prefer_file: bool, race: i32, players: i32, outcome: Outcome, local_player: i32, tutorial: crate::tutorial::Shared, world: &mut World) -> Self {
        let chunk = map.script().ok().flatten().map(|s| s.into_bytes());
        let src = match resolve_map_script(chunk.as_deref(), map_name, prefer_file) {
            ScriptSource::Embedded(b) => Some((b, map_name.to_string())),
            ScriptSource::File(f) => std::fs::read(s4_formats::path::resolve(s4_formats::game_dir().join(&f))).ok().map(|b| (b, f)),
            ScriptSource::None => None,
        };
        let host = GameHost {
            local_player,
            race,
            tick: 0,
            rng: 290_671,
            unhandled: 0,
            players,
            difficulty: 0,
            lost: 0,
            outcome,
            default_check_logged: false,
            unanswered: 0,
            refused_logged: false,
            world: std::ptr::null_mut(),
            tutorial: tutorial.clone(),
        };
        let engine = src.and_then(|(bytes, name)| {
            let mut e = ScriptEngine::new(host).ok()?;
            e.host_mut().world = world;
            let started = e.load(&bytes, &name).and_then(|_| e.start());
            e.host_mut().world = std::ptr::null_mut();
            match started {
                Ok(()) => {
                    tracing::info!("script {name} started");
                    Some(e)
                }
                Err(err) => {
                    tracing::warn!("script {name}: {err}");
                    None
                }
            }
        });
        ScriptRunner { engine }
    }

}

impl ScriptDriver for ScriptRunner {
    fn update(&mut self, world: &mut World, tick: u32) {
        if let Some(e) = &mut self.engine {
            let pending = {
                let mut h = e.host_mut();
                h.tick = tick;
                h.unanswered = 0;
                h.world = world;
                let lost = std::mem::take(&mut world.script_events.lost);
                for p in (1..=8).filter(|p| lost & (1 << p) != 0) {
                    h.set_lost(p);
                }
                std::mem::take(&mut h.tut().pending)
            };
            // The simulation's game events of this tick (`GOODARRIVE`, `SETTLER_CHANGE_TYPE`),
            // then the UI's since the last update (tutorial.rs), before `TICK`.
            let sim_events = std::mem::take(&mut world.script_events.queue);
            for (event, args) in sim_events {
                if let Err(err) = e.send_event(event, &args) {
                    tracing::warn!("script event {event}: {err}");
                }
            }
            for (event, args) in pending {
                // UI event 9: `WARRIOR_SENT` when a settler is selected (0x4fab80).
                if event == crate::tutorial::RIGHT_DOWN {
                    let settler = {
                        let h = e.host_mut();
                        let sel = h.tut().selection.clone();
                        h.world().is_some_and(|w| sel.iter().any(|&id| w.entities.by_index(id as u32).is_some_and(|en| en.obj_type == s4_sim::entity::ObjType::Settler)))
                    };
                    if settler && let Err(err) = e.send_event(s4_script::events::WARRIOR_SENT, &[]) {
                        tracing::warn!("script event WARRIOR_SENT: {err}");
                    }
                    continue;
                }
                // The menu events set `gActiveMenu` first (event 0: only that).
                if event == 0 || event == s4_script::events::MENUCLICK {
                    let _ = e.set_global(crate::tutorial::ACTIVE_MENU, args.first().copied().unwrap_or(0));
                }
                if event != 0
                    && let Err(err) = e.send_event(event, &args)
                {
                    tracing::warn!("script event {event}: {err}");
                }
            }
            if let Err(err) = e.update(tick) {
                tracing::warn!("script tick {tick}: {err}");
            }
            e.host_mut().world = std::ptr::null_mut();
            // Which game events the script wants from the simulation (its requests now).
            world.script_events.wants = (1..32).filter(|&ev| e.requests(ev).slots.iter().any(|&s| s != 255)).fold(0, |m, ev| m | 1 << ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(src: &str, outcome: Outcome) -> ScriptEngine<GameHost> {
        let tutorial = crate::tutorial::shared();
        let host = GameHost {
            local_player: 1,
            race: 0,
            tick: 0,
            rng: 290_671,
            unhandled: 0,
            players: 2,
            difficulty: 0,
            lost: 0,
            outcome,
            default_check_logged: false,
            unanswered: 0,
            refused_logged: false,
            world: std::ptr::null_mut(),
            tutorial: tutorial.clone(),
        };
        let mut e = ScriptEngine::new(host).unwrap();
        e.load(src.as_bytes(), "t").unwrap();
        e.start().unwrap();
        e
    }

    /// The goods race's hook 0x5c8580 on roman02 (players 1 and 2): nothing before the hour, then
    /// goods won, the totals, the RNG; other game types have no hook. Player 1 is local.
    #[test]
    fn goods_race_is_decided_after_an_hour() {
        let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
        let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
        let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
        w.setup.race_goods = [1, 2, 3, 4, 5, 6, 7];
        w.alliances.alliance_id[1] = 1;
        w.alliances.alliance_id[2] = 2;
        for p in 3..w.alliances.alliance_id.len() {
            w.alliances.alliance_id[p] = 0;
        }
        for p in 1..=w.number_of_players() as usize {
            w.statistic.players[p].goods.fill(0);
        }
        let goods = |w: &mut World, mine: [i32; 7], rest: [i32; 7]| {
            for (i, (a, b)) in mine.into_iter().zip(rest).enumerate() {
                w.statistic.players[1].goods[i + 1] = a;
                w.statistic.players[2].goods[i + 1] = b;
            }
        };
        // (tick, the hook ran, the outcome, the winner)
        let run = |w: &mut World, tick: u32| {
            let outcome = Outcome::default();
            let e = engine("", outcome.clone());
            let mut h = e.host_mut();
            h.world = w;
            h.tick = tick;
            let hooked = h.victory_hook();
            let winner = h.tut().winner;
            h.world = std::ptr::null_mut();
            (hooked, outcome.load(Ordering::Relaxed), winner)
        };

        goods(&mut w, [9; 7], [1; 7]);
        assert!(!run(&mut w, 800).0, "game type 0 has no hook");
        w.setup.game_type = 3;
        assert_eq!(run(&mut w, 50_704), (true, 0, 0), "3599 s");
        assert_eq!(run(&mut w, 50_705), (true, WON, 1), "3600 s");
        // Goods won count before the totals: 4 to 3, although the others hold far more.
        goods(&mut w, [2, 2, 2, 2, 0, 0, 0], [1, 1, 1, 1, 90, 90, 90]);
        assert_eq!(run(&mut w, 60_000), (true, WON, 1));
        goods(&mut w, [0, 0, 0, 90, 90, 90, 90], [1, 1, 1, 91, 91, 91, 91]);
        assert_eq!(run(&mut w, 60_000), (true, LOST, 2));
        // Goods won equal: the totals.
        goods(&mut w, [5, 0, 0, 0, 0, 0, 0], [0, 4, 0, 0, 0, 0, 0]);
        assert_eq!(run(&mut w, 60_000), (true, WON, 1));
        goods(&mut w, [4, 0, 0, 0, 0, 0, 0], [0, 5, 0, 0, 0, 0, 0]);
        assert_eq!(run(&mut w, 60_000), (true, LOST, 2));
        // Everything equal: one draw of the game RNG.
        goods(&mut w, [3; 7], [3; 7]);
        let mut rng = w.rng;
        let team = (rng.rand() & 1) as u8 + 1;
        assert_eq!(run(&mut w, 60_000), (true, if team == 1 { WON } else { LOST }, team));
        assert_eq!(w.rng, rng, "exactly one draw");
    }

    /// The missions' host calls on roman02 with the world reachable: `Game.FindAnyUnit`,
    /// `SetFightingStrength` / `GetOffenceFightingStrength`, `Settlers.KillSelectableSettlers`,
    /// `Map.AddDecoObject` / `DeleteDecoObject`, `Game.GetAmountOfTreesInArea`,
    /// `Magic.DecreaseMana`, `Game.IsAreaDarkLand`, `Game.GetNumberOfSquaresWithDarkLand`.
    #[test]
    fn mission_calls_on_a_world() {
        let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
        let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
        let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
        let (sx, sy) = w.player(1).map(|p| (p.start_x as i32, p.start_y as i32)).unwrap();
        let e = engine("", Default::default());
        let run = |w: &mut World, code: &str| -> [f64; 9] {
            e.host_mut().world = w;
            assert!(e.console(code.as_bytes()), "{code}");
            let v = e.save_vars().unwrap();
            e.host_mut().world = std::ptr::null_mut();
            assert_eq!(e.host().unanswered, 0, "{code}: unanswered calls");
            v
        };
        // FindAnyUnit: player 1's units at its start, none in the corner, no team for player 9.
        let v = run(&mut w, &format!("Vars.Save1 = Game.FindAnyUnit(1, {sx}, {sy}, 10) Vars.Save2 = Game.FindAnyUnit(1, 3, 3, 2) Vars.Save3 = Game.FindAnyUnit(9, {sx}, {sy}, 10)"));
        assert_eq!(&v[..3], &[1.0, 0.0, 0.0]);
        // 150 % → the override 384, read back as 150 %.
        let v = run(&mut w, "Game.SetFightingStrength(1, 150) Vars.Save1 = Game.GetOffenceFightingStrength(1)");
        assert_eq!((w.statistic.players[1].over, v[0]), (384, 150.0));
        // Five swordsmen added at the start, then killed in the circle (other types spared).
        let before = w.entities.iter().filter(|(_, e)| e.owner == 1 && e.kind == 29 && !e.is_dead()).count();
        s4_sim::create::add_settlers_around(&mut w, sx, sy, 1, 29, 5);
        run(&mut w, &format!("Settlers.KillSelectableSettlers(1, 29, {sx}, {sy}, 20, 1)"));
        // Those on their node (the map-object layer); a garrison is not there and is spared.
        let near = |w: &World| {
            w.entities
                .iter()
                .filter(|(i, e)| e.owner == 1 && e.kind == 29 && !e.is_dead() && s4_sim::spiral::grid_distance(e.x() - sx, e.y() - sy) <= 20 && w.grid.checked_index(e.x(), e.y()).is_some_and(|n| w.layers.map_object[n] == *i))
                .count()
        };
        assert_eq!(near(&w), 0, "{before} before");
        // A tree on free land, counted, then deleted.
        let size = w.grid.size() as i32;
        let spot = (20..size - 20).flat_map(|y| (20..size - 20).map(move |x| (x, y))).find(|&(x, y)| area_free(&w, x, y, 0x79)).unwrap();
        let v = run(&mut w, &format!("Vars.Save1 = Game.GetAmountOfTreesInArea({x}, {y}, 3) Map.AddDecoObject({x}, {y}, 1) Vars.Save2 = Game.GetAmountOfTreesInArea({x}, {y}, 3)", x = spot.0, y = spot.1));
        assert_eq!((v[0], v[1]), (0.0, 1.0));
        run(&mut w, &format!("Map.DeleteDecoObject({}, {}, 2)", spot.0, spot.1));
        assert_eq!(w.layers.deco_object[w.grid.index(spot.0 as u32, spot.1 as u32)], 0);
        // Mana down, not below 0.
        w.magic.players[1].mana = 10;
        run(&mut w, "Magic.DecreaseMana(1, 4)");
        assert_eq!(w.magic.players[1].mana, 6);
        run(&mut w, "Magic.DecreaseMana(1, 40)");
        assert_eq!(w.magic.players[1].mana, 0);
        // roman02 has no dark land.
        let v = run(&mut w, &format!("Vars.Save1 = Game.IsAreaDarkLand({sx}, {sy}, 3) Vars.Save2 = Game.GetNumberOfSquaresWithDarkLand(1)"));
        assert_eq!((v[0], v[1]), (0.0, 0.0));
    }

    /// `Game.ShowText` and the lost events reach the client's queue: an unknown key nothing, each
    /// player's 0x38 once (`PlayerLost` twice, then `EnemyPlayersLost`), the end texts' ids.
    #[test]
    fn script_messages_reach_the_client() {
        let e = engine(
            r#"
            function new_game()
                Game.ShowText("AO_ROMAN_SCRIPT_THREE_01", 5, 6)
                Game.ShowText("NO_SUCH_KEY")
                Game.PlayerLost(2)
                Game.PlayerLost(2)
                Game.EnemyPlayersLost(2)
                Game.DefaultGameEndCheck("AO_MAYAN_SCRIPT_THREE_01", "AO_MAYAN_SCRIPT_THREE_02")
                Sounds.PlaySound(0)
                Sounds.PlaySound(109)
                Sounds.PlaySound(76.9)
                Debug.ShowTextMessage("hello")
            end
            "#,
            Default::default(),
        );
        let t = e.host().tutorial.clone();
        let t = t.lock().unwrap();
        let id = |k| s4_script::text_key_id(k).unwrap();
        assert_eq!(t.requests, vec![Request::ShowText { text: id("AO_ROMAN_SCRIPT_THREE_01"), x: 5, y: 6 }, Request::PlayerLost(2), Request::PlayerLost(1), Request::PlaySound(76), Request::DebugText("hello".into())]);
        assert_eq!(t.end_texts, (id("AO_MAYAN_SCRIPT_THREE_01"), id("AO_MAYAN_SCRIPT_THREE_02")));
    }

    /// The campaign scripts' pattern (roman02.txt, AO_*): a victory-check handler that declares
    /// the enemies lost, then `DefaultGameEndCheck`. It runs on the check ticks only
    /// (`tick & 7 == 0 && tick > 140`). The lost marks alone (0x5c7d90, 0x5c8100) decide nothing.
    #[test]
    fn script_victory_decides_the_game() {
        let marks: Outcome = Default::default();
        let mut e = engine(
            r#"
            function check() Game.EnemyPlayersLost(1) Game.DefaultPlayersLostCheck() end
            function register_functions() reg_func(check) end
            function new_game() request_event(check, Events.VICTORY_CONDITION_CHECK) end
            "#,
            marks.clone(),
        );
        e.update(144).unwrap();
        assert_eq!(marks.load(Ordering::Relaxed), 0);

        let won: Outcome = Default::default();
        let mut e = engine(
            r#"
            function check() if Game.Time() >= 0 then Game.EnemyPlayersLost(1) Game.DefaultGameEndCheck() end end
            function register_functions() reg_func(check) end
            function new_game() request_event(check, Events.VICTORY_CONDITION_CHECK) end
            "#,
            won.clone(),
        );
        e.update(140).unwrap();
        assert_eq!(won.load(Ordering::Relaxed), 0);
        e.update(144).unwrap();
        assert_eq!(won.load(Ordering::Relaxed), WON);

        let lost: Outcome = Default::default();
        let mut e = engine(
            r#"
            function check() Game.PlayerLost(1) end
            function register_functions() reg_func(check) end
            function new_game() request_event(check, Events.VICTORY_CONDITION_CHECK) end
            "#,
            lost.clone(),
        );
        e.update(144).unwrap();
        assert_eq!(lost.load(Ordering::Relaxed), LOST);

        // A decision resting on a query the host cannot answer yet is ignored.
        let unknown: Outcome = Default::default();
        let mut e = engine(
            r#"
            function check()
                if Buildings.ExistsBuildingInArea(2, Buildings.SMALLTEMPLE, 397, 368, 4) == 0 then Game.EnemyPlayersLost(1) end
            end
            function register_functions() reg_func(check) end
            function new_game() request_event(check, Events.VICTORY_CONDITION_CHECK) end
            "#,
            unknown.clone(),
        );
        e.update(144).unwrap();
        assert_eq!(unknown.load(Ordering::Relaxed), 0);

        // No handler: the default check runs, which is a stub (nothing decided).
        let none: Outcome = Default::default();
        let mut e = engine("function new_game() end", none.clone());
        e.update(144).unwrap();
        assert_eq!(none.load(Ordering::Relaxed), 0);
    }

    /// roman02's script (`Script/roman02.txt`) run by the game start with player 2 a computer
    /// player (control 2): the AI records come out as the original saved them in rom2.sav (chunk
    /// 150: player 2 at level 1, `UsePioneers` 0 and the soldier limits of all three columns).
    /// Skipped without the game data or the savegame.
    #[test]
    fn roman02_ai_records_as_the_save() {
        let map_path = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
        let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_default();
        let mut stack = vec![home.join(r"Documents\TheSettlers4\Save")];
        let mut save = None;
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("rom2.sav")) {
                    save = Some(p);
                }
            }
        }
        let (Some(save), Ok(map), Ok(rules)) = (save, s4_formats::map::MapFile::open(&map_path), s4_sim::Rules::load_default()) else { return };
        let c = s4_formats::map::container::Container::parse(std::fs::read(&save).unwrap()).unwrap();
        let (_, bytes) = c.load_id(150, 0).unwrap().unwrap();
        let saved = s4_sim::ai::chunk::parse(&bytes).unwrap();

        let mut setup = s4_sim::NewGame::default();
        setup.control[1] = 2;
        let (mut w, _) = s4_sim::new_game(&map, rules, &setup).unwrap();
        assert!(w.ai.is_ai(2));
        let players = w.player_count() as i32;
        let _runner = ScriptRunner::new(&map, "roman02.map", true, 0, players, Arc::new(AtomicU8::new(0)), 1, crate::tutorial::shared(), &mut w);
        for (k, r) in saved.records.iter().enumerate() {
            assert_eq!(w.ai.vars.records[k], s4_sim::ai::chunk::record_of(r), "player {k}");
        }
    }
}
