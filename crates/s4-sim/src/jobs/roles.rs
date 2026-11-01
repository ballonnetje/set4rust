//! Role virtual dispatch (`ISettlerRole` vtable, offsets from the release vtables at
//! 0x105aa60 / `CHouseWorkerRole` 0x105b8b0): +16 `Go`, +24 `LogicUpdate`, +28 `LogicUpdateJob`,
//! +32 `UpdateJob`, +36 `GetNextJob`, +40 `TakeJob`, +44 `Init`, +48 `SetBuilding`, +52
//! `SetEntity`, +56 `ConvertEventIntoGoal`, +64 `SetFree`, +124 `CheckHome`.

use super::core::{self, job};
use super::state::{EntityEvent, role};
use super::{builder, carrier, dark_gardener, digger, donkey, door, flee, free_worker, gardener, geologist, house_worker, hunter, mushroom_farmer, pioneer, priest, shaman, slave, soldier, thief};
use crate::World;

fn role_of(world: &World, i: u16) -> u8 {
    job(world, i).map_or(role::NONE, |j| j.role)
}

/// vtbl+24.
pub fn logic_update(world: &mut World, i: u16) {
    match role_of(world, i) {
        role::CARRIER => carrier::logic_update(world, i),
        role::TOWER_SOLDIER => soldier::tower_logic_update(world, i),
        role::DOOR => door::logic_update(world, i),
        role::SABOTEUR => soldier::saboteur_logic_update(world, i),
        role::FLEE => flee::logic_update(world, i),
        role::THIEF => thief::logic_update(world, i),
        _ => core::role_logic_update(world, i),
    }
}

/// vtbl+40.
pub fn take_job(world: &mut World, i: u16) {
    match role_of(world, i) {
        role::HOUSE_WORKER => house_worker::take_job(world, i),
        role::CARRIER => carrier::take_job(world, i),
        role::DIGGER => digger::take_job(world, i),
        role::BUILDER => builder::take_job(world, i),
        role::FREE_WORKER => free_worker::take_job(world, i),
        role::HUNTER => hunter::take_job(world, i),
        role::DONKEY => donkey::take_job(world, i),
        role::DARK_GARDENER => dark_gardener::take_job(world, i),
        role::GARDENER => gardener::take_job(world, i),
        role::MUSHROOM_FARMER => mushroom_farmer::take_job(world, i),
        role::FLEE => flee::take_job(world, i),
        role::GEOLOGIST => geologist::take_job(world, i),
        role::PIONEER => pioneer::take_job(world, i),
        role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR | role::SHAMAN => soldier::take_job(world, i),
        role::TOWER_SOLDIER => soldier::tower_take_job(world, i),
        role::PRIEST => priest::take_job(world, i),
        role::SLAVE => slave::take_job(world, i),
        role::THIEF => thief::take_job(world, i),
        _ => {}
    }
}

/// vtbl+28.
pub fn logic_update_job(world: &mut World, i: u16) {
    match role_of(world, i) {
        role::HOUSE_WORKER => house_worker::logic_update_job(world, i),
        role::CARRIER => carrier::logic_update_job(world, i),
        role::DIGGER => digger::logic_update_job(world, i),
        role::BUILDER => builder::logic_update_job(world, i),
        role::FREE_WORKER => free_worker::logic_update_job(world, i),
        role::HUNTER => hunter::logic_update_job(world, i),
        role::DONKEY => donkey::logic_update_job(world, i),
        role::DARK_GARDENER => dark_gardener::logic_update_job(world, i),
        role::GARDENER => gardener::logic_update_job(world, i),
        role::MUSHROOM_FARMER => mushroom_farmer::logic_update_job(world, i),
        role::GEOLOGIST => geologist::logic_update_job(world, i),
        role::PIONEER => pioneer::logic_update_job(world, i),
        role::SOLDIER | role::SABOTEUR => soldier::logic_update_job(world, i),
        role::SQUAD_LEADER => soldier::leader_logic_update_job(world, i),
        role::SHAMAN => shaman::logic_update_job(world, i),
        role::SLAVE => slave::logic_update_job(world, i),
        role::PRIEST => priest::logic_update_job(world, i),
        role::THIEF => thief::logic_update_job(world, i),
        _ => {}
    }
}

