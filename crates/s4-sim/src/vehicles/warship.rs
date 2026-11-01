//! `CWarShip` and its race classes (`CRomanWarShip` vtable 0x105cc20, `CVikingWarShip` 0x105ca10,
//! `CMayaWarShip` 0x105cb0c, `CTrojanWarShip` 0x105c8fc; 0x592000..0x593800, decompiles
//! `tools/re/vehicles/warship.c`, `warship2.c`). A `CShip` with a `CWarriorBehavior` at +0x98
//! (ctor 0x592070: row table 0x11cdc18, +0xb0 = 1 "takes ammunition", `SetMode(−1, 0)`): the
//! behaviour is the catapults' ([`super::catapult`]: the same `SetMode` 0x598290 and vehicle
//! update 0x598a70; its slots 0x592750 walk step, 0x592720 drift step, 0x592780 shot with gfx
//! job 1, 0x5927f0 next task are the catapult's 0x561360 / 0x561310 / 0x561390 / 0x561400 with
//! the ship's job and without a transporter).
//!
//! * **Update** (vtbl+0x90, 0x592530): "no ammunition" (vtbl+0xcc: +0x63, the Vikings the
//!   player's mana, 0x592e60) and "reloading" (`now < +0x68`) are taken first; every 21 ticks
//!   (+0x94) ammunition from a shipyard's pile (0x5928d0 / 0x592950) and the ships' repair; then
//!   the behaviour.
//! * **Ammunition** (0x5928d0): with +0xb0 and room below `MaxAmmo`, the nearest pile (not for
//!   the Vikings) of the ammunition good (info +0x50) with goods not leaving of a READY shipyard
//!   (type 0x1f) **of the ship's race** (not owner: the original compares the race nibble)
//!   within 20 (0x592950); per good 4 (the Maya 8, vtbl+0xdc), at most what fits; the pile gives
//!   `n / per` goods, the ship takes `n`.
//! * **Next task** (vtbl+0xbc, 0x5925c0): standing, then `SetMode` from the task: GO 0x200000,
//!   GO_TO_POS / WAYPOINT 0x100000 (a −1 waypoint becomes the current place), 10 (follow) the
//!   task's entity with 0x600000, 0x24 deletes the ship, none / anything else `SetMode(−1, 0)`;
//!   then the update.
//! * **A move order** (vtbl+0x6c, 0x5923b0) on an enemy within spiral radius 2 attacks it
//!   (0x592800, mode 0x700000); else the task list.

use super::{catapult, get, get_mut, logic};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;
use crate::jobs::state::EntityEvent;

/// The shipyard (0x590a90 / 0x592950).
const SHIPYARD: u16 = 0x1f;
/// `Vehicle/ShipRepairRate` (0x11cdbf8).
const REPAIR_RATE: u8 = 1;

pub fn is_warship(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Ship && e.kind == super::kind::WARSHIP) && catapult::data(world, id).is_some()
}

/// The ctor 0x592070 after `CShip`'s: the behaviour, +0xb0 = 1, `SetMode(−1, 0)`.
pub fn init(world: &mut World, id: u16) {
    if let Some(v) = get_mut(world, id) {
        v.catapult = Some(Box::new(catapult::CatapultData { may_load: true, ..catapult::CatapultData::default() }));
    }
    catapult::set_mode(world, id, -1, 0);
}

/// vtbl+0x90 (0x592530), see the module doc.
pub fn update(world: &mut World, id: u16) {
    let now = world.tick;
    let next = get(world, id).map_or(0, |v| v.next_tick as u32);
    let f = if catapult::has_ammo(world, id) { 0 } else { 2 } | (now < next) as u8;
    let last = get(world, id).map_or(0, |v| v.x9c as u32);
    if last.wrapping_add(0x14) < now {
        ammo_from_shipyard(world, id);
        if super::ship::shipyard_near(world, id) {
            let Some(e) = world.entities.by_index(id as u32) else { return };
            let max = world.rules.vehicles.get(e.race, e.kind as u8).hitpoints;
            if let Some(v) = get_mut(world, id)
                && (v.health as i32) < max
            {
                v.health = v.health.wrapping_add(REPAIR_RATE);
            }
        }
        if let Some(v) = get_mut(world, id) {
            v.x9c = now as i32;
        }
    }
    catapult::behavior_update(world, id, now, f);
}

