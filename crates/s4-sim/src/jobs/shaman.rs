//! `CShamanRole` (role 16, vtable 0x105c14c; behaviour vtable 0x105c1d4): the Dark Tribe's
//! shaman, a `CSoldierRole` (search row 18, [`super::warrior`]) with its own mana (+0x68). Own
//! reverse engineering of the shipped exe.
//!
//! * The constructor (0x5876b0) sets the mana to `DTShamanManaPool`; `Init` is the soldier's.
//! * `ConvertEventIntoGoal` (0x587ac0): event 3 (from the AI) orders a cast at `data0` — the
//!   shaman walks there (0x589810, flag 4); event 0x1c ties it to a manakopter.
//! * `LogicUpdateJob` (0x587820, [`logic_update_job`]): with mana and a cast order, a pile of
//!   boards or iron there is teleported (spell 7), else a field within 2 rots (spell 2), both
//!   deferred and free (`CastSpell` flags `DEFER | FREE`) but paid from the shaman's mana
//!   (`DTManaCostTeleport` / `DTManaCostRot`). On dark land the mana is refilled. Then the
//!   behaviour update (0x598610) — without attack searches while the shaman has no mana or a
//!   cast order.
//! * Its attack (behaviour vtbl+0xc, 0x587b60, [`attack`]) costs `DTManaCostEnslave`: the fight
//!   animation, and 5 ticks later (future event 8, 0x4e5460, [`enslave`]) the enemy civilian it
//!   hit becomes a slave of the shaman's player.

use super::core::{self, job, job_mut, register};
use super::state::{EntityEvent, SoldierData};
use super::{soldier, warrior};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::grid::Y16X16;

/// The Dark Tribe's slave (`0x37`).
pub const SLAVE: u16 = 0x37;
/// The Dark Tribe's spells the shaman casts: 2 rot (0x4f4c70), 7 teleport (0x4f4d10).
const SPELL_ROT: u8 = 2;
const SPELL_TELEPORT: u8 = 7;
/// `CFutureEvents` type of an enslaving hit (handler 0x4e5460) and its delay.
pub const FUTURE_ENSLAVE: u8 = 8;
const ENSLAVE_DELAY: u32 = 5;

fn sd(world: &mut World, i: u16) -> Option<&mut SoldierData> {
    job_mut(world, i).map(|j| &mut j.soldier)
}

pub fn is_shaman(world: &World, i: u16) -> bool {
    job(world, i).is_some_and(|j| j.role == super::state::role::SHAMAN)
}

/// The ctor 0x5876b0 (+0x60 / +0x64 / +0x6c = 0, mana `DTShamanManaPool`) and the soldier's
/// `Init` (0x5890f0).
pub fn init_role(world: &mut World, i: u16) {
    soldier::init_role(world, i);
    let pool = world.rules.magic.dark.shaman_mana_pool;
    if let Some(s) = sd(world, i) {
        s.shaman_ride = 0;
        s.shaman_ride_uid = 0;
        s.shaman_mana = pool;
        s.shaman_cast_at = 0;
    }
}

/// `ConvertEventIntoGoal` (0x587ac0).
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match ev.event {
        3 => {
            if let Some(s) = sd(world, i) {
                s.shaman_cast_at = ev.data[0];
            }
            soldier::walk_to(world, i, Y16X16(ev.data[0] as u32), 4);
            register(world, i, 1);
            return;
        }
        0x1c => {
            let ride = ev.data[0] as u16;
            let uid = world.entities.by_index(ride as u32).filter(|e| e.flags & flag::DIED == 0).map(|e| e.uid);
            if let Some(s) = sd(world, i) {
                s.shaman_ride = if uid.is_some() { ride } else { 0 };
                s.shaman_ride_uid = uid.unwrap_or(0);
                s.shaman_cast_at = 0;
            }
        }
        _ => {}
    }
    soldier::convert_event_into_goal(world, i, ev);
}

