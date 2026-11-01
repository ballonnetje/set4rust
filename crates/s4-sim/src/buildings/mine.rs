//! `CMineRole` (house kind 7, release vtable 0x1059484): the stone, iron, gold, coal and sulfur
//! mines. The miner is a house worker (`jobs::house_worker`); each work cycle the mine first
//! needs work units, bought with food from its delivery piles, then digs one unit of its resource
//! from a random node under it and, when that succeeds, orders the miner's work list (push the
//! cart out, tip the good on the production pile, push it back in, check in).
//!
//! States: 1 order the inhabitant, 2 production delay (at least 4 updates of 14 ticks), 3 work.
//! Evidence: `tools/re/decompiled/script/mine/`.

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works.
const FLAG_WORKING: u32 = 0x1000;
/// Food goods: bread, fish, meat.
const BREAD: u8 = 9;
const FISH: u8 = 11;
const MEAT: u8 = 24;
/// Work units a fresh mine starts with (`Init`: +0x17c = 0x0a00).
const START_UNITS: u8 = 10;

/// `CMineRole` (+0x17c..+0x1b0).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    /// +0x17c: the resource the mine digs (`TYPE << 4` of the resource layer).
    pub resource: u8,
    /// +0x17d: work units left; one per dig attempt, bought with food.
    pub units: u8,
    /// +0x17e: the production pile.
    pub product: u16,
    /// +0x188: the delivery piles, in buildingInfo order.
    pub delivery: Vec<u16>,
    /// +0x194: the food piles in the order the mine eats from them.
    pub food: Vec<u16>,
    /// +0x1a0..+0x1ae: the last 15 dig attempts, oldest first (100 = found, 0 = not).
    pub history: [u8; 15],
    /// +0x1b0: updates in a row with a dig rate below 1 %.
    pub exhausted: i32,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.work.hash(h);
        h.u8(self.resource);
        h.u8(self.units);
        h.u16(self.product);
        h.u16s(&self.delivery);
        h.u16s(&self.food);
        self.history.iter().for_each(|&b| h.u8(b));
        h.u32(self.exhausted as u32);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::Mine(s) } => Some((base, s)),
        _ => None,
    }
}

fn state(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Mine(s), .. } => Some(s),
        _ => None,
    }
}

fn info(world: &World, id: u16) -> Option<&work::WorkInfo> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    world.rules.work.info(e.race, d.type_ex)
}

/// 0x54b9e0: the mine's resource and the order of its food, by building type.
fn resource_and_food(kind: u16) -> Option<(u8, [u8; 3])> {
    Some(match kind {
        14 => (0x50, [BREAD, MEAT, FISH]), // stone
        15 => (0x20, [MEAT, BREAD, FISH]), // iron
        16 => (0x30, [FISH, BREAD, MEAT]), // gold
        17 => (0x10, [BREAD, MEAT, FISH]), // coal
        18 => (0x40, [MEAT, BREAD, FISH]), // sulfur
        _ => return None,
    })
}

/// `CMineRole::Init` (0x54b280, vtbl+24), after `IBuildingRole::InitCommon`: state 1, first
/// update in 2 ticks; the piles of the buildingInfo records as in `CWorkUpBuildingRole::Init`
/// (type 0 production, type 1 delivery; a patch pile is hidden); 10 work units; the food order
/// and resource (0x54b9e0, through `GetPileIdWithGood`).
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, kind) = (e.x(), e.y(), e.kind);
    world.register_for_logic_update(id, 2);
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
            0 => product = pile,
            1 => delivery.push(pile),
            _ => {}
        }
    }
    if let Some((base, s)) = role(world, id) {
        base.inhabitant = 0;
        base.worker_in_house = false;
        *s = State { work: WorkBase { state: 1, ..WorkBase::default() }, units: START_UNITS, product, delivery, ..State::default() };
    }
    if let Some((resource, food)) = resource_and_food(kind) {
        // `GetPileIdWithGood` of each food, pushed as is (0 when the mine has no such pile).
        let food: Vec<u16> = food.iter().map(|&g| pile_with_good(world, id, g)).collect();
        if let Some((_, s)) = role(world, id) {
            s.resource = resource;
            s.food = food;
        }
    }
}

