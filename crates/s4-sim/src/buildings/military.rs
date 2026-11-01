//! `CMilitaryBuildingRole` (house kind 10: small / large tower; release vtable 0x1058f8c) and
//! `CCastleRole` (house kind 15, vtable 0x1058c74), which shares every virtual below except
//! `Init` (0x5443f0: the military `Init`, then four castle bytes = 100) and the store/load pair.
//! Own reverse engineering of the shipped exe; addresses are release VAs.
//!
//! **Garrison bookkeeping** (role bytes +0x17c..+0x181): soldiers inside, ordered (on their
//! way) and wished, separately for bowmen (warrior type 3, the `top` slots) and swordsmen
//! (type 2, the other slots). The slot vector (+0x1a4) holds the soldier in each buildingInfo
//! `settler` slot. The **door settler** (+0x182, type 69, `CDoorRole`, see
//! `jobs::door`) stands invisible on the door node; while a soldier guards the door (+0x184)
//! it is gone.
//!
//! * `Init` 0x548390: `InitCommon`, flags `(f & ~3) | 0x203000c`, garrison zeroed, the slot
//!   vector sized to the info's slot count, the door node unblocked, the door settler created,
//!   logic update in 16.
//! * `LogicUpdate` 0x548ed0: a damaged building repairs itself (`Rand() < 33 %`: +1 health,
//!   damage level 0x548dc0), soldiers beyond the wish leave (0x549680), missing soldiers are
//!   called (0x549c50), again in 15.
//! * Soldier search 0x582ca0: `CSettlerMgr`'s list of the wanted type (newest first), free
//!   (SELECTABLE) soldiers in the building's sector (the ensign's tile sector), nearest by hex
//!   distance below the radius; tried for levels 3, 2, 1 within 20, then 40, then 80. The
//!   chosen soldier is attached to the building and gets event 1; `jobs::soldier` walks it to
//!   the door and lets it enter (`SettlerEnter` 0x5488d0), where it becomes a `CTowerSoldier`.
//! * The first soldier makes the building inhabited (`FirstSettlerEnter` 0x5484f0): a new door
//!   settler and the territory around the building (`territory::update_owner`, 0x60cd30).
//!
//! Not ported here (TODO(M4)): the war map (enemy checks 0x5b0a20 return "no enemy"), capture
//! by an enemy soldier (the foreign branch of `FirstSettlerEnter`), damage from fights
//! (+0x6c 0x549520), destruction (+0x74 0x549440), the GUI messages and AI notices.

use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;
use crate::jobs::{core as jcore, role};

/// Role state (store version 3, 0x548210: the six garrison bytes, +0x182, +0x184, +0x188,
/// +0x190, +0x194, +0x18c, the vector at +0x198, the slot vector).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// +0x17c / +0x17d: bowmen / swordsmen inside.
    pub bow_in: u8,
    pub sword_in: u8,
    /// +0x17e / +0x17f: bowmen / swordsmen ordered (walking here).
    pub bow_coming: u8,
    pub sword_coming: u8,
    /// +0x180 / +0x181: wished bowmen / swordsmen.
    pub bow_wish: u8,
    pub sword_wish: u8,
    /// +0x182: the door settler (0 while a soldier guards the door).
    pub door: u16,
    /// +0x184: the soldier guarding the door outside (0 = none).
    pub guard: u16,
    /// +0x188: stored, never seen non-zero (meaning unknown).
    pub unknown_188: i32,
    /// +0x18c: damage level 0..5 (0x548dc0), +0x190: the building burns.
    pub damage_level: i32,
    pub burning: bool,
    /// +0x198: a second u16 vector, emptied by `Init` and stored; no writer found besides load.
    pub unknown_198: Vec<u16>,
    /// +0x1a4: the soldier in each slot (0 = empty).
    pub slots: Vec<u16>,
    /// +0x150: the flag is up (door settler with flag 0x10000 present); drives the flag gfx.
    pub flag_up: bool,
    /// `CCastleRole` +0x1b1/+0x1b3/+0x1b5/+0x1b7, set to 100 by its `Init` (meaning unknown).
    pub castle: [u8; 4],
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for v in [self.bow_in, self.sword_in, self.bow_coming, self.sword_coming, self.bow_wish, self.sword_wish] {
            h.u8(v);
        }
        h.u16(self.door);
        h.u16(self.guard);
        h.u32(self.unknown_188 as u32);
        h.u32(self.damage_level as u32);
        h.bool(self.burning);
        h.u16s(&self.unknown_198);
        h.u16s(&self.slots);
        h.bool(self.flag_up);
        self.castle.iter().for_each(|&b| h.u8(b));
    }
}

/// What the role reads from its buildingInfo record (+0x178).
struct Info {
    tops: Vec<bool>,
    dirs: Vec<u8>,
    first_top: i8,
    max_sword: u8,
    max_bow: u8,
    hitpoints: u8,
}

fn info(world: &World, id: u16) -> Option<Info> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    let r = world.rules.building(e.race, d.type_ex);
    Some(Info {
        tops: r.soldier_slots.iter().map(|s| s.top).collect(),
        dirs: r.soldier_slots.iter().map(|s| s.dir).collect(),
        first_top: r.first_top_slot,
        max_sword: r.melee_slots,
        max_bow: r.ranged_slots,
        hitpoints: r.hitpoints,
    })
}

pub fn state(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Military(s), .. } => Some(s),
        _ => None,
    }
}

fn st(world: &mut World, id: u16) -> Option<&mut State> {
    match world.buildings.roles.get_mut(&id)? {
        Role { kind: RoleKind::Military(s), .. } => Some(s),
        _ => None,
    }
}

