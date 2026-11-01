//! `CFreeWorkerRole` (MapObjects/Settler/FreeWorkerRole.cpp, role id 5): the workers who work
//! outside their building: woodcutter, stonecutter, forester, fisher, hunter, … The building
//! (`buildings::gather`) finds the work and sends it as an event:
//!
//! * event 1: the worker's building (home), walk in (`TakeAnimList(0)`, the check-in list);
//! * event 5: work on entity `data1` (a tree, a stone) from position `data2`
//!   (`TakeAnimList(1)`, e.g. JOB_WOODCUTTER_WORK: walk there, chop, pick up the log, carry it
//!   to the building's pile, check in);
//! * event 6: plant object `data1` at `data2` (`TakeAnimList(2)`).
//!
//! The virtual tasks move the worker inside its node by pixel offsets (+36/+40, here fixed
//! point, see [`super::state::OFFSET_ONE`]).

use super::core::{self, job, job_mut, register};
use super::state::{EntityEvent, FreeWorkerData, OFFSET_ONE};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::flag;
use crate::grid::Y16X16;

/// Node flag 0x20 (FLAG_WORK): the building marked the node as some worker's target.
const NODE_FLAG_WORK: u8 = 0x20;
/// `WalkToXY` flag 0x1000: arrive next to the target (piles).
const WALK_NEAR: i32 = 0x1000;
/// Fisher (9) and water worker (10) keep their target entity on event 5 (`SetEntity` skipped;
/// release 0x578bd0).
const FISHER: u16 = 9;
const WATERWORKER: u16 = 10;

fn fw(world: &mut World, i: u16) -> Option<&mut FreeWorkerData> {
    job_mut(world, i).map(|j| &mut j.free_worker)
}

/// `CFreeWorkerRole::CFreeWorkerRole` (release 0x577fa0), `InitWalking` (release 0x578190: `CWalking::Create(1,
/// owner)` + vtbl+8(-1, 0)) and `Init` (release 0x578140).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, 1);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    super::hooks::war_map_add(world, i);
    if let Some(j) = job_mut(world, i) {
        j.free_worker = FreeWorkerData::default();
        j.offset = (0, 0);
        j.step = (0, 0);
    }
}

/// The node the worker's search offset points back to: `(x − dx, y − dy)` of search `k`.
fn search_node(world: &World, i: u16, k: usize) -> (i32, i32) {
    let Some(e) = world.entities.by_index(i as u32) else { return (0, 0) };
    let t = world.rules.jobs.search(e.race, e.kind, k).unwrap_or(0);
    let (dx, dy) = crate::buildings::search::offset(t);
    (e.x() - dx, e.y() - dy)
}

/// `CFreeWorkerRole::CheckResource(settler, k)` (release 0x578eb0): the deco object at the node search
/// `k` works on.
fn check_resource(world: &World, i: u16, k: usize) -> u16 {
    let (x, y) = search_node(world, i, k);
    world.grid.checked_index(x, y).map_or(0, |n| world.layers.deco_object[n])
}

fn clear_work_flag(world: &mut World, pos: Y16X16) {
    if pos.0 != 0
        && let Some(n) = world.grid.checked_index(pos.x(), pos.y())
    {
        world.layers.flags[n] &= !NODE_FLAG_WORK;
    }
}

/// `TakeAnimList(k)` with the destination `pos` (`ISettlerRole::NewDestination`).
fn go_list(world: &mut World, i: u16, pos: Y16X16, k: usize) {
    core::new_destination(world, i, pos);
    core::take_anim_list(world, i, k);
}

/// Target of a `GO_VIRTUAL` to cell (x, y): the original computes, in pixels,
/// `(255x − 127.5y − (ox + 127.5), 255y − (oy + 255))` from the current offset (ox, oy).
fn virtual_target(x: i16, y: i16) -> (i64, i64) {
    let one = OFFSET_ONE as i64;
    let (x, y) = (x as i64, y as i64);
    ((510 * x - 255 * y - 255) * one / 2, (255 * y - 255) * one)
}

/// `CFreeWorkerRole::GetNextJob` (release 0x578ba0): next task; at the end the worker just waits
/// (task WAIT, not registered: the next event wakes it).
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

