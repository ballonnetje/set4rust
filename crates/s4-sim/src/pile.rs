//! Piles of goods: `CPile`, its roles (`IPileRole` and subclasses), `CPileObserverList` and
//! `CPileMgr` (MapObjects/Pile/*.cpp).
//!
//! A pile holds up to 8 goods of one type on one node. `leaving` counts goods carriers have
//! reserved, `coming` goods on their way; the observer list remembers who (an observer with a
//! negative delta takes goods, +1 brings one). The role decides when the pile offers its goods
//! to the eco-sector (offer lists in `eco`) and what its logic update does:
//! production piles ask for storage space, delivery piles ask for goods for their building,
//! free ("anywhere") piles look for storage and vanish when empty.
//!
//! Role vtables and the creating constructor were checked in the shipped exe (RTTI vtables
//! `IPileRole` 0x105a6c0 … `CTradePileRole` 0x105a950, slots +8 Increase … +72
//! ChangeGoodTypeUnforseen; `CPile` ctor 0x56efb0, `CAnyWherePileRole::Init` 0x56e700, which
//! also clears the pile's event list; `ConvertGoodType` 0x570ee0 with its table at .data
//! 0x11651b0).

use crate::eco;
use crate::entity::{Entity, EntityData, ObjType, PileData, flag};
use crate::grid::Y16X16;
use crate::jobs::state::EntityEvent;
use crate::world::World;

/// `MAX_PILE_AMOUNT`.
pub const MAX_AMOUNT: u8 = 8;

/// `IPileRole::PILE_*` (`CPileMgr::CreatePileRole`).
pub mod role {
    pub const PRODUCTION: u8 = 0;
    pub const DELIVER: u8 = 1;
    pub const STORAGE: u8 = 2;
    pub const ANYWHERE: u8 = 3;
    pub const TRADE: u8 = 4;
    pub const BUILDING_SITE: u8 = 5;
}

/// `T_OBSERVER_TARGET` values a pile is attached under.
pub mod target {
    /// The building a pile belongs to (the pile's own target).
    pub const BUILDING: u8 = 0;
    pub const PILE_DST: u8 = 1;
    pub const PILE_SRC: u8 = 2;
    pub const PILE_SRC_2: u8 = 3;
    pub const PILE_SRC_3: u8 = 4;
}

/// `CEntityEvent` ids a pile sends / receives.
pub mod event {
    /// Building stopped working (`IPileRole::SubjectStopped`).
    pub const SUBJECT_STOPPED: i32 = 7;
    /// Building started working (`SubjectStarted`).
    pub const SUBJECT_STARTED: i32 = 8;
    /// `TARGET_DIE`: the pile is gone or its observers are dropped.
    pub const TARGET_DIE: i32 = 9;
}

/// One `SPileObserver`: who, how many goods (negative: taking), under which target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PileObserver {
    pub entity: u16,
    pub delta: i8,
    pub target: u8,
}

/// `CPileMgr::ConvertGoodType` table (.data 0x11651b0, 15 pairs per race, 0-terminated):
/// goods another race does not use become the local equivalent.
const CONVERT: [&[(u8, u8)]; 5] = [
    // Roman
    &[(1, 16), (5, 34), (6, 8), (13, 30), (19, 16), (23, 37), (35, 37), (26, 30), (39, 30), (42, 16), (41, 37), (38, 34)],
    // Viking
    &[(1, 19), (6, 5), (13, 26), (30, 26), (35, 23), (37, 23), (39, 26), (42, 19), (41, 23), (38, 5)],
    // Maya
    &[(5, 6), (19, 1), (23, 35), (30, 13), (26, 13), (37, 35), (39, 13), (42, 1), (41, 35), (38, 6)],
    // Dark Tribe
    &[],
    // Trojan
    &[(30, 39), (26, 39), (13, 39), (5, 38), (6, 38), (37, 41), (23, 41), (35, 41), (19, 42), (1, 42)],
];

/// `CPileMgr::ConvertGoodType(good, race)`.
pub fn convert_good_type(good: u8, race: u8) -> u8 {
    CONVERT.get(race as usize).and_then(|t| t.iter().find(|p| p.0 == good)).map_or(good, |p| p.1)
}

fn data(w: &World, id: u16) -> Option<&PileData> {
    match &w.entities.by_index(id as u32)?.data {
        EntityData::Pile(p) => Some(p),
        _ => None,
    }
}

fn data_mut(w: &mut World, id: u16) -> Option<&mut PileData> {
    match &mut w.entities.by_index_mut(id as u32)?.data {
        EntityData::Pile(p) => Some(p),
        _ => None,
    }
}

/// The pile's state, if `id` is a pile.
pub fn get(w: &World, id: u16) -> Option<&PileData> {
    data(w, id)
}

fn node(w: &World, id: u16) -> Option<usize> {
    let p = w.entities.by_index(id as u32)?.pos;
    w.grid.checked_index(p.x(), p.y())
}

fn pos(w: &World, id: u16) -> (i32, i32) {
    w.entities.by_index(id as u32).map_or((0, 0), |e| (e.pos.x(), e.pos.y()))
}

fn flags(w: &World, id: u16) -> u32 {
    w.entities.by_index(id as u32).map_or(0, |e| e.flags)
}

fn in_offer_list(w: &World, id: u16) -> bool {
    flags(w, id) & flag::OFFERED != 0
}

