//! The vehicles' update and their moves (`CVehicle` / `CWheeler`, decompiles in
//! `tools/re/vehicles/vehicle.c`, `wheeler.c`, `tasks.c`).
//!
//! * `LogicUpdate` (vtbl+0x64, 0x594f60): the animation (0x594db0); a finished vehicle handles its
//!   queued events (`ProcessAllEvents` → vtbl+0x6c) and then either takes the first task of a new
//!   list (vtbl+0xbc) or goes on with its task (vtbl+0x90, 0x595510): walking (task 6: the
//!   walker's `Walk`) or standing (0x11: its `IdleWalk`), each step through 0x5950d0. A vehicle
//!   under construction waits 16 ticks at a time until its progress reaches its end.
//! * Orders: `SendTo` sends event 0x11 / 0xd (`CVehicle` 0x5954b0: the place in data 1), which
//!   builds the task list (0x595240): a GO_TO_POS (7) to the place, or GO (6) with flag 4.
//! * A cart takes a GO_TO_POS by walking there (0x55b6d0); everything else is standing (0x11).
//! * One step (0x59a3a0): the footprint leaves the old node, the new node must be catapult
//!   ground with room for it, else the vehicle stays (step | 0x88); a turn costs a tick per
//!   1/16 (0x594e90). After a step the next update comes in speed + turn ticks, else in 1.
//!
//! The classes' own parts are in `cart`, `catapult`, `ship` and `ferry` (a land vehicle's
//! ferry events 0x18 / 0x19 and boarding are here, in the `CWheeler` part).

use super::{VehicleTask, get, get_mut, kind};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;
use crate::jobs::state::EntityEvent;
use crate::pathing::walking::{self, WalkState};

/// The vehicle tasks (`CEntityTask::*`).
pub mod task {
    pub const GO: u8 = 6;
    pub const GO_TO_POS: u8 = 7;
    pub const WAYPOINT: u8 = 8;
    pub const ANIMATION: u8 = 0x10;
    pub const IDLE: u8 = 0x11;
}

/// `CVehicle::DIR6_TO_DIR16` (.rdata 0x101b488).
const DIR16: [u8; 6] = [0, 2, 6, 8, 10, 14];
/// `Y16X16::g_iNeighborY16X16Modifiers` (.rdata 0x101bd58).
const MODIFIER: [i32; 6] = [1, 0x10001, 0x10000, -1, -0x10001, -0x10000];

/// `IAnimatedEntity::SetEvent` (0x532e60): queued; the vehicle updates next tick if it is not
/// registered.
pub fn set_event(world: &mut World, id: u16, ev: EntityEvent) {
    let registered = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0);
    if !registered {
        world.register_for_logic_update(id, 1);
    }
    if let Some(v) = get_mut(world, id) {
        v.events.push(ev);
    }
}

/// `CVehicle::LogicUpdate` (vtbl+0x64, 0x594f60).
pub fn logic_update(world: &mut World, id: u16) {
    animate(world, id);
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.flags & flag::BIRTH != 0 {
        let done = get(world, id).is_some_and(|v| v.progress[1] <= v.progress[0]);
        if done {
            // vtbl+0xa0: a cart calls its donkey (0x55dc50), the others are finished (0x594830).
            if e.kind == kind::CART && e.obj_type == ObjType::LandVehicle {
                super::cart::finish(world, id);
            } else {
                super::finish(world, id);
            }
        } else {
            world.register_for_logic_update(id, 16);
        }
        return;
    }
    if let Some(v) = get_mut(world, id) {
        v.b5c = 8;
        v.distance = 0;
    }
    // `ProcessAllEvents`; the global 0x12dbc54 says whether an event started a new list.
    let mut new_list = false;
    while let Some(ev) = get_mut(world, id).and_then(|v| (!v.events.is_empty()).then(|| v.events.remove(0))) {
        new_list |= convert_event(world, id, &ev);
    }
    if new_list {
        take_next_task(world, id);
    } else {
        vtbl_continue(world, id);
    }
}

