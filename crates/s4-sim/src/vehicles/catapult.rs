//! `CCatapult` and its race classes (`CRomanCatapult` vtable 0x1059a1c, `CVikingCatapult`
//! 0x1059b20, `CMayaCatapult` 0x1059814, `CTrojanCatapult` 0x1059918; 0x560660..0x562200,
//! decompiles `tools/re/vehicles/catapult.c`, `catapult2.c`, `catbehav.c`, `catshot.c`,
//! `catmisc.c`, `effects*.c`). A `CWheeler` with a `CWarriorBehavior` at +0xa0 (its vtable
//! 0x598290 SetMode, 0x561360 walk step, 0x561310 idle step, 0x561390 shoot, 0x561400 end) and
//! +0xb8 "takes ammunition from piles next to it", +0xbc the carrier / donkey bringing
//! ammunition, +0xc0 the tick of the last ammunition / repair step.
//!
//! * **Update** (vtbl+0x90 0x5610c0): every 21 ticks ammunition from a pile of its ammunition
//!   good (info +0x50) within spiral radius 5 (0x560c90 / 0x561240; up to `MaxAmmo` +0x54, per
//!   good vtbl+0xe4: 4, the Maya 8) and repair by `WheelerRepairRate` (1) next to a vehicle
//!   hall within radius 15 (0x59a720); then, not on board, the behaviour's vehicle update
//!   0x598a70 with "no ammunition" (vtbl+0xcc: +0x63 != 0; the Vikings: the player's mana at
//!   least `VikingWarmachineShotCost`) and "reloading" (`now < +0x68`).
//! * **The behaviour** ([`behavior_update`] 0x598a70): states 0 / 1 / 7 search every 20 ticks
//!   (40 after nothing) for the best enemy (0x5999c0: war-map cells around, first the vehicles
//!   by the row's priority per warrior type, +10 within range, then the buildings with priority
//!   row +0x58) and shoot one above 10; state 4 walks to a position; 5 attacks one target (a
//!   firing place near it, 0x599e30; in range row +0x1c..+0x20, 0x599db0); 8 and the rest end
//!   (0x561400 → the next task).
//! * **Shot** (0x561390 → 0x595010): the catapult turns to the target, `CFutureEvents` type 6 in
//!   `info +0x48 + turn` ticks, the next shot after `ReadyToFireDelay` (+0x4c), one ammunition
//!   (or `VikingWarmachineShotCost` mana, 0x4f59f0), the shooting animation (job 0x23, task
//!   0x10). Event 6 (0x4e53f0 → 0x594c80) while the catapult is READY: the missile (effects
//!   0x4e3b20, info +0x3c; the Viking's effect 0x17) and in its flight time + 1 the damage event
//!   2: `strength · Damage + 127 >> 8` (at least 1; own-land or foreign strength by the
//!   catapult's land) from the owner.
//! * **Damage** (vtbl+0x20 0x5956d0): above the armour the excess halved (at least 1), the hit
//!   points down, at 0 killed.
//! * **Next task** (vtbl+0xbc 0x561150): standing, then `SetMode` from the task: GO 0x200000,
//!   GO_TO_POS / WAYPOINT 0x100000, none idle.
//! * **A move order** (0x560f80) on an enemy within spiral radius 2 of the place attacks it
//!   (0x5614b0, mode 0x700000) instead.
//!
//! The missile flight time is the effects manager's (`CEffects` record +0xc), from
//! [`crate::projectile::launch`], assuming a free effect record (with a full record pool the
//! original's delay is 1) [inf].

use super::{get, get_mut, kind};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// `CWarriorBehavior` row of the catapults (their warrior type, info +0x10).
const ROW: usize = 8;
/// The shooting animation (job 0x23) and its task.
const SHOOT_JOB: u16 = 0x23;
const TASK_ANIMATION: u8 = 0x10;
/// `Vehicle/WheelerRepairRate` (static config var 0x11cdc14, set by 0x4250c0).
const REPAIR_RATE: u8 = 1;
/// `CFutureEvents` type of a shot (0x4e53f0).
pub const FUTURE_SHOT: u8 = 6;
/// Building type of the vehicle hall (0x59a720).
const VEHICLE_HALL: u16 = 0x17;
/// The Viking war machine's impact effect (0x594c80).
const VIKING_EFFECT: usize = 0x17;

/// The vehicle part of `CWarriorBehavior` (+0xa0: +4 row, +8 state, +9 flags, +0xa target,
/// +0xc position, +0x10 mode, +0x14 the tick of the next search; not stored).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Behavior {
    pub row: u8,
    pub state: u8,
    pub bflags: u8,
    pub target: u16,
    pub pos: i32,
    pub mode: u32,
    pub next_search: u32,
}