/// `CPile::NumberOfAvailableGoods`.
pub fn available(w: &World, id: u16) -> i32 {
    data(w, id).map_or(0, |p| p.amount as i32 - p.leaving as i32)
}

fn node_eco(w: &World, id: u16) -> u16 {
    node(w, id).map_or(0, |n| eco::eco_of(w, n))
}

// ---- CPileMgr ----

/// `CPileMgr::AddPile(x, y, good, amount, role, 0, 0, 0, 0)`.
pub fn add_pile(w: &mut World, x: i32, y: i32, good: u8, amount: u8, role: u8) -> Option<u16> {
    add_pile_ex(w, x, y, good, amount, role, 0, 0, (0, 0))
}

/// `CPileMgr::AddPile(x, y, good, amount, role, variant, patch, offset x, offset y)` (release 0x5708a0):
/// cap 20000; the `CPile` constructor puts the pile on the node (pile id, move cost 7, flag
/// 0x02), counts its goods for the eco-sector and runs the role's `Init`.
#[allow(clippy::too_many_arguments)]
pub fn add_pile_ex(w: &mut World, x: i32, y: i32, good: u8, amount: u8, role: u8, variant: u8, patch: u8, offset: (i16, i16)) -> Option<u16> {
    if role > role::BUILDING_SITE {
        return None; // "The programm can't use this pile role" (would crash)
    }
    if w.counters.piles >= 20000 {
        return None; // "NO more piles!!!"
    }
    let n = w.grid.checked_index(x, y)?;
    w.counters.piles += 1;
    let slot = w.entities.free_slot()?;
    let d = PileData { good, amount: amount.min(MAX_AMOUNT), variant, patch, offset, ..Default::default() };
    let mut e = Entity::new(ObjType::Pile, 0, 0, 0, Y16X16::pack(x, y), EntityData::Pile(d));
    e.flags = (e.flags & !flag::REGISTERED) | 0x1100;
    w.entities.insert(slot, e);
    w.layers.set_pile_id(n, slot);
    adjust_statistic(w, slot, amount as i32, 0);
    role_init(w, slot, role);
    Some(slot)
}

/// `CPileMgr::DeletePile`.
pub fn delete_pile(w: &mut World, id: u16) {
    if id != 0 {
        w.counters.piles = w.counters.piles.saturating_sub(1);
        w.kill(id, 0);
    }
}

/// `CPile::Delete` (from `CMapObjectMgr::Kill`): off the node, observers told `TARGET_DIE` and
/// dropped, out of the logic update.
pub(crate) fn on_delete(w: &mut World, id: u16) {
    if let Some(n) = node(w, id) {
        w.layers.set_pile_id(n, 0);
    }
    notify_and_detach_all(w, id, EntityEvent { event: event::TARGET_DIE, kind: 0, data: [id as i32, 0, 0] });
    w.unregister_from_logic_update(id);
}

/// `CPileMgr::GetNearestPile(first, x, y)`: along an offer list from `first`, the pile with goods
/// not all reserved nearest to (x, y); production piles count half the distance.
pub fn get_nearest_pile(w: &World, first: u16, x: i32, y: i32) -> u16 {
    let (mut best, mut best_d) = (0, u32::MAX);
    let mut id = first;
    while let Some(p) = data(w, id) {
        if p.amount != p.leaving {
            let (px, py) = pos(w, id);
            let mut d = crate::spiral::grid_distance(x - px, y - py);
            if p.role == role::PRODUCTION {
                d /= 2; // CProductionPileRole::ReassessDistance
            }
            if (d as u32) < best_d {
                best_d = d as u32;
                best = id;
            }
        }
        id = w.entities.by_index(id as u32).map_or(0, |e| e.next);
    }
    best
}

/// `CPileMgr::SearchSpaceForGoods(x, y, good, amount)`: fill free anywhere piles of the good
/// along the spiral; 50 steps behind, drop the rest on the first free node.
pub fn search_space_for_goods(w: &mut World, x: i32, y: i32, good: u8, amount: u8) {
    if !w.grid.in_world(x, y) {
        return;
    }
    let mut left = amount.min(MAX_AMOUNT) as i32;
    let mut i = 0usize;
    while left > 0 && i <= 9999 {
        let (sx, sy) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
        if let Some(n) = w.grid.checked_index(sx, sy) {
            let p = w.layers.deco_object[n];
            if p != 0 && data(w, p).is_some() {
                let owner = w.owner_of(n);
                let g = if owner != 0 { convert_good_type(good, w.race_of(owner)) } else { good };
                if let Some(d) = data(w, p)
                    && d.good == g
                    && d.role == role::ANYWHERE
                    && (d.amount + d.coming) < MAX_AMOUNT
                {
                    let space = (MAX_AMOUNT - d.amount - d.coming) as i32;
                    let k = space.min(left);
                    increase_unforeseen(w, p, k);
                    left -= k;
                }
            }
        }
        if left > 0 && i >= 50 {
            let (tx, ty) = (x + crate::spiral::dx(i - 50), y + crate::spiral::dy(i - 50));
            if let Some(n) = w.grid.checked_index(tx, ty)
                && w.layers.flags[n] & 0x39 == 0
                && w.layers.deco_object[n] == 0
                && w.layers.map_object[n] == 0
                && w.tiling.sector_of(n) != 0
            {
                let owner = w.owner_of(n);
                let g = if owner != 0 { convert_good_type(good, w.race_of(owner)) } else { good };
                add_pile(w, tx, ty, g, left as u8, role::ANYWHERE);
                left = 0;
            }
        }
        i += 1;
    }
}