/// 0x5928d0 with 0x592950, see the module doc.
fn ammo_from_shipyard(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (race, k, p) = (e.race, e.kind, e.pos);
    let info = world.rules.vehicles.get(race, k as u8);
    let cargo = get(world, id).map_or(0, |v| v.cargo) as i32;
    if !catapult::data(world, id).is_some_and(|c| c.may_load) || cargo >= info.max_ammo || race == 1 {
        return;
    }
    let good = info.x50;
    let (mut best, mut best_d) = (0u16, 0x4000);
    for b in crate::magic::spells::buildings_within(world, p.x(), p.y(), 0x14) {
        let Some(be) = world.entities.by_index(b as u32) else { continue };
        if be.kind != SHIPYARD || be.race != race || crate::buildings::workshop::state(world, b).is_none() {
            continue;
        }
        let Ok(g) = u8::try_from(good) else { continue };
        let pile = crate::buildings::workshop::pile_with_good(world, b, g);
        let Some(d) = crate::pile::get(world, pile).filter(|_| pile != 0) else { continue };
        if d.amount as i32 - d.leaving as i32 <= 0 {
            continue;
        }
        let pp = world.entities.by_index(pile as u32).map_or(Y16X16(0), |e| e.pos);
        let dist = crate::spiral::grid_distance(pp.x() - p.x(), pp.y() - p.y());
        if dist < best_d {
            best_d = dist;
            best = pile;
        }
    }
    if best == 0 {
        return;
    }
    let Some(d) = crate::pile::get(world, best) else { return };
    let per = catapult::per_good(world, id);
    let n = (per * (d.amount as i32 - d.leaving as i32)).min(info.max_ammo - cargo);
    crate::pile::decrease_unforeseen(world, best, n / per);
    if let Some(v) = get_mut(world, id) {
        v.cargo = v.cargo.wrapping_add(n as u8);
    }
}

/// vtbl+0xbc (0x5925c0), see the module doc.
pub fn next_task(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags |= flag::VISIBLE;
    let (race, k, pos) = (e.race, e.kind, e.pos);
    let job = world.rules.vehicles.get(race, k as u8).job as u16;
    let frames = world.rules.vehicles.frame_count(race, job);
    let Some(v) = get_mut(world, id) else { return };
    v.task = logic::task::IDLE;
    v.job_part = job;
    v.frame = 0;
    v.half_frame = 0;
    v.frames = frames;
    v.b5c = 8;
    v.distance = 0;
    let k = v.listed as usize;
    let t = (k != 0).then(|| v.tasks.get(k - 1).copied()).flatten();
    let at = |t: super::VehicleTask| Y16X16::pack(t.x as i32, t.y as i32).0 as i32;
    match t.map(|t| t.task) {
        Some(logic::task::GO) => catapult::set_mode(world, id, at(t.unwrap_or_default()), 0x20_0000),
        Some(logic::task::WAYPOINT) | Some(logic::task::GO_TO_POS) => {
            if let Some(v) = get_mut(world, id)
                && let Some(t) = v.tasks.get_mut(k - 1)
                && t.task == logic::task::WAYPOINT
                && (t.y as u16 | t.x as u16) > 0x7fff
            {
                t.x = pos.x() as i16;
                t.y = pos.y() as i16;
            }
            let t = get(world, id).and_then(|v| v.tasks.get(k - 1).copied()).unwrap_or_default();
            catapult::set_mode(world, id, at(t), 0x10_0000);
        }
        Some(10) => catapult::set_mode(world, id, t.unwrap_or_default().entity as i32, 0x60_0000),
        Some(0x24) => {
            if let Some(e) = world.entities.by_index_mut(id as u32) {
                e.flags &= !flag::VISIBLE;
            }
            super::delete(world, id);
            return;
        }
        _ => catapult::set_mode(world, id, -1, 0),
    }
    super::ship::update(world, id);
}

/// vtbl+0x6c (0x5923b0): a move order (0x11 / 0xd) on an enemy near the place attacks it
/// (0x592800); otherwise the task list (0xd: data 1 / 2; 0x11 / 0xd: packed data 1). Returns
/// whether a new list started.
pub fn convert_event(world: &mut World, id: u16, ev: &EntityEvent) -> bool {
    if ev.event == 0x11 && ev.kind == 0xd && catapult::attack_at(world, id, Y16X16(ev.data[1] as u32)) {
        return false;
    }
    match ev.event {
        0xd => logic::new_task_list(world, id, ev.data[1], ev.data[2], ev.data[0] as u32),
        0x11 if ev.kind == 0xd => {
            let p = Y16X16(ev.data[1] as u32);
            logic::new_task_list(world, id, p.x(), p.y(), ev.data[0] as u32)
        }
        _ => false,
    }
}
