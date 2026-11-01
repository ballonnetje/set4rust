//! `CWorkshopBuildingRole` (house kind 5: the vehicle hall and the shipyards), analysed from the
//! shipped exe (vtable 0x1059610, 0x558500..0x559a00, decompile `tools/re/vehicles/workshop.c`).
//! The player orders vehicles per type (command 5040, `TakeOrder` 0x5590b0: five counters
//! +0x190, 100 = endless); the role picks the next ordered type round robin (0x559140), places
//! the vehicle under construction at the building's `workingpos` (`AddVehicle(.., 1)`, +0x17e)
//! and sends its worker (a house worker) with event 4 to bring it boards and ironbars from the
//! input piles; each PRODUCE_VIRTUAL adds one good ([`crate::vehicles::add_building_material`])
//! and the vehicle finishes itself when the progress reaches its end (`CVehicle` 0x594f60 →
//! 0x594830). Command 5030 (`CancelCurrentProduction` 0x559740) destroys it.
//!
//! `LogicUpdate` (0x558e20): 1 order the inhabitant; 2 production delay; 3 when the worker is
//! home and the building works: no vehicle → make the next one; else the first input pile
//! (buildingInfo order) whose good the vehicle still needs and that has a good not yet taken →
//! event 4 `[0, vAnimListId[i], vehicle]` to the worker, piles locked; a vehicle no longer
//! BIRTH is forgotten.
//!
//! Not simulation: the dialog packets (0x559930: type 9 with the piles, the vehicle's type
//! +0x17c and its progress — with `p` = vehicle +0x70 and `end` = +0x72, in f32
//! `r = (p + end) / 100` (1.0 when 0, the original stores it in the vehicle +0x7c) and
//! `trunc(100 − (end − p) / r)` clamped to 0..=100; 0x5597b0: sidebar type 0x11 with the order
//! counts), the AI notice
//! (0x146b148 vtbl+0x2c(7, player, building, 0)) when a worker enters with no order.
//! [`post_load`] (vtbl+0x1c, 0x5588e0) for a map's shipyards; after one of our own loads the
//! water flags come from the save.

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Building entity flag 0x1000: the building works.
const FLAG_WORKING: u32 = 0x1000;
/// Pile entity flag 0x10 (`LockPiles` 0x559680).
const PILE_LOCKED: u32 = 0x10;
/// The order slots (+0x190..+0x194); slot `i` orders vehicle type `i + 1` (.rdata 0x101a3c0,
/// type → slot pairs at 0x1165134).
pub const SLOTS: usize = 5;
/// `TakeOrder` amount meaning "endless".
pub const ENDLESS: u8 = 100;

/// `CWorkshopBuildingRole` (0x198 bytes): +4 / +5 state and delay ([`WorkBase`]), +0x17c the
/// type being made, +0x17d the next slot to look at, +0x17e the vehicle under construction,
/// +0x180 (only cleared and saved), +0x184 the input piles, +0x190 the order counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    pub kind: u8,
    pub next: u8,
    pub vehicle: u16,
    pub x180: i32,
    pub piles: Vec<u16>,
    pub orders: [u8; SLOTS],
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.work.hash(h);
        h.u8(self.kind);
        h.u8(self.next);
        h.u16(self.vehicle);
        h.u32(self.x180 as u32);
        h.u16s(&self.piles);
        h.bytes(&self.orders);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::Workshop(s) } => Some((base, s)),
        _ => None,
    }
}

/// The workshop's role state (the dialogs read it).
pub fn state(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Workshop(s), .. } => Some(s),
        _ => None,
    }
}

fn info(world: &World, id: u16) -> Option<&work::WorkInfo> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    world.rules.work.info(e.race, d.type_ex)
}

/// The `workingpos` of the building (`pos + info +0x2a / +0x2b`, 0x559200): where its vehicles
/// are made.
fn working_pos(world: &World, id: u16) -> Option<(i32, i32)> {
    let e = world.entities.by_index(id as u32)?;
    let EntityData::Building(d) = &e.data else { return None };
    let (dx, dy) = world.rules.building(e.race, d.type_ex).working;
    Some((e.x() + dx as i32, e.y() + dy as i32))
}