/// vtbl+32 (the base `ISettlerRole::UpdateJob` is empty).
pub fn update_job(world: &mut World, i: u16) {
    match role_of(world, i) {
        role::HOUSE_WORKER => house_worker::update_job(world, i),
        role::CARRIER => carrier::update_job(world, i),
        role::FREE_WORKER => free_worker::update_job(world, i),
        role::HUNTER => hunter::update_job(world, i),
        role::MUSHROOM_FARMER => mushroom_farmer::update_job(world, i),
        role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR | role::SHAMAN => soldier::update_job(world, i),
        role::SLAVE => slave::update_job(world, i),
        role::THIEF => thief::update_job(world, i),
        role::DONKEY => donkey::update_job(world, i),
        _ => {}
    }
}

/// vtbl+36.
pub fn get_next_job(world: &mut World, i: u16) {
    match role_of(world, i) {
        role::HOUSE_WORKER => house_worker::get_next_job(world, i),
        role::CARRIER => carrier::get_next_job(world, i),
        role::DIGGER => digger::get_next_job(world, i),
        role::BUILDER => builder::get_next_job(world, i),
        role::FREE_WORKER => free_worker::get_next_job(world, i),
        role::HUNTER => hunter::get_next_job(world, i),
        role::DONKEY => donkey::get_next_job(world, i),
        role::DARK_GARDENER => dark_gardener::get_next_job(world, i),
        role::GARDENER => gardener::get_next_job(world, i),
        role::MUSHROOM_FARMER => mushroom_farmer::get_next_job(world, i),
        // 0x577ec0: straight to TakeJob.
        role::FLEE => flee::take_job(world, i),
        role::GEOLOGIST => geologist::get_next_job(world, i),
        role::PIONEER => pioneer::get_next_job(world, i),
        role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR | role::SHAMAN => soldier::get_next_job(world, i),
        role::SLAVE => slave::get_next_job(world, i),
        role::TOWER_SOLDIER => soldier::tower_get_next_job(world, i),
        role::PRIEST => priest::get_next_job(world, i),
        role::THIEF => thief::get_next_job(world, i),
        _ => {}
    }
}

/// vtbl+64 `SetFree(settler, eco_sector)`.
pub fn set_free(world: &mut World, i: u16, eco_sector: i32) -> bool {
    crate::debug::log(world, i, "set free", eco_sector, 0);
    match role_of(world, i) {
        role::CARRIER => carrier::set_free(world, i, eco_sector),
        role::DIGGER => digger::set_free(world, i, eco_sector),
        role::BUILDER => builder::set_free(world, i, eco_sector),
        role::FREE_WORKER => free_worker::set_free(world, i, eco_sector),
        role::HUNTER => hunter::set_free(world, i, eco_sector),
        role::GARDENER => gardener::set_free(world, i, eco_sector),
        role::MUSHROOM_FARMER => mushroom_farmer::set_free(world, i, eco_sector),
        // 0x577ed0: nothing.
        role::FLEE => false,
        role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR | role::SHAMAN => soldier::set_free(world, i, eco_sector),
        role::SLAVE => slave::set_free(world, i, eco_sector),
        role::TOWER_SOLDIER => soldier::tower_set_free(world, i, eco_sector),
        role::DOOR => door::set_free(world, i, eco_sector),
        role::THIEF => thief::set_free(world, i, eco_sector),
        _ => core::set_free_base(world, i, eco_sector),
    }
}

/// vtbl+56 `ConvertEventIntoGoal(settler, event)`.
pub fn convert_event_into_goal(world: &mut World, i: u16, ev: &EntityEvent) {
    match role_of(world, i) {
        role::HOUSE_WORKER => house_worker::convert_event_into_goal(world, i, ev),
        role::CARRIER => carrier::convert_event_into_goal(world, i, ev),
        role::DIGGER => digger::convert_event_into_goal(world, i, ev),
        role::BUILDER => builder::convert_event_into_goal(world, i, ev),
        role::FREE_WORKER => free_worker::convert_event_into_goal(world, i, ev),
        role::HUNTER => hunter::convert_event_into_goal(world, i, ev),
        role::DONKEY => donkey::convert_event_into_goal(world, i, ev),
        role::DARK_GARDENER => dark_gardener::convert_event_into_goal(world, i, ev),
        role::GARDENER => gardener::convert_event_into_goal(world, i, ev),
        role::MUSHROOM_FARMER => mushroom_farmer::convert_event_into_goal(world, i, ev),
        role::FLEE => flee::convert_event_into_goal(world, i),
        role::GEOLOGIST => geologist::convert_event_into_goal(world, i, ev),
        role::PIONEER => pioneer::convert_event_into_goal(world, i, ev),
        role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR => soldier::convert_event_into_goal(world, i, ev),
        role::SHAMAN => shaman::convert_event_into_goal(world, i, ev),
        role::SLAVE => slave::convert_event_into_goal(world, i, ev),
        role::TOWER_SOLDIER => soldier::tower_convert_event(world, i, ev),
        role::PRIEST => priest::convert_event_into_goal(world, i, ev),
        role::THIEF => thief::convert_event_into_goal(world, i, ev),
        // CDoorRole 0x5776b0: empty.
        _ => {}
    }
}