/// 0x5879e0: 0 while the shaman's manakopter (an entity of type 0x40 with the kept unique id)
/// lives. Otherwise the link and the mana go (−1) and the AI is asked for another manakopter
/// (`CAIShaman` vtbl+0x18; a pure virtual slot in the release's static `CAIShaman` object, so
/// inferred: none) — without one, or on another sector, the shaman is killed (2).
fn check_ride(world: &mut World, i: u16) -> i32 {
    let Some(s) = job(world, i).map(|j| j.soldier) else { return 2 };
    let ok = world.entities.by_index(s.shaman_ride as u32).is_some_and(|e| e.obj_type == ObjType::Flying && e.uid == s.shaman_ride_uid);
    if ok {
        return 0;
    }
    if let Some(s) = sd(world, i) {
        s.shaman_ride = 0;
        s.shaman_ride_uid = 0;
        s.shaman_mana = -1;
    }
    // TODO(port): the manakopters (CAIShaman vtbl+0x18 → a sector check of its +0x48).
    world.kill(i, 0);
    2
}

/// `CShamanRole::LogicUpdateJob` (0x587820).
pub fn logic_update_job(world: &mut World, i: u16) {
    let Some(s) = job(world, i).map(|j| j.soldier) else { return };
    if s.fight_task.is_some() {
        if let Some(s) = sd(world, i) {
            s.fight_task = None;
        }
        soldier::take_job(world, i);
        if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::REGISTERED == 0) {
            register(world, i, 1);
        }
        return;
    }
    let mut busy = true;
    if s.shaman_mana < 1 {
        if s.shaman_ride != 0 && check_ride(world, i) == 2 {
            return;
        }
    } else if s.shaman_cast_at == 0 {
        busy = false;
    } else {
        let at = Y16X16(s.shaman_cast_at as u32);
        let (mut x, mut y) = (at.x(), at.y());
        let spell = if teleportable(world, x, y) {
            Some(SPELL_TELEPORT)
        } else if let Some((px, py)) = field_near(world, x, y) {
            (x, y) = (px, py);
            Some(SPELL_ROT)
        } else {
            None
        };
        match spell {
            None => busy = false,
            Some(spell) => {
                let player = world.entities.by_index(i as u32).map_or(0, |e| e.owner);
                let flags = crate::magic::cast_flag::DEFER | crate::magic::cast_flag::FREE;
                if crate::magic::cast_spell(world, player, spell, x, y, flags, i) > 0 {
                    let d = &world.rules.magic.dark;
                    let cost = if spell == SPELL_ROT { d.mana_cost_rot } else { d.mana_cost_teleport };
                    if let Some(s) = sd(world, i) {
                        s.shaman_mana -= cost;
                    }
                    soldier::behaviour_done(world, i);
                    if let Some(s) = sd(world, i) {
                        s.shaman_cast_at = 0;
                    }
                }
            }
        }
    }
    // Dark land (node flag 0x04) refills the mana.
    let dark = world.entities.by_index(i as u32).and_then(|e| world.grid.checked_index(e.x(), e.y())).is_some_and(|n| world.layers.flags[n] & 4 != 0);
    if dark {
        let pool = world.rules.magic.dark.shaman_mana_pool;
        if let Some(s) = sd(world, i) {
            s.shaman_mana = pool;
        }
    }
    if world.entities.by_index(i as u32).is_some_and(|e| e.flags & flag::ON_BOARD == 0) {
        warrior::update_flags(world, i, if busy { warrior::NO_SEARCH } else { 0 });
    }
}

/// A pile of boards (7) or iron (20) with goods not yet reserved on the node.
fn teleportable(world: &World, x: i32, y: i32) -> bool {
    let Some(n) = world.grid.checked_index(x, y) else { return false };
    let id = world.layers.deco_object[n];
    match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Pile(p)) => matches!(p.good, 7 | 20) && p.amount > p.leaving,
        _ => false,
    }
}

