//! `CDonkeyRole` (release vtable 0x105ae90, role 20, `SETTLER_DONKEY` 60; ctor 0x575f20).
//! Decompiles in `tools/re/decompiled/script/donkey/` and `tools/re/trade/helpers.c`.
//!
//! A donkey leaves the donkey ranch (`buildings::workup`) and waits until it gets an order:
//! * event 10, a trade route: market +0x38 → market +0x34 (`JOB_DONKEY_TRANSPORT_GOOD` 163;
//!   the trading building orders it, 0x5558b0, see [`crate::buildings::trading`]);
//! * event 11, goods from a pile to a vehicle (`JOB_DONKEY_TRANSPORT_GOOD_TO_VEHICLE` 164: the
//!   catapult's `RequestAmmoDonkey`);
//! * when hit while loaded, it takes list 162 (`JOB_DONKEY_ATTACK_REACTION`: task 32, see
//!   [`hit`]) and drops its load; a donkey takes no damage;
//! * a built cart calls it (0x576f00, list 237 `JOB_DONKEY_COME_TO_VEHICLEHALL`): it walks behind
//!   the cart and its arrival (0x576d80) hitches the cart (`vehicles::cart::hitch`, the donkey is
//!   deleted).
//!
//! **The trade route** (role +0x40 the order state, [`DonkeyData`]): the market sets source,
//! target and state 2 and sends event 10; the donkey takes list 163: GO_TO_SOURCE_PILE (18,
//! 0x576640: walk to the source's ensign), at the ensign the source market's arrival hook
//! (0x5561f0) makes it state 4 and loads it (two slots of 8, 0x5768e0) on its updates; when
//! nothing more is coming the market sends it off (0x555e40: state 1, update next tick), the
//! wait task (22) takes the next task GO_TO_DESTINATION_PILE (19: recorded as the target's
//! transport, walk to the target's ensign, vulnerable 0x30000 while loaded), PUT_GOOD (21) calls
//! the target's arrival hook (state 3) and the target unloads it into its piles (0x555ed0,
//! 0x5769a0), frees it (state 0) and sends it home to where it got the order when that is in
//! the same eco-sector (0x577010).
//!
//! A foreign transport (+0x68) runs the same list twice: there with the source's exports
//! (leg 1, +0x74), then the partner takes it back loaded with its own (leg 2, 0x5572e0 /
//! 0x577620); called off it turns back (0x5770d0).
//!
//! * a vehicle order (event 11 `[goods, pile, vehicle]`, a catapult's ammunition,
//!   `vehicles::ammo`): list 164 GO_TO_SOURCE_PILE (to the pile, flag 0x1000), GET_GOOD (the
//!   goods taken), GO_TO_TARGET (the vehicle's place), LOAD_GOOD (vtbl+0x84, detached, home).
//!

use super::core::{self, job, job_mut, register};
use super::state::{DonkeyData, EntityEvent};
use super::task::task_type;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::grid::Y16X16;

/// `JOB_DONKEY_TRANSPORT_GOOD`.
pub const LIST_TRANSPORT_GOOD: u16 = 163;
/// `JOB_DONKEY_COME_TO_VEHICLEHALL`.
pub const LIST_COME_TO_VEHICLE: u16 = 237;
/// `JOB_DONKEY_TRANSPORT_GOOD_TO_VEHICLE`.
pub const LIST_TO_VEHICLE: u16 = 164;
/// Goods per load slot.
pub const SLOT_SIZE: i32 = 8;

/// +0x40 order states.
pub mod state {
    pub const FREE: u32 = 0;
    pub const TO_TARGET: u32 = 1;
    pub const TO_SOURCE: u32 = 2;
    pub const UNLOADING: u32 = 3;
    pub const LOADING: u32 = 4;
    pub const VEHICLE: u32 = 5;
}

/// The settler flags `0x30000` (vulnerable): set while a loaded domestic donkey is on its way.
const VULNERABLE: u32 = flag::VULNERABLE_MASK;

