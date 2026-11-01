//! `CShadowHerb` (object type 216, release vtable 0x1059fc0): what the dark gardener plants. It
//! grows, darkens the land around it twice and dies. Evidence:
//! `tools/re/decompiled/script/dark/shadowherb.c`, `deco_add.c`.
//!
//! The constructor (0x5667a0) gives it job `first`, stage 0 and registers it after that job's
//! frames. `LogicUpdate` (0x566b00) by stage (+0x45):
//! * 0: stage 1, job `first + 1`, frame 0; next update in 1;
//! * 1: a frame per tick; after the last, stage 2, the land around darkens
//!   (`CDarkTribe` vtbl+0xc with the ring, [`crate::dark::darken_around`]), back one frame, 31
//!   ticks;
//! * 2: stage 3, job `first + 2`, frame 0, the land around darkens again; next update after the
//!   job's frames;
//! * 3: deleted (`CDecoObjMgr::Delete` 0x563410).
//!
//! Drawing (0x566a40) advances the frame by the ticks since it was last drawn, except in stages 1
//! and 2 (where the logic sets it).

use crate::entity::{DecoData, EntityData};
use crate::world::World;

pub const SHADOW_HERB: u16 = 216;

pub fn is_shadow_herb(kind: u16) -> bool {
    kind == SHADOW_HERB
}

fn data(world: &mut World, id: u16) -> Option<&mut DecoData> {
    match &mut world.entities.by_index_mut(id as u32)?.data {
        EntityData::Deco(d) => Some(d),
        _ => None,
    }
}

/// Job `first + job` and its frame count, frame 0.
fn set_job(world: &mut World, id: u16, stage: u8, job: u8) -> u8 {
    let frames = world.rules.object_frames(SHADOW_HERB, job as usize);
    if let Some(d) = data(world, id) {
        d.stage = stage;
        d.job = job;
        d.frame = 0;
        d.cycle_frames = frames;
    }
    frames
}

/// `CShadowHerb::LogicUpdate` (0x566b00).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y) = (e.x(), e.y());
    let EntityData::Deco(d) = &e.data else { return };
    match d.stage {
        0 => {
            set_job(world, id, 1, 1);
            world.register_for_logic_update(id, 1);
        }
        1 => {
            let Some(d) = data(world, id) else { return };
            d.frame = d.frame.wrapping_add(1);
            if d.frame >= d.cycle_frames {
                d.stage = 2;
                d.frame = d.frame.wrapping_sub(1);
                crate::dark::darken_around(world, x, y, true);
                world.register_for_logic_update(id, 31);
            } else {
                world.register_for_logic_update(id, 1);
            }
        }
        2 => {
            let frames = set_job(world, id, 3, 2);
            crate::dark::darken_around(world, x, y, true);
            world.register_for_logic_update(id, frames as i32);
        }
        3 => super::delete(world, id),
        _ => {}
    }
}