fn inhabited(world: &World, id: u16) -> bool {
    super::role(world, id).is_some_and(|r| r.base.worker_in_house)
}

/// Building position, door, owner.
fn building(world: &World, id: u16) -> Option<(crate::grid::Y16X16, crate::grid::Y16X16, u8)> {
    let e = world.entities.by_index(id as u32)?;
    match &e.data {
        EntityData::Building(d) => Some((e.pos, d.door, e.owner)),
        _ => None,
    }
}



/// `CMilitaryBuildingRole::Init` (0x548390); `CCastleRole::Init` (0x5443f0) adds the castle
/// bytes. `IBuildingRole::InitCommon` ran before (`buildings::init`).
pub fn init(world: &mut World, id: u16, castle: bool) {
    let Some(inf) = info(world, id) else { return };
    let Some((_, door, owner)) = building(world, id) else { return };
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags = (e.flags & !3) | 0x203_000c;
    }
    if let Some(s) = st(world, id) {
        *s = State { slots: vec![0; inf.tops.len()], castle: if castle { [100; 4] } else { [0; 4] }, ..State::default() };
    }
    // Building flag 0x200 (selected by the local player): the GUI message (+0x58). Not sim.
    world.clear_blocked_land(door.x(), door.y());
    new_door(world, id, door, owner, false);
    world.register_for_logic_update(id, 16);
}

/// 0x549fc0 (and the same lines in `Init` / 0x54a120): a new door settler at the door, homed at
/// the building. Normally it carries flag 0x10000 and the flag goes up; for a capture (`down`)
/// it is not vulnerable and the flag stays down.
fn new_door(world: &mut World, id: u16, door: crate::grid::Y16X16, owner: u8, down: bool) {
    let d = crate::create::add_door_settler(world, door.x(), door.y(), owner);
    if let Some(j) = jcore::job_mut(world, d) {
        j.home = id; // role vtbl+0x30 SetBuilding
    }
    if let Some(e) = world.entities.by_index_mut(d as u32) {
        if down {
            e.flags &= !flag::VULNERABLE_MASK;
        } else {
            e.flags |= 0x10000;
        }
    }
    if let Some(s) = st(world, id) {
        s.door = d;
        s.flag_up = !down;
    }
}

/// 0x54a060 / the door part of 0x549390: the door settler is made invisible, loses its home and
/// is killed.
fn kill_door(world: &mut World, id: u16) {
    let Some(d) = st(world, id).map(|s| std::mem::take(&mut s.door)) else { return };
    if let Some(s) = st(world, id) {
        s.flag_up = false;
    }
    if d == 0 {
        return;
    }
    if let Some(e) = world.entities.by_index_mut(d as u32) {
        e.flags &= !flag::VISIBLE;
    }
    if let Some(j) = jcore::job_mut(world, d) {
        j.home = 0;
    }
    world.kill(d, 0);
}

/// `LogicUpdate` (0x548ed0).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(inf) = info(world, id) else { return };
    let health = match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => d.health,
        _ => return,
    };
    if health < inf.hitpoints && world.rng.rand() < crate::rng::Random16::percent_value(33) {
        let h = heal(world, id, 1);
        damage_level(world, id, h, inf.hitpoints);
    }
    release_surplus(world, id);
    call_soldiers(world, id);
    // Building flag 0x200: GUI refresh (+0x58). Not sim.
    world.register_for_logic_update(id, 15);
}

/// `CBuilding` vtbl+0x18 (0x566fb0): health + `n`, at most 255. Returns the new health.
fn heal(world: &mut World, id: u16, n: u8) -> u8 {
    match world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
        Some(EntityData::Building(d)) => {
            d.health = d.health.saturating_add(n);
            d.health
        }
        _ => 0,
    }
}

/// 0x548dc0: the damage level follows the health in percent of the hit points with hysteresis
/// (level 0..5 bands 80–100, 65–100, 50–80, 35–65, 20–50, 0–35); a damaged building burns
/// (+0x190; the fire gfx frame count +0x194 and the counters +0x164/+0x167 are not modelled),
/// full health puts the fire out.
fn damage_level(world: &mut World, id: u16, health: u8, max: u8) {
    let Some(s) = st(world, id) else { return };
    let h = health as i32 * 100;
    let max = max as i32;
    let (lo, hi) = match s.damage_level {
        1 => (65, 100),
        2 => (50, 80),
        3 => (35, 65),
        4 => (20, 50),
        5 => (0, 35),
        _ => (80, 100),
    };
    if h < lo * max {
        s.damage_level += 1;
    }
    if hi * max <= h {
        s.damage_level -= 1;
    }
    if !s.burning && s.damage_level != 0 {
        s.burning = true;
    }
    if max * 100 <= h {
        s.burning = false;
        s.damage_level = 0;
    }
}

/// 0x54a990: the last slot of the given kind that holds a soldier other than the door guard.
fn last_slot_with(s: &State, tops: &[bool], top: bool) -> Option<usize> {
    (0..s.slots.len()).rev().find(|&k| tops.get(k) == Some(&top) && s.slots[k] != 0 && s.slots[k] != s.guard)
}

/// 0x549680: one soldier beyond the wish leaves — a swordsman first, else a bowman — unless
/// enemy warriors are within 10.
fn release_surplus(world: &mut World, id: u16) {
    let Some((pos, _, owner)) = building(world, id) else { return };
    if crate::warmap::enemy_warriors_near(world, pos.x(), pos.y(), 10, owner) {
        return;
    }
    let Some(inf) = info(world, id) else { return };
    let Some(s) = state(world, id) else { return };
    let mut pick = None;
    if s.sword_wish < s.sword_in {
        pick = last_slot_with(s, &inf.tops, false).map(|k| (k, true));
    }
    if pick.is_none() && s.bow_wish < s.bow_in {
        pick = last_slot_with(s, &inf.tops, true).map(|k| (k, false));
    }
    if let Some((k, sword)) = pick {
        soldier_out(world, id, k);
        if sword {
            compact_swordsmen(world, id);
        } else {
            fill_first_top(world, id);
        }
    }
}

