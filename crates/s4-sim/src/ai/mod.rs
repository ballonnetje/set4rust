//! The computer players: `CAIMain` (release vtable 0x102a8a4, the one instance at 0x146b148,
//! ctor 0x438230). Decompiles and the call-site inventory in `tools/re/decompiled/script/ai/`
//! and `docs/AI.md`.
//!
//! * `Init` (vtbl+0xc, 0x438370, end of the map load): the AI tables reset, the land cells built
//!   ([`crate::animals::cells::init`], 0x43c100), the per-player records reset to their defaults
//!   ([`vars`], 0x43c8c0). It runs whether AI players exist or not.
//! * At the game start (0x4ff5f0) every player whose control is 2 or 3 gets an AI (`AddPlayer`,
//!   vtbl+0x1c, 0x438530) and level 1 or 2.
//! * `Update` (vtbl+0x18, 0x4384d0) runs every tick between `Logic::Update` and
//!   `Tiling::Update` (GameLogicTick 0x4fe060): the AI tick, the land-cell rescan (0x43c280),
//!   then, with AI players, the global scheduler and each player AI (stage S1).
//! * `PostAIEvent` (vtbl+0x2c, 0x438620): the notices the rest of the game sends; per event type
//!   to one player AI, to all of them, or to nobody ([`post_event`]).
//! * Saved in chunk 150 ([`chunk`]).
//!
//! The AI draws the game RNG and acts through the same commands a player sends (SendNetEvent
//! 0x5a7990 / 0x5a7a90), so it belongs to the simulation.

pub mod attack;
pub mod chunk;
pub mod dark;
pub mod dark_attack;
pub mod dark_agents;
pub mod eco;
pub mod evaluation;
pub mod model;
pub mod normal_agents;
pub mod player;
pub mod priests;
pub mod sched;
pub mod sector;
pub mod shaman;
pub mod squads;
pub mod taskforce;
pub mod vars;

use crate::hash::StateHasher;
use crate::world::World;

/// Players 1..=8 (index 0 unused, as `CAIMain +0xc + 4·p`).
pub const PLAYERS: usize = 9;
/// AI regions 1..=15 (`AI.DefineRegion`; slot 0 unused).
pub const REGIONS: usize = 16;

/// One `PostAIEvent` record (16 bytes: type, player, two data words).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AiEvent {
    pub kind: i32,
    pub player: i32,
    pub a: i32,
    pub b: i32,
}

/// `CAIEventQueue` (vtable 0x102a4f4): a vector of events and a read index (Load 0x436f50,
/// Save 0x437030, Get 0x436ec0). Pushed by 0x43af20; the player AI's dispatch (0x43a420) reads
/// from the index to the end and then empties it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EventQueue {
    pub read: u32,
    pub events: Vec<AiEvent>,
}

/// `CAIPlayerAI` (base ctor 0x439b20; `CAINormalPlayerAI` 0x102aa0c or `CAIDarkTribePlayerAI`
/// 0x102a9f0 by the player's tribe, factory 0x4399c0).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerAi {
    /// +0xc.
    pub player: u8,
    /// The Dark Tribe class (`TribeTable` race 3).
    pub dark: bool,
    /// +0x6c.
    pub queue: EventQueue,
    /// +0x1c: the scheduler of its agents.
    pub sched: sched::Scheduler,
    /// The task forces, agents and sector AIs.
    pub body: model::PlayerBody,
    /// The Dark Tribe sector AI's `CAIDarkTribe` (+0x40c), made with that sector AI.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dark_ai: Option<Box<dark::DarkTribe>>,
}

impl PlayerAi {
    /// The ctor (0x439b20): the objects of [`model::PlayerBody::new`], the global suicide mission
    /// registered.
    pub fn new(player: u8, dark: bool) -> PlayerAi {
        let mut a = PlayerAi { player, dark, queue: EventQueue::default(), sched: Default::default(), body: model::PlayerBody::new(player, dark), dark_ai: None };
        sched::register_player(&mut a.sched, &mut a.body);
        a
    }

