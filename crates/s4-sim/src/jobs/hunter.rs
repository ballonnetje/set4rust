//! `CHunterRole` (release vtable 0x105bb18, role id 8; created by `CreateSettlerRole` 0x583310
//! for `HUNTER_ROLE`, i.e. `SETTLER_HUNTER` 11). Decompiles in
//! `tools/re/decompiled/script/hunter/`.
//!
//! The hunter's hut (`buildings::gather`, building 7) finds the nearest huntable animal on its
//! eco-sector (`CScanner::FindNearestAnimal` 0x5b08b0) and sends event 5. The hunter then:
//!
//! 1. walks to it (`JOB_HUNTER_HUNT`, list 182: GO_TO_TARGET, WORK_ON_ENTITY). While walking it
//!    follows the animal (re-targets when it moved) and gives up when the animal is gone or
//!    on another eco-sector. Within distance 12 it stops and shoots.
//! 2. The shot (WORK_ON_ENTITY, one arrow effect) kills the animal at the end of the animation
//!    (`CLandAnimal` vtbl+0x20, [`crate::animals::start_dying`]).
//! 3. `JOB_HUNTER_HARVEST` (list 181): walk to the body, take the meat (the animal is deleted),
//!    carry it to the hut's pile, go home, check in.
//!
//! Giving up is `JOB_HUNTER_CHECKIN` (list 77) back to the door (0x57d440).
//!
//! The role's one member, +0x2c the good carried, lives in [`FreeWorkerData::good`]: the same
//! offset of the same `ISettlerRole` base.
//!
//! [`FreeWorkerData::good`]: super::state::FreeWorkerData::good

use super::core::{self, job, job_mut, register};
use super::state::EntityEvent;
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;

/// `WalkToXY` flag 0x1000: arrive next to the target.
const WALK_NEAR: i32 = 0x1000;
/// `JOB_HUNTER_CHECKIN` / `JOB_HUNTER_HARVEST`.
const LIST_CHECKIN: u16 = 77;
const LIST_HARVEST: u16 = 181;
/// The shooting distance: within it the hunter stops following (0x57d18f).
const SHOT_RANGE: i32 = 12;

fn good(world: &World, i: u16) -> u8 {
    job(world, i).map_or(0, |j| j.free_worker.good)
}

fn set_good(world: &mut World, i: u16, g: u8) {
    if let Some(j) = job_mut(world, i) {
        j.free_worker.good = g;
    }
}

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

/// `ISettlerRole::SetEntity` (vtbl+52).
fn set_entity(world: &mut World, i: u16, entity: u16) {
    if let Some(j) = job_mut(world, i) {
        j.entity = entity;
    }
}

/// The target's position (0x57d930); 0 when it is gone.
fn target_pos(world: &World, i: u16) -> Y16X16 {
    let entity = job(world, i).map_or(0, |j| j.entity);
    core::entity_pos(world, entity).unwrap_or(Y16X16(0))
}

fn eco_of(world: &World, p: Y16X16) -> u16 {
    world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.tiling.eco_sector_of(n))
}

/// `CHunterRole::InitWalking` (0x57d000: a `CWalkingWorker`, +8(−1, 0)) and `Init` (0x57cfc0:
/// into the war map, nothing carried).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::WORKER);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    super::hooks::war_map_add(world, i);
    set_good(world, i, 0);
}

/// 0x57d440: give up. Without a target, back to the door with the check-in list.
fn give_up(world: &mut World, i: u16) {
    if !core::check_home(world, i) {
        return;
    }
    set_entity(world, i, 0);
    let home = job(world, i).map_or(0, |j| j.home);
    let door = core::building_door(world, home).unwrap_or(Y16X16(0));
    let here = pos(world, i);
    if let Some(j) = job_mut(world, i) {
        j.start = here;
        j.destination = door;
    }
    core::new_shared_list(world, i, LIST_CHECKIN);
}

/// 0x57d360: the animal is down; walk to it and harvest (on the same eco-sector).
fn harvest(world: &mut World, i: u16) {
    let target = target_pos(world, i);
    let here = pos(world, i);
    if target.0 == 0 || eco_of(world, here) != eco_of(world, target) {
        give_up(world, i);
        return;
    }
    if let Some(j) = job_mut(world, i) {
        j.start = here;
        j.destination = target;
    }
    core::new_shared_list(world, i, LIST_HARVEST);
}