/// A role object made by `CSettlerMgr::CreateSettlerRole(race, type)` (release 0x583310), before it
/// is given to a settler: the constructors' random draws happen here (the carrier's idle
/// threshold), which in `CSettlerMgr::AddSettler` is before the settler's own direction draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NewRole {
    pub role: u8,
    idle_after: u16,
}

/// `CSettlerMgr::CreateSettlerRole` for role id `role` (`SettlerInfo::m_uRole`).
pub fn create_settler_role(world: &mut World, role: u8) -> NewRole {
    let idle_after = if role == role::CARRIER { carrier::idle_threshold(world) } else { 0 };
    NewRole { role, idle_after }
}

/// `CSettler::NewRole(role)` / the `CSettler` constructor's role setup: set the role and its
/// home, then `InitWalking` (vtbl+12) and `Init` (vtbl+44).
pub fn set_role(world: &mut World, i: u16, r: NewRole, home: u16) {
    if let Some(j) = core::job_mut(world, i) {
        j.role = r.role;
        j.home = home;
    }
    match r.role {
        role::HOUSE_WORKER => house_worker::init_role(world, i),
        role::CARRIER => carrier::init_role(world, i, r.idle_after),
        role::DIGGER => digger::init_role(world, i),
        role::BUILDER => builder::init_role(world, i),
        role::FREE_WORKER => free_worker::init_role(world, i),
        role::HUNTER => hunter::init_role(world, i),
        role::DONKEY => donkey::init_role(world, i),
        role::DARK_GARDENER => dark_gardener::init_role(world, i),
        role::GARDENER => gardener::init_role(world, i),
        role::MUSHROOM_FARMER => mushroom_farmer::init_role(world, i),
        role::FLEE => flee::init_role(world, i),
        role::GEOLOGIST => geologist::init_role(world, i),
        role::PIONEER => pioneer::init_role(world, i),
        role::SOLDIER => soldier::init_role(world, i),
        role::SQUAD_LEADER => soldier::leader_init_role(world, i),
        role::SABOTEUR => soldier::saboteur_init_role(world, i),
        role::SHAMAN => shaman::init_role(world, i),
        role::SLAVE => slave::init_role(world, i),
        role::DOOR => door::init_role(world, i),
        role::PRIEST => priest::init_role(world, i),
        role::THIEF => thief::init_role(world, i),
        _ => {}
    }
}

/// The role part of the `CSettler` constructor (release 0x580330), after the settler is on the map:
/// displacement costs 0, the role's `InitWalking` / `Init`, then `TakeWaitList`.
/// Roles not ported yet stay `role::NONE` (no updates), as before roles were created here.
pub fn settler_constructed(world: &mut World, i: u16, r: NewRole) {
    if let Some(j) = core::job_mut(world, i) {
        j.displacement_costs = 0;
    }
    if !is_ported(r.role) {
        return;
    }
    set_role(world, i, r, 0);
    core::take_wait_list(world, i);
}

/// The roles this module implements.
pub fn is_ported(r: u8) -> bool {
    matches!(r, role::CARRIER | role::DIGGER | role::BUILDER | role::HOUSE_WORKER | role::FREE_WORKER | role::SOLDIER | role::SQUAD_LEADER | role::SABOTEUR | role::HUNTER | role::DONKEY | role::DARK_GARDENER | role::GARDENER | role::MUSHROOM_FARMER | role::FLEE | role::GEOLOGIST | role::PIONEER | role::PRIEST | role::SHAMAN | role::SLAVE | role::THIEF)
}

/// `CreateSettlerRole` + `CSettler::NewRole` for the ported roles (tests and `ChangeType`):
/// the role, its walking object and `Init`. The home building is `ISettlerRole::SetBuilding`.
/// Callers (building / eco-sector code) then give the settler a list.
pub fn new_role(world: &mut World, i: u16, new_role: u8, home: u16) {
    crate::debug::log(world, i, "role", new_role as i32, home as i32);
    let r = create_settler_role(world, new_role);
    set_role(world, i, r, home);
}