    fn hash(&self, h: &mut StateHasher) {
        let mut w = Vec::new();
        self.body.save(&self.queue, &mut w);
        h.u32(w.len() as u32);
        w.iter().for_each(|&v| h.u32(v));
        hash_sched(&self.sched, &self.body, h);
        for s in &self.body.sectors {
            let (model::SectorKind::Dark { agents } | model::SectorKind::Normal { agents, .. }) = &s.kind;
            agents.iter().for_each(|a| h.u32(a.state as u32));
            s.group.items.iter().for_each(|i| h.u32(i.tf.next_cast));
            if let model::SectorKind::Normal { eco, cursors, caches, .. } = &s.kind {
                caches.iter().for_each(|c| h.u32(c.cleaned));
                for c in cursors {
                    h.u8(c.player);
                    h.u16(c.kind);
                    h.u16(c.entity);
                }
                h.u32(eco.len() as u32);
                for e in eco {
                    h.u16(e.eco);
                    h.u32(e.uid);
                    h.u32(e.made);
                    h.u32(e.queue.len() as u32);
                }
            }
        }
        if let Some(d) = &self.dark_ai {
            d.hash(h);
        }
    }
}

/// An order the AI sent (`SendNetEvent` 0x5a7990 / `SendNetEvent2` 0x5a7a90): message id,
/// owner, `wParam`, `lParam`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AiOrder {
    pub kind: u32,
    pub player: u8,
    pub wparam: u32,
    pub lparam: u32,
}

/// `SendNetEvent(kind, wParam, lParam, player)`: the release hands the order to the network
/// engine, which every peer then runs at the start of a later tick (only the machine that
/// controls the player sends it, 0x149b608 + 0x3c·p). s4rs, whose peers all run every AI, keeps
/// it in the world and runs it at the start of the next tick after the players' commands
/// (`crate::tick`) [inf: the release's delay and order among the commands of that tick are not
/// traced].
pub fn send(world: &mut World, kind: u32, player: u8, wparam: u32, lparam: u32) {
    world.ai.orders.push(AiOrder { kind, player, wparam, lparam });
}

fn hash_sched(s: &sched::Scheduler, agents: &impl sched::Agents, h: &mut StateHasher) {
    h.u32(s.next);
    h.u32(s.queue.len() as u32);
    for &r in &s.queue {
        let (t, a, b) = match r {
            sched::AgentRef::Evaluation => (0, 0, 0),
            sched::AgentRef::GlobalSuicide => (1, 0, 0),
            sched::AgentRef::Sector(s, k) => (2, s, k),
        };
        h.bytes(&[t, a, b]);
        if let Some(a) = agents.get(r) {
            [a.next, a.period, a.cost, a.prio].iter().for_each(|&v| h.u32(v));
        }
    }
}

fn hash_ref(r: &player::TfRef, h: &mut StateHasher) {
    let (t, a, b, c) = match *r {
        player::TfRef::Reservoir(p, k) => (0, p, 0, k as u32),
        player::TfRef::Squad(p) => (1, p, 0, 0),
        player::TfRef::Slot(p, s, k) => (2, p, s, k as u32),
        player::TfRef::Force(p, s, k) => (3, p, s, k as u32),
        player::TfRef::Group(p, s, u) => (4, p, s, u),
    };
    h.bytes(&[t, a, b]);
    h.u32(c);
}

/// `AI.DefineRegion` (0x43cbe0 → 0x437ec0): a circle, radius > 0 when defined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub radius: i32,
}