/// The `CCatapult` members.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CatapultData {
    pub behavior: Behavior,
    /// +0xb8 (1 from the ctor), +0xbc, +0xc0.
    pub may_load: bool,
    pub carrier: i32,
    pub last_tick: u32,
}

impl CatapultData {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        let b = &self.behavior;
        for v in [b.row, b.state, b.bflags, self.may_load as u8] {
            h.u8(v);
        }
        h.u16(b.target);
        for v in [b.pos as u32, b.mode, b.next_search, self.carrier as u32, self.last_tick] {
            h.u32(v);
        }
    }
}

pub fn data(world: &World, id: u16) -> Option<&CatapultData> {
    get(world, id)?.catapult.as_deref()
}

fn data_mut(world: &mut World, id: u16) -> Option<&mut CatapultData> {
    get_mut(world, id)?.catapult.as_deref_mut()
}

fn bh(world: &mut World, id: u16) -> Option<&mut Behavior> {
    data_mut(world, id).map(|c| &mut c.behavior)
}

fn row(world: &World, id: u16) -> &'static [u32; 25] {
    let r = data(world, id).map_or(ROW, |c| c.behavior.row as usize);
    &crate::jobs::warrior::ROWS[r.min(crate::jobs::warrior::ROWS.len() - 1)]
}

pub(crate) fn is_catapult(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::LandVehicle && e.kind == kind::WAR_MACHINE)
}

fn pos_of(world: &World, id: u16) -> Y16X16 {
    world.entities.by_index(id as u32).map_or(Y16X16(0), |e| e.pos)
}

fn dist(a: Y16X16, b: Y16X16) -> i32 {
    crate::spiral::grid_distance(b.x() - a.x(), b.y() - a.y())
}

// ---- the class ----

/// The ctor (0x560860) after `CWheeler`'s: +0xb8 = 1, `SetMode(−1, 0)`; a finished catapult
/// takes its first task (0x561150).
pub fn init(world: &mut World, id: u16, under_construction: bool) {
    if let Some(v) = get_mut(world, id) {
        v.catapult = Some(Box::new(CatapultData { may_load: true, ..CatapultData::default() }));
    }
    set_mode(world, id, -1, 0);
    if !under_construction {
        next_task(world, id);
    }
}

/// vtbl+0xcc: ammunition left (+0x63); the Vikings (0x561b90): the player's mana is at least
/// `VikingWarmachineShotCost` (at least 1).
pub(super) fn has_ammo(world: &World, id: u16) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    if e.race == 1 {
        let cost = world.rules.magic.viking_warmachine_shot_cost.max(1) as u32;
        return crate::magic::mana(world, e.owner) as u32 >= cost;
    }
    get(world, id).is_some_and(|v| v.cargo != 0)
}

/// vtbl+0xd4: one ammunition (0x594c20); the Vikings' shot cost (0x561bf0 → 0x4f59f0).
fn use_ammo(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.race == 1 {
        let owner = e.owner;
        crate::magic::warmachine_shot(world, owner);
        return;
    }
    if let Some(v) = get_mut(world, id)
        && v.cargo != 0
    {
        v.cargo -= 1;
    }
}

/// vtbl+0xe4: ammunition per good (0x5607f0: 4; the Maya 0x560820: 8).
pub(super) fn per_good(world: &World, id: u16) -> i32 {
    if world.entities.by_index(id as u32).is_some_and(|e| e.race == 2) { 8 } else { 4 }
}

/// vtbl+0x8c (0x561580): the first ring-2 node behind the catapult free of footprints (as the
/// cart's 0x55ae60, without the READY test).
pub fn exit_place(world: &World, id: u16) -> i32 {
    let Some(e) = world.entities.by_index(id as u32) else { return 0 };
    let dir = get(world, id).map_or(0, |v| v.dir as i32);
    for d in dir..dir + 6 {
        let i = (((d.rem_euclid(6)) + 3) % 6) as usize * 2 + 8;
        let (x, y) = (e.pos.x() + crate::spiral::dx(i), e.pos.y() + crate::spiral::dy(i));
        let Some(n) = world.grid.checked_index(x, y) else { continue };
        let m = world.layers.map_object[n];
        if m == 0 || world.entities.by_index(m as u32).is_some_and(|o| o.obj_type != ObjType::None) {
            return Y16X16::pack(x, y).0 as i32;
        }
    }
    0
}