// ---- CPile ----

/// `CPile::AdjustStatistic(delta, eco)` (release 0x56fe80): goods count of the eco-sector (the
/// node's when `eco` is 0; goods 1..=42, clamped at 0), then the eco-sector owner's goods
/// statistic ([`crate::statistic::adjust_goods`]).
fn adjust_statistic(w: &mut World, id: u16, delta: i32, eco: u16) {
    if delta == 0 {
        return;
    }
    let e = if eco == 0 { node_eco(w, id) } else { eco };
    if e != 0 {
        let good = data(w, id).map_or(0, |p| p.good);
        eco::change_number_of_goods_pile(w, e, good, delta);
        let owner = w.economy.owner(e);
        crate::statistic::adjust_goods(w, owner, good, delta);
    }
}

/// `CPile::Offer(_, eco)` (`OfferCompletePileIfPossible`): into the eco-sector's offer list of its
/// good (the node's eco-sector when `eco` is 0). The original asserts the pile is not offered yet;
/// here a second offer is ignored.
pub fn offer(w: &mut World, id: u16, eco: u16) {
    let e = if eco == 0 { node_eco(w, id) } else { eco };
    if e == 0 || in_offer_list(w, id) {
        return;
    }
    let good = data(w, id).map_or(0, |p| p.good);
    eco::set_good_offer(w, e, good, id);
}

/// `CPile::CancelOffer(_, eco)`.
pub fn cancel_offer(w: &mut World, id: u16, eco: u16) {
    let e = if eco == 0 { node_eco(w, id) } else { eco };
    if e != 0 && in_offer_list(w, id) {
        eco::get_pile_out_of_offer(w, e, id);
    }
}

/// `CPile::CancelCompleteOfferIfInOfferList(eco)`.
pub fn cancel_complete_offer_if_in_offer_list(w: &mut World, id: u16, eco: u16) {
    if in_offer_list(w, id) {
        cancel_offer(w, id, eco);
    }
}

/// `CPile::IncreaseUnforeseen(n)`: add up to the free space; returns how many.
pub fn increase_unforeseen(w: &mut World, id: u16, n: i32) -> i32 {
    let Some(p) = data(w, id) else { return 0 };
    if n <= 0 {
        return 0;
    }
    let space = MAX_AMOUNT as i32 - p.amount as i32 - p.coming as i32;
    if space <= 0 {
        return 0;
    }
    let n = n.min(space);
    if let Some(p) = data_mut(w, id) {
        p.amount += n as u8;
    }
    adjust_statistic(w, id, n, 0);
    role_increase(w, id, n);
    n
}

/// `CPile::DecreaseUnforeseen(n)`: take up to the available goods.
pub fn decrease_unforeseen(w: &mut World, id: u16, n: i32) {
    if n <= 0 {
        return;
    }
    let avail = available(w, id);
    if avail <= 0 {
        return;
    }
    let n = n.min(avail);
    if let Some(p) = data_mut(w, id) {
        p.amount -= n as u8;
    }
    adjust_statistic(w, id, -n, 0);
    role_decrease(w, id);
    if data(w, id).is_some_and(|p| p.amount == 0) {
        if let Some(p) = data_mut(w, id) {
            p.leaving = 0;
        }
        role_empty(w, id);
    }
}

/// `CPile::IncreaseEx(n)`: a carrier delivered (limited by space and goods coming).
fn increase_ex(w: &mut World, id: u16, n: i32) {
    let Some(p) = data_mut(w, id) else { return };
    let mut space = MAX_AMOUNT as i32 - p.amount as i32;
    if space < 0 {
        space = 0;
        p.amount = MAX_AMOUNT;
    }
    let n = n.min(space).min(p.coming as i32);
    if n <= 0 {
        return;
    }
    p.amount += n as u8;
    p.coming -= n as u8;
    adjust_statistic(w, id, n, 0);
    role_increase(w, id, n);
}

/// `CPile::DecreaseEx(n)`: a carrier took reserved goods.
fn decrease_ex(w: &mut World, id: u16, n: i32) {
    let Some(p) = data_mut(w, id) else { return };
    let n = n.min(p.amount as i32).min(p.leaving as i32).max(0);
    p.amount -= n as u8;
    p.leaving -= n as u8;
    adjust_statistic(w, id, -n, 0);
    role_decrease(w, id);
    if data(w, id).is_some_and(|p| p.amount == 0) {
        if let Some(p) = data_mut(w, id) {
            p.leaving = 0;
        }
        role_empty(w, id);
    }
}

fn inc_amount_leaving(w: &mut World, id: u16, n: i32) {
    for _ in 0..n {
        if let Some(p) = data_mut(w, id) {
            p.leaving = p.leaving.saturating_add(1);
        }
        role_inc_leaving(w, id);
    }
}

fn dec_amount_leaving(w: &mut World, id: u16, n: i32) {
    let n = n.min(data(w, id).map_or(0, |p| p.leaving as i32));
    for _ in 0..n {
        if let Some(p) = data_mut(w, id) {
            p.leaving -= 1;
        }
        role_dec_leaving(w, id);
    }
}