/// The donkey's role data (any settler with the donkey role).
pub fn data(world: &World, i: u16) -> Option<&DonkeyData> {
    job(world, i).filter(|j| j.role == super::state::role::DONKEY).and_then(|j| j.donkey.as_deref())
}

pub(crate) fn data_mut(world: &mut World, i: u16) -> Option<&mut DonkeyData> {
    job_mut(world, i).filter(|j| j.role == super::state::role::DONKEY).map(|j| &mut **j.donkey.get_or_insert_with(Default::default))
}

/// The donkey's D41 fill (0x577240, event 0x25a; 0x25c refreshes): the packet (0x11aa918) is
/// cleared after its +4, info code 0x18 at +4, the two slots `{good, amount}` at +8 / +0xc and
/// +0x14 / +0x18.
pub fn dialog(world: &World, i: u16) -> Option<[(u8, i32); 2]> {
    data(world, i).map(|d| d.slots)
}

/// `CDonkeyRole` vtbl+0x4c (0x575f00): no order (+0x40 == 0).
pub fn is_free(world: &World, i: u16) -> bool {
    data(world, i).is_some_and(|d| d.state == state::FREE)
}

/// `0x436990`: `b` is a building (else the original reads through a null pointer).
fn is_building(world: &World, b: u16) -> bool {
    b != 0 && world.entities.by_index(b as u32).is_some_and(|e| e.obj_type == ObjType::Building)
}

/// Building +0x48, the ensign (0 when `b` is not a building).
fn ensign(world: &World, b: u16) -> Y16X16 {
    match world.entities.by_index(b as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => d.ensign,
        _ => Y16X16(0),
    }
}

/// Entity flags `0x3000000` (birth / ready): the entity lives.
fn alive(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0)
}

fn set_displacement(world: &mut World, i: u16, v: u8) {
    if let Some(j) = job_mut(world, i) {
        j.displacement_costs = v;
    }
}

/// `CDonkeyRole::InitWalking` (0x576400: a `CWalkingNormal` of the owner, +8(−1, 0)) and `Init`
/// (0x576300: into the war map; flags 0x200, 0x2000, 0x10000, 0x20000 off, 0x4000 on; no
/// markets, no order).
pub fn init_role(world: &mut World, i: u16) {
    ActivePathing::create(world, i, crate::pathing::walking::walking_type::NORMAL);
    ActivePathing::walk_to_xy(world, i, Y16X16(u32::MAX), 0);
    super::hooks::war_map_add(world, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags = (e.flags & !0x32200) | 0x4000;
    }
    if let Some(d) = data_mut(world, i) {
        d.source = 0;
        d.target = 0;
        d.state = state::FREE;
    }
}

/// 0x576fa0: walk to `pos` as the walking task (role +4 = GO, displacement 5, one `Go`); a 0
/// position does nothing.
fn walk_to(world: &mut World, i: u16, pos: Y16X16, flags: i32) {
    if pos.0 == 0 {
        return;
    }
    ActivePathing::walk_to_xy(world, i, pos, flags);
    if let Some(j) = job_mut(world, i) {
        j.task = task_type::GO;
        j.displacement_costs = 5;
    }
    core::go(world, i);
}

/// `CDonkeyRole::GetNextJob` (0x576890): the next task; at the end the load goes down and the
/// donkey takes its wait list (0x5771d0).
pub fn get_next_job(world: &mut World, i: u16) {
    if let Some(j) = job_mut(world, i) {
        j.cursor += 1;
    }
    if core::is_end_iter(world, i) {
        drop_and_wait(world, i);
    } else {
        take_job(world, i);
    }
}

