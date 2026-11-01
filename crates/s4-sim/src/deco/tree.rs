//! `CTree` (MapObjects/Tree.cpp, object types 1..=22). A grown tree is a shared static instance
//! with resource `0x70 | 1` on its node. A woodcutter's search (`CSearchRoutines::SearchTree`)
//! gives it a dynamic instance of its own; work then steps it through the felling stages:
//!
//! * 0..2 growing (planted by a forester): 40 logic updates of 31 ticks per stage; reaching 3
//!   sets the resource and turns the tree back into a static instance;
//! * 3 grown; `Decrease` (the woodcutter's WORK) clears the resource and starts 4, falling;
//! * 4 falls for its animation, then 5; 5..8 are chopped by further `Decrease`s;
//! * 9: the last chop; two updates later 10, the stump fading out frame by frame (every 2nd
//!   update), then the object is deleted.
//!
//! Each stage shows gfx job `GetObjectFirstJob(type) + stage` (+0x26, drawn by `GetGfxInfo`
//! 0x593b80, which advances the frame by the ticks since its last call except at stage 10).
//!
//! Re-verified against the release (vtable 0x105ce68): constructor 0x593880, copy constructor
//! 0x593970, LogicUpdate 0x593c50, Decrease 0x593e50 (vtbl+32), Take 0x593ef0 (vtbl+36);
//! evidence in `tools/re/decompiled/script/tree/`.