/// vtbl+0x84 (0x560f50) `(good, n)`: `n` goods of ammunition arrived (+0x63 += per good · n, no
/// cap), the transporter is gone (+0xbc = 0).
pub fn ammo_arrived(world: &mut World, id: u16, n: i32) {
    let per = per_good(world, id);
    if let Some(c) = data_mut(world, id) {
        c.carrier = 0;
    }
    if let Some(v) = get_mut(world, id) {
        v.cargo = v.cargo.wrapping_add((per * n) as u8);
    }
}

/// +0xbc: the transporter bringing ammunition (`CatapultLoad`).
pub fn set_carrier(world: &mut World, id: u16, t: i32) {
    if let Some(c) = data_mut(world, id) {
        c.carrier = t;
    }
}

/// vtbl+0x7c / +0x88 (0x560f30 / 0x560f40): the transporter is forgotten.
pub fn carrier_gone(world: &mut World, id: u16) {
    if let Some(c) = data_mut(world, id) {
        c.carrier = 0;
    }
}

/// vtbl+0xbc (0x561150): standing (VISIBLE, task 0x11, the idle job), then the behaviour's mode
/// from the current task — none: idle; GO 0x200000 to it; GO_TO_POS 0x100000; WAYPOINT
/// 0x100000 (a −1 place becomes the current position) — and the update (vtbl+0x90).
pub fn next_task(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags |= flag::VISIBLE;
    let (race, k, pos) = (e.race, e.kind, e.pos);
    let job = world.rules.vehicles.get(race, k as u8).job as u16;
    let frames = world.rules.vehicles.frame_count(race, job);
    let Some(v) = get_mut(world, id) else { return };
    v.task = super::logic::task::IDLE;
    v.job_part = job;
    v.frame = 0;
    v.half_frame = 0;
    v.frames = frames;
    v.b5c = 8;
    v.distance = 0;
    let t = (v.listed != 0).then(|| v.tasks.get(v.listed as usize - 1).copied()).flatten();
    match t.map(|t| t.task) {
        Some(super::logic::task::GO) => {
            let t = t.unwrap_or_default();
            set_mode(world, id, Y16X16::pack(t.x as i32, t.y as i32).0 as i32, 0x20_0000);
        }
        Some(super::logic::task::GO_TO_POS) => {
            let t = t.unwrap_or_default();
            set_mode(world, id, Y16X16::pack(t.x as i32, t.y as i32).0 as i32, 0x10_0000);
        }
        Some(super::logic::task::WAYPOINT) => {
            let k = get(world, id).map_or(0, |v| v.listed as usize - 1);
            if let Some(v) = get_mut(world, id)
                && let Some(t) = v.tasks.get_mut(k)
                && (t.y as u16 | t.x as u16) > 0x7fff
            {
                t.x = pos.x() as i16;
                t.y = pos.y() as i16;
            }
            let t = get(world, id).and_then(|v| v.tasks.get(k).copied()).unwrap_or_default();
            set_mode(world, id, Y16X16::pack(t.x as i32, t.y as i32).0 as i32, 0x10_0000);
        }
        _ => set_mode(world, id, -1, 0),
    }
    update(world, id);
}

/// vtbl+0x90 (0x5610c0).
pub fn update(world: &mut World, id: u16) {
    let now = world.tick;
    let last = data(world, id).map_or(0, |c| c.last_tick);
    if last.wrapping_add(0x14) < now {
        load_from_piles(world, id);
        if hall_near(world, id) {
            let Some(e) = world.entities.by_index(id as u32) else { return };
            let max = world.rules.vehicles.get(e.race, e.kind as u8).hitpoints;
            if let Some(v) = get_mut(world, id)
                && (v.health as i32) < max
            {
                v.health = v.health.wrapping_add(REPAIR_RATE);
            }
        }
        if let Some(c) = data_mut(world, id) {
            c.last_tick = now;
        }
    }
    let next = get(world, id).map_or(0, |v| v.next_tick as u32);
    let f = if has_ammo(world, id) { 0 } else { 2 } | (now < next) as u8;
    let on_board = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::ON_BOARD != 0);
    if !on_board {
        behavior_update(world, id, now, f);
    }
}

/// 0x59a720: a vehicle hall (building type 0x17) on a node of the first 721 hex points (radius
/// 15) around the catapult.
fn hall_near(world: &World, id: u16) -> bool {
    let p = pos_of(world, id);
    crate::jobs::warrior::hex().iter().take(721).any(|&(dx, dy)| {
        world.grid.checked_index(p.x() + dx, p.y() + dy).is_some_and(|n| {
            let m = world.layers.map_object[n];
            m != 0 && world.entities.by_index(m as u32).is_some_and(|e| e.obj_type == ObjType::Building && e.kind == VEHICLE_HALL)
        })
    })
}