/// `CDonkeyRole::TakeJob` (0x576640).
pub fn take_job(world: &mut World, i: u16) {
    let Some(t) = core::actual_task(world, i) else { return };
    core::init_common_task_values(world, i, &t);
    match t.task {
        // GO_HOME
        14 => go_home(world, i),
        task_type::WAIT => {
            if let Some(j) = job_mut(world, i) {
                j.displacement_costs = 0;
            }
            let r = world.rng.rand();
            register(world, i, (r & 0xf) as i32 + 1);
        }
        // GO_TO_SOURCE_PILE: remember where the order started, walk to the source's ensign (a
        // vehicle order walks to its pile, flag 0x1000).
        18 => {
            let pos = world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos);
            if let Some(j) = job_mut(world, i) {
                j.start = pos;
            }
            let Some(d) = data_mut(world, i) else { return };
            d.to_source = true;
            d.to_target = false;
            let (source, pile) = (d.source, d.pile);
            if source != 0 {
                let p = ensign(world, source);
                walk_to(world, i, p, 0);
            } else if pile != 0 {
                let p = world.entities.by_index(pile as u32).map_or(Y16X16(0), |e| e.pos);
                walk_to(world, i, p, 0x1000);
            }
        }
        // GO_TO_DESTINATION_PILE: the target records the transport; loaded domestic donkeys are
        // vulnerable on the way.
        19 => {
            let Some(d) = data_mut(world, i) else { return };
            d.to_source = false;
            d.to_target = true;
            let (target, foreign) = (d.target, d.foreign);
            if target != 0 {
                crate::buildings::trading::add_transport(world, target, i, crate::buildings::trading::transport::DONKEY, foreign);
                let p = ensign(world, target);
                walk_to(world, i, p, 0);
                if has_goods(world, i)
                    && !foreign
                    && let Some(e) = world.entities.by_index_mut(i as u32)
                {
                    e.flags |= VULNERABLE;
                }
            }
        }
        // PUT_GOOD (at the target) / GET_GOOD (at the source): the market takes over.
        21 | 22 => {
            if t.task == 21
                && let Some(e) = world.entities.by_index_mut(i as u32)
            {
                e.flags &= !VULNERABLE;
            }
            set_displacement(world, i, 10);
            arrive(world, i);
        }
        // GO_TO_TARGET (list 237, called to a cart): frame 1, the destination moves by the
        // task's offset, the walk there, +0x2f set.
        task_type::GO_TO_TARGET => {
            let Some(j) = job_mut(world, i) else { return };
            j.frame = 1;
            let d = j.destination;
            let dest = Y16X16(((d.0 & 0xffff) as i32 + ((t.y as i32 + (d.0 as i32 >> 16)) << 16) + t.x as i32) as u32);
            j.destination = dest;
            walk_to(world, i, dest, 0);
            if let Some(d) = data_mut(world, i) {
                d.to_target = true;
            }
        }
        // LOAD_GOOD (list 164): displacement 10, the task's time.
        23 => {
            set_displacement(world, i, 10);
            let c = job(world, i).map_or(1, |j| j.counter as i32);
            register(world, i, c);
        }
        // ATTACK_REACTION: half the task's time less one.
        32 => {
            let c = job(world, i).map_or(0, |j| j.counter as i32);
            register(world, i, c / 2 - 1);
        }
        _ => register(world, i, 3),
    }
}

/// `CDonkeyRole::LogicUpdateJob` (0x576460). The selection refresh (flags 0x600, vtbl+0x84) is
/// the client's. Walking steps; the market tasks take the next task once the market sent the
/// donkey on. When the task changed away from walking with a vehicle or the way home pending,
/// the arrival hook runs (0x576d80); +0x2c keeps the task.
pub fn logic_update_job(world: &mut World, i: u16) {
    let task = job(world, i).map_or(0, |j| j.task);
    match task {
        task_type::GO => {
            if let Some(j) = job_mut(world, i) {
                j.distance = 0;
            }
            core::go(world, i);
        }
        21 | 22 => get_next_job(world, i),
        23 => load_vehicle(world, i),
        32 => attack_reaction(world, i),
        _ => {}
    }
    let Some(j) = job(world, i) else { return };
    let (task, d) = (j.task, j.donkey.as_deref().copied().unwrap_or_default());
    if task != d.last_task && (d.vehicle != 0 || d.going_home) && d.last_task == task_type::GO {
        arrive(world, i);
    }
    if let Some(d) = data_mut(world, i) {
        d.last_task = task;
    }
}

