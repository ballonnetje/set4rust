//! `CGatherBuildingRole` (MapObjects/Building/GatherBuildingRole.cpp, house kind 6): the
//! buildings whose worker fetches a resource from the working area: woodcutter, stonecutter,
//! hunter, fisher, … The building orders its inhabitant, then loops:
//!
//! * state 3: when the worker is home, the output pile has room (< 8) and the building works
//!   (flag 0x1000), search the working area. Found: mark the target node (0x20, FLAG_WORK),
//!   hide the worker and send it event 5 (work on entity `+12` at `+16`) or 6 (a position only).
//!   The building then waits for the worker to check in again.
//! * `SettlerEnter`: the worker is back: state 2.
//! * state 2: wait `productionDelay` updates of 14 ticks, then state 3.

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works (cleared while it is stopped).
const FLAG_WORKING: u32 = 0x1000;
/// Hunters search animals instead (`CScanner::FindNearestAnimal`).
const HUNTER: u16 = 7;

/// `CGatherBuildingRole` (store version 1: the output pile, +380).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    /// +380: the output pile.
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
        Role { base, kind: RoleKind::Gather(s) } => Some((base, s)),
        _ => None,
    }
}

fn building_type(world: &World, id: u16) -> (u8, u8) {
    world.entities.by_index(id as u32).map_or((0, 0), |e| match &e.data {
        EntityData::Building(d) => (e.race, d.type_ex),
        _ => (e.race, 0),
    })
}


/// `CGatherBuildingRole::Init` (release 0x546ad0), after `IBuildingRole::InitCommon`: state 1 (order
/// the inhabitant), the output pile from buildingInfo's first pile (`AddPile` +
/// `SetBuildingId`), first update in 2 ticks.
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y) = (e.x(), e.y());
    let (race, type_ex) = building_type(world, id);
    let info = world.rules.work.info(race, type_ex).cloned().unwrap_or_default();
    // "m_pBuildingInfo->iNumberOfPiles == 1" asserted.
    let pile = info
        .piles
        .first()
        .map_or(0, |p| super::add_building_pile(world, id, x + p.x as i32, y + p.y as i32, p.good, p.kind));
    if let Some((base, s)) = role(world, id) {
        base.inhabitant = 0;
        base.worker_in_house = false;
        *s = State { work: WorkBase { state: 1, ..WorkBase::default() }, pile };
    }
    // +32 = the inhabitant's first search ("m_pSearchFkt != 0" asserted).
    let routine = work::inhabitant_search(world, id, 0);
    if let Some((_, s)) = role(world, id) {
        s.work.routine = routine;
    }
    world.register_for_logic_update(id, 2);
}

/// `CGatherBuildingRole::LogicUpdate` (release 0x546c50).
pub fn logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let Some((_, s)) = role(world, id) else { return };
    match s.work.state {
        1 => {
            if work::order_inhabitant(world, id) {
                if let Some((_, s)) = role(world, id) {
                    s.work.state = 3;
                }
                return;
            }
            // 0x546e7f: no inhabitant yet, look again in 31 ticks (also when there is no
            // eco-sector, where `OrderInhabitant` itself does not register).
            world.register_for_logic_update(id, 31);
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
        3 => search_and_send(world, id),
        _ => {}
    }
}

