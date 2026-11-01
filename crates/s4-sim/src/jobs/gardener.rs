//! `CGardenerRole` (release vtable 0x105b51c, role 13, `SETTLER_GARDENER` 50): the other
//! tribes' specialist who turns dark land light again and beats the Dark Tribe's mushrooms where
//! the player sends him. Decompiles in `tools/re/decompiled/script/dark/gardener.c`,
//! `gardener2.c`.
//!
//! The move order (event 0x11 kind 0xd, [`crate::group::send_to`]) makes the place his work
//! centre and starts `JOB_GARDENER_BEAT_MUSHROOM` (174); mode bit 2 only walks there and waits
//! (`JOB_GARDENER_FORCE_WALK`, 176). The work lists (GO_TO_POS, the work, task 31) start over
//! at their end (`GetNextJob` 0x579a40 wraps).
//! * Task 31 (TakeJob 0x579660): the next target, nearest the work centre among the next 48
//!   spiral entries around him in his sector (0x579f80): first a node below-left of a ripe static
//!   mushroom (→ list 174), else a reserved-free dark node (→ `JOB_GARDENER_WORK` 175); it is
//!   reserved (flag 0x20). Without one he looks further out; after 57 rounds he waits.
//! * RESOURCE_GATHERING (13), the beating: with the mushroom still there (0x579e70) the static
//!   one gets a dynamic copy (0x565bd0) as his target; at the end it is hit
//!   ([`crate::deco::mushroom::hit`]).
//! * WORK (16), on a reserved dark node: at the end the node turns light (`CDarkTribe` vtbl+0x10
//!   for his player, [`crate::dark::lighten`]).
//! * GO_TO_POS is re-taken by 0x580150 (the walk to the destination, keeping the frame when the
//!   job did not change).
//!
//! Ferries (`vehicles::ferry`): boarding on a move order (0x5800b0), events 0x18 / 0x19, CHECKIN
//! into the vehicle. Event 0xd's walk list (0x586e40) is never sent by the release.

use super::core::{self, job, job_mut, register};
use super::state::{EntityEvent, GardenerData};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::grid::Y16X16;

/// `JOB_GARDENER_BEAT_MUSHROOM` / `JOB_GARDENER_WORK` / `JOB_GARDENER_FORCE_WALK`.
const LIST_BEAT: u16 = 174;
const LIST_WORK: u16 = 175;
const LIST_FORCE_WALK: u16 = 176;
/// Node flag 0x20: reserved as somebody's target.
const RESERVED: u8 = 0x20;
const RESOURCE_GATHERING: u8 = 13;
/// Task 31: look for the next target.
const NEXT_TARGET: u8 = 31;

fn g(world: &mut World, i: u16) -> Option<&mut GardenerData> {
    job_mut(world, i).map(|j| &mut j.gardener)
}

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

fn clear_reserved(world: &mut World, x: i32, y: i32) {
    if let Some(n) = world.grid.checked_index(x, y) {
        world.layers.flags[n] &= !RESERVED;
    }
}

/// `CGardenerRole::InitWalking` (0x579410: `CWalkingNormal` of the owner) and `Init` (0x5793a0:
/// +0x12 = own id, flags 0x32000, war map, no spot, no centre, no target).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= 0x32000;
    }
    super::hooks::war_map_add(world, i);
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
        j.gardener = GardenerData::default();
        j.entity = 0;
    }
}

/// `CGardenerRole::GetNextJob` (0x579a40): the next task, the list starting over at its end.
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