/// 0x576d80: the donkey arrived. A donkey walking to the source / target tells that market
/// (0x5561f0, which starts the loading / unloading); one coming home takes its wait list.
fn arrive(world: &mut World, i: u16) {
    let Some(d) = data_mut(world, i) else { return };
    d.unloaded = 0;
    let d = *d;
    if d.to_source {
        if is_building(world, d.source) {
            crate::buildings::trading::donkey_arrived(world, d.source, i);
        }
        // A vehicle order takes its goods from the pile (`ChangeAmountAndDetach`) and goes on.
        if d.vehicle_order && d.x48 == 0 {
            let good = crate::pile::get(world, d.pile).map_or(0, |p| p.good);
            crate::pile::change_amount_and_detach(world, d.pile, i);
            if let Some(m) = data_mut(world, i) {
                m.x48 = good;
                m.pile = 0;
            }
            get_next_job(world, i);
        }
    }
    let d = data(world, i).copied().unwrap_or_default();
    if d.to_target && is_building(world, d.target) {
        crate::buildings::trading::donkey_arrived(world, d.target, i);
    }
    // A donkey called to a cart (0x576f00) hitches it when the cart is still a vehicle.
    if d.vehicle > 0 {
        let cart = d.vehicle as u16;
        let vehicle = world.entities.by_index(cart as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle));
        if let Some(d) = data_mut(world, i) {
            d.vehicle = 0;
        }
        if !vehicle {
            return;
        }
        crate::vehicles::cart::hitch(world, cart, i);
    }
    if data(world, i).is_some_and(|d| d.going_home) {
        if let Some(d) = data_mut(world, i) {
            d.going_home = false;
        }
        drop_and_wait(world, i);
    }
}

/// 0x576ac0: a slot has goods.
pub fn has_goods(world: &World, i: u16) -> bool {
    data(world, i).is_some_and(|d| d.slots.iter().any(|s| s.1 != 0))
}

/// 0x576a70: nothing more fits: domestic, both slots have a good and no room; foreign, more
/// than 15 goods.
pub fn is_full(world: &World, i: u16) -> bool {
    let Some(d) = data(world, i) else { return true };
    if d.foreign {
        return d.slots[0].1 + d.slots[1].1 > 15;
    }
    d.slots.iter().all(|s| s.0 != 0 && SLOT_SIZE - s.1 < 1)
}

/// 0x5768e0: load `n` goods of `good`: first the slots of the good, then empty slots (8 each,
/// counted in +0x70). Returns what did not fit.
pub fn load(world: &mut World, i: u16, good: u8, n: i32) -> i32 {
    let Some(d) = data_mut(world, i) else { return n };
    let mut n = n;
    for s in d.slots.iter_mut() {
        let room = SLOT_SIZE - s.1;
        if s.0 == good && room != 0 {
            let k = n.min(room);
            s.1 += k;
            n -= k;
        }
    }
    if n != 0 {
        for s in d.slots.iter_mut() {
            if s.0 == 0 && n != 0 {
                s.0 = good;
                let k = n.min(SLOT_SIZE);
                s.1 += k;
                d.loaded += k;
                n -= k;
            }
        }
    }
    n
}

/// 0x5769a0: take up to `n` goods of `good` out of the slots (a slot left empty forgets its
/// good), counted in +0x6c. Returns what was not there.
pub fn unload(world: &mut World, i: u16, good: u8, n: i32) -> i32 {
    if n < 0 {
        return 0;
    }
    let Some(d) = data_mut(world, i) else { return n };
    let mut n = n;
    for s in d.slots.iter_mut() {
        if s.0 == good {
            let k = s.1.min(n);
            s.1 -= k;
            if s.1 == 0 {
                s.0 = 0;
            }
            n -= k;
            d.unloaded += k;
        }
    }
    n
}

/// The goods of `good` in the slots (the sum the market and 0x576b10 compute).
pub fn amount_of(world: &World, i: u16, good: u8) -> i32 {
    data(world, i).map_or(0, |d| d.slots.iter().filter(|s| s.0 == good).map(|s| s.1).sum())
}

