//! The temples (own reverse engineering of the shipped exe; SIM.md §20): `CSmallTempleRole`
//! (house kind 16, vtable 0x1058bf0) turns the race's drink into mana, `CBigTempleRole` (17,
//! vtable 0x1058508) gives mana once and makes priests.
//!
//! Small temple: the temple servant (buildingInfo `inhabitant`, type 56, a `CHouseWorkerRole`)
//! runs the work list (`vAnimListId[0]`, 223) whenever the delivery pile (Roman wine 37, Viking
//! mead 23, Maya tequila 35, Trojan good 41) has a good: he takes one, walks round the altar and
//! checks in again; every check-in after the first is a donation (`ManaPerDonation`).
//! Big temple: `ManaPerBigTemple` when it is finished; while it works, a priest (`inhabitant`,
//! type 45) every `(productionDelay + 2) · 14 + 1` ticks at its door, ten in its life.

use super::work;
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works.
const FLAG_WORKING: u32 = 0x1000;
/// Pile entity flag 0x10: locked while the servant fetches from it (role vtbl+0x48, 0x54fff0).
const PILE_LOCKED: u32 = 0x10;
/// A big temple makes at most this many priests (0x535781).
pub const PRIESTS: u8 = 10;

/// `CSmallTempleRole`: +4 state, +5 delay counter, +0x17c the delivery pile (+0x180, a dword
/// cleared by `Init`, is not read by the role).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SmallState {
    pub state: u8,
    pub delay: u8,
    pub pile: u16,
}

/// `CBigTempleRole`: +4 state, +5 delay counter, +0x17c priests made (stored from version 2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BigState {
    pub state: u8,
    pub delay: u8,
    pub priests: u8,
}

impl SmallState {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        h.u8(self.delay);
        h.u16(self.pile);
    }
}

impl BigState {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        h.u8(self.delay);
        h.u8(self.priests);
    }
}

fn small(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut SmallState)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::SmallTemple(s) } => Some((base, s)),
        _ => None,
    }
}

fn big(world: &mut World, id: u16) -> Option<&mut BigState> {
    match world.buildings.roles.get_mut(&id)? {
        Role { kind: RoleKind::BigTemple(s), .. } => Some(s),
        _ => None,
    }
}

pub fn small_state(world: &World, id: u16) -> Option<&SmallState> {
    match &world.buildings.roles.get(&id)?.kind {
        RoleKind::SmallTemple(s) => Some(s),
        _ => None,
    }
}

pub fn big_state(world: &World, id: u16) -> Option<&BigState> {
    match &world.buildings.roles.get(&id)?.kind {
        RoleKind::BigTemple(s) => Some(s),
        _ => None,
    }
}

fn owner_working(world: &World, id: u16) -> (u8, bool) {
    world.entities.by_index(id as u32).map_or((0, false), |e| (e.owner, e.flags & FLAG_WORKING != 0))
}

/// Role vtbl+0x48 (0x54fff0): flag 0x10 on the delivery pile.
fn lock_pile(world: &mut World, id: u16, lock: bool) {
    let pile = small_state(world, id).map_or(0, |s| s.pile);
    if let Some(p) = world.entities.by_index_mut(pile as u32) {
        if lock {
            p.flags |= PILE_LOCKED;
        } else {
            p.flags &= !PILE_LOCKED;
        }
    }
}

/// `CSmallTempleRole::Init` (0x54f870), after `InitCommon`: state 1, one pile per buildingInfo
/// pile record (`AddPile(pos + offset, good, 0, type, appearance, patch, pixel offset)`,
/// attached; a patch pile loses VISIBLE; the type-1 pile is the role's), then
/// `OrderInhabitant` (0x53ff40). The role does not register itself.
pub fn small_init(world: &mut World, id: u16) {
    if let Some((_, s)) = small(world, id) {
        *s = SmallState { state: 1, ..SmallState::default() };
    }
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, race) = (e.x(), e.y(), e.race);
    let EntityData::Building(d) = &e.data else { return };
    let piles = world.rules.work.info(race, d.type_ex).map(|i| i.piles.clone()).unwrap_or_default();
    for p in piles {
        let Some(q) = crate::pile::add_pile_ex(world, x + p.x as i32, y + p.y as i32, p.good, 0, p.kind, p.appearance, p.patch, p.pixel) else { continue };
        super::attach(world, id, q);
        if let Some(r) = super::role_mut(world, id) {
            r.base.piles.push(q);
        }
        if p.patch == 1
            && let Some(e) = world.entities.by_index_mut(q as u32)
        {
            e.flags &= !flag::VISIBLE;
        }
        if p.kind == 1
            && let Some((_, s)) = small(world, id)
        {
            s.pile = q;
        }
    }
    work::order_inhabitant(world, id);
}

