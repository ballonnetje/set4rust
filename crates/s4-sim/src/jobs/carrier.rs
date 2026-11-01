//! `CCarrierRole` (MapObjects/Settler/CarrierRole.cpp, role id 1): carriers. Idle on their wait
//! list (with an occasional idle animation), carry one good from a source pile to a destination
//! pile or a vehicle, and turn into specialists or soldiers (`ChangeToNextType`). The transport
//! orders arrive as entity events from the eco-sector.

use super::core::{self, job, job_mut, register};
use super::hooks;
use super::state::{CarrierData, EntityEvent};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;

/// `JOB_CARRIER_TRANSPORT_GOOD`.
pub const LIST_TRANSPORT_GOOD: u16 = 152;
/// `JOB_CARRIER_TRANSPORT_GOOD_TO_VEHICLE`.
pub const LIST_TRANSPORT_GOOD_TO_VEHICLE: u16 = 153;
/// `JOB_CARRIER_IDLE1..3` (154..156).
pub const LIST_IDLE1: u16 = 154;
/// Gfx jobs during which the idle counter does not run (`GetJobPart() == 48 / 49`; inferred:
/// the strike animations).
const NO_IDLE_JOB_PARTS: [u16; 2] = [48, 49];
/// `WalkToXY` flags: towards a pile (`0x1000`), roughly (`0x2000`, GO_TO_TARGET_ROUGHLY).
const WALK_TO_PILE: i32 = 0x1000;
const WALK_ROUGHLY: i32 = 0x2000;

fn carrier(world: &mut World, i: u16) -> Option<&mut CarrierData> {
    job_mut(world, i).map(|j| &mut j.carrier)
}

/// `Rand() % 300 + 300` from the game RNG (`CStateGame::Random16`).
pub(crate) fn idle_threshold(world: &mut World) -> u16 {
    (world.rng.rand() % 300 + 300) as u16
}

/// `CCarrierRole::CCarrierRole` (release 0x572de0) drew `idle_after` (see `roles::create_settler_role`);
/// then `InitWalking` (release 0x573160:
/// `CWalking::Create(1, owner)` + vtbl+8(-1, 0)) and `Init` (release 0x5730c0).
pub fn init_role(world: &mut World, i: u16, idle_after: u16) {
    let t = idle_after;
    if let Some(c) = carrier(world, i) {
        *c = CarrierData { idle_after: t, ..CarrierData::default() };
    }
    ActivePathing::create(world, i, 1);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    // Init: "!m_uHomeEntityId" asserted.
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
        j.home = 0;
        j.source_pile = 0;
        let idle = (j.carrier.idle_count, j.carrier.idle_after);
        j.carrier = CarrierData { idle_count: idle.0, idle_after: idle.1, ..CarrierData::default() };
    }
    hooks::war_map_add(world, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !(flag::VULNERABLE_MASK | flag::SELECTABLE | flag::SELECTED);
    }
    // 0x57312f: offered (0x4d4090) to the eco-sector at its node when there is one and it is
    // the owner's.
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (owner, pos) = (e.owner, e.pos);
    let eco = world.grid.checked_index(pos.x(), pos.y()).map_or(0, |n| crate::eco::eco_of(world, n));
    if eco != 0 && world.economy.owner(eco) == owner {
        hooks::eco_sector_offer(world, i);
    }
}

/// `CCarrierRole::LogicUpdate` (release 0x5731c0, vtbl+24). WAIT goes through the base, then counts
/// idle updates while the carrier stood still; at the threshold it plays one of the three idle
/// lists (`Rand() % 3 + 154`) and draws a new threshold.
pub fn logic_update(world: &mut World, i: u16) {
    if job(world, i).is_none_or(|j| j.task != task_type::WAIT) {
        logic_update_job(world, i);
        return;
    }
    core::role_logic_update(world, i);
    let Some(j) = job_mut(world, i) else { return };
    if j.walk & 8 == 0 || NO_IDLE_JOB_PARTS.contains(&j.job_part) {
        return;
    }
    j.carrier.idle_count = j.carrier.idle_count.wrapping_add(1);
    if j.carrier.idle_count < j.carrier.idle_after {
        return;
    }
    j.carrier.idle_count = 0;
    let t = idle_threshold(world);
    if let Some(c) = carrier(world, i) {
        c.idle_after = t;
    }
    let list = LIST_IDLE1 + (world.rng.rand() % 3) as u16;
    core::new_shared_list(world, i, list);
}