/// 0x54c330: the dig rate of the last 15 attempts in percent.
fn dig_rate(s: &State) -> i32 {
    s.history.iter().map(|&b| b as i8 as i32).sum::<i32>() / 15
}

/// Mine `id`'s role, for the building dialog (D19: `FillDialog` 0x54c060 shows the dig rate
/// [`productivity`], the delivery piles' goods and amounts, and the production pile's).
pub fn get(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Mine(s), .. } => Some(s),
        _ => None,
    }
}

/// The dig rate of mine `id` in percent (0x54c330 over the 15-attempt history), as its dialog
/// shows it.
pub fn productivity(world: &World, id: u16) -> Option<i32> {
    get(world, id).map(dig_rate)
}

/// `CMineRole::LogicUpdate` (0x54b630; with building flag 0x200 it first refreshes the building's
/// dialog, vtbl+88, which is GUI only).
pub fn logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id).max(4);
    let Some((_, s)) = role(world, id) else { return };
    match s.work.state {
        1 => {
            if work::order_inhabitant(world, id)
                && let Some((_, s)) = role(world, id)
            {
                s.work.state = 3;
                s.work.delay = 0;
            }
        }
        2 => {
            if s.work.delay > delay {
                s.work.state = 3;
                s.work.delay = 0;
                world.register_for_logic_update(id, 1);
            } else {
                s.work.delay += 1;
                world.register_for_logic_update(id, 14);
            }
        }
        3 => {
            let Some(b) = world.entities.by_index(id as u32) else { return };
            let working = b.flags & FLAG_WORKING != 0;
            let home = super::role(world, id).is_some_and(|r| r.base.worker_in_house);
            let full = state(world, id).and_then(|s| crate::pile::get(world, s.product)).is_none_or(|p| p.amount >= 8);
            if !home || !working || full {
                world.register_for_logic_update(id, 31);
                return;
            }
            let mut exhausted = false;
            if let Some((_, s)) = role(world, id) {
                if dig_rate(s) < 1 {
                    s.exhausted += 1;
                } else {
                    s.exhausted = 0;
                }
                if s.exhausted > 15 {
                    exhausted = true;
                    s.exhausted = 0;
                }
            }
            if exhausted && let Some(b) = world.entities.by_index(id as u32) {
                // GUI_WARN_MINE_EMPTY (0x54b6e4).
                let ev = crate::event::SimEvent::warning(b.owner, 0x9de, b.pos);
                world.events.push(ev);
            }
            do_work(world, id);
        }
        _ => {}
    }
}

/// 0x54bc20: without work units, eat; with one, spend it on a dig attempt. Found: order the
/// miner's first work list (event 3); not found: look again in 14 ticks. Either way state 2.
/// Without food nothing happens until a good arrives (`GoodArrive`).
fn do_work(world: &mut World, id: u16) {
    if state(world, id).is_some_and(|s| s.units == 0) {
        eat(world, id);
    }
    let Some((base, s)) = role(world, id) else { return };
    if s.units == 0 {
        return;
    }
    s.units -= 1;
    let settler = base.inhabitant;
    let found = dig(world, id);
    record(world, id, found);
    if found {
        let list = info(world, id).and_then(|i| i.anim_lists.first().copied()).unwrap_or(0);
        crate::jobs::core::set_event(world, settler, EntityEvent { event: 3, kind: 0, data: [0, list as i32, 0] });
    } else {
        world.register_for_logic_update(id, 14);
    }
    if let Some((_, s)) = role(world, id) {
        s.work.state = 2;
        s.work.delay = 0;
    }
}

/// 0x54c260: shift the attempt into the history.
fn record(world: &mut World, id: u16, found: bool) {
    if let Some((_, s)) = role(world, id) {
        s.history.rotate_left(1);
        s.history[14] = if found { 100 } else { 0 };
    }
}

/// `IsAIPlayer(player)` (`CAIMain` vtbl+40, 0x4385f0) and the AI player's level (player record
/// int at 0x1292640 + 0x17c · player; 0x434600).
fn ai_level(world: &World, player: u8) -> Option<i32> {
    world.ai.is_ai(player).then(|| world.ai.vars.records[player as usize].level)
}