fn inc_amount_coming(w: &mut World, id: u16) {
    if let Some(p) = data_mut(w, id) {
        p.coming = p.coming.saturating_add(1);
        // CTradePileRole::IncAmountComing.
        if p.role == role::TRADE && p.expected > 0 {
            p.expected -= 1;
        }
    }
}

fn dec_amount_coming(w: &mut World, id: u16) {
    if let Some(p) = data_mut(w, id) {
        p.coming = p.coming.saturating_sub(1);
    }
    trade_dec_coming(w, id);
}

/// `CTradePileRole::DecAmountComing`: an expected good is expected again (up to the room).
fn trade_dec_coming(w: &mut World, id: u16) {
    if let Some(p) = data_mut(w, id)
        && p.role == role::TRADE
        && p.expected < MAX_AMOUNT as i32 - p.amount as i32 - p.coming as i32
    {
        p.expected += 1;
    }
}

/// `CPile::Detach(observer)`: drop the observer and its reservation.
pub fn detach(w: &mut World, id: u16, observer: u16) {
    let d = observers_detach(w, id, observer);
    if d > 0 {
        dec_amount_coming(w, id);
    } else if d < 0 {
        dec_amount_leaving(w, id, -d);
    }
}

/// `CPile::ChangeAmountAndDetach(observer)`: the observer took its goods (or delivered one).
pub fn change_amount_and_detach(w: &mut World, id: u16, observer: u16) {
    let d = observers_detach(w, id, observer);
    if d > 0 {
        increase_ex(w, id, 1);
    } else if d < 0 {
        decrease_ex(w, id, -d);
    }
}

/// `CPile::AttachAndIncAmountLeaving(observer, n, target)` (target 2..4).
pub fn attach_and_inc_amount_leaving(w: &mut World, id: u16, observer: u16, n: i32, target: u8) {
    observers_attach(w, id, target, observer, -(n as i8));
    inc_amount_leaving(w, id, n);
}

/// `CPile::AttachAndIncAmountComing(observer)` (target 1).
pub fn attach_and_inc_amount_coming(w: &mut World, id: u16, observer: u16) {
    observers_attach(w, id, target::PILE_DST, observer, 1);
    inc_amount_coming(w, id);
}

/// `CPile::NotifyTargetDieAndDetachAllObservers`: observers get `TARGET_DIE`, reservations end.
pub fn notify_target_die_and_detach_all(w: &mut World, id: u16) {
    notify_and_detach_all(w, id, EntityEvent { event: event::TARGET_DIE, kind: 0, data: [id as i32, 0, 0] });
    if data(w, id).is_some_and(|p| p.coming != 0) {
        if let Some(p) = data_mut(w, id) {
            p.coming = 0;
        }
        trade_dec_coming(w, id);
    }
    if data(w, id).is_some_and(|p| p.leaving != 0) {
        if let Some(p) = data_mut(w, id) {
            p.leaving = 0;
        }
        role_dec_leaving(w, id);
    }
}

/// `CPile::SetObserverTarget(0, building)`: attach to / detach from the owning building.
pub fn set_building(w: &mut World, id: u16, building: u16) {
    if building != 0 {
        if let Some(p) = data_mut(w, id) {
            p.building = building;
        }
        if let Some(e) = w.entities.by_index_mut(id as u32) {
            e.flags |= flag::ATTACHED;
        }
        return;
    }
    if let Some(e) = w.entities.by_index_mut(id as u32) {
        e.flags &= !flag::ATTACHED;
    }
    let old = data(w, id).map_or(0, |p| p.building);
    if old != 0 {
        notify_target_die_and_detach_all(w, id);
        if let Some(p) = data_mut(w, id) {
            p.building = 0;
        }
        role_subject_die(w, id, old);
    }
}

/// `CPile::Occupied(eco)`: the node's land got an owner — local good, role, count.
pub fn occupied(w: &mut World, id: u16, eco_id: u16) {
    let race = w.race_of(w.economy.owner(eco_id));
    if let Some(p) = data_mut(w, id) {
        p.good = convert_good_type(p.good, race);
    }
    match data(w, id).map(|p| (p.role, p.trade_type, p.amount, p.leaving, p.offer)) {
        Some((role::ANYWHERE, ..)) => offer(w, id, eco_id),
        // CTradePileRole::Occupied.
        Some((role::TRADE, 1, amount, leaving, false)) if amount > leaving => {
            offer(w, id, eco_id);
            set_offer_flag(w, id, true);
        }
        _ => {}
    }
    let amount = data(w, id).map_or(0, |p| p.amount as i32);
    adjust_statistic(w, id, amount, eco_id);
}

/// `CPile::SetFree(eco)`: the node's land lost its owner.
pub fn set_free(w: &mut World, id: u16, eco_id: u16) {
    match data(w, id).map(|p| (p.role, p.trade_type, p.amount, p.leaving, p.offer)) {
        Some((role::ANYWHERE, ..)) => cancel_complete_offer_if_in_offer_list(w, id, eco_id),
        // CTradePileRole::SetFree.
        Some((role::TRADE, 1, amount, leaving, offer_flag)) => {
            if amount == leaving && offer_flag {
                cancel_offer(w, id, 0);
                set_offer_flag(w, id, false);
            }
        }
        Some((role::TRADE, 2, ..)) => {
            cancel_offer(w, id, eco_id);
            set_offer_flag(w, id, false);
        }
        _ => {}
    }
    let amount = data(w, id).map_or(0, |p| p.amount as i32);
    adjust_statistic(w, id, -amount, eco_id);
}