/// `CCarrierRole::GetNextJob` (release 0x573b50): next task, or `SetFree(-1)` at the end of the list.
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i) {
        super::roles::set_free(world, i, -1);
    } else {
        take_job(world, i);
    }
}

/// Walk to `target` as the walking task (`task = GO`, displacement 5, one `Go`).
fn start_walk(world: &mut World, i: u16, target: Y16X16, flags: i32) {
    ActivePathing::walk_to_xy(world, i, target, flags);
    if let Some(j) = job_mut(world, i) {
        j.task = task_type::GO;
        j.displacement_costs = 5;
    }
    core::go(world, i);
}

/// vtbl+64 `SetFree(settler, -1)`.
fn free(world: &mut World, i: u16) {
    super::roles::set_free(world, i, -1);
}

/// `CCarrierRole::TakeJob` (release 0x5735e0).
pub fn take_job(world: &mut World, i: u16) {
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !0x10_0000; // read and cleared; the value is not used
    }
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    let Some(j) = job(world, i) else { return };
    let (counter, cycle, good) = (j.counter, j.cycle_frames, j.carrier.good);
    match t.task {
        // GO_TO_TARGET, GO_TO_TARGET_ROUGHLY: carrying shows the good's gfx job (job + good).
        10 | 37 => {
            let Some(j) = job_mut(world, i) else { return };
            if good != 0 {
                j.job_part = j.job_part.wrapping_add(good as u16);
            }
            j.frame = 1;
            let d = Y16X16::pack(j.x as i32 + j.destination.x(), j.y as i32 + j.destination.y());
            j.destination = d;
            start_walk(world, i, d, if t.task == 37 { WALK_ROUGHLY } else { 0 });
        }
        task_type::WORK => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
                j.frame = 0;
            }
            register(world, i, counter as i32);
        }
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            let r = world.rng.rand();
            register(world, i, (r % 4 + 1) as i32);
        }
        // GO_TO_SOURCE_PILE
        18 => {
            let src = job(world, i).map_or(0, |j| j.source_pile);
            match core::entity_pos(world, src) {
                Some(p) => {
                    let pos = world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos);
                    if let Some(j) = job_mut(world, i) {
                        j.start = pos;
                    }
                    start_walk(world, i, p, WALK_TO_PILE);
                }
                None => free(world, i),
            }
        }
        // GO_TO_DESTINATION_PILE
        19 => {
            let dst = job(world, i).map_or(0, |j| j.carrier.dest_pile);
            match core::entity_pos(world, dst) {
                Some(p) => {
                    if let Some(j) = job_mut(world, i) {
                        j.job_part = j.job_part.wrapping_add(good as u16);
                    }
                    start_walk(world, i, p, WALK_TO_PILE);
                }
                None => free(world, i),
            }
        }
        // GET_GOOD_CHANGE_JOB, GET_GOOD
        20 | 22 => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter as i32 / 2 - 1);
        }
        // PUT_GOOD
        21 => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
                j.frame = cycle.saturating_sub(1);
            }
            register(world, i, counter as i32 / 2);
        }
        // LOAD_GOOD
        23 => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            register(world, i, counter as i32);
        }
        task_type::CHECKIN => {
            let home = job(world, i).map_or(0, |j| j.home);
            if home == 0 {
                return;
            }
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 10;
            }
            if let Some(e) = world.entities.by_index_mut(i as u32) {
                e.flags &= !flag::VISIBLE;
            }
            match world.entities.by_index(home as u32).map(|e| e.obj_type) {
                Some(ObjType::Building) => hooks::building_settler_enter(world, home, i),
                Some(ObjType::LandVehicle) => crate::vehicles::settler_entered(world, home, i),
                _ => {} // "pEntity!= NULL" asserted
            }
        }
        // CHANGE_JOB
        26 => {
            let next = job(world, i).map_or(0, |j| j.carrier.next_type);
            if next == 0 {
                free(world, i);
            } else {
                // "HomeEntityId() == 0" asserted.
                change_to_next_type_ex(world, i, true, true);
            }
        }
        // CHANGE_TYPE_AT_BARRACKS: needs home, the three tool piles and a next type.
        38 => {
            let ok = job(world, i).is_some_and(|j| {
                j.carrier.next_type != 0 && j.home != 0 && j.source_pile != 0 && j.carrier.source_pile2 != 0 && j.carrier.source_pile3 != 0
            });
            if ok {
                change_type_at_barracks(world, i);
            } else {
                free(world, i);
            }
        }
        // CHANGE_JOB_COME_TO_WORK
        39 => {
            let ok = job(world, i).is_some_and(|j| j.carrier.next_type != 0 && j.home != 0);
            if ok {
                change_job_come_to_work(world, i);
            } else {
                free(world, i);
            }
        }
        _ => register(world, i, 3), // "Carrier TakeJob - unknown job"
    }
}