/// `CSmallTempleRole::LogicUpdate` (0x54fb60): 1 order the servant; 2 the pause between two
/// donations (`productionDelay` + 1 updates of 14 ticks); 3 with the servant home, the temple
/// working and a good on the pile, event 3 (the work list `vAnimListId[0]`) to the servant and
/// the pile locked; otherwise again in 31 ticks.
pub fn small_logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let Some((base, s)) = small(world, id) else { return };
    match s.state {
        1 => {
            if work::order_inhabitant(world, id) {
                if let Some((_, s)) = small(world, id) {
                    s.state = 3;
                }
                return;
            }
        }
        2 => {
            if s.delay > delay {
                s.state = 3;
                s.delay = 0;
                world.register_for_logic_update(id, 1);
            } else {
                s.delay += 1;
                world.register_for_logic_update(id, 14);
            }
            return;
        }
        3 => {
            let (home, servant, pile) = (base.worker_in_house, base.inhabitant, s.pile);
            let (_, working) = owner_working(world, id);
            let goods = crate::pile::get(world, pile).is_some_and(|p| p.amount != 0);
            if home && working && goods {
                let Some(e) = world.entities.by_index(id as u32) else { return };
                let EntityData::Building(d) = &e.data else { return };
                let list = world.rules.work.info(e.race, d.type_ex).and_then(|i| i.anim_lists.first().copied()).unwrap_or(0);
                crate::jobs::core::set_event(world, servant, EntityEvent { event: 3, kind: 0, data: [0, list as i32, 0] });
                lock_pile(world, id, true);
                return;
            }
        }
        _ => return,
    }
    world.register_for_logic_update(id, 31);
}

/// `CSmallTempleRole::SettlerEnter` (0x54f9d0): the pile unlocked; the first servant becomes the
/// inhabitant (event 8 to the observers when the temple works, which starts the pile's
/// requests); any later check-in is a donation ([`crate::magic::donate`]). The servant is
/// hidden (MAGIC_INVISIBLE, the temple draws him at the door's cell offset), displacement costs
/// 10; then the pause (state 2, in 14 ticks), or without a `productionDelay` the next update in
/// 31.
pub fn small_settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    lock_pile(world, id, false);
    let (owner, working) = owner_working(world, id);
    let first = small(world, id).is_some_and(|(b, _)| !b.worker_in_house);
    if first {
        if let Some((base, _)) = small(world, id) {
            base.worker_in_house = true;
            base.inhabitant = settler;
        }
        if working {
            super::notify(world, id, EntityEvent { event: 8, kind: 0, data: [id as i32, 0, 0] });
        }
    } else {
        crate::magic::donate(world, owner);
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags = (e.flags & !flag::VISIBLE) | flag::MAGIC_INVISIBLE;
    }
    if let Some(b) = world.entities.by_index(id as u32)
        && let EntityData::Building(d) = &b.data
    {
        let (dx, dy) = (d.door.x() - b.x(), d.door.y() - b.y());
        let one = crate::jobs::state::OFFSET_ONE;
        if let Some(j) = crate::jobs::core::job_mut(world, settler) {
            j.displacement_costs = 10;
            j.offset = ((24 * dx - 12 * dy) * one, 12 * dy * one);
        }
    }
    if work::production_delay(world, id) == 0 {
        world.register_for_logic_update(id, 31);
    } else {
        if let Some((_, s)) = small(world, id) {
            s.delay = 0;
            s.state = 2;
        }
        world.register_for_logic_update(id, 14);
    }
    true
}

/// `CSmallTempleRole` vtbl+0x38 (0x53b680) and +0x40 (the base 0x534400 → +0x38): the delivery
/// pile, whatever the good.
pub fn small_pile(world: &World, id: u16) -> u16 {
    small_state(world, id).map_or(0, |s| s.pile)
}

/// `CBigTempleRole::Init` (0x5356a0), after `InitCommon`: state 3, `ManaPerBigTemple` for the
/// owner ([`crate::magic::big_temple`]), the next update in 31 ticks.
pub fn big_init(world: &mut World, id: u16) {
    if let Some(s) = big(world, id) {
        s.state = 3;
    }
    let (owner, _) = owner_working(world, id);
    crate::magic::big_temple(world, owner);
    world.register_for_logic_update(id, 31);
}

/// `CBigTempleRole::LogicUpdate` (0x5356f0): state 2 counts `productionDelay` + 1 updates of 14
/// ticks, then state 3 in one tick; state 3 makes a priest while the temple works (0x5357d0)
/// and, below [`PRIESTS`], starts the next count (or without a `productionDelay` looks again in
/// 31 ticks). After the tenth priest the temple is not updated any more.
pub fn big_logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let Some(s) = big(world, id) else { return };
    match s.state {
        2 => {
            if s.delay <= delay {
                s.delay += 1;
                world.register_for_logic_update(id, 14);
            } else {
                s.state = 3;
                s.delay = 0;
                world.register_for_logic_update(id, 1);
            }
        }
        3 => {
            let (_, working) = owner_working(world, id);
            if working {
                make_priest(world, id);
            }
            let Some(s) = big(world, id) else { return };
            if s.priests < PRIESTS {
                if delay != 0 {
                    s.delay = 0;
                    s.state = 2;
                    world.register_for_logic_update(id, 14);
                } else {
                    world.register_for_logic_update(id, 31);
                }
            }
        }
        _ => {}
    }
}

/// 0x5357d0: when the door node is free (no blocked land, no map object), one priest (the
/// buildingInfo `inhabitant`) is placed near the door (0x582770) and counted — counted even if
/// none could be placed.
fn make_priest(world: &mut World, id: u16) {
    let Some(b) = world.entities.by_index(id as u32) else { return };
    let (owner, race) = (b.owner, b.race);
    let EntityData::Building(d) = &b.data else { return };
    let (door, type_ex) = (d.door, d.type_ex);
    let Some(n) = world.grid.checked_index(door.x(), door.y()) else { return };
    if world.layers.flags[n] & 1 != 0 || world.layers.map_object[n] != 0 {
        return;
    }
    let kind = world.rules.work.info(race, type_ex).map_or(0, |i| i.inhabitant);
    crate::create::add_settlers_around(world, door.x(), door.y(), owner, kind, 1);
    if let Some(s) = big(world, id) {
        s.priests = s.priests.wrapping_add(1);
    }
}
