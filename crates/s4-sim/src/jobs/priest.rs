//! `CPriestRole` (role 9, vtable 0x105bd1c; own reverse engineering of the shipped exe, SIM.md
//! §20): the priest the big temple makes. The player's spell order (command 5048 → event 3)
//! walks him towards the target until it is within the spell's range and casts there with
//! [`crate::magic::cast_flag::DEFER`] (the mana is paid at once, the spell happens 12 ticks
//! later).
//!
//! Lists of every race: 195 (GO_TO_TARGET, WORK = the cast), 252 (the move order: GO_TO_POS,
//! WAIT 13), the wait list (type 45: WAIT 1) and 111 (GO_TO_TARGET, CHECKIN: into a vehicle).
//! `LogicUpdate` (0x586930), `Go` (0x586c00) and `SetFree` (0x587050) are the base ones.

use super::core::{self, job, job_mut, register};
use super::state::EntityEvent;
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::flag;
use crate::grid::Y16X16;

/// The cast list (`SettlerJobList(race, 0xc3)`).
pub const LIST_CAST: u16 = 0xc3;
/// The move-order list (0xfc).
pub const LIST_MOVE: u16 = 0xfc;
/// No spell ordered (role +0x38).
const NO_SPELL: i32 = -1;

fn unported(world: &mut World, what: &'static str, entity: u16) {
    world.events.push(crate::event::SimEvent::Unported { what, entity });
}

fn spell(world: &World, i: u16) -> i32 {
    job(world, i).map_or(NO_SPELL, |j| j.priest.spell)
}

fn set_spell(world: &mut World, i: u16, s: i32) {
    if let Some(j) = job_mut(world, i) {
        j.priest.spell = s;
    }
}

/// `InitWalking` (0x57f470: a `CWalkingNormal` of the owner, target −1) and `Init` (0x57f420):
/// +0x12 = own id, VULNERABLE | SELECTABLE, into the war map again, no spell. A new settler gets
/// the walking object before `Init` (ctor 0x580330); a settler changing role gets it after
/// (`CSettler::SetRole` 0x580c20), replacing its old one (a carrier's `CWalkingWorker`).
pub fn init_role(world: &mut World, i: u16) {
    let fresh = job(world, i).is_some_and(|j| j.walking.is_none());
    if fresh {
        init_walking(world, i);
    }
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
        j.priest.spell = NO_SPELL;
    }
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= flag::VULNERABLE_MASK | flag::SELECTABLE;
    }
    crate::warmap::readd(world, i);
    if !fresh {
        init_walking(world, i);
    }
}

/// `InitWalking` (0x57f470).
fn init_walking(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
}

/// 0x5afad0 `(dx, dy)`: the direction 0..5 of a node offset.
pub fn direction_xy(dx: i32, dy: i32) -> u8 {
    let (mut a, mut b) = (dx.wrapping_sub(dy), dx.wrapping_add(dy));
    let back = (a < 0 && b == 0) || b < 0;
    if back {
        a = a.wrapping_neg();
        b = b.wrapping_neg();
    }
    let two = a.wrapping_mul(2);
    ((two <= b.wrapping_neg()) as u8) + ((two <= b) as u8) + 3 * back as u8
}

/// The hex distance from the priest to its destination (+0x18).
fn distance_to_target(world: &World, i: u16) -> i32 {
    let (Some(e), Some(j)) = (world.entities.by_index(i as u32), job(world, i)) else { return i32::MAX };
    crate::spiral::grid_distance(j.destination.x() - e.x(), j.destination.y() - e.y())
}

fn in_range(world: &World, i: u16) -> bool {
    let s = spell(world, i);
    if s == NO_SPELL {
        return false;
    }
    let race = world.entities.by_index(i as u32).map_or(0, |e| e.race);
    distance_to_target(world, i) <= world.rules.magic.range(race, s as u8)
}

/// `CPriestRole::TakeJob` (0x57f590).
/// * GO_TO_POS: 0x580150 (the walk to the destination, the frame kept).
/// * GO_TO_TARGET (10): the destination moves by the task's offset, the walk starts (task GO,
///   displacement costs 5); already within the spell's range → `GetNextJob`; with events queued
///   the first step waits a tick, else it is taken now.
/// * 14: back to the start, the spell forgotten.
/// * WORK (16), the cast: `CastSpell(owner, spell, destination, DEFER, priest)`; the spell is
///   forgotten either way. Cast → displacement costs 10, the task's frames, turned to the target
///   (0x5afad0); not cast → the wait list.
/// * WAIT: displacement costs 0, again in `1 + Rand() % 16` ticks.
/// * CHECKIN: into the vehicle he was called to (home vtbl+0x80), the spell forgotten.
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    if t.task == task_type::GO_TO_POS {
        super::gardener::retake_go(world, i, &t);
        return;
    }
    core::init_common_task_values(world, i, &t);
    match t.task {
        task_type::GO_TO_TARGET => {
            let Some(j) = job_mut(world, i) else { return };
            j.frame = 1;
            let d = j.destination;
            let dest = Y16X16::pack(d.x() + j.x as i32, d.y() + j.y as i32);
            j.destination = dest;
            ActivePathing::walk_to_xy(world, i, dest, 0);
            if let Some(j) = job_mut(world, i) {
                j.task = task_type::GO;
                j.displacement_costs = 5;
            }
            if in_range(world, i) {
                get_next_job(world, i);
                return;
            }
            if job(world, i).is_some_and(|j| !j.events.is_empty()) {
                if let Some(j) = job_mut(world, i) {
                    j.walk = 0x88;
                }
                register(world, i, 1);
                return;
            }
            core::go(world, i);
        }
        14 => {
            let Some(j) = job_mut(world, i) else { return };
            j.frame = 1;
            let start = j.start;
            ActivePathing::walk_to_xy(world, i, start, 0);
            if let Some(j) = job_mut(world, i) {
                j.task = task_type::GO;
                j.displacement_costs = 5;
                j.priest.spell = NO_SPELL;
            }
            core::go(world, i);
        }
        task_type::WORK => cast(world, i),
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            let r = world.rng.rand();
            register(world, i, (r & 0xf) as i32 + 1);
        }
        task_type::CHECKIN => {
            let home = job(world, i).map_or(0, |j| j.home);
            match world.entities.by_index(home as u32).map(|e| e.obj_type) {
                Some(crate::entity::ObjType::LandVehicle | crate::entity::ObjType::Ship) => crate::vehicles::settler_entered(world, home, i),
                Some(_) => unported(world, "CPriestRole CHECKIN (home vtbl+0x80)", i),
                None => {}
            }
            set_spell(world, i, NO_SPELL);
        }
        _ => {}
    }
}

