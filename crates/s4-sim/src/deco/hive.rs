//! `CHive` (object type 217, release vtable 0x1059e4c): the beekeeper's beehive. Evidence:
//! `tools/re/decompiled/script/planters/chive.c`.
//!
//! Stages (+0x45), each logic update (31 ticks) adding 1 to a counter (+0x48), 3 with flowers
//! nearby (object types 184..=188 among the 80 spiral entries from `First(2)`, 0x565950):
//!
//! * 0 → 1 after 20: job first + 0; 1 → 2 after 20; 2 → 3 after 20: job first + 1, READY (honey);
//! * 3: the hive's life (+0x4c, 406 from the constructor) runs down by one per update; at 0 it
//!   loses READY and turns to 4 (job first + 2);
//! * 4 → 5 after 20 (job first + 3); 5: a frame per 20 updates, deleted after the last.
//!
//! The beekeeper's harvest (`Decrease`, 0x565890): with 94 % the hive loses READY and starts over
//! at stage 0 (job first + 1), or 1 (job first + 0) with flowers nearby; else it only re-registers.

use super::delete;
use crate::entity::{DecoData, Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::world::World;

pub const HIVE: u16 = 217;
const UPDATE: i32 = 31;
/// Updates per stage step (`cmp [hive+0x48], 0x13`).
const STEP: i32 = 20;
/// +0x4c from the constructor (0x196).
const LIFE: i32 = 406;

pub fn is_hive(kind: u16) -> bool {
    kind == HIVE
}

fn data(world: &mut World, id: u16) -> Option<&mut DecoData> {
    match &mut world.entities.by_index_mut(id as u32)?.data {
        EntityData::Deco(d) => Some(d),
        _ => None,
    }
}

/// 0x565950: a flower (object types 184..=188) on one of the 80 spiral entries from `First(2)`
/// around (x, y).
fn flowers_near(world: &World, x: i32, y: i32) -> bool {
    let first = crate::spiral::first(2);
    (first..first + 80).any(|k| {
        let Some(n) = world.grid.checked_index(x + crate::spiral::dx(k), y + crate::spiral::dy(k)) else { return false };
        let id = world.layers.deco_object[n];
        world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Deco && (184..=188).contains(&e.kind))
    })
}

/// Stage `stage`, gfx job `first + job` and that job's cycle; frame 0 unless `keep_frame` (the
/// step 0 → 1 keeps it).
fn set(world: &mut World, id: u16, stage: u8, job: u8, keep_frame: bool) {
    let frames = world.rules.object_frames(HIVE, job as usize);
    if let Some(d) = data(world, id) {
        d.stage = stage;
        d.job = job;
        if !keep_frame {
            d.frame = 0;
        }
        d.cycle_frames = frames;
    }
}

/// `CHive::CHive(x, y, type, id)` (0x5652a0, via `AddDecoObjWithoutFlags`, from the map or a
/// beekeeper's PLANT): a hive of its own entity, life 406, stage 1 (job first + 0) with flowers
/// nearby, else stage 0 (job first + 1); first update in 31 ticks.
pub fn create(world: &mut World, x: i32, y: i32) -> Option<u16> {
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let flowers = flowers_near(world, x, y);
    let (stage, job) = if flowers { (1, 0) } else { (0, 1) };
    let data = DecoData { stage, job, cycle_frames: world.rules.object_frames(HIVE, job as usize), timer: LIFE, ..DecoData::default() };
    let mut e = Entity::new(ObjType::Deco, HIVE, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    e.flags |= flag::VISIBLE;
    world.entities.insert(slot, e);
    world.layers.deco_object[node] = slot;
    world.register_for_logic_update(slot, UPDATE);
    Some(slot)
}

/// `CHive::LogicUpdate` (0x5656b0).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y) = (e.x(), e.y());
    let EntityData::Deco(d) = &e.data else { return };
    let (stage, counter, frame, timer, job) = (d.stage, d.counter, d.frame, d.timer, d.job);
    let step = if flowers_near(world, x, y) { 3 } else { 1 };
    let bump = |world: &mut World, by: i32| -> bool {
        let Some(d) = data(world, id) else { return false };
        d.counter = counter + by;
        if d.counter >= STEP {
            d.counter = 0;
            true
        } else {
            false
        }
    };
    match stage {
        0 => {
            if bump(world, step) {
                set(world, id, 1, 0, true);
            }
        }
        1 => {
            if bump(world, step)
                && let Some(d) = data(world, id)
            {
                d.stage = 2;
            }
        }
        2 => {
            if bump(world, step) {
                set(world, id, 3, 1, false);
                if let Some(e) = world.entities.by_index_mut(id as u32) {
                    e.flags |= flag::READY;
                }
            }
        }
        3 => {
            if let Some(d) = data(world, id) {
                d.timer = timer - 1;
            }
            if timer - 1 < 1 {
                if let Some(e) = world.entities.by_index_mut(id as u32) {
                    e.flags &= !flag::READY;
                }
                set(world, id, 4, 2, false);
            }
        }
        4 => {
            if bump(world, 1) {
                set(world, id, 5, 3, false);
            }
        }
        5 => {
            if bump(world, 1) {
                let last = world.rules.object_frames(HIVE, job as usize);
                if let Some(d) = data(world, id) {
                    d.frame = frame.wrapping_add(1);
                }
                if frame.wrapping_add(1) >= last {
                    delete(world, id);
                    return;
                }
            }
        }
        _ => return,
    }
    world.register_for_logic_update(id, UPDATE);
}

/// `CHive::Decrease` (0x565890, vtbl+32): the beekeeper's harvest. With the game RNG at most
/// 0x10000000 (6 %) the hive only re-registers in 31 ticks; otherwise it loses READY, frame 0,
/// and starts over at stage 1 (job first + 0) with flowers nearby, else stage 0 (job first + 1).
pub fn decrease(world: &mut World, id: u16) {
    if world.rng.rand() << 16 <= 0x1000_0000 {
        world.register_for_logic_update(id, UPDATE);
        return;
    }
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags &= !flag::READY;
    let (x, y) = (e.x(), e.y());
    if flowers_near(world, x, y) {
        set(world, id, 1, 0, false);
    } else {
        set(world, id, 0, 1, false);
    }
}
