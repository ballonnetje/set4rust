//! Entities (`IEntity` and subclasses) and their arena (`CMapObjectMgr`, MapObjects/MapObjectMgr.cpp).
//!
//! The original has one id space for every object: `m_vEntities[0xFFFF + 1]` of `IEntity*`, id 0
//! reserved. Each entity also carries `m_iUniqueId` from a global counter; `Kill` ORs in
//! `0x20000000` so stale references can be detected. [`Entities`] keeps exactly that: slot index =
//! original entity id, generation = unique id (with the same dead bit), so an [`EntityId`] held
//! across a kill/reuse no longer resolves.
//!
//! **Id order is behaviour.** The original walks entities by id in several places (searches over
//! `m_vEntities`, per-type lists built in creation order, "first free carrier"), and RNG draws
//! happen in creation order. So the allocation policy is copied exactly: `GetFreeSlot` returns the
//! lowest free id `>= m_iMinFreeId`, `Destroy` lowers `m_iMinFreeId`, killed entities are destroyed
//! (their id freed) only 31 ticks later (see [`crate::Schedule`]). The 6 placeholder settlers the
//! `CMapObjectMgr` constructor creates are reproduced, so map entities start at id 7 as in the game.

use crate::grid::Y16X16;
use crate::hash::StateHasher;

/// `MAX_ENTRIES`.
pub const MAX_ENTRIES: u32 = 0xFFFF;
/// `IEntity::UNIQUE_ID_DEAD_ENTITY_BIT`.
pub const UID_DEAD: u32 = 0x2000_0000;

/// `IEntity::EntityType` (`m_objType`, IEntity.h). Observed: settlers 1 (`CSettler` ctor), buildings
/// 8 (`CBuilding` ctor), decos 32 (`IDecoObject` ctor, `CDecoObjMgr::GetDecoObjPtr`), flying 64
/// (`CFlyingMgr`, whose assert calls it `DECO_OBJ`), animals 128 (`CAnimalMgr`), piles 16
/// (`CObserverList`). Ships 2 / land vehicles 4 per IEntity.h (`Kill` counts both as vehicles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum ObjType {
    /// The `CMapObjectMgr` placeholder settlers are constructed with `m_objType = 0`.
    None = 0,
    Settler = 1,
    Ship = 2,
    LandVehicle = 4,
    Building = 8,
    Pile = 16,
    Deco = 32,
    Flying = 64,
    Animal = 128,
}

/// `EntityFlag` bits (IEntity.h); low 4 bits hold the warrior type (`CSettler` ctor ORs in
/// `CSettlerMgr::SettlerWarriorType`).
pub mod flag {
    pub const WARRIOR_TYPE_MASK: u32 = 0x0F;
    pub const ATTACHED: u32 = 0x20;
    pub const OFFERED: u32 = 0x40;
    pub const VISIBLE: u32 = 0x100;
    pub const SELECTED: u32 = 0x200;
    pub const SELECTABLE: u32 = 0x2000;
    pub const ON_BOARD: u32 = 0x8000;
    pub const VULNERABLE_MASK: u32 = 0x30000;
    pub const MAGIC_INVISIBLE: u32 = 0x80000;
    pub const BIRTH: u32 = 0x100_0000;
    pub const READY: u32 = 0x200_0000;
    pub const ALIVE_MASK: u32 = 0x300_0000;
    pub const DIED: u32 = 0x400_0000;
    /// Queued in a logic-update slot.
    pub const REGISTERED: u32 = 0x2000_0000;
    pub const GLOBAL_OFFERED: u32 = 0x4000_0000;
}