/// `CAIShaman` vtbl+0xc (0x4444d0) with radius 2 ([`crate::ai::shaman::field_near`]).
fn field_near(world: &World, x: i32, y: i32) -> Option<(i32, i32)> {
    crate::ai::shaman::field_near(world, x, y, 2)
}

/// Behaviour vtbl+0xc (0x587b60) `Attack(target)`: the mana pays `DTManaCostEnslave`, the fight
/// task (the first of anim list 1 with the target) is taken; then — unless the shaman's
/// manakopter is gone ([`check_ride`]) — a living target gets the enslaving future event 8 in 5
/// ticks (`(manakopter << 16 | target, its unique id)`) and effect 0x4c (sound 0x54).
pub fn attack(world: &mut World, i: u16, target: u16) {
    let cost = world.rules.magic.dark.mana_cost_enslave;
    if let Some(s) = sd(world, i) {
        s.shaman_mana -= cost;
    }
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (race, kind) = (e.race, e.kind);
    let Some(list) = world.rules.jobs.anim_list(race, kind, 1) else { return };
    let Some(mut t) = world.rules.jobs.list(race, list).and_then(|l| l.first().copied()) else { return };
    t.entity = target;
    if let Some(s) = sd(world, i) {
        s.fight_task = Some(t);
    }
    soldier::take_job(world, i);
    let uid = world.entities.by_index(target as u32).map_or(0, |t| t.uid);
    let ride = job(world, i).map_or(0, |j| j.soldier.shaman_ride);
    if ride != 0 && check_ride(world, i) != 0 {
        return;
    }
    let Some(t) = world.entities.by_index(target as u32).filter(|t| t.flags & flag::DIED == 0) else { return };
    let pos = t.pos;
    crate::future::add(world, FUTURE_ENSLAVE, ENSLAVE_DELAY, i, ((ride as u32) << 16 | target as u32) as i32, uid as i32);
    world.events.push(crate::event::SimEvent::Effect { effect: 0x4c, sound: 0x54, x: pos.x(), y: pos.y() });
}

/// Future event 8 (0x4e5460, `entity` = the shaman, `a` = manakopter << 16 | target, `b` = the
/// target's unique id): the target settler, still the same one, visible and not on board and on
/// its node's map-object layer, is hidden and killed; a slave of the shaman's player takes its
/// place (`AddSettler(x, y, player, 0x37, 2)`). With a manakopter (type 0x40, alive; otherwise
/// nothing happens) the slave is handed to it (vtbl+0x70, [`crate::flying::attach`]) and gets
/// event 0x1c.
pub fn enslave(world: &mut World, shaman: u16, a: i32, b: i32) {
    let (target, ride) = ((a as u32 & 0xffff) as u16, (a as u32 >> 16) as u16);
    let Some(t) = world.entities.by_index(target as u32).filter(|t| t.obj_type == ObjType::Settler && t.uid == b as u32) else { return };
    let (pos, flags) = (t.pos, t.flags);
    let on_node = flags & (flag::ON_BOARD | flag::VISIBLE) == flag::VISIBLE && world.grid.checked_index(pos.x(), pos.y()).is_some_and(|n| world.layers.map_object[n] == target);
    if ride != 0 && !world.entities.by_index(ride as u32).is_some_and(|r| r.obj_type == ObjType::Flying && r.flags & flag::DIED == 0) {
        return;
    }
    if !on_node {
        return;
    }
    if let Some(e) = world.entities.by_index_mut(target as u32) {
        e.flags &= !flag::VISIBLE;
    }
    world.kill(target, 0);
    let player = world.entities.by_index(shaman as u32).map_or(0, |e| e.owner);
    let slave = crate::create::add_settler_ex(world, pos.x(), pos.y(), player, SLAVE, 2);
    if ride != 0
        && let Some(s) = slave
    {
        crate::flying::attach(world, ride, s);
        core::set_event(world, s, EntityEvent { event: 0x1c, kind: 0, data: [ride as i32, 0, 0] });
    }
}