/// 0x579e70 `(x, y, kind, reserved)`: whether node `(x, y)` is a target. Not reserved (with
/// `reserved`: reserved) and — kind 0 — below-left of a READY static mushroom (212..=214 at
/// `(x + 1, y − 1)`), or — kind 1 — not blocked (bit 0), without a settler (unless `reserved`)
/// and dark.
fn is_target(world: &World, x: i32, y: i32, kind: u8, reserved: bool) -> bool {
    let Some(n) = world.grid.checked_index(x, y) else { return false };
    let f = world.layers.flags[n];
    let mut mask = if kind != 0 { 1 } else { 0 };
    if !reserved {
        mask |= RESERVED;
    } else if f & RESERVED == 0 {
        return false;
    }
    if mask & f != 0 {
        return false;
    }
    match kind {
        0 => {
            let Some(m) = world.grid.checked_index(x + 1, y - 1) else { return false };
            let id = world.layers.deco_object[m];
            world.entities.by_index(id as u32).is_some_and(|e| {
                e.obj_type == ObjType::Deco
                    && e.flags & flag::READY != 0
                    && crate::deco::mushroom::is_mushroom(e.kind)
                    && matches!(&e.data, EntityData::Deco(d) if d.static_instance)
            })
        }
        1 => (world.layers.map_object[n] == 0 || reserved) && f & 4 != 0,
        _ => false,
    }
}

