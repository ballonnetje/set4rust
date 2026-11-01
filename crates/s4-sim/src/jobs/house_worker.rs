//! `CHouseWorkerRole` (role id 4, release vtable 0x105b8b0): workers whose job runs in and
//! around their building (sawmill, smith, healer, ...). Checked against the release: TakeJob
//! 0x57c960 (vtbl+40), LogicUpdateJob 0x57c610 (+28), UpdateJob 0x57c880 (+32), GetNextJob
//! 0x57cca0 (+36), ConvertEventIntoGoal 0x57ccd0 (+56); evidence in
//! `tools/re/decompiled/script/hw2/`.
//!
//! After CHECKIN the building's `SettlerEnter` (workup 0x55a050, production 0x54ca90) gives the
//! settler MAGIC_INVISIBLE and puts its offset on the door's cell, and the work lists move it
//! with `GO_VIRTUAL` between cells of the building (the piles, the saw, the anvil: many of them
//! outside the walls). The offsets are **pixels at zoom 1 from the building's anchor**: the
//! settler is not drawn by `CSettler::GetGfxInfo` but by the building's `FillGfxInfo` (0x55a3a0 /
//! 0x54d0e0), while its task is `visible` (see `s4_viewer::population::house_worker`).

use super::core::{self, job, job_mut, register};
use super::hooks;
use super::state::{EntityEvent, OFFSET_ONE};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::flag;
use crate::grid::Y16X16;

/// Pixel offset of cell (x, y) from the building's anchor: `(24x − 12y, 12y)` (`TakeJob`
/// GO_VIRTUAL, constants 24.0 at 0x1070f24 and 12.0 at 0x1070ef8: one node column / row at zoom 1).
fn cell_offset(x: i16, y: i16) -> (i32, i32) {
    let (x, y) = (x as i32, y as i32);
    ((24 * x - 12 * y) * OFFSET_ONE, 12 * y * OFFSET_ONE)
}

/// `CHouseWorkerRole::InitWalking` (vtbl+20, 0x57c5b0: a `CWalkingWorker` of type 1, then
/// vtbl+8(-1, 0)) and `Init` (vtbl+44, 0x57c570: the war-map calls 0x5bb440 / 0x5bb330 (unverified
/// label `CWarMap::AddEntity`), offset = 0).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, 1);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    hooks::war_map_add(world, i);
    if let Some(j) = job_mut(world, i) {
        j.offset = (0, 0);
    }
}

/// `CHouseWorkerRole::GetNextJob` (0x57cca0): next task, or WAIT at the end of the list (no
/// re-registration: the building hands out the next list).
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if !core::is_end_iter(world, i) {
        take_job(world, i);
    } else if let Some(j) = job_mut(world, i) {
        j.task = task_type::WAIT;
    }
}

