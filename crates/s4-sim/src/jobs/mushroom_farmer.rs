//! `CMushroomFarmerRole` (release vtable 0x105bbf8, role 15, `SETTLER_MUSHROOMFARMER` 53): the
//! Dark Tribe's mushroom farmer. Decompiles in `tools/re/decompiled/script/dark/mushfarmer.c`.
//!
//! The dark temple makes him when a mushroom farm needs one (`CDarkTribe` vtbl+0, see
//! [`crate::dark::order_mushroom_farmer`]) and sends him there (event 1, `JOB_MUSHROOMFARMER_
//! CHECKIN` 119). The farm sends him out to plant (event 6: the mushroom type and the node,
//! `JOB_MUSHROOMFARMER_PLANT` 190: GO_TO_POS, PLANT, GO_HOME to where he started, CHECKIN).
//! When the farm is done with him he is set free and sent to the temple (event 2,
//! `JOB_MUSHROOMFARMER_WORK` 191), which takes him in for good.
//!
//! The planted mushroom goes in at his node minus the offset of his second search
//! (`SEARCH_MUSHROOM_SEED_POS`), without flags (the farm set them when it sent him).

use super::core::{self, job, job_mut, register};
use super::state::EntityEvent;
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::grid::Y16X16;

const PLANT: u8 = 25;
const PUT_GOOD: u8 = 21;
const GET_GOOD: u8 = 22;
/// Task 33: GO_TO_POS with walk flag 0x2000.
const GO_TO_POS_2000: u8 = 33;

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

/// `CMushroomFarmerRole::InitWalking` (0x57db30: `CWalkingNormal` of the owner) and `Init`
/// (0x57db00: the war map).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    super::hooks::war_map_add(world, i);
    if let Some(j) = job_mut(world, i) {
        j.free_worker.plant = 0;
    }
}

/// `CMushroomFarmerRole::GetNextJob` (0x57deb0): the next task; at the end he waits (WAIT, not
/// registered).
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i) {
        if let Some(j) = job_mut(world, i) {
            j.task = task_type::WAIT;
        }
    } else {
        take_job(world, i);
    }
}

fn start_walk(world: &mut World, i: u16, to: Y16X16, flags: i32) {
    ActivePathing::walk_to_xy(world, i, to, flags);
    if let Some(j) = job_mut(world, i) {
        j.task = task_type::GO;
        j.displacement_costs = 5;
    }
    core::go(world, i);
}

/// `CMushroomFarmerRole::TakeJob` (0x57dcf0).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, cycle, destination, start, home) = (j.counter, j.cycle_frames, j.destination, j.start, j.home);
    match t.task {
        task_type::GO_TO_POS | task_type::GO_TO_TARGET | GO_TO_POS_2000 => {
            // The destination plus the task's cell offset, the halves added separately.
            let d = destination.0 as i32;
            let dest = ((t.y as i32 + (d >> 16)) << 16).wrapping_add(t.x as i32 + (d & 0xffff));
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
                j.destination = Y16X16(dest as u32);
            }
            start_walk(world, i, Y16X16(dest as u32), if t.task == GO_TO_POS_2000 { 0x2000 } else { 0 });
        }
        13 | task_type::WORK | PLANT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, cycle as i32);
        }
        // GO_HOME: back to where the walk out started.
        14 => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            start_walk(world, i, start, 0);
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            let r = world.rng.rand();
            register(world, i, (r & 0xf) as i32 + 1);
        }
        PUT_GOOD => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
                j.frame = cycle.wrapping_sub(1);
            }
            register(world, i, counter as i32 / 2);
        }
        GET_GOOD => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter as i32 / 2 - 1);
        }
        task_type::CHECKIN => {
            if home == 0 {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.offset = (0, 0);
                j.destination = Y16X16(0);
                j.displacement_costs = 10;
            }
            super::hooks::building_settler_enter(world, home, i);
        }
        _ => {}
    }
}