/// 0x549c50: order the soldiers that are missing. An empty building with no wish and no order
/// asks for one swordsman (and, if none was found, one bowman) and sets that wish to 1.
fn call_soldiers(world: &mut World, id: u16) {
    let Some(s) = state(world, id).cloned() else { return };
    let none_in = s.bow_in as u32 + s.sword_in as u32 == 0;
    let no_wish = s.bow_wish as u32 + s.sword_wish as u32 == 0;
    let none_coming = s.bow_coming as u32 + s.sword_coming as u32 == 0;
    let empty = no_wish && none_coming && none_in;
    if ((s.sword_coming as u32 + s.sword_in as u32) < s.sword_wish as u32 || empty)
        && order_soldier(world, id, 2, [31, 30, 29]) != 0
        && let Some(s) = st(world, id)
    {
        s.sword_coming = s.sword_coming.wrapping_add(1);
        if empty {
            s.sword_wish = 1;
        }
    }
    let Some(s2) = state(world, id).cloned() else { return };
    let no_sword_coming = s2.sword_coming == 0;
    if ((s2.bow_coming as u32 + s2.bow_in as u32) < s2.bow_wish as u32 || (no_sword_coming && empty))
        && order_soldier(world, id, 3, [34, 33, 32]) != 0
        && let Some(s) = st(world, id)
    {
        s.bow_coming = s.bow_coming.wrapping_add(1);
        if no_sword_coming && empty {
            s.bow_wish = 1;
        }
    }
    // Nobody in and nobody coming: GUI_WARN_BUILD_NOWAR (0x549eb0).
    if none_in
        && let Some(s) = state(world, id)
        && s.sword_coming as u32 + s.bow_coming as u32 == 0
        && let Some(b) = world.entities.by_index(id as u32)
    {
        let ev = crate::event::SimEvent::warning(b.owner, 0x99d, b.pos);
        world.events.push(ev);
    }
}

/// One order of 0x549c50: the soldier the AI linked to the building (0x582f70), else the search
/// 0x582ca0 for the three levels (best first) within 20, 40, 80.
fn order_soldier(world: &mut World, id: u16, warrior: u8, kinds: [u16; 3]) -> u16 {
    let g = ai_linked_soldier(world, id, warrior);
    if g != 0 {
        return g;
    }
    for radius in [20, 40, 80] {
        for kind in kinds {
            let s = call_soldier(world, id, kind, radius);
            if s != 0 {
                return s;
            }
        }
    }
    0
}

/// 0x582f70: the soldier named by the building's AI record (`IEntity +0x10`, +2 an entity id):
/// taken if it is a SELECTABLE settler (flags & 0x200a000 = 0x2002000) of the building's owner
/// whose class (flags & 0xf, 2 or 3) is `warrior`, in the sector (tile record +0) of the
/// building's ensign; it loses SELECTABLE, is attached (0x5816f0) and gets event 1.
///
/// The record (0x18 bytes) is the AI's per-entity data: only `0x435370` creates it, for an
/// entity with a flag of 0x3000000, and all its callers are AI code. `IEntity`'s constructor
/// clears +0x10 and `Kill` / the destructors free it (0x566d80). +2 (the partner) has exactly
/// two writers (scan of every `0x435370` caller): the normal sector AI's 0x441de0 (from its
/// military-building pass 0x441eb0: a soldier found in its goal cache 0x43fab0 and the building
/// name each other) and `CAIAgentIntruderCombat` 0x431bd0, which clears a soldier's +2. Both
/// belong to the normal (non Dark Tribe) AI, not ported yet (S2), so no record has a partner
/// and this finds nothing — as for every human player's building in the original.
fn ai_linked_soldier(_world: &mut World, _id: u16, _warrior: u8) -> u16 {
    0
}

/// `CSettlerMgr` 0x582ca0: the nearest free soldier of `kind` (hex distance from the building
/// position, strictly below `radius`; ties: first in the list) in the sector of the building's
/// ensign. It loses SELECTABLE, is attached to the building and gets event 1.
pub fn call_soldier(world: &mut World, id: u16, kind: u16, radius: i32) -> u16 {
    let Some(b) = world.entities.by_index(id as u32) else { return 0 };
    let EntityData::Building(d) = &b.data else { return 0 };
    let (bpos, owner) = (b.pos, b.owner);
    let Some(en) = world.grid.checked_index(d.ensign.x(), d.ensign.y()) else { return 0 };
    let sector = world.tiling.sector_of(en);
    let mut best = (u32::MAX, 0u16);
    for s in crate::settler_lists::iter(world, owner, kind) {
        let Some(e) = world.entities.by_index(s as u32) else { continue };
        if e.flags & flag::SELECTABLE == 0 {
            continue;
        }
        let Some(n) = world.grid.checked_index(e.x(), e.y()) else { continue };
        if world.tiling.sector_of(n) != sector {
            continue;
        }
        let dist = crate::spiral::grid_distance(bpos.x() - e.x(), bpos.y() - e.y()) as u32;
        if dist < radius as u32 && dist < best.0 {
            best = (dist, s);
        }
    }
    let s = best.1;
    if s != 0 {
        send_soldier(world, id, s);
    }
    s
}