/// `CAIMain` and the AI globals it saves.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ai {
    /// +4: set by `Init`, cleared by `Exit` (0x438430).
    pub active: bool,
    /// +8: bit `p` while player `p` has an AI.
    pub mask: u32,
    /// +0xc + 4·p.
    pub players: Vec<Option<PlayerAi>>,
    /// 0x1266b14: the game tick of the current update.
    pub tick: u32,
    /// 0x1266b18: saved after the version; cleared by `Init`, set by the map load right after it
    /// (0x4ff5ad: 0 on a campaign map of a campaign type below 11, else 0x10,
    /// [`crate::NewGame::campaign`]) and read by `CAIEcoSectorAIEx` (0x4346d0) into its engine
    /// (+0x20), where bit 0x10 lets the eco AI want hunters (0x651a90 type 7: `>> 3 & 2`) and stone
    /// mines (type 0xe).
    pub b18: i32,
    /// 0x1266b24: regions 0..15 (`AI.DefineRegion`, 0x437ec0; slot 0 never set).
    pub regions: [Region; REGIONS],
    /// 0x1266b20: the highest defined region (0x4380c0).
    pub top_region: i32,
    /// 0x1266be4: per player and region a flag word (`AI.GuardRegion` sets bit 0, 0x43c730).
    pub guard: [[u32; REGIONS]; PLAYERS],
    /// 0x1292640: the per-player records (level, script flags, variables).
    pub vars: vars::PlayerVars,
    /// The entities' AI infos (entity +0x10): the task force each one is in.
    #[cfg_attr(feature = "serde", serde(default))]
    pub infos: player::Infos,
    /// +0x30 (0x146b178): the global scheduler, with only [`Ai::evaluation`].
    pub sched: sched::Scheduler,
    /// 0x146b1a4: `CAIAgentEvaluation` "player evaluation".
    pub evaluation: model::Agent,
    /// Its +0x24: the player it evaluated last (1 from the ctor).
    pub evaluated: i32,
    /// The orders sent this tick ([`send`]), run at the start of the next.
    #[cfg_attr(feature = "serde", serde(default))]
    pub orders: Vec<AiOrder>,
    /// 0x15aa208: every task force, newest first ([`taskforce`]'s timer list); 0x15aa20c the
    /// cursor, 0x15aa204 the carried remainder.
    #[cfg_attr(feature = "serde", serde(default))]
    pub timers: Vec<player::TfRef>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub timer_cursor: Option<player::TfRef>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub timer_acc: i32,
    /// The entities' AI infos beyond the task force ([`player::InfoExt`]).
    #[cfg_attr(feature = "serde", serde(default))]
    pub ext: std::collections::BTreeMap<u16, player::InfoExt>,
    /// 0x1266e28 + 0x58·p: the player evaluations ([`evaluation`]; not in chunk 150).
    #[cfg_attr(feature = "serde", serde(default))]
    pub evals: Vec<[i32; evaluation::WORDS]>,
}

impl Default for Ai {
    fn default() -> Self {
        Ai {
            active: false,
            mask: 0,
            players: vec![None; PLAYERS],
            tick: 0,
            b18: 0,
            regions: [Region::default(); REGIONS],
            top_region: 0,
            guard: [[0; REGIONS]; PLAYERS],
            vars: vars::PlayerVars::default(),
            infos: Default::default(),
            sched: Default::default(),
            evaluation: model::Agent::new(model::AgentKind::Base),
            evaluated: 1,
            orders: Vec::new(),
            timers: Vec::new(),
            timer_cursor: None,
            timer_acc: 0,
            ext: Default::default(),
            evals: vec![[0; evaluation::WORDS]; PLAYERS],
        }
    }
}

impl Ai {
    /// `CAIMain`'s ctor (0x438230): the player evaluation registered (period 0xf) with the global
    /// scheduler. The release builds `CAIMain` once per process and never resets these two, so
    /// a second game in one process inherits them; s4rs makes them anew for each game (equal to
    /// the first game of a process), which keeps every game reproducible.
    pub(crate) fn register_evaluation(&mut self) {
        self.sched = Default::default();
        self.evaluated = 1;
        self.sched.register(&mut self.evaluation, sched::AgentRef::Evaluation, sched::EVALUATION_PERIOD);
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.bool(self.active);
        h.u32(self.mask);
        for p in &self.players {
            match p {
                None => h.u8(0),
                Some(p) => {
                    h.u8(1 + p.dark as u8);
                    p.hash(h);
                }
            }
        }
        h.u32(self.tick);
        h.u32(self.b18 as u32);
        for r in &self.regions {
            h.u32(r.x as u32);
            h.u32(r.y as u32);
            h.u32(r.radius as u32);
        }
        h.u32(self.top_region as u32);
        for g in &self.guard {
            g.iter().for_each(|&v| h.u32(v));
        }
        self.vars.hash(h);
        hash_sched(&self.sched, &self.evaluation, h);
        h.u32(self.evaluated as u32);
        h.u32(self.orders.len() as u32);
        for o in &self.orders {
            [o.kind, o.player as u32, o.wparam, o.lparam].iter().for_each(|&v| h.u32(v));
        }
        h.u32(self.infos.len() as u32);
        for (&id, r) in &self.infos {
            h.u16(id);
            hash_ref(r, h);
        }
        h.u32(self.timers.len() as u32);
        self.timers.iter().for_each(|r| hash_ref(r, h));
        match &self.timer_cursor {
            Some(r) => hash_ref(r, h),
            None => h.u8(0xff),
        }
        h.u32(self.timer_acc as u32);
        h.u32(self.ext.len() as u32);
        for (&id, x) in &self.ext {
            h.u16(id);
            h.u16(x.partner);
            h.u32(x.tick);
            x.tower.unwrap_or([-1; 3]).iter().for_each(|&v| h.u32(v as u32));
        }
        for e in &self.evals {
            e.iter().for_each(|&v| h.u32(v as u32));
        }
    }