/// vtbl+0x90: the cart's (0x55bca0) or `CVehicle`'s (0x595510).
fn vtbl_continue(world: &mut World, id: u16) {
    if world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::LandVehicle && e.kind == kind::CART) {
        super::cart::continue_task(world, id);
    } else if super::catapult::is_catapult(world, id) {
        super::catapult::update(world, id);
    } else if super::ship::is_ship(world, id) {
        super::ship::update(world, id);
    } else {
        continue_task(world, id);
    }
}

/// vtbl+0x6c `ConvertEventIntoGoal`: the class's own handling, then `CWheeler` 0x59a200 and
/// `CVehicle` 0x5954b0. Returns whether a new task list started.
fn convert_event(world: &mut World, id: u16, ev: &EntityEvent) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let (k, obj, pos) = (e.kind, e.obj_type, e.pos);
    // Ships: transport ships 0x591d20 (their ports' events 7 / 9, then the move), war ships
    // 0x5923b0 (a move order onto an enemy attacks it); ferries `CVehicle` 0x5954b0 only.
    if super::transport::is_transport(world, id) {
        return super::transport::convert_event(world, id, ev);
    }
    if super::warship::is_warship(world, id) {
        return super::warship::convert_event(world, id, ev);
    }
    if obj == ObjType::Ship {
        return match ev.event {
            0xd => new_task_list(world, id, ev.data[1], ev.data[2], ev.data[0] as u32),
            0x11 if ev.kind == 0xd => {
                let p = Y16X16(ev.data[1] as u32);
                new_task_list(world, id, p.x(), p.y(), ev.data[0] as u32)
            }
            _ => false,
        };
    }
    // CCart 0x55c170: its trade buildings gone (events 7 / 9); a move order keeps where the
    // cart stood (+0x114).
    if k == kind::CART {
        super::cart::convert_event(world, id, ev);
    }
    // CCatapult 0x560f80: a move order may be an attack (0x5614b0); a transporter follows.
    if k == kind::WAR_MACHINE && super::catapult::convert_event(world, id, ev) {
        return false;
    }
    // CWheeler 0x59a200.
    match ev.event {
        // The building it stood in started (7): out, a new list on its place.
        7 => {
            if let Some(e) = world.entities.by_index_mut(id as u32) {
                e.flags = (e.flags & !0x8000) | 0x2000;
            }
            let r = new_task_list(world, id, pos.x(), pos.y(), 0);
            if let Some(v) = get_mut(world, id) {
                v.target_entity = 0;
                v.target_pos = 0;
            }
            r
        }
        // vtbl+0xe0: come to the ferry data 0 (`CWheeler` 0x59a0e0, the catapults' 0x561630).
        0x18 => come_to_vehicle(world, id, ev.data[0] as u16),
        // Ashore at data 1 / data 2 if it can stand there (0x597020): on the map, visible and
        // SELECTABLE, off board; else back aboard ferry data 0 (0x58ecd0).
        0x19 => {
            let (x, y) = (ev.data[1], ev.data[2]);
            if super::can_place(world, x, y, k) {
                let to = Y16X16::pack(x, y);
                super::put_on_map(world, id, to);
                if let Some(e) = world.entities.by_index_mut(id as u32) {
                    e.pos = to;
                    e.flags = (e.flags & !flag::ON_BOARD) | 0x2100;
                }
                if let Some(v) = get_mut(world, id) {
                    v.target_pos = 0;
                }
            } else {
                if let Some(v) = get_mut(world, id) {
                    v.target_pos = 0;
                }
                let ferry = ev.data[0] as u16;
                if super::ferry::is_ferry(world, ferry) {
                    super::ferry::reboard(world, ferry, id);
                }
            }
            false
        }
        // A move order (without a vehicle at +0x98) whose click (data 2) is on a ferry boards it
        // (0x58e760, its result is not a new list).
        0x11 if ev.kind == 0xd && get(world, id).is_some_and(|v| v.target_entity < 1) && {
            let p = Y16X16(ev.data[2] as u32);
            super::ferry::ferry_near(world, p.x(), p.y()) != 0
        } =>
        {
            let p = Y16X16(ev.data[2] as u32);
            let ferry = super::ferry::ferry_near(world, p.x(), p.y());
            super::ferry::board(world, ferry, id);
            false
        }
        // CVehicle 0x5954b0: a move to data 1/2 (event 0xd) or the packed data 1 (0x11 / 0xd),
        // flags in data 0.
        0xd => new_task_list(world, id, ev.data[1], ev.data[2], ev.data[0] as u32),
        // (0x59a272..0x59a2a3, leaving the vehicle +0x98 on a move order, is unreachable: it
        // runs only when +0x98 ≤ 0.)
        0x11 if ev.kind == 0xd => {
            let p = Y16X16(ev.data[1] as u32);
            new_task_list(world, id, p.x(), p.y(), ev.data[0] as u32)
        }
        _ => false,
    }
}