/// 0x582ea0 (and the tail of 0x582ca0): the soldier is ordered to the building: not
/// selectable, out of its groups (`CGroupMgr` vtbl+0x1c, [`crate::group::remove_all`]),
/// attached, event 1. The local player's deselection is not simulation.
fn send_soldier(world: &mut World, id: u16, s: u16) {
    if let Some(e) = world.entities.by_index_mut(s as u32) {
        e.flags &= !flag::SELECTABLE;
    }
    crate::group::remove_all(world, s);
    attach_settler(world, id, s);
    jcore::set_event(world, s, EntityEvent { event: 1, kind: 0, data: [id as i32, 0, 0] });
}

/// `CBuilding::Attach(settler)` (vtbl+0x74, 0x5370e0): the building's observer list, which sets
/// the settler's observer target 0 (`CSettler::SetObserverTarget` 0x581710: the role's home,
/// flag ATTACHED).
pub fn attach_settler(world: &mut World, id: u16, s: u16) {
    super::attach(world, id, s);
    set_home(world, s, id);
}

/// `CSettler::SetObserverTarget(0, home)` (0x581710 → role 0x586ff0).
pub(crate) fn set_home(world: &mut World, s: u16, home: u16) {
    if let Some(j) = jcore::job_mut(world, s) {
        j.home = home;
    }
    if let Some(e) = world.entities.by_index_mut(s as u32) {
        if home != 0 {
            e.flags |= flag::ATTACHED;
        } else {
            e.flags &= !flag::ATTACHED;
        }
    }
}

/// `CBuilding::Detach(settler)` (vtbl+0x40, 0x537100): off the observer list, observer target
/// cleared, then the role's +0x7c (military: [`settler_detached`]; the `IBuildingRole` default
/// 0x534450 does nothing).
pub fn detach_settler(world: &mut World, id: u16, s: u16) {
    super::detach(world, id, s);
    set_home(world, s, 0);
    if state(world, id).is_some() {
        settler_detached(world, id, s);
    }
}

/// Role +0x7c (0x54aa40): an ordered soldier no longer comes. A settler of another player (the
/// capture case) cancels every order and raises the flag again.
fn settler_detached(world: &mut World, id: u16, s: u16) {
    let Some(e) = world.entities.by_index(s as u32) else { return };
    if e.obj_type != ObjType::Settler {
        return;
    }
    let (wt, sp) = (e.warrior_type(), e.owner);
    let bp = world.entities.by_index(id as u32).map_or(0, |b| b.owner);
    if let Some(st) = st(world, id) {
        if wt == 3 && st.bow_coming != 0 {
            st.bow_coming -= 1;
        }
        if wt == 2 && st.sword_coming != 0 {
            st.sword_coming -= 1;
        }
    }
    if sp != bp {
        cancel_orders(world, id);
        let door = state(world, id).map_or(0, |s| s.door);
        if door != 0 {
            if let Some(d) = world.entities.by_index_mut(door as u32) {
                d.flags |= 0x10000;
            }
            if let Some(s) = st(world, id) {
                s.flag_up = true;
            }
        }
    }
}

/// 0x54aaf0: event 9 (data: the building) to every observer, which are then detached (the
/// soldiers on their way become free), and no soldier is ordered any more.
fn cancel_orders(world: &mut World, id: u16) {
    super::notify_and_detach_all(world, id, EntityEvent { event: 9, kind: 0, data: [id as i32, 0, 0] });
    if let Some(s) = st(world, id) {
        s.bow_coming = 0;
        s.sword_coming = 0;
    }
}

/// 0x549580 with `test_only` (`IncWishAndOrder`): whether a soldier of warrior type `wt` may be
/// added — the wish is below the slots of that kind, and so are inside + ordered.
pub fn can_inc_wish_and_order(world: &World, id: u16, wt: u8) -> bool {
    let (Some(inf), Some(s)) = (info(world, id), state(world, id)) else { return false };
    match wt {
        2 => s.sword_wish < inf.max_sword && (s.sword_in as u32 + s.sword_coming as u32) < inf.max_sword as u32,
        3 => s.bow_wish < inf.max_bow && (s.bow_in as u32 + s.bow_coming as u32) < inf.max_bow as u32,
        _ => false,
    }
}

/// 0x549580 (`IncWishAndOrder`; 0x549f00 is the same without the flag): wish and order + 1 when
/// [`can_inc_wish_and_order`]. Returns whether it fitted.
pub fn inc_wish_and_order(world: &mut World, id: u16, wt: u8, test_only: bool) -> bool {
    if !can_inc_wish_and_order(world, id, wt) {
        return false;
    }
    if !test_only && let Some(s) = st(world, id) {
        if wt == 2 {
            s.sword_wish += 1;
            s.sword_coming += 1;
        } else {
            s.bow_wish += 1;
            s.bow_coming += 1;
        }
    }
    true
}