/// 0x576b10: drop the load where the donkey stands (`SearchSpaceForGoods`, at most 8 at a time)
/// and become invulnerable.
fn drop_load(world: &mut World, i: u16) {
    let Some(pos) = world.entities.by_index(i as u32).map(|e| e.pos) else { return };
    for g in 0..crate::eco::GOODS as u8 {
        let mut n = amount_of(world, i, g);
        while n != 0 {
            let k = n.min(SLOT_SIZE);
            crate::pile::search_space_for_goods(world, pos.x(), pos.y(), g, k as u8);
            unload(world, i, g, k);
            n -= k;
        }
    }
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !VULNERABLE;
    }
}

/// Role vtbl+0x44 (0x576360) `(damage)`: a loaded donkey (a slot amount, +0x54 / +0x60) that is
/// not reacting already takes list 162 (`JOB_DONKEY_ATTACK_REACTION`); the damage is always 0.
pub fn hit(world: &mut World, i: u16) -> i32 {
    if !has_goods(world, i) || job(world, i).is_some_and(|j| j.task == 32) {
        return 0;
    }
    core::new_shared_list(world, i, 162);
    0
}

/// Task 32 of `LogicUpdateJob` (0x576460): the task's time (+6) goes down by half the frames
/// (+7 >> 1) a step; while at least that is left the load goes down (0x576b10, the first step
/// drops it all) and the next step is in half the frames less one. Then a trade transport
/// turns to its source (+0x34 = +0x38, +0x38 = 0) and the next task follows.
fn attack_reaction(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    let half = j.cycle_frames >> 1;
    j.counter = j.counter.wrapping_sub(half as i8);
    if half as i32 <= j.counter as i32 {
        drop_load(world, i);
        register(world, i, half as i32 - 1);
        return;
    }
    if let Some(d) = data_mut(world, i)
        && d.source != 0
    {
        d.target = d.source;
        d.source = 0;
    }
    get_next_job(world, i);
}

/// `CDonkeyRole::UpdateJob` (0x576610): task 32 animates its frames (+0x24 += the ticks since
/// the last update, back to 0 at the frame count).
pub fn update_job(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    if j.task != 32 {
        return;
    }
    j.frame = j.frame.wrapping_add(j.elapsed as u8);
    if j.frame >= j.cycle_frames {
        j.frame = 0;
    }
}

/// 0x5771d0: a load goes down, then the wait list.
pub fn drop_and_wait(world: &mut World, i: u16) {
    if has_goods(world, i) {
        drop_load(world, i);
    }
    core::take_wait_list(world, i);
}

/// 0x577010: back to where the order started (+0x1c) when that is in the donkey's eco-sector:
/// the load goes down, the wait list, then the walk (+0x78 set: the arrival takes the wait list
/// again).
pub fn go_home(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let pos = e.pos;
    let home = job(world, i).map_or(Y16X16(0), |j| j.start);
    let eco = |p: Y16X16| world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.tiling.eco_sector_of(n));
    if eco(home) != eco(pos) {
        return;
    }
    drop_and_wait(world, i);
    if let Some(j) = job_mut(world, i) {
        j.frame = 1;
    }
    ActivePathing::walk_to_xy(world, i, home, 0);
    if let Some(j) = job_mut(world, i) {
        j.task = task_type::GO;
        j.displacement_costs = 5;
    }
    core::go(world, i);
    if let Some(d) = data_mut(world, i) {
        d.going_home = true;
    }
}

