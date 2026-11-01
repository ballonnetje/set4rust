//! `CBarrackRole` (house kind 11; release vtable 0x1058484): the barracks turns carriers into
//! soldiers. Own reverse engineering of the shipped exe (release VAs).
//!
//! * `Init` (0x534870): `InitCommon`, the rotation byte +0x17c = 2, one pile per buildingInfo
//!   `pile` (info +0x39 count, +0x3c + 16·i: x, y, good, type) attached to the building and kept
//!   in the vector +0x180; event 8 to the observers when the building works (flag 0x1000);
//!   logic update in 31.
//! * `LogicUpdate` (0x534ac0), while the building works: of the race's ten recipes (.data
//!   0x1164660, 24 bytes each: soldier type, weapon good, second good, gold bars, a value not
//!   read here, 0), the ones with a soldier order in the eco-sector at the ensign (+0x33c through
//!   the wish table 0x1164b10) whose goods lie on the barracks' piles (gold: the pile of good 14)
//!   compete: the highest level wins, ties go to the warrior kind the rotation byte prefers.
//!   If the eco-sector has a carrier to spare (`+0x22 + +0x1e + +0x20 < carriers`), the nearest
//!   free one (`GetNearestSettler(1, building)`) is ordered: the order count drops (not at 100),
//!   it reserves the weapon (target 2), the gold (3) and the second good (4) — 0xffff for none —,
//!   is attached to the building, leaves the offer list, gets the soldier type as its next type,
//!   +0x22 + 1, and event 26 `(type, weapon pile)`. With no order at all the rotation restarts
//!   (2). Always again after `13 + (Rand() & 2)` ticks.
//! * The carrier (`jobs::carrier`, event 26 / task 38 `CHANGE_TYPE_AT_BARRACKS`) walks in, takes
//!   the goods, becomes the soldier and walks to the barracks' working area (event 0x11, kind
//!   0xd).
//! * `SettlerEnter` (0x534a40) only answers true; `SettlerLeft` (0x535490), `ConvertEventIntoGoal`
//!   (0x5344c0) and +0x7c (0x534450) are empty.

use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;
use crate::pile::target;

/// `CBarrackRole` (store 0x5347c0: base, a version, +0x17c, the pile count and ids).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// +0x17c: which warrior kind (0 swordsman, 1 bowman, 2 third kind) the rotation favours.
    pub rotation: u8,
    /// +0x180: the piles, in buildingInfo order.
    pub piles: Vec<u16>,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.rotation);
        h.u16s(&self.piles);
    }
}

/// One recipe of the table at .data 0x1164660 (`[race * 10 + k]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub soldier: u16,
    pub weapon: u8,
    pub second: u8,
    pub gold: u8,
}

/// .data 0x1164660, read from the shipped exe: per race (Roman, Viking, Maya, Dark Tribe —
/// empty —, Trojan) ten `(soldier type, weapon, second good, gold bars)`; the fifth dword
/// (14/28/48/42/56) is not read by the barracks.
pub const RECIPES: [[(u16, u8, u8, u8); 10]; 5] = [
    [(32, 8, 0, 0), (29, 34, 0, 0), (35, 34, 0, 0), (33, 8, 0, 1), (30, 34, 0, 1), (36, 34, 0, 1), (34, 8, 0, 2), (31, 34, 0, 2), (37, 34, 0, 2), (44, 34, 3, 3)],
    [(32, 8, 0, 0), (29, 34, 0, 0), (38, 5, 0, 0), (33, 8, 0, 1), (30, 34, 0, 1), (39, 5, 0, 1), (34, 8, 0, 2), (31, 34, 0, 2), (40, 5, 0, 2), (44, 34, 3, 3)],
    [(32, 8, 0, 0), (29, 34, 0, 0), (41, 6, 0, 0), (33, 8, 0, 1), (30, 34, 0, 1), (42, 6, 0, 1), (34, 8, 0, 2), (31, 34, 0, 2), (43, 6, 0, 2), (44, 34, 3, 3)],
    [(0, 0, 0, 0); 10],
    [(32, 8, 0, 0), (29, 34, 0, 0), (61, 38, 0, 0), (33, 8, 0, 1), (30, 34, 0, 1), (62, 38, 0, 1), (34, 8, 0, 2), (31, 34, 0, 2), (63, 38, 0, 2), (44, 34, 3, 3)],
];

