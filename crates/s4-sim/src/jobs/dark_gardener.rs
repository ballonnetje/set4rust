//! `CDarkGardenerRole` (release vtable 0x105ad04, role 14, `SETTLER_DARKGARDENER` 52): the Dark
//! Tribe's specialist who plants shadow herbs (deco 216) where the player sends him. Decompiles in
//! `tools/re/decompiled/script/dark/darkgardener.c`, `darkgardener2.c`.
//!
//! The player's move order (`CGroupMgr::SendTo`, event 0x11 kind 0xd, [`crate::group::send_to`])
//! makes the place his work centre and starts `JOB_DARKGARDENER_WORK` (160): GO_TO_POS, WORK
//! (26 ticks), task 31; the list then starts over (`GetNextJob` wraps). With mode bit 2 he only
//! walks there and waits (`JOB_DARKGARDENER_FORCE_WALK`, 241).
//! * WORK (TakeJob 0x574bc0): a free node among his six neighbours (no flags 0x3F, no settler,
//!   no object, ground not 0x2X, room for a herb) becomes the spot; he faces it. At the end
//!   (0x574ac0) the herb goes in (`AddDecoObj(spot, 216, 1)`) and his own node darkens
//!   (`CDarkTribe` vtbl+0xc without the ring).
//! * Task 31 (0x5750a0): the next place, among the next 64 spiral entries around him the free
//!   node of his sector nearest the work centre (a weighted distance, at most 32 candidates), is
//!   reserved (flag 0x20) and walked to. Without one he looks further out; after 57 rounds he
//!   gives up and waits.

use super::core::{self, job, job_mut, register};
use super::state::{EntityEvent, GardenerData};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::flag;
use crate::grid::Y16X16;

/// `JOB_DARKGARDENER_WORK` / `JOB_DARKGARDENER_FORCE_WALK`.
const LIST_WORK: u16 = 160;
const LIST_FORCE_WALK: u16 = 241;
const SHADOW_HERB: u16 = 216;
/// Node flag 0x20: reserved as somebody's target.
const RESERVED: u8 = 0x20;
/// Task 31: look for the next place.
const NEXT_PLACE: u8 = 31;

fn g(world: &mut World, i: u16) -> Option<&mut GardenerData> {
    job_mut(world, i).map(|j| &mut j.gardener)
}

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

fn clear_reserved(world: &mut World, spot: (i32, i32)) {
    if spot.0 != -1
        && let Some(n) = world.grid.checked_index(spot.0, spot.1)
    {
        world.layers.flags[n] &= !RESERVED;
    }
}

/// A node a herb or the gardener may take: in the world, none of flags 0x3F, no settler, no deco
/// object, ground not 0x2X.
fn free_node(world: &World, x: i32, y: i32) -> Option<usize> {
    let n = world.grid.checked_index(x, y)?;
    let l = &world.layers;
    (l.flags[n] & 0x3f == 0 && l.map_object[n] == 0 && l.deco_object[n] == 0 && l.ground[n] & 0xf0 != 0x20).then_some(n)
}

/// `CDarkGardenerRole::InitWalking` (0x574a60: `CWalkingNormal` of the owner) and `Init`
/// (0x5749f0: war map, flags 0x32000, no spot, no centre, +0x22 = 0, +0x12 = own id).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    super::hooks::war_map_add(world, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= 0x32000;
    }
    if let Some(j) = job_mut(world, i) {
        j.gardener = GardenerData { centre: (0, 0), spot: (-1, -1), rounds: 0, spiral: 0 };
        j.entity = 0;
        j.w18 = i;
    }
}

/// `CDarkGardenerRole::GetNextJob` (0x574ed0): the next task, the list starting over at its end.
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

/// `CDarkGardenerRole::TakeJob` (0x574bc0).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, destination) = (j.counter, j.destination);
    match t.task {
        task_type::GO_TO_POS => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            // A positive task offset first makes the offset the destination, then the offset is
            // added to the destination as well (0x574dbf; the gardener's lists hold 0, 0).
            let (x, y) = (t.x as i32, t.y as i32);
            let mut dest = destination.0 as i32;
            if x > 0 || y > 0 {
                let here = pos(world, i);
                if let Some(j) = job_mut(world, i) {
                    j.start = here;
                }
                dest = (y << 16).wrapping_add(x);
            }
            let dest = ((y + (dest >> 16)) << 16).wrapping_add(dest & 0xffff).wrapping_add(x);
            if let Some(j) = job_mut(world, i) {
                j.destination = Y16X16(dest as u32);
            }
            ActivePathing::walk_to_xy(world, i, Y16X16(dest as u32), 0);
            if let Some(j) = job_mut(world, i) {
                j.task = task_type::GO;
                j.displacement_costs = 5;
            }
            core::go(world, i);
        }
        task_type::WORK => {
            let spot = g(world, i).map_or((-1, -1), |g| g.spot);
            clear_reserved(world, spot);
            if let Some(g) = g(world, i) {
                g.spot = (-1, -1);
            }
            let here = pos(world, i);
            let found = (1..=6).map(|k| (here.x() + crate::spiral::dx(k), here.y() + crate::spiral::dy(k))).find(|&(x, y)| {
                free_node(world, x, y).is_some() && crate::deco::is_add_deco_obj_ok(world, x, y, SHADOW_HERB, 0)
            });
            let Some((x, y)) = found else {
                register(world, i, 1);
                return;
            };
            if let Some(g) = g(world, i) {
                g.spot = (x, y);
            }
            let dir = crate::pathing::walking::direction(here.0 as i32, Y16X16::pack(x, y).0 as i32) as u8;
            if let Some(s) = core::settler_mut(world, i) {
                s.direction = dir;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter as i32);
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            let r = world.rng.rand();
            register(world, i, (r & 0xf) as i32 + 1);
        }
        NEXT_PLACE => {
            if next_place(world, i) {
                register(world, i, 1);
                return;
            }
            let Some(g) = g(world, i) else { return };
            g.spiral += 0x40;
            g.rounds += 1;
            if g.rounds < 0x39 {
                if let Some(j) = job_mut(world, i) {
                    j.frame = 0;
                }
                get_next_job(world, i);
            } else {
                core::take_wait_list(world, i);
            }
        }
        _ => {}
    }
}

