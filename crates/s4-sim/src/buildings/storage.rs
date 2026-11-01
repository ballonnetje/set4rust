//! `CStorageBuildingRole` (MapObjects/Building/CStorageBuildingRole.cpp; release vtable
//! 0x1059508, `Init` 0x550420): the storage area. Its piles are trade-role piles (role 4) from
//! the building info; the player chooses which goods it takes (command 5044). The eco-sector
//! keeps the registrations `GetNearestStoragePile` searches: the goods taken, the free piles and
//! the piles holding a good. (`CheckGoodToStore` and the full-pile stacks are never called in
//! the original and are left out.)

use crate::World;
use crate::eco;
use crate::entity::flag;
use crate::hash::StateHasher;
use crate::pile;

/// `HOUSE_KIND_STORAGE` building type.
pub const STORAGE_AREA: u16 = 34;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// +384 goods the player chose to store.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub store: [bool; eco::GOODS],
    /// +428 the piles, in building-info order.
    pub piles: Vec<u16>,
}

impl Default for State {
    fn default() -> Self {
        State { store: [false; eco::GOODS], piles: Vec::new() }
    }
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.store.iter().for_each(|&b| h.bool(b));
        h.u16s(&self.piles);
    }
}

fn state(world: &World, id: u16) -> Option<&State> {
    match &super::role(world, id)?.kind {
        super::RoleKind::Storage(s) => Some(s),
        _ => None,
    }
}

fn state_mut(world: &mut World, id: u16) -> Option<&mut State> {
    match &mut super::role_mut(world, id)?.kind {
        super::RoleKind::Storage(s) => Some(s),
        _ => None,
    }
}

/// `CBuilding::EcoSectorId`: the eco-sector at the building's flag.
pub fn eco_of_building(world: &World, id: u16) -> u16 {
    match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(crate::entity::EntityData::Building(b)) => {
            world.grid.checked_index(b.ensign.x(), b.ensign.y()).map_or(0, |n| eco::eco_of(world, n))
        }
        _ => 0,
    }
}

/// 0x551060 `(good)` (the eco-sector AI's manager slot 0x144): over the storage's piles, a free
/// one (trade role 0) counts 8, one holding or awaiting `good` (role 1 or 2) `8 − coming −
/// amount`. Not a storage: 0 [inf: the release would read another role's fields].
pub fn free_space(world: &World, id: u16, good: u8) -> i32 {
    let Some(s) = state(world, id) else { return 0 };
    let mut n = 0;
    for &p in &s.piles {
        let Some(d) = pile::get(world, p) else { continue };
        match d.trade_type {
            0 => n += 8,
            1 | 2 if d.good == good => n += 8 - d.coming as i32 - d.amount as i32,
            _ => {}
        }
    }
    n
}

/// `CStorageBuildingRole::Init`: the trade piles from the building info (amount 0; patch
/// piles invisible), registered at the eco-sector; logic update every 14 ticks. `InitCommon`
/// (0x550420 calls it first) runs in [`super::init`].
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, race) = (e.x(), e.y(), e.race);
    let type_ex = match &e.data {
        crate::entity::EntityData::Building(b) => b.type_ex,
        _ => return,
    };
    let rules = world.rules.building(race, type_ex).piles.clone();
    if let Some(s) = state_mut(world, id) {
        *s = State::default();
    }
    for p in rules {
        let Some(pid) = pile::add_pile_ex(world, x + p.x as i32, y + p.y as i32, p.good, 0, p.kind, p.patch, p.appearance, p.pixel)
        else {
            continue;
        };
        super::attach(world, id, pid);
        if p.appearance == 1
            && let Some(e) = world.entities.by_index_mut(pid as u32)
        {
            e.flags &= !flag::VISIBLE;
        }
        if let Some(s) = state_mut(world, id) {
            s.piles.push(pid);
        }
    }
    let e = eco_of_building(world, id);
    register_at_eco(world, id, e);
    world.register_for_logic_update(id, 14);
}