/// 0x579f80 `(kind)`: the target of `kind` among spiral entries `spiral .. spiral + 48` around
/// him, in his sector (at most 32 looked at), with the least weighted distance to the work
/// centre and of its own offset. It is reserved (the old spot released), the search starts
/// over, and he starts list 174 (mushrooms) or 175 (dark land) to it.
fn find_target(world: &mut World, i: u16, kind: u8) -> bool {
    let Some(gd) = job(world, i).map(|j| j.gardener) else { return false };
    let here = pos(world, i);
    let Some(hn) = world.grid.checked_index(here.x(), here.y()) else { return false };
    let sector = world.tiling.sector_of(hn);
    let mut best: (u32, i32, i32) = (u32::MAX, -1, -1);
    let mut seen = 0;
    for k in gd.spiral..gd.spiral + 0x30 {
        let (kx, ky) = (crate::spiral::dx(k as usize), crate::spiral::dy(k as usize));
        let (x, y) = (here.x() + kx, here.y() + ky);
        let Some(n) = world.grid.checked_index(x, y) else { continue };
        if world.tiling.sector_of(n) != sector {
            continue;
        }
        if is_target(world, x, y, kind, false) {
            seen += 1;
            let a = (y - gd.centre.1).wrapping_mul(0x57);
            let b = ((x - gd.centre.0) * 2 - (y - gd.centre.1)).wrapping_mul(0x32);
            let c = (kx * 2 - ky).wrapping_mul(0x32);
            let e = ky.wrapping_mul(0x57);
            let score = (a.wrapping_mul(a) as u32)
                .wrapping_add((e.wrapping_mul(e).wrapping_add(c.wrapping_mul(c)) as u32).wrapping_mul(2))
                .wrapping_add(b.wrapping_mul(b) as u32);
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
    if gd.spot.0 != -1 {
        clear_reserved(world, gd.spot.0, gd.spot.1);
    }
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
    core::new_shared_list(world, i, if kind == 0 { LIST_BEAT } else { LIST_WORK });
    true
}

/// 0x580150: GO_TO_POS taken again — the frame stays when the gfx job did not change (and is
/// within the cycle), the walk goes to the destination; with no events queued the first step is
/// taken now, else next tick.
pub(super) fn retake_go(world: &mut World, i: u16, t: &super::task::EntityTask) {
    let Some(j) = job(world, i) else { return };
    let (frame, job_part) = (j.frame, j.job_part);
    core::init_common_task_values(world, i, t);
    let Some(j) = job_mut(world, i) else { return };
    j.frame = if j.job_part == job_part && frame != 0 && frame < j.cycle_frames { frame } else { 1 };
    let dest = j.destination;
    ActivePathing::walk_to_xy(world, i, dest, 0);
    let events = job(world, i).is_some_and(|j| !j.events.is_empty());
    if let Some(j) = job_mut(world, i) {
        j.displacement_costs = 5;
        j.task = task_type::GO;
    }
    if !events {
        core::go(world, i);
    } else {
        if let Some(j) = job_mut(world, i) {
            j.walk = 0x88;
        }
        register(world, i, 1);
    }
}

/// `CGardenerRole::TakeJob` (0x579660).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    if t.task == task_type::GO_TO_POS {
        retake_go(world, i, &t);
        return;
    }
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, destination) = (j.counter, j.destination);
    let here = pos(world, i);
    match t.task {
        task_type::GO_TO_TARGET => {
            if let Some(j) = job_mut(world, i) {
                j.frame = 1;
            }
            // As the dark gardener's GO_TO_POS (0x574dbf): a positive offset is applied twice.
            let (x, y) = (t.x as i32, t.y as i32);
            let mut dest = destination.0 as i32;
            if x > 0 || y > 0 {
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
        RESOURCE_GATHERING => {
            if is_target(world, here.x(), here.y(), 0, true) {
                if let Some(j) = job_mut(world, i) {
                    j.displacement_costs = 10;
                }
                register(world, i, counter as i32);
                let (mx, my) = (here.x() + 1, here.y() - 1);
                let obj = world.grid.checked_index(mx, my).map_or(0, |m| world.layers.deco_object[m]);
                let is_static = world.entities.by_index(obj as u32).is_some_and(|e| matches!(&e.data, EntityData::Deco(d) if d.static_instance));
                if obj != 0 && is_static {
                    let copy = crate::deco::mushroom::make_dynamic(world, obj, mx, my).unwrap_or(0);
                    if let Some(j) = job_mut(world, i) {
                        j.entity = copy;
                    }
                    return;
                }
            }
            if let Some(g) = g(world, i)
                && g.spot.0 != -1
            {
                g.spot = (-1, -1);
                clear_reserved(world, here.x(), here.y());
            }
            get_next_job(world, i);
        }
        task_type::WORK => {
            let reserved_dark = world.grid.checked_index(here.x(), here.y()).is_some_and(|n| {
                let f = world.layers.flags[n];
                f & RESERVED != 0 && f & 1 == 0 && f & 4 != 0
            });
            if reserved_dark {
                if let Some(j) = job_mut(world, i) {
                    j.displacement_costs = 10;
                }
                register(world, i, counter as i32);
            } else {
                get_next_job(world, i);
            }
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            let r = world.rng.rand();
            register(world, i, (r & 0xf) as i32 + 1);
        }
        NEXT_TARGET => {
            if find_target(world, i, 0) || find_target(world, i, 1) {
                return;
            }
            let Some(g) = g(world, i) else { return };
            g.spiral += 0x30;
            g.rounds += 1;
            if g.rounds < 0x39 {
                if let Some(j) = job_mut(world, i) {
                    j.frame = 0;
                }
                register(world, i, 1);
            } else {
                core::take_wait_list(world, i);
            }
        }
        // Home vtbl+0x80 (0x5797c8): the vehicle he was called to takes him in.
        task_type::CHECKIN => {
            let home = job(world, i).map_or(0, |j| j.home);
            match world.entities.by_index(home as u32).map(|e| e.obj_type) {
                Some(ObjType::LandVehicle | ObjType::Ship) => crate::vehicles::settler_entered(world, home, i),
                Some(_) => world.events.push(crate::event::SimEvent::Unported { what: "CGardenerRole CHECKIN (home vtbl+0x80)", entity: i }),
                None => {}
            }
        }
        _ => {}
    }
}

/// `CGardenerRole::LogicUpdateJob` (0x5794e0).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    let (task, entity) = (j.task, j.entity);
    let here = pos(world, i);
    match task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        // The beating: the mushroom is hit.
        RESOURCE_GATHERING => {
            if entity != 0 {
                if world.entities.by_index(entity as u32).is_some() {
                    crate::deco::mushroom::hit(world, entity);
                    if let Some(g) = g(world, i) {
                        g.spot = (-1, -1);
                    }
                    clear_reserved(world, here.x(), here.y());
                }
                if let Some(j) = job_mut(world, i) {
                    j.entity = 0;
                }
            }
            get_next_job(world, i);
        }
        // The node turns light for his player.
        task_type::WORK => {
            clear_reserved(world, here.x(), here.y());
            if let Some(g) = g(world, i) {
                g.spot = (-1, -1);
            }
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            crate::dark::lighten(world, here.x(), here.y(), owner as i32);
            get_next_job(world, i);
        }
        NEXT_TARGET => get_next_job(world, i),
        _ => {}
    }
}