/// 0x560c90: with +0xb8 and room below `MaxAmmo`, the first pile of the ammunition good with
/// goods within the first 91 hex points (0x561240, not for the Vikings) gives what is not
/// leaving, at most what fits (whole goods), `DecreaseUnforeseen`.
fn load_from_piles(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (race, k, p) = (e.race, e.kind, e.pos);
    let info = world.rules.vehicles.get(race, k as u8);
    let cargo = get(world, id).map_or(0, |v| v.cargo) as i32;
    if !data(world, id).is_some_and(|c| c.may_load) || cargo >= info.max_ammo || race == 1 {
        return;
    }
    let pile = crate::jobs::warrior::hex().iter().take(91).find_map(|&(dx, dy)| {
        let n = world.grid.checked_index(p.x() + dx, p.y() + dy)?;
        let id = world.layers.deco_object[n];
        let d = crate::pile::get(world, id)?;
        (d.good as i32 == info.x50 && d.amount > 0).then_some(id)
    });
    let Some(pile) = pile else { return };
    let Some(d) = crate::pile::get(world, pile) else { return };
    let per = per_good(world, id);
    let avail = d.amount as i32 - d.leaving as i32;
    let room = info.max_ammo - cargo;
    let n = if per * avail > room && per * avail != room { room / per } else { avail };
    crate::pile::decrease_unforeseen(world, pile, n);
    if let Some(v) = get_mut(world, id) {
        v.cargo = v.cargo.wrapping_add((per * n) as u8);
    }
}

// ---- the behaviour (vehicle variant of CWarriorBehavior) ----

/// `CWarriorBehavior::SetMode(vehicle, target, mode)` (0x598290) for a catapult: row 8 (its
/// warrior type), the target position only for modes 0x100000..0x700000, the owner and mode
/// flags; 0x700000 attacks the entity `target` (state 5); else the state by mode (0x100000 → 1,
/// 0x200000 / 0x300000 → 4, 0x500000 → 6, 0x600000 → 7, 0) and the walker heads for the
/// position (flags `mode & 0x1060 | 0x2000`).
pub fn set_mode(world: &mut World, id: u16, target: i32, mode: u32) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let owner = e.owner;
    let r = (e.flags & 0xF) as u8;
    let m = mode & 0x70_0000;
    let target = if m == 0 { -1 } else { target };
    let Some(b) = bh(world, id) else { return };
    b.row = r;
    b.pos = target;
    b.mode = owner as u32 | (mode & !0xF);
    if m == 0x70_0000 {
        b.target = target as u16;
        b.state = 5;
        b.next_search = 0;
        b.pos = 0;
        return;
    }
    b.state = match m {
        0x50_0000 => 6,
        0x10_0000 => 1,
        0x20_0000 | 0x30_0000 => 4,
        0x60_0000 => 7,
        _ => 0,
    };
    b.bflags = 0;
    b.target = 0;
    b.next_search = 0;
    walker_to(world, id, target, (mode & 0x1060) | 0x2000);
}

fn walker_to(world: &mut World, id: u16, target: i32, flags: u32) {
    let Some(mut w) = get_mut(world, id).and_then(|v| v.walking.take()) else { return };
    w.init(world, target, flags);
    if let Some(v) = get_mut(world, id) {
        v.walking = Some(w);
    }
}

/// 0x598da0 `(pos)`: the walker heads for `pos` with the mode's flags.
fn walker_to_pos(world: &mut World, id: u16, pos: i32) {
    let mode = data(world, id).map_or(0, |c| c.behavior.mode);
    walker_to(world, id, pos, (mode & 0x1060) | 0x2000);
}

