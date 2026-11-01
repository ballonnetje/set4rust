//! `CSimpleBuildingRole` (MapObjects/Building/SimpleBuilding.cpp, house kind 1): buildings
//! whose worker leaves the house to do something in the working area that yields no good for a
//! pile: the forester (building type 2) plants trees, the healer (type 27) heals soldiers.
//! The states are those of `gather`: 1 order the inhabitant, 2 production delay, 3 send out.
//!
//! Forester, state 3: search the working area with the second search (`SEARCH_TREE_SEED_POS`),
//! pick the tree for the ground there (`CDecoObjMgr::GetObjectType`), reserve its rings
//! (`SetFlagsForObject`), put the hidden worker on the door node and send it event 6 (plant
//! object `data1` at `data2`).

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works.
const FLAG_WORKING: u32 = 0x1000;
const FORESTERHUT: u16 = 2;
const HEALERHUT: u16 = 27;

/// `CSimpleBuildingRole` (store version 1: +380).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    /// +380: the healer's pause after a heal order (10 updates).
    pub pause: u8,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.work.hash(h);
        h.u8(self.pause);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::Simple(s) } => Some((base, s)),
        _ => None,
    }
}

/// `CSimpleBuildingRole::Init` (0x54edd0), after `IBuildingRole::InitCommon`: state 1; the
/// search routine is the inhabitant's second search (none for the healer).
pub fn init(world: &mut World, id: u16) {
    let kind = world.entities.by_index(id as u32).map_or(0, |e| e.kind);
    let routine = if kind == HEALERHUT { 0 } else { work::inhabitant_search(world, id, 1) };
    if let Some((base, s)) = role(world, id) {
        base.inhabitant = 0;
        base.worker_in_house = false;
        *s = State { work: WorkBase { state: 1, routine, ..WorkBase::default() }, pause: 0 };
    }
    world.register_for_logic_update(id, 2);
}

/// `CSimpleBuildingRole::LogicUpdate` (0x54ef60).
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
            // 0x54f21f: no inhabitant yet, look again in 31 ticks (also when there is no
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
        3 => send_out(world, id),
        _ => {}
    }
}

fn send_out(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (working, kind) = (e.flags & FLAG_WORKING != 0, e.kind);
    let EntityData::Building(d) = &e.data else { return };
    let door = d.door;
    let home = role(world, id).is_some_and(|(b, _)| b.worker_in_house);
    if !home || !working {
        world.register_for_logic_update(id, 31);
        return;
    }
    match kind {
        FORESTERHUT => {
            let mut w = role(world, id).map(|(_, s)| std::mem::take(&mut s.work)).unwrap_or_default();
            let found = work::search_in_working_area(world, id, &mut w, 1);
            let target = w.target;
            if let Some((_, s)) = role(world, id) {
                s.work = w;
            }
            if !found {
                world.register_for_logic_update(id, 31);
                return;
            }
            plant_order(world, id, Y16X16(target), door);
        }
        HEALERHUT => heal(world, id),
        _ => world.register_for_logic_update(id, 31),
    }
}

/// Role tasks of a soldier the healer may call: its behaviour task (0x1b) or WAIT (0x11).
const CALLABLE_TASKS: [u8; 2] = [0x1b, 0x11];

/// A hurt soldier of the healer's team at `n`: a settler (`0x4e1ee0`: map object of class 1)
/// with a warrior type, health below its type's maximum (`SettlerInfo` health, vtbl+0x28), whose
/// owner is in the healer owner's team (0x54f2c7: `CAlliances` 0x12dcd0c equal).
fn hurt_soldier(world: &World, n: usize, owner: u8) -> Option<u16> {
    let s = world.layers.map_object[n];
    let e = world.entities.by_index(s as u32)?;
    let EntityData::Settler(d) = &e.data else { return None };
    (crate::warmap::team(world, e.owner) == crate::warmap::team(world, owner) && e.warrior_type() != 0 && d.health < world.rules.settler_health(e.race, e.kind)).then_some(s)
}