/// `CStorageBuildingRole::LogicUpdate` (the dialog refresh is UI).
pub fn logic_update(world: &mut World, id: u16) {
    world.register_for_logic_update(id, 14);
}

fn piles(world: &World, id: u16) -> Vec<u16> {
    state(world, id).map(|s| s.piles.clone()).unwrap_or_default()
}

/// `CStorageBuildingRole::GetPileIdWithGood(good)`: a pile of the good with room.
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    if good as usize >= eco::GOODS {
        return 0;
    }
    for p in piles(world, id) {
        if let Some(d) = pile::get(world, p)
            && d.good == good
            && d.amount < 8
            && d.coming < 8 - d.amount
        {
            return p;
        }
    }
    0
}

/// `CStorageBuildingRole::GetNextFreePile`: the first pile with trade role 0.
pub fn next_free_pile(world: &World, id: u16) -> u16 {
    piles(world, id).into_iter().find(|&p| pile::get(world, p).is_some_and(|d| d.trade_type == 0)).unwrap_or(0)
}

/// `CStorageBuildingRole::ReserveFreePile(good, pile)`: the free pile becomes a pile of `good`
/// (trade role 2).
pub fn reserve_free_pile(world: &mut World, id: u16, good: u8, p: u16) -> bool {
    if !piles(world, id).contains(&p) || pile::get(world, p).is_none_or(|d| d.trade_type != 0) {
        return false;
    }
    pile::set_trade_type(world, p, 2);
    pile::set_good_type(world, p, good);
    true
}

/// 0x551260 `(pile, old, new)`, a pile's good changed (`CTradePileRole::ChangeGoodType`): with an
/// old good the pile leaves the eco-sector's list for it (0x4d44f0) and joins the new good's
/// (0x4d4480) if the area stores that good (+0x180).
pub fn pile_good_changed(world: &mut World, id: u16, p: u16, old: u8, new: u8) {
    if old == 0 {
        return;
    }
    let e = eco_of_building(world, id);
    eco::unregister_good_storage_pile(world, e, id, p, old);
    if state(world, id).is_some_and(|s| s.store.get(new as usize).copied().unwrap_or(false)) {
        eco::register_good_storage_pile(world, e, id, p, new);
    }
}

/// `CStorageBuildingRole::RegisterPilesAndStorageAtEcosector(eco)`.
pub fn register_at_eco(world: &mut World, id: u16, e: u16) {
    for p in piles(world, id) {
        let Some(d) = pile::get(world, p) else { continue };
        let (t, good) = (d.trade_type, d.good);
        if t == 0 {
            eco::register_free_storage_pile(world, e, id, p);
        } else if t == 1 || t == 2 {
            eco::register_good_storage_pile(world, e, id, p, good);
        }
    }
    let store = state(world, id).map(|s| s.store).unwrap_or([false; eco::GOODS]);
    for (g, &on) in store.iter().enumerate().skip(1) {
        if on {
            eco::register_good_storage(world, e, id, g as u8);
        }
    }
}

/// `CStorageBuildingRole::SwitchGood(good, on)` (command 5044): store the good or not. Turning
/// it off drops the reservations on its piles and frees its empty piles.
pub fn switch_good(world: &mut World, id: u16, good: u8, on: bool) {
    if good == 0 || good as usize >= eco::GOODS {
        return;
    }
    let e = eco_of_building(world, id);
    if on {
        if world.entities.by_index(id as u32).is_some_and(|b| b.flags & 0x1000 != 0) {
            eco::register_good_storage(world, e, id, good);
        }
    } else {
        eco::unregister_good_storage(world, e, id, good);
        for p in piles(world, id) {
            if pile::get(world, p).is_some_and(|d| d.good == good) {
                pile::notify_target_die_and_detach_all(world, p);
            }
        }
        while let Some(p) = reserved_pile(world, id, good) {
            check_empty_pile(world, id, p);
        }
    }
    if let Some(s) = state_mut(world, id) {
        s.store[good as usize] = on;
    }
}