/// 0x598a70 `(vehicle, now, flags)`: flags bit 0 reloading, bit 1 no ammunition.
pub fn behavior_update(world: &mut World, id: u16, now: u32, f: u8) {
    let Some(b) = data(world, id).map(|c| c.behavior) else { return };
    let r = row(world, id);
    match b.state {
        0 | 1 | 7 => {}
        4 => {
            walk(world, id);
            return;
        }
        5 => {
            attack_one(world, id, now, f, r);
            return;
        }
        _ => {
            if let Some(b) = bh(world, id) {
                b.state = 0;
            }
            end(world, id);
            return;
        }
    }
    if f & 2 == 0 {
        let mut due = b.next_search <= now;
        let mut t = b.target;
        let (mut tdist, mut tprio) = (0u32, 0u32);
        if t != 0 {
            if world.entities.by_index(t as u32).is_none() || !enemy_target(world, id, t, r[5]) {
                t = 0;
                due = true;
            } else {
                let tp = pos_of(world, t);
                tdist = dist(pos_of(world, id), tp) as u32;
                let wt = world.entities.by_index(t as u32).map_or(0, |e| e.flags & 0xF) as usize;
                tprio = if r[8] < tdist { 0 } else { 10 } + r[10 + wt];
            }
        }
        if f & 1 != 0 {
            if t != 0 {
                idle_step(world, id);
                return;
            }
            base_step(world, id, b.state);
            return;
        }
        if due {
            let s = search(world, id, r);
            if let Some(b) = bh(world, id) {
                b.next_search = now + if s.target == 0 { 0x28 } else { 0x14 };
            }
            if s.target != 0 && s.prio > 10 {
                let pick = if t != 0 && s.prio <= tprio && tdist <= s.dist * 2 { t } else { s.target };
                if let Some(b) = bh(world, id) {
                    b.bflags |= 1;
                    b.target = pick;
                }
                shoot(world, id, pick);
                return;
            }
        } else if t != 0 && tprio > 10 {
            if let Some(b) = bh(world, id) {
                b.bflags |= 1;
            }
            shoot(world, id, t);
            return;
        }
    }
    if let Some(b) = bh(world, id) {
        b.target = 0;
    }
    base_step(world, id, b.state);
}

/// The step of states 0 (0x598960: idle), 1 (0x598970: walk) and 7 (0x5989c0: attack-move).
fn base_step(world: &mut World, id: u16, state: u8) {
    match state {
        0 => idle_step(world, id),
        1 => walk(world, id),
        _ => attack_move(world, id),
    }
}

/// State 5 (attack one target).
fn attack_one(world: &mut World, id: u16, now: u32, f: u8, r: &[u32; 25]) {
    let Some(b) = data(world, id).map(|c| c.behavior) else { return };
    let t = b.target;
    if t == 0 || world.entities.by_index(t as u32).is_none() {
        end(world, id);
        return;
    }
    let mut place = b.pos;
    if place == 0 {
        place = firing_place(world, id, t, r);
        if let Some(b) = bh(world, id) {
            b.pos = place;
        }
    }
    if place > 0 {
        if let Some(b) = bh(world, id) {
            b.bflags |= 1;
        }
        walker_to_pos(world, id, place);
    }
    if b.next_search <= now {
        let ready = world.entities.by_index(t as u32).is_some_and(|e| e.flags & flag::READY != 0);
        if !ready {
            end(world, id);
            return;
        }
        let p = firing_place(world, id, t, r);
        if p != 0 && p != place {
            place = p;
            if let Some(b) = bh(world, id) {
                b.bflags |= 1;
                b.pos = p;
            }
            walker_to_pos(world, id, p);
        }
    }
    if place > 0 && f & 2 == 0 {
        if !in_range(world, id, t, r) {
            walk(world, id);
            return;
        }
        if f & 1 != 0 {
            idle_step(world, id);
            return;
        }
        if let Some(b) = bh(world, id) {
            b.bflags |= 1;
        }
        shoot(world, id, t);
        return;
    }
    end(world, id);
}

/// 0x599e30 `(vehicle, target)`: the first node of the spiral up to row +0x20 around the target
/// where the vehicle can stand (0x597020), packed; 0 when none.
fn firing_place(world: &World, id: u16, target: u16, r: &[u32; 25]) -> i32 {
    let k = world.entities.by_index(id as u32).map_or(0, |e| e.kind);
    let tp = pos_of(world, target);
    let last = crate::spiral::last(r[8].min(255) as u8);
    (0..=last)
        .map(|i| (tp.x() + crate::spiral::dx(i), tp.y() + crate::spiral::dy(i)))
        .find(|&(x, y)| world.grid.checked_index(x, y).is_some() && super::can_place(world, x, y, k))
        .map_or(0, |(x, y)| Y16X16::pack(x, y).0 as i32)
}

/// 0x599db0: the target within row +0x1c..=+0x20 (+1 for a land vehicle target).
fn in_range(world: &World, id: u16, target: u16, r: &[u32; 25]) -> bool {
    let d = dist(pos_of(world, id), pos_of(world, target));
    let lv = world.entities.by_index(target as u32).is_some_and(|e| e.obj_type == ObjType::LandVehicle) as i32;
    d <= r[8] as i32 + lv && r[7] as i32 <= d
}

