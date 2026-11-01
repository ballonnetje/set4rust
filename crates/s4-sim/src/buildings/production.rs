//! `CProductionBuildingRole` (house kind 4: toolsmith 21, weaponsmith 22), analysed from the
//! shipped exe (vtable 0x1059094). Like `workup`, its worker is a house worker ordered with
//! event 3; unlike it, the product is chosen per cycle from the eco-sector's tool / weapon
//! orders (`choose_product`, 0x54ce20) and the worker puts that good (`GetProductType` = +0x17c)
//! on the matching product pile.
//!
//! `LogicUpdate` (0x54cc20): 1 order the inhabitant, 2 production delay, 3 when the worker is
//! home, the building works, the product piles hold at most 7 goods together (0x54cdc0), every
//! delivery pile has a good (0x54d080) and a product is chosen: event 3 with the building's
//! first work list, piles locked (vtbl+72) until the worker checks in.

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works.
const FLAG_WORKING: u32 = 0x1000;
/// Pile entity flag 0x10, set by `LockPiles(true)` (0x54d9e0).
const PILE_LOCKED: u32 = 0x10;

/// `CProductionBuildingRole` (+0x17c product, +0x17d last order index, +0x180 product piles,
/// +0x18c delivery piles, +0x198 the product goods in product-pile order).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    /// +0x17c: the good the next cycle makes (`GetProductType`).
    pub product: u8,
    /// +0x17d: index of the order last served (round robin).
    pub last: u8,
    pub products: Vec<u16>,
    pub delivery: Vec<u16>,
    pub goods: Vec<u8>,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.work.hash(h);
        h.u8(self.product);
        h.u8(self.last);
        h.u16s(&self.products);
        h.u16s(&self.delivery);
        h.bytes(&self.goods);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::Production(s) } => Some((base, s)),
        _ => None,
    }
}

fn state(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Production(s), .. } => Some(s),
        _ => None,
    }
}

/// `CProductionBuildingRole::Init` (0x54c810), after `IBuildingRole::InitCommon`: the
/// buildingInfo piles. A product pile (type 0) goes on the spiral around the ensign (the j-th
/// product pile at spiral entry j); a delivery pile (type 1) at its offset from the building.
/// State 1, product 0, first update in 2 ticks.
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, race) = (e.x(), e.y(), e.race);
    let EntityData::Building(d) = &e.data else { return };
    let (type_ex, ensign) = (d.type_ex, d.ensign);
    let piles = world.rules.work.info(race, type_ex).map(|i| i.piles.clone()).unwrap_or_default();
    let (mut products, mut delivery, mut goods) = (Vec::new(), Vec::new(), Vec::new());
    for p in piles {
        let (px, py, role) = match p.kind {
            0 => {
                let j = products.len();
                (ensign.x() + crate::spiral::dx(j), ensign.y() + crate::spiral::dy(j), 0)
            }
            1 => (x + p.x as i32, y + p.y as i32, 1),
            _ => continue,
        };
        let Some(q) = crate::pile::add_pile_ex(world, px, py, p.good, 0, role, 0, 0, (0, 0)) else { continue };
        super::attach(world, id, q);
        if let Some(r) = super::role_mut(world, id) {
            r.base.piles.push(q);
        }
        if role == 0 {
            products.push(q);
            goods.push(p.good);
        } else {
            delivery.push(q);
        }
    }
    if let Some((base, s)) = role(world, id) {
        base.inhabitant = 0;
        base.worker_in_house = false;
        *s = State { work: WorkBase { state: 1, ..WorkBase::default() }, product: 0, last: 0, products, delivery, goods };
    }
    world.register_for_logic_update(id, 2);
}