/// 0x576bb0: the target is gone or the transport is called off. The target forgets the
/// transport; with the source still a building the donkey takes its goods back there (the
/// source becomes the target and records the transport), else it drops them, goes home and is
/// free.
pub fn target_gone(world: &mut World, i: u16) {
    use crate::buildings::trading;
    let Some(d) = data(world, i).copied() else { return };
    if d.to_target && d.target != 0 && is_building(world, d.target) {
        trading::remove_transport(world, d.target, i, trading::transport::DONKEY);
    }
    let src = d.source;
    if let Some(d) = data_mut(world, i) {
        d.target = 0;
    }
    if !is_building(world, src) {
        drop_and_wait(world, i);
        go_home(world, i);
        set_displacement(world, i, 5);
        if let Some(d) = data_mut(world, i) {
            d.target = 0;
            d.state = state::FREE;
        }
        return;
    }
    if let Some(d) = data_mut(world, i) {
        d.source = 0;
        d.target = src;
    }
    trading::add_transport(world, src, i, trading::transport::DONKEY, d.foreign);
    let p = ensign(world, src);
    walk_to(world, i, p, 0);
}

/// 0x576d10: the source is gone or the transport is called off: the source forgets it; without
/// a target the donkey is free and goes home.
pub fn source_gone(world: &mut World, i: u16) {
    use crate::buildings::trading;
    let Some(d) = data(world, i).copied() else { return };
    if let Some(dm) = data_mut(world, i) {
        dm.source = 0;
    }
    if d.to_source && is_building(world, d.source) {
        trading::remove_transport(world, d.source, i, trading::transport::DONKEY);
    }
    if d.target != 0 {
        return;
    }
    set_displacement(world, i, 5);
    if let Some(d) = data_mut(world, i) {
        d.state = state::FREE;
    }
    go_home(world, i);
}

/// 0x576f00 `(pos, cart)`: a finished cart calls the donkey (`CCart` 0x55dc50): list 237
/// (`JOB_DONKEY_COME_TO_VEHICLEHALL`), its first task (`TakeJob`), the walk to `pos` (GO,
/// displacement 5, one `Go`; none for a 0 position), +0x3c the cart, state 5. The arrival
/// (0x576d80) hitches the cart.
pub fn come_to_vehicle(world: &mut World, i: u16, pos: Y16X16, cart: u16) {
    core::new_shared_list(world, i, LIST_COME_TO_VEHICLE);
    take_job(world, i);
    walk_to(world, i, pos, 0);
    if let Some(d) = data_mut(world, i) {
        d.vehicle = cart as i32;
        d.state = state::VEHICLE;
    }
}

/// Event 11 (0x577330 case 4) `[goods, pile, vehicle]`, a free or called donkey (state 0 / 5):
/// at most the pile's goods (none: nothing); the vehicle's place (vtbl+0x8c) — none or a dead
/// vehicle: the load down and the wait list (0x5771d0); else the destination, list 164
/// (`JOB_DONKEY_TRANSPORT_GOOD_TO_VEHICLE`), the pile reserves the goods
/// (`AttachAndIncAmountLeaving(n, 2)`), +0x44 = n, the vehicle attaches it (vtbl+0xa4) and counts
/// the order (vtbl+0x78), +0x2d, state 1. A busy donkey: the vehicle forgets it (vtbl+0x7c).
fn vehicle_order(world: &mut World, i: u16, ev: &EntityEvent) {
    let Some(d) = data_mut(world, i) else { return };
    d.to_target = false;
    if d.state != state::FREE && d.state != state::VEHICLE {
        let v = ev.data[2] as u16;
        if crate::vehicles::catapult::is_catapult(world, v) {
            crate::vehicles::catapult::carrier_gone(world, v);
        }
        return;
    }
    let (pile, vehicle) = (ev.data[1] as u16, ev.data[2] as u16);
    d.pile = pile;
    d.vehicle2 = vehicle;
    d.x48 = 0;
    let amount = crate::pile::get(world, pile).map_or(0, |p| p.amount as i32);
    let n = ev.data[0].min(amount);
    if n == 0 {
        return;
    }
    let place = crate::vehicles::exit_place(world, vehicle, i);
    let alive = world.entities.by_index(vehicle as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0);
    if !alive || place == 0 {
        drop_and_wait(world, i);
        return;
    }
    let pos = world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos);
    if let Some(j) = job_mut(world, i) {
        j.start = pos;
        j.destination = Y16X16(place as u32);
    }
    core::new_shared_list(world, i, LIST_TO_VEHICLE);
    crate::pile::attach_and_inc_amount_leaving(world, pile, i, n, crate::pile::target::PILE_SRC);
    if let Some(d) = data_mut(world, i) {
        d.x44 = n;
    }
    crate::vehicles::attach(world, vehicle, i);
    let good = crate::pile::get(world, pile).map_or(0, |p| p.good) as i32;
    crate::vehicles::good_ordered(world, vehicle, good, n);
    if let Some(d) = data_mut(world, i) {
        d.vehicle_order = true;
        d.state = state::TO_TARGET;
    }
}