/// `SettlerEnter` (0x5488d0). Returns whether the settler was taken in.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let Some(e) = world.entities.by_index(settler as u32) else { return false };
    let (sp, kind) = (e.owner, e.kind);
    let Some((bpos, _, bp)) = building(world, id) else { return false };
    let guard = state(world, id).map_or(0, |s| s.guard);
    if sp != bp && guard == settler {
        // "[MM] Settler tried to turn into enemy guard!"
        world.kill(settler, 0);
        return false;
    }
    if !inhabited(world, id) && first_settler_enter(world, id, settler) {
        // A Dark Tribe soldier destroys the empty building (vtbl+0x20 with 10000).
        world.kill(settler, 0);
        super::crush::damage(world, id, 10000);
        return true;
    }
    if guard == settler {
        guard_in(world, id, true);
        return true;
    }
    // Off the observer list (+0x54) without the +0x7c callback, observer target cleared.
    super::detach(world, id, settler);
    set_home(world, settler, 0);
    crate::jobs::soldier::replace_role(world, settler, role::TOWER_SOLDIER);
    if let Some(j) = jcore::job_mut(world, settler) {
        j.home = id; // vtbl+0x30 SetBuilding
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.pos = bpos;
    }
    jcore::take_wait_list(world, settler);
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags &= !0x30020;
    }
    let Some(inf) = info(world, id) else { return true };
    let wt = world.rules.warrior_type(kind);
    if wt != 2 && wt != 3 {
        return true;
    }
    let want_top = wt == 3;
    let free = state(world, id).and_then(|s| (0..inf.tops.len()).find(|&k| inf.tops[k] == want_top && s.slots.get(k) == Some(&0)));
    if let Some(k) = free {
        if let Some(s) = st(world, id) {
            s.slots[k] = settler;
        }
        put_in_slot(world, settler, k, &inf);
    }
    if let Some(s) = st(world, id) {
        if want_top {
            s.bow_in = s.bow_in.wrapping_add(1);
            s.bow_coming = s.bow_coming.wrapping_sub(1);
        } else {
            s.sword_in = s.sword_in.wrapping_add(1);
            s.sword_coming = s.sword_coming.wrapping_sub(1);
        }
    }
    true
}

/// The slot set-up repeated in 0x5488d0 / 0x549aa0 / 0x548790: the role's in-building pixel
/// offset (+0x24/+0x28, the slot's x/y; not modelled), the settler's direction and the
/// `CTowerSoldier` slot, state (0 not `top`; `top`: 1 for the first top slot, else 2) and
/// direction; +0x30 = now only for state 3 (never here).
fn put_in_slot(world: &mut World, settler: u16, k: usize, inf: &Info) {
    let dir = inf.dirs.get(k).copied().unwrap_or(0);
    let state = if inf.tops.get(k) == Some(&true) { (k as i32 != inf.first_top as i32) as u8 + 1 } else { 0 };
    if let Some(EntityData::Settler(s)) = world.entities.by_index_mut(settler as u32).map(|e| &mut e.data) {
        s.direction = dir;
        s.job.tower.slot = k as u8;
        s.job.tower.state = state;
        s.job.tower.dir = dir;
        s.job.tower.tick = 0;
    }
}

/// Role +0x6c (0x549520): damage `n` becomes `max(1, (n − 5) / 2)` hit points (0 for `n < 1`);
/// a building that survives them updates its damage level (0x548dc0).
pub fn hit(world: &mut World, id: u16, n: i32) -> i32 {
    if n < 1 {
        return 0;
    }
    let d = ((n - 5) / 2).max(1);
    let Some(e) = world.entities.by_index(id as u32) else { return d };
    let (dead, health) = match &e.data {
        EntityData::Building(b) => (e.flags & flag::DIED != 0, b.health as i32),
        _ => return d,
    };
    if !dead && health - d > 0 {
        let max = info(world, id).map_or(0, |i| i.hitpoints);
        damage_level(world, id, (health - d) as u8, max);
    }
    d
}

/// Role +0x74 (0x549440), a destroyed building: the door guard goes in, the door settler and
/// every soldier inside are killed (home cleared, invisible). First the owner's statistic counts
/// one more military building lost (+0xe3c).
pub fn destroyed(world: &mut World, id: u16) {
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    crate::statistic::military_building_lost(world, owner);
    if state(world, id).is_some_and(|s| s.guard != 0) {
        guard_in(world, id, false);
    }
    kill_door(world, id);
    let slots = state(world, id).map(|s| s.slots.clone()).unwrap_or_default();
    for (k, s) in slots.into_iter().enumerate() {
        if s == 0 {
            continue;
        }
        if let Some(j) = jcore::job_mut(world, s) {
            j.home = 0;
        }
        if let Some(e) = world.entities.by_index_mut(s as u32) {
            e.flags &= !flag::VISIBLE;
        }
        world.kill(s, 0);
        if let Some(st) = st(world, id) {
            st.slots[k] = 0;
        }
    }
}

/// `FirstSettlerEnter` (0x5484f0). Returns true for a Dark Tribe settler (the caller then
/// destroys the building); otherwise the building is inhabited: while the map loads only the
/// tower bits are set (0x60cc80); else the door settler is replaced, the territory around the
/// building recomputed (0x60cd30) and a new door settler placed (0x549fc0).
fn first_settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let Some(e) = world.entities.by_index(settler as u32) else { return false };
    if e.race == 3 {
        return true;
    }
    let sp = e.owner;
    if let Some(r) = super::role_mut(world, id) {
        r.base.worker_in_house = true;
    }
    let Some(b) = world.entities.by_index(id as u32) else { return false };
    let (x, y, kind, bp) = (b.x(), b.y(), b.kind, b.owner);
    if world.loading_buildings {
        crate::territory::set_tower_bits(world, x, y, kind, bp);
        return false;
    }
    kill_door(world, id);
    if sp != bp {
        // Capture: a new unique id, the owner change (0x53f050), the building off the map node,
        // `RemoveFromWorld` (0x536d00), `SetOwner(x, y)` (0x60cd30) and `SetToWorld` (0x536470)
        // for the new owner, back on the node, AI notice 3 (new owner, building, old owner).
        // The conquered / lost counts per type follow after `SetToWorld` (0x54871c / 0x548730).
        world.entities.assign_new_uid(id);
        // The old owner's GUI_WARN_CAPTURE (0x548633).
        if let Some(b) = world.entities.by_index(id as u32) {
            let ev = crate::event::SimEvent::warning(bp, 0x99e, b.pos);
            world.events.push(ev);
        }
        change_owner(world, id, sp);
        let n = world.grid.checked_index(x, y);
        if let Some(n) = n {
            world.layers.map_object[n] = 0;
        }
        super::crush::remove_from_world(world, id);
        crate::territory::update_owner(world, x, y);
        crate::create::set_to_world(world, id);
        if let Some(n) = n {
            world.layers.map_object[n] = id;
        }
        crate::statistic::building_captured(world, sp, bp, kind);
        crate::ai::post_event(world, 3, sp as i32, id as i32, bp as i32);
    } else {
        // Own soldier: AI notice 10 (0x548592) and UI event 0x2bd (not sim).
        crate::ai::post_event(world, 10, bp as i32, id as i32, 0);
    }
    crate::territory::update_owner(world, x, y);
    if let Some((_, door, owner)) = building(world, id) {
        new_door(world, id, door, owner, false);
    }
    false
}

