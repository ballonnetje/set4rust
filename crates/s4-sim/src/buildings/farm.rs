//! `CFarmBuildingRole` (house kind 8), analysed from the shipped exe (vtable 0x1058cf8): farms
//! whose worker harvests and sows in the working area (grain farm, agave farm, …). The worker is
//! a `CFreeWorkerRole` with two searches: the first finds something ripe, the second a place to
//! sow.
//!
//! `LogicUpdate` (0x5464a0), states as in `gather`: 1 order the inhabitant, 2 production delay
//! (`productionDelay` updates of 14 ticks), 3 when the worker is home and the farm works (flag
//! 0x1000):
//! * the pile has room (< 8) and the first search finds a ripe plant: event 5 (harvest it);
//! * otherwise the second search finds a free place: the plant for the ground there
//!   (`CDecoObjMgr::GetObjectType` with the worker's `other`), its rings reserved
//!   (`SetFlagsForObject`), event 6 (sow it);
//! * either way the worker is made visible and the pile locked (vtbl+72, pile flag 0x10) until
//!   the worker checks in again; with nothing to do, look again in 31 ticks.

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works.
const FLAG_WORKING: u32 = 0x1000;
/// Pile entity flag 0x10, set by `LockPiles(true)` (release 0x546960).
const PILE_LOCKED: u32 = 0x10;

/// `CFarmBuildingRole` (+0x17c: the output pile).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    /// +0x17c: the output pile.
    pub pile: u16,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.work.hash(h);
        h.u16(self.pile);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::Farm(s) } => Some((base, s)),
        _ => None,
    }
}

fn building_info(world: &World, id: u16) -> Option<work::WorkInfo> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    world.rules.work.info(e.race, d.type_ex).cloned()
}

/// `CFarmBuildingRole::Init` (release 0x5462e0), after `IBuildingRole::InitCommon`: state 1, the
/// output pile from buildingInfo's first pile record (`AddPile` + attach; a patch pile hidden),
/// the search routine left at the second search, first update in 2 ticks.
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y) = (e.x(), e.y());
    let info = building_info(world, id).unwrap_or_default();
    let mut pile = 0;
    if let Some(p) = info.piles.first().copied()
        && let Some(q) = crate::pile::add_pile_ex(world, x + p.x as i32, y + p.y as i32, p.good, 0, p.kind, p.appearance, p.patch, p.pixel)
    {
        {
            super::attach(world, id, q);
            if let Some(r) = super::role_mut(world, id) {
                r.base.piles.push(q);
            }
            if p.patch == 1
                && let Some(e) = world.entities.by_index_mut(q as u32)
            {
                e.flags &= !flag::VISIBLE;
            }
            pile = q;
        }
    }
    // +0x20 is set to the first search's routine and then overwritten with the second's.
    let routine = work::inhabitant_search(world, id, 1);
    if let Some((base, s)) = role(world, id) {
        base.inhabitant = 0;
        base.worker_in_house = false;
        *s = State { work: WorkBase { state: 1, routine, ..WorkBase::default() }, pile };
    }
    world.register_for_logic_update(id, 2);
}

/// `CFarmBuildingRole::LockPiles(lock)` (release 0x546960, vtbl+72).
fn lock_piles(world: &mut World, id: u16, lock: bool) {
    let pile = role(world, id).map_or(0, |(_, s)| s.pile);
    if let Some(e) = world.entities.by_index_mut(pile as u32) {
        if lock {
            e.flags |= PILE_LOCKED;
        } else {
            e.flags &= !PILE_LOCKED;
        }
    }
}

/// `CFarmBuildingRole::LogicUpdate` (release 0x5464a0).
pub fn logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let Some((_, s)) = role(world, id) else { return };
    match s.work.state {
        1 => {
            if work::order_inhabitant(world, id)
                && let Some((_, s)) = role(world, id)
            {
                s.work.state = 3;
            } else {
                world.register_for_logic_update(id, 31);
            }
        }
        2 => {
            if s.work.delay > delay {
                s.work.delay = 0;
                s.work.state = 3;
                world.register_for_logic_update(id, 1);
            } else {
                s.work.delay += 1;
                world.register_for_logic_update(id, 14);
            }
        }
        3 => send_out(world, id),
        _ => {}
    }
}