/// Task 38 `CHANGE_TYPE_AT_BARRACKS` (release 0x5735e0 case 0x26): the barracks `Detach`es the
/// carrier, it takes the goods it reserved (observer targets 2..4, 0x5873f0 with take: weapon,
/// gold, second good; 0xffff = none), becomes its next type (`ChangeToNextType` 0x574540, no
/// offer) and — unless events are queued — walks as a soldier to the first node of the radius-15
/// spiral around the barracks' working area in its own sector (event 0x11, kind 0xd). The
/// statistic counts the settler made (0x5739c5, before the change).
fn change_type_at_barracks(world: &mut World, i: u16) {
    let home = job(world, i).map_or(0, |j| j.home);
    let area = core::building_working_area(world, home);
    crate::buildings::detach_settler(world, home, i);
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
    }
    for t in [crate::pile::target::PILE_SRC, crate::pile::target::PILE_SRC_2, crate::pile::target::PILE_SRC_3] {
        let p = match t {
            crate::pile::target::PILE_SRC => job(world, i).map_or(0, |j| j.source_pile),
            crate::pile::target::PILE_SRC_2 => job(world, i).map_or(0, |j| j.carrier.source_pile2),
            _ => job(world, i).map_or(0, |j| j.carrier.source_pile3),
        };
        if p != 0 && p != 0xffff {
            crate::pile::change_amount_and_detach(world, p, i);
        }
        if let Some(j) = job_mut(world, i) {
            match t {
                crate::pile::target::PILE_SRC => j.source_pile = 0,
                crate::pile::target::PILE_SRC_2 => j.carrier.source_pile2 = 0,
                _ => j.carrier.source_pile3 = 0,
            }
        }
    }
    let next = job(world, i).map_or(0, |j| j.carrier.next_type);
    let owner = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
    crate::statistic::settler_produced(world, owner, next as u16);
    change_to_next_type_ex(world, i, true, false);
    if job(world, i).is_none_or(|j| !j.events.is_empty()) {
        return;
    }
    let (Some(area), Some(e)) = (area, world.entities.by_index(i as u32)) else { return };
    let Some(sector) = world.grid.checked_index(e.x(), e.y()).map(|n| world.tiling.sector_of(n)) else { return };
    let rally = crate::spiral::walk(world.grid, area.x(), area.y(), 15).find(|&(x, y)| world.tiling.sector_of(world.grid.index(x as u32, y as u32)) == sector);
    if let Some((x, y)) = rally {
        core::set_event(world, i, EntityEvent { event: 0x11, kind: 0xd, data: [0, Y16X16::pack(x, y).0 as i32, 0] });
    }
}

