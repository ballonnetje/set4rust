//! `CThiefRole` (release vtable 0x105c4b4, role 19, `SETTLER_THIEF` 48): the specialist the player
//! sends into enemy land to steal goods. Decompiles in `tools/re/decompiled/sim/thief/`.
//!
//! The move order (event 0x11 kind 0xd, [`crate::group::send_to`]) picks a list by where he is
//! sent (0x58bed0: the land's owner equals his team, as the release compares) and what he
//! carries: on his own land `JOB_THIEF_PUT_DOWN` (226) with a good, `JOB_THIEF_GET_GOOD` (225)
//! without; on other land the good he carries is dropped where he stands and he goes stealing
//! (225); with mode bit 2 he only walks there (`JOB_THIEF_GO_FULL` 227 / `GO_EMPTY` 228). His
//! starting point (+0x1c) is where he stood when sent from his own land.
//! * GO_TO_TARGET (10, 0x58bdb0): over spiral entries 0..=79 around him, the first pile on a node
//!   of his own player or of a player of another team (on his own land only free piles, pile
//!   role 3) without flag 0x10 and with a good not reserved (leaving < amount): one good is
//!   reserved for him (observer target 2) and he walks there (flag 0x1000); none: WAIT.
//! * GET_GOOD (0x16): facing the pile; halfway through the animation the good is his
//!   (`ChangeAmountAndDetach`); without the pile he is set free.
//! * GO_HOME (0xe): back to his starting point.
//! * PUT_GOOD (0x15): halfway through, the good goes to the nearest room for it
//!   (`SearchSpaceForGoods` 0x570c60).
//!
//! Shown as carrying (gfx job 0x12e for 0x12d) while he has a good.
//!
//! Every 15 ticks while hidden, else 45 (`LogicUpdate` 0x58b600 → 0x5801f0): visible, on land of
//! his team or nobody's with no enemy warrior within 15, he hides (flag 0x10000000, the
//! vulnerability bits 0x30000 off); hidden, an enemy warrior within 6 makes him visible again.
//!
//! Ferries: boarding on a move order (0x5800b0, `vehicles::ferry`). Not ported: the role's own
//! task list (+0x30, filled
//! only by a load), the release's store / load (0x58b3a0 / 0x58b240).

use super::core::{self, job, job_mut, register};
use super::state::EntityEvent;
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::grid::Y16X16;

/// `JOB_THIEF_GET_GOOD`, `PUT_DOWN`, `GO_FULL`, `GO_EMPTY`.
const LIST_GET_GOOD: u16 = 0xe1;
const LIST_PUT_DOWN: u16 = 0xe2;
const LIST_GO_FULL: u16 = 0xe3;
const LIST_GO_EMPTY: u16 = 0xe4;
const GO_HOME: u8 = 0xe;
const PUT_GOOD: u8 = 0x15;
const GET_GOOD: u8 = 0x16;
/// `TH_WALK` and `TH_WALK_GOOD`.
const PART_WALK: u16 = 0x12d;
const PART_WALK_GOOD: u16 = 0x12e;
/// Entity flag 0x10000000: hidden; 0x30000: vulnerable to fights.
const HIDDEN: u32 = 0x1000_0000;
const VULNERABLE: u32 = 0x3_0000;

fn pos(world: &World, i: u16) -> Y16X16 {
    world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos)
}

fn loot(world: &World, i: u16) -> u8 {
    job(world, i).map_or(0, |j| j.thief.loot)
}

/// The good shown (+0xb) is the good carried (+0x3c).
fn show_loot(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.good = j.thief.loot;
    }
}

/// 0x58bed0 `(settler, node)`: the owner of the node's land equals the settler's team (the
/// release compares an owner with a team).
fn own_land(world: &World, i: u16, p: Y16X16) -> bool {
    let Some(e) = world.entities.by_index(i as u32) else { return false };
    let owner = world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.owner_of(n));
    owner as usize == world.alliances.team(e.owner)
}

/// `InitWalking` (0x58b560: a `CWalkingNormal`, `WalkToXY(−1, 0)`), the constructor (0x58b120:
/// nothing carried, the check due at once) and `Init` (0x58b500: +0x12 = own id, flags 0x32000,
/// into the war map, the starting point here).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= 0x32000;
    }
    super::hooks::war_map_add(world, i);
    let here = pos(world, i);
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
        j.thief = Default::default();
        j.good = 0;
        j.start = here;
    }
}