/// `CMushroomFarmerRole::LogicUpdateJob` (0x57db90).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    let (task, counter, cycle, entity) = (j.task, j.counter as i32, j.cycle_frames as i32, j.entity);
    match task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        task_type::WORK | PLANT => {
            let v = counter.min(cycle);
            if let Some(j) = job_mut(world, i) {
                j.counter = (counter - v) as i8;
            }
            if counter - v > 0 {
                register(world, i, v);
                return;
            }
            if task == PLANT {
                // `CDecoObjMgr` 0x563940 with grown = 0: a planted mushroom, no flags.
                let Some(e) = world.entities.by_index(i as u32) else { return };
                let t = world.rules.jobs.search(e.race, e.kind, 1).unwrap_or(0);
                let (dx, dy) = crate::buildings::search::offset(t);
                let (x, y) = (e.x() - dx, e.y() - dy);
                let kind = job(world, i).map_or(0, |j| j.free_worker.plant);
                crate::deco::plant(world, x, y, kind);
                if let Some(j) = job_mut(world, i) {
                    j.free_worker.plant = 0;
                }
            } else if entity != 0 && world.entities.by_index(entity as u32).is_some() {
                // vtbl+0x20 on the entity he works on (not set by anything ported).
                world.events.push(crate::event::SimEvent::Unported { what: "CMushroomFarmerRole WORK on an entity", entity: i });
            }
            get_next_job(world, i);
        }
        _ => {}
    }
}

/// `CMushroomFarmerRole::UpdateJob` (0x57dc90): task 13 loops its frames, PUT_GOOD plays
/// backwards, GET_GOOD forwards (wrapping to 0).
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    let elapsed = j.elapsed as u8;
    match j.task {
        13 if j.cycle_frames != 0 => j.frame = ((j.elapsed as u32 + j.frame as u32) % j.cycle_frames as u32) as u8,
        PUT_GOOD => j.frame = j.frame.wrapping_sub(elapsed),
        GET_GOOD => {
            j.frame = j.frame.wrapping_add(elapsed);
            if j.frame >= j.cycle_frames {
                j.frame = 0;
            }
        }
        _ => {}
    }
}

/// `CMushroomFarmerRole::SetFree` (0x57e040): off his home building's observer list
/// (`CBuilding::Detach`) and his queued events dropped. He is not offered anywhere: the caller
/// sends him to the temple.
pub fn set_free(world: &mut World, i: u16, _eco_sector: i32) -> bool {
    let home = job(world, i).map_or(0, |j| j.home);
    if home != 0 {
        crate::buildings::detach_settler(world, home, i);
    }
    if let Some(j) = job_mut(world, i) {
        j.events.clear();
    }
    false
}

/// `CMushroomFarmerRole::ConvertEventIntoGoal` (0x57dee0).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    let home = job(world, i).map_or(0, |j| j.home);
    let (event, index) = match ev.event {
        // To the farm (or to the temple: event 2, list 1 with data1, else 0) — without a home he
        // is let go as on event 9.
        1 | 2 if home != 0 => (ev.event, if ev.event == 2 && ev.data[1] != 0 { 1 } else { 0 }),
        1 | 2 | 9 => {
            super::roles::set_free(world, i, -1);
            crate::dark::send_to_temple(world, i, false);
            return;
        }
        6 => (6, 2),
        _ => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags |= core::FLAG_CONTINUE_AFTER_EVENT;
            }
            return;
        }
    };
    let here = pos(world, i);
    let dest = if event == 6 {
        if let Some(j) = job_mut(world, i) {
            j.free_worker.plant = ev.data[1] as u16;
        }
        Y16X16(ev.data[2] as u32)
    } else {
        core::building_door(world, ev.data[0] as u16).unwrap_or(Y16X16(0))
    };
    if let Some(j) = job_mut(world, i) {
        j.start = here;
        j.destination = dest;
    }
    core::take_anim_list(world, i, index);
}