/// Handle to an entity: original id + unique id. Resolves only while that exact entity lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityId {
    pub index: u16,
    pub uid: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entity {
    /// `m_iUniqueId`.
    pub uid: u32,
    pub obj_type: ObjType,
    /// `m_nType`: settler / building / object / good / vehicle type, by `obj_type`.
    pub kind: u16,
    pub owner: u8,
    pub race: u8,
    /// `m_uPackedXY`. Static deco instances sit at (0, 0) (`IDecoObject` ctor).
    pub pos: Y16X16,
    /// [`flag`] bits.
    pub flags: u32,
    /// `IAnimatedEntity::m_uLastLogicUpdate`: slot of the last `RegisterForLogicUpdate`
    /// (`None` = -1, never registered or unregistered).
    pub last_slot: Option<u8>,
    /// `IAnimatedEntity::m_wNextEntity` / `m_wPrevEntity`: the one intrusive list an entity can be
    /// in (eco-sector offer lists of settlers and piles, `CSettlerMgr` per-type lists of
    /// warriors, `CBuildingMgr` per-type lists). 0 = none.
    pub next: u16,
    pub prev: u16,
    pub data: EntityData,
}

impl Entity {
    pub fn new(obj_type: ObjType, kind: u16, owner: u8, race: u8, pos: Y16X16, data: EntityData) -> Entity {
        Entity { uid: 0, obj_type, kind, owner, race, pos, flags: 0, last_slot: None, next: 0, prev: 0, data }
    }
    pub fn x(&self) -> i32 {
        self.pos.x()
    }
    pub fn y(&self) -> i32 {
        self.pos.y()
    }
    pub fn is_dead(&self) -> bool {
        self.uid & UID_DEAD != 0
    }
    /// `IEntity::WarriorType`.
    pub fn warrior_type(&self) -> u8 {
        (self.flags & flag::WARRIOR_TYPE_MASK) as u8
    }
}