/// 0x5801f0: hiding or showing himself; the ticks to the next check (15 hidden, else 45).
fn visibility(world: &mut World, i: u16) -> u32 {
    let Some(e) = world.entities.by_index(i as u32) else { return 0x2d };
    let (p, owner, flags) = (e.pos, e.owner, e.flags);
    let land = world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.owner_of(n));
    let team = |q: u8| world.alliances.team(q);
    let new = if flags & HIDDEN == 0 {
        let foreign = team(land) != team(owner) && team(land) != 0;
        (!foreign && !crate::warmap::enemy_warriors_near(world, p.x(), p.y(), 0xf, owner)).then_some((flags & !VULNERABLE) | HIDDEN)
    } else {
        crate::warmap::enemy_warriors_near(world, p.x(), p.y(), 6, owner).then_some((flags & !HIDDEN) | VULNERABLE)
    };
    let mut hidden = flags & HIDDEN != 0;
    if let Some(f) = new
        && let Some(e) = world.entities.by_index_mut(i as u32)
    {
        e.flags = f;
        hidden = f & HIDDEN != 0;
    }
    if hidden { 0xf } else { 0x2d }
}

/// `CThiefRole::LogicUpdate` (0x58b600, vtbl+0x18): the visibility check when due, then the
/// base `LogicUpdate`.
pub fn logic_update(world: &mut World, i: u16) {
    let now = world.tick;
    if job(world, i).is_some_and(|j| j.thief.check_at <= now) {
        let d = visibility(world, i);
        if let Some(j) = job_mut(world, i) {
            j.thief.check_at = now.wrapping_add(d);
        }
    }
    core::role_logic_update(world, i);
}

/// 0x58bdb0: a pile to steal from among spiral entries 0..=79 around him; its node becomes the
/// destination and one good is reserved for him.
fn find_loot(world: &mut World, i: u16) -> bool {
    let Some(e) = world.entities.by_index(i as u32) else { return false };
    let (here, owner) = (e.pos, e.owner);
    let team = |q: u8| world.alliances.team(q);
    for k in 0..80 {
        let (x, y) = (here.x() + crate::spiral::dx(k), here.y() + crate::spiral::dy(k));
        let Some(n) = world.grid.checked_index(x, y) else { continue };
        let land = world.owner_of(n);
        if land != owner && team(land) == team(owner) {
            continue;
        }
        let pile = world.layers.deco_object[n];
        let Some(p) = world.entities.by_index(pile as u32).filter(|p| p.obj_type == ObjType::Pile) else { continue };
        let EntityData::Pile(d) = &p.data else { continue };
        if (land == owner && d.role != crate::pile::role::ANYWHERE) || p.flags & 0x10 != 0 || d.leaving >= d.amount {
            continue;
        }
        crate::pile::attach_and_inc_amount_leaving(world, pile, i, 1, crate::pile::target::PILE_SRC);
        if let Some(j) = job_mut(world, i) {
            j.destination = Y16X16::pack(x, y);
        }
        return true;
    }
    false
}