/// Good 14 (gold bars), the barracks' gold pile.
const GOLD: u8 = 14;

fn st(world: &mut World, id: u16) -> Option<&mut State> {
    match world.buildings.roles.get_mut(&id)? {
        Role { kind: RoleKind::Barrack(s), .. } => Some(s),
        _ => None,
    }
}

pub fn state(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Barrack(s), .. } => Some(s),
        _ => None,
    }
}

/// `CBarrackRole::Init` (0x534870), after `InitCommon`.
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, race, working) = (e.x(), e.y(), e.race, e.flags & 0x1000 != 0);
    let type_ex = match &e.data {
        EntityData::Building(d) => d.type_ex,
        _ => return,
    };
    if let Some(s) = st(world, id) {
        *s = State { rotation: 2, piles: Vec::new() };
    }
    let piles = world.rules.building(race, type_ex).piles.clone();
    for p in piles {
        let pile = super::add_building_pile(world, id, x + p.x as i32, y + p.y as i32, p.good, p.kind);
        if let Some(s) = st(world, id) {
            s.piles.push(pile);
        }
    }
    // Building flag 0x200 (selected): the GUI message (+0x58). Not sim.
    if working {
        super::notify(world, id, EntityEvent { event: 8, kind: 0, data: [id as i32, 0, 0] });
    }
    world.register_for_logic_update(id, 31);
}

/// vtbl+0x38 (0x535010): the pile whose good is `good`.
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    state(world, id).and_then(|s| s.piles.iter().copied().find(|&p| crate::pile::get(world, p).is_some_and(|d| d.good == good))).unwrap_or(0)
}

/// Goods on a pile not reserved by carriers (+0x41 − +0x42).
fn available(world: &World, pile: u16) -> i32 {
    crate::pile::available(world, pile)
}

/// `CBarrackRole::LogicUpdate` (0x534ac0).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(b) = world.entities.by_index(id as u32) else { return };
    let (pos, race, working) = (b.pos, b.race, b.flags & 0x1000 != 0);
    // Building flag 0x200: GUI refresh (+0x58). Not sim.
    if working {
        recruit(world, id, pos, race);
    }
    let r = world.rng.rand();
    world.register_for_logic_update(id, 13 + (r & 2) as i32);
}

