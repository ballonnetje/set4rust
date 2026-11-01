//! `CPlant` (object types 162 vine, 209 / 210 grain, 211 agave, 255 sunflower), from the shipped
//! exe (vtable 0x1059f44). A grown plant is a shared static instance (READY); a farmer's search
//! (`SearchGrain`) gives it a dynamic instance. Stages:
//!
//! * 0..2 growing (planted): 30 logic updates of 31 ticks per stage; reaching 3 turns the plant
//!   into the static instance again (READY);
//! * 3 grown; `Take(1)` (the harvest) moves it to 4;
//! * 4/5 harvested: grain is deleted after 30 updates; the others step through their last
//!   animation (a frame per 30 updates) and are deleted at its end.
//!
//! Stage `s` shows gfx job `GetObjectFirstJob(type) + s`. The good is per type
//! (`CDecoObjMgr::Create` 0x563d27): vine → wine, grain → grain, agave → agave, sunflower →
//! sunflower.

use super::{change_to_static_instance, delete};
use crate::entity::{DecoData, Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::world::World;

const UPDATE: i32 = 31;
/// Logic updates per growth / fading step (`cmp [plant+0x48], 0x1e`).
const STEP: i32 = 30;
/// Deco entity flag 0x100000 (the Maya's crop spell, `magic::spells::crops`): the update interval
/// becomes `(FasterCropsDecrease256 · 31 + 127) >> 8` (release 0x566574; the global 0x11abdbc is
/// `Magic.cfg`'s `FasterCropsDecrease256`, default 128 → 15 ticks).
pub const FLAG_FAST: u32 = 0x10_0000;

pub fn is_plant(kind: u16) -> bool {
    matches!(kind, 162 | 209 | 210 | 211 | 255)
}

/// The plant's good (`GOOD_*`).
pub fn good_of(kind: u16) -> u8 {
    match kind {
        162 => 37,       // WINE
        209 | 210 => 16, // GRAIN
        211 => 1,        // AGAVE
        255 => 42,       // SUNFLOWER
        _ => 0,
    }
}

fn data(world: &mut World, id: u16) -> Option<&mut DecoData> {
    match &mut world.entities.by_index_mut(id as u32)?.data {
        EntityData::Deco(d) => Some(d),
        _ => None,
    }
}

fn reschedule(world: &mut World, id: u16) {
    let fast = world.entities.by_index(id as u32).is_some_and(|e| e.flags & FLAG_FAST != 0);
    let delta = if fast { (world.rules.magic.faster_crops_decrease256.wrapping_mul(31) + 127) >> 8 } else { UPDATE };
    world.register_for_logic_update(id, delta);
}

/// `CPlant::CPlant(const CPlant& static, id, x, y)` (release 0x5661e0) via `SearchGrain`: a
/// dynamic, READY copy at stage 3 with the static instance's frame, cycle and good.
pub fn make_dynamic(world: &mut World, static_id: u16, x: i32, y: i32) -> Option<u16> {
    let e = world.entities.by_index(static_id as u32)?;
    let EntityData::Deco(d) = &e.data else { return None };
    let (kind, frame, cycle, good) = (e.kind, d.frame, d.cycle_frames, d.amount);
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let data = DecoData { static_instance: false, frame, stage: 3, cycle_frames: cycle, amount: good, counter: 0, ..DecoData::default() };
    let mut p = Entity::new(ObjType::Deco, kind, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    p.flags |= flag::VISIBLE | flag::READY;
    world.entities.insert(slot, p);
    world.layers.deco_object[node] = slot;
    Some(slot)
}

/// `CPlant::CPlant(x, y, type, id, good, 0)` (release 0x566030) via `Create(…, grown = 0)`: a
/// planted, dynamic plant at stage 0, first update in 31 ticks.
pub fn plant(world: &mut World, x: i32, y: i32, kind: u16) -> Option<u16> {
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let cycle = world.rules.object_frames(kind, 0);
    let data = DecoData { static_instance: false, frame: 0, stage: 0, cycle_frames: cycle, amount: good_of(kind), counter: 0, ..DecoData::default() };
    let mut p = Entity::new(ObjType::Deco, kind, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    p.flags |= flag::VISIBLE;
    world.entities.insert(slot, p);
    world.layers.deco_object[node] = slot;
    world.register_for_logic_update(slot, UPDATE);
    Some(slot)
}

/// `CPlant::LogicUpdate` (release 0x5664f0).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (kind, x, y) = (e.kind, e.x(), e.y());
    let EntityData::Deco(d) = &e.data else { return };
    let (stage, counter, frame) = (d.stage, d.counter + 1, d.frame);
    match stage {
        0..=2 => {
            if counter < STEP {
                if let Some(d) = data(world, id) {
                    d.counter = counter;
                }
                reschedule(world, id);
                return;
            }
            // Only the job changes; frame and cycle stay those of the planted stage.
            if let Some(d) = data(world, id) {
                d.counter = 0;
                d.stage = stage + 1;
            }
            if stage + 1 == 3 {
                if let Some(e) = world.entities.by_index_mut(id as u32) {
                    e.flags |= flag::READY;
                }
                change_to_static_instance(world, x, y, kind, 0);
            } else {
                reschedule(world, id);
            }
        }
        4 | 5 => {
            if counter < STEP {
                if let Some(d) = data(world, id) {
                    d.counter = counter;
                }
                world.register_for_logic_update(id, UPDATE);
                return;
            }
            if kind == 209 || kind == 210 {
                delete(world, id);
                return;
            }
            let frames = world.rules.object_frames(kind, stage as usize) as u32;
            if let Some(d) = data(world, id) {
                d.counter = 0;
                d.frame = frame.wrapping_add(1);
            }
            if frame as u32 + 1 >= frames {
                delete(world, id);
            } else {
                world.register_for_logic_update(id, UPDATE);
            }
        }
        _ => {}
    }
}

/// `CPlant::Take(n)` (release 0x566620) with `n = 1`: the harvest. Stage + 1, not READY any more,
/// frame 0, the new stage's animation, counter 0, next update in 31 ticks.
pub fn take(world: &mut World, id: u16) {
    take_n(world, id, 1);
}

/// `CPlant::Take(n)` (release 0x566620): stage + `n` (a byte; the Dark Tribe's rot,
/// `magic::spells::rot_at`, passes other amounts, also negative ones), then as [`take`].
pub fn take_n(world: &mut World, id: u16, n: i8) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let kind = e.kind;
    let EntityData::Deco(d) = &e.data else { return };
    let stage = d.stage.wrapping_add(n as u8);
    let cycle = world.rules.object_frames(kind, stage as usize);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::READY;
    }
    if let Some(d) = data(world, id) {
        d.stage = stage;
        d.frame = 0;
        d.cycle_frames = cycle;
        d.counter = 0;
    }
    world.register_for_logic_update(id, UPDATE);
}
