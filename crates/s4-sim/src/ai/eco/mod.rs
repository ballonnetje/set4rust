//! The eco-sector AI: the economy half of a computer player (docs/ECOAI.md). Own reverse
//! engineering of the release; decompiles in `tools/re/decompiled/script/ecoai/`.
//!
//! * `CAIEcoSectorAIEx` ([`EcoSectorAi`], vtable 0x102a480, 0x2c bytes from the pool 0x117d12c):
//!   one per eco-sector of the player with at least 5 carriers, kept by the normal sector AI
//!   (+0xd94 / +0xd98; the list, its upkeep and the events are in [`super::sector`]). It owns the
//!   world interface `CEcoManager` ([`manager`]) and the planning engine `CEcoSectorAI`
//!   ([`engine`]).
//! * The original saves none of it: the end of a sector AI's load (0x443060) makes them anew
//!   (0x4439f0). The s4rs save keeps them whole, so a load continues as the run would have.

pub mod board;
pub mod engine;
pub mod filter;
pub mod manager;
pub mod places;
pub mod plan;
pub mod problems;
pub mod scores;
pub mod siedler;
pub mod tables;
pub mod tasks;
pub mod trace;
pub mod view;

use crate::World;
use super::EventQueue;

/// `CAIEcoSectorAIEx` (ctor 0x4346d0).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EcoSectorAi {
    /// +4 the eco-sector.
    pub eco: u16,
    /// +8 its unique id (`AllEcoSectors[id] + 0xc`) at creation.
    pub uid: u32,
    /// +0xc the AI tick at creation (0x1266b14).
    pub made: u32,
    /// +0x10 the events the sector AI forwards (`CAIEventQueue`, 0x4434a0 / 0x443440).
    pub queue: EventQueue,
    /// +0x24 `CEcoManager`.
    pub manager: manager::State,
    /// +0x28 `CEcoSectorAI`.
    pub engine: engine::Engine,
}

/// The ctor 0x4346d0 `(sector AI, eco-sector)`: eco +0x360 = 0 (the eco-sector's weapon
/// auto-production off), the manager (0x44d7f0), the engine (0x645210 with manager slots 0x14c
/// / 0x80 / 0x88: the sector AI, the player, the race), then the engine's vtbl+0x10 with
/// 0x1266b18 (`CEcoSectorAI` +0x20).
pub fn new(world: &mut World, player: u8, _sector_ai: u8, eco: u16) -> EcoSectorAi {
    let uid = world.economy.get(eco).map_or(0, |e| e.unique_id);
    if let Some(e) = world.economy.get_mut(eco) {
        e.weapon_auto = 0;
    }
    let race = world.race_of(player);
    let made = world.ai.tick;
    let mut ai = EcoSectorAi { eco, uid, made, queue: EventQueue::default(), manager: manager::State::new(world, eco, player), engine: engine::Engine::default() };
    let b18 = world.ai.b18;
    {
        let mut m = manager::World2 { world, state: &mut ai.manager, queue: &mut ai.queue };
        if trace::on() {
            trace::line(&format!("new {eco} {player} {race} {b18}"));
            let mut r = trace::Recorder { inner: &mut m };
            ai.engine = engine::Engine::new(&mut r, player as i32, race as i32);
        } else {
            ai.engine = engine::Engine::new(&mut m, player as i32, race as i32);
        }
    }
    ai.engine.set_f20(b18);
    ai
}

/// `Execute` (vtbl+4, 0x434910) of the eco-sector AI `k` of sector AI `sector_ai`: while the
/// eco-sector exists with the same unique id, one engine step (vtbl+4 0x645510 → 0x6516a0). (The
/// release brackets it with two debug-log entries.) The AI is taken out of its list for the
/// step; nothing the step does reaches the eco-sector AI list.
pub fn execute(world: &mut World, player: u8, sector_ai: u8, k: usize) {
    let Some(slot) = super::sector::eco_mut(world, player, sector_ai, k) else { return };
    let mut ai = std::mem::take(slot);
    ai.step(world);
    if let Some(slot) = super::sector::eco_mut(world, player, sector_ai, k) {
        *slot = ai;
    }
}

impl EcoSectorAi {
    fn step(&mut self, world: &mut World) {
        let alive = self.eco < 0x4000 && world.economy.get(self.eco).is_some_and(|e| e.unique_id == self.uid);
        if !alive {
            return;
        }
        let mut m = manager::World2 { world, state: &mut self.manager, queue: &mut self.queue };
        if trace::on() {
            trace::line(&format!("step {}", self.eco));
            let mut r = trace::Recorder { inner: &mut m };
            self.engine.step(&mut r);
        } else {
            self.engine.step(&mut m);
        }
    }
}

/// The sector AI's side of the eco-sector AI's queue: 0x43af20 pushes, 0x443b50 (with the
/// player's script flag bit 0) empties it.
impl EventQueue {
    pub fn push(&mut self, ev: super::AiEvent) {
        self.events.push(ev);
    }

    /// The events held (read or not).
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn clear(&mut self) {
        *self = EventQueue::default();
    }
}