fn recruit(world: &mut World, id: u16, pos: crate::grid::Y16X16, race: u8) {
    let eco = super::storage::eco_of_building(world, id);
    let Some(orders) = world.economy.get(eco).map(|e| e.soldier_orders) else { return };
    let gold_pile = pile_with_good(world, id, GOLD);
    let gold = available(world, gold_pile);
    let rotation = state(world, id).map_or(2, |s| s.rotation) as i32 % 3 + 3;
    let recipes = RECIPES.get(race as usize).copied().unwrap_or([(0, 0, 0, 0); 10]);
    let mut none_ordered = true;
    // (level, preference, recipe index, slot, rotation kind)
    let mut best: Option<(i32, i32, usize, usize, i32)> = None;
    for k in (0..10).rev() {
        let (soldier, weapon, second, need_gold) = recipes[k];
        let Some(&(_, slot)) = crate::eco::SOLDIER_WISH_TYPES.iter().find(|&&(t, _)| t == soldier) else { continue };
        if orders[slot as usize] <= 0 {
            continue;
        }
        none_ordered = false;
        let p1 = pile_with_good(world, id, weapon);
        let p2 = pile_with_good(world, id, second);
        let ok = weapon != 0 && available(world, p1) > 0 && (second == 0 || available(world, p2) > 0) && (need_gold == 0 || need_gold as i32 <= gold);
        if !ok {
            continue;
        }
        let level = match soldier {
            0x1d | 0x20 | 0x23 | 0x26 | 0x29 | 0x3d => 1,
            0x1e | 0x21 | 0x24 | 0x27 | 0x2a | 0x3e => 2,
            0x1f | 0x22 | 0x25 | 0x28 | 0x2b | 0x3f => 3,
            0x2c => 4,
            _ => 0,
        };
        let (kind, pref) = match world.rules.warrior_type(soldier) {
            2 => (0, rotation % 3),
            3 => (1, (rotation - 1) % 3),
            4 => (2, (rotation - 2) % 3),
            _ => (-1, -1),
        };
        let better = match best {
            None => level > -1,
            Some((bl, bp, ..)) => bl < level || (level == bl && bp < pref),
        };
        if better {
            best = Some((level, pref, k, slot as usize, kind));
        }
    }
    if none_ordered && let Some(s) = st(world, id) {
        s.rotation = 2;
    }
    let Some((_, _, k, slot, kind)) = best else { return };
    let Some(e) = world.economy.get(eco) else { return };
    let spare = (e.carriers_with_job_order as u32 + e.extra_min_carrier as u32 + e.min_carrier as u32) < e.settlers[1] as u32;
    if !spare {
        // GUI_WARN_BUILD_NOSETTLER (0x534f88).
        let owner = world.entities.by_index(id as u32).map_or(0, |b| b.owner);
        world.events.push(crate::event::SimEvent::warning(owner, 0x99b, pos));
        return;
    }
    let carrier = crate::eco::get_nearest_settler(world, eco, 1, pos.x(), pos.y());
    if carrier == 0 {
        return;
    }
    let (soldier, weapon, second, need_gold) = recipes[k];
    if kind >= 0
        && let Some(s) = st(world, id)
    {
        s.rotation = kind as u8;
    }
    if let Some(e) = world.economy.get_mut(eco)
        && e.soldier_orders[slot] != 100
    {
        e.soldier_orders[slot] -= 1;
    }
    let p1 = pile_with_good(world, id, weapon);
    let p2 = pile_with_good(world, id, second);
    crate::pile::attach_and_inc_amount_leaving(world, p1, carrier, 1, target::PILE_SRC);
    if need_gold < 1 {
        set_carrier_target(world, carrier, target::PILE_SRC_2, 0xffff);
    } else {
        crate::pile::attach_and_inc_amount_leaving(world, gold_pile, carrier, need_gold as i32, target::PILE_SRC_2);
    }
    if second == 0 {
        set_carrier_target(world, carrier, target::PILE_SRC_3, 0xffff);
    } else {
        crate::pile::attach_and_inc_amount_leaving(world, p2, carrier, 1, target::PILE_SRC_3);
    }
    // CBuilding::Attach (observer list, home, ATTACHED).
    super::attach(world, id, carrier);
    if let Some(j) = crate::jobs::core::job_mut(world, carrier) {
        j.home = id;
    }
    if let Some(e) = world.entities.by_index_mut(carrier as u32) {
        e.flags |= flag::ATTACHED;
    }
    crate::eco::get_settler_out_of_offer(world, eco, carrier);
    if let Some(j) = crate::jobs::core::job_mut(world, carrier) {
        j.carrier.next_type = soldier as u8; // role vtbl+0x74 (0x574520)
    }
    if let Some(e) = world.economy.get_mut(eco) {
        e.carriers_with_job_order += 1;
    }
    crate::jobs::core::set_event(world, carrier, EntityEvent { event: 26, kind: 0, data: [soldier as i32, p1 as i32, 0] });
    // The script's `SETTLER_CHANGE_TYPE` (0x12) with the soldier's type: 0x534f5d calls
    // 0x5bdb00(0x12, type) when GameScriptManager +0x4e4 (the event's requests) > 0.
    world.script_events.send(crate::event::ScriptEvents::SETTLER_CHANGE_TYPE, [soldier as i32, 0, 0]);
}

/// `CCarrierRole` vtbl+0x50 (0x574450) for targets 3 / 4 without a pile (0xffff).
fn set_carrier_target(world: &mut World, carrier: u16, tgt: u8, v: u16) {
    if let Some(j) = crate::jobs::core::job_mut(world, carrier) {
        match tgt {
            target::PILE_SRC_2 => j.carrier.source_pile2 = v,
            target::PILE_SRC_3 => j.carrier.source_pile3 = v,
            _ => {}
        }
    }
}