fn search_and_send(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (working, kind) = (e.flags & FLAG_WORKING != 0, e.kind);
    let Some((base, s)) = role(world, id) else { return };
    s.work.target_entity = 0;
    s.work.target = 0;
    let (home, pile) = (base.worker_in_house, s.pile);
    if home && working && crate::pile::get(world, pile).is_some_and(|p| p.amount < 8) {
        if kind == HUNTER {
            // Release 0x546ce2: `CScanner::FindNearestAnimal` around the working area, within the
            // working radius, on the eco-sector of the ensign (0x537080); the animal there must
            // still be alive (flags 0x3000000).
            let found = hunt_target(world, id);
            if let (Some((animal, pos)), Some((_, s))) = (found, role(world, id)) {
                s.work.target_entity = animal;
                s.work.target = pos.0;
            }
        } else {
            let mut w = role(world, id).map(|(_, s)| std::mem::take(&mut s.work)).unwrap_or_default();
            work::search_in_working_area(world, id, &mut w, 0);
            if let Some((_, s)) = role(world, id) {
                s.work = w;
            }
        }
    }
    let Some((base, s)) = role(world, id) else { return };
    let (target, entity, settler) = (s.work.target, s.work.target_entity, base.inhabitant);
    if target == 0 {
        world.register_for_logic_update(id, 31);
        return;
    }
    if let Some(w) = world.entities.by_index_mut(settler as u32) {
        w.flags &= !flag::VISIBLE;
    }
    let pos = crate::grid::Y16X16(target);
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
        world.layers.flags[n] |= 0x20;
    }
    let ev = if entity != 0 {
        EntityEvent { event: 5, kind: 0, data: [0, entity as i32, target as i32] }
    } else {
        EntityEvent { event: 6, kind: 0, data: [0, 0, target as i32] }
    };
    crate::jobs::core::set_event(world, settler, ev);
}

fn hunt_target(world: &World, id: u16) -> Option<(u16, crate::grid::Y16X16)> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    let (race, type_ex) = building_type(world, id);
    let radius = world.rules.work.info(race, type_ex).map_or(0, |i| i.working_radius);
    let eco = world.grid.checked_index(d.ensign.x(), d.ensign.y()).map_or(0, |n| world.tiling.eco_sector_of(n));
    let pos = crate::animals::find_nearest_huntable(world, d.working_area.x(), d.working_area.y(), radius, eco)?;
    let n = world.grid.checked_index(pos.x(), pos.y())?;
    let animal = world.layers.map_object[n];
    let a = world.entities.by_index(animal as u32)?;
    (a.obj_type == crate::entity::ObjType::Animal && a.flags & flag::ALIVE_MASK != 0).then_some((animal, pos))
}

/// `CGatherBuildingRole::SettlerEnter` (release 0x546be0): the worker is home (and hidden). With a
/// production delay the building waits (state 2), otherwise it looks again in 31 ticks.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let delay = work::production_delay(world, id);
    if let Some((base, _)) = role(world, id) {
        base.worker_in_house = true;
        base.inhabitant = settler;
    }
    if let Some(w) = world.entities.by_index_mut(settler as u32) {
        w.flags &= !flag::VISIBLE;
    }
    if delay != 0 {
        if let Some((_, s)) = role(world, id) {
            s.work.delay = 0;
            s.work.state = 2;
        }
        world.register_for_logic_update(id, 14);
    } else {
        // "has no production delay" is traced.
        world.register_for_logic_update(id, 31);
    }
    true
}

/// `CGatherBuildingRole::GoodArrive` (release 0x53b080): nothing.
pub fn good_arrive(_world: &mut World, _id: u16, _good: u8) {}

/// `CGatherBuildingRole::ConvertEventIntoGoal` (release 0x53b0e0): nothing.
pub fn convert_event_into_goal(_world: &mut World, _id: u16, _ev: &EntityEvent) {}

/// `CGatherBuildingRole::GetPileIdWithGood(good)` (release 0x53b070): the output pile, whatever the good.
pub fn pile_with_good(world: &World, id: u16, _good: u8) -> u16 {
    match world.buildings.roles.get(&id) {
        Some(Role { kind: RoleKind::Gather(s), .. }) => s.pile,
        _ => 0,
    }
}

/// `IBuildingRole::GetPileIdWithNeedForGood` (release 0x534400, not overridden): vtbl+56,
/// `GetPileIdWithGood`.
pub fn pile_with_need_for_good(world: &World, id: u16, good: u8) -> u16 {
    pile_with_good(world, id, good)
}
