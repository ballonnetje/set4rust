//! `CWorkUpBuildingRole` (MapObjects/Building/WorkUpBuilding.cpp, house kind 2): buildings that
//! turn delivered goods into a product inside the house: sawmill, smelter, mill, … Its worker is
//! a house worker (`jobs::house_worker`), ordered with event 3 to run one of the building's work
//! lists (`vAnimListId`): fetch the material from the delivery piles (GET_GOOD_VIRTUAL), work,
//! put the product on the production pile (PUT_GOOD_VIRTUAL), check in again.
//!
//! States as in `gather`: 1 order the inhabitant, 2 production delay, 3 order one work cycle when
//! the worker is home, the building works, the production pile has room (< 8) and every
//! delivery pile has a good.
//!
//! `CAnimalRanchRole` (house kind 3, vtable 0x105837c) is the same role with its own `Init`
//! (0x533f50) and `LogicUpdate` (0x533ff0) and an animal count (+0x18c); every other slot is the
//! workup role's. See [`ranch_logic_update`].

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works.
const FLAG_WORKING: u32 = 0x1000;
/// Pile entity flag 0x10, set by `LockPiles(true)` (0x55a8d0).
const PILE_LOCKED: u32 = 0x10;
/// The settler type whose building alternates its work lists (every 6th cycle list 0, else
/// list 1; `IEntity::Type(inhabitant) == 21`).
const ALTERNATING_WORKER: u16 = 21;

/// `CWorkUpBuildingRole` (store version 2: +380, +382, the delivery piles).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    /// +380: cycle counter of the alternating worker.
    pub cycle: u8,
    /// +382: the production pile.
    pub product: u16,
    /// +384: the delivery piles, in buildingInfo order.
    pub delivery: Vec<u16>,
    /// `CAnimalRanchRole` +0x18c: the animals in the pen (1 after `Init`; 0 for other buildings).
    /// The ranch's gfx sequence (+0x4c) is the building's first sequence + animals + 1.
    pub animals: u8,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.work.hash(h);
        h.u8(self.cycle);
        h.u16(self.product);
        h.u16s(&self.delivery);
        h.u8(self.animals);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::WorkUp(s) } => Some((base, s)),
        _ => None,
    }
}

fn state(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::WorkUp(s), .. } => Some(s),
        _ => None,
    }
}

/// The workup (or animal / donkey ranch) role of building `id`, for the building dialog (D19:
/// the delivery piles, the production pile and, for a ranch, the animals).
pub fn get(world: &World, id: u16) -> Option<&State> {
    state(world, id)
}

fn info(world: &World, id: u16) -> Option<&work::WorkInfo> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    world.rules.work.info(e.race, d.type_ex)
}

/// `CWorkUpBuildingRole::Init` (0x559e70, vtbl+24), after `IBuildingRole::InitCommon`: the piles of the
/// buildingInfo records (`AddPile` + `SetBuildingId`; a patch pile is hidden): type 0 is the
/// production pile, type 1 a delivery pile. State 1, first update in 2 ticks.
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y) = (e.x(), e.y());
    let piles = info(world, id).map(|i| i.piles.clone()).unwrap_or_default();
    let mut product = 0;
    let mut delivery = Vec::new();
    for p in piles {
        let Some(pile) = crate::pile::add_pile_ex(world, x + p.x as i32, y + p.y as i32, p.good, 0, p.kind, p.appearance, p.patch, p.pixel) else {
            continue;
        };
        super::attach(world, id, pile);
        if let Some(r) = super::role_mut(world, id) {
            r.base.piles.push(pile);
        }
        if p.patch == 1
            && let Some(e) = world.entities.by_index_mut(pile as u32)
        {
            e.flags &= !flag::VISIBLE;
        }
        match p.kind {
            0 => product = pile, // "More than one production pile" asserted
            1 => delivery.push(pile),
            _ => {}
        }
    }
    // "delivery piles exist" asserted.
    if let Some((base, s)) = role(world, id) {
        base.inhabitant = 0;
        base.worker_in_house = false;
        *s = State { work: WorkBase { state: 1, ..WorkBase::default() }, cycle: 0, product, delivery, animals: 0 };
    }
    world.register_for_logic_update(id, 2);
    if super::role(world, id).is_some_and(|r| r.base.house_kind == super::house_kind::ANIMALRANCH) {
        ranch_init(world, id);
    }
}

/// `CAnimalRanchRole::Init` (0x533f50), after the workup `Init`: one animal; the ensign node
/// (building +0x48) becomes the pen's gate: move cost 7 and node flag 0x02. TODO(port): the
/// building's animation bytes +0x9c / +0x9e (gfx only).
fn ranch_init(world: &mut World, id: u16) {
    if let Some((_, s)) = role(world, id) {
        s.animals = 1;
    }
    let gate = match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => d.ensign,
        _ => return,
    };
    if let Some(n) = world.grid.checked_index(gate.x(), gate.y()) {
        world.layers.walk_move[n] |= 7;
        world.layers.flags[n] |= 2;
    }
}

