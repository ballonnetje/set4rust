//! `CPioneerRole` (release vtable 0x105bc94, role 11, `SETTLER_PIONEER` 47): the specialist who
//! claims land for his player node by node where the player sends him. Decompiles in
//! `tools/re/decompiled/script/pioneer/`.
//!
//! The move order (event 0x11 kind 0xd, [`crate::group::send_to`]) makes the place his work
//! centre and starts `JOB_PIONEER_WORK` (192: GO_TO_POS, SEARCH (31), WORK (16),
//! RESOURCE_GATHERING (13)); with mode bit 2 he only walks there and waits
//! (`JOB_PIONEER_FORCE_WALK`, 193). `GetNextJob` (0x57ea20) wraps at the list's end.
//! * RESOURCE_GATHERING (TakeJob 0x57e760, search 0x57edf0): among the next 32 spiral entries
//!   around him, the claimable node ([`claimable`] 0x57efb0) nearest the work centre (the
//!   geologist's weighted distance, at most 17 candidates; a node off the map ends the search
//!   with nothing). It is reserved (flag 0x20) and becomes his destination; the search cursor
//!   becomes `cursor % 6`. Without one the cursor moves on by 32 and he looks again after a tick
//!   from the list's second task (LogicUpdateJob 13); after 29 rounds he goes idle (his wait
//!   list).
//! * SEARCH (31): the node he stands on must still be claimable (without the reservation and
//!   map-object tests), else the reservation goes and WORK is skipped.
//! * WORK (16): at the end (0x57f0a0) the node he stands on becomes his player's
//!   (`CTiling` vtbl+0x1c 0x5b79a0 = [`World::change_owner`], no `SetOwner`) and its reservation
//!   goes; the list then skips RESOURCE_GATHERING and starts over with GO_TO_POS to the same
//!   place, whose SEARCH fails on his own land and so leads to the next RESOURCE_GATHERING.
//!
//! Ferries (`vehicles::ferry`): boarding on a move order (0x5800b0), events 0x18 / 0x19, CHECKIN
//! into the vehicle; tasks 10 / 0x25 walk with flag 0x2000 after a landing (+0x14 bit 0x100000,
//! set by event 0x18, cleared by every TakeJob). Not ported: the role's own task list (+0x40, only filled by `Load` 0x57e290 from a save) and the
//! destructor (0x57e190: war map, reservation, home observer).

use super::core::{self, job, job_mut, register};
use super::state::EntityEvent;
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::flag;
use crate::grid::Y16X16;

/// `JOB_PIONEER_WORK` / `JOB_PIONEER_FORCE_WALK`.
const LIST_WORK: u16 = 192;
const LIST_FORCE_WALK: u16 = 193;
const RESERVED: u8 = 0x20;
const RESOURCE_GATHERING: u8 = 13;
const SEARCH: u8 = 31;
/// Task 0x25: a walk to the target with flag 0x2000 (after a landing).
const GO_TO_TARGET_SHIP: u8 = 0x25;
/// Entity flag 0x100000: he came off a ship (event 0x18); cleared by every TakeJob.
const LANDED: u32 = 0x10_0000;

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

fn set_reserved(world: &mut World, p: Y16X16, on: bool) {
    if let Some(n) = world.grid.checked_index(p.x(), p.y()) {
        if on {
            world.layers.flags[n] |= RESERVED;
        } else {
            world.layers.flags[n] &= !RESERVED;
        }
    }
}

/// The destination (+0x18) is the reserved node: released when set.
fn release_spot(world: &mut World, i: u16) {
    let d = job(world, i).map_or(Y16X16(0), |j| j.destination);
    if d.0 != 0 {
        set_reserved(world, d, false);
        if let Some(j) = job_mut(world, i) {
            j.destination = Y16X16(0);
        }
    }
}

/// `CPioneerRole::InitWalking` (0x57e5c0), the constructor (0x57e0a0: work centre (−1, −1),
/// no destination) and `Init` (0x57e580: +0x12 = own id, flags 0x32000, war map).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= 0x32000;
    }
    super::hooks::war_map_add(world, i);
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
        j.destination = Y16X16(0);
        j.gardener = Default::default();
        j.gardener.centre = (-1, -1);
    }
}

/// `CPioneerRole::GetNextJob` (0x57ea20): the next task, the list starting over at its end.
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