/// `CBuilding::PostLoad` (vtbl+0xc, 0x536170) of a workshop: flags 0x600 off, out of and into the
/// war map, the role's `PostLoad` (vtbl+0x1c, 0x5588e0: with the water masks (info +6), a
/// shipyard (0x3a..=0x3f, 0x4c, 0x4d) whose `workingpos` is water (water flags above 0xbfff) gets
/// water bit 0x1000 there), then for a READY building with water masks 0x541100 (the +0x314
/// cells' `SetBlockedWater` again, a no-op after `SetToWorld`). `LoadBuildingData` (0x53d940)
/// calls it for a map's shipyards.
pub fn post_load(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags &= !0x600;
    let EntityData::Building(d) = &e.data else { return };
    let (race, t) = (e.race, d.type_ex);
    crate::warmap::readd(world, id);
    if !world.rules.building(race, t).has_water_masks || !matches!(t, 0x3a..=0x3f | 0x4c | 0x4d) {
        return;
    }
    if let Some((x, y)) = working_pos(world, id)
        && let Some(n) = world.grid.checked_index(x, y)
        && world.tiling.water_flags[n] > 0xbfff
    {
        world.tiling.water_flags[n] |= 0x1000;
    }
}

/// `CWorkshopBuildingRole::Init` (0x5589b0, vtbl+0x18), after `IBuildingRole::InitCommon`: the
/// buildingInfo piles (`AddPile` 0x5708a0 + building vtbl+0x74; a patch pile is hidden), state
/// 1, first update in 2 ticks; the working-area centre (+0x4c) becomes the `workingpos`; the
/// ensign node (+0x48) gets move cost 7 and node flag 0x02.
pub fn init(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y) = (e.x(), e.y());
    let ensign = match &e.data {
        EntityData::Building(d) => d.ensign,
        _ => return,
    };
    let records = info(world, id).map(|i| i.piles.clone()).unwrap_or_default();
    let mut piles = Vec::new();
    for p in records {
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
        piles.push(pile);
    }
    if let Some((_, s)) = role(world, id) {
        *s = State { work: WorkBase { state: 1, ..WorkBase::default() }, piles, ..State::default() };
    }
    world.register_for_logic_update(id, 2);
    if let Some((wx, wy)) = working_pos(world, id)
        && let Some(EntityData::Building(d)) = world.entities.by_index_mut(id as u32).map(|e| &mut e.data)
    {
        d.working_area = crate::Y16X16::pack(wx, wy);
    }
    if let Some(n) = world.grid.checked_index(ensign.x(), ensign.y()) {
        world.layers.walk_move[n] |= 7;
        world.layers.flags[n] |= 2;
    }
}