/// Building types of the two ranches.
const ANIMALRANCH: u16 = 12;
const DONKEYRANCH: u16 = 13;
/// `SETTLER_DONKEY`.
const DONKEY: u16 = 60;

/// `CAnimalRanchRole::LogicUpdate` (0x533ff0): states 1 and 2 as the workup role. State 3, when
/// the farmer is home, the building works and every delivery pile has a good (`HaveMaterial`):
/// * animal ranch: if the production pile has room (< 8): with fewer than 6 animals, order the
///   feeding list (event 3), and above 0x3ae10000 on the game RNG (77 %) one more animal; lock
///   the piles. With 6, one animal goes onto the production pile (`IncreaseUnforeseen(1)` on
///   +0x17e, 0x53430c).
/// * donkey ranch: order the feeding list and lock the piles; then with 25 % (game RNG at most
///   0x3fffffff): with more than 2 animals one leaves as a donkey (`AddSettler` at the gate,
///   the ensign +0x48, if that node is free: flag 0x01 clear and no map object), else one more
///   animal.
///
/// Otherwise look again in 31 ticks. An order is not re-registered: the farmer's check-in is.
pub fn ranch_logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (kind, owner, working) = (e.kind, e.owner, e.flags & FLAG_WORKING != 0);
    let gate = match &e.data {
        EntityData::Building(d) => d.ensign,
        _ => return,
    };
    let Some((_, s)) = role(world, id) else { return };
    if s.work.state != 3 {
        // States 1 and 2 are the workup role's.
        return logic_update_states(world, id);
    }
    let home = super::role(world, id).is_some_and(|r| r.base.worker_in_house);
    let Some(s) = state(world, id) else { return };
    if !(home && working && have_material(world, s)) {
        world.register_for_logic_update(id, 31);
        return;
    }
    let (product, animals) = (s.product, s.animals);
    let settler = super::role(world, id).map_or(0, |r| r.base.inhabitant);
    let list = info(world, id).and_then(|i| i.anim_lists.first().copied()).unwrap_or(0);
    let order = |world: &mut World| {
        crate::jobs::core::set_event(world, settler, EntityEvent { event: 3, kind: 0, data: [0, list as i32, 0] });
    };
    match kind {
        ANIMALRANCH => {
            if crate::pile::get(world, product).is_none_or(|p| p.amount >= 8) {
                world.register_for_logic_update(id, 31);
                return;
            }
            if animals < 6 {
                order(world);
                if world.rng.rand() << 16 > 0x3ae1_0000
                    && let Some((_, s)) = role(world, id)
                {
                    s.animals += 1;
                }
                lock_piles(world, id, true);
                return;
            }
            if let Some((_, s)) = role(world, id) {
                s.animals -= 1;
            }
            crate::pile::increase_unforeseen(world, product, 1);
        }
        DONKEYRANCH => {
            order(world);
            lock_piles(world, id, true);
            if world.rng.rand() << 16 > 0x3fff_ffff {
                return;
            }
            if animals > 2 {
                let free = world.grid.checked_index(gate.x(), gate.y()).is_some_and(|n| world.layers.flags[n] & 1 == 0 && world.layers.map_object[n] == 0);
                if free {
                    if let Some((_, s)) = role(world, id) {
                        s.animals -= 1;
                    }
                    crate::create::add_settler(world, gate.x(), gate.y(), owner, DONKEY);
                }
            } else if let Some((_, s)) = role(world, id) {
                s.animals += 1;
            }
            return;
        }
        _ => {}
    }
    world.register_for_logic_update(id, 31);
}

/// `CWorkUpBuildingRole::HaveMaterial` (0x55a580): every delivery pile has a good available
/// (pile `+0x42 < +0x41`, read here as
/// `pile::available > 0`).
fn have_material(world: &World, s: &State) -> bool {
    s.delivery.iter().all(|&p| crate::pile::available(world, p) > 0)
}

/// `CWorkUpBuildingRole::LockPiles(lock)` (0x55a8d0, vtbl+72): flag 0x10 on every delivery pile
/// (not the production pile).
fn lock_piles(world: &mut World, id: u16, lock: bool) {
    let piles = state(world, id).map(|s| s.delivery.clone()).unwrap_or_default();
    for p in piles {
        if let Some(e) = world.entities.by_index_mut(p as u32) {
            if lock {
                e.flags |= PILE_LOCKED;
            } else {
                e.flags &= !PILE_LOCKED;
            }
        }
    }
}

/// `CWorkUpBuildingRole::LogicUpdate` (0x55a1e0; with building flag 0x200 it first refreshes the
/// building's dialog, vtbl+88, which is GUI only). A ranch has its own ([`ranch_logic_update`]).
pub fn logic_update(world: &mut World, id: u16) {
    if super::role(world, id).is_some_and(|r| r.base.house_kind == super::house_kind::ANIMALRANCH) {
        return ranch_logic_update(world, id);
    }
    logic_update_states(world, id);
}