/// 0x578680's work case for the fisher (type 9 on job part 0x52): over spiral entries 0..=18
/// around him, the first node holding fish (resource type 0 with an amount) gives his direction
/// (the entry's direction byte); without one, the last water node (ground below 9 or `0x6?`)
/// does, else 0.
fn fisher_faces_water(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (kind, pos) = (e.kind, e.pos);
    if kind != 9 || job(world, i).is_none_or(|j| j.job_part != 0x52) {
        return;
    }
    let mut dir = 0;
    for k in 0..=18 {
        let (dx, dy, _, d) = crate::spiral::entry(k);
        // The release indexes the node unchecked; off the map nothing is read here [inf].
        let Some(n) = world.grid.checked_index(pos.x() + dx as i32, pos.y() + dy as i32) else { continue };
        let g = world.layers.ground[n];
        if g < 9 || g & 0xf0 == 0x60 {
            dir = d;
        }
        let r = world.layers.resource[n];
        if r & 0xf0 == 0 && r & 0xf != 0 {
            dir = d;
            break;
        }
    }
    if let Some(s) = core::settler_mut(world, i) {
        s.direction = dir;
    }
}

/// `CFreeWorkerRole::TakeJob` (release 0x578680).
pub fn take_job(world: &mut World, i: u16) {
    let tick = world.tick;
    if let Some(j) = job_mut(world, i) {
        j.last_update_tick = tick;
    }
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, cycle, home, destination, good) = (j.counter, j.cycle_frames, j.home, j.destination, j.free_worker.good);
    match t.task {
        task_type::GO_VIRTUAL => {
            if !core::check_home(world, i) {
                return;
            }
            let Some(j) = job_mut(world, i) else { return };
            j.displacement_costs = 10;
            let (tx, ty) = virtual_target(j.x, j.y);
            let (dx, dy) = (tx - j.offset.0 as i64, ty - j.offset.1 as i64);
            // `(int)(|v42| / 255 + 0.5)` and `(int)(|v37 − v42 / 2| / 255 + 0.5)`: the offsets sit
            // on half pixels of 127.5, so these are exact halves rounded up.
            let d = 255 * OFFSET_ONE as i64;
            let a = (2 * dy.abs() + d) / (2 * d);
            let b = ((2 * dx - dy).abs() + d) / (2 * d);
            let n = 9 * a.max(b) as i32; // "iJobCounter < 128" asserted
            j.counter = n as i8;
            if n <= 0 {
                j.step = (0, 0);
                register(world, i, 1);
            } else {
                j.step = ((dx / n as i64) as i32, (dy / n as i64) as i32);
                register(world, i, 9);
            }
        }
        // WORK_VIRTUAL, WAIT_VIRTUAL, RESOURCE_GATHERING(_VIRTUAL), WORK
        1 | 2 | 13 | 16 | 30 => {
            if !core::check_home(world, i) {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            // "Invalid cycle frames" is traced for <= 1.
            register(world, i, if cycle <= 1 { 1 } else { cycle as i32 - 1 });
            fisher_faces_water(world, i);
        }
        // PUT_GOOD(_VIRTUAL)
        4 | 21 => {
            if !core::check_home(world, i) {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
                j.frame = cycle.wrapping_sub(1);
            }
            register(world, i, counter as i32 / 2);
        }
        // GET_GOOD(_VIRTUAL)
        5 | 22 => {
            if !core::check_home(world, i) {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter as i32 / 2 - 1);
        }
        task_type::GO_TO_TARGET => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            start_walk(world, i, destination, 0);
        }
        // GO_HOME
        14 => {
            if !core::check_home(world, i) {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.frame = 0;
            }
            if let Some(door) = core::building_door(world, home) {
                start_walk(world, i, door, 0);
            }
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            register(world, i, 1);
        }
        // GO_TO_DESTINATION_PILE: the building's pile for the good carried.
        19 => {
            if !core::check_home(world, i) {
                return;
            }
            let pile = crate::buildings::pile_with_good(world, home, good);
            let pos = core::entity_pos(world, pile).unwrap_or(Y16X16(0));
            start_walk(world, i, pos, WALK_NEAR);
        }
        task_type::CHECKIN => {
            if !core::check_home(world, i) {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.offset = (0, 0);
                j.destination = Y16X16(0);
                j.free_worker.on_entity = false;
                j.displacement_costs = 10;
            }
            super::hooks::building_settler_enter(world, home, i);
        }
        // PLANT
        25 => {
            if !core::check_home(world, i) {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, cycle as i32 - 1); // "cycle frames > 1" asserted
        }
        // WORK_ON_ENTITY(_VIRTUAL): the stored entity must still be the object at the node.
        28 | 29 => {
            if !core::check_home(world, i) {
                return;
            }
            let entity = job(world, i).map_or(0, |j| j.entity);
            if check_resource(world, i, 0) == entity {
                if let Some(j) = job_mut(world, i) {
                    j.free_worker.on_entity = true;
                    j.displacement_costs = 10;
                }
                register(world, i, cycle as i32 - 1);
            } else {
                // "Working entity differs from stored entity! Should not happen!"
                if let Some(door) = core::building_door(world, home) {
                    go_list(world, i, door, 0);
                }
            }
        }
        _ => {} // "FreeWorkerJob - TakeJob unknown task"
    }
}