/// `CPile::NotifyChangeEcoSector(old, new)` (same owner): move the count and the offer.
pub fn notify_change_eco_sector(w: &mut World, id: u16, old: u16, new: u16) {
    let amount = data(w, id).map_or(0, |p| p.amount as i32);
    adjust_statistic(w, id, -amount, old);
    adjust_statistic(w, id, amount, new);
    // IPileRole::NotifyChangeEcoSector (trade piles too, then their storage building).
    if in_offer_list(w, id) {
        cancel_offer(w, id, old);
        offer(w, id, new);
    }
    if let Some(b) = data(w, id).filter(|p| p.role == role::TRADE).map(|p| p.building)
        && is_storage(w, b)
    {
        crate::buildings::storage::notify_change_eco_sector(w, b, id, old, new);
    }
}

fn is_storage(w: &World, building: u16) -> bool {
    building != 0
        && w.entities
            .by_index(building as u32)
            .is_some_and(|e| e.obj_type == ObjType::Building && e.kind == crate::buildings::storage::STORAGE_AREA)
}

fn set_offer_flag(w: &mut World, id: u16, on: bool) {
    if let Some(p) = data_mut(w, id) {
        p.offer = on;
    }
}

/// `CPile::SetGoodType` (no recount: used on empty trade piles).
pub fn set_good_type(w: &mut World, id: u16, good: u8) {
    if let Some(p) = data_mut(w, id) {
        p.good = good;
    }
}

/// `CTradePileRole::SetExpectedAmount`.
pub fn set_expected(w: &mut World, id: u16, n: i32) {
    if let Some(p) = data_mut(w, id) {
        p.expected = n;
    }
}

/// `CTradePileRole +0x10`: goods reserved for a foreign transport (role 4).
pub fn set_reserve(w: &mut World, id: u16, n: i32) {
    if let Some(p) = data_mut(w, id) {
        p.reserve = n;
    }
}

/// `CTradePileRole::SetTradeRoleType(t)`: incoming piles (1 / 3) withdraw their offer, a stored
/// pile (2) with goods is offered.
pub fn set_trade_type(w: &mut World, id: u16, t: u8) {
    let Some(p) = data_mut(w, id) else { return };
    if p.trade_type == 4 {
        p.reserve = 0;
    }
    p.trade_type = t;
    let (amount, leaving, offer_flag) = (p.amount, p.leaving, p.offer);
    if (t == 1 || t == 3) && offer_flag && amount > leaving {
        cancel_offer(w, id, 0);
        set_offer_flag(w, id, false);
    }
    if t == 2 && amount != 0 && !offer_flag {
        offer(w, id, 0);
        set_offer_flag(w, id, true);
    }
}

/// `CPile::ChangeRole(role)` (only to anywhere in the original): the building forgets the pile
/// (TODO(port): building vtbl+64), the new role's `Init` runs.
pub fn change_role(w: &mut World, id: u16, new_role: u8) {
    role_init(w, id, new_role);
}

/// `IPileRole` vtbl+0x48 `ChangeGoodType(pile, good)`: `CAnyWherePileRole` (0x56e8b0) and
/// `CTradePileRole` (0x5721a0) change the pile's good ([`change_good_type_unforeseen`]); every
/// other role answers false (0x56e9b0). The trade role then, with an old good (vtbl+0x3c before
/// the change): a storage area's pile (building type 0x22, 0x551260) leaves the eco-sector's
/// list for the old good (0x4d44f0) and joins the new good's (0x4d4480) if the area stores it
/// (+0x180); a marketplace's (0x21) goods expected (+0xc) go back to its goods still to collect
/// for the old good (+0x180) and the pile expects none.
pub fn change_good_type(w: &mut World, id: u16, good: u8) -> bool {
    match role_of(w, id) {
        role::ANYWHERE => change_good_type_unforeseen(w, id, good),
        role::TRADE => {
            let (old, building, expected) = get(w, id).map_or((0, 0, 0), |d| (d.good, d.building, d.expected));
            let changed = change_good_type_unforeseen(w, id, good);
            let kind = w.entities.by_index(building as u32).filter(|e| e.obj_type == ObjType::Building).map_or(0, |e| e.kind);
            if kind == crate::buildings::storage::STORAGE_AREA {
                crate::buildings::storage::pile_good_changed(w, building, id, old, good);
            }
            if kind == crate::buildings::trading::MARKETPLACE && old != 0 && expected != 0 {
                crate::buildings::trading::collect_back(w, building, old, expected);
                set_expected(w, id, 0);
            }
            changed
        }
        _ => false,
    }
}

/// `CPile::ExecuteChangeGoodTypeUnforseen(good)` (0x56fe00): a good 1..42 other than the pile's
/// replaces it — observers told and detached, offer withdrawn, the statistic moved, offered
/// again if the pile offers.
pub fn change_good_type_unforeseen(w: &mut World, id: u16, good: u8) -> bool {
    let Some(p) = data(w, id) else { return false };
    if good == 0 || good >= 0x2b || p.good == good {
        return false;
    }
    notify_target_die_and_detach_all(w, id);
    let amount = data(w, id).map_or(0, |p| p.amount as i32);
    cancel_complete_offer_if_in_offer_list(w, id, 0);
    adjust_statistic(w, id, -amount, 0);
    if let Some(p) = data_mut(w, id) {
        p.good = good;
    }
    adjust_statistic(w, id, amount, 0);
    if data(w, id).is_some_and(|p| p.offer) {
        offer(w, id, 0);
    }
    true
}