/// `LogicUpdateJob` LOAD_GOOD (0x576460 case 0x17): the vehicle gets the goods (vtbl+0x84) and
/// detaches the donkey (vtbl+0x40); the load down and the wait list (0x5771d0), home (0x577010),
/// no target, displacement 5, free.
fn load_vehicle(world: &mut World, i: u16) {
    let Some(d) = data(world, i).copied() else { return };
    crate::vehicles::good_arrived(world, d.vehicle2, d.x48 as i32, d.x44);
    crate::vehicles::detach_settler(world, d.vehicle2, i);
    if let Some(m) = data_mut(world, i) {
        m.vehicle2 = 0;
        m.x44 = 0;
    }
    drop_and_wait(world, i);
    go_home(world, i);
    set_displacement(world, i, 5);
    if let Some(m) = data_mut(world, i) {
        m.target = 0;
        m.state = state::FREE;
    }
}

/// 0x576eb0 `(pos)`: a donkey bringing goods to a vehicle gets a new destination; on its way
/// there it walks to it at once.
pub fn vehicle_moved(world: &mut World, i: u16, pos: Y16X16) {
    let Some(d) = data(world, i).copied() else { return };
    if d.vehicle2 == 0 {
        return;
    }
    if let Some(j) = job_mut(world, i) {
        j.destination = pos;
    }
    if d.to_target {
        ActivePathing::walk_to_xy(world, i, pos, 0);
        core::go(world, i);
    }
}

/// The donkey side of the market's order (0x5558b0 and 0x577620): source, target, state 2, the
/// position the order started at; then event 10. Foreign orders are not ported.
pub fn order(world: &mut World, i: u16, source: u16, target: u16) {
    let pos = world.entities.by_index(i as u32).map_or(Y16X16(0), |e| e.pos);
    if let Some(d) = data_mut(world, i) {
        d.source = source;
        d.target = target;
        d.state = state::TO_SOURCE;
    }
    if let Some(j) = job_mut(world, i) {
        j.start = pos;
    }
}

/// 0x5770d0 (a foreign transport called off): the walker heads for the source's ensign on the
/// way there (leg 1), the target's on the way back (leg 2), else where the order started (the
/// task is not changed); source and target swap, no longer foreign, no leg, home pending.
pub fn foreign_return(world: &mut World, i: u16) {
    let Some(d) = data(world, i).copied() else { return };
    let mut pos = job(world, i).map_or(Y16X16(0), |j| j.start);
    if d.leg == 1 && d.source != 0 {
        pos = ensign(world, d.source);
    }
    if d.leg == 2 && d.target != 0 {
        pos = ensign(world, d.target);
    }
    ActivePathing::walk_to_xy(world, i, pos, 0);
    if let Some(m) = data_mut(world, i) {
        std::mem::swap(&mut m.source, &mut m.target);
        m.foreign = false;
        m.leg = 0;
        m.going_home = true;
    }
}

/// The second half of the order (after the market recorded the transport): event 10
/// (`CEntityEvent(10, 0, 0, 0, market)`), then 0x577620.
pub fn send_order_event(world: &mut World, i: u16, market: u16, foreign: bool) {
    core::set_event(world, i, EntityEvent { event: 10, kind: 0, data: [0, 0, market as i32] });
    set_leg(world, i, foreign);
}