/// `CCarrierRole::ChangeToNextType` (release 0x574540).
fn change_to_next_type(world: &mut World, i: u16) {
    change_to_next_type_ex(world, i, true, false);
}

/// `CCarrierRole::ChangeToNextType(settler, take_wait_list, offer)` (release 0x574540): the eco-sector
/// forgets the order (`ResetCarrierNextSettlerTypeAndDecrementOrder`), then `CSettler::ChangeType`.
fn change_to_next_type_ex(world: &mut World, i: u16, take_wait: bool, offer: bool) {
    let next = job(world, i).map_or(0, |j| j.carrier.next_type);
    // "NextSettlerType() != 0" asserted.
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let eco = world.grid.checked_index(e.x(), e.y()).map_or(0, |n| crate::eco::eco_of(world, n));
    crate::eco::reset_carrier_next_settler_type_and_decrement_order(world, eco, i);
    if let Some(c) = carrier(world, i) {
        c.next_type = 0;
    }
    core::change_type(world, i, next as u16, take_wait, offer);
}

/// Task 39 and event 22: become the next type, then go to work at the home building
/// (`CSettler::SetBuilding(home)` + event 1).
fn change_job_come_to_work(world: &mut World, i: u16) {
    let home = job(world, i).map_or(0, |j| j.home);
    // "FlagBits(ENTITY_FLAG_ATTACHED)" asserted before and after.
    change_to_next_type_ex(world, i, true, false);
    if let Some(j) = job_mut(world, i) {
        j.home = home;
    }
    core::set_event(world, i, EntityEvent { event: 1, kind: 0, data: [home as i32, 0, 0] });
}

/// `CCarrierRole::TryToGoHome` (release 0x573b80): walk back to the start position if it lies in the
/// carrier's eco-sector.
fn try_to_go_home(world: &mut World, i: u16) {
    let Some(j) = job(world, i) else { return };
    let start = j.start;
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let pos = e.pos;
    let eco = |w: &World, p: Y16X16| w.grid.checked_index(p.x(), p.y()).map(|n| w.tiling.eco_sector_of(n));
    if eco(world, start) != eco(world, pos) {
        return;
    }
    if let Some(j) = job_mut(world, i) {
        j.frame = 1;
    }
    start_walk(world, i, start, 0);
}

/// `CCarrierRole::LogicUpdateJob` (release 0x573290, vtbl+28).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    let cycle = j.cycle_frames as i32;
    match j.task {
        task_type::GO => {
            j.distance = 0;
            core::go(world, i);
        }
        task_type::WORK => get_next_job(world, i),
        // GET_GOOD_CHANGE_JOB
        20 => {
            let v = cycle / 2;
            j.counter = (j.counter as i32 - v) as i8;
            let (next, src) = (j.carrier.next_type, j.source_pile);
            if (j.counter as i32) < v {
                if next == 0 {
                    free(world, i);
                }
            } else if next == 0 || src == 0 {
                free(world, i);
            } else {
                take_from_pile(world, i, src);
                register(world, i, v);
            }
        }
        // PUT_GOOD
        21 => {
            let dst = j.carrier.dest_pile;
            if dst == 0 {
                free(world, i);
                return;
            }
            let v = cycle / 2;
            j.counter = (j.counter as i32 - v) as i8;
            if (j.counter as i32) < v {
                free(world, i);
                try_to_go_home(world, i);
            } else {
                // "m_uGood == pPile->GetGoodType()" asserted.
                if core::pile_good(world, dst).is_some() {
                    hooks::pile_change_amount_and_detach(world, dst, i);
                }
                if let Some(c) = carrier(world, i) {
                    c.good = 0;
                }
                register(world, i, v - 1);
            }
        }
        // GET_GOOD
        22 => {
            let v = cycle / 2;
            j.counter = (j.counter as i32 - v) as i8;
            let src = j.source_pile;
            if (j.counter as i32) < v {
                get_next_job(world, i);
            } else if src == 0 {
                free(world, i);
            } else {
                take_from_pile(world, i, src);
                register(world, i, v);
            }
        }
        // LOAD_GOOD into the home vehicle (catapult or ship).
        23 => {
            let home = j.home;
            if home == 0 {
                free(world, i);
                return;
            }
            let good = job(world, i).map_or(0, |j| j.carrier.good) as i32;
            crate::vehicles::good_arrived(world, home, good, 1);
            crate::vehicles::detach_settler(world, home, i);
            if let Some(c) = carrier(world, i) {
                c.good = 0;
            }
            free(world, i);
            try_to_go_home(world, i);
        }
        _ => {} // "LogicUpdateJob Carrier - unknown task %u"
    }
}