/// Per-class state. Fields are added as systems are ported; each names the original member.
/// `Settler` (the job state of every role) is by far the largest variant; it is kept inline for
/// now (boxing it would touch every settler match in the workspace).
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[allow(clippy::large_enum_variant)]
pub enum EntityData {
    /// One of the six `CSettler(0x44, id)` placeholders from the `CMapObjectMgr` constructor.
    Placeholder,
    Settler(SettlerData),
    Building(BuildingData),
    Pile(PileData),
    Deco(DecoData),
    Vehicle(VehicleData),
    Animal(crate::animals::AnimalData),
    /// `IFlyingEntity` / `CManakopter` ([`crate::flying`]).
    Flying(Box<crate::flying::Flyer>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SettlerData {
    /// `m_cDirection`, 0..5 (`CSettler` ctor: `Rand() % 6`).
    pub direction: u8,
    /// `m_cHealth` (`SettlerInfo::m_bHealth` at creation).
    pub health: u8,
    /// `CSettlerMgr::AddSettler` gives a carrier on foreign land a `CFleeRole` instead of its normal role.
    pub fleeing: bool,
    /// Job execution state (`crate::jobs`).
    pub job: crate::jobs::JobState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuildingData {
    /// `CBuilding +0x40` (release ctor 0x535b40): the type as placed (ports/shipyards keep their
    /// race variant here; `kind` holds the generic 32 / 31).
    pub type_ex: u8,
    /// `+0x44`, `DoorPackedXY` = position + buildingInfo door offset (info +0x28/+0x29).
    pub door: Y16X16,
    /// `+0x48`, `EnsignPackedXY` = position + buildingInfo flag offset (info +0x26/+0x27).
    pub ensign: Y16X16,
    /// `+0x4c`, working-area centre; initialised to the ensign, set by `SetWorkingAreaPackedXY`.
    pub working_area: Y16X16,
    /// `IEntity +0x21` health: 0 for a construction site, `IBuildingRole::InitCommon` (0x540b20)
    /// adds the info hit points (`CBuilding` vtbl+0x18 0x566fb0, saturating at 255); vtbl+0x20
    /// (0x566fe0) takes damage and kills at 0.
    pub health: u8,
    /// `IAnimatedEntity` event queue (`SetEvent` 0x532e60; processed by `CBuilding::LogicUpdate`
    /// 0x537180 before the role's update).
    pub events: Vec<crate::jobs::state::EntityEvent>,
}

/// `CPile` (MapObjects/Pile/Pile.cpp); offsets are the debug build's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PileData {
    /// +68 good type.
    pub good: u8,
    /// +69 amount (0..=8).
    pub amount: u8,
    /// +70 goods reserved by carriers about to take them.
    pub leaving: u8,
    /// +71 goods on their way here.
    pub coming: u8,
    /// +72 `IPileRole` type ([`crate::pile::role`]).
    pub role: u8,
    /// +73 patch pile (drawn as a ground patch).
    pub patch: u8,
    /// +74 graphics variant.
    pub variant: u8,
    /// +76 / +78 patch offsets.
    pub offset: (i16, i16),
    /// +80 offer flag: offer the goods when there are any.
    pub offer: bool,
    /// +82 building the pile belongs to (its observer target 0), 0 = none.
    pub building: u16,
    /// +88 `CPileObserverList` (at most 8).
    pub observers: Vec<crate::pile::PileObserver>,
    /// `IAnimatedEntity` event list (events 7 / 8 from the building).
    pub events: Vec<crate::jobs::state::EntityEvent>,
    /// `CTradePileRole` +8 / +12 / +16 (trade piles: storage areas, marketplaces, ports): the
    /// trade role (0 free, 1 / 3 incoming, 2 stored, 4 reserved), goods expected, goods reserved.
    pub trade_type: u8,
    pub expected: i32,
    pub reserve: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecoData {
    /// `IDecoObject +68`: one instance shared by many nodes.
    pub static_instance: bool,
    /// `+36`: current animation frame.
    pub frame: u8,
    /// `+73`: growth stage (`TGROWN_STATE`; 3 = grown for trees/plants/mushrooms).
    pub stage: u8,
    /// `+74`: frames of the current animation cycle (`GetObjectFrameCount`).
    pub cycle_frames: u8,
    /// `CStone +33`: stone amount (1..13).
    pub amount: u8,
    /// `CTree +76` / `CPlant`: logic updates spent in the current growth stage.
    pub counter: i32,
    /// `CHive`: the gfx job +0x26 as an offset from the type's first job (a hive's stages do not
    /// map to `first + stage`); 0 for the other classes.
    pub job: u8,
    /// `CHive +0x4c`: updates left as a ready hive (406 from the constructor).
    pub timer: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VehicleData(pub Box<crate::vehicles::Vehicle>);

/// The entity arena: `CMapObjectMgr::m_vEntities` + its id bookkeeping.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Entities {
    slots: Vec<Option<Entity>>,
    /// `m_iMinFreeId`.
    min_free: u32,
    /// `m_iLastUsedId`.
    last_used: u32,
    /// `m_iMaxLastUsedId`.
    max_last_used: u32,
    /// `m_iCurrentUniqueId`.
    next_uid: u32,
    /// Occupied slots (derived, kept for O(1) [`Entities::len`]).
    used: u32,
}

impl Default for Entities {
    fn default() -> Self {
        Self::new()
    }
}

impl Entities {
    /// `CMapObjectMgr::CMapObjectMgr`: empty table, then six placeholder settlers (`CSettler(0x44, id)`,
    /// ids 1..6, `m_objType` 0, never on the map or in the schedule).
    pub fn new() -> Entities {
        let mut e = Entities {
            slots: vec![None; MAX_ENTRIES as usize + 1],
            min_free: 1,
            last_used: 0,
            max_last_used: 0,
            next_uid: 1,
            used: 0,
        };
        for _ in 0..6 {
            let p = Entity::new(ObjType::None, 0x44, 0, 0, Y16X16(0), EntityData::Placeholder);
            e.alloc(p).expect("empty table");
        }
        e
    }

    /// `CMapObjectMgr::GetFreeSlot` (release 0x56cf10): lowest free id `>= m_iMinFreeId`; remembers it as
    /// the new minimum. Does not reserve the slot.
    pub fn free_slot(&mut self) -> Option<u16> {
        for i in self.min_free..MAX_ENTRIES {
            if self.slots[i as usize].is_none() {
                self.min_free = i;
                return Some(i as u16);
            }
        }
        self.min_free = MAX_ENTRIES;
        None
    }

    /// `CMapObjectMgr::RegisterEntity(id, e, true)` (release 0x56cfb0): store at `index`, assign the next
    /// unique id, track the highest used id.
    pub fn insert(&mut self, index: u16, mut e: Entity) -> EntityId {
        assert!(index != 0 && self.slots[index as usize].is_none(), "slot {index} not free");
        e.uid = self.next_uid;
        self.next_uid += 1;
        let i = index as u32;
        if i > self.last_used {
            self.last_used = i;
            self.max_last_used = self.max_last_used.max(i);
        }
        let id = EntityId { index, uid: e.uid };
        self.slots[index as usize] = Some(e);
        self.used += 1;
        id
    }

    /// `GetFreeSlot` + `RegisterEntity`, the pattern every manager's `Add*` uses.
    pub fn alloc(&mut self, e: Entity) -> Option<EntityId> {
        let i = self.free_slot()?;
        Some(self.insert(i, e))
    }

    /// The entity `id` refers to, if it still exists with that unique id.
    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.slots.get(id.index as usize)?.as_ref().filter(|e| e.uid == id.uid)
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.slots.get_mut(id.index as usize)?.as_mut().filter(|e| e.uid == id.uid)
    }

    /// `CMapObjectMgr::EntityPtr` (raw id as stored in maps and messages; ids beyond the table are
    /// out of range in the original too).
    pub fn by_index(&self, index: u32) -> Option<&Entity> {
        self.slots.get(index as usize)?.as_ref()
    }

    pub fn by_index_mut(&mut self, index: u32) -> Option<&mut Entity> {
        self.slots.get_mut(index as usize)?.as_mut()
    }

    pub fn id_of(&self, index: u16) -> Option<EntityId> {
        self.by_index(index as u32).map(|e| EntityId { index, uid: e.uid })
    }

    /// `CMapObjectMgr::Destroy` (release 0x56d200): free the slot, lower `m_iMinFreeId`, recompute
    /// `m_iLastUsedId`.
    pub fn destroy(&mut self, index: u16) {
        assert!(self.slots[index as usize].take().is_some(), "destroying empty slot {index}");
        self.used -= 1;
        let i = index as u32;
        if i < self.min_free {
            self.min_free = i;
        }
        let mut last = self.last_used;
        while last > 0 && self.slots[last as usize].is_none() {
            last -= 1;
        }
        self.last_used = last;
    }

    /// `CMapObjectMgr::AssignNewUniqueId`.
    pub fn assign_new_uid(&mut self, index: u16) {
        if let Some(e) = self.slots.get_mut(index as usize).and_then(Option::as_mut)
            && e.uid & UID_DEAD == 0
        {
            e.uid = self.next_uid;
            self.next_uid += 1;
        }
    }

    /// Live and dead-but-not-destroyed entities in id order.
    pub fn iter(&self) -> impl Iterator<Item = (u16, &Entity)> {
        self.slots[..=self.last_used as usize]
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (i as u16, e)))
    }

