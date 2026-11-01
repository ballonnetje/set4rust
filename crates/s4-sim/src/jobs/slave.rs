//! `CSlaveRole` (role 17, release vtable 0x105c254, `SETTLER_SLAVE` 0x37): the Dark Tribe's slave,
//! made by a shaman's enslaving hit ([`super::shaman::enslave`]). Own reverse engineering of the
//! shipped exe.
//!
//! The dark temple sends a free slave (no home, not in task 0x1b: vtbl+0x4c 0x588710) to the
//! mushroom farm with the fewest slaves ([`crate::dark::slave_to_farm`], event 1) or, without
//! one, to the temple (event 2, which takes it in for good). At the farm (anim list 1:
//! GO_TO_POS 0x21 to the working area, task 0x1c → anim list 2) it works: GO_TO_POS to a node it
//! reserved, WORK, WAIT — the node (node flag 0x20) is one next to a ripe mushroom within the
//! farm's working radius ([`find_spot`], 0x588730, the farmer's search 0). A waiting slave tries
//! again when the farm tells it (event 5, every 8th farm update).
//!
//! Events (`ConvertEventIntoGoal` 0x5882f0): 1 to the farm, 2 to the temple, 3 a work order, 5
//! the farm's call, 9 the farm is gone (back to a farm or the temple on the same sector, else
//! the slave dies; with a player in `data1` it becomes that player's carrier 4 ticks later,
//! future event 9 0x4e55d0), 0x19 out of a manakopter, 0x1c to a manakopter (see
//! [`crate::flying`]; aboard at CHECKIN within distance 4).

use super::core::{self, job, job_mut, register};
use super::state::{EntityEvent, SlaveData};
use super::task::{EntityTask, task_type};
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::grid::Y16X16;

/// Task 0x21: GO_TO_POS with walk flag 0x2000.
const GO_TO_POS_2000: u8 = 0x21;
/// Task 0x1c: take anim list 2 (the work list).
const TAKE_WORK_LIST: u8 = 0x1c;
/// Node flag 0x20: a slave's work node.
const NODE_RESERVED: u8 = 0x20;
/// `CFutureEvents` type 9 (0x4e55d0): a freed slave becomes a carrier.
pub const FUTURE_FREED: u8 = 9;
/// The mushroom farm's building type.
const MUSHROOM_FARM: u16 = 0x31;

fn sd(world: &mut World, i: u16) -> Option<&mut SlaveData> {
    job_mut(world, i).map(|j| &mut j.slave)
}

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

/// `InitWalking` (0x587e90: a `CWalkingNormal` of the owner, target −1) and `Init` (0x587e40:
/// the war map, no reserved node, cursor 0).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    super::hooks::war_map_add(world, i);
    if let Some(s) = sd(world, i) {
        *s = SlaveData::default();
    }
}

/// Role vtbl+0x4c (0x588710): free for the temple's call — no home and not in task 0x1b.
pub fn is_free(world: &World, i: u16) -> bool {
    job(world, i).is_some_and(|j| j.role == super::state::role::SLAVE && j.home == 0 && j.task != 0x1b)
}

/// 0x588a20: the reserved node is given back (node flag 0x20 cleared).
fn release_spot(world: &mut World, i: u16) {
    let Some((x, y)) = job(world, i).map(|j| j.slave.spot) else { return };
    if x < 0 {
        return;
    }
    if let Some(n) = world.grid.checked_index(x, y) {
        world.layers.flags[n] &= !NODE_RESERVED;
    }
    if let Some(s) = sd(world, i) {
        s.spot = (-1, -1);
    }
}

