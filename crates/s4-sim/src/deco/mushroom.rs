//! `CMushroom` (object types 212..=214, release vtable 0x1059ec8): the Dark Tribe's mushrooms.
//! Evidence: `tools/re/decompiled/script/dark/mushroom.c`, `deco_add.c`, `gardener2.c`.
//!
//! * A grown mushroom is a shared static instance, READY, at stage 3 (ctor 0x565a20 with stage 3:
//!   a random start frame, game RNG).
//! * A planted one (ctor with stage 0) is dynamic, first update in 31 ticks. `LogicUpdate`
//!   (0x565ea0): every 5 updates of 31 ticks a stage (job `first + stage`; frame and cycle stay);
//!   reaching 3 it turns back into the static instance (0x564390).
//! * A gardener beating a grown one first gives it a dynamic copy at stage 3 (copy ctor 0x565bd0:
//!   job `first + 3`, the static one's frame and cycle). The beat (vtbl+0x20, 0x565f90) moves it
//!   to stage 4: READY off, job `first + 4`, frame 0; then a frame every 5 updates, deleted after
//!   the last (`CDecoObjMgr::Delete` 0x563410).
//! * Drawing (0x565df0) advances the frame by the ticks since it was last drawn, at every stage.

use super::{change_to_static_instance, delete};
use crate::entity::{DecoData, Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::world::World;

const UPDATE: i32 = 31;
/// Updates per stage (`cmp [mushroom+0x48], 4`, then `> 4`).
const STEP: i32 = 5;

pub fn is_mushroom(kind: u16) -> bool {
    matches!(kind, 212..=214)
}

fn data(world: &mut World, id: u16) -> Option<&mut DecoData> {
    match &mut world.entities.by_index_mut(id as u32)?.data {
        EntityData::Deco(d) => Some(d),
        _ => None,
    }
}

/// The ctor (0x565a20) with stage 0, via `Create(…, grown = 0)`: a planted, dynamic mushroom,
/// frame 0, first update in 31 ticks.
pub fn plant(world: &mut World, x: i32, y: i32, kind: u16) -> Option<u16> {
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let cycle = world.rules.object_frames(kind, 0);
    let data = DecoData { static_instance: false, frame: 0, stage: 0, cycle_frames: cycle, amount: 0, counter: 0, job: 0, ..DecoData::default() };
    let mut m = Entity::new(ObjType::Deco, kind, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    m.flags |= flag::VISIBLE;
    world.entities.insert(slot, m);
    world.layers.deco_object[node] = slot;
    world.register_for_logic_update(slot, UPDATE);
    Some(slot)
}

/// The copy ctor (0x565bd0) a beating gardener makes of the static instance at `(x, y)`: dynamic,
/// stage 3, job `first + 3`, the static one's frame and cycle (READY is not copied: the copy has
/// the base flags). Returns the new id.
pub fn make_dynamic(world: &mut World, static_id: u16, x: i32, y: i32) -> Option<u16> {
    let e = world.entities.by_index(static_id as u32)?;
    let EntityData::Deco(d) = &e.data else { return None };
    let (kind, frame, cycle) = (e.kind, d.frame, d.cycle_frames);
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let data = DecoData { static_instance: false, frame, stage: 3, cycle_frames: cycle, amount: 0, counter: 0, ..DecoData::default() };
    let mut m = Entity::new(ObjType::Deco, kind, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    m.flags |= flag::VISIBLE;
    world.entities.insert(slot, m);
    world.layers.deco_object[node] = slot;
    Some(slot)
}

/// `CMushroom` vtbl+0x20 (0x565f90): beaten — the next stage, READY off, job `first + stage`
/// with its frame count, frame 0; 31 ticks to the next update.
pub fn hit(world: &mut World, id: u16) {
    let Some(kind) = world.entities.by_index(id as u32).map(|e| e.kind) else { return };
    let Some(stage) = data(world, id).map(|d| d.stage.wrapping_add(1)) else { return };
    let frames = world.rules.object_frames(kind, stage as usize);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::READY;
    }
    if let Some(d) = data(world, id) {
        d.stage = stage;
        d.frame = 0;
        d.cycle_frames = frames;
        d.counter = 0;
    }
    world.register_for_logic_update(id, UPDATE);
}

/// `CMushroom::LogicUpdate` (0x565ea0).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (kind, x, y) = (e.kind, e.x(), e.y());
    let EntityData::Deco(d) = &e.data else { return };
    let (stage, counter, frame, cycle) = (d.stage, d.counter + 1, d.frame, d.cycle_frames);
    match stage {
        0..=2 => {
            if counter < STEP {
                if let Some(d) = data(world, id) {
                    d.counter = counter;
                }
            } else {
                // Only the job changes; frame and cycle stay those of the planted stage.
                if let Some(d) = data(world, id) {
                    d.counter = 0;
                    d.stage = stage + 1;
                }
                if stage + 1 == 3 {
                    // 0x564390: the dynamic one goes, the static instance takes the node.
                    change_to_static_instance(world, x, y, kind, 0);
                    return;
                }
            }
            world.register_for_logic_update(id, UPDATE);
        }
        3 => {
            if let Some(d) = data(world, id) {
                d.counter = 0;
            }
            world.register_for_logic_update(id, UPDATE);
        }
        4 => {
            if counter < STEP {
                if let Some(d) = data(world, id) {
                    d.counter = counter;
                }
                world.register_for_logic_update(id, UPDATE);
                return;
            }
            let frame = frame.wrapping_add(1);
            if let Some(d) = data(world, id) {
                d.frame = frame;
                d.counter = 0;
            }
            // (`GetObjectFrameCount(job)` again, the cycle of the beaten stage.)
            if frame >= cycle {
                delete(world, id);
                return;
            }
            world.register_for_logic_update(id, UPDATE);
        }
        _ => {}
    }
}