/// `CThiefRole::TakeJob` (0x58b7c0, vtbl+0x28).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    // 0x580150 takes GO_TO_POS.
    if t.task == task_type::GO_TO_POS {
        super::gardener::retake_go(world, i, &t);
        return;
    }
    let Some(j) = job(world, i) else { return };
    let part0 = j.job_part;
    core::init_common_task_values(world, i, &t);
    let carrying = loot(world, i) != 0;
    let Some(j) = job_mut(world, i) else { return };
    if j.job_part == PART_WALK && carrying {
        j.job_part = PART_WALK_GOOD;
    }
    let (counter, cycle) = (j.counter as i32, j.cycle_frames);
    match t.task {
        task_type::GO_TO_TARGET => {
            let attached = world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::ATTACHED != 0);
            if attached {
                // Sent to the task's own offset (taken as a node).
                let here = pos(world, i);
                let Some(j) = job_mut(world, i) else { return };
                j.frame = 1;
                if j.x > 0 || j.y > 0 {
                    j.start = here;
                    j.destination = Y16X16(((j.y as i32) << 16).wrapping_add(j.x as i32) as u32);
                }
                let dest = j.destination;
                walk_now(world, i, dest, 0);
                return;
            }
            if find_loot(world, i) {
                let Some(j) = job_mut(world, i) else { return };
                if part0 != j.job_part || cycle <= j.frame {
                    j.frame = 1;
                }
                let d = j.destination;
                let dest = Y16X16::pack(d.x() + j.x as i32, d.y() + j.y as i32);
                j.destination = dest;
                ActivePathing::walk_to_xy(world, i, dest, 0x1000);
                if let Some(j) = job_mut(world, i) {
                    j.task = task_type::GO;
                    j.displacement_costs = 5;
                    j.walk = 0x88;
                }
            } else if let Some(j) = job_mut(world, i) {
                j.task = task_type::WAIT;
            }
            register(world, i, 1);
        }
        GO_HOME => {
            j.frame = 1;
            let start = j.start;
            walk_now(world, i, start, 0);
        }
        task_type::WAIT => {
            j.displacement_costs = 0;
            register(world, i, 1);
        }
        PUT_GOOD => {
            j.displacement_costs = 10;
            j.frame = cycle.wrapping_sub(1);
            register(world, i, counter / 2);
        }
        GET_GOOD => {
            let pile = j.source_pile;
            if pile == 0 {
                super::roles::set_free(world, i, -1);
                return;
            }
            let here = pos(world, i);
            let at = world.entities.by_index(pile as u32).map_or(here, |p| p.pos);
            let dir = crate::pathing::walking::direction(here.0 as i32, at.0 as i32) as u8;
            if let Some(s) = core::settler_mut(world, i) {
                s.direction = dir;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter / 2 - 1);
        }
        // Home vtbl+0x80: a vehicle he was called to takes him in.
        task_type::CHECKIN => {
            let home = job(world, i).map_or(0, |j| j.home);
            match world.entities.by_index(home as u32).map(|e| e.obj_type) {
                Some(ObjType::LandVehicle | ObjType::Ship) => crate::vehicles::settler_entered(world, home, i),
                Some(_) => world.events.push(crate::event::SimEvent::Unported { what: "CThiefRole CHECKIN (home vtbl+0x80)", entity: i }),
                None => {}
            }
        }
        _ => {}
    }
}

/// A walk to `dest` started at once (`WalkToXY`, GO, displacement 5, `Go`).
fn walk_now(world: &mut World, i: u16, dest: Y16X16, flags: i32) {
    ActivePathing::walk_to_xy(world, i, dest, flags);
    if let Some(j) = job_mut(world, i) {
        j.task = task_type::GO;
        j.displacement_costs = 5;
    }
    core::go(world, i);
}

/// `CThiefRole::LogicUpdateJob` (0x58b630, vtbl+0x1c).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    match j.task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        PUT_GOOD | GET_GOOD => {
            let half = j.cycle_frames >> 1;
            let task = j.task;
            let Some(j) = job_mut(world, i) else { return };
            j.counter = j.counter.wrapping_sub(half as i8);
            if (half as i32) > j.counter as i32 {
                get_next_job(world, i);
            } else if task == PUT_GOOD {
                let (p, good) = (pos(world, i), loot(world, i));
                crate::pile::search_space_for_goods(world, p.x(), p.y(), good, 1);
                register(world, i, half as i32 - 1);
                if let Some(j) = job_mut(world, i) {
                    j.thief.loot = 0;
                }
            } else {
                let pile = j.source_pile;
                if pile == 0 {
                    super::roles::set_free(world, i, -1);
                    return;
                }
                let good = crate::pile::get(world, pile).map_or(0, |p| p.good);
                if let Some(j) = job_mut(world, i) {
                    j.thief.loot = good;
                }
                crate::pile::change_amount_and_detach(world, pile, i);
                register(world, i, half as i32);
            }
        }
        _ => {}
    }
    show_loot(world, i);
}