/// `CWheeler` vtbl+0xe0 (0x59a0e0) `(vehicle)`: the place the vehicle (a ferry) gives it (its
/// vtbl+0x8c) is +0x94; if the walker may stand there, a new task list to it, the vehicle
/// attaches it (+0x98), it leaves its groups, SELECTABLE / SELECTED go and the AI hears (0x12).
/// A catapult (0x561630) then lets its ammunition transporter go (event 9 `[catapult, its
/// eco-sector]`). Returns whether a new list started.
fn come_to_vehicle(world: &mut World, id: u16, vehicle: u16) -> bool {
    if !world.entities.by_index(vehicle as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle)) {
        return false;
    }
    let place = super::exit_place(world, vehicle, id);
    if let Some(v) = get_mut(world, id) {
        v.target_pos = place;
    }
    let mut new_list = false;
    if place != 0 {
        let p = Y16X16(place as u32);
        let n = world.grid.checked_index(p.x(), p.y());
        let base = WalkState::default();
        let ok = {
            let w = get(world, id).and_then(|v| v.walking.as_deref()).unwrap_or(&base);
            walking::is_not_blocked(world, w, n) && walking::walker_not_occupied(world, w, n)
        };
        if ok {
            new_list = new_task_list(world, id, p.x(), p.y(), 0);
            super::attach(world, vehicle, id);
            crate::group::remove_all(world, id);
            // (Deselected if selected, 0x4eda10: the client's.)
            let owner = world.entities.by_index_mut(id as u32).map_or(0, |e| {
                e.flags &= !0x2200;
                e.owner
            });
            crate::ai::post_event(world, 0x12, owner as i32, id as i32, vehicle as i32);
        }
    }
    if super::catapult::is_catapult(world, id) {
        let carrier = super::catapult::data(world, id).map_or(0, |c| c.carrier);
        let on = get(world, id).is_some_and(|v| v.target_entity > 0);
        if on && carrier > 0 && world.entities.by_index(carrier as u32).is_some_and(|e| e.obj_type == ObjType::Settler) {
            super::ammo::release(world, id, carrier as u16);
        }
    }
    new_list
}