/// 0x57d060: whether the target is alive. A target id whose entity is gone is dropped and the
/// hunter gives up (still "not alive").
fn target_alive(world: &mut World, i: u16) -> bool {
    let entity = job(world, i).map_or(0, |j| j.entity);
    if entity == 0 {
        return false;
    }
    match world.entities.by_index(entity as u32) {
        Some(e) => e.flags & flag::ALIVE_MASK != 0,
        None => {
            set_entity(world, i, 0);
            give_up(world, i);
            false
        }
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

/// `CHunterRole::GetNextJob` (0x57d760): the next task; at the end the hunter waits (WAIT, not
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

/// `CHunterRole::TakeJob` (0x57d4d0).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, cycle, home, destination) = (j.counter, j.cycle_frames, j.home, j.destination);
    match t.task {
        // PUT_GOOD(_VIRTUAL)
        4 | 21 => {
            if core::check_home(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.displacement_costs = 10;
                    j.frame = cycle.wrapping_sub(1);
                }
                register(world, i, counter as i32 / 2);
            }
        }
        task_type::GO_TO_POS | task_type::GO_TO_TARGET => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            let flags = if t.task == task_type::GO_TO_TARGET { WALK_NEAR } else { 0 };
            start_walk(world, i, destination, flags);
        }
        // RESOURCE_GATHERING
        13 => {
            if core::check_home(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.displacement_costs = 10;
                }
                register(world, i, counter as i32);
            }
        }
        // GO_HOME
        14 => {
            if core::check_home(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.frame = 0;
                }
                let door = core::building_door(world, home).unwrap_or(Y16X16(0));
                start_walk(world, i, door, 0);
            }
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            register(world, i, 1);
        }
        // GO_TO_DESTINATION_PILE: the hut's pile for the good carried.
        19 => {
            if core::check_home(world, i) {
                let pile = crate::buildings::pile_with_good(world, home, good(world, i));
                let to = core::entity_pos(world, pile).unwrap_or(Y16X16(0));
                start_walk(world, i, to, WALK_NEAR);
            }
        }
        task_type::CHECKIN => {
            if core::check_home(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.offset = (0, 0);
                    j.destination = Y16X16(0);
                    j.displacement_costs = 10;
                }
                super::hooks::building_settler_enter(world, home, i);
            }
        }
        // WORK_ON_ENTITY: the shot. Facing the animal; it falls when the animation ends.
        28 => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            let target = target_pos(world, i);
            if target.0 == 0 {
                give_up(world, i);
                return;
            }
            let dir = crate::pathing::walking::direction(pos(world, i).0 as i32, target.0 as i32) as u8;
            if let Some(s) = core::settler_mut(world, i) {
                s.direction = dir;
            }
            register(world, i, counter as i32);
            // The arrow (0x57d60b, `CEffects` vtbl+0x18): kind 0 from the hunter to the target,
            // `counter − 3` ticks from now; drawn only.
            let from = pos(world, i);
            crate::projectile::launch(world, 0, from, target, counter as i32 - 3, (0, 0));
        }
        _ => {}
    }
}