/// 0x57efb0 `(x, y, settler, search)`: whether the pioneer may claim the node. It must be on his
/// node's sector and have none of flags 0xb9 (searching; 0x99 otherwise: a reservation is fine)
/// and, when searching, no map object. Unowned land is claimable; land of another team only on
/// its border (render byte 3 bit 0x80, the border-stone bit); his own team's land never. (The
/// release does not check the node is on the map; the callers pass map nodes.)
pub fn claimable(world: &World, x: i32, y: i32, i: u16, search: bool) -> bool {
    let Some(e) = world.entities.by_index(i as u32) else { return false };
    let (Some(here), Some(n)) = (world.grid.checked_index(e.x(), e.y()), world.grid.checked_index(x, y)) else { return false };
    if world.tiling.sector_of(n) != world.tiling.sector_of(here) {
        return false;
    }
    let mask = if search { 0xb9 } else { 0x99 };
    if world.layers.flags[n] & mask != 0 || (search && world.layers.map_object[n] != 0) {
        return false;
    }
    let owner = world.owner_of(n);
    if owner == 0 {
        return true;
    }
    world.alliances.team(owner) != world.alliances.team(e.owner) && world.layers.gfx_bits[n] & 0x80 != 0
}

/// 0x57edf0: the next node to claim. Among spiral entries `spiral .. spiral + 32` around him (at
/// most 17 candidates; a node off the map ends the search with nothing), the claimable node on
/// his sector with the least `3 · ((87 ky)² + (50 (2kx − ky))²) + (87 (y − cy))² + (50 (2 (x − cx)
/// − (y − cy)))²` (u32, wrapping) for its spiral offset (kx, ky) and the work centre (cx, cy). It
/// is reserved and becomes the destination; rounds 0, the spiral cursor `cursor % 6`.
fn search_next(world: &mut World, i: u16) -> bool {
    let Some(gd) = job(world, i).map(|j| j.gardener) else { return false };
    let here = pos(world, i);
    let Some(hn) = world.grid.checked_index(here.x(), here.y()) else { return false };
    let sector = world.tiling.sector_of(hn);
    let size = world.grid.size() as i32;
    let (cx, cy) = gd.centre;
    let mut best: (u32, i32, i32) = (u32::MAX, -1, -1);
    let mut seen = 0;
    let mut k = gd.spiral;
    while k < gd.spiral + 0x20 {
        let (kx, ky) = (crate::spiral::dx(k as usize), crate::spiral::dy(k as usize));
        let (x, y) = (here.x() + kx, here.y() + ky);
        if !(0..size).contains(&x) || !(0..size).contains(&y) {
            return false;
        }
        let n = world.grid.index(x as u32, y as u32);
        if world.tiling.sector_of(n) == sector && claimable(world, x, y, i, true) {
            seen += 1;
            let a = (y - cy).wrapping_mul(0x57);
            let b = ((x - cx).wrapping_mul(2).wrapping_sub(y - cy)).wrapping_mul(0x32);
            let c = (kx * 2 - ky).wrapping_mul(0x32);
            let e = ky.wrapping_mul(0x57);
            let score = ((e.wrapping_mul(e).wrapping_add(c.wrapping_mul(c))) as u32)
                .wrapping_mul(3)
                .wrapping_add(a.wrapping_mul(a) as u32)
                .wrapping_add(b.wrapping_mul(b) as u32);
            if score < best.0 {
                best = (score, x, y);
            }
        }
        if seen >= 0x11 {
            break;
        }
        k += 1;
    }
    let (_, x, y) = best;
    if x == -1 {
        return false;
    }
    let spot = Y16X16::pack(x, y);
    set_reserved(world, spot, true);
    if let Some(j) = job_mut(world, i) {
        j.destination = spot;
        j.gardener.rounds = 0;
        j.gardener.spiral = gd.spiral % 6;
    }
    true
}

/// `CPioneerRole::TakeJob` (0x57e760).
pub fn take_job(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index_mut(i as u32) else { return };
    let landed = e.flags & LANDED != 0;
    e.flags &= !LANDED;
    let Some(t) = core::actual_task(world, i) else { return };
    // 0x580150 takes GO_TO_POS.
    if t.task == task_type::GO_TO_POS {
        super::gardener::retake_go(world, i, &t);
        return;
    }
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let counter = j.counter;
    match t.task {
        task_type::GO_TO_TARGET | GO_TO_TARGET_SHIP => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            let dest = job(world, i).map_or(Y16X16(0), |j| j.destination);
            let flags = if landed || t.task == GO_TO_TARGET_SHIP { 0x2000 } else { 0 };
            ActivePathing::walk_to_xy(world, i, dest, flags);
            if let Some(j) = job_mut(world, i) {
                j.task = task_type::GO;
                j.displacement_costs = 5;
            }
            core::go(world, i);
        }
        RESOURCE_GATHERING => {
            if search_next(world, i) {
                get_next_job(world, i);
                return;
            }
            let Some(j) = job_mut(world, i) else { return };
            j.gardener.spiral += 0x20;
            j.gardener.rounds += 1;
            if j.gardener.rounds > 0x1c {
                core::take_wait_list(world, i);
                return;
            }
            register(world, i, 1);
        }
        task_type::WORK => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter as i32);
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            register(world, i, 1);
        }
        // Home vtbl+0x80: a vehicle he was called to takes him in (`CCart` 0x55d0c0).
        task_type::CHECKIN => {
            let home = job(world, i).map_or(0, |j| j.home);
            match world.entities.by_index(home as u32).map(|e| e.obj_type) {
                Some(crate::entity::ObjType::LandVehicle | crate::entity::ObjType::Ship) => crate::vehicles::settler_entered(world, home, i),
                Some(_) => world.events.push(crate::event::SimEvent::Unported { what: "CPioneerRole CHECKIN (home vtbl+0x80)", entity: i }),
                None => {}
            }
        }
        SEARCH => {
            let here = pos(world, i);
            if !claimable(world, here.x(), here.y(), i, false) {
                release_spot(world, i);
                if let Some(j) = job_mut(world, i) {
                    j.cursor += 1;
                }
                get_next_job(world, i);
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter as i32);
        }
        _ => {}
    }
}