/// `CHouseWorkerRole::TakeJob` (0x57c960).
pub fn take_job(world: &mut World, i: u16) {
    let tick = world.tick;
    if let Some(j) = job_mut(world, i) {
        j.last_update_tick = tick;
    }
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, cycle) = (j.counter, j.cycle_frames);
    match t.task {
        task_type::GO_VIRTUAL => {
            if !core::check_home(world, i) {
                return;
            }
            let Some(j) = job_mut(world, i) else { return };
            j.displacement_costs = 10;
            let (tx, ty) = cell_offset(j.x, j.y);
            let dx = tx - j.offset.0; // v16
            let dy = ty - j.offset.1; // v19
            // `(int)(|dy| · 0.083333336f + 0.5f)` and `(int)((double)(|dx − dy| · 0.041666668f)
            // + 0.5)` (0x57ca10..0x57ca64): the node distance in rows and in columns minus rows;
            // the constants are slightly above 1/12 and 1/24, so exact halves round up. Offsets here sit exactly on cell targets, so the integer
            // forms below give the same counts; the original's float drift could flip a tie
            // (not reproduced).
            let a = dy.unsigned_abs() as i64;
            let b = (dx - dy).unsigned_abs() as i64;
            let one = OFFSET_ONE as i64;
            let v11 = (2 * a + 12 * one) / (24 * one);
            let v10 = (2 * b + 24 * one) / (48 * one);
            let n = 9 * v10.max(v11) as i32; // "iJobCounter < 128" asserted
            j.counter = n as i8;
            if n <= 0 {
                j.step = (0, 0);
                register(world, i, 1);
            } else {
                j.step = (dx / n, dy / n);
                register(world, i, 9);
            }
        }
        task_type::WORK_VIRTUAL | task_type::PRODUCE_VIRTUAL => {
            if core::check_home(world, i) {
                register(world, i, cycle as i32 - 1);
            }
        }
        task_type::WAIT_VIRTUAL => {
            if core::check_home(world, i) {
                register(world, i, (counter as i32).min(31));
            }
        }
        task_type::PUT_GOOD_VIRTUAL => {
            if core::check_home(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.frame = cycle.wrapping_sub(1);
                }
                register(world, i, counter as i32 / 2);
            }
        }
        task_type::GET_GOOD_VIRTUAL => {
            if core::check_home(world, i) {
                register(world, i, counter as i32 / 2);
            }
        }
        task_type::GO_TO_TARGET => {
            let Some(j) = job_mut(world, i) else { return };
            j.frame = 1;
            let d = Y16X16::pack(j.x as i32 + j.destination.x(), j.y as i32 + j.destination.y());
            j.destination = d;
            ActivePathing::walk_to_xy(world, i, d, 0);
            if let Some(j) = job_mut(world, i) {
                j.task = task_type::GO;
                j.displacement_costs = 5;
            }
            core::go(world, i);
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            let r = world.rng.rand();
            register(world, i, (r % 4 + 1) as i32);
        }
        task_type::CHECKIN if core::check_home(world, i) => {
            let home = job(world, i).map_or(0, |j| j.home);
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
                j.offset = (0, 0);
            }
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags &= !flag::VISIBLE;
            }
            hooks::building_settler_enter(world, home, i);
        }
        _ => {} // "HouseWorkerRole nr %u - TakeJob unknown task" (debug trace only)
    }
}

/// `CHouseWorkerRole::LogicUpdateJob` (0x57c610, vtbl+28).
pub fn logic_update_job(world: &mut World, i: u16) {
    if !core::check_home(world, i) {
        return;
    }
    core::update(world, i);
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let kind = e.kind;
    let Some(j) = job_mut(world, i) else { return };
    let cycle = j.cycle_frames as i32;
    match j.task {
        task_type::GO_VIRTUAL => {
            j.counter = j.counter.wrapping_sub(9);
            if j.counter <= 0 {
                // The float offset ends on the target up to rounding; snap it there.
                j.offset = cell_offset(j.x, j.y);
                get_next_job(world, i);
            } else {
                register(world, i, 9);
            }
        }
        task_type::WORK_VIRTUAL => {
            let v = (j.counter as i32).min(cycle);
            j.counter = (j.counter as i32 - v) as i8;
            if j.counter <= 0 {
                if kind == 20 {
                    // Healer: tell the settler being healed ("m_uEntityId" asserted).
                    let served = j.entity;
                    hooks::served_settler_done(world, served);
                }
                get_next_job(world, i);
            } else {
                register(world, i, v);
            }
        }
        task_type::WAIT_VIRTUAL => {
            j.counter = j.counter.wrapping_sub(31);
            if j.counter <= 0 {
                get_next_job(world, i);
            } else {
                let c = j.counter as i32;
                register(world, i, c.min(31));
            }
        }
        task_type::PRODUCE_VIRTUAL => {
            let v = (j.counter as i32).min(cycle);
            j.counter = (j.counter as i32 - v) as i8;
            if j.counter <= 0 {
                let (vehicle, good) = (j.entity, j.good);
                // An entity id in range (the original compares it with the highest id, else it
                // skips the rest): the vehicle, if it exists, gets the material (good 7 / 20);
                // the good is dropped either way.
                if vehicle != 0 && world.entities.by_index(vehicle as u32).is_some() {
                    hooks::vehicle_add_building_material(world, vehicle, good);
                }
                if let Some(j) = job_mut(world, i) {
                    j.good = 0;
                }
                get_next_job(world, i);
            } else {
                let frames = core::actual_task(world, i).map_or(0, |t| t.frames);
                if let Some(j) = job_mut(world, i) {
                    j.cycle_frames = frames;
                }
                register(world, i, v);
            }
        }
        task_type::PUT_GOOD_VIRTUAL => {
            let v = cycle / 2;
            j.counter = (j.counter as i32 - v) as i8;
            if (j.counter as i32) < v {
                get_next_job(world, i);
            } else {
                let home = j.home;
                if kind == 15 {
                    let g = hooks::production_product_type(world, home);
                    if let Some(j) = job_mut(world, i) {
                        j.good = g;
                    }
                }
                let good = job(world, i).map_or(0, |j| j.good);
                hooks::building_pile_unforeseen(world, home, good, 1);
                if let Some(j) = job_mut(world, i) {
                    j.good = 0;
                }
                register(world, i, v - 1);
            }
        }
        task_type::GET_GOOD_VIRTUAL => {
            let v = cycle / 2;
            j.counter = (j.counter as i32 - v) as i8;
            if (j.counter as i32) < v {
                get_next_job(world, i);
            } else {
                let (home, good) = (j.home, j.good);
                hooks::building_pile_unforeseen(world, home, good, -1);
                register(world, i, v);
            }
        }
        task_type::GO => {
            j.distance = 0;
            core::go(world, i);
        }
        _ => {} // "HouseWorkerRole nr %u - LogicUpdate unknown task"
    }
}