    /// `CAIMain` vtbl+0x28 (0x4385f0) `IsAIPlayer(p)`.
    pub fn is_ai(&self, player: u8) -> bool {
        self.active && (1..=8).contains(&player) && self.players.get(player as usize).is_some_and(|p| p.is_some())
    }
}

/// `CAIMain::Init` (vtbl+0xc, 0x438370) as far as S0 goes: no players, the regions and guard
/// flags cleared (0x437fa0), the records at their defaults (0x43c8c0), the land cells built
/// (0x43c100), active. (The per-player statistics 0x437810 and the scheduler objects 0x4453f0
/// are stage S1.)
pub fn init(world: &mut World) {
    let vars = std::mem::take(&mut world.ai.vars);
    world.ai = Ai { vars, ..Ai::default() };
    world.ai.register_evaluation();
    world.ai.vars.reset();
    crate::animals::cells::init(world);
    world.ai.active = true;
}

/// The game start (0x4ff5f0): a player whose control (0x149b640 + 0x3c·p) is 2 or 3 gets an AI
/// (`AddPlayer`) and the level 1 (control 2) or 2 (control 3).
pub fn add_players_at_start(world: &mut World, control: &[u8; 8]) {
    for p in 1..=world.player_count() {
        let c = control.get(p as usize - 1).copied().unwrap_or(0);
        if c == 2 || c == 3 {
            add_player(world, p);
            world.ai.vars.set_level_unchecked(p, if c == 2 { 1 } else { 2 });
        }
    }
}

/// `CAIMain` vtbl+0x1c (0x438530) `AddPlayer(p)`: the player AI by the player's tribe (0x4399c0).
pub fn add_player(world: &mut World, player: u8) {
    if !world.ai.active || !(1..=8).contains(&player) || world.ai.players[player as usize].is_some() {
        return;
    }
    // 0x43854f: the first player AI (mask 0) starts from fresh evaluations (0x437e10: the nine
    // records cleared, 0x437810, then players 1..=PlayerCount evaluated, 0x4378b0).
    if world.ai.mask == 0 {
        world.ai.evals = vec![[0; evaluation::WORDS]; PLAYERS];
        for p in 1..=world.player_count() {
            evaluation::evaluate(world, p);
        }
    }
    let dark = world.race_of(player) == 3;
    world.ai.players[player as usize] = Some(PlayerAi::new(player, dark));
    taskforce::register_player(&mut world.ai, player);
    world.ai.mask |= 1 << player;
    // The factory's `Init` (vtbl+8).
    player::init(world, player);
}

/// A player left the network game (command 4009, [`crate::command::Command::PlayerLeft`]):
/// 0x5a2da0 sets its control (`GlobalGameState +0x6c`) to 2 or 3 and posts event 0x39, whose
/// handler (`EventManagement` 0x51777c) gives it an AI (`AddPlayer`) while it is still in the
/// game (record byte 0x149b608, [`crate::world::GameSetup::out`]). [inf] The AI level is not
/// set there (0x5a2da0 / 0x517795 write none): the record keeps its level. Not kept: +0x204 (the
/// tick it left), which nothing in the simulation reads.
pub fn player_left(world: &mut World, player: u8, control: u8) {
    if !(1..=8).contains(&player) || world.player(player).is_none() {
        return;
    }
    world.setup.control[player as usize] = control;
    if world.setup.out & (1 << player) == 0 {
        add_player(world, player);
    }
}

/// A player is out of the game (0x513070 / 0x517631: the record byte 0x149b608 cleared); the
/// loss path 0x517631 also removes its AI when it is a computer player (0x4f7a40, vtbl+0x20).
pub fn player_out(world: &mut World, player: u8) {
    if !(1..=8).contains(&player) {
        return;
    }
    world.setup.out |= 1 << player;
    if matches!(world.setup.control[player as usize], 2 | 3) {
        remove_player(world, player);
    }
}