/// `CHunterRole::LogicUpdateJob` (0x57d0b0).
pub fn logic_update_job(world: &mut World, i: u16) {
    if !core::check_home(world, i) {
        return;
    }
    let Some(j) = job(world, i) else { return };
    let (task, counter, cycle, entity, destination) = (j.task, j.counter as i32, j.cycle_frames as i32, j.entity, j.destination);
    match task {
        // Walking: follow a living target, shoot from within SHOT_RANGE.
        task_type::GO => {
            if target_alive(world, i) {
                let target = target_pos(world, i);
                if target.0 == 0 {
                    give_up(world, i);
                    return;
                }
                let here = pos(world, i);
                if eco_of(world, here) != eco_of(world, target) {
                    give_up(world, i);
                    return;
                }
                if crate::spiral::grid_distance(target.x() - here.x(), target.y() - here.y()) < SHOT_RANGE {
                    get_next_job(world, i);
                    return;
                }
                if destination != target {
                    if let Some(j) = job_mut(world, i) {
                        j.start = here;
                        j.destination = target;
                    }
                    ActivePathing::walk_to_xy(world, i, target, 0);
                }
            }
            // (After `target_alive` gave up this is the check-in list's first step, taken a
            // second time this tick, as in the original.)
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        // RESOURCE_GATHERING: the meat (`CLandAnimal` vtbl+0x3c: GOOD 24); the body goes
        // (vtbl+0x24, `CAnimalMgr` 0x52e7d0).
        13 => {
            let target = target_pos(world, i);
            if target.0 == 0 {
                give_up(world, i);
                return;
            }
            let dir = crate::pathing::walking::direction(pos(world, i).0 as i32, target.0 as i32) as u8;
            if let Some(s) = core::settler_mut(world, i) {
                s.direction = dir;
            }
            let is_animal = world.entities.by_index(entity as u32).is_some_and(|e| e.obj_type == ObjType::Animal);
            // [inf] Only animals are ever targets; another entity's vtbl+0x3c is not ported.
            let g = if is_animal { crate::animals::MEAT } else { 0 };
            set_good(world, i, g);
            if g == 0 {
                give_up(world, i);
                return;
            }
            crate::animals::delete(world, entity);
            if let Some(e) = world.entities.by_index_mut(entity as u32) {
                e.flags &= !flag::VISIBLE;
            }
            set_entity(world, i, 0);
            get_next_job(world, i);
        }
        // PUT_GOOD: the meat goes onto the hut's pile half-way through the animation.
        21 => {
            let half = cycle / 2;
            let left = counter - half;
            if let Some(j) = job_mut(world, i) {
                j.counter = left as i8;
            }
            if left >= half {
                let home = job(world, i).map_or(0, |j| j.home);
                let pile = crate::buildings::pile_with_good(world, home, good(world, i));
                crate::pile::increase_unforeseen(world, pile, 1);
                // [inf] The meat is no longer carried. The hunter code (0x57ce50..0x57d9a0)
                // writes +0x2c only in Init, the load constructor and the harvest, yet every
                // saved hunter past the delivery (55 records at CHECKIN, all on a new hunt) has
                // 0 there. The clearing instruction is not found; it is put here, where the
                // free worker clears its good.
                set_good(world, i, 0);
                register(world, i, half - 1);
            } else {
                get_next_job(world, i);
            }
        }
        // WORK_ON_ENTITY: the shot hits. Without a target id nothing happens (not even a new
        // registration), as in the original.
        28 => {
            if entity == 0 {
                return;
            }
            let Some(e) = world.entities.by_index(entity as u32) else {
                set_entity(world, i, 0);
                give_up(world, i);
                return;
            };
            if e.obj_type == ObjType::Animal {
                crate::animals::start_dying(world, entity);
            }
            if world.entities.by_index(entity as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK == 0) {
                harvest(world, i);
            } else {
                set_entity(world, i, 0);
                give_up(world, i);
            }
        }
        _ => {}
    }
}

/// `CHunterRole::UpdateJob` (0x57d4a0): PUT_GOOD plays its frames backwards.
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    if matches!(j.task, 4 | 21) {
        let elapsed = j.elapsed;
        j.frame = if elapsed < j.frame as u16 { j.frame - elapsed as u8 } else { 0 };
    }
}

/// `CHunterRole::SetFree(settler, eco_sector)` (0x57d8f0): the meat carried is dropped.
pub fn set_free(world: &mut World, i: u16, eco_sector: i32) -> bool {
    let g = good(world, i);
    if g != 0 {
        let p = pos(world, i);
        crate::pile::search_space_for_goods(world, p.x(), p.y(), g, 1);
    }
    core::set_free_base(world, i, eco_sector)
}

/// `CHunterRole::ConvertEventIntoGoal` (0x57d790).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // Come to work: to the door, check-in list.
        1 => {
            let building = ev.data[0] as u16;
            if let Some(j) = job_mut(world, i) {
                j.home = building;
            }
            if core::check_home(world, i) {
                let door = core::building_door(world, building).unwrap_or(Y16X16(0));
                core::new_destination(world, i, door);
                core::take_anim_list(world, i, 0);
            }
        }
        // Hunt animal `data1` at `data2`: the hut's work mark goes at once.
        5 => {
            let target = Y16X16(ev.data[2] as u32);
            let here = pos(world, i);
            if let Some(j) = job_mut(world, i) {
                j.start = here;
                j.destination = target;
            }
            if let Some(n) = world.grid.checked_index(target.x(), target.y()) {
                world.layers.flags[n] &= !0x20;
            }
            if core::check_home(world, i) {
                set_entity(world, i, ev.data[1] as u16);
                core::take_anim_list(world, i, 1);
            }
        }
        9 => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags |= flag::VISIBLE;
            }
            super::roles::set_free(world, i, -1);
        }
        _ => {
            if core::check_home(world, i) && world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1);
            }
        }
    }
}