/// `CHouseWorkerRole::UpdateJob` (0x57c880, vtbl+32): animation of the virtual tasks.
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    let (elapsed, cycle) = (j.elapsed as u32, j.cycle_frames as u32);
    match j.task {
        task_type::GO_VIRTUAL => {
            if cycle == 0 {
                return;
            }
            j.frame = ((elapsed + j.frame as u32) % cycle) as u8;
            if j.frame == 0 && cycle > 1 {
                j.frame = 1;
            }
            for _ in 0..elapsed {
                j.offset.0 += j.step.0;
                j.offset.1 += j.step.1;
            }
        }
        task_type::PUT_GOOD_VIRTUAL => {
            let f = j.frame as u32;
            j.frame = if f > elapsed { (f - elapsed) as u8 } else { 0 };
        }
        task_type::GET_GOOD_VIRTUAL => {
            let f = elapsed + j.frame as u32;
            j.frame = if f < cycle {
                f as u8
            } else if cycle != 0 {
                (cycle - 1) as u8
            } else {
                0
            };
        }
        _ => {}
    }
}

/// `CHouseWorkerRole::ConvertEventIntoGoal` (0x57ccd0). Events from the building:
/// 1 = come home to building `data[0]` (destination its door, check-in list = anim list 0);
/// 3 / 4 = run list `data[1]` on entity `data[2]` (3 sets the entity after the list starts,
/// 4 before; 3 asserts MAGIC_INVISIBLE); 9 = set free in eco-sector `data[2]`.
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        1 => {
            if core::check_home(world, i) {
                let b = ev.data[0] as u16;
                if let Some(j) = job_mut(world, i) {
                    j.home = b;
                }
                if let Some(door) = core::building_door(world, b) {
                    core::new_destination(world, i, door);
                }
                let Some(e) = world.entities.by_index(i as u32) else { return };
                let (race, kind) = (e.race, e.kind);
                let list = world.rules.jobs.anim_list(race, kind, 0).unwrap_or(0);
                core::new_shared_list(world, i, list);
            }
        }
        3 => {
            if core::check_home(world, i) {
                core::new_shared_list(world, i, ev.data[1] as u16);
                if let Some(j) = job_mut(world, i) {
                    j.entity = ev.data[2] as u16;
                }
            }
        }
        4 => {
            if core::check_home(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.entity = ev.data[2] as u16;
                }
                core::new_shared_list(world, i, ev.data[1] as u16);
            }
        }
        9 => {
            super::roles::set_free(world, i, ev.data[2]);
        }
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & crate::entity::flag::REGISTERED == 0) {
                register(world, i, 1); // "ConvertEventIntoGoal HouseWorker - unknown event %u"
            }
        }
    }
}