/// 0x5981f0: a hostile owner and a valid target for the row's mask (0x598180).
fn enemy_target(world: &World, id: u16, t: u16, mask: u32) -> bool {
    let me = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    world.entities.by_index(t as u32).is_some_and(|e| crate::warmap::hostile(world, me, e.owner)) && crate::jobs::warrior::valid_target(world, t, mask)
}

struct Found {
    target: u16,
    prio: u32,
    dist: u32,
}

/// 0x5999c0: over the war-map cells around (0x5b0310 with 0x1f), the vehicles and ships
/// (category 1) that are valid enemies: priority = row's per warrior type + 10 within row +0x20;
/// one replaces the best when its priority is higher or it is nearer (as the original's `||`).
/// When the best is below row +0x58 + 10, the nearest enemy building (category 2) with priority
/// row +0x58 (+10 within range) is taken if higher.
fn search(world: &World, id: u16, r: &[u32; 25]) -> Found {
    let p = pos_of(world, id);
    let range = r[8] as i32;
    let cells = crate::warmap::cells_within(world, p.x(), p.y(), 0x1f);
    let (mut best, mut best_v, mut best_d) = (0u16, 0u32, 0x4000i32);
    for &(cx, cy) in &cells {
        for e in world.warmap.cell(cx as usize, cy as usize, 1) {
            if !enemy_target(world, id, e, r[5]) {
                continue;
            }
            let d = dist(p, pos_of(world, e));
            let wt = world.entities.by_index(e as u32).map_or(0, |x| x.flags & 0xF) as usize;
            let v = if d <= range { 10 } else { 0 } + r[10 + wt];
            if (best_v as i32) < v as i32 || d < best_d {
                best = e;
                best_v = v;
                best_d = d;
            }
        }
    }
    if (best_v as i32) < r[22] as i32 + 10 {
        let (mut nb, mut nd) = (0u16, 0x4000i32);
        for &(cx, cy) in &cells {
            for e in world.warmap.cell(cx as usize, cy as usize, 2) {
                if !enemy_target(world, id, e, r[5]) {
                    continue;
                }
                let d = dist(p, pos_of(world, e));
                if d < nd {
                    nb = e;
                    nd = d;
                }
            }
        }
        if nb != 0 {
            let v = if nd <= range { 10 } else { 0 } + r[22];
            if (best_v as i32) < v as i32 {
                return Found { target: nb, prio: v, dist: nd as u32 };
            }
        }
    }
    Found { target: best, prio: best_v, dist: best_d as u32 }
}

/// 0x598970: re-aim the walker when asked (+9 bit 0), then one walk step; arrived or failed
/// ends the state (8).
fn walk(world: &mut World, id: u16) {
    if data(world, id).is_some_and(|c| c.behavior.bflags & 1 != 0) {
        let pos = data(world, id).map_or(0, |c| c.behavior.pos);
        if let Some(b) = bh(world, id) {
            b.bflags &= !1;
        }
        walker_to_pos(world, id, pos);
    }
    let s = walk_step(world, id);
    if s & 0x70 != 0
        && let Some(b) = bh(world, id)
    {
        b.state = 8;
    }
}

/// 0x5989c0: attack-move: without a READY target end; within 10 of it stand, else head for it
/// (0x598df0) and walk.
fn attack_move(world: &mut World, id: u16) {
    let t = data(world, id).map_or(0, |c| c.behavior.pos);
    let target = if t > 0 { t as u16 } else { 0 };
    let ok = t > 0 && world.entities.by_index(target as u32).is_some_and(|e| e.flags & flag::READY != 0);
    if !ok {
        end(world, id);
        return;
    }
    let tp = pos_of(world, target);
    if dist(pos_of(world, id), tp) < 10 {
        idle_step(world, id);
        return;
    }
    approach(world, id, tp);
    walk_step(world, id);
}

/// 0x598df0 `(pos)`: re-aim the walker unless it is more than 4 away and its goal (walker
/// vtbl+0x2c) is within 2 of `pos`.
fn approach(world: &mut World, id: u16, to: Y16X16) {
    let me = pos_of(world, id);
    let goal = get(world, id).and_then(|v| v.walking.as_deref()).map_or(-1, |w| w.walk_to);
    if goal >= 0 && dist(me, to) > 4 && dist(Y16X16(goal as u32), to) < 3 {
        return;
    }
    walker_to_pos(world, id, to.0 as i32);
}