/// 0x53f050 `(building, player)`: a building changes hands — out of the old player's building
/// counts and fighting strength (0x4f8730 −1 with the old player's race), the owner nibble set,
/// into the new player's (+1); it leaves its list (0x53d160) for the head of the new owner's.
/// The statistic's per-type count 0x11ac05c / total 0x11ac440 move with the finished counts
/// (`PlayerStrength::count`).
fn change_owner(world: &mut World, id: u16, player: u8) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (old, kind) = (e.owner, e.kind);
    if old == player {
        return;
    }
    crate::statistic::building(world, old, kind, -1);
    super::list_remove(world, id);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.owner = player;
    }
    super::list_add(world, id);
    crate::statistic::building(world, player, kind, 1);
}

/// `SettlerLeft` (role +0x50, 0x549720), called by the leaving settler's role (`SetFree` of a
/// `CTowerSoldier` / `CDoorRole`): the door settler (a soldier takes over the door, 0x54a120),
/// a soldier in a slot (the slots are tidied up), or an ordered soldier (order − 1, then
/// `Detach`, whose +0x7c lowers the order again if it is still non-zero — as in the original).
pub fn settler_left(world: &mut World, id: u16, settler: u16) {
    let Some(s) = state(world, id) else { return };
    if settler == s.door {
        if let Some(s) = st(world, id) {
            s.door = 0;
            s.flag_up = false;
        }
        new_guard(world, id);
        return;
    }
    let Some(inf) = info(world, id) else { return };
    let idx = if settler == 0 { None } else { s.slots.iter().position(|&x| x == settler) };
    match idx {
        None => {
            let wt = world.entities.by_index(settler as u32).map_or(0, |e| e.warrior_type());
            if let Some(s) = st(world, id) {
                match wt {
                    3 => s.bow_coming = s.bow_coming.wrapping_sub(1),
                    2 => s.sword_coming = s.sword_coming.wrapping_sub(1),
                    _ => {}
                }
            }
            detach_settler(world, id, settler);
        }
        Some(k) => {
            let top = inf.tops.get(k) == Some(&true);
            if let Some(s) = st(world, id) {
                s.slots[k] = 0;
                if top {
                    s.bow_in = s.bow_in.wrapping_sub(1);
                } else {
                    s.sword_in = s.sword_in.wrapping_sub(1);
                }
            }
            if top {
                fill_first_top(world, id);
            } else {
                compact_swordsmen(world, id);
            }
            if state(world, id).is_some_and(|s| s.guard == settler) {
                if let Some(s) = st(world, id) {
                    s.guard = 0;
                }
                new_guard(world, id);
            }
        }
    }
}

/// 0x549800: the soldier in slot `k` leaves: count − 1, its warrior bits and VULNERABLE back,
/// a fresh role of its type (`CreateSettlerRole` 0x583310 + `SetRole` 0x580c20), on the map
/// next to the door (0x582a40), visible and selectable again, AI notice 0x15 (its player, the
/// soldier, the building; 0x549920). TODO(port): deselection (vtbl+0x68).
fn soldier_out(world: &mut World, id: u16, k: usize) {
    let Some(inf) = info(world, id) else { return };
    let Some((_, door, _)) = building(world, id) else { return };
    let Some(s) = st(world, id) else { return };
    let Some(&soldier) = s.slots.get(k) else { return };
    s.slots[k] = 0;
    if inf.tops.get(k) == Some(&true) {
        s.bow_in = s.bow_in.wrapping_sub(1);
    } else {
        s.sword_in = s.sword_in.wrapping_sub(1);
    }
    let Some(e) = world.entities.by_index(soldier as u32) else { return };
    let (race, kind) = (e.race, e.kind);
    let wt = world.rules.warrior_type(kind) as u32;
    world.unregister_from_logic_update(soldier);
    if let Some(e) = world.entities.by_index_mut(soldier as u32) {
        e.flags = (e.flags & !0xF) | wt | flag::VULNERABLE_MASK;
    }
    let r = world.rules.jobs.settler_role(race, kind);
    crate::jobs::soldier::replace_role(world, soldier, r);
    crate::create::put_settler_near(world, soldier, door.x(), door.y());
    crate::warmap::readd(world, soldier);
    if let Some(e) = world.entities.by_index_mut(soldier as u32) {
        e.flags = (e.flags & !flag::ON_BOARD) | 0x2100;
    }
    let owner = world.entities.by_index(soldier as u32).map_or(0, |e| e.owner);
    crate::ai::post_event(world, 0x15, owner as i32, soldier as i32, id as i32);
}