/// 0x595240 `(x, y, flags)`: the task list. Without flag 2 the list is cleared (with it, a list
/// that repeats keeps running). Flag 1 makes a repeating patrol (a waypoint at −1/−1, i.e. where
/// it stands, then the place); flag 0x10 a task 0x24; else GO_TO_POS (GO with flag 4) to the
/// place. A new list starts (`true`) unless one was running.
pub fn new_task_list(world: &mut World, id: u16, x: i32, y: i32, flags: u32) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let info = world.rules.vehicles.get(e.race, e.kind as u8);
    let ship = e.obj_type == ObjType::Ship;
    let (job, frames) = (info.job as u16, world.rules.vehicles.frame_count(e.race, info.job as u16) as i8);
    let Some(v) = get_mut(world, id) else { return false };
    let mk = |t: u8, x: i16, y: i16| VehicleTask { task: t, good: 0, dir: -1, duration: -1, frames, trigger: 0, x, y, job_part: job, entity: 0, w12: 0, visible: true, forward: true };
    if flags & 1 == 0 {
        if flags & 2 == 0 {
            v.tasks.clear();
            v.listed = 0;
            v.list_len = 0;
        } else if v.list_len != 0 {
            return false;
        }
        if flags & 0x10 != 0 {
            v.tasks.push(VehicleTask { task: 0x24, x: 0, y: 0, ..mk(0x24, 0, 0) });
        } else {
            // (Ships with flag 4 first look for a ship of the player at the place, 0x5b1c60: not
            // ported, ships do not move.)
            let _ = ship;
            let t = if (flags >> 2) & 1 == 0 { task::GO_TO_POS } else { task::GO };
            v.tasks.push(mk(t, x as i16, y as i16));
        }
    } else {
        if v.list_len == 0 {
            if flags & 2 == 0 {
                v.tasks.clear();
                v.listed = 0;
            }
            v.tasks.push(mk(task::WAYPOINT, -1, -1));
            v.list_len = v.tasks.len() as u8;
        }
        v.tasks.push(mk(task::WAYPOINT, x as i16, y as i16));
    }
    if v.listed != 0 {
        return false;
    }
    v.listed = 1;
    true
}

/// vtbl+0xbc: the class's first / next task. Carts (0x55b6d0); catapults and ships are not
/// ported (they stand).
pub fn take_next_task(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (k, obj, race) = (e.kind, e.obj_type, e.race);
    if k == kind::WAR_MACHINE && obj == ObjType::LandVehicle {
        super::catapult::next_task(world, id);
        return;
    }
    if obj == ObjType::Ship {
        if super::transport::is_transport(world, id) {
            super::transport::next_task(world, id);
        } else if super::warship::is_warship(world, id) {
            super::warship::next_task(world, id);
        } else {
            super::ship::next_task(world, id);
        }
        return;
    }
    let info = world.rules.vehicles.get(race, k as u8);
    let t = get(world, id).and_then(|v| (v.listed != 0).then(|| v.tasks.get(v.listed as usize - 1).copied()).flatten());
    apply_task(world, id, t);
    let cur = get(world, id).map_or(task::IDLE, |v| v.task);
    if cur == task::GO_TO_POS {
        let t = t.unwrap_or_default();
        // The cart moves unless it is a foundation cart without all its pioneers and goods.
        if super::cart::may_move(world, id) {
            let walker = get_mut(world, id).and_then(|v| {
                v.task = info.x24 as u8;
                v.walking.take()
            });
            if let Some(mut w) = walker {
                w.init(world, (t.y as i32) << 16 | (t.x as u16 as i32), 0);
                if let Some(v) = get_mut(world, id) {
                    v.walking = Some(w);
                }
            }
        } else if let Some(v) = get_mut(world, id) {
            v.task = task::IDLE;
        }
    } else if cur != task::IDLE {
        idle(world, id);
    }
    vtbl_continue(world, id);
}

/// 0x595150 `(task)`: the task's direction, visibility, task and gfx job with its frame count
/// (none: standing, visible, the class's idle job).
pub(crate) fn apply_task(world: &mut World, id: u16, t: Option<VehicleTask>) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let race = e.race;
    let info = world.rules.vehicles.get(e.race, e.kind as u8);
    let visible = t.is_none_or(|t| t.visible);
    let frames = world.rules.vehicles.frame_count(race, t.map_or(info.job as u16, |t| t.job_part));
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        if visible {
            e.flags |= flag::VISIBLE;
        } else {
            e.flags &= !flag::VISIBLE;
        }
    }
    let Some(v) = get_mut(world, id) else { return };
    match t {
        None => {
            v.task = task::IDLE;
            v.job_part = info.job as u16;
        }
        Some(t) => {
            if t.dir >= 0 {
                v.dir = t.dir;
            }
            v.task = t.task;
            v.job_part = t.job_part;
        }
    }
    v.frames = frames;
    v.frame = 0;
    v.half_frame = 0;
    v.b5c = 8;
    v.distance = 0;
}