    pub fn len(&self) -> usize {
        self.used as usize
    }

    pub fn is_empty(&self) -> bool {
        self.last_used == 0
    }

    pub fn count(&self, t: ObjType) -> usize {
        self.iter().filter(|(_, e)| e.obj_type == t).count()
    }

    pub fn last_used(&self) -> u32 {
        self.last_used
    }

    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u32(self.min_free);
        h.u32(self.last_used);
        h.u32(self.max_last_used);
        h.u32(self.next_uid);
        for (i, e) in self.iter() {
            h.u16(i);
            hash_entity(e, h);
        }
    }
}

fn hash_entity(e: &Entity, h: &mut StateHasher) {
    h.u32(e.uid);
    h.u8(e.obj_type as u8);
    h.u16(e.kind);
    h.u8(e.owner);
    h.u8(e.race);
    h.u32(e.pos.0);
    h.u32(e.flags);
    h.opt_u8(e.last_slot);
    h.u16(e.next);
    h.u16(e.prev);
    match &e.data {
        EntityData::Placeholder => h.u8(0),
        EntityData::Settler(s) => {
            h.u8(1);
            h.u8(s.direction);
            h.u8(s.health);
            h.bool(s.fleeing);
            s.job.hash(h);
        }
        EntityData::Building(b) => {
            h.u8(2);
            h.u8(b.type_ex);
            h.u32(b.door.0);
            h.u32(b.ensign.0);
            h.u32(b.working_area.0);
            h.u8(b.health);
            h.u32(b.events.len() as u32);
            for ev in &b.events {
                h.u32(ev.event as u32);
                h.u32(ev.kind as u32);
                ev.data.iter().for_each(|&d| h.u32(d as u32));
            }
        }
        EntityData::Pile(p) => {
            h.u8(3);
            for v in [p.good, p.amount, p.leaving, p.coming, p.role, p.patch, p.variant] {
                h.u8(v);
            }
            h.u16(p.offset.0 as u16);
            h.u16(p.offset.1 as u16);
            h.bool(p.offer);
            h.u16(p.building);
            h.u32(p.observers.len() as u32);
            for o in &p.observers {
                h.u16(o.entity);
                h.u8(o.delta as u8);
                h.u8(o.target);
            }
            h.u8(p.trade_type);
            h.u32(p.expected as u32);
            h.u32(p.reserve as u32);
            h.u32(p.events.len() as u32);
            for ev in &p.events {
                h.u32(ev.event as u32);
                h.u32(ev.kind as u32);
                ev.data.iter().for_each(|&d| h.u32(d as u32));
            }
        }
        EntityData::Deco(d) => {
            h.u8(4);
            h.bool(d.static_instance);
            h.u8(d.frame);
            h.u8(d.stage);
            h.u8(d.cycle_frames);
            h.u8(d.amount);
            h.u32(d.counter as u32);
            h.u8(d.job);
            h.u32(d.timer as u32);
        }
        EntityData::Vehicle(v) => {
            h.u8(5);
            v.0.hash(h);
        }
        EntityData::Animal(a) => {
            h.u8(6);
            a.hash(h);
        }
        EntityData::Flying(f) => {
            h.u8(7);
            f.hash(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pile() -> Entity {
        Entity::new(ObjType::Pile, 1, 1, 0, Y16X16(0), EntityData::Pile(PileData { good: 1, amount: 1, role: 2, ..Default::default() }))
    }

    #[test]
    fn placeholders_take_ids_1_to_6() {
        let mut e = Entities::new();
        assert_eq!(e.count(ObjType::None), 6);
        assert_eq!(e.alloc(pile()).unwrap().index, 7);
    }

    #[test]
    fn lowest_free_id_is_reused_and_stale_handles_fail() {
        let mut e = Entities::new();
        let a = e.alloc(pile()).unwrap();
        let b = e.alloc(pile()).unwrap();
        let c = e.alloc(pile()).unwrap();
        assert_eq!((a.index, b.index, c.index), (7, 8, 9));
        e.destroy(b.index);
        e.destroy(a.index);
        // min_free dropped to 7: the next two allocations reuse 7 then 8, then continue at 10.
        let d = e.alloc(pile()).unwrap();
        let f = e.alloc(pile()).unwrap();
        let g = e.alloc(pile()).unwrap();
        assert_eq!((d.index, f.index, g.index), (7, 8, 10));
        assert!(e.get(a).is_none() && e.get(d).is_some());
        e.destroy(g.index);
        assert_eq!(e.last_used(), 9);
    }
}