/// `CFreeWorkerRole::LogicUpdateJob` (release 0x5781f0).
pub fn logic_update_job(world: &mut World, i: u16) {
    if !core::check_home(world, i) {
        return;
    }
    core::update(world, i);
    let Some(j) = job(world, i) else { return };
    let (task, counter, cycle) = (j.task, j.counter as i32, j.cycle_frames as i32);
    let (home, destination, entity, good) = (j.home, j.destination, j.entity, j.free_worker.good);
    let spend = |world: &mut World, v: i32| {
        if let Some(j) = job_mut(world, i) {
            j.counter = (counter - v) as i8;
        }
    };
    match task {
        task_type::GO_VIRTUAL => {
            spend(world, 9);
            if counter - 9 > 0 {
                register(world, i, 9);
            } else {
                // The float offset ends on the target up to rounding; snap it there.
                if let Some(j) = job_mut(world, i) {
                    let (tx, ty) = virtual_target(j.x, j.y);
                    j.offset = (tx as i32, ty as i32);
                }
                get_next_job(world, i);
            }
        }
        1 | 16 => {
            let v = counter.min(cycle);
            spend(world, v);
            if counter - v <= 0 {
                clear_work_flag(world, destination);
                get_next_job(world, i);
            } else {
                register(world, i, v);
            }
        }
        2 => get_next_job(world, i),
        4 | 21 => {
            let half = cycle / 2;
            spend(world, half);
            if counter - half < half {
                get_next_job(world, i);
            } else {
                let pile = crate::buildings::pile_with_good(world, home, good);
                crate::pile::increase_unforeseen(world, pile, 1);
                if let Some(f) = fw(world, i) {
                    f.good = 0;
                }
                register(world, i, half - 1);
            }
        }
        5 | 22 => {
            let half = cycle / 2;
            spend(world, half);
            if counter - half < half {
                get_next_job(world, i);
            } else {
                let g = crate::deco::good_type(world, entity);
                // "m_uGood > 0 && m_uGood < GOOD_MAX" asserted.
                let g = if g >= 43 { 0 } else { g };
                if let Some(f) = fw(world, i) {
                    f.good = g;
                }
                if g != 0 {
                    crate::deco::decrease(world, entity);
                }
                register(world, i, half);
            }
        }
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        // RESOURCE_GATHERING(_VIRTUAL): take the good off the entity.
        13 | 30 => {
            let v = counter.min(cycle);
            spend(world, v);
            if counter - v > 0 {
                register(world, i, v);
                return;
            }
            // "m_uEntityId" asserted.
            let g = if world.entities.by_index(entity as u32).is_some() { crate::deco::good_type(world, entity) } else { 0 };
            let g = if g >= 43 { 0 } else { g };
            if let Some(j) = job_mut(world, i) {
                j.free_worker.good = g;
                j.entity = 0;
                j.free_worker.on_entity = false;
            }
            clear_work_flag(world, destination);
            if g != 0 {
                crate::deco::take(world, entity);
                get_next_job(world, i);
            } else {
                // "Could not get good!": back home (destination = start position).
                if let Some(j) = job_mut(world, i) {
                    j.destination = j.start;
                }
                core::take_anim_list(world, i, 0);
            }
        }
        14 => {}
        // PLANT: the object goes in at the search-1 node.
        25 => {
            let v = counter.min(cycle);
            spend(world, v);
            if counter - v > 0 {
                register(world, i, v);
                return;
            }
            let (x, y) = search_node(world, i, 1);
            let kind = job(world, i).map_or(0, |j| j.free_worker.plant);
            crate::deco::plant(world, x, y, kind);
            if let Some(f) = fw(world, i) {
                f.plant = 0;
            }
            get_next_job(world, i);
        }
        // WORK_ON_ENTITY(_VIRTUAL): one step of work on the entity (a tree: one chop).
        28 | 29 => {
            let v = counter.min(cycle);
            spend(world, v);
            if counter - v > 0 {
                register(world, i, v);
                return;
            }
            if world.entities.by_index(entity as u32).is_some() {
                crate::deco::decrease(world, entity);
            }
            // "Invalid target entity" is traced otherwise.
            clear_work_flag(world, destination);
            get_next_job(world, i);
        }
        _ => {} // "unknown job"
    }
}