/// 0x577620: a domestic order has no leg (+0x74 = 0); a foreign one is not vulnerable (flags
/// 0x30000 off) and goes to the next leg (0 / 2 → 1 there, 1 → 2 back); +0x68 = `foreign`.
pub fn set_leg(world: &mut World, i: u16, foreign: bool) {
    if foreign && let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !VULNERABLE;
    }
    if let Some(d) = data_mut(world, i) {
        if !foreign {
            d.leg = 0;
        } else {
            d.leg = if d.leg == 1 { 2 } else if d.leg == 0 || d.leg == 2 { 1 } else { d.leg };
        }
        d.foreign = foreign;
    }
}

/// `CDonkeyRole::ConvertEventIntoGoal` (0x577330).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        // SUBJECT_STOPPED / TARGET_DIE: a vehicle order's pile or vehicle, else a market.
        7 | 9 => {
            let Some(d) = data(world, i).copied() else { return };
            // A vehicle order: its pile gone, detached from it; its vehicle gone, detached and
            // home (0x577010).
            if d.vehicle_order {
                if ev.data[0] == d.pile as i32 && d.pile != 0 {
                    crate::pile::detach(world, d.pile, i);
                    if let Some(m) = data_mut(world, i) {
                        m.pile = 0;
                    }
                }
                if ev.data[0] == d.vehicle2 as i32 && d.vehicle2 != 0 {
                    let t = world.entities.by_index(d.vehicle2 as u32).map(|e| e.obj_type);
                    match t {
                        Some(ObjType::Ship | ObjType::LandVehicle) => crate::vehicles::detach_settler(world, d.vehicle2, i),
                        Some(ObjType::Pile) => crate::pile::detach(world, d.vehicle2, i),
                        _ => {}
                    }
                    if let Some(m) = data_mut(world, i) {
                        m.vehicle2 = 0;
                    }
                    go_home(world, i);
                }
                if let Some(d) = data_mut(world, i) {
                    d.vehicle_order = false;
                }
                return;
            }
            let id = ev.data[0];
            if id == d.target as i32 {
                target_gone(world, i);
            } else if id == d.source as i32 {
                source_gone(world, i);
                register(world, i, 1);
            }
        }
        // The trade order: with both markets alive, `JOB_DONKEY_TRANSPORT_GOOD`; a dead one is
        // handled as gone.
        10 => {
            let Some(d) = data_mut(world, i) else { return };
            d.vehicle_order = false;
            d.going_home = false;
            let (source, target) = (d.source, d.target);
            if source == 0 || target == 0 {
                return;
            }
            let (src_alive, dst_alive) = (alive(world, source), alive(world, target));
            if src_alive && dst_alive {
                core::new_shared_list(world, i, LIST_TRANSPORT_GOOD);
                return;
            }
            if !src_alive {
                source_gone(world, i);
            }
            if !dst_alive {
                target_gone(world, i);
            }
        }
        11 => vehicle_order(world, i, ev),
        _ => {
            let registered = world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0);
            if !registered {
                register(world, i, 1);
            }
        }
    }
}

/// 0x4fa050 for donkeys: the nearest free donkey (`CSettlerMgr` list of `player`, settler type
/// 60, head first; ties keep the first) with flag 0x1000 on the same sector as (x, y), by hex
/// distance below 0xffff.
pub fn nearest_free(world: &World, player: u8, x: i32, y: i32) -> u16 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let sector = world.tiling.sector_of(n);
    let (mut best, mut best_d) = (0u16, 0xffff);
    for id in crate::settler_lists::iter(world, player, crate::buildings::trading::DONKEY_SETTLER) {
        let Some(e) = world.entities.by_index(id as u32) else { continue };
        if e.obj_type != ObjType::Settler {
            continue;
        }
        let Some(en) = world.grid.checked_index(e.x(), e.y()) else { continue };
        if world.tiling.sector_of(en) != sector || !is_free(world, id) || e.flags & 0x1000 == 0 {
            continue;
        }
        let d = crate::spiral::grid_distance(x - e.x(), y - e.y());
        if d < best_d {
            best_d = d;
            best = id;
        }
    }
    best
}
