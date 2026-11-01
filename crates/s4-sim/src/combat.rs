//! Damage to settlers (own reverse engineering of the shipped exe, release VAs).
//!
//! `IEntity` vtbl+0x1c (0x567020) `TakeDamage(n, attacker)` keeps the attacker in a global
//! (0x129b0b8) for the kill statistics and calls vtbl+0x20: for a settler 0x581780 — the role's
//! +0x44 turns `n` into hit points (`ISettlerRole` 0x572d00: unchanged; `CSoldierRole` 0x580020:
//! a Dark Tribe soldier takes one less, the squad leader its armor (settler info +4) less, at
//! least 1, the shield factor (entity flag 0x200000, `ShieldDmgDecrease256` 0x11abda4);
//! `CDoorRole` 0x5778c0 wakes the home building; `CTowerSoldier` 0x58cc50 notes the tick;
//! `CDonkeyRole` 0x576360 reacts and takes nothing); at or
//! above its health the settler dies (`Kill(id, attacker)`), else loses them. Buildings:
//! `buildings::crush::damage` (vtbl+0x20 0x5371f0).

use crate::entity::{EntityData, ObjType};
use crate::jobs::role;
use crate::world::World;

/// vtbl+0x1c `TakeDamage(n, attacker)` (0x567020).
pub fn take_damage(world: &mut World, id: u16, n: i32, attacker: u8) {
    match world.entities.by_index(id as u32).map(|e| e.obj_type) {
        Some(ObjType::Settler) => settler_damage(world, id, n, attacker),
        Some(ObjType::Building) => crate::buildings::crush::damage(world, id, n),
        Some(ObjType::Ship | ObjType::LandVehicle) => crate::vehicles::take_damage(world, id, n, attacker),
        Some(ObjType::Flying) => crate::flying::damage(world, id, n, attacker),
        Some(_) => world.events.push(crate::event::SimEvent::Unported { what: "TakeDamage (animal)", entity: id }),
        None => {}
    }
}

/// `CSettler` vtbl+0x20 (0x581780).
fn settler_damage(world: &mut World, id: u16, n: i32, attacker: u8) {
    let d = role_hit(world, id, n);
    if d <= 0 {
        return;
    }
    let Some(EntityData::Settler(s)) = world.entities.by_index_mut(id as u32).map(|e| &mut e.data) else { return };
    if d >= s.health as i32 {
        s.health = 0;
        world.kill(id, attacker);
    } else {
        s.health -= d as u8;
    }
}

/// Role vtbl+0x44: hit points from damage.
pub fn role_hit(world: &mut World, id: u16, n: i32) -> i32 {
    let Some(e) = world.entities.by_index(id as u32) else { return n };
    let EntityData::Settler(s) = &e.data else { return n };
    match s.job.role {
        role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR | role::SHAMAN | role::PRIEST | role::GARDENER => {
            // 0x580020 (vtbl+0x44 of the soldier, saboteur, priest, gardener, …). A shielded
            // settler (entity flag 0x200000, the Romans' spell 4) takes
            // `d · ShieldDmgDecrease256 >> 8`.
            if n < 1 {
                return 0;
            }
            if e.is_dead() {
                return 0;
            }
            let mut d = n;
            if e.flags & crate::magic::spells::FLAG_SHIELD != 0 {
                d = d.wrapping_mul(world.rules.magic.shield_dmg_decrease256) >> 8;
            }
            if e.race == 3 {
                return (d - 1).max(1);
            }
            if e.kind == 0x2c {
                d -= world.rules.settler_armor(e.race, 0x2c) as i32;
            }
            d.max(1)
        }
        role::DOOR => {
            // 0x5778c0: the home building is woken for its next update.
            if n > 0 {
                let home = s.job.home;
                if world.entities.by_index(home as u32).is_some_and(|b| b.flags & crate::entity::flag::REGISTERED == 0) {
                    world.register_for_logic_update(home, 15);
                }
            }
            n
        }
        // 0x576360: a loaded donkey reacts; it takes no damage.
        role::DONKEY => crate::jobs::donkey::hit(world, id),
        role::TOWER_SOLDIER => {
            // 0x58cc50: the tick of the last hit.
            let tick = world.tick;
            if let Some(EntityData::Settler(s)) = world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
                s.job.tower.tick = tick;
            }
            n
        }
        _ => n,
    }
}