/// 0x5750a0: among spiral entries `spiral .. spiral + 64` around the gardener, the free node of
/// his sector (at most 32 looked at) with the least weighted distance to the work centre and of
/// its own offset; it is reserved (the old spot released), the search starts over, and he walks
/// there.
fn next_place(world: &mut World, i: u16) -> bool {
    let Some(gd) = job(world, i).map(|j| j.gardener) else { return false };
    let here = pos(world, i);
    let Some(hn) = world.grid.checked_index(here.x(), here.y()) else { return false };
    let sector = world.tiling.sector_of(hn);
    // (score, x, y); a candidate must score below the best so far (initially 0xffffffff).
    let mut best: (u32, i32, i32) = (u32::MAX, -1, -1);
    let mut seen = 0;
    for k in gd.spiral..gd.spiral + 0x40 {
        let (kx, ky) = (crate::spiral::dx(k as usize), crate::spiral::dy(k as usize));
        let (x, y) = (here.x() + kx, here.y() + ky);
        if !world.grid.in_world(x, y) {
            continue;
        }
        if let Some(n) = free_node(world, x, y)
            && world.tiling.sector_of(n) == sector
        {
            seen += 1;
            let dy = y - gd.centre.1;
            let a = dy.wrapping_mul(0x57);
            let b = ((x - gd.centre.0) * 2 - dy).wrapping_mul(0x32);
            let c = (kx * 2 - ky).wrapping_mul(0x32);
            let e = ky.wrapping_mul(0x57);
            let score = (e.wrapping_mul(e) as u32)
                .wrapping_add((a.wrapping_mul(a).wrapping_add(b.wrapping_mul(b)) as u32).wrapping_mul(4))
                .wrapping_add(c.wrapping_mul(c) as u32);
            if score < best.0 {
                best = (score, x, y);
            }
        }
        if seen > 0x1f {
            break;
        }
    }
    let (_, x, y) = best;
    if x == -1 {
        return false;
    }
    clear_reserved(world, gd.spot);
    if let Some(n) = world.grid.checked_index(x, y) {
        world.layers.flags[n] |= RESERVED;
    }
    if let Some(j) = job_mut(world, i) {
        j.gardener.spot = (x, y);
        j.gardener.spiral = 0;
        j.gardener.rounds = 0;
        j.start = here;
        j.destination = Y16X16::pack(x, y);
    }
    true
}

/// `CDarkGardenerRole::LogicUpdateJob` (0x574ac0).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(task) = job(world, i).map(|j| j.task) else { return };
    match task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        task_type::WORK => {
            let spot = g(world, i).map_or((-1, -1), |g| g.spot);
            if spot.0 != -1 && crate::deco::is_add_deco_obj_ok(world, spot.0, spot.1, SHADOW_HERB, 0) {
                // `AddDecoObj(x, y, 216, 1)` (0x563300).
                let _ = crate::deco::add_deco_obj_without_flags(world, spot.0, spot.1, SHADOW_HERB, 1);
                crate::deco::set_flags_for_object(world, spot.0, spot.1, SHADOW_HERB);
                let here = pos(world, i);
                crate::dark::darken_around(world, here.x(), here.y(), false);
                if let Some(g) = g(world, i) {
                    g.spot = (-1, -1);
                }
            } else {
                let here = pos(world, i);
                crate::dark::darken_around(world, here.x(), here.y(), false);
            }
            get_next_job(world, i);
        }
        NEXT_PLACE => get_next_job(world, i),
        _ => {}
    }
}

/// `CDarkGardenerRole::ConvertEventIntoGoal` (0x574f10).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // Event 0xd → vtbl+0x80 (0x586e40: GO_TO_POS to (data0, data1), then WAIT; flag 2
        // appends): no code of the release makes an entity event 0xd (every CEntityEvent
        // construction, vtable 0x102a49c, checked: docs/JOBS.md), so this never runs.
        0xd => {}
        0x11 => {
            let spot = g(world, i).map_or((-1, -1), |g| g.spot);
            if spot.0 != -1 {
                clear_reserved(world, spot);
                if let Some(g) = g(world, i) {
                    g.spot = (-1, -1);
                }
            }
            let (mode, place) = (ev.data[0] as u32, Y16X16(ev.data[1] as u32));
            if ev.kind != 0xd || mode & 3 != 0 {
                return;
            }
            let here = pos(world, i);
            if let Some(j) = job_mut(world, i) {
                j.start = here;
                j.destination = place;
                if mode & 4 == 0 {
                    j.gardener.centre = (place.x(), place.y());
                    j.gardener.spiral = 0;
                    j.gardener.rounds = 0;
                }
            }
            core::new_shared_list(world, i, if mode & 4 == 0 { LIST_WORK } else { LIST_FORCE_WALK });
        }
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1);
            }
        }
    }
}
