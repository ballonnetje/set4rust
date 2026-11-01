//! `CFutureEvents` (release vtable 0x102f0c4): events due at a later tick (own reverse
//! engineering of the shipped exe).
//!
//! `AddEvent(type, delay, entity, a, b)` (0x4e4b10) takes a node from the free list and puts it
//! at the head of the bucket `(now + delay) & 31`, keeping the due tick and the entity's unique
//! id. Each tick (0x4e4c00, `FutureEvents::Execute` in the tick) walks the current bucket head
//! first: a node whose entity no longer exists (unique id changed) is dropped; a due one runs
//! its handler (table .rdata 0x1000810, types 0..9) and is dropped; one due in a later round
//! (delays of 32 and more) stays. Handlers ported: 1 (0x4e5280, entity flags `|= a, &= !b`),
//! 2 (0x4e52c0, damage `a` from player `b` to the entity while it is READY or BIRTH), 3 (0x4e5300,
//! a deferred spell: `magic::cast_deferred`), 4 (0x4e5320, a terrain spell's ring:
//! `magic::spells::ring`). The others
//! are reported as unported.

use crate::entity::flag;
use crate::event::SimEvent;
use crate::hash::StateHasher;
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FutureEvent {
    /// +0x10 type.
    pub kind: u8,
    /// +0x12 entity (0 = none) and +0x14 its unique id when added.
    pub entity: u16,
    pub uid: u32,
    /// +0x18 due tick, +0x1c / +0x20 data.
    pub due: u32,
    pub a: i32,
    pub b: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FutureEvents {
    /// 32 buckets, head first.
    pub buckets: Vec<Vec<FutureEvent>>,
}

impl Default for FutureEvents {
    fn default() -> Self {
        FutureEvents { buckets: vec![Vec::new(); 32] }
    }
}

impl FutureEvents {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for b in &self.buckets {
            h.u32(b.len() as u32);
            for e in b {
                h.u8(e.kind);
                h.u16(e.entity);
                h.u32(e.uid);
                h.u32(e.due);
                h.u32(e.a as u32);
                h.u32(e.b as u32);
            }
        }
    }
}

/// 0x4e4b10 `AddEvent(type, delay, entity, a, b)`. TODO(port): the node pool limit (0x3fdf).
pub fn add(world: &mut World, kind: u8, delay: u32, entity: u16, a: i32, b: i32) {
    if kind == 0 {
        return;
    }
    let due = world.tick.wrapping_add(delay);
    let uid = if entity == 0 { 0 } else { world.entities.by_index(entity as u32).map_or(0, |e| e.uid) };
    let ev = FutureEvent { kind, entity, uid, due, a, b };
    world.future.buckets[(due & 31) as usize].insert(0, ev);
}

/// 0x4e4c00: the current tick's bucket.
pub fn execute(world: &mut World) {
    let now = world.tick;
    let slot = (now & 31) as usize;
    let mut i = 0;
    while i < world.future.buckets[slot].len() {
        let ev = world.future.buckets[slot][i];
        let alive = ev.entity == 0 || world.entities.by_index(ev.entity as u32).is_some_and(|e| e.uid == ev.uid);
        if alive && ev.due > now {
            i += 1;
            continue;
        }
        world.future.buckets[slot].remove(i);
        if alive {
            run(world, &ev);
        }
    }
}

fn run(world: &mut World, ev: &FutureEvent) {
    match ev.kind {
        2 => {
            // 0x4e52c0: damage while the target is READY or BIRTH.
            let ok = ev.a > 0 && world.entities.by_index(ev.entity as u32).is_some_and(|e| e.flags & (flag::READY | flag::BIRTH) != 0);
            if ok {
                crate::combat::take_damage(world, ev.entity, ev.a, ev.b as u8);
            }
        }
        1 => {
            // 0x4e5280: flags of an entity that is READY, BIRTH or 0x1000: `|= a`, then `&= !b`
            // (a blowgun's freeze ends: b = 0x800000).
            if let Some(e) = world.entities.by_index_mut(ev.entity as u32)
                && ev.entity != 0
                && e.flags & 0x300_1000 != 0
            {
                e.flags = (e.flags | ev.a as u32) & !(ev.b as u32);
            }
        }
        // 0x4e5300: a deferred spell (`magic::cast_spell` with DEFER).
        3 => crate::magic::cast_deferred(world, ev.entity, ev.a, ev.b),
        // 0x4e5320: a ring of the terrain spell.
        4 => crate::magic::spells::ring(world, ev.a, ev.b),
        // 0x4e5460: a shaman's enslaving hit.
        8 => crate::jobs::shaman::enslave(world, ev.entity, ev.a, ev.b),
        // 0x4e55d0: a freed slave becomes a carrier of player `a`.
        9 => crate::jobs::slave::freed(world, ev.entity, ev.a),
        // 0x4e53f0: a catapult's shot lands (`vehicles::catapult::shot_lands`).
        6 => crate::vehicles::catapult::shot_lands(world, ev.entity, ev.a, ev.b),
        _ => world.events.push(SimEvent::Unported { what: "CFutureEvents handler", entity: ev.entity }),
    }
}