/// The workup `LogicUpdate` proper (states 1 and 2 are shared with the ranch).
fn logic_update_states(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let Some((_, s)) = role(world, id) else { return };
    match s.work.state {
        1 => {
            if work::order_inhabitant(world, id)
                && let Some((_, s)) = role(world, id)
            {
                s.work.state = 3;
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
        3 => order_work(world, id),
        _ => {}
    }
}

fn order_work(world: &mut World, id: u16) {
    let working = world.entities.by_index(id as u32).is_some_and(|e| e.flags & FLAG_WORKING != 0);
    let home = super::role(world, id).is_some_and(|r| r.base.worker_in_house);
    let Some(s) = state(world, id) else { return };
    let full = crate::pile::get(world, s.product).is_none_or(|p| p.amount >= 8);
    if !home || !working || full || !have_material(world, s) {
        world.register_for_logic_update(id, 31);
        return;
    }
    let settler = super::role(world, id).map_or(0, |r| r.base.inhabitant);
    let kind = world.entities.by_index(settler as u32).map_or(0, |e| e.kind);
    let mut list = 0;
    if kind == ALTERNATING_WORKER
        && let Some((_, s)) = role(world, id)
    {
        if s.cycle >= 5 {
            s.cycle = 0;
        } else {
            list = 1;
            s.cycle += 1;
        }
    }
    // "vAnimListId[iWorkList] != 0" asserted.
    let list_id = info(world, id).and_then(|i| i.anim_lists.get(list).copied()).unwrap_or(0);
    let ev = EntityEvent { event: 3, kind: 0, data: [0, list_id as i32, 0] };
    crate::jobs::core::set_event(world, settler, ev);
    lock_piles(world, id, true);
}

/// `CWorkUpBuildingRole::SettlerEnter` (0x55a050, vtbl+76): the first entry makes the worker the
/// inhabitant and, if the building works, starts its piles (event 8 to the observers). The
/// worker gets MAGIC_INVISIBLE (from now on the building draws it, `FillGfxInfo` 0x55a3a0) and
/// its offset is put on the door's cell; then the production delay (state 2) and the delivery
/// piles are unlocked.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let delay = work::production_delay(world, id);
    let Some(b) = world.entities.by_index(id as u32) else { return true };
    let (working, bx, by) = (b.flags & FLAG_WORKING != 0, b.x(), b.y());
    let EntityData::Building(d) = &b.data else { return true };
    let door = d.door;
    let first = super::role(world, id).is_some_and(|r| !r.base.worker_in_house);
    if first {
        if let Some((base, _)) = role(world, id) {
            base.worker_in_house = true;
            base.inhabitant = settler;
        }
        if working {
            super::notify(world, id, EntityEvent { event: 8, kind: 0, data: [id as i32, 0, 0] });
        }
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags = (e.flags & !flag::VISIBLE) | flag::MAGIC_INVISIBLE;
    }
    // CSettler::SetOffset(((dx − dy / 2) · 24, dy · 12)) for the door's cell offset (dx, dy).
    let (dx, dy) = (door.x() - bx, door.y() - by);
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.displacement_costs = 10;
        let one = crate::jobs::state::OFFSET_ONE;
        j.offset = ((24 * dx - 12 * dy) * one, 12 * dy * one);
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
    lock_piles(world, id, false);
    true
}

/// `CWorkUpBuildingRole::GoodArrive` (vtbl+68, 0x55a1d0): nothing.
pub fn good_arrive(_world: &mut World, _id: u16, _good: u8) {}

/// `CWorkUpBuildingRole::ConvertEventIntoGoal` (vtbl+92, 0x533d00): nothing.
pub fn convert_event_into_goal(_world: &mut World, _id: u16, _ev: &EntityEvent) {}

/// `CWorkUpBuildingRole::GetPileIdWithGood(good)` (0x55a5d0, vtbl+56): the delivery pile of that good,
/// else the production pile if it holds it ("Illegal goodcheck" otherwise).
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    let Some(s) = state(world, id) else { return 0 };
    let good_of = |p: u16| crate::pile::get(world, p).map(|d| d.good);
    if let Some(&p) = s.delivery.iter().find(|&&p| good_of(p) == Some(good)) {
        return p;
    }
    if good_of(s.product) == Some(good) { s.product } else { 0 }
}

/// `CWorkUpBuildingRole::GetBuildingNeed(good)` (0x55a660, vtbl+52): the delivery pile of that
/// good.
pub fn building_need(world: &World, id: u16, good: u8) -> u16 {
    let Some(s) = state(world, id) else { return 0 };
    s.delivery.iter().copied().find(|&p| crate::pile::get(world, p).is_some_and(|d| d.good == good)).unwrap_or(0)
}

/// `IBuildingRole::GetPileIdWithNeedForGood` (release 0x534400, not overridden): vtbl+56,
/// `GetPileIdWithGood`.
pub fn pile_with_need_for_good(world: &World, id: u16, good: u8) -> u16 {
    pile_with_good(world, id, good)
}