/// 0x549aa0: swordsmen move to the first non-`top` slots, in slot order.
fn compact_swordsmen(world: &mut World, id: u16) {
    let Some(inf) = info(world, id) else { return };
    let count = inf.tops.len();
    let len = state(world, id).map_or(0, |s| s.slots.len());
    let mut next = 0usize;
    for c in 0..len {
        let Some(s) = state(world, id) else { return };
        let soldier = s.slots[c];
        let mut cand = next;
        let sword = soldier != 0 && world.entities.by_index(soldier as u32).is_some_and(|e| world.rules.warrior_type(e.kind) == 2);
        if sword && next < count {
            let mut k = next;
            let mut target = None;
            while k < count {
                cand += 1;
                if !inf.tops[k] {
                    target = Some(k);
                    break;
                }
                k += 1;
            }
            if let Some(k) = target {
                if let Some(s) = st(world, id) {
                    if let Some(p) = s.slots.iter().take(count).position(|&x| x == soldier) {
                        s.slots[p] = 0;
                    }
                    s.slots[k] = soldier;
                }
                put_in_slot(world, soldier, k, &inf);
            }
        }
        next = cand;
    }
}

/// 0x549940: an empty first `top` slot gets the bowman of the highest settler type from the
/// other `top` slots (the last such slot on ties).
fn fill_first_top(world: &mut World, id: u16) {
    let Some(inf) = info(world, id) else { return };
    let ft = inf.first_top;
    if ft < 0 {
        return;
    }
    let ft = ft as usize;
    let Some(s) = state(world, id) else { return };
    if s.slots.get(ft) != Some(&0) {
        return;
    }
    let mut best: Option<usize> = None;
    let mut max_kind = 0u16;
    for k in (0..inf.tops.len()).rev() {
        let x = s.slots.get(k).copied().unwrap_or(0);
        if inf.tops[k] && x != 0 {
            let kind = world.entities.by_index(x as u32).map_or(0, |e| e.kind);
            if max_kind < kind {
                best = Some(k);
                max_kind = kind;
            }
        }
    }
    let Some(k) = best else { return };
    let soldier = s.slots[k];
    if let Some(s) = st(world, id) {
        s.slots[k] = 0;
        s.slots[ft] = soldier;
    }
    let dir = inf.dirs.get(ft).copied().unwrap_or(0);
    let waiting = jcore::job(world, soldier).is_some_and(|j| j.task == crate::jobs::task::task_type::WAIT);
    if let Some(EntityData::Settler(s)) = world.entities.by_index_mut(soldier as u32).map(|e| &mut e.data) {
        if waiting {
            s.direction = dir;
        }
        s.job.tower.slot = ft as u8;
        s.job.tower.state = 1;
        s.job.tower.dir = dir;
        s.job.tower.tick = 0;
    }
}

/// 0x548790: the door guard goes back into its slot (off the map, its wait list, not
/// vulnerable, slot direction and state); with `new_door` a door settler is placed again.
fn guard_in(world: &mut World, id: u16, with_door: bool) {
    let Some(inf) = info(world, id) else { return };
    let Some(g) = state(world, id).map(|s| s.guard) else { return };
    if let Some(e) = world.entities.by_index_mut(g as u32) {
        // vtbl+0x38 (0x5670e0): off the map and out of the war map.
        e.flags |= flag::ON_BOARD;
        let pos = e.pos;
        if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
            world.layers.map_object[n] = 0;
        }
    }
    if world.warmap.contains(g) {
        crate::warmap::remove(world, g);
    }
    world.unregister_from_logic_update(g);
    jcore::take_wait_list(world, g);
    if let Some(e) = world.entities.by_index_mut(g as u32) {
        e.flags &= !flag::VULNERABLE_MASK;
    }
    let k = jcore::job(world, g).map_or(0, |j| j.tower.slot as usize);
    put_in_slot(world, g, k, &inf);
    if let Some(s) = st(world, id) {
        s.guard = 0;
    }
    if with_door && let Some((_, door, owner)) = building(world, id) {
        new_door(world, id, door, owner, false);
    }
}