fn search(world: &mut World, id: u16, i: usize) -> bool {
    let routine = work::inhabitant_search(world, id, i);
    let mut w = role(world, id).map(|(_, s)| std::mem::take(&mut s.work)).unwrap_or_default();
    w.routine = routine;
    let found = work::search_in_working_area(world, id, &mut w, i);
    if let Some((_, s)) = role(world, id) {
        s.work = w;
    }
    found
}

fn send(world: &mut World, id: u16, settler: u16, ev: EntityEvent) {
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags |= flag::VISIBLE;
    }
    crate::jobs::core::set_event(world, settler, ev);
    lock_piles(world, id, true);
}

fn send_out(world: &mut World, id: u16) {
    let working = world.entities.by_index(id as u32).is_some_and(|e| e.flags & FLAG_WORKING != 0);
    let Some((base, s)) = role(world, id) else { return };
    let (home, settler, pile) = (base.worker_in_house, base.inhabitant, s.pile);
    if !home || !working {
        world.register_for_logic_update(id, 31);
        return;
    }
    // vtbl+40 of the pile: its amount.
    let room = crate::pile::get(world, pile).is_some_and(|p| p.amount < 8);
    if room && search(world, id, 0) {
        let (entity, target) = role(world, id).map_or((0, 0), |(_, s)| (s.work.target_entity, s.work.target));
        send(world, id, settler, EntityEvent { event: 5, kind: 0, data: [0, entity as i32, target as i32] });
        return;
    }
    if !search(world, id, 1) {
        world.register_for_logic_update(id, 31);
        return;
    }
    let target = role(world, id).map_or(0, |(_, s)| s.work.target);
    let Some(w) = world.entities.by_index(settler as u32) else { return };
    let (race, kind) = (w.race, w.kind);
    let t = world.rules.jobs.search(race, kind, 1).unwrap_or(0);
    let (dx, dy) = super::search::offset(t);
    let pos = Y16X16(target);
    let (x, y) = (pos.x() - dx, pos.y() - dy);
    let ground = world.grid.checked_index(x, y).map_or(0, |n| world.layers.ground[n]);
    let other = world.rules.jobs.other(race, kind);
    let object = crate::deco::get_object_type(world, other, race, ground);
    // (The exe goes on even when no object type fits.)
    crate::deco::set_flags_for_object(world, x, y, object);
    send(world, id, settler, EntityEvent { event: 6, kind: 0, data: [0, object as i32, target as i32] });
}

/// `CFarmBuildingRole::SettlerEnter` (release 0x546430): the first entry makes the worker the
/// inhabitant; it is hidden, displacement 10; then the production delay (state 2) and the pile
/// unlocked.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let delay = work::production_delay(world, id);
    if let Some((base, s)) = role(world, id) {
        if !base.worker_in_house {
            base.worker_in_house = true;
            base.inhabitant = settler;
        }
        if delay > 0 {
            s.work.delay = 0;
        }
        s.work.state = 2;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags &= !flag::VISIBLE;
    }
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.displacement_costs = 10;
    }
    world.register_for_logic_update(id, if delay > 0 { 14 } else { 31 });
    lock_piles(world, id, false);
    true
}

/// `CFarmBuildingRole::GoodArrive` (release 0x53b460): nothing.
pub fn good_arrive(_world: &mut World, _id: u16, _good: u8) {}

/// `CFarmBuildingRole::ConvertEventIntoGoal` (release 0x53b4d0): nothing.
pub fn convert_event_into_goal(_world: &mut World, _id: u16, _ev: &EntityEvent) {}

/// `CFarmBuildingRole::GetPileIdWithGood` (release 0x53b450): the output pile, whatever the good.
pub fn pile_with_good(world: &World, id: u16, _good: u8) -> u16 {
    match world.buildings.roles.get(&id) {
        Some(Role { kind: RoleKind::Farm(s), .. }) => s.pile,
        _ => 0,
    }
}

/// `IBuildingRole::GetPileIdWithNeedForGood` (not overridden): vtbl+56, `GetPileIdWithGood`.
pub fn pile_with_need_for_good(world: &World, id: u16, good: u8) -> u16 {
    pile_with_good(world, id, good)
}