/// `m_uGood = pile->GetGoodType(); CPile::ChangeAmountAndDetach(settler)` ("m_uGood > 0").
fn take_from_pile(world: &mut World, i: u16, pile: u16) {
    let g = core::pile_good(world, pile).unwrap_or(0);
    if let Some(c) = carrier(world, i) {
        c.good = g;
    }
    hooks::pile_change_amount_and_detach(world, pile, i);
}

/// `CCarrierRole::UpdateJob` (release 0x573560, vtbl+32): GET/LOAD animate forwards and hold the last
/// frame, PUT plays backwards to frame 0.
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    let (elapsed, cycle) = (j.elapsed as i32, j.cycle_frames as i32);
    match j.task {
        20 | 22 | 23 => {
            let f = elapsed + j.frame as i32;
            j.frame = if f >= cycle { (if cycle <= 0 { 0 } else { cycle - 1 }) as u8 } else { f as u8 };
        }
        21 => {
            let f = j.frame as i32;
            j.frame = if f <= elapsed { 0 } else { (f - elapsed) as u8 };
        }
        _ => {}
    }
}

/// `CCarrierRole::SetFree(settler, eco_sector)` (release 0x573c10, vtbl+64; it starts with
/// `RemoveCarrierFromStrikeSettlers(owner, settler)`, 0x4df7a0): leave the strike list, drop
/// a carried good, undo a pending type change, detach from the home, then the base `SetFree`.
/// The eco-sector is `eco_sector` if positive, else the one under the settler (read before
/// anything moves). A pending type change (+0x2d): an attached carrier leaves its home
/// (role +0x50 `SettlerLeft`, which makes the building order again, then `Detach` if still
/// attached, home 0); the eco-sector takes the order back (builder / digger:
/// `ChangeMinMaxValues(type, 1)` 0x4d5750; pioneer / geologist / gardener: the specialist wish
/// +0x288 + index (table 0x1164214) + 1, at most 99; other types nothing). The eco-sector's
/// carriers-with-job-order count is not lowered (as in the original).
pub fn set_free(world: &mut World, i: u16, eco_sector: i32) -> bool {
    hooks::remove_carrier_from_strike(world, i);
    let eco = if eco_sector > 0 {
        eco_sector as u16
    } else {
        world.entities.by_index(i as u32).and_then(|e| world.grid.checked_index(e.pos.x(), e.pos.y())).map_or(0, |n| crate::eco::eco_of(world, n))
    };
    let Some(j) = job(world, i) else { return false };
    let CarrierData { good, next_type, .. } = j.carrier;
    if good != 0 {
        hooks::drop_good(world, i, good);
        if let Some(c) = carrier(world, i) {
            c.good = 0;
        }
    }
    if next_type != 0 {
        let attached = |w: &World| w.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::ATTACHED != 0);
        if attached(world) {
            let home = job(world, i).map_or(0, |j| j.home);
            crate::buildings::settler_left(world, home, i);
            if attached(world) {
                hooks::home_detach(world, home, i);
            }
            if let Some(j) = job_mut(world, i) {
                j.home = 0;
            }
        }
        hooks::cancel_next_type(world, eco, next_type);
    }
    if let Some(c) = carrier(world, i) {
        c.next_type = 0;
    }
    let home = job(world, i).map_or(0, |j| j.home);
    if home != 0 {
        hooks::home_detach(world, home, i);
    }
    core::set_free_base(world, i, eco_sector)
}

