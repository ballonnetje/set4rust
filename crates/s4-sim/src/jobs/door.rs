//! `CDoorRole` (role 21, release vtable 0x105af38): the invisible door settler (type 69) a
//! military building keeps on its door node (`CSettlerMgr` 0x582810, see
//! `create::add_door_settler`). Enemies fight it to get in; while it lives it regains health.
//! It has no walking object (vtbl+0x14 0x577690 returns none) and ignores events and jobs
//! (`TakeJob`, `LogicUpdateJob`, `GetNextJob`, `ConvertEventIntoGoal` are empty).

use super::core::{job, job_mut, register};
use crate::World;
use crate::entity::EntityData;

/// `CDoorRole::Init` (0x5777f0): +0x12 = own id, war map (out first if in), health =
/// `Logic/MaxTowerDoorHealth` (0x11cdb5c, default 50).
pub fn init_role(world: &mut World, i: u16) {
    let max = world.rules.logic.max_tower_door_health;
    if let Some(j) = job_mut(world, i) {
        j.w18 = i;
    }
    crate::warmap::readd(world, i);
    if let Some(EntityData::Settler(s)) = world.entities.by_index_mut(i as u32).map(|e| &mut e.data) {
        s.health = max;
    }
}

/// `CDoorRole::LogicUpdate` (0x577860): below the settler info health, +1 (up to
/// `MaxTowerDoorHealth`) and again in 15.
pub fn logic_update(world: &mut World, i: u16) {
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let EntityData::Settler(s) = &e.data else { return };
    let info = world.rules.settler_health(e.race, e.kind);
    let max = world.rules.logic.max_tower_door_health;
    if s.health < info {
        if s.health < max
            && let Some(EntityData::Settler(s)) = world.entities.by_index_mut(i as u32).map(|e| &mut e.data)
        {
            s.health += 1;
        }
        register(world, i, 15);
    }
}

/// `CDoorRole::SetFree` (0x577900): the home building's `SettlerLeft` (role +0x50). Returns
/// false.
pub fn set_free(world: &mut World, i: u16, _eco_sector: i32) -> bool {
    let home = job(world, i).map_or(0, |j| j.home);
    if home != 0 {
        crate::buildings::settler_left(world, home, i);
    }
    false
}