/// `CFreeWorkerRole::UpdateJob` (release 0x578560, vtbl+32): the virtual walk's frames and offset, the
/// put / get animations.
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    let (elapsed, cycle) = (j.elapsed as u32, j.cycle_frames as u32);
    match j.task {
        task_type::GO_VIRTUAL => {
            if cycle == 0 {
                j.frame = 0;
            } else {
                let f = ((elapsed + j.frame as u32) % cycle) as u8;
                j.frame = if f != 0 || cycle <= 1 { f } else { 1 };
            }
            for _ in 0..elapsed {
                j.offset.0 += j.step.0;
                j.offset.1 += j.step.1;
            }
        }
        4 | 21 => {
            let f = j.frame as u32;
            j.frame = if f > elapsed { (f - elapsed) as u8 } else { 0 };
        }
        5 | 22 => {
            let f = elapsed + j.frame as u32;
            j.frame = if f < cycle { f as u8 } else { cycle.saturating_sub(1) as u8 };
        }
        _ => {}
    }
}

/// `CFreeWorkerRole::SetFree(settler, eco_sector)` (release 0x578d90, vtbl+64): an unfinished target
/// goes back to a static instance, a planned plant loses its flags, a carried good is dropped.
pub fn set_free(world: &mut World, i: u16, eco_sector: i32) -> bool {
    let Some(j) = job(world, i) else { return false };
    let (on_entity, entity, plant, good, destination) = (j.free_worker.on_entity, j.entity, j.free_worker.plant, j.free_worker.good, j.destination);
    if !on_entity {
        if entity != 0 {
            crate::deco::release(world, entity);
            if let Some(j) = job_mut(world, i) {
                j.entity = 0;
            }
        }
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags |= flag::VISIBLE;
        }
    }
    if plant != 0 {
        let Some(e) = world.entities.by_index(i as u32) else { return false };
        let t = world.rules.jobs.search(e.race, e.kind, 1).unwrap_or(0);
        let (dx, dy) = crate::buildings::search::offset(t);
        crate::deco::clear_flags_for_object(world, destination.x() - dx, destination.y() - dy, plant);
        if let Some(f) = fw(world, i) {
            f.plant = 0;
        }
    }
    if good != 0 {
        if good < 43 {
            let Some(e) = world.entities.by_index(i as u32) else { return false };
            let (x, y) = (e.x(), e.y());
            crate::pile::search_space_for_goods(world, x, y, good, 1);
        }
        if let Some(f) = fw(world, i) {
            f.good = 0;
        }
    }
    core::set_free_base(world, i, eco_sector)
}

/// `CFreeWorkerRole::ConvertEventIntoGoal` (release 0x578bd0).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        1 => {
            let building = ev.data[0] as u16;
            if let Some(j) = job_mut(world, i) {
                j.home = building;
            }
            if core::check_home(world, i)
                && let Some(door) = core::building_door(world, building)
            {
                go_list(world, i, door, 0);
            }
        }
        5 => {
            if !core::check_home(world, i) {
                return;
            }
            let kind = world.entities.by_index(i as u32).map_or(0, |e| e.kind);
            core::new_destination(world, i, Y16X16(ev.data[2] as u32));
            if kind != WATERWORKER && kind != FISHER
                && let Some(j) = job_mut(world, i)
            {
                j.entity = ev.data[1] as u16; // SetEntity (vtbl+52)
            }
            core::take_anim_list(world, i, 1);
        }
        6 => {
            if !core::check_home(world, i) {
                return;
            }
            if let Some(f) = fw(world, i) {
                f.plant = ev.data[1] as u16;
            }
            go_list(world, i, Y16X16(ev.data[2] as u32), 2);
        }
        9 => {
            if job(world, i).is_some_and(|j| j.task == task_type::WAIT) {
                super::roles::set_free(world, i, ev.data[2]);
            }
            let registered = world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0);
            if !registered {
                register(world, i, 1);
            }
        }
        _ => {
            let registered = world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0);
            if !registered {
                register(world, i, 1); // "unknown event"
            }
        }
    }
}