/// The standing state (0x55b6d0's else branch / 0x561150's head): visible, task 0x11, the idle
/// job.
pub(crate) fn idle(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags |= flag::VISIBLE;
    let info = world.rules.vehicles.get(e.race, e.kind as u8);
    let frames = world.rules.vehicles.frame_count(e.race, info.job as u16);
    let Some(v) = get_mut(world, id) else { return };
    v.task = task::IDLE;
    v.job_part = info.job as u16;
    v.frames = frames;
    v.frame = 0;
    v.half_frame = 0;
    v.b5c = 8;
    v.distance = 0;
}

/// `CVehicle` vtbl+0x90 (0x595510): walking (6) steps with `Walk`, standing (0x11) with
/// `IdleWalk`.
pub(crate) fn continue_task(world: &mut World, id: u16) {
    let Some(v) = get(world, id) else { return };
    let t = v.task;
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return };
    let Some(mut w) = get_mut(world, id).and_then(|v| v.walking.take()) else { return };
    let r = match t {
        task::GO => Some((walking::walk(world, &mut w, pos), true)),
        task::IDLE => Some((walking::idle_walk(world, &mut w, pos.0 as i32, 0), false)),
        _ => None,
    };
    if let Some(v) = get_mut(world, id) {
        v.walking = Some(w);
    }
    if let Some((s, check)) = r {
        step(world, id, s, check);
    }
}

/// 0x5950d0 `(step, check)`: the move; arrived / failed / idle (0x70) with `check` → the next
/// task; else the next update after the step's time.
pub(crate) fn step(world: &mut World, id: u16, s: u32, check: bool) {
    // vtbl+0xd8: `CWheeler` 0x59a3a0, `CShip` 0x5908d0.
    if super::ship::is_ship(world, id) {
        super::ship::move_step(world, id, s);
    } else {
        move_step(world, id, s);
    }
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let info = world.rules.vehicles.get(e.race, e.kind as u8);
    let Some(v) = get(world, id) else { return };
    if check && v.b5c & 0x70 != 0 {
        next_task(world, id);
        return;
    }
    let t = if v.task == task::IDLE { task::IDLE } else { task::GO };
    let delay = if v.b5c & 0xf < 6 { v.speed as i32 + v.turn as i32 } else { 1 };
    set_task(world, id, t, delay, info.job as u16);
}

/// 0x596280 `(task, delay, job)`.
fn set_task(world: &mut World, id: u16, t: u8, delay: i32, job: u16) {
    let race = world.entities.by_index(id as u32).map_or(0, |e| e.race);
    let frames = world.rules.vehicles.frame_count(race, job);
    if let Some(v) = get_mut(world, id) {
        v.task = t;
        if v.job_part != job {
            v.job_part = job;
            v.frame = 0;
            v.half_frame = 0;
            v.frames = frames;
        }
    }
    world.register_for_logic_update(id, delay);
}

/// 0x5951e0: the next task of the list, or from the start of a repeating part (+0x65), or the
/// list ends; then vtbl+0xbc.
pub(crate) fn next_task(world: &mut World, id: u16) {
    if let Some(v) = get_mut(world, id) {
        if (v.listed as usize) < v.tasks.len() {
            v.listed += 1;
        } else {
            v.listed = v.list_len;
            if v.list_len == 0 {
                v.tasks.clear();
            }
        }
    }
    take_next_task(world, id);
}

