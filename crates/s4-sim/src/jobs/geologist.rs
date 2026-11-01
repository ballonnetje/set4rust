//! `CGeologistRole` (release vtable 0x105b5f8, role 12, `SETTLER_GEOLOGIST` 49): the specialist
//! who looks for resources in the mountains where the player sends him and marks them with
//! signs. Decompiles in `tools/re/decompiled/script/geologist/`.
//!
//! The move order (event 0x11 kind 0xd, [`crate::group::send_to`]) makes the place his work
//! centre and starts `JOB_GEOLOGIST_WORK` (177: GO_TO_POS, SEARCH (31), WORK, RESOURCE_GATHERING
//! (13)); with mode bit 2 he only walks there and waits (`JOB_GEOLOGIST_FORCE_WALK`, 178). The
//! list starts over at its end (`GetNextJob` 0x57ab40 wraps).
//! * RESOURCE_GATHERING (TakeJob 0x57a890, search 0x57aef0): the next spot, among the next 64
//!   spiral entries around him the free mountain node (with a free node above it) nearest the
//!   work centre (a weighted distance, at most 32 candidates). The spot and the node above are
//!   reserved (flag 0x20) and he walks there. Without one he looks further out; after 57 rounds
//!   he waits.
//! * SEARCH (31): the spot must still be his (reserved) mountain node, else it is released and
//!   WORK is skipped.
//! * WORK (16): at the end (0x57b1b0) a sign for the resource under the node above him goes there
//!   (`SHIELD_*`, by resource type and amount), and his player may get a message.
//!
//! Ferries (`vehicles::ferry`): boarding on a move order (0x5800b0), events 0x18 / 0x19, CHECKIN
//! into the vehicle. (No event 0xd case: only the gardeners call role vtbl+0x80, and no code
//! sends event 0xd.)

use super::core::{self, job, job_mut, register};
use super::state::EntityEvent;
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;

/// `JOB_GEOLOGIST_WORK` / `JOB_GEOLOGIST_FORCE_WALK`.
const LIST_WORK: u16 = 177;
const LIST_FORCE_WALK: u16 = 178;
const RESERVED: u8 = 0x20;
const RESOURCE_GATHERING: u8 = 13;
const SEARCH: u8 = 31;
/// `SHIELD_EMPTY`.
const NO_SIGN: u16 = 0xc1;

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

/// Flag 0x20 on or off at `p` and the node above it.
fn reserve(world: &mut World, p: Y16X16, on: bool) {
    for (x, y) in [(p.x(), p.y()), (p.x(), p.y() - 1)] {
        if let Some(n) = world.grid.checked_index(x, y) {
            if on {
                world.layers.flags[n] |= RESERVED;
            } else {
                world.layers.flags[n] &= !RESERVED;
            }
        }
    }
}

/// The destination (+0x18) is the reserved spot: released when set.
fn release_spot(world: &mut World, i: u16) {
    let d = job(world, i).map_or(Y16X16(0), |j| j.destination);
    if d.0 != 0 {
        reserve(world, d, false);
        if let Some(j) = job_mut(world, i) {
            j.destination = Y16X16(0);
        }
    }
}

/// `CGeologistRole::InitWalking` (0x57a6f0) and `Init` (0x57a670): +0x12 = own id, flags
/// 0x32000, war map, no destination, centre, search; last message sign none.
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
        j.gardener.centre = (0, 0);
        j.geologist = Default::default();
    }
}

/// `CGeologistRole::GetNextJob` (0x57ab40): the next task, the list starting over at its end.
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

/// The deco object id at linear node `n + d` (0 off the layer).
fn deco_at(world: &World, n: usize, d: isize) -> u16 {
    n.checked_add_signed(d).and_then(|m| world.layers.deco_object.get(m)).copied().unwrap_or(0)
}

/// 0x57b0d0 `(x, y, reserved)`: a mountain node (ground 0x2X) with no deco object on it nor on
/// its six neighbours, none of flags 0x79 (with `reserved`: flag 0x20 set and none of 0x59), and
/// (unless `reserved`) no settler.
fn is_spot(world: &World, x: i32, y: i32, reserved: bool) -> bool {
    let Some(n) = world.grid.checked_index(x, y) else { return false };
    if world.layers.ground[n] & 0xf0 != 0x20 {
        return false;
    }
    let size = world.grid.size() as isize;
    if [0, -1, 1, -size, size, -size - 1, size + 1].iter().any(|&d| deco_at(world, n, d) != 0) {
        return false;
    }
    let f = world.layers.flags[n];
    let mask = if reserved {
        if f & RESERVED == 0 {
            return false;
        }
        0x59
    } else {
        0x79
    };
    f & mask == 0 && (world.layers.map_object[n] == 0 || reserved)
}