/// Behaviour vtbl+4 (0x561360): one `Walk` step (0x5950d0 without the next-task check);
/// returns the step (+0x5c).
fn walk_step(world: &mut World, id: u16) -> u8 {
    let pos = pos_of(world, id);
    let Some(mut w) = get_mut(world, id).and_then(|v| v.walking.take()) else { return 0 };
    let s = crate::pathing::walking::walk(world, &mut w, pos);
    if let Some(v) = get_mut(world, id) {
        v.walking = Some(w);
        v.task = super::logic::task::GO;
    }
    super::logic::step(world, id, s, false);
    get(world, id).map_or(0, |v| v.b5c)
}

/// Behaviour vtbl+8 (0x561310): one `IdleWalk` step on the map.
fn idle_step(world: &mut World, id: u16) {
    let pos = pos_of(world, id);
    if world.grid.checked_index(pos.x(), pos.y()).is_none() {
        return;
    }
    let Some(mut w) = get_mut(world, id).and_then(|v| v.walking.take()) else { return };
    let s = crate::pathing::walking::idle_walk(world, &mut w, pos.0 as i32, 0);
    if let Some(v) = get_mut(world, id) {
        v.walking = Some(w);
    }
    super::logic::step(world, id, s, false);
}

/// Behaviour vtbl+0x10 (0x561400): a transporter bringing ammunition is pointed at the new
/// place (V4 ammunition orders), then the next task (0x5951e0).
fn end(world: &mut World, id: u16) {
    if data(world, id).is_some_and(|c| c.carrier > 0) {
        crate::vehicles::ammo::retarget(world, id);
    }
    super::logic::next_task(world, id);
}

/// Behaviour vtbl+0xc (0x561390; a war ship's 0x592780 with gfx job 1) `(target)`: the shot
/// (0x595010), then the shooting animation: job 0x23, frame 0, task 0x10, the next update after
/// the turn and the animation.
fn shoot(world: &mut World, id: u16, target: u16) {
    fire(world, id, target);
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let race = e.race;
    let job = if e.obj_type == ObjType::Ship { 1 } else { SHOOT_JOB };
    let frames = world.rules.vehicles.frame_count(race, job);
    let Some(v) = get_mut(world, id) else { return };
    v.job_part = job;
    v.frame = 0;
    v.half_frame = 0;
    v.frames = frames;
    v.task = TASK_ANIMATION;
    let delay = v.turn as i32 + v.frames as i32;
    world.register_for_logic_update(id, delay);
}

/// 0x595010 `(target)`: turn to the target (the direction from the six sectors of .rdata
/// 0x101c8fc), `CFutureEvents` 6 in `info +0x48 + turn` ticks with the target and its position,
/// the next shot at `now + ReadyToFireDelay`, one ammunition.
fn fire(world: &mut World, id: u16, target: u16) {
    const SECTOR: [i8; 8] = [0, 1, 2, 2, 5, 3, 4, 3];
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (race, k, p) = (e.race, e.kind, e.pos);
    let tp = pos_of(world, target);
    let (dx, dy) = (tp.x() - p.x(), tp.y() - p.y());
    let i = (dx <= dy * 2) as usize | if dx < -dy { 4 } else { 0 } | if dx * 2 <= dy { 2 } else { 0 };
    let turn = super::logic::turn_to(world, id, SECTOR[i]);
    let info = world.rules.vehicles.get(race, k as u8);
    crate::future::add(world, FUTURE_SHOT, (info.x48 + turn as i32) as u32, id, target as i32, tp.0 as i32);
    let now = world.tick as i32;
    if let Some(v) = get_mut(world, id) {
        v.next_tick = info.fire_delay + now;
    }
    use_ammo(world, id);
}