/// `IAnimatedEntity::SetEvent` for a pile (the building's started / stopped notices).
pub fn set_event(w: &mut World, id: u16, ev: EntityEvent) {
    if flags(w, id) & flag::REGISTERED == 0 {
        w.register_for_logic_update(id, 1);
    }
    if let Some(p) = data_mut(w, id) {
        p.events.push(ev);
    }
}

/// `CPile::LogicUpdate`: queued events (`ConvertEventIntoGoal` always asks to continue), then the
/// role's update.
pub fn logic_update(w: &mut World, id: u16) {
    while let Some(ev) = data_mut(w, id).and_then(|p| (!p.events.is_empty()).then(|| p.events.remove(0))) {
        match ev.event {
            event::SUBJECT_STOPPED => role_subject_stopped(w, id),
            event::SUBJECT_STARTED => role_subject_started(w, id),
            _ => {} // TARGET_DIE: "Illegal event" (debug report)
        }
    }
    let Some(p) = data(w, id) else { return };
    match p.role {
        role::PRODUCTION => {
            if available(w, id) >= 2 {
                let e = node_eco(w, id);
                eco::request_space(w, e, id);
            }
            w.register_for_logic_update(id, 31);
        }
        role::DELIVER => {
            if p.offer {
                return;
            }
            if p.amount + p.coming >= MAX_AMOUNT {
                w.register_for_logic_update(id, 31);
                return;
            }
            let (good, building) = (p.good, p.building);
            let e = node_eco(w, id);
            if e == 0 {
                return;
            }
            eco::request_good(w, e, building, good);
            w.register_for_logic_update(id, 31);
        }
        role::ANYWHERE => {
            // CPile::RequestSpaceIfPossible.
            let e = node_eco(w, id);
            if e != 0 && available(w, id) > 0 {
                eco::request_space(w, e, id);
            }
            w.register_for_logic_update(id, 31);
        }
        role::TRADE => trade_logic_update(w, id),
        _ => {}
    }
}

/// `CTradePileRole::LogicUpdate`: incoming piles (1 / 3) of a working building ask for their
/// expected goods; a stored pile (2) of a working non-storage building (a marketplace / port)
/// sends its goods to storage.
fn trade_logic_update(w: &mut World, id: u16) {
    let Some(p) = data(w, id) else { return };
    let (t, offer_flag, expected, good, building) = (p.trade_type, p.offer, p.expected, p.good, p.building);
    let working = |w: &World, b: u16| w.entities.by_index(b as u32).is_some_and(|e| e.flags & 0x1000 != 0);
    if (t == 1 || t == 3) && !offer_flag {
        if expected != 0 && working(w, building) {
            let e = node_eco(w, id);
            eco::request_good(w, e, building, good);
        }
        w.register_for_logic_update(id, 31);
    }
    if t != 2 {
        return;
    }
    if building == 0 || is_storage(w, building) || !working(w, building) || available(w, id) < 1 {
        w.register_for_logic_update(id, 31);
        return;
    }
    let e = node_eco(w, id);
    eco::request_space(w, e, id);
    w.register_for_logic_update(id, 31);
}

// ---- roles ----

/// `IPileRole::Init` of the role (vtbl+36).
fn role_init(w: &mut World, id: u16, r: u8) {
    let set = |w: &mut World, offer: bool| {
        if let Some(p) = data_mut(w, id) {
            p.role = r;
            p.offer = offer;
        }
    };
    match r {
        role::PRODUCTION => {
            set(w, true);
            offer(w, id, 0);
            w.register_for_logic_update(id, 31);
        }
        role::DELIVER => set(w, true),
        role::STORAGE => {
            set(w, true);
            offer(w, id, 0);
        }
        role::ANYWHERE => {
            if let Some(p) = data_mut(w, id) {
                p.events.clear();
            }
            if let Some(e) = w.entities.by_index_mut(id as u32) {
                e.flags |= flag::VISIBLE;
            }
            set(w, true);
            if data(w, id).is_some_and(|p| p.amount > 0) {
                offer(w, id, 0);
                w.register_for_logic_update(id, 31);
            } else {
                delete_pile(w, id);
            }
        }
        role::BUILDING_SITE => set(w, false),
        _ => {
            // CTradePileRole::Init: out of any list, no offer, free.
            set(w, false);
            if let Some(e) = w.entities.by_index_mut(id as u32) {
                e.next = 0;
                e.prev = 0;
            }
            if let Some(p) = data_mut(w, id) {
                p.trade_type = 0;
            }
        }
    }
}

/// The script's `GOODARRIVE` (17): the building's type, the good, 1.
fn goods_arrive_event(w: &mut World, building: u16, good: u8) {
    let kind = w.entities.by_index(building as u32).map_or(0, |e| e.kind as i32);
    w.script_events.send(crate::event::ScriptEvents::GOODARRIVE, [kind, good as i32, 1]);
}

fn role_of(w: &World, id: u16) -> u8 {
    data(w, id).map_or(role::STORAGE, |p| p.role)
}