/// 0x54a120: after the door settler is gone a soldier guards the door — the last swordsman, else
/// the last bowman (the counts of a kind found empty are zeroed) — standing on the door node,
/// facing the first unblocked neighbour direction of 5..0 (else 2), state 3. Without soldiers
/// the building is no longer inhabited: an enemy swordsman or bowman next to the door (spiral
/// radius 3) captures it (orders for one soldier of its kind, then 0x582ea0), and a new door
/// settler is placed either way.
fn new_guard(world: &mut World, id: u16) {
    let Some(inf) = info(world, id) else { return };
    let Some((_, door, owner)) = building(world, id) else { return };
    let Some(s) = state(world, id) else { return };
    let count = inf.tops.len();
    let mut found = (0..count).rev().find(|&k| !inf.tops[k] && s.slots.get(k).is_some_and(|&x| x != 0));
    if found.is_none() {
        if let Some(s) = st(world, id) {
            s.sword_in = 0;
        }
        let s = state(world, id).expect("checked");
        found = (0..count).rev().find(|&k| inf.tops[k] && s.slots.get(k).is_some_and(|&x| x != 0));
        if found.is_none()
            && let Some(s) = st(world, id)
        {
            s.bow_in = 0;
        }
    }
    let g = found.and_then(|k| state(world, id).map(|s| s.slots[k])).unwrap_or(0);
    if let Some(s) = st(world, id) {
        s.guard = g;
    }
    if let Some(k) = found {
        let dir = (0..6usize)
            .rev()
            .find(|&d| {
                let (dx, dy) = crate::jobs::walking::NEIGHBOR[d];
                world.grid.checked_index(door.x() + dx, door.y() + dy).is_some_and(|n| world.layers.flags[n] & 1 == 0)
            })
            .unwrap_or(2) as u8;
        world.unregister_from_logic_update(g);
        if let Some(e) = world.entities.by_index_mut(g as u32) {
            e.flags = (e.flags & 0xfffd_fffd) | 0x1_010d;
        }
        // vtbl+0x34 (0x567080): on the map at the door, in the war map.
        if let Some(e) = world.entities.by_index_mut(g as u32) {
            e.flags &= !flag::ON_BOARD;
            e.pos = door;
        }
        if let Some(n) = world.grid.checked_index(door.x(), door.y()) {
            world.layers.map_object[n] = g;
        }
        crate::warmap::readd(world, g);
        jcore::take_wait_list(world, g);
        let tick = world.tick;
        if let Some(EntityData::Settler(s)) = world.entities.by_index_mut(g as u32).map(|e| &mut e.data) {
            s.direction = dir;
            s.job.tower.dir = dir;
            s.job.tower.slot = k as u8;
            s.job.tower.state = 3;
            s.job.tower.tick = tick;
        }
        return;
    }
    if let Some(r) = super::role_mut(world, id) {
        r.base.worker_in_house = false;
    }
    // An enemy soldier next to the door (spiral iterator 0x547f20, radius 3) takes the building.
    let enemy = crate::spiral::walk(world.grid, door.x(), door.y(), 3).find_map(|(x, y)| {
        let n = world.grid.index(x as u32, y as u32);
        let m = world.layers.map_object[n];
        let e = world.entities.by_index(m as u32).filter(|e| e.obj_type == ObjType::Settler)?;
        let wt = e.warrior_type();
        ((wt == 2 || wt == 3) && e.flags & 0x2020 == 0x2000 && crate::warmap::hostile(world, owner, e.owner)).then_some((m, wt))
    });
    match enemy {
        Some((soldier, wt)) => {
            new_door(world, id, door, owner, true);
            // 0x54a38d: `NotifyAndDetachAll` — the soldiers on their way are free and detached.
            super::notify_and_detach_all(world, id, EntityEvent { event: 9, kind: 0, data: [id as i32, 0, 0] });
            if let Some(s) = st(world, id) {
                s.bow_coming = 0;
                s.sword_coming = 0;
                s.bow_wish = 0;
                s.sword_wish = 0;
                if wt == 2 && inf.max_sword != 0 && s.sword_in < inf.max_sword {
                    s.sword_wish = 1;
                    s.sword_coming = 1;
                } else if wt == 3 && inf.max_bow != 0 && s.bow_in < inf.max_bow {
                    s.bow_wish = 1;
                    s.bow_coming = 1;
                }
            }
            send_soldier(world, id, soldier);
        }
        None => new_door(world, id, door, owner, false),
    }
}

/// Role +0x64 (0x549390, `CBuilding::CrushBuilding`): the door guard goes in, the door settler
/// is killed, every soldier leaves (0x549800).
pub fn all_leave(world: &mut World, id: u16) {
    if state(world, id).is_some_and(|s| s.guard != 0) {
        guard_in(world, id, false);
    }
    kill_door(world, id);
    let n = state(world, id).map_or(0, |s| s.slots.len());
    for k in 0..n {
        if state(world, id).is_some_and(|s| s.slots[k] != 0) {
            soldier_out(world, id, k);
            if let Some(s) = st(world, id) {
                s.slots[k] = 0;
            }
        }
    }
}

/// Role +0x2c (0x549370), after the crushed or destroyed building was killed: the territory
/// around it (0x60cd30, `territory::update_owner`, which destroys buildings on land that
/// changes hands).
pub fn crushed(world: &mut World, id: u16) {
    let Some((pos, _, _)) = building(world, id) else { return };
    crate::territory::update_owner(world, pos.x(), pos.y());
}

/// `ConvertEventIntoGoal` (0x54a560): event 0x12 fills the building (wish = slots); event 0x13
/// lowers a wish (data0 < 0: everyone out but one — orders cancelled, one swordsman wished if
/// one is inside, else one bowman if any is inside or coming; data1 = 0 swordsmen, else bowmen,
/// never below 1 unless the other kind is wished and present). Entity flag bit 31 is set.
pub fn convert_event_into_goal(world: &mut World, id: u16, ev: &EntityEvent) {
    let Some(inf) = info(world, id) else { return };
    match ev.event {
        0x12 => {
            if let Some(s) = st(world, id) {
                s.bow_wish = inf.max_bow;
                s.sword_wish = inf.max_sword;
            }
        }
        0x13 if ev.data[0] < 0 => {
            cancel_orders(world, id);
            if let Some(s) = st(world, id) {
                if s.sword_in == 0 && (s.sword_coming == 0 || s.bow_in != 0) {
                    let one_bow = s.bow_in != 0 || s.bow_coming != 0;
                    s.bow_wish = one_bow as u8;
                    s.sword_wish = 0;
                } else {
                    s.bow_wish = 0;
                    s.sword_wish = 1;
                }
            }
        }
        0x13 => {
            if let Some(s) = st(world, id) {
                if ev.kind == 0 {
                    if s.sword_wish > 1 || (s.sword_wish == 1 && s.bow_wish != 0 && s.bow_in != 0) {
                        s.sword_wish -= 1;
                    }
                } else if s.bow_wish > 1 || (s.bow_wish == 1 && s.sword_wish != 0 && s.sword_in != 0) {
                    s.bow_wish -= 1;
                }
            }
        }
        _ => {}
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags |= jcore::FLAG_CONTINUE_AFTER_EVENT;
    }
}

/// `HaveInhabitant` of a military building (`IBuildingRole +0x1d`).
pub fn is_inhabited(world: &World, id: u16) -> bool {
    state(world, id).is_some() && inhabited(world, id)
}

/// Soldiers in the building's slots, slot order (0 = empty slot).
pub fn slots(world: &World, id: u16) -> &[u16] {
    state(world, id).map_or(&[], |s| s.slots.as_slice())
}