/// Role vtbl+0x84 (0x588730): a work node at the home farm. Up to 35 entries of the spiral
/// around the slave, from its cursor; past `Last(workingAreaRadius)` the cursor restarts at 0
/// and the search fails. An entry `(x, y)` counts when it and the node below `(x, y + 1)` lie on
/// the map, it is within the radius of the farm's working area, `(x, y + 1)` has no node flag
/// 0x20 and the farmer's search 0 (`SearchMushroom`) finds a ripe mushroom at `(x, y)` — then a
/// game-RNG draw keeps it unless its top three bits are all 0 (7/8). The kept node `(x, y + 1)`
/// is reserved and becomes the destination, the start the slave's position.
fn find_spot(world: &mut World, i: u16) -> bool {
    release_spot(world, i);
    let Some(home) = job(world, i).map(|j| j.home).filter(|&h| h != 0) else { return false };
    let Some(b) = world.entities.by_index(home as u32) else { return false };
    let EntityData::Building(d) = &b.data else { return false };
    if b.kind != MUSHROOM_FARM {
        return false;
    }
    let (race, type_ex, centre) = (b.race, d.type_ex, d.working_area);
    let Some(info) = world.rules.work.info(race, type_ex) else { return false };
    let (inhabitant, radius) = (info.inhabitant, info.working_radius);
    let search = world.rules.jobs.search(race, inhabitant, 0).unwrap_or(0);
    let last = crate::spiral::last(radius.clamp(0, 255) as u8) as i32;
    let me = pos(world, i);
    let size = world.grid.size() as i32;
    let start = job(world, i).map_or(0, |j| j.slave.cursor);
    let mut k = start;
    while k < start + 35 {
        k += 1;
        if let Some(s) = sd(world, i) {
            s.cursor = k;
        }
        if k > last {
            if let Some(s) = sd(world, i) {
                s.cursor = 0;
            }
            return false;
        }
        let (x, y) = (me.x() + crate::spiral::dx(k as usize), me.y() + crate::spiral::dy(k as usize));
        if !(0..size).contains(&x) || !(0..size).contains(&y) || y + 1 >= size {
            continue;
        }
        if crate::spiral::grid_distance(x - centre.x(), y - centre.y()) > radius {
            continue;
        }
        let below = world.grid.index(x as u32, (y + 1) as u32);
        if world.layers.flags[below] & NODE_RESERVED != 0 || crate::buildings::search::run(world, search, x, y) == 0 {
            continue;
        }
        if world.rng.rand() < 0x2000 {
            continue;
        }
        world.layers.flags[below] |= NODE_RESERVED;
        if let Some(j) = job_mut(world, i) {
            j.slave.spot = (x, y + 1);
            j.start = me;
            j.destination = Y16X16::pack(x, y + 1);
        }
        return true;
    }
    false
}

fn start_walk(world: &mut World, i: u16, to: Y16X16, flags: i32) {
    ActivePathing::walk_to_xy(world, i, to, flags);
    if let Some(j) = job_mut(world, i) {
        j.task = task_type::GO;
        j.displacement_costs = 5;
    }
    core::go(world, i);
}

/// `GetNextJob` (0x5882b0): the next task, wrapping to the list's start.
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i)
        && let Some(j) = job_mut(world, i)
    {
        j.cursor = 0;
    }
    take_job(world, i);
}

/// `TakeJob` (0x588080).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (destination, start, home, spot) = (j.destination, j.start, j.home, j.slave.spot);
    match t.task {
        task_type::GO_TO_POS | GO_TO_POS_2000 => {
            // The destination plus the task's cell offset, the halves added separately.
            let d = destination.0 as i32;
            let dest = ((t.y as i32 + (d >> 16)) << 16).wrapping_add(t.x as i32 + (d & 0xffff));
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
                j.destination = Y16X16(dest as u32);
            }
            start_walk(world, i, Y16X16(dest as u32), if t.task == GO_TO_POS_2000 { 0x2000 } else { 0 });
        }
        // GO_HOME: back to where the walk out started.
        14 => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            start_walk(world, i, start, 0);
        }
        task_type::WORK => {
            if spot.0 >= 0 {
                if let Some(j) = job_mut(world, i) {
                    j.counter = 5;
                    j.displacement_costs = 10;
                }
                register(world, i, 31);
            } else {
                register(world, i, 1);
            }
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            register(world, i, 31);
        }
        task_type::CHECKIN => {
            if home == 0 {
                die(world, i);
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.offset = (0, 0);
                j.destination = Y16X16(0);
                j.displacement_costs = 10;
            }
            match world.entities.by_index(home as u32).map(|e| (e.obj_type, e.pos)) {
                Some((ObjType::Building, _)) => super::hooks::building_settler_enter(world, home, i),
                // 0x5881f0: aboard when within 4 of a living manakopter (0x5afb60, 0x56b890).
                Some((ObjType::Flying, at)) => {
                    let me = pos(world, i);
                    if crate::spiral::grid_distance(at.x() - me.x(), at.y() - me.y()) < 4 && crate::flying::alive(world, home) {
                        crate::flying::board(world, home, i);
                    } else {
                        die(world, i);
                    }
                }
                _ => die(world, i),
            }
        }
        TAKE_WORK_LIST => core::take_anim_list(world, i, 2),
        _ => {}
    }
}