/// The healer's state 3 (`LogicUpdate` 0x54ef60, building type 27). The patients' place is the
/// ensign (+0x48), 4 nodes further left for race 4.
/// * A hurt soldier stands there (0x54f720): the healer gets event 3 (the first work list, on
///   that soldier) and the pause (+0x17c) is 0.
/// * Else, without a pause: the next 30 spiral entries around the working area (+0x4c), from the
///   cursor (+0x14), up to `Last(workingAreaRadius)` and then from 0 again, are searched for a
///   hurt soldier on its behaviour task or waiting; the first one is called to the place (event
///   0x11, kind 0xd, data1 = the place). Found: a pause of 10 updates, next in 14 ticks.
/// * With a pause: one less.
///
/// Otherwise the next update is in 31 ticks.
fn heal(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (race, owner) = (e.race, e.owner);
    let EntityData::Building(d) = &e.data else { return };
    let (ensign, area, type_ex) = (d.ensign, d.working_area, d.type_ex);
    let place = Y16X16::pack(ensign.x() - if race == 4 { 4 } else { 0 }, ensign.y());
    let healer = role(world, id).map_or(0, |(b, _)| b.inhabitant);
    if let Some(patient) = world.grid.checked_index(place.x(), place.y()).and_then(|n| hurt_soldier(world, n, owner)) {
        if let Some((_, s)) = role(world, id) {
            s.pause = 0;
        }
        let list = world.rules.work.info(race, type_ex).and_then(|i| i.anim_lists.first().copied()).unwrap_or(0);
        crate::jobs::core::set_event(world, healer, EntityEvent { event: 3, kind: 0, data: [0, list as i32, patient as i32] });
        return;
    }
    let Some((_, s)) = role(world, id) else { return };
    if s.pause != 0 {
        s.pause -= 1;
        world.register_for_logic_update(id, 31);
        return;
    }
    let cursor = s.work.cursors[0][0] as usize;
    let mut found = false;
    let mut steps = 30;
    for i in 0..30 {
        let k = cursor + i;
        let Some(n) = world.grid.checked_index(area.x() + crate::spiral::dx(k), area.y() + crate::spiral::dy(k)) else { continue };
        let Some(soldier) = hurt_soldier(world, n, owner) else { continue };
        if !crate::jobs::core::job(world, soldier).is_some_and(|j| CALLABLE_TASKS.contains(&j.task)) {
            continue;
        }
        crate::jobs::core::set_event(world, soldier, EntityEvent { event: 0x11, kind: 0xd, data: [0, place.0 as i32, 0] });
        found = true;
        steps = i + 1;
        break;
    }
    let radius = world.rules.work.info(race, type_ex).map_or(0, |i| i.working_radius);
    let last = crate::spiral::last(radius.clamp(0, crate::spiral::MAX_RADIUS as i32) as u8);
    if let Some((_, s)) = role(world, id) {
        let c = (cursor + steps) as u16;
        s.work.cursors[0][0] = if last as u16 <= c { 0 } else { c };
        if found {
            s.pause = 10;
        }
    }
    world.register_for_logic_update(id, if found { 14 } else { 31 });
}

/// The forester's order: the tree type for the ground at the planting node, its flags, and
/// event 6 to the worker, who is put on the door node first.
fn plant_order(world: &mut World, id: u16, target: Y16X16, door: Y16X16) {
    let settler = role(world, id).map_or(0, |(b, _)| b.inhabitant);
    let Some(s) = world.entities.by_index(settler as u32) else { return };
    let (race, kind) = (s.race, s.kind);
    let t = world.rules.jobs.search(race, kind, 1).unwrap_or(0);
    let (dx, dy) = super::search::offset(t);
    let (x, y) = (target.x() - dx, target.y() - dy);
    let ground = world.grid.checked_index(x, y).map_or(0, |n| world.layers.ground[n]);
    let other = world.rules.jobs.other(race, kind);
    let object = crate::deco::get_object_type(world, other, race, ground);
    if object == 0 {
        world.register_for_logic_update(id, 14);
        return;
    }
    crate::deco::set_flags_for_object(world, x, y, object);
    // IEntity::SetPosition(door) + CWorldManager::SetSettlerId(door, settler).
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.pos = door;
        e.flags &= !(flag::ON_BOARD | flag::VISIBLE);
    }
    if let Some(n) = world.grid.checked_index(door.x(), door.y()) {
        world.layers.map_object[n] = settler;
    }
    let ev = EntityEvent { event: 6, kind: 0, data: [0, object as i32, target.0 as i32] };
    crate::jobs::core::set_event(world, settler, ev);
}

/// `CSimpleBuildingRole::SettlerEnter` (0x54ee50, vtbl+76): the worker is home and hidden; then
/// state 2, the production delay (its counter starts again only when the building has a delay;
/// vtbl+0x48 after it is empty). The healer gets MAGIC_INVISIBLE and its offset on the door's
/// cell: from now on the building draws it (`FillGfxInfo` 0x54f420 → 0x581490, sub-sprite 0).
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let delay = work::production_delay(world, id);
    if let Some((base, s)) = role(world, id) {
        if !base.worker_in_house {
            base.worker_in_house = true;
            base.inhabitant = settler;
        }
        if delay != 0 {
            s.work.delay = 0;
        }
        s.work.state = 2;
    }
    let Some(b) = world.entities.by_index(id as u32) else { return true };
    if b.kind == HEALERHUT
        && let EntityData::Building(d) = &b.data
    {
        let (dx, dy) = (d.door.x() - b.x(), d.door.y() - b.y());
        let one = crate::jobs::state::OFFSET_ONE;
        if let Some(j) = crate::jobs::core::job_mut(world, settler) {
            j.offset = ((24 * dx - 12 * dy) * one, 12 * dy * one);
        }
        if let Some(w) = world.entities.by_index_mut(settler as u32) {
            w.flags |= flag::MAGIC_INVISIBLE;
        }
    }
    if let Some(w) = world.entities.by_index_mut(settler as u32) {
        w.flags &= !flag::VISIBLE;
    }
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.displacement_costs = 10;
    }
    // "has no production delay" is traced for 0.
    world.register_for_logic_update(id, if delay != 0 { 14 } else { 31 });
    true
}

/// `CSimpleBuildingRole::GoodArrive`: nothing.
pub fn good_arrive(_world: &mut World, _id: u16, _good: u8) {}

/// The base `ConvertEventIntoGoal` (not overridden).
pub fn convert_event_into_goal(_world: &mut World, _id: u16, _ev: &EntityEvent) {}

/// `CSimpleBuildingRole::GetPileIdWithGood`: 0.
pub fn pile_with_good(_world: &World, _id: u16, _good: u8) -> u16 {
    0
}

/// `IBuildingRole::GetPileIdWithNeedForGood` (release 0x534400, not overridden): vtbl+56,
/// `GetPileIdWithGood`.
pub fn pile_with_need_for_good(world: &World, id: u16, good: u8) -> u16 {
    pile_with_good(world, id, good)
}
