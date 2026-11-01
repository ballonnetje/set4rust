//! Deterministic simulation of The Settlers IV, ported system by system from `S4_Main.exe`.
//!
//! Mechanics follow the original (each function names the one it mirrors, addresses are labels
//! in the IDA decompilation, see `docs/ADDRESSES.md`); the structure is our own:
//!
//! * everything lives in one [`World`] value, no globals;
//! * integer arithmetic only — no `f32`/`f64` anywhere in this crate (enforced by the lints
//!   below and by `tests/no_floats.rs`);
//! * node data is structure-of-arrays ([`Layers`]), entities sit in one generational arena
//!   ([`Entities`]) that keeps the original id allocation policy;
//! * one entry point, [`tick`], runs the original update order (`CGameRun::Run`);
//! * [`World::state_hash`] fingerprints the whole state for replay and desync tests.
//!
//! See `docs/SIM.md` for the research behind each part and the porting order.

#![forbid(unsafe_code)]
#![deny(clippy::float_arithmetic, clippy::float_cmp, clippy::float_cmp_const, clippy::cast_precision_loss)]

pub mod ai;
pub mod alliances;
pub mod animals;
pub mod build;
pub mod buildings;
pub mod cfg;
pub mod combat;
pub mod command;
pub mod create;
pub mod dark;
pub mod debug;
pub mod deco;
pub mod eco;
pub mod entity;
pub mod event;
pub mod flying;
pub mod fog;
pub mod future;
pub mod grid;
pub mod group;
pub mod hash;
pub mod jobs;
pub mod load;
pub mod magic;
pub mod rng;
pub mod rules;
pub mod pathing;
pub mod pile;
pub mod projectile;
pub mod query;
pub mod save;
pub mod save_economy;
pub mod schedule;
pub mod settler_lists;
pub mod sfloat;
pub mod spiral;
pub mod statistic;
pub mod territory;
pub mod tick;
pub mod tiling;
pub mod vehicles;
pub mod warmap;
pub mod wear;
pub mod world;

pub use command::{Command, NetMessage, PlayerCommand, Unsupported};
pub use entity::{Entities, Entity, EntityData, EntityId, ObjType};
pub use grid::{Grid, Layers, Y16X16};
pub use load::{LoadReport, NewGame, StartResources, new_game, new_game_with};
pub use rng::Random16;
pub use rules::Rules;
pub use schedule::Schedule;
pub use tick::{NoProbe, NoScript, Probe, ScriptDriver, TickProbe, tick, tick_probed, tick_with_script};
pub use world::{Player, World};