/// 0x588990: shown again if it stands on its node (not on board), then killed.
fn die(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (p, f) = (e.pos, e.flags);
    let on_node = f & flag::ON_BOARD == 0 && world.grid.checked_index(p.x(), p.y()).is_some_and(|n| world.layers.map_object[n] == i);
    if on_node && let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= flag::VISIBLE;
    }
    world.kill(i, 0);
}

/// `LogicUpdateJob` (0x587ef0).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(task) = job(world, i).map(|j| j.task) else { return };
    match task {
        2 => {
            if find_spot(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.cursor = 0;
                }
                take_job(world, i);
            } else {
                register(world, i, 31);
            }
        }
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        task_type::WORK => {
            release_spot(world, i);
            if find_spot(world, i) {
                // Past the WAIT: GetNextJob wraps to the list's GO_TO_POS.
                if let Some(j) = job_mut(world, i) {
                    j.cursor += 1;
                }
                get_next_job(world, i);
            } else {
                get_next_job(world, i);
                register(world, i, 1);
            }
        }
        _ => {}
    }
}

/// `UpdateJob` (0x587ff0): tasks 0xd / WORK loop their frames, PUT_GOOD plays backwards, GET_GOOD
/// forwards (wrapping to 0).
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    let elapsed = j.elapsed;
    match j.task {
        0xd | task_type::WORK if j.cycle_frames != 0 => j.frame = ((elapsed as u32 + j.frame as u32) % j.cycle_frames as u32) as u8,
        0x15 => j.frame = j.frame.wrapping_sub(elapsed as u8),
        0x16 => {
            j.frame = j.frame.wrapping_add(elapsed as u8);
            if j.frame >= j.cycle_frames {
                j.frame = 0;
            }
        }
        _ => {}
    }
}

/// `SetFree` (0x588930): the reserved node is given back and the home (a building or a
/// manakopter) detaches the slave (vtbl+0x40). Returns false.
pub fn set_free(world: &mut World, i: u16, _eco_sector: i32) -> bool {
    release_spot(world, i);
    let home = job(world, i).map_or(0, |j| j.home);
    if home != 0 && world.entities.by_index(home as u32).is_some() {
        super::hooks::home_detach(world, home, i);
    }
    false
}