/// `CPioneerRole::LogicUpdateJob` (0x57e620).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(task) = job(world, i).map(|j| j.task) else { return };
    match task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        // The list from its second task.
        RESOURCE_GATHERING => {
            if let Some(j) = job_mut(world, i) {
                j.cursor = 0;
            }
            get_next_job(world, i);
        }
        task_type::WORK => {
            claim(world, i);
            if let Some(j) = job_mut(world, i) {
                j.cursor += 1;
            }
            get_next_job(world, i);
        }
        SEARCH => get_next_job(world, i),
        _ => {}
    }
}

/// 0x57f0a0: the node he stands on becomes his player's (`CTiling` vtbl+0x1c) and loses its
/// reservation.
fn claim(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (x, y, owner) = (e.x(), e.y(), e.owner);
    world.change_owner(x, y, owner);
    set_reserved(world, Y16X16::pack(x, y), false);
}

/// `CPioneerRole::ConvertEventIntoGoal` (0x57ea60).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // Stop: flags 0x8000 off and 0x2000 on, the wait list, the AI notice.
        7 => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags = (e.flags & !0x8000) | 0x2000;
            }
            core::take_wait_list(world, i);
            // The AI notice (0x15, owner, settler, the event's first word).
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, ev.data[0]);
        }
        // Flag 0x2000, then SetFree (vtbl+0x40) with the event's eco sector.
        9 => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags |= 0x2000;
            }
            super::roles::set_free(world, i, ev.data[1]);
        }
        0x11 => {
            release_spot(world, i);
            // A move order onto a ferry boards it (0x5800b0).
            if ev.kind != 0xd || crate::vehicles::ferry::settler_move_order(world, i, ev) {
                return;
            }
            let (mode, place) = (ev.data[0] as u32, Y16X16(ev.data[1] as u32));
            let here = pos(world, i);
            if let Some(j) = job_mut(world, i) {
                if mode & 4 == 0 {
                    j.gardener.rounds = 0;
                    j.gardener.spiral = 0;
                    j.gardener.centre = (place.x(), place.y());
                }
                j.start = here;
                j.destination = place;
            }
            core::new_shared_list(world, i, if mode & 4 == 0 { LIST_WORK } else { LIST_FORCE_WALK });
        }
        // A vehicle calls him (a foundation cart, 0x583130): as the carrier's event 24, and he
        // leaves his groups (`CGroupMgr` vtbl+0x1c).
        0x18 => {
            let vehicle = ev.data[0] as u16;
            let place = crate::vehicles::exit_place(world, vehicle, i);
            let alive = world.entities.by_index(vehicle as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0);
            if place == 0 || !alive {
                if let Some(e) = world.entities.by_index_mut(i as u32) {
                    e.flags |= 0x2000;
                }
                core::take_wait_list(world, i);
                return;
            }
            let here = pos(world, i);
            if let Some(j) = job_mut(world, i) {
                j.destination = Y16X16(place as u32);
                j.start = here;
            }
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags |= LANDED;
            }
            core::take_anim_list(world, i, 0);
            crate::vehicles::attach(world, vehicle, i);
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags &= !0x2200;
            }
            if crate::group::word(world, i) != 0 {
                crate::group::remove_all(world, i);
            }
            let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
            crate::ai::post_event(world, 0x12, owner as i32, i as i32, home as i32);
        }
        // Out of the vehicle at data 1 (0x582a40), into the war map, off board, visible and
        // selectable, the wait list, the AI notice (0x15 with his home).
        0x19 => {
            let at = Y16X16(ev.data[1] as u32);
            crate::vehicles::ferry::settler_ashore(world, i, at);
            core::take_wait_list(world, i);
            let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, home as i32);
        }
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1);
            }
        }
    }
}