/// `CStorageBuildingRole::Switch` (vtbl+0x20, 0x5506b0; command 5003): the base switch
/// ([`super::work::switch`]: flag 0x1000, events 8 / 7), the eco-sector at the ensign drops the
/// building's requests (0x4d5b70); working, every good it stores is registered again (if not
/// yet, 0x4d4960 / 0x4d6cd0), stopped, they are unregistered (the last registration fills the
/// hole).
pub fn switch(world: &mut World, id: u16) {
    super::work::switch(world, id);
    let e = eco_of_building(world, id);
    eco::clean_up_building_need(world, e, id);
    let working = world.entities.by_index(id as u32).is_some_and(|b| b.flags & 0x1000 != 0);
    let store = state(world, id).map(|s| s.store).unwrap_or([false; eco::GOODS]);
    for (g, &on) in store.iter().enumerate().skip(1) {
        if !on {
            continue;
        }
        if working {
            eco::register_good_storage(world, e, id, g as u8);
        } else {
            eco::unregister_good_storage(world, e, id, g as u8);
        }
    }
}

/// `CStorageBuildingRole::GetReservedPile(good)`: an empty pile still set to the good.
fn reserved_pile(world: &World, id: u16, good: u8) -> Option<u16> {
    piles(world, id).into_iter().find(|&p| pile::get(world, p).is_some_and(|d| d.good == good && d.amount == 0))
}

/// `CStorageBuildingRole::CheckEmptyPile(pile)`: an empty good pile becomes free again.
fn check_empty_pile(world: &mut World, id: u16, p: u16) {
    let Some(d) = pile::get(world, p) else { return };
    if d.amount != 0 {
        return;
    }
    let good = d.good;
    let e = eco_of_building(world, id);
    eco::unregister_good_storage_pile(world, e, id, p, good);
    eco::register_free_storage_pile(world, e, id, p);
    pile::set_good_type(world, p, 0);
    pile::set_expected(world, p, 0);
    pile::set_trade_type(world, p, 0);
}

/// `CStorageBuildingRole` vtable +0x2c (release 0x550190, from `CBuilding::CrushBuilding`): the
/// eco-sector at the ensign drops the building's goods registrations (goods 1..=42).
pub fn crush(world: &mut World, id: u16) {
    let e = eco_of_building(world, id);
    for g in 1..eco::GOODS as u8 {
        eco::unregister_good_storage(world, e, id, g);
    }
}

/// `CStorageBuildingRole::UnregisterPile(pile)` (the pile lost its building).
pub fn unregister_pile(world: &mut World, id: u16, p: u16) {
    if !piles(world, id).contains(&p) {
        return;
    }
    let e = eco_of_building(world, id);
    let Some(d) = pile::get(world, p) else { return };
    match d.trade_type {
        0 => {
            eco::unregister_free_storage_pile(world, e, id, p);
        }
        1 | 2 => {
            let good = d.good;
            eco::unregister_good_storage_pile(world, e, id, p, good);
        }
        _ => {}
    }
}

/// `CStorageBuildingRole::NotifyChangeEcoSector(pile, old, new)`: the pile's and the goods'
/// registrations move with it.
pub fn notify_change_eco_sector(world: &mut World, id: u16, p: u16, old: u16, new: u16) {
    if old == new {
        return;
    }
    let Some(d) = pile::get(world, p) else { return };
    let (t, good) = (d.trade_type, d.good);
    if t == 0 {
        eco::unregister_free_storage_pile(world, old, id, p);
        eco::register_free_storage_pile(world, new, id, p);
    } else if t == 1 || t == 2 {
        eco::unregister_good_storage_pile(world, old, id, p, good);
        eco::register_good_storage_pile(world, new, id, p, good);
    }
    let store = state(world, id).map(|s| s.store).unwrap_or([false; eco::GOODS]);
    for (g, &on) in store.iter().enumerate().skip(1) {
        if on {
            eco::unregister_good_storage(world, old, id, g as u8);
            eco::register_good_storage(world, new, id, g as u8);
        }
    }
}