/// Event 24 (0x573d80 case 0x18): a vehicle calls the carrier. With the vehicle's place
/// (vtbl+0x8c) and the vehicle alive: the place is the destination, flag 0x100000, the check-in
/// list (`SettlerInfo` list 0), the vehicle attaches it (vtbl+0xa4), 0x2200 off, the AI notice
/// 0x12 (owner, carrier, its home: the vehicle; 0x57422c); else flag 0x2000 and the wait list.
pub fn board_vehicle(world: &mut World, i: u16, vehicle: u16) {
    let place = crate::vehicles::exit_place(world, vehicle, i);
    let alive = world.entities.by_index(vehicle as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0);
    if place == 0 || !alive {
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags |= 0x2000;
        }
        core::take_wait_list(world, i);
        return;
    }
    let pos = world.entities.by_index(i as u32).map_or(crate::grid::Y16X16(0), |e| e.pos);
    if let Some(j) = job_mut(world, i) {
        j.destination = crate::grid::Y16X16(place as u32);
        j.start = pos;
    }
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags |= 0x10_0000;
    }
    core::take_anim_list(world, i, 0);
    crate::vehicles::attach(world, vehicle, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !0x2200;
    }
    let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), job(world, i).map_or(0, |j| j.home));
    crate::ai::post_event(world, 0x12, owner as i32, i as i32, home as i32);
}

/// Event 25 (0x573d80 case 0x19): out of the vehicle at `at` (0x582a40, the default list), into
/// the war map (0x5bb560), visible and off board, the wait list, the AI notice 0x15 (owner,
/// carrier, home; 0x5742db); on an eco-sector of its owner it is offered there and counted
/// (0x4d40f0); on land not its owner's it is set free.
fn leave_vehicle(world: &mut World, i: u16, at: crate::grid::Y16X16) {
    crate::create::put_settler_near(world, i, at.x(), at.y());
    crate::warmap::readd(world, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags = (e.flags & !flag::ON_BOARD) | flag::VISIBLE;
    }
    core::take_wait_list(world, i);
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (owner, kind, pos) = (e.owner, e.kind, e.pos);
    let home = job(world, i).map_or(0, |j| j.home);
    crate::ai::post_event(world, 0x15, owner as i32, i as i32, home as i32);
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return };
    let eco = crate::eco::eco_of(world, n);
    if eco != 0 && world.economy.owner(eco) == owner {
        crate::eco::set_settler_offer(world, eco, 1, i);
        crate::eco::change_nr_of_settler(world, eco, 1, 1);
    }
    let _ = kind;
    if world.owner_of(n) != owner {
        free(world, i);
    }
}