/// `LockPiles(lock)` (0x559680, vtbl+0x48): flag 0x10 on every input pile.
fn lock_piles(world: &mut World, id: u16, lock: bool) {
    let piles = state(world, id).map(|s| s.piles.clone()).unwrap_or_default();
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

/// `TakeOrder(type, amount)` (0x5590b0, command 5040): 0 clears the type's counter; 100, or a
/// counter already at 100, makes it endless (100); else the sum, at least 0 and at most 99.
/// Types outside 1..=5 change nothing here (the original's lookup ends on the table's {0, -1}
/// terminator and writes the byte before the counters, +0x18f [inf]).
pub fn take_order(world: &mut World, id: u16, kind: u16, amount: i32) {
    if !(1..=SLOTS as u16).contains(&kind) {
        return;
    }
    let Some((_, s)) = role(world, id) else { return };
    let c = &mut s.orders[kind as usize - 1];
    *c = if amount == 0 {
        0
    } else if amount == ENDLESS as i32 || *c >= ENDLESS {
        ENDLESS
    } else {
        (*c as i32 + amount).clamp(0, 99) as u8
    };
}

/// 0x559140: from slot +0x17d on, the first slot with an order: +0x17c its type, +0x17d the
/// slot after it. None: +0x17c = 0.
fn next_kind(s: &mut State) -> bool {
    let mut slot = s.next as usize % SLOTS;
    for _ in 0..SLOTS {
        if s.orders[slot] != 0 {
            s.kind = slot as u8 + 1;
            s.next = ((slot + 1) % SLOTS) as u8;
            return true;
        }
        slot = (slot + 1) % SLOTS;
    }
    s.kind = 0;
    false
}

/// 0x559240: the direction a new vehicle faces, by building type (the shipyards 0x3a..=0x3e,
/// 0x4c, 0x4d; -1 = random); every other type 2.
fn new_dir(type_ex: u8) -> i32 {
    match type_ex {
        0x3a => 4,
        0x3b | 0x3e => -1,
        0x3c => 5,
        0x3d => 1,
        0x4c => 0,
        0x4d => 3,
        _ => 2,
    }
}

fn is_birth(world: &World, vehicle: u16) -> bool {
    world.entities.by_index(vehicle as u32).is_some_and(|e| e.flags & flag::BIRTH != 0)
}

/// State 3 of `LogicUpdate`. Returns whether the worker was sent (no re-registration then).
fn produce(world: &mut World, id: u16) -> bool {
    let Some(b) = world.entities.by_index(id as u32) else { return false };
    let (working, owner) = (b.flags & FLAG_WORKING != 0, b.owner);
    let type_ex = match &b.data {
        EntityData::Building(d) => d.type_ex,
        _ => return false,
    };
    let home = super::role(world, id).is_some_and(|r| r.base.worker_in_house);
    if !(home && working) {
        return false;
    }
    let vehicle = state(world, id).map_or(0, |s| s.vehicle);
    if vehicle == 0 {
        let Some((_, s)) = role(world, id) else { return false };
        if !next_kind(s) {
            return false;
        }
        let kind = s.kind as u16;
        let Some((x, y)) = working_pos(world, id) else { return false };
        if !crate::vehicles::can_place(world, x, y, kind) {
            return false;
        }
        let v = crate::vehicles::add_vehicle(world, x, y, owner, kind, new_dir(type_ex), true).unwrap_or(0);
        if let Some((_, s)) = role(world, id) {
            s.vehicle = v;
        }
        if v != 0 {
            take_order(world, id, kind, -1);
        }
        return false;
    }
    let (records, lists) = info(world, id).map(|i| (i.piles.clone(), i.anim_lists.clone())).unwrap_or_default();
    let (boards, iron) = crate::vehicles::get(world, vehicle).map_or((0, 0), |v| (v.build[1], v.build[3]));
    for (i, p) in records.iter().enumerate() {
        let needed = match p.good {
            7 => boards,
            20 => iron,
            _ => continue,
        };
        if needed == 0 {
            continue;
        }
        let pile = pile_with_good(world, id, p.good);
        if crate::pile::get(world, pile).is_some_and(|d| d.leaving < d.amount) {
            let list = lists.get(i).copied().unwrap_or(0);
            let settler = super::role(world, id).map_or(0, |r| r.base.inhabitant);
            crate::jobs::core::set_event(world, settler, EntityEvent { event: 4, kind: 0, data: [0, list as i32, vehicle as i32] });
            lock_piles(world, id, true);
            return true;
        }
    }
    if !is_birth(world, vehicle)
        && let Some((_, s)) = role(world, id)
    {
        s.vehicle = 0;
    }
    false
}

/// `CWorkshopBuildingRole::LogicUpdate` (0x558e20, vtbl+0xc).
pub fn logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let Some(s) = state(world, id) else { return };
    match s.work.state {
        1 => {
            if work::order_inhabitant(world, id) {
                if let Some((_, s)) = role(world, id) {
                    s.work.state = 3;
                }
                return;
            }
        }
        2 => {
            let Some((_, s)) = role(world, id) else { return };
            if s.work.delay > delay {
                s.work.state = 3;
                s.work.delay = 0;
                world.register_for_logic_update(id, 1);
            } else {
                s.work.delay += 1;
                world.register_for_logic_update(id, 14);
            }
            return;
        }
        3 => {
            if produce(world, id) {
                return;
            }
        }
        _ => return,
    }
    world.register_for_logic_update(id, 31);
}