/// `CGardenerRole::SetFree` (0x579470): a mushroom copy he was beating goes back to the static
/// instance (`ChangeToStaticInstance`).
pub fn set_free(world: &mut World, i: u16, eco_sector: i32) -> bool {
    release_target(world, i);
    core::set_free_base(world, i, eco_sector)
}

fn release_target(world: &mut World, i: u16) {
    let entity = job(world, i).map_or(0, |j| j.entity);
    if entity == 0 {
        return;
    }
    if let Some(e) = world.entities.by_index(entity as u32)
        && e.obj_type == ObjType::Deco
        && let EntityData::Deco(d) = &e.data
        && !d.static_instance
    {
        let (x, y, kind, amount) = (e.x(), e.y(), e.kind, d.amount);
        crate::deco::change_to_static_instance(world, x, y, kind, amount);
    }
    if let Some(j) = job_mut(world, i) {
        j.entity = 0;
    }
}

/// `CGardenerRole::ConvertEventIntoGoal` (0x579a80).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // Stop: waiting, flags 0x8000 off and 0x2000 on, task 0x1b. (The AI notice is not ported.)
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
        // Event 0xd → vtbl+0x80 (0x586e40: GO_TO_POS to (data0, data1), then WAIT; flag 2
        // appends): no code of the release makes an entity event 0xd (every CEntityEvent
        // construction, vtable 0x102a49c, checked: docs/JOBS.md), so this never runs.
        0xd => {}
        0x11 => {
            let spot = g(world, i).map_or((-1, -1), |g| g.spot);
            if spot.0 != -1 {
                clear_reserved(world, spot.0, spot.1);
                if let Some(g) = g(world, i) {
                    g.spot = (-1, -1);
                }
            }
            // A move order onto a ferry boards it (0x5800b0).
            if ev.kind != 0xd || crate::vehicles::ferry::settler_move_order(world, i, ev) {
                return;
            }
            // A mushroom copy being beaten goes back to the static instance.
            let entity = job(world, i).map_or(0, |j| j.entity);
            if entity != 0 {
                if let Some(e) = world.entities.by_index(entity as u32)
                    && let EntityData::Deco(d) = &e.data
                    && !d.static_instance
                {
                    let (x, y, kind, amount) = (e.x(), e.y(), e.kind, d.amount);
                    crate::deco::change_to_static_instance(world, x, y, kind, amount);
                }
                if let Some(j) = job_mut(world, i) {
                    j.entity = 0;
                }
            }
            let (mode, place) = (ev.data[0] as u32, Y16X16(ev.data[1] as u32));
            let here = pos(world, i);
            if let Some(j) = job_mut(world, i) {
                if mode & 4 == 0 {
                    j.gardener.centre = (place.x(), place.y());
                    j.gardener.spiral = 0;
                    j.gardener.rounds = 0;
                }
                j.start = here;
                j.destination = place;
            }
            core::new_shared_list(world, i, if mode & 4 == 0 { LIST_BEAT } else { LIST_FORCE_WALK });
        }
        // A vehicle (a ferry) calls him.
        0x18 => {
            crate::vehicles::ferry::settler_called(world, i, ev.data[0] as u16);
        }
        // Ashore at data 1: no home, task 0x1b, the AI notice (0x15 with home 0).
        0x19 => {
            if let Some(j) = job_mut(world, i) {
                j.home = 0;
            }
            crate::vehicles::ferry::settler_ashore(world, i, Y16X16(ev.data[1] as u32));
            if let Some(j) = job_mut(world, i) {
                j.task = 0x1b;
            }
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, 0);
        }
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1);
            }
        }
    }
}