/// 0x54bd70: eat one good from the first food pile that has one available (`DecreaseUnforeseen`):
/// the first food gives 10 work units, the others 2. AI players get more (see the release).
fn eat(world: &mut World, id: u16) {
    let Some(s) = state(world, id) else { return };
    let mut units: i32 = 0;
    for (k, &pile) in s.food.clone().iter().enumerate() {
        if crate::pile::available(world, pile) > 0 {
            crate::pile::decrease_unforeseen(world, pile, 1);
            units = if k == 0 { 10 } else { 2 };
            break;
        }
    }
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    if let Some(level) = ai_level(world, owner).filter(|&l| l > 1) {
        let product_amount = state(world, id).and_then(|s| crate::pile::get(world, s.product)).map_or(0, |p| p.amount as i32);
        if units != 0 {
            if product_amount < 4 {
                units += 3;
            }
            units += level;
        } else if product_amount < 2 {
            let r = world.rng.rand() << 16;
            if level < 3 {
                if r < 0xccc_0000 {
                    units = level + 3;
                }
            } else if r < 0x4000_0000 {
                units = 10;
            }
        }
    }
    if let Some((_, s)) = role(world, id) {
        s.units = s.units.wrapping_add(units as u8);
    }
}

/// 0x54bef0: dig one unit of the mine's resource: a random node of the first 64 spiral entries
/// around the mine; if it holds the resource and a random 0..=3 (at most 3) is below its amount,
/// the amount drops by one. Human players get one try; AI players more, and they never exhaust
/// a node (see the release).
fn dig(world: &mut World, id: u16) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let (x, y, owner) = (e.x(), e.y(), e.owner);
    let resource = state(world, id).map_or(0, |s| s.resource);
    let level = ai_level(world, owner).unwrap_or(0);
    let tries = level.max(1);
    let cap = if level >= 3 { 0 } else if level > 1 { 3 - level } else { 3 };
    let keep = u8::from(level != 0);
    for _ in 0..tries {
        let k = (world.rng.rand() & 0x3f) as usize;
        let Some(n) = world.grid.checked_index(x + crate::spiral::dx(k), y + crate::spiral::dy(k)) else { continue };
        let r = world.layers.resource[n];
        if r & 0xf0 != resource {
            continue;
        }
        let roll = ((world.rng.rand() & 3) as i32).min(cap);
        let amount = r & 0x0f;
        if roll < amount as i32 {
            let left = if keep < amount { amount - 1 } else { keep };
            world.layers.set_resource(n, resource, left);
            return true;
        }
    }
    false
}

/// `CMineRole::SettlerEnter` (0x54b4c0, vtbl+76): the first entry makes the worker the
/// inhabitant and, if the building works, starts its piles (event 8 to the observers). The miner
/// gets MAGIC_INVISIBLE; its offset is the door's cell, except for Roman mines (race 0), where it
/// is 0. Then state 2, next update in 14 ticks.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let Some(b) = world.entities.by_index(id as u32) else { return true };
    let (working, bx, by, race) = (b.flags & FLAG_WORKING != 0, b.x(), b.y(), b.race);
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
    let one = crate::jobs::state::OFFSET_ONE;
    let offset = if race == 0 {
        (0, 0)
    } else {
        let (dx, dy) = (door.x() - bx, door.y() - by);
        ((24 * dx - 12 * dy) * one, 12 * dy * one)
    };
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.offset = offset;
        j.displacement_costs = 10;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags = (e.flags & !flag::VISIBLE) | flag::MAGIC_INVISIBLE;
    }
    if let Some((_, s)) = role(world, id) {
        s.work.state = 2;
        s.work.delay = 0;
    }
    world.register_for_logic_update(id, 14);
    true
}

/// `CMineRole::GoodArrive` (0x54b930, vtbl+68): wake the mine (16 ticks) unless it is scheduled.
pub fn good_arrive(world: &mut World, id: u16, _good: u8) {
    let registered = world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::REGISTERED != 0);
    if !registered {
        world.register_for_logic_update(id, 16);
    }
}

/// `CMineRole::GetPileIdWithGood(good)` (0x54b960, vtbl+56): the delivery pile of that good, else
/// the production pile if it holds it.
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    let Some(s) = state(world, id) else { return 0 };
    let good_of = |p: u16| crate::pile::get(world, p).map(|d| d.good);
    if let Some(&p) = s.delivery.iter().find(|&&p| good_of(p) == Some(good)) {
        return p;
    }
    if good_of(s.product) == Some(good) { s.product } else { 0 }
}