/// `CAIMain` vtbl+0x20 (0x438580) `RemovePlayer(p)`.
pub fn remove_player(world: &mut World, player: u8) {
    if !world.ai.active || !(1..=8).contains(&player) {
        return;
    }
    if world.ai.players[player as usize].take().is_some() {
        world.ai.mask &= !(1 << player);
        // The task forces' destructors free their members' infos and leave the timer list.
        world.ai.infos.retain(|_, r| r.player() != player);
        let gone: Vec<player::TfRef> = world.ai.timers.iter().filter(|r| r.player() == player).copied().collect();
        for r in gone {
            taskforce::unregister(&mut world.ai, r);
        }
    }
}

/// `CAIMain` vtbl+0x24 (0x4385c0): every player AI removed (`AI.DeactivateAllPlayerAIs`).
pub fn remove_all(world: &mut World) {
    for p in 1..9 {
        remove_player(world, p);
    }
}

/// `CAIMain::Update` (vtbl+0x18, 0x4384d0).
pub fn update(world: &mut World) {
    world.ai.tick = world.tick;
    crate::animals::cells::rescan(world);
    if world.ai.mask == 0 {
        return;
    }
    // 0x445610: the task forces' timer list.
    taskforce::run_timers(world);
    player::run_scheduler(world, None, world.ai.tick, sched::BUDGET);
    for p in 1..=world.player_count().min(8) {
        player::update(world, p);
    }
}

/// `CAIMain::PostAIEvent(type, player, a, b)` (vtbl+0x2c, 0x438620).
/// * 0, 1, 0xc: nobody (sent to "player 0").
/// * 2: the land cells' incremental resource update (0x43c2e0); the release posts it nowhere.
/// * 3, 0x13: entity `a` leaves its AI task force (0x4456b0), then every player AI.
/// * 4..6, 0x11, 0x12, 0x16..0x18: the same, then player `player` (9: all).
/// * 7..0xb, 0xd..0x10, 0x14, 0x15, 0x19..0x1c: player `player` (9: all).
/// * 0x1d (the dark-land setter: `player` = x, `a` = y, `b` = dark): the land cell of `(x, y)`
///   (0x43c3c0), then every player AI.
/// * anything else: every player AI.
pub fn post_event(world: &mut World, kind: i32, player: i32, a: i32, b: i32) {
    let ev = AiEvent { kind, player, a, b };
    // 0x4456b0 (types 3..6, 0x11..0x13, 0x16..0x18): entity `a` leaves its AI task force.
    if matches!(kind, 3..=6 | 0x11..=0x13 | 0x16..=0x18) && a > 0 {
        player::on_delete(world, a as u16);
    }
    let to = match kind {
        0 | 1 | 0xc | 2 => return,
        3 | 0x13 => None,
        4..=6 | 0x11 | 0x12 | 0x16..=0x18 | 7..=0xb | 0xd..=0x10 | 0x14 | 0x15 | 0x19..=0x1c => {
            if player == 9 {
                None
            } else {
                Some(player)
            }
        }
        0x1d => {
            crate::animals::cells::dark_land_changed(world, player, a);
            None
        }
        _ => None,
    };
    match to {
        Some(p) => {
            if let Some(Some(ai)) = usize::try_from(p).ok().filter(|&p| (1..=8).contains(&p)).and_then(|p| world.ai.players.get_mut(p)) {
                ai.queue.events.push(ev);
            }
        }
        None => {
            let count = world.player_count() as usize;
            for ai in world.ai.players.iter_mut().take(count + 1).skip(1).flatten() {
                ai.queue.events.push(ev);
            }
        }
    }
}

/// `AI.DefineRegion(region, x, y, radius)` (0x43cbe0 → 0x437ec0): region 1..=15, `(x, y)` on the
/// map, radius > 0.
pub fn define_region(world: &mut World, region: i32, x: i32, y: i32, radius: i32) {
    let size = world.grid.size() as i32;
    if !(1..=15).contains(&region) || !(0..size).contains(&x) || !(0..size).contains(&y) || radius <= 0 {
        return;
    }
    world.ai.regions[region as usize] = Region { x, y, radius };
    world.ai.top_region = world.ai.top_region.max(region);
}

/// `AI.GuardRegion(player, region)` (0x43d850, inlined 0x43c730): player 1..=8 in the game,
/// region 1..=15: bit 0 of the player's flag word for the region.
pub fn guard_region(world: &mut World, player: i32, region: i32) {
    if !(1..=8).contains(&player) || world.alliances.team(player as u8) == 0 || !(1..=15).contains(&region) {
        return;
    }
    world.ai.guard[player as usize][region as usize] |= 1;
}