/// `CWheeler` 0x59a3a0 `(step)`: one node in the step's direction if the walker finds it
/// catapult ground with room for the footprint; else the step is marked blocked (| 0x88).
/// +0x5c keeps the step.
fn move_step(world: &mut World, id: u16, s: u32) {
    let mut s = s;
    let d = (s & 0xf) as usize;
    if d < 6 {
        let Some(e) = world.entities.by_index(id as u32) else { return };
        let pos = e.pos;
        let to = Y16X16(pos.0.wrapping_add(MODIFIER[d] as u32));
        super::take_off_map(world, pos);
        let n = world.grid.checked_index(to.x(), to.y());
        let base = WalkState::default();
        let ok = {
            let w = get(world, id).and_then(|v| v.walking.as_deref()).unwrap_or(&base);
            walking::is_not_blocked(world, w, n) && walking::walker_not_occupied(world, w, n)
        };
        if ok {
            super::put_on_map(world, id, to);
            if get(world, id).is_some_and(|v| d as i8 != v.dir) {
                turn_to(world, id, d as i8);
            }
            if let Some(e) = world.entities.by_index_mut(id as u32) {
                e.pos = to;
            }
            if let Some(v) = get_mut(world, id) {
                v.distance = 0xff;
            }
            crate::warmap::moved(world, id);
        } else {
            super::put_on_map(world, id, pos);
            s |= 0x88;
        }
    }
    // Arrived at the place a vehicle (+0x98, a ferry) gave it: it checks in there (vtbl+0x80).
    let (home, at, pos) = match (get(world, id), world.entities.by_index(id as u32)) {
        (Some(v), Some(e)) => (v.target_entity, v.target_pos, e.pos.0 as i32),
        _ => (0, 0, 0),
    };
    if s & 0x20 != 0 && home != 0 && pos == at && world.entities.by_index(home as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle)) {
        super::settler_entered(world, home as u16, id);
        if let Some(v) = get_mut(world, id) {
            v.target_entity = 0;
        }
    }
    if let Some(v) = get_mut(world, id) {
        v.b5c = s as u8;
    }
}

/// 0x594e90 `(dir)`: the new direction; the 16-way direction starts one step towards it and
/// +0x5d counts the 1/16 steps of the turn.
pub(crate) fn turn_to(world: &mut World, id: u16, dir: i8) -> u8 {
    let Some(v) = get_mut(world, id) else { return 0 };
    let old = v.dir;
    let diff = dir as i32 - old as i32;
    if diff == 0 {
        v.turn = 0;
        return 0;
    }
    v.dir = dir;
    let clockwise = if diff < 1 { diff < -3 } else { diff < 4 };
    let from = DIR16[(old.rem_euclid(6)) as usize] as i32;
    let cur = if clockwise { ((from + 1) & 0xf) as u8 | 0x80 } else { ((from - 1) & 0xf) as u8 };
    v.dir16 = cur;
    let mut steps = ((cur & 0xf) as i32 - DIR16[dir.rem_euclid(6) as usize] as i32).abs();
    if steps > 8 {
        steps = 16 - steps;
    }
    v.turn = steps as u8;
    v.turn
}

/// 0x594db0: the turn goes on a 1/16 per tick, then the frames of an animation (0x10) or a
/// walk / stand (6 / 0x11, unless the last step was none) advance. The gfx frame count (+0x61)
/// is the client's; without it the frame stays.
fn animate(world: &mut World, id: u16) {
    let tick = world.tick;
    let flags = world.entities.by_index(id as u32).map_or(0, |e| e.flags);
    let Some(v) = get_mut(world, id) else { return };
    let mut elapsed = tick.wrapping_sub(v.anim_tick);
    if elapsed == 0 {
        return;
    }
    v.anim_tick = tick;
    if v.turn != 0 {
        let k = (v.turn as u32).min(elapsed);
        v.turn -= k as u8;
        let d = if v.dir16 & 0x80 != 0 { k as i32 } else { -(k as i32) };
        v.dir16 = (((v.dir16 & 0x7f) as i32 + d) & 0xf) as u8 | (v.dir16 & 0x80);
        elapsed -= k;
        if elapsed == 0 {
            return;
        }
    }
    let frames = v.frames as u32;
    match v.task {
        task::ANIMATION => {
            if frames != 0 {
                v.frame = ((v.frame as u32 + elapsed) % frames) as u8;
            }
        }
        task::GO | task::IDLE if v.b5c & 8 == 0 && frames != 0 => {
            if flags & 0x4_0000 != 0 {
                v.half_frame = ((v.half_frame as u32 + elapsed) % (frames * 2)) as u8;
                v.frame = v.half_frame >> 1;
            } else {
                v.frame = ((v.frame as u32 + elapsed) % frames) as u8;
            }
        }
        _ => {}
    }
}