/// vtbl+8 `Increase(n)`.
fn role_increase(w: &mut World, id: u16, n: i32) {
    match role_of(w, id) {
        role::DELIVER | role::BUILDING_SITE => {
            let (building, good) = data(w, id).map_or((0, 0), |p| (p.building, p.good));
            if building != 0 {
                // `CDeliverPileRole` 0x56ef30 / `CBuildingSitePileRole` 0x56ec10: the script's
                // `GOODARRIVE` (building type, good, 1), then the building's `GoodArrive`.
                goods_arrive_event(w, building, good);
                crate::buildings::good_arrive(w, building, good);
            }
        }
        role::TRADE => {
            let Some(p) = data(w, id) else { return };
            let (t, good, building, amount, leaving, offer_flag) = (p.trade_type, p.good, p.building, p.amount, p.leaving, p.offer);
            if (t == 1 || t == 3) && building != 0 {
                goods_arrive_event(w, building, good);
                crate::buildings::good_arrive(w, building, good);
            }
            // Type 2 (`CTradePileRole` 0x571db0): `GOODARRIVE`, then offered when it had nothing
            // available.
            if t == 2 {
                goods_arrive_event(w, building, good);
            }
            if t == 2 && amount as i32 - n == leaving as i32 && !offer_flag {
                offer(w, id, 0);
                set_offer_flag(w, id, true);
            }
        }
        // IPileRole::Increase (0x5713c0); `CProductionPileRole` (0x571810) then counts the goods
        // made for the owner of the pile's tile.
        r => {
            if !in_offer_list(w, id) && data(w, id).is_some_and(|p| p.offer) {
                offer(w, id, 0);
            }
            if r == role::PRODUCTION
                && n > 0
                && let Some(e) = w.entities.by_index(id as u32)
                && let Some(node) = w.grid.checked_index(e.x(), e.y())
            {
                let owner = w.tiling.tile(w.tiling.ids(false)[node]).owner();
                let good = data(w, id).map_or(0, |p| p.good);
                crate::statistic::goods_made(w, owner, good, n);
            }
        }
    }
}

/// vtbl+12 `Decrease`: no goods left to offer → out of the list.
fn role_decrease(w: &mut World, id: u16) {
    if role_of(w, id) == role::TRADE {
        // CTradePileRole::Decrease 0x571eb0 (its pile vtbl+0x3c call is a getter, result unused).
        let Some(p) = data(w, id) else { return };
        let (t, amount, expected, coming, offer_flag) = (p.trade_type, p.amount, p.expected, p.coming, p.offer);
        if t == 1 && amount == 0 && expected == 0 && coming == 0 {
            if offer_flag {
                cancel_offer(w, id, 0);
                set_offer_flag(w, id, false);
            }
            if let Some(p) = data_mut(w, id) {
                p.trade_type = 0;
                p.good = 0;
            }
        }
        if let Some(p) = data_mut(w, id)
            && p.trade_type == 4
            && p.amount == 0
        {
            p.reserve = 0;
            p.trade_type = 0;
            p.good = 0;
        }
        return;
    }
    if in_offer_list(w, id) && available(w, id) <= 0 {
        cancel_offer(w, id, 0);
    }
}

/// vtbl+16 `Empty`.
fn role_empty(w: &mut World, id: u16) {
    match role_of(w, id) {
        role::ANYWHERE => delete_pile(w, id),
        role::TRADE => {
            // CTradePileRole::Empty: an emptied storage pile is free again.
            let Some(p) = data(w, id) else { return };
            if p.expected != 0 || p.coming != 0 {
                return;
            }
            let (offer_flag, building) = (p.offer, p.building);
            if offer_flag {
                cancel_offer(w, id, 0);
                set_offer_flag(w, id, false);
            }
            if let Some(p) = data_mut(w, id) {
                p.trade_type = 0;
                p.good = 0;
            }
            if is_storage(w, building) {
                let e = crate::buildings::storage::eco_of_building(w, building);
                eco::register_free_storage_pile(w, e, building, id);
            }
        }
        _ => {}
    }
}

/// vtbl+20 `IncAmoutLeaving`.
fn role_inc_leaving(w: &mut World, id: u16) {
    if role_of(w, id) == role::TRADE {
        // CTradePileRole::IncAmoutLeaving: a stored pile fully reserved leaves the list.
        if let Some(p) = data(w, id)
            && p.trade_type == 2
            && p.amount == p.leaving
            && p.offer
        {
            cancel_offer(w, id, 0);
            set_offer_flag(w, id, false);
        }
        return;
    }
    if in_offer_list(w, id) && available(w, id) <= 0 {
        cancel_offer(w, id, 0);
    }
}

/// vtbl+24 `DecAmountLeaving`.
fn role_dec_leaving(w: &mut World, id: u16) {
    if role_of(w, id) == role::TRADE {
        // CTradePileRole::DecAmountLeaving (release 0x572520): offered when amount == leaving
        // after the decrement — copied as found.
        if let Some(p) = data(w, id)
            && p.trade_type == 2
            && p.amount == p.leaving
            && !p.offer
        {
            offer(w, id, 0);
            set_offer_flag(w, id, true);
        }
        return;
    }
    if !in_offer_list(w, id) && data(w, id).is_some_and(|p| p.offer) {
        offer(w, id, 0);
    }
}