/// `CWorkshopBuildingRole::SettlerEnter` (0x558c10, vtbl+0x4c): as the production role's (the
/// first entry makes the worker the inhabitant and starts the piles with event 8; the worker is
/// hidden at the door's in-house offset); a vehicle under construction that is no longer BIRTH
/// is forgotten (+0x17e, +0x17c = 0); with no order at all the AI hears of it (event 7); then
/// the production delay (state 2) or 31 ticks, and the piles unlocked.
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
    let (dx, dy) = (door.x() - bx, door.y() - by);
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.displacement_costs = 10;
        let one = crate::jobs::state::OFFSET_ONE;
        j.offset = ((24 * dx - 12 * dy) * one, 12 * dy * one);
    }
    let vehicle = state(world, id).map_or(0, |s| s.vehicle);
    let done = vehicle != 0
        && world.entities.by_index(vehicle as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle) && e.flags & flag::BIRTH == 0);
    if done && let Some((_, s)) = role(world, id) {
        s.vehicle = 0;
        s.kind = 0;
    }
    // 0x558d78..0x558dab: with no order in any slot, the AI notice 7 (owner, building, 0) — the
    // eco-sector AIs take it (their productivity control of the building).
    if state(world, id).is_some_and(|s| s.orders.iter().all(|&n| n == 0)) {
        let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
        crate::ai::post_event(world, 7, owner as i32, id as i32, 0);
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

/// 0x5594c0 (vtbl+0x38) / 0x559610 (vtbl+0x34): the first input pile holding `good`.
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    let Some(s) = state(world, id) else { return 0 };
    s.piles.iter().copied().find(|&p| crate::pile::get(world, p).is_some_and(|d| d.good == good)).unwrap_or(0)
}

/// 0x559590 (vtbl+0x40): the first input pile of `good` with room (amount + coming below 8).
pub fn pile_with_need_for_good(world: &World, id: u16, good: u8) -> u16 {
    let Some(s) = state(world, id) else { return 0 };
    s.piles
        .iter()
        .copied()
        .find(|&p| crate::pile::get(world, p).is_some_and(|d| d.good == good && (d.amount as u32 + d.coming as u32) < 8))
        .unwrap_or(0)
}

/// 0x5596e0 (vtbl+0x2c, crush) and `CancelCurrentProduction` (0x559740, command 5030, with
/// `register`): the vehicle under construction goes up in effect 0x3e (sound 3) and is deleted
/// (0x597100); the cancel then updates the building in 1 tick. The original reads the vehicle
/// unchecked (a vehicle already gone would crash it); here it is only forgotten then.
fn drop_vehicle(world: &mut World, id: u16, register: bool) {
    let vehicle = state(world, id).map_or(0, |s| s.vehicle);
    if vehicle == 0 {
        return;
    }
    if let Some(e) = world.entities.by_index(vehicle as u32) {
        let (x, y) = (e.x(), e.y());
        world.events.push(crate::event::SimEvent::Effect { effect: 0x3e, sound: 3, x, y });
        crate::vehicles::delete(world, vehicle);
    }
    if let Some((_, s)) = role(world, id) {
        s.vehicle = 0;
    }
    if register {
        world.register_for_logic_update(id, 1);
    }
}

/// Role vtbl+0x2c (0x5596e0): the building is crushed or destroyed.
pub fn crush(world: &mut World, id: u16) {
    drop_vehicle(world, id, false);
}

/// `CancelCurrentProduction` (0x559740, command 5030).
pub fn cancel_production(world: &mut World, id: u16) {
    drop_vehicle(world, id, true);
}