/// `LockPiles(lock)` (0x54d9e0): flag 0x10 on every product and delivery pile.
fn lock_piles(world: &mut World, id: u16, lock: bool) {
    let piles: Vec<u16> = state(world, id).map(|s| s.products.iter().chain(&s.delivery).copied().collect()).unwrap_or_default();
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

fn amount(world: &World, pile: u16) -> u32 {
    crate::pile::get(world, pile).map_or(0, |p| p.amount as u32)
}

/// 0x54ce20: the product of the next cycle from the eco-sector's orders at the ensign (toolsmith:
/// tool orders round robin from +0x17d; weaponsmith: weapon orders, else the weapon percentages),
/// sim's `eco::choose_product`. Sets +0x17c / +0x17d.
fn choose_product(world: &mut World, id: u16) -> bool {
    let kind = world.entities.by_index(id as u32).map_or(0, |e| e.kind);
    let eco = super::storage::eco_of_building(world, id);
    let Some(s) = state(world, id) else { return false };
    let (goods, mut last, mut product) = (s.goods.clone(), s.last, s.product);
    let works = crate::eco::choose_product(world, eco, kind, &goods, &mut last, &mut product);
    if let Some((_, s)) = role(world, id) {
        s.last = last;
        s.product = product;
    }
    works
}

/// `CProductionBuildingRole::LogicUpdate` (0x54cc20).
pub fn logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let Some((_, s)) = role(world, id) else { return };
    match s.work.state {
        1 => {
            if work::order_inhabitant(world, id)
                && let Some((_, s)) = role(world, id)
            {
                s.work.state = 3;
                return;
            }
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
        3 => {
            let working = world.entities.by_index(id as u32).is_some_and(|e| e.flags & FLAG_WORKING != 0);
            let home = super::role(world, id).is_some_and(|r| r.base.worker_in_house);
            let Some(s) = state(world, id) else { return };
            let room = s.products.iter().map(|&p| amount(world, p)).sum::<u32>() <= 7;
            let material = s.delivery.iter().all(|&p| amount(world, p) != 0);
            if !(home && working && room && material && choose_product(world, id)) {
                world.register_for_logic_update(id, 31);
                return;
            }
            let (race, type_ex) = world.entities.by_index(id as u32).map_or((0, 0), |e| match &e.data {
                EntityData::Building(d) => (e.race, d.type_ex),
                _ => (e.race, 0),
            });
            let list = world.rules.work.info(race, type_ex).and_then(|i| i.anim_lists.first().copied()).unwrap_or(0);
            let settler = super::role(world, id).map_or(0, |r| r.base.inhabitant);
            crate::jobs::core::set_event(world, settler, EntityEvent { event: 3, kind: 0, data: [0, list as i32, 0] });
            lock_piles(world, id, true);
        }
        _ => {}
    }
}

/// `CProductionBuildingRole::SettlerEnter` (0x54ca90): as `workup`'s: the first entry makes the
/// worker the inhabitant and starts the piles (event 8); the worker is hidden at the door's
/// in-house offset; then the production delay (state 2) and the piles unlocked.
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
    // CSettler offset ((dx − dy / 2) · 24, dy · 12) for the door's cell offset (dx, dy).
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
        world.register_for_logic_update(id, 31);
    }
    lock_piles(world, id, false);
    true
}

/// `CProductionBuildingRole::GoodArrive` (0x54cc10): nothing.
pub fn good_arrive(_world: &mut World, _id: u16, _good: u8) {}

/// The base `ConvertEventIntoGoal` (0x53ae00 is empty).
pub fn convert_event_into_goal(_world: &mut World, _id: u16, _ev: &EntityEvent) {}

/// `CProductionBuildingRole::GetPileIdWithGood(good)` (0x54d350): the delivery pile of that
/// good, else the product pile of that good.
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    let Some(s) = state(world, id) else { return 0 };
    let good_of = |p: u16| crate::pile::get(world, p).map(|d| d.good);
    s.delivery.iter().chain(&s.products).copied().find(|&p| good_of(p) == Some(good)).unwrap_or(0)
}

/// `IBuildingRole::GetPileIdWithNeedForGood` (not overridden): vtbl+56, `GetPileIdWithGood`.
pub fn pile_with_need_for_good(world: &World, id: u16, good: u8) -> u16 {
    pile_with_good(world, id, good)
}

/// `GetProductType` as the house worker (settler type 15, SMITH) reads it on PUT_GOOD_VIRTUAL
/// (release 0x57c610: home role +0x17c).
pub fn product_type(world: &World, id: u16) -> u8 {
    state(world, id).map_or(0, |s| s.product)
}