use super::{change_to_static_instance, delete};
use crate::entity::{DecoData, Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::world::World;

/// `CTree::GetGoodType`: `GOOD_LOG`.
pub const GOOD: u8 = 22;
/// `CTree::LogicUpdate` re-registers every 31 ticks while growing or fading.
const UPDATE: i32 = 31;
/// Resource type of a grown tree (`CWorldManager::SetResource(x, y, 112, 1)`).
pub const RESOURCE_TREE: u8 = 0x70;

pub fn is_tree(kind: u16) -> bool {
    (1..=22).contains(&kind)
}

fn data(world: &mut World, id: u16) -> Option<&mut DecoData> {
    match &mut world.entities.by_index_mut(id as u32)?.data {
        EntityData::Deco(d) => Some(d),
        _ => None,
    }
}

/// Set stage `stage` and its animation (`job = first + stage`, frame 0).
fn set_stage(world: &mut World, id: u16, stage: u8) -> u8 {
    let Some(kind) = world.entities.by_index(id as u32).map(|e| e.kind) else { return 0 };
    let frames = world.rules.object_frames(kind, stage as usize);
    if let Some(d) = data(world, id) {
        d.stage = stage;
        d.frame = 0;
        d.cycle_frames = frames;
    }
    frames
}

/// `CTree::CTree(const CTree& static, id, x, y)` (0x593970) via `CSearchRoutines::SearchTree`: a dynamic instance of the static tree `static_id` at (x, y), grown (stage 3),
/// taking over the static instance's frame and cycle. It replaces the static id in the node's
/// deco slot and is not registered for logic updates. Returns the new entity, if a slot is free.
pub fn make_dynamic(world: &mut World, static_id: u16, x: i32, y: i32) -> Option<u16> {
    let e = world.entities.by_index(static_id as u32)?;
    let EntityData::Deco(d) = &e.data else { return None };
    let (kind, frame, cycle) = (e.kind, d.frame, d.cycle_frames);
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let data = DecoData { static_instance: false, frame, stage: 3, cycle_frames: cycle, amount: 0, counter: 0, ..DecoData::default() };
    let mut t = Entity::new(ObjType::Deco, kind, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    t.flags |= flag::VISIBLE;
    world.entities.insert(slot, t);
    world.layers.deco_object[node] = slot;
    Some(slot)
}

/// `CTree::LogicUpdate` (0x593c50). Stages 3 and 5..8 never re-register.
/// The fall's end (4 → 5) plays sound 0x60 where the node is visible (client-side, not here).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (kind, x, y) = (e.kind, e.x(), e.y());
    let EntityData::Deco(d) = &e.data else { return };
    let (stage, counter, frame) = (d.stage, d.counter, d.frame);
    match stage {
        0..=2 => {
            if counter + 1 < 40 {
                if let Some(d) = data(world, id) {
                    d.counter += 1;
                }
                world.register_for_logic_update(id, UPDATE);
                return;
            }
            if let Some(d) = data(world, id) {
                d.counter = 0;
            }
            // Only the job changes here; frame and cycle stay those of the planted stage.
            if let Some(d) = data(world, id) {
                d.stage += 1;
            }
            if stage + 1 != 3 {
                world.register_for_logic_update(id, UPDATE);
                return;
            }
            if let Some(n) = world.grid.checked_index(x, y) {
                world.layers.set_resource(n, RESOURCE_TREE, 1);
            }
            change_to_static_instance(world, x, y, kind, 0);
        }
        4 => {
            set_stage(world, id, 5);
            if let Some(d) = data(world, id) {
                d.counter = 0;
            }
            // `IsPositionVisible`, then `CalcPanningAndVolume` + `PlaySoundFX(96)`
            // (`SOUND_WORK_TREEFALL`): local fog and screen, so the client plays it.
            world.events.push(crate::event::SimEvent::Sound { sound: 96, x, y });
        }
        9 => {
            if counter + 1 >= 2 {
                if let Some(d) = data(world, id) {
                    d.counter = 0;
                    d.stage = 10;
                    d.frame = 0;
                    d.cycle_frames = 1;
                }
            } else if let Some(d) = data(world, id) {
                d.counter += 1;
            }
            world.register_for_logic_update(id, UPDATE);
        }
        10 => {
            if counter + 1 < 2 {
                if let Some(d) = data(world, id) {
                    d.counter += 1;
                }
                world.register_for_logic_update(id, UPDATE);
                return;
            }
            let last = world.rules.object_frames(kind, 10) as i32 - 1;
            if let Some(d) = data(world, id) {
                d.counter = 0;
                d.frame = frame.wrapping_add(1);
            }
            if (frame as i32) < last {
                world.register_for_logic_update(id, UPDATE);
            } else {
                delete(world, id);
            }
        }
        _ => {}
    }
}

/// `CTree::Decrease(n)` (0x593e50, vtbl+32), also `Take` (0x593ef0, vtbl+36 → `Decrease(1)`): one
/// step of felling.
/// From the grown stage it clears the node's resource; the fall (4) re-registers after its
/// animation, the last chop (9) after 31 ticks.
pub fn decrease(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let EntityData::Deco(d) = &e.data else { return };
    // "!IsStaticInstance()" asserted.
    let (stage, x, y) = (d.stage, e.x(), e.y());
    if stage == 3
        && let Some(n) = world.grid.checked_index(x, y)
    {
        world.layers.set_resource(n, 0, 0);
    }
    let frames = set_stage(world, id, stage + 1);
    match stage + 1 {
        4 => world.register_for_logic_update(id, frames as i32 - 1),
        9 => world.register_for_logic_update(id, UPDATE),
        _ => {}
    }
}

/// `CTree::CTree(x, y, type, id, 0)` (0x593880) via `AddDecoObjWithoutFlags(x, y, type, 0, 0)`:
/// a planted tree, a dynamic instance at stage 0 (growing), first update in 31 ticks.
pub fn plant(world: &mut World, x: i32, y: i32, kind: u16) -> Option<u16> {
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let cycle = world.rules.object_frames(kind, 0);
    let data = DecoData { static_instance: false, frame: 0, stage: 0, cycle_frames: cycle, amount: 0, counter: 0, ..DecoData::default() };
    let mut t = Entity::new(ObjType::Deco, kind, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    t.flags |= flag::VISIBLE;
    world.entities.insert(slot, t);
    world.layers.deco_object[node] = slot;
    world.register_for_logic_update(slot, UPDATE);
    Some(slot)
}