/// 0x57aef0: the next spot. His node and the one above must be on one sector. Among spiral
/// entries `spiral .. spiral + 64` around him (at most 32 candidates), the spot (`is_spot` at
/// the node and above) with the least `3 · ((87 ky)² + (50 (2kx − ky))²) + (87 (y − cy))² + (50
/// (2 (x − cx) − (y − cy)))²` (u32, wrapping) for its spiral offset (kx, ky) and the work centre
/// (cx, cy). It becomes the reserved destination and the search starts over.
fn search_next(world: &mut World, i: u16) -> bool {
    let Some(gd) = job(world, i).map(|j| j.gardener) else { return false };
    let here = pos(world, i);
    let sector = |w: &World, x: i32, y: i32| w.grid.checked_index(x, y).map(|n| w.tiling.sector_of(n));
    if sector(world, here.x(), here.y() - 1) != sector(world, here.x(), here.y()) {
        return false;
    }
    let (cx, cy) = gd.centre;
    let mut best: (u32, i32, i32) = (u32::MAX, -1, -1);
    let mut seen = 0;
    let mut k = gd.spiral;
    while k < gd.spiral + 0x40 {
        let (kx, ky) = (crate::spiral::dx(k as usize), crate::spiral::dy(k as usize));
        let (x, y) = (here.x() + kx, here.y() + ky);
        if is_spot(world, x, y, false) && is_spot(world, x, y - 1, false) {
            seen += 1;
            let a = (y - cy).wrapping_mul(0x57);
            let b = ((x - cx) * 2 - (y - cy)).wrapping_mul(0x32);
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
        if seen >= 0x20 {
            break;
        }
        k += 1;
    }
    let (_, x, y) = best;
    if x == -1 {
        return false;
    }
    release_spot(world, i);
    let spot = Y16X16::pack(x, y);
    reserve(world, spot, true);
    if let Some(j) = job_mut(world, i) {
        j.destination = spot;
        j.gardener.spiral = 0;
        j.gardener.rounds = 0;
    }
    true
}

/// `CGeologistRole::TakeJob` (0x57a890).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    // 0x580150 takes GO_TO_POS.
    if t.task == task_type::GO_TO_POS {
        super::gardener::retake_go(world, i, &t);
        return;
    }
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let counter = j.counter;
    let here = pos(world, i);
    match t.task {
        task_type::GO_TO_TARGET => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
                if t.x > 0 || t.y > 0 {
                    j.start = here;
                    j.destination = Y16X16(((t.y as i32) << 16).wrapping_add(t.x as i32) as u32);
                }
            }
            let dest = job(world, i).map_or(Y16X16(0), |j| j.destination);
            ActivePathing::walk_to_xy(world, i, dest, 0);
            if let Some(j) = job_mut(world, i) {
                j.task = task_type::GO;
                j.displacement_costs = 5;
            }
            core::go(world, i);
        }
        RESOURCE_GATHERING => {
            if !search_next(world, i) {
                let Some(j) = job_mut(world, i) else { return };
                j.gardener.spiral += 0x40;
                j.gardener.rounds += 1;
                if j.gardener.rounds > 0x38 {
                    j.task = task_type::WAIT;
                    register(world, i, 1);
                    return;
                }
                j.frame = 0;
                // Back to the list's start and two on: GetNextJob takes this task again.
                j.cursor = 2;
            }
            get_next_job(world, i);
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
        // Home vtbl+0x80: the vehicle he was called to takes him in.
        task_type::CHECKIN => {
            let home = job(world, i).map_or(0, |j| j.home);
            match world.entities.by_index(home as u32).map(|e| e.obj_type) {
                Some(ObjType::LandVehicle | ObjType::Ship) => crate::vehicles::settler_entered(world, home, i),
                Some(_) => world.events.push(crate::event::SimEvent::Unported { what: "CGeologistRole CHECKIN (home vtbl+0x80)", entity: i }),
                None => {}
            }
        }
        SEARCH => {
            if let Some(j) = job_mut(world, i) {
                j.geologist.tick += 1;
                if j.geologist.tick > 5 {
                    j.geologist.tick = 0;
                }
            }
            if !is_spot(world, here.x(), here.y(), true) || !is_spot(world, here.x(), here.y() - 1, true) {
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

/// `CGeologistRole::LogicUpdateJob` (0x57a750).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    let (task, dest) = (j.task, j.destination);
    match task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        task_type::WORK => {
            if dest.0 != 0 {
                place_sign(world, i);
                release_spot(world, i);
            }
            get_next_job(world, i);
        }
        RESOURCE_GATHERING | SEARCH => get_next_job(world, i),
        _ => {}
    }
}

/// 0x57b1b0: the sign for the resource at the node above him (resource type 1 coal, 2 iron,
/// 3 gold, 4 sulfur, 5 stone; level `(amount − 1) / 5`, 0..=2; else `SHIELD_EMPTY`), added
/// (`AddDecoObj`, grown) with its flags. For a resource: a rich one (level 2) sounds 0x26 at
/// him, and with the signs of that resource (types `base ..= base + 3`) in the spiral up to
/// radius 3 around him counted: 5 or more → message 0x78f.. (coal, gold, iron, stone, sulfur:
/// 0x78f, 0x790, 0x791, 0x792, 0x793); else, when the type differs from the last one reported
/// (+0x4c, then updated), 0x77f..0x783 in the same order. The release only does the message part
/// for the local player's geologists; here every message names its player.
fn place_sign(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (x, y, owner) = (e.x(), e.y(), e.owner);
    let (sx, sy) = (x, y - 1);
    let Some(n) = world.grid.checked_index(sx, sy) else { return };
    let r = world.layers.resource[n];
    let amount = (r & 0xf) as i32;
    let level = (if amount == 0 { 0 } else { amount - 1 }) / 5;
    let base: u16 = match r & 0xf0 {
        0x10 => 0xc2,
        0x20 => 0xc8,
        0x30 => 0xc5,
        0x40 => 0xce,
        0x50 => 0xcb,
        _ => NO_SIGN,
    };
    let kind = if base == NO_SIGN { NO_SIGN } else { base + level as u16 };
    let _ = crate::deco::add_deco_obj_without_flags(world, sx, sy, kind, 0);
    crate::deco::set_flags_for_object(world, sx, sy, kind);
    if kind == NO_SIGN {
        return;
    }
    if level == 2 {
        world.events.push(crate::event::SimEvent::Effect { effect: 0, sound: 0x26, x, y });
    }
    let count = crate::spiral::walk(world.grid, x, y, 3)
        .filter(|&(px, py)| {
            let id = world.layers.deco_object[world.grid.index(px as u32, py as u32)];
            id != 0 && world.entities.by_index(id as u32).is_some_and(|d| d.obj_type == ObjType::Deco && (base..=base + 3).contains(&d.kind))
        })
        .count();
    let slot = match base {
        0xc2 => 0,
        0xc5 => 1,
        0xc8 => 2,
        0xcb => 3,
        _ => 4,
    };
    let text = if count >= 5 {
        Some(0x78f + slot)
    } else if count != 0 && job(world, i).is_some_and(|j| j.geologist.last_sign != base) {
        if let Some(j) = job_mut(world, i) {
            j.geologist.last_sign = base;
        }
        Some(0x77f + slot)
    } else {
        None
    };
    if let Some(text) = text {
        world.events.push(crate::event::SimEvent::Message { player: owner, text, x, y, filtered: true });
    }
}

/// `CGeologistRole::ConvertEventIntoGoal` (0x57ab80). Every event first resets the last
/// message's sign (+0x4c).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    if let Some(j) = job_mut(world, i) {
        j.geologist.last_sign = NO_SIGN;
    }
    match ev.event {
        // Stop: waiting, flags 0x8000 off and 0x2000 on, task 0x1b, the AI notice.
        7 => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags = (e.flags & !0x8000) | 0x2000;
            }
            core::take_wait_list(world, i);
            if let Some(j) = job_mut(world, i) {
                j.task = 0x1b;
            }
            // The AI notice (0x15, owner, settler, the event's first word).
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, ev.data[0]);
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
        // A vehicle (a ferry) calls him.
        0x18 => {
            crate::vehicles::ferry::settler_called(world, i, ev.data[0] as u16);
        }
        // Ashore at data 1: task 0x1b, the AI notice (0x15 with his home), then no home.
        0x19 => {
            crate::vehicles::ferry::settler_ashore(world, i, Y16X16(ev.data[1] as u32));
            if let Some(j) = job_mut(world, i) {
                j.task = 0x1b;
            }
            let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, home as i32);
            if let Some(j) = job_mut(world, i) {
                j.home = 0;
            }
        }
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1);
            }
        }
    }
}

/// The signs on the map (for tests and tools): `(node, kind)` of every node whose deco object is
/// a `SHIELD_*` (193..=208; they are shared static instances).
pub fn signs(world: &World) -> Vec<(Y16X16, u16)> {
    let size = world.grid.size() as usize;
    (0..world.layers.deco_object.len())
        .filter_map(|n| {
            let id = world.layers.deco_object[n];
            let e = world.entities.by_index(id as u32).filter(|_| id != 0)?;
            (e.obj_type == ObjType::Deco && (0xc1..=0xd0).contains(&e.kind)).then(|| (Y16X16::pack((n % size) as i32, (n / size) as i32), e.kind))
        })
        .collect()
}