/// TakeJob WORK (0x57f60a): the cast.
fn cast(world: &mut World, i: u16) {
    let s = spell(world, i);
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (owner, pos) = (e.owner, e.pos);
    let dest = job(world, i).map_or(Y16X16(0), |j| j.destination);
    let cast = (0..8).contains(&s) && crate::magic::cast_spell(world, owner, s as u8, dest.x(), dest.y(), crate::magic::cast_flag::DEFER, i) > 0;
    set_spell(world, i, NO_SPELL);
    if !cast {
        core::take_wait_list(world, i);
        return;
    }
    let frames = job(world, i).map_or(1, |j| j.cycle_frames);
    if let Some(j) = job_mut(world, i) {
        j.displacement_costs = 10;
    }
    register(world, i, frames as i32);
    let dir = direction_xy(dest.x() - pos.x(), dest.y() - pos.y());
    if let Some(s) = core::settler_mut(world, i) {
        s.direction = dir;
    }
}

/// `CPriestRole::LogicUpdateJob` (0x57f4d0): walking (GO), one step (`Go`), then within the
/// spell's range of the destination → `GetNextJob` (the cast); after the cast (WORK) the wait
/// list.
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(task) = job(world, i).map(|j| j.task) else { return };
    match task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
            if in_range(world, i) {
                get_next_job(world, i);
            }
        }
        task_type::WORK => core::take_wait_list(world, i),
        _ => {}
    }
}

/// `CPriestRole::GetNextJob` (0x57f830): the next task; after the last one the list starts
/// again from its first task and the spell is forgotten.
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i)
        && let Some(j) = job_mut(world, i)
    {
        j.cursor = 0;
        j.priest.spell = NO_SPELL;
    }
    take_job(world, i);
}

/// `CPriestRole::ConvertEventIntoGoal` (0x57f880).
/// * 3 (command 5048): the spell (`kind`) and its target (`data[2]`) are kept, start = here,
///   the cast list.
/// * 7 (stopped): visible on the map again, SELECTABLE, the wait list, task 0x1b, no spell.
/// * 0x11 kind 0xd (a move order): unless a ferry takes him (0x5800b0), the walk to `data[1]`
///   (list 252, start = here); no spell.
/// * 0x18 / 0x19: into / out of a vehicle (a ferry).
/// * Anything else: the normal update continues.
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    let pos = world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos);
    match ev.event {
        3 => {
            if let Some(j) = job_mut(world, i) {
                j.priest.spell = ev.kind;
                j.start = pos;
                j.destination = Y16X16(ev.data[2] as u32);
            }
            core::new_shared_list(world, i, LIST_CAST);
        }
        7 => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags = (e.flags & !flag::ON_BOARD) | flag::SELECTABLE;
            }
            core::take_wait_list(world, i);
            if let Some(j) = job_mut(world, i) {
                j.task = 0x1b;
                j.priest.spell = NO_SPELL;
            }
            // AI notice 0x15 (0x57faef).
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, ev.data[0]);
        }
        0x11 => {
            if ev.kind == 0xd {
                // A move order onto a ferry boards it (0x5800b0); the spell stays then.
                if crate::vehicles::ferry::settler_move_order(world, i, ev) {
                    return;
                }
                if let Some(j) = job_mut(world, i) {
                    j.start = pos;
                    j.destination = Y16X16(ev.data[1] as u32);
                }
                core::new_shared_list(world, i, LIST_MOVE);
            }
            set_spell(world, i, NO_SPELL);
        }
        // A vehicle (a ferry) calls him.
        0x18 => {
            crate::vehicles::ferry::settler_called(world, i, ev.data[0] as u16);
            set_spell(world, i, NO_SPELL);
        }
        // Ashore at data 1: task 0x1b, the AI notice (0x15 with his home), no spell, no home.
        0x19 => {
            crate::vehicles::ferry::settler_ashore(world, i, Y16X16(ev.data[1] as u32));
            if let Some(j) = job_mut(world, i) {
                j.task = 0x1b;
            }
            let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, home as i32);
            set_spell(world, i, NO_SPELL);
            if let Some(j) = job_mut(world, i) {
                j.home = 0;
            }
        }
        _ => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags |= core::FLAG_CONTINUE_AFTER_EVENT;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_of_the_neighbours() {
        // 0x5afad0 agrees with the neighbour table (direction d is NEIGHBOR[d]).
        let dirs: Vec<u8> = super::super::walking::NEIGHBOR.iter().map(|&(dx, dy)| direction_xy(dx, dy)).collect();
        assert_eq!(dirs, [0, 1, 2, 3, 4, 5]);
        assert_eq!(direction_xy(10, 1), 0);
        assert_eq!(direction_xy(-3, -7), 4);
    }
}