/// vtbl+60 `SubjectDie(building)`: the building is gone; the pile becomes a free pile.
fn role_subject_die(w: &mut World, id: u16, building: u16) {
    match role_of(w, id) {
        role::ANYWHERE => {}
        role::TRADE => {
            if is_storage(w, building) {
                crate::buildings::storage::unregister_pile(w, building, id);
            }
            if data(w, id).is_some_and(|p| p.offer && p.amount > 0) {
                cancel_offer(w, id, 0);
                set_offer_flag(w, id, false);
            }
            change_role(w, id, role::ANYWHERE);
        }
        _ => {
            cancel_complete_offer_if_in_offer_list(w, id, 0);
            change_role(w, id, role::ANYWHERE);
        }
    }
}

/// vtbl+64 `SubjectStopped`: delivery / site piles offer their goods again.
fn role_subject_stopped(w: &mut World, id: u16) {
    if role_of(w, id) == role::TRADE {
        notify_target_die_and_detach_all(w, id);
        if let Some(p) = data(w, id)
            && p.trade_type == 1
            && p.amount > 0
            && !p.offer
        {
            offer(w, id, 0);
            set_offer_flag(w, id, true);
        }
        return;
    }
    if matches!(role_of(w, id), role::DELIVER | role::BUILDING_SITE) {
        notify_target_die_and_detach_all(w, id);
        if let Some(p) = data_mut(w, id) {
            p.offer = true;
        }
        offer(w, id, 0);
    }
}

/// vtbl+68 `SubjectStarted`: delivery / site piles keep their goods (and deliver piles start
/// asking for more).
fn role_subject_started(w: &mut World, id: u16) {
    let r = role_of(w, id);
    if r == role::TRADE {
        notify_target_die_and_detach_all(w, id);
        if data(w, id).is_some_and(|p| p.trade_type == 1 && p.offer) {
            cancel_offer(w, id, 0);
            set_offer_flag(w, id, false);
        }
        w.register_for_logic_update(id, 1);
        return;
    }
    if matches!(r, role::DELIVER | role::BUILDING_SITE) {
        notify_target_die_and_detach_all(w, id);
        cancel_complete_offer_if_in_offer_list(w, id, 0);
        if let Some(p) = data_mut(w, id) {
            p.offer = false;
        }
        if r == role::DELIVER {
            w.register_for_logic_update(id, 31);
        }
    }
}

// ---- CPileObserverList ----

/// `CPileObserverList::Attach(target, pile, observer, delta)`: the observer's target is set to
/// the pile.
fn observers_attach(w: &mut World, id: u16, tgt: u8, observer: u16, delta: i8) {
    let Some(p) = data_mut(w, id) else { return };
    if p.observers.len() >= 8 || p.observers.iter().any(|o| o.entity == observer) {
        return; // asserted in the original
    }
    p.observers.push(PileObserver { entity: observer, delta, target: tgt });
    set_observer_target(w, observer, tgt, id);
}

/// `CPileObserverList::Detach(observer)`: swap-remove; returns its delta (0 if not listed).
fn observers_detach(w: &mut World, id: u16, observer: u16) -> i32 {
    let Some(p) = data_mut(w, id) else { return 0 };
    let Some(i) = p.observers.iter().position(|o| o.entity == observer) else { return 0 };
    let o = p.observers.swap_remove(i);
    set_observer_target(w, observer, o.target, 0);
    o.delta as i32
}

/// `NotifyAndDetachAllObservers(event)`.
fn notify_and_detach_all(w: &mut World, id: u16, ev: EntityEvent) {
    let list = data(w, id).map(|p| p.observers.clone()).unwrap_or_default();
    for o in &list {
        notify_entity(w, o.entity, ev);
    }
    for o in &list {
        set_observer_target(w, o.entity, o.target, 0);
    }
    if let Some(p) = data_mut(w, id) {
        p.observers.clear();
    }
}

/// `IEntity::Notify(event)` (vtbl+80) on an observer: settlers queue it.
fn notify_entity(w: &mut World, observer: u16, ev: EntityEvent) {
    match w.entities.by_index(observer as u32).map(|e| e.obj_type) {
        Some(ObjType::Settler) => crate::jobs::core::set_event(w, observer, ev),
        Some(_) => w.events.push(crate::event::SimEvent::Unported { what: "IEntity::Notify (non-settler pile observer)", entity: observer }),
        None => {}
    }
}

/// `IEntity::SetObserverTarget(target, pile)` (vtbl+68) on an observer. Settlers keep pile
/// targets in their role (`ISettlerRole` +20 source, carrier +46 destination, +48 / +50 second
/// and third source).
fn set_observer_target(w: &mut World, observer: u16, tgt: u8, pile: u16) {
    let Some(e) = w.entities.by_index_mut(observer as u32) else { return };
    match &mut e.data {
        EntityData::Settler(s) => match tgt {
            target::PILE_DST => s.job.carrier.dest_pile = pile,
            target::PILE_SRC => s.job.source_pile = pile,
            target::PILE_SRC_2 => s.job.carrier.source_pile2 = pile,
            target::PILE_SRC_3 => s.job.carrier.source_pile3 = pile,
            _ => {}
        },
        _ => w.events.push(crate::event::SimEvent::Unported { what: "IEntity::SetObserverTarget (non-settler)", entity: observer }),
    }
}