/// `ConvertEventIntoGoal` (0x5882f0).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    let home = job(world, i).map_or(0, |j| j.home);
    let here = pos(world, i);
    match ev.event {
        // To the farm: the working area (anim list 1).
        1 => {
            release_spot(world, i);
            if home == 0 {
                set_free(world, i, -1);
                return;
            }
            let Some(area) = core::building_working_area(world, home) else { return };
            if let Some(j) = job_mut(world, i) {
                j.destination = area;
                j.start = here;
            }
            core::take_anim_list(world, i, 1);
        }
        // To the temple: its door (anim list 0).
        2 => {
            if home == 0 {
                set_free(world, i, -1);
                return;
            }
            let door = core::building_door(world, home).unwrap_or(Y16X16(0));
            if let Some(j) = job_mut(world, i) {
                j.destination = door;
                j.start = here;
            }
            core::take_anim_list(world, i, 0);
        }
        // A work order: data2 the destination, data1 the entity (vtbl+0x34), anim list 2.
        3 => {
            if let Some(j) = job_mut(world, i) {
                j.destination = Y16X16(ev.data[2] as u32);
                j.start = here;
                j.entity = ev.data[1] as u16;
            }
            core::take_anim_list(world, i, 2);
        }
        // The farm's call: a waiting slave looks for work again.
        5 => {
            let waiting = job(world, i).is_some_and(|j| j.task == task_type::WAIT);
            if waiting && find_spot(world, i) {
                if let Some(j) = job_mut(world, i) {
                    j.cursor = 0;
                }
                take_job(world, i);
            }
            register(world, i, 1);
        }
        // The farm is gone.
        9 => {
            set_free(world, i, -1);
            let player = ev.data[1];
            if player > 0 {
                // The one-task list 0x1549a68 (task 0x1b, job part 0x145), then a carrier of
                // `player` 4 ticks later.
                let t = EntityTask::new(0x1b, 0x145, 0, 0, -1, 1, -1, true, true, 0, 0, 0, 0);
                core::new_to_do_list(world, i, Some(super::ToDo::Own(vec![t])), -1);
                if let Some(j) = job_mut(world, i) {
                    j.displacement_costs = 10;
                }
                crate::future::add(world, FUTURE_FREED, 4, i, player, 0);
                return;
            }
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            let Some(temple) = crate::dark::temple_of(world, owner) else { return };
            let ensign = match world.entities.by_index(temple as u32).map(|e| &e.data) {
                Some(EntityData::Building(d)) => d.ensign,
                _ => return,
            };
            let sector = |w: &World, p: Y16X16| w.grid.checked_index(p.x(), p.y()).map_or(0, |n| w.tiling.sector_of(n));
            if sector(world, here) == sector(world, ensign) {
                crate::dark::slave_to_farm(world, i);
            } else {
                die(world, i);
            }
        }
        // Out of the manakopter at data1 (0x582a40), into the war map again, off board, VISIBLE
        // and SELECTABLE, `SetFree(-1)`, task 0x11, then to a farm (`CDarkTribe` vtbl+8).
        0x19 => {
            let at = Y16X16(ev.data[1] as u32);
            crate::create::put_settler_near(world, i, at.x(), at.y());
            crate::warmap::readd(world, i);
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags = (e.flags & !flag::ON_BOARD) | 0x2100;
            }
            set_free(world, i, -1);
            if let Some(j) = job_mut(world, i) {
                j.task = 0x11;
            }
            crate::dark::slave_to_farm(world, i);
        }
        // To a living manakopter with a position: the walk there (anim list 0), no longer
        // SELECTABLE, out of the groups. A dead one: off the temple's sector the slave dies.
        0x1c => {
            let mk = ev.data[0] as u16;
            if crate::flying::alive(world, mk) {
                let at = world.entities.by_index(mk as u32).map_or(Y16X16(0), |e| e.pos);
                if at.0 != 0 {
                    if let Some(j) = job_mut(world, i) {
                        j.start = here;
                        j.destination = at;
                    }
                    core::take_anim_list(world, i, 0);
                    if let Some(e) = world.entities.by_index_mut(i as u32) {
                        e.flags &= !0x2200;
                    }
                    crate::group::remove_all(world, i);
                }
            } else {
                let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
                if let Some(temple) = crate::dark::temple_of(world, owner) {
                    let ensign = match world.entities.by_index(temple as u32).map(|e| &e.data) {
                        Some(EntityData::Building(d)) => d.ensign,
                        _ => Y16X16(0),
                    };
                    let sector = |w: &World, p: Y16X16| w.grid.checked_index(p.x(), p.y()).map_or(0, |n| w.tiling.sector_of(n));
                    if sector(world, here) != sector(world, ensign) {
                        die(world, i);
                    }
                }
            }
        }
        _ => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags |= core::FLAG_CONTINUE_AFTER_EVENT;
            }
        }
    }
}

/// Future event 9 (0x4e55d0, `entity` = the slave, `a` = a player): the slave, visible, not on
/// board and on its node, is hidden and killed; a carrier of player `a` takes its place
/// (`AddSettler(x, y, a, 1, 0)`).
pub fn freed(world: &mut World, slave: u16, player: i32) {
    let Some(e) = world.entities.by_index(slave as u32).filter(|e| e.obj_type == ObjType::Settler) else { return };
    let (p, f) = (e.pos, e.flags);
    let on_node = f & (flag::ON_BOARD | flag::VISIBLE) == flag::VISIBLE && world.grid.checked_index(p.x(), p.y()).is_some_and(|n| world.layers.map_object[n] == slave);
    if !on_node {
        return;
    }
    if let Some(e) = world.entities.by_index_mut(slave as u32) {
        e.flags &= !flag::VISIBLE;
    }
    world.kill(slave, 0);
    crate::create::add_settler(world, p.x(), p.y(), player as u8, 1);
}