/// `CFutureEvents` handler 6 (0x4e53f0): a READY catapult's missile lands (0x594c80).
pub fn shot_lands(world: &mut World, id: u16, target: i32, at: i32) {
    let ready = world.entities.by_index(id as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle) && e.flags & flag::READY != 0);
    if !ready {
        return;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (race, k, p, owner) = (e.race, e.kind, e.pos, e.owner);
    let info = world.rules.vehicles.get(race, k as u8);
    let at = Y16X16(at as u32);
    // The effects' durations (CEffects 0x4e3a80 / 0x4e3b20 with delay 1); the missile is a
    // flying effect ([`crate::projectile::launch`]).
    let flight = if race == 1 && k == kind::WAR_MACHINE {
        if world.grid.checked_index(at.x(), at.x()).is_some() { effect_frames(world, VIKING_EFFECT) + 1 } else { 1 }
    } else {
        crate::projectile::launch(world, u8::try_from(info.x3c).unwrap_or(u8::MAX), p, at, 1, (0, 0))
    };
    if race == 1 && k == kind::WAR_MACHINE {
        world.events.push(crate::event::SimEvent::EffectLater { effect: VIKING_EFFECT as u8, sound: 0, x: at.x(), y: at.x(), delay: 1 });
    }
    if info.x40 != 0 {
        // Delay: the missile's (or the Viking effect's) end + 1, as the damage.
        world.events.push(crate::event::SimEvent::EffectLater { effect: info.x40 as u8, sound: info.x44 as u16, x: at.x(), y: at.y(), delay: (flight + 1) as u32 });
    }
    // The owner's strength on own (the team of the land's eco-sector owner) or foreign land.
    let land = world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.economy.owner(world.tiling.eco_sector_of(n)));
    let strength = crate::statistic::strength(world, owner, land);
    let d = (strength * info.damage + 0x7f) >> 8;
    let target = target as u16;
    if world.entities.by_index(target as u32).is_some_and(|e| e.flags & flag::READY != 0) {
        crate::future::add(world, 2, (flight + 1) as u32, target, d.max(1), owner as i32);
    }
}

/// `CEffects::AddEffect` duration: the effect's frame count (gfx 6, direction 0; at least 1).
fn effect_frames(world: &World, effect: usize) -> i32 {
    (world.rules.vehicles.effect_frames.get(effect).copied().unwrap_or(0) as i32).max(1)
}

/// vtbl+0x20 (`CVehicle` 0x5956d0) `(damage, attacker)`: above the armour the excess halved
/// (at least 1), else 1; the hit points down, at 0 killed by `attacker`; a dead vehicle's effect
/// 0x3f (sound 0x5b) is the client's.
pub fn take_damage(world: &mut World, id: u16, n: i32, attacker: u8) {
    if n <= 0 {
        return;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (pos, armor) = (e.pos, world.rules.vehicles.get(e.race, e.kind as u8).armor);
    let mut d = 1;
    if armor < n {
        let x = n - armor;
        if x > 1 {
            d = x / 2;
        }
    }
    let Some(v) = get_mut(world, id) else { return };
    if d < v.health as i32 {
        v.health -= d as u8;
        return;
    }
    v.health = 0;
    world.kill(id, attacker);
    world.events.push(crate::event::SimEvent::Effect { effect: 0x3f, sound: 0x5b, x: pos.x(), y: pos.y() });
}

/// vtbl+0x6c (0x560f80) before `CWheeler` 0x59a200: a move order to a place with an enemy within
/// spiral radius 2 (0x5614b0) attacks it instead (no move); otherwise an ammunition carrier is
/// sent away (a donkey follows, 0x576eb0; a carrier gets event 9) and the move goes on.
/// Returns whether the event was consumed.
pub fn convert_event(world: &mut World, id: u16, ev: &EntityEvent) -> bool {
    if ev.event != 0x11 || ev.kind != 0xd {
        return false;
    }
    let to = Y16X16(ev.data[1] as u32);
    if attack_at(world, id, to) {
        return true;
    }
    let c = data(world, id).map_or(0, |c| c.carrier);
    if c > 0 {
        crate::vehicles::ammo::move_ordered(world, id, c as u16, to);
    }
    false
}

/// 0x5614b0 `(x, y)`: over the spiral up to `last(2)`, the first map object of a hostile player
/// (0x12dcd54) that is a valid target (0x598180 with the row's mask) is attacked
/// (`SetMode(entity, 0x700000)`).
pub(super) fn attack_at(world: &mut World, id: u16, to: Y16X16) -> bool {
    let r = row(world, id);
    for i in 0..=crate::spiral::last(2) {
        let (x, y) = (to.x() + crate::spiral::dx(i), to.y() + crate::spiral::dy(i));
        let Some(n) = world.grid.checked_index(x, y) else { continue };
        let m = world.layers.map_object[n];
        if m != 0 && m > 6 && enemy_target(world, id, m, r[5]) {
            set_mode(world, id, m as i32, 0x70_0000);
            return true;
        }
    }
    false
}

/// vtbl+0x10 (0x560b50) before `CVehicle` 0x594a10: the vehicle it is going to (+0x98, a ferry)
/// drops it (vtbl+0x7c).
pub fn on_delete(world: &mut World, id: u16) {
    let ferry = get(world, id).map_or(0, |v| v.target_entity);
    if ferry > 0 && world.entities.by_index(ferry as u32).is_some() {
        super::remove_passenger(world, ferry as u16, id);
    }
}