/// `CCarrierRole::ConvertEventIntoGoal` (release 0x573d80). The events the eco-sector, buildings and
/// vehicles send (`m_iEvent`):
/// * 1: come home to building `data[0]` (its door, anim list 0 = check-in);
/// * 7 / 9: set free in eco-sector `data[1]` (7 is ignored while the carrier has a home: it
///   re-registers instead);
/// * 10: transport a good (source pile `data[1]`, destination pile `data[2]`): list 152;
/// * 11: transport to vehicle `data[2]` from pile `data[1]`: list 153 (vehicle side unported);
/// * 20: change type now; 21: change job (needs next type and source pile): list 151;
/// * 22: become the next type at home `data[1]`; 23: list 240 (type / pile / home checks);
/// * 24: board vehicle `data[0]`; 25: leave a vehicle at `data[1]`; 26: tool change at home:
///   anim list 3; 27: list 238 (`data[0]` != 0) or 239.
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    let Some(j) = job(world, i) else { return };
    let (home, src, dst, next) = (j.home, j.source_pile, j.carrier.dest_pile, j.carrier.next_type);
    match ev.event {
        1 => {
            if home as i32 != ev.data[0] {
                free(world, i);
                return;
            }
            if let Some(door) = core::building_door(world, home) {
                core::new_destination(world, i, door);
            }
            core::take_anim_list(world, i, 0);
        }
        7 | 9 => {
            if home != 0 && ev.event == 7 {
                register(world, i, 1);
                return;
            }
            super::roles::set_free(world, i, ev.data[1]);
        }
        10 => {
            // "(SourcePileId() == 0) || (SourcePileId() == data1)" and the same for the destination.
            if src as i32 != ev.data[1] && dst as i32 != ev.data[2] {
                free(world, i);
                return;
            }
            core::new_shared_list(world, i, LIST_TRANSPORT_GOOD);
        }
        // A good for vehicle `data[2]` from pile `data[1]`: without a home (the vehicle) the
        // carrier is free and the vehicle's order falls through (vtbl+0x88); without a source
        // pile it is free; else to the vehicle's place (vtbl+0x8c; none: as without a home),
        // list 153, the vehicle counts the order (vtbl+0x78).
        11 => {
            let (vehicle, pile) = (ev.data[2] as u16, ev.data[1] as u16);
            let good = core::pile_good(world, pile).unwrap_or(0) as i32;
            if home == 0 {
                free(world, i);
                crate::vehicles::good_cancelled(world, vehicle, good, 1);
                return;
            }
            if src == 0 {
                free(world, i);
                return;
            }
            let place = crate::vehicles::exit_place(world, vehicle, i);
            if place == 0 {
                free(world, i);
                crate::vehicles::good_cancelled(world, vehicle, good, 1);
                return;
            }
            let pos = world.entities.by_index(i as u32).map_or(crate::grid::Y16X16(0), |e| e.pos);
            if let Some(j) = job_mut(world, i) {
                j.destination = crate::grid::Y16X16(place as u32);
                j.start = pos;
            }
            core::new_shared_list(world, i, LIST_TRANSPORT_GOOD_TO_VEHICLE);
            crate::vehicles::good_ordered(world, vehicle, good, 1);
        }
        20 => {
            if next == 0 {
                free(world, i);
                return;
            }
            change_to_next_type(world, i);
        }
        21 => {
            if next == 0 || src == 0 {
                free(world, i);
                return;
            }
            core::new_shared_list(world, i, 151);
        }
        22 => {
            if next as i32 != ev.data[0] || home as i32 != ev.data[1] {
                free(world, i);
                return;
            }
            change_job_come_to_work(world, i);
        }
        23 => {
            if next as i32 != ev.data[0] || src as i32 != ev.data[1] || home as i32 != ev.data[2] {
                free(world, i);
                return;
            }
            core::new_shared_list(world, i, 240);
        }
        24 => board_vehicle(world, i, ev.data[0] as u16),
        25 => leave_vehicle(world, i, crate::grid::Y16X16(ev.data[1] as u32)),
        26 => {
            let ok = job(world, i).is_some_and(|j| {
                j.carrier.next_type != 0 && j.home != 0 && j.source_pile != 0 && j.carrier.source_pile2 != 0 && j.carrier.source_pile3 != 0
            });
            if ok {
                if let Some(door) = core::building_door(world, home) {
                    core::new_destination(world, i, door);
                }
                core::take_anim_list(world, i, 3);
            } else {
                free(world, i);
            }
        }
        // Strike (release 0x573d80 case 27): data0 1 → GOINTO_STRIKE (238), 0 → QUIT_STRIKE (239).
        27 => core::new_shared_list(world, i, if ev.data[0] != 0 { 238 } else { 239 }),
        _ => {
            if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
                register(world, i, 1); // "ConvertEventIntoGoal CarrierRole - unknown event %u"
            }
        }
    }
}