/// `CThiefRole::UpdateJob` (0x58b750, vtbl+0x20): PUT_GOOD plays backwards to frame 0, GET_GOOD
/// forwards to its last frame.
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    match j.task {
        PUT_GOOD => {
            j.frame = if j.elapsed < j.frame as u16 { j.frame - j.elapsed as u8 } else { 0 };
        }
        GET_GOOD => {
            let v = j.elapsed as u32 + j.frame as u32;
            let cycle = j.cycle_frames as u32;
            j.frame = if cycle <= v { cycle.saturating_sub(1) as u8 } else { v as u8 };
        }
        _ => {}
    }
}

/// `CThiefRole::GetNextJob` (0x58ba20, vtbl+0x24): the next task; at the list's end he just
/// waits (task WAIT, nothing taken).
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i) {
        if let Some(j) = job_mut(world, i) {
            j.task = task_type::WAIT;
        }
        return;
    }
    take_job(world, i);
}

/// `CThiefRole::SetFree(settler, eco_sector)` (0x58b5c0, vtbl+0x40): the good he carries goes to
/// the nearest room for it, then the base `SetFree` with −1.
pub fn set_free(world: &mut World, i: u16, _eco_sector: i32) -> bool {
    let good = loot(world, i);
    if good != 0 {
        let p = pos(world, i);
        crate::pile::search_space_for_goods(world, p.x(), p.y(), good, 1);
        if let Some(j) = job_mut(world, i) {
            j.thief.loot = 0;
        }
    }
    core::set_free_base(world, i, -1);
    false
}

/// `CThiefRole::ConvertEventIntoGoal` (0x58ba50, vtbl+0x38).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // Stop: flag 0x8000 off, 0x2000 on, the wait list, the AI notice.
        7 => {
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags = (e.flags & !0x8000) | 0x2000;
            }
            core::take_wait_list(world, i);
            let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, ev.data[0]);
        }
        9 => register(world, i, 1),
        0x11 => {
            if ev.kind != 0xd {
                return;
            }
            if job(world, i).is_some_and(|j| j.source_pile != 0) {
                super::roles::set_free(world, i, -1);
            }
            // A move order onto a ferry boards it (0x5800b0).
            if crate::vehicles::ferry::settler_move_order(world, i, ev) {
                return;
            }
            let (mode, place) = (ev.data[0] as u32, Y16X16(ev.data[1] as u32));
            let here = pos(world, i);
            let at_home = own_land(world, i, here);
            if let Some(j) = job_mut(world, i) {
                if at_home || j.start.0 == 0 {
                    j.start = here;
                }
                j.destination = place;
            }
            let carrying = loot(world, i) != 0;
            let list = if mode & 4 != 0 {
                if carrying { LIST_GO_FULL } else { LIST_GO_EMPTY }
            } else if own_land(world, i, place) {
                if carrying { LIST_PUT_DOWN } else { LIST_GET_GOOD }
            } else {
                if carrying {
                    let good = loot(world, i);
                    crate::pile::search_space_for_goods(world, here.x(), here.y(), good, 1);
                    if let Some(j) = job_mut(world, i) {
                        j.thief.loot = 0;
                    }
                }
                LIST_GET_GOOD
            };
            core::new_shared_list(world, i, list);
        }
        // A vehicle calls him (as the pioneer's).
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
                j.start = here;
                j.destination = Y16X16(place as u32);
            }
            crate::vehicles::attach(world, vehicle, i);
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags &= !0x2200;
            }
            if crate::group::word(world, i) != 0 {
                crate::group::remove_all(world, i);
            }
            core::take_anim_list(world, i, 0);
            let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
            crate::ai::post_event(world, 0x12, owner as i32, i as i32, home as i32);
        }
        // Out of the vehicle at data 1 (0x582a40), into the war map, visible and selectable, the
        // wait list, the AI notice.
        0x19 => {
            let at = Y16X16(ev.data[1] as u32);
            crate::create::put_settler_near(world, i, at.x(), at.y());
            crate::warmap::readd(world, i);
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags = (e.flags & !0x8000) | 0x2100;
            }
            core::take_wait_list(world, i);
            let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
            crate::ai::post_event(world, 0x15, owner as i32, i as i32, home as i32);
        }
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & 0x2000_0000 != 0) {
                return;
            }
            register(world, i, 1);
        }
    }
}
