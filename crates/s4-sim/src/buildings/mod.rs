//! Building roles (`IBuildingRole` and subclasses, MapObjects/Building/*Role.cpp): dispatch, the
//! common `IBuildingRole` data and behaviour, the building's observer list, and the roles owned
//! here (storage, military, …). The worker-driving roles live in `gather`, `farm`, `workup` and
//! `production` (each a `State` plus free functions per overridden virtual); the construction-site
//! role and placement are in `build`.
//!
//! `CBuildingMgr::CreateBuildingRole(type, race, site)` (0x... decompiled) picks the role by the
//! building info's house kind (byte +477): 1 simple, 2 work-up, 3 animal ranch, 4 production,
//! 5 workshop, 6 gather, 7 mine, 8 farm, 9 lookout tower, 10 military, 11 barrack, 12 storage,
//! 13 trading, 14 residence, 15 castle, 16 small temple, 17 big temple, 18 dark temple,
//! 19 eye catcher, 20 mushroom farm, 21 manakopter hall. The role's `Init` (vtbl+24) runs in the
//! `CBuilding` constructor (map buildings) or in `BuildingDone` (finished sites).

use std::collections::BTreeMap;

use crate::entity::flag;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;
use crate::world::World;

pub mod barrack;
pub mod crush;
pub mod dark_temple;
pub mod farm;
pub mod gather;
pub mod lookout;
pub mod manakopter;
pub mod military;
pub mod mine;
pub mod mushfarm;
pub mod production;
pub mod residence;
pub mod search;
pub mod simple;
pub mod storage;
pub mod trader;
pub mod trading;
pub mod temple;
pub mod work;
pub mod workshop;
pub mod workup;

/// `HOUSE_KIND_*`.
pub mod house_kind {
    pub const SIMPLE: u8 = 1;
    pub const WORKUP: u8 = 2;
    pub const ANIMALRANCH: u8 = 3;
    pub const PRODUCTION: u8 = 4;
    pub const WORKSHOP: u8 = 5;
    pub const GATHER: u8 = 6;
    pub const MINE: u8 = 7;
    pub const FARM: u8 = 8;
    pub const LOOKOUTTOWER: u8 = 9;
    pub const MILITARY: u8 = 10;
    pub const BARRACK: u8 = 11;
    pub const STORAGE: u8 = 12;
    pub const TRADE: u8 = 13;
    pub const RESIDENCE: u8 = 14;
    pub const CASTLE: u8 = 15;
    pub const SMALLTEMPLE: u8 = 16;
    pub const BIGTEMPLE: u8 = 17;
    pub const DARKTEMPLE: u8 = 18;
    pub const EYECATCHER: u8 = 19;
    pub const MUSHROOMFARM: u8 = 20;
    pub const MANAKOPTERHALL: u8 = 21;
}

/// `CBuilding` flag 0x10000: the building stands finished (set after `SetToWorld` /
/// `BuildingDone`).
pub const FLAG_BUILT: u32 = 0x10000;

/// `IBuildingRole` data every role has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Base {
    /// House kind the role was created from.
    pub house_kind: u8,
    /// +29 `m_bWorkerInHouse`.
    pub worker_in_house: bool,
    /// The inhabitant settler (0 = none).
    pub inhabitant: u16,
    /// `CBuilding`'s `CObserverList`: (entity, target) of settlers and piles attached.
    pub observers: Vec<(u16, u8)>,
    /// Piles the role created, in creation order.
    pub piles: Vec<u16>,
}

impl Base {
    fn hash(&self, h: &mut StateHasher) {
        h.u8(self.house_kind);
        h.bool(self.worker_in_house);
        h.u16(self.inhabitant);
        h.u32(self.observers.len() as u32);
        for &(e, t) in &self.observers {
            h.u16(e);
            h.u8(t);
        }
        h.u16s(&self.piles);
    }
}

/// The role-specific part.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RoleKind {
    /// A role not ported yet (the `IBuildingRole` base behaviour only).
    Other,
    Gather(gather::State),
    Farm(farm::State),
    WorkUp(workup::State),
    Production(production::State),
    Storage(storage::State),
    Simple(simple::State),
    /// `CMilitaryBuildingRole` / `CCastleRole`.
    Military(military::State),
    Mine(mine::State),
    /// `CBarrackRole`.
    Barrack(barrack::State),
    Residence(residence::State),
    /// `CDarkMushroomFarmRole`.
    MushroomFarm(mushfarm::State),
    /// `CDarkTempleRole` (dark temple and fortress).
    DarkTemple(dark_temple::State),
    /// `CTradingBuildingRole` (marketplace, port).
    Trading(Box<trading::State>),
    /// `CLookoutTowerRole`.
    Lookout(lookout::State),
    /// `CSmallTempleRole` / `CBigTempleRole`.
    SmallTemple(temple::SmallState),
    BigTemple(temple::BigState),
    /// `CManakopterHallRole`.
    ManakopterHall(manakopter::State),
    /// `CWorkshopBuildingRole` (vehicle hall, shipyards).
    Workshop(workshop::State),
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Role {
    pub base: Base,
    pub kind: RoleKind,
}

/// The roles of all buildings that have one (construction sites have theirs in `build`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuildingRoles {
    pub roles: BTreeMap<u16, Role>,
    /// The `CInternationalTrader` table (0x14865b0) of the foreign trade ([`trader`]).
    #[cfg_attr(feature = "serde", serde(default))]
    pub traders: trader::Traders,
    /// `CBuildingMgr`'s list per (player, type) (heads 0x14eea74 + 4 · (83 · player + type),
    /// linked through entity +0x28 / +0x2a), head first: `AddBuilding` (0x53c350) puts a new
    /// building (a site too) at the head, `CheckOutBuilding` (0x53c680, from `Delete`) takes it
    /// out, a change of owner (0x53f050) moves it to the head of the new owner's list.
    #[cfg_attr(feature = "serde", serde(default))]
    pub lists: BTreeMap<(u8, u16), Vec<u16>>,
}

/// [`BuildingRoles::lists`]: building `id` (its owner and type now) to the head of its list.
pub fn list_add(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let key = (e.owner, e.kind);
    world.buildings.lists.entry(key).or_default().insert(0, id);
}

/// Building `id` out of whatever list holds it (0x53d160 / 0x53c6b0).
pub fn list_remove(world: &mut World, id: u16) {
    let mut empty = None;
    for (k, v) in world.buildings.lists.iter_mut() {
        if let Some(i) = v.iter().position(|&b| b == id) {
            v.remove(i);
            if v.is_empty() {
                empty = Some(*k);
            }
            break;
        }
    }
    if let Some(k) = empty {
        world.buildings.lists.remove(&k);
    }
}

/// The list of (player, type), head first.
pub fn list(world: &World, player: u8, kind: u16) -> &[u16] {
    world.buildings.lists.get(&(player, kind)).map_or(&[], |v| &v[..])
}

impl BuildingRoles {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u32(self.lists.len() as u32);
        for (&(p, k), v) in &self.lists {
            h.u8(p);
            h.u16(k);
            h.u16s(v);
        }
        h.u32(self.roles.len() as u32);
        for (&id, r) in &self.roles {
            h.u16(id);
            r.base.hash(h);
            match &r.kind {
                RoleKind::Other => h.u8(0),
                RoleKind::Gather(s) => {
                    h.u8(1);
                    s.hash(h);
                }
                RoleKind::Farm(s) => {
                    h.u8(2);
                    s.hash(h);
                }
                RoleKind::WorkUp(s) => {
                    h.u8(3);
                    s.hash(h);
                }
                RoleKind::Production(s) => {
                    h.u8(4);
                    s.hash(h);
                }
                RoleKind::Storage(s) => {
                    h.u8(5);
                    s.hash(h);
                }
                RoleKind::Simple(s) => {
                    h.u8(6);
                    s.hash(h);
                }
                RoleKind::Military(s) => {
                    h.u8(7);
                    s.hash(h);
                }
                RoleKind::Mine(s) => {
                    h.u8(8);
                    s.hash(h);
                }
                RoleKind::Barrack(s) => {
                    h.u8(9);
                    s.hash(h);
                }
                RoleKind::Trading(s) => {
                    h.u8(13);
                    s.hash(h);
                }
                RoleKind::Residence(s) => {
                    h.u8(10);
                    s.hash(h);
                }
                RoleKind::MushroomFarm(s) => {
                    h.u8(11);
                    s.hash(h);
                }
                RoleKind::DarkTemple(s) => {
                    h.u8(12);
                    s.hash(h);
                }
                RoleKind::Lookout(s) => {
                    h.u8(14);
                    s.hash(h);
                }
                RoleKind::SmallTemple(s) => {
                    h.u8(15);
                    s.hash(h);
                }
                RoleKind::BigTemple(s) => {
                    h.u8(16);
                    s.hash(h);
                }
                RoleKind::ManakopterHall(s) => {
                    h.u8(17);
                    s.hash(h);
                }
                RoleKind::Workshop(s) => {
                    h.u8(18);
                    s.hash(h);
                }
            }
        }
        self.traders.hash(h);
    }
}

pub fn role(world: &World, id: u16) -> Option<&Role> {
    world.buildings.roles.get(&id)
}

/// Roles that builds before the destroy fix (`map_object_logic_update`: `~CBuilding` 0x535e40
/// deletes the role) left behind in savegames: a building role whose entity is gone, is no
/// building or is a construction site, and a site role whose entity is gone or no unfinished
/// building (`BIRTH`; a dead one keeps its roles until it is destroyed). Returns how many went.
pub fn drop_stale_roles(world: &mut World) -> usize {
    let building = |world: &World, id: u16| world.entities.by_index(id as u32).filter(|e| e.obj_type == crate::entity::ObjType::Building).map(|e| e.flags);
    let sites: Vec<u16> = world.sites.roles.keys().copied().filter(|&id| building(world, id).is_none_or(|f| f & (flag::BIRTH | flag::DIED) == 0)).collect();
    for id in &sites {
        world.sites.roles.remove(id);
    }
    let roles: Vec<u16> = world.buildings.roles.keys().copied().filter(|&id| building(world, id).is_none() || world.sites.roles.contains_key(&id)).collect();
    for id in &roles {
        world.buildings.roles.remove(id);
    }
    sites.len() + roles.len()
}

pub fn role_mut(world: &mut World, id: u16) -> Option<&mut Role> {
    world.buildings.roles.get_mut(&id)
}

fn kind_of(world: &World, id: u16) -> Option<&RoleKind> {
    role(world, id).map(|r| &r.kind)
}

/// `CBuildingMgr::CreateBuildingRole(type, race, false)`: the role for the building's house kind.
pub fn create_role(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let type_ex = match &e.data {
        crate::entity::EntityData::Building(b) => b.type_ex,
        _ => return,
    };
    let hk = world.rules.building(e.race, type_ex).house_kind;
    let kind = match hk {
        house_kind::GATHER => RoleKind::Gather(Default::default()),
        house_kind::FARM => RoleKind::Farm(Default::default()),
        // CAnimalRanchRole shares all but Init / LogicUpdate with the workup role (see workup).
        house_kind::WORKUP | house_kind::ANIMALRANCH => RoleKind::WorkUp(Default::default()),
        house_kind::PRODUCTION => RoleKind::Production(Default::default()),
        house_kind::STORAGE => RoleKind::Storage(Default::default()),
        house_kind::SIMPLE => RoleKind::Simple(Default::default()),
        house_kind::MILITARY | house_kind::CASTLE => RoleKind::Military(Default::default()),
        house_kind::MINE => RoleKind::Mine(Default::default()),
        house_kind::BARRACK => RoleKind::Barrack(Default::default()),
        house_kind::RESIDENCE => RoleKind::Residence(Default::default()),
        house_kind::MUSHROOMFARM => RoleKind::MushroomFarm(Default::default()),
        house_kind::DARKTEMPLE => RoleKind::DarkTemple(Default::default()),
        house_kind::TRADE => RoleKind::Trading(Default::default()),
        house_kind::LOOKOUTTOWER => RoleKind::Lookout(Default::default()),
        // `CEyeCatcherRole` (0x1058b6c): `InitCommon` and an empty `LogicUpdate`, i.e. `Other`.
        house_kind::SMALLTEMPLE => RoleKind::SmallTemple(Default::default()),
        house_kind::BIGTEMPLE => RoleKind::BigTemple(Default::default()),
        house_kind::MANAKOPTERHALL => RoleKind::ManakopterHall(Default::default()),
        house_kind::WORKSHOP => RoleKind::Workshop(Default::default()),
        _ => RoleKind::Other,
    };
    world.buildings.roles.insert(id, Role { base: Base { house_kind: hk, ..Default::default() }, kind });
}

/// Role vtbl+24 `Init`.
pub fn init(world: &mut World, id: u16) {
    init_common(world, id);
    match kind_of(world, id) {
        Some(RoleKind::Gather(_)) => gather::init(world, id),
        Some(RoleKind::Farm(_)) => farm::init(world, id),
        Some(RoleKind::WorkUp(_)) => workup::init(world, id),
        Some(RoleKind::Production(_)) => production::init(world, id),
        Some(RoleKind::Simple(_)) => simple::init(world, id),
        Some(RoleKind::Storage(_)) => storage::init(world, id),
        Some(RoleKind::Military(_)) => {
            let castle = role(world, id).is_some_and(|r| r.base.house_kind == house_kind::CASTLE);
            military::init(world, id, castle);
        }
        Some(RoleKind::Mine(_)) => mine::init(world, id),
        Some(RoleKind::Barrack(_)) => barrack::init(world, id),
        Some(RoleKind::Residence(_)) => residence::init(world, id),
        Some(RoleKind::MushroomFarm(_)) => mushfarm::init(world, id),
        Some(RoleKind::DarkTemple(_)) => dark_temple::init(world, id),
        Some(RoleKind::Trading(_)) => trading::init(world, id),
        Some(RoleKind::Lookout(_)) => lookout::init(world, id),
        Some(RoleKind::SmallTemple(_)) => temple::small_init(world, id),
        Some(RoleKind::BigTemple(_)) => temple::big_init(world, id),
        Some(RoleKind::Workshop(_)) => workshop::init(world, id),
        Some(RoleKind::ManakopterHall(_)) => manakopter::init(world, id),
        // `Other` is the eye catchers': `InitCommon` (0x545ff0), plus vtbl+0x58 for a selected
        // building (flag 0x200; the selection refresh). Every building role's Init is ported.
        _ => {}
    }
}

/// `IBuildingRole::InitCommon` (release 0x540b20; every role's Init calls it first): the role
/// forgets its inhabitant (+8) and the worker-in-house byte (dword +0x1a..+0x1d), BIRTH → READY,
/// the building's id goes into the map-object layer at its position, the building's health
/// grows by the info hit points (+0x1f4 → building vtbl+0x18, 0x566fb0, saturating at 255).
/// Also cleared there: role byte +5, dword +0x14 and word +0x18, zero in a new role anyway [inf];
/// then `InitHousePatches` (0x540bc0) fills the role's patch table +0x9c (200 bytes) from the
/// building info +0x1f8.. (the house's animated overlays) [inf: draw-side, not ported here].
fn init_common(world: &mut World, id: u16) {
    if let Some(r) = role_mut(world, id) {
        r.base.inhabitant = 0;
        r.base.worker_in_house = false;
    }
    let Some(e) = world.entities.by_index_mut(id as u32) else { return };
    e.flags = (e.flags & !flag::BIRTH) | flag::READY;
    let (x, y, race) = (e.x(), e.y(), e.race);
    let type_ex = match &e.data {
        crate::entity::EntityData::Building(d) => d.type_ex,
        _ => return,
    };
    if let Some(n) = world.grid.checked_index(x, y) {
        world.layers.map_object[n] = id;
    }
    let hp = world.rules.building(race, type_ex).hitpoints;
    if let Some(crate::entity::EntityData::Building(d)) = world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
        d.health = d.health.saturating_add(hp);
    }
}

/// `CBuilding::BuildingDone`, called by the construction-site role when the building is
/// finished (after it removed its material piles): the real role is created and initialised,
/// the building gets flag 0x10000.
pub fn building_done(world: &mut World, id: u16) {
    // 0x5362f0: the finished building counts into the fighting strength (0x4f8730); the AI
    // notice (6, owner, building, 0).
    if let Some(e) = world.entities.by_index(id as u32) {
        let (owner, kind) = (e.owner, e.kind);
        crate::statistic::building(world, owner, kind, 1);
        crate::statistic::building_built(world, owner, kind);
        crate::ai::post_event(world, 6, owner as i32, id as i32, 0);
    }
    create_role(world, id);
    init(world, id);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags |= FLAG_BUILT;
    }
    // Type 0x22 as placed: AO_STORAGEAREA_READY, unfiltered (0x536448).
    if let Some(e) = world.entities.by_index(id as u32)
        && let crate::entity::EntityData::Building(b) = &e.data
        && b.type_ex == 0x22
    {
        let ev = crate::event::SimEvent::Message { player: e.owner, text: 0x1fd, x: e.x(), y: e.y(), filtered: false };
        world.events.push(ev);
    }
}

/// `CBuilding::LogicUpdate` (release vtbl+0x64, 0x537180): queued events first
/// (`ProcessAllEvents` 0x532be0 → vtbl+0x6c 0x537310 → role +0x5c `ConvertEventIntoGoal`); the
/// role's `LogicUpdate` runs unless events were processed and the last one did not set entity
/// flag bit 31.
pub fn logic_update(world: &mut World, id: u16) {
    if process_all_events(world, id) {
        return;
    }
    match kind_of(world, id) {
        Some(RoleKind::Gather(_)) => gather::logic_update(world, id),
        Some(RoleKind::Farm(_)) => farm::logic_update(world, id),
        Some(RoleKind::WorkUp(_)) => workup::logic_update(world, id),
        Some(RoleKind::Production(_)) => production::logic_update(world, id),
        Some(RoleKind::Simple(_)) => simple::logic_update(world, id),
        Some(RoleKind::Storage(_)) => storage::logic_update(world, id),
        Some(RoleKind::Military(_)) => military::logic_update(world, id),
        Some(RoleKind::Mine(_)) => mine::logic_update(world, id),
        Some(RoleKind::Barrack(_)) => barrack::logic_update(world, id),
        Some(RoleKind::Residence(_)) => residence::logic_update(world, id),
        Some(RoleKind::MushroomFarm(_)) => mushfarm::logic_update(world, id),
        Some(RoleKind::DarkTemple(_)) => dark_temple::logic_update(world, id),
        Some(RoleKind::Trading(_)) => trading::logic_update(world, id),
        Some(RoleKind::Lookout(_)) => lookout::logic_update(world, id),
        Some(RoleKind::SmallTemple(_)) => temple::small_logic_update(world, id),
        Some(RoleKind::BigTemple(_)) => temple::big_logic_update(world, id),
        Some(RoleKind::Workshop(_)) => workshop::logic_update(world, id),
        Some(RoleKind::ManakopterHall(_)) => manakopter::logic_update(world, id),
        _ => {}
    }
}

/// `IAnimatedEntity::SetEvent` (0x532e60) on a building: queued, and the building is registered
/// for the next tick unless it already is.
pub fn set_event(world: &mut World, id: u16, ev: EntityEvent) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.flags & flag::REGISTERED == 0 {
        world.register_for_logic_update(id, 1);
    }
    if let Some(crate::entity::EntityData::Building(d)) = world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
        d.events.push(ev);
    }
}

/// `IAnimatedEntity::ProcessAllEvents` (0x532be0) for a building.
fn process_all_events(world: &mut World, id: u16) -> bool {
    let mut done = false;
    loop {
        let ev = match world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
            Some(crate::entity::EntityData::Building(d)) if !d.events.is_empty() => d.events.remove(0),
            _ => return done,
        };
        if let Some(e) = world.entities.by_index_mut(id as u32) {
            e.flags &= !crate::jobs::core::FLAG_CONTINUE_AFTER_EVENT;
        }
        convert_event_into_goal(world, id, &ev);
        done = world.entities.by_index(id as u32).is_some_and(|e| e.flags & crate::jobs::core::FLAG_CONTINUE_AFTER_EVENT == 0);
    }
}

/// `CBuilding::SettlerEnter(settler)` → role `SettlerEnter`.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    match kind_of(world, id) {
        Some(RoleKind::Gather(_)) => gather::settler_enter(world, id, settler),
        Some(RoleKind::Farm(_)) => farm::settler_enter(world, id, settler),
        Some(RoleKind::WorkUp(_)) => workup::settler_enter(world, id, settler),
        Some(RoleKind::Production(_)) => production::settler_enter(world, id, settler),
        Some(RoleKind::Simple(_)) => simple::settler_enter(world, id, settler),
        Some(RoleKind::Military(_)) => military::settler_enter(world, id, settler),
        Some(RoleKind::Mine(_)) => mine::settler_enter(world, id, settler),
        // CBarrackRole::SettlerEnter (0x534a40).
        Some(RoleKind::Barrack(_)) => true,
        Some(RoleKind::MushroomFarm(_)) => mushfarm::settler_enter(world, id, settler),
        Some(RoleKind::DarkTemple(_)) => dark_temple::settler_enter(world, id, settler),
        Some(RoleKind::Trading(_)) => trading::settler_enter(world, id, settler),
        Some(RoleKind::Lookout(_)) => lookout::settler_enter(world, id, settler),
        Some(RoleKind::SmallTemple(_)) => temple::small_settler_enter(world, id, settler),
        Some(RoleKind::Workshop(_)) => workshop::settler_enter(world, id, settler),
        _ => false,
    }
}

/// Role +0x50 `SettlerLeft(settler)`: a settler of the building is set free (its role's
/// `SetFree` calls this). Military and castle: 0x549720; barracks (0x535490) and sites
/// (0x541200): nothing; every other role the `IBuildingRole` default (0x541080): the
/// inhabitant (+8) is forgotten, the word +4 = 1 (state 1, which orders a new inhabitant; +5 = 0),
/// the worker is not home (+0x1d), the building updates next tick (0x56d5d0(1)).
pub fn settler_left(world: &mut World, id: u16, settler: u16) {
    match kind_of(world, id) {
        Some(RoleKind::Military(_)) => military::settler_left(world, id, settler),
        Some(RoleKind::Barrack(_)) | None => {}
        Some(_) => {
            let Some(r) = role_mut(world, id) else { return };
            r.base.inhabitant = 0;
            r.base.worker_in_house = false;
            match &mut r.kind {
                RoleKind::Gather(gather::State { work, .. })
                | RoleKind::Farm(farm::State { work, .. })
                | RoleKind::WorkUp(workup::State { work, .. })
                | RoleKind::Production(production::State { work, .. })
                | RoleKind::Simple(simple::State { work, .. })
                | RoleKind::Mine(mine::State { work, .. })
                | RoleKind::Residence(residence::State { work, .. })
                | RoleKind::Workshop(workshop::State { work, .. }) => {
                    work.state = 1;
                    work.delay = 0;
                }
                RoleKind::Trading(s) => {
                    s.state = 1;
                    s.sub = 0;
                }
                RoleKind::SmallTemple(temple::SmallState { state, delay, .. }) | RoleKind::BigTemple(temple::BigState { state, delay, .. }) => {
                    *state = 1;
                    *delay = 0;
                }
                RoleKind::Lookout(lookout::State { state, .. })
                | RoleKind::MushroomFarm(mushfarm::State { state, .. })
                | RoleKind::ManakopterHall(manakopter::State { state, .. }) => *state = 1,
                // s4rs does not model their +4 [inf: no ported path of theirs reads it].
                RoleKind::Other | RoleKind::Storage(_) | RoleKind::DarkTemple(_) | RoleKind::Military(_) | RoleKind::Barrack(_) => {}
            }
            world.register_for_logic_update(id, 1);
        }
    }
}

/// `CBuilding::Detach(settler)` (vtbl+0x40, 0x537100): off the observer list, the settler's
/// observer target 0 (home) cleared, then role +0x7c (military 0x54aa40; the default 0x534450
/// is empty).
pub fn detach_settler(world: &mut World, id: u16, settler: u16) {
    if matches!(kind_of(world, id), Some(RoleKind::Military(_))) {
        military::detach_settler(world, id, settler);
        return;
    }
    detach(world, id, settler);
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.home = 0;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags &= !flag::ATTACHED;
    }
}

/// `CBuilding::GoodArrive(good)` (a delivery or site pile of the building got a good).
pub fn good_arrive(world: &mut World, id: u16, good: u8) {
    match kind_of(world, id) {
        Some(RoleKind::Gather(_)) => gather::good_arrive(world, id, good),
        Some(RoleKind::Farm(_)) => farm::good_arrive(world, id, good),
        Some(RoleKind::WorkUp(_)) => workup::good_arrive(world, id, good),
        Some(RoleKind::Production(_)) => production::good_arrive(world, id, good),
        Some(RoleKind::Simple(_)) => simple::good_arrive(world, id, good),
        Some(RoleKind::Storage(_)) => {}
        Some(RoleKind::Mine(_)) => mine::good_arrive(world, id, good),
        Some(RoleKind::Residence(_)) => {} // 0x53b160: nothing.
        Some(RoleKind::Trading(_)) => trading::good_arrive(world, id, good),
        // CSmallTempleRole 0x53b690, CBigTempleRole 0x5354e0: nothing.
        Some(RoleKind::SmallTemple(_) | RoleKind::BigTemple(_)) => {}
        // CWorkshopBuildingRole 0x558e10 / 0x558540: nothing.
        Some(RoleKind::Workshop(_)) => {}
        // CBarrackRole vtbl+0x44 (0x534470): `ret 4`.
        Some(RoleKind::Barrack(_)) => {}
        // CBuildingSiteRole vtbl+0x44 (0x543020): nothing.
        None if world.sites.roles.contains_key(&id) => {}
        _ => world.events.push(crate::event::SimEvent::Unported { what: "CBuilding::GoodArrive", entity: id }),
    }
}

/// `CBuilding::ConvertEventIntoGoal` → role.
pub fn convert_event_into_goal(world: &mut World, id: u16, ev: &EntityEvent) {
    match kind_of(world, id) {
        Some(RoleKind::Gather(_)) => gather::convert_event_into_goal(world, id, ev),
        Some(RoleKind::Farm(_)) => farm::convert_event_into_goal(world, id, ev),
        Some(RoleKind::WorkUp(_)) => workup::convert_event_into_goal(world, id, ev),
        Some(RoleKind::Production(_)) => production::convert_event_into_goal(world, id, ev),
        Some(RoleKind::Simple(_)) => simple::convert_event_into_goal(world, id, ev),
        Some(RoleKind::Military(_)) => military::convert_event_into_goal(world, id, ev),
        Some(RoleKind::Trading(_)) => trading::convert_event_into_goal(world, id, ev),
        _ => {}
    }
}

/// `CBuilding::GetPileIdWithGood(good)` (role vtbl): the pile holding the building's `good`.
pub fn pile_with_good(world: &World, id: u16, good: u8) -> u16 {
    match kind_of(world, id) {
        Some(RoleKind::Gather(_)) => gather::pile_with_good(world, id, good),
        Some(RoleKind::Farm(_)) => farm::pile_with_good(world, id, good),
        Some(RoleKind::WorkUp(_)) => workup::pile_with_good(world, id, good),
        Some(RoleKind::Production(_)) => production::pile_with_good(world, id, good),
        Some(RoleKind::Simple(_)) => simple::pile_with_good(world, id, good),
        Some(RoleKind::Storage(_)) => storage::pile_with_good(world, id, good),
        // vtbl+56 (0x54b960).
        Some(RoleKind::Mine(_)) => mine::pile_with_good(world, id, good),
        Some(RoleKind::Barrack(_)) => barrack::pile_with_good(world, id, good),
        Some(RoleKind::Trading(_)) => trading::pile_with_good(world, id, good),
        // 0x53b680: the delivery pile for any good; CBigTempleRole 0x5354d0: none.
        Some(RoleKind::SmallTemple(_)) => temple::small_pile(world, id),
        Some(RoleKind::BigTemple(_)) => 0,
        Some(RoleKind::Workshop(_)) => workshop::pile_with_good(world, id, good),
        _ => world.sites.roles.get(&id).map_or(0, |s| crate::build::order::pile_with_good(s, good)),
    }
}

/// `CBuilding::GetPileIdWithNeedForGood(good)` (`IBuildingRole` base: 0).
pub fn pile_with_need_for_good(world: &World, id: u16, good: u8) -> u16 {
    match kind_of(world, id) {
        Some(RoleKind::Gather(_)) => gather::pile_with_need_for_good(world, id, good),
        Some(RoleKind::Farm(_)) => farm::pile_with_need_for_good(world, id, good),
        Some(RoleKind::WorkUp(_)) => workup::pile_with_need_for_good(world, id, good),
        Some(RoleKind::Production(_)) => production::pile_with_need_for_good(world, id, good),
        Some(RoleKind::Simple(_)) => simple::pile_with_need_for_good(world, id, good),
        // The base (0x534400) → vtbl+56.
        Some(RoleKind::Mine(_)) => mine::pile_with_good(world, id, good),
        // IBuildingRole default 0x534400: vtbl+0x38.
        Some(RoleKind::Barrack(_)) => barrack::pile_with_good(world, id, good),
        // CTradingBuildingRole: the base 0x534400 → vtbl+0x38 (0x5521f0).
        Some(RoleKind::Trading(_)) => trading::pile_with_good(world, id, good),
        // The base 0x534400 → vtbl+0x38.
        Some(RoleKind::SmallTemple(_)) => temple::small_pile(world, id),
        Some(RoleKind::BigTemple(_)) => 0,
        Some(RoleKind::Workshop(_)) => workshop::pile_with_need_for_good(world, id, good),
        // CBuildingSiteRole: the base, i.e. GetPileIdWithGood (0x534400 → vtbl+56).
        _ => world.sites.roles.get(&id).map_or(0, |s| crate::build::order::pile_with_good(s, good)),
    }
}

/// `IBuildingRole::TakeJobTrigger(trigger)` (0x540da0, from 0x586dcd when the home is a
/// building): it only changes the role's patch display state, which is the client's
/// ([`crate::event::SimEvent::JobTrigger`]).
pub fn take_job_trigger(world: &mut World, id: u16, trigger: u8) {
    if world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == crate::entity::ObjType::Building) {
        world.events.push(crate::event::SimEvent::JobTrigger { building: id, trigger });
    }
}

/// `CBuilding::Switch` → role vtbl+32 (command 5003). Every role ported here uses the base
/// `IBuildingRole::Switch` (0x53ffd0, see [`work::switch`]; also vtbl+0x20 of `CMilitaryBuildingRole`
/// 0x1058f8c, `CCastleRole` 0x1058c74 and `CBarrackRole` 0x1058484); trading is
/// [`trading::switch`] (0x552120), storage [`storage::switch`] (0x5506b0), a construction site
/// [`crate::build::site::switch`] (0x542bf0).
pub fn switch(world: &mut World, id: u16) {
    if world.sites.roles.contains_key(&id) {
        crate::build::site::switch(world, id);
        return;
    }
    match kind_of(world, id) {
        Some(RoleKind::Storage(_)) => storage::switch(world, id),
        Some(
            RoleKind::Gather(_)
            | RoleKind::Farm(_)
            | RoleKind::WorkUp(_)
            | RoleKind::Production(_)
            | RoleKind::Simple(_)
            | RoleKind::Mine(_)
            | RoleKind::Military(_)
            | RoleKind::Barrack(_)
            | RoleKind::Residence(_)
            | RoleKind::MushroomFarm(_)
            | RoleKind::DarkTemple(_)
            | RoleKind::Lookout(_)
            | RoleKind::SmallTemple(_)
            | RoleKind::BigTemple(_)
            | RoleKind::Workshop(_)
            | RoleKind::ManakopterHall(_),
        ) => work::switch(world, id),
        Some(RoleKind::Trading(_)) => trading::switch(world, id),
        // The eye catchers' vtbl+0x20 is the base one too.
        Some(RoleKind::Other) if role(world, id).is_some_and(|r| r.base.house_kind == house_kind::EYECATCHER) => work::switch(world, id),
        _ => world.events.push(crate::event::SimEvent::Unported { what: "IBuildingRole::Switch", entity: id }),
    }
}

/// The role-side half of command 5001 (release 0x4f62f5): role +0x14 / +0x18 (dwords) and +0x1c
/// (byte) are cleared, for the roles here the working-area search cursors and the "nothing
/// found" warning ([`work::WorkBase`]). They are `IBuildingRole` fields (stored by the base
/// store / load 0x53fd30 / 0x540490); no virtual of the other roles (barracks, temples, eye
/// catcher, lookout, military, castle, site, dark temple, manakopter hall, storage, trading)
/// touches them, so for those the reset changes nothing.
pub fn reset_search(world: &mut World, id: u16) {
    let Some(r) = role_mut(world, id) else { return };
    let w = match &mut r.kind {
        RoleKind::Gather(s) => &mut s.work,
        RoleKind::Farm(s) => &mut s.work,
        RoleKind::WorkUp(s) => &mut s.work,
        RoleKind::Production(s) => &mut s.work,
        RoleKind::Simple(s) => &mut s.work,
        RoleKind::Mine(s) => &mut s.work,
        RoleKind::Residence(s) => &mut s.work,
        RoleKind::Workshop(s) => &mut s.work,
        _ => return,
    };
    w.cursors = [[0; 2]; 2];
    w.warned = false;
}

/// `CBuilding::IsBuildUp`: finished (flag 0x10000).
pub fn is_build_up(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.flags & FLAG_BUILT != 0)
}

/// `CBuilding::Attach(entity)` (`CObserverList::Attach` with target 0): a pile or settler
/// belongs to the building; piles get it as their observer target.
pub fn attach(world: &mut World, id: u16, entity: u16) {
    if let Some(r) = role_mut(world, id)
        && !r.base.observers.iter().any(|o| o.0 == entity)
    {
        r.base.observers.push((entity, 0));
    }
    if crate::pile::get(world, entity).is_some() {
        crate::pile::set_building(world, entity, id);
    }
}

/// `CBuilding::Detach(entity)`.
pub fn detach(world: &mut World, id: u16, entity: u16) {
    if let Some(r) = role_mut(world, id) {
        r.base.observers.retain(|o| o.0 != entity);
    }
}

/// `CBuilding::Notify(event)`: every observer gets the event (piles: started / stopped).
pub fn notify(world: &mut World, id: u16, ev: EntityEvent) {
    let list = role(world, id).map(|r| r.base.observers.clone()).unwrap_or_default();
    for (e, _) in list {
        match world.entities.by_index(e as u32).map(|x| x.obj_type) {
            Some(crate::entity::ObjType::Pile) => crate::pile::set_event(world, e, ev),
            Some(crate::entity::ObjType::Settler) => crate::jobs::core::set_event(world, e, ev),
            _ => {}
        }
    }
}

/// `NotifyAndDetachAll` (0x537430): every observer gets `ev`, then each its observer target 0
/// (0x56e130, vtbl+0x44 `(0, 0)`: a settler's home 0 and ATTACHED cleared, a pile's building
/// gone) and the list is emptied.
pub fn notify_and_detach_all(world: &mut World, id: u16, ev: EntityEvent) {
    notify(world, id, ev);
    let observers = role_mut(world, id).map(|r| std::mem::take(&mut r.base.observers)).unwrap_or_default();
    for (o, _) in observers {
        match world.entities.by_index(o as u32).map(|e| e.obj_type) {
            Some(crate::entity::ObjType::Pile) => crate::pile::set_building(world, o, 0),
            Some(crate::entity::ObjType::Settler) => military::set_home(world, o, 0),
            _ => {}
        }
    }
}

/// [inf] A construction site has no `buildings` role, so no observer list (as in
/// `build::site::building_done`): the settlers whose home it is and the piles it owns, in entity
/// order, stand in for it.
pub fn site_observers(world: &World, id: u16) -> Vec<u16> {
    world
        .entities
        .iter()
        .filter(|(_, e)| !e.is_dead())
        .filter(|(_, e)| match &e.data {
            crate::entity::EntityData::Settler(s) => s.job.home == id,
            crate::entity::EntityData::Pile(p) => p.building == id,
            _ => false,
        })
        .map(|(i, _)| i)
        .collect()
}

/// `CBuilding::Delete` (release 0x5361d0, from `World::kill`): a site's id leaves the
/// deco-object layer; every observer gets event 9 (data: building id, byte +0x25 of the building
/// (the cause 0x536a80 sets while a destroyed building is killed, [`World::delete_cause`]), 0)
/// and is detached (`CObserverList::NotifyAndDetachAll`); the building is unregistered from the
/// logic update (0x56d720) and checked out (0x53c680, the fighting-strength part). The war-map
/// entry went in `World::kill`. Last, the AI notice (4, owner, building, 0). `CheckOutBuilding` takes it out
/// of its list ([`list_remove`]); the statistic's 0x11ac05c / 0x11ac440 are the finished counts
/// (`PlayerStrength::count`). Not ported: the list cursor 0x14f0d78 and the site counter
/// 0x14ef620 — the ported consumers count from the entity table; the call to the
/// entity at building +0x42 (vtbl+0x40 with the building id): +0x42 is written only by the ctor
/// (0) and building vtbl+0x44 (0x5371a0, observer target 0, with the ATTACHED flag), which no
/// ported code calls.
pub fn on_delete(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.flags & flag::BIRTH != 0
        && let Some(n) = world.grid.checked_index(e.x(), e.y())
    {
        world.layers.deco_object[n] = 0;
    }
    let cause = world.delete_cause as i32;
    let ev = EntityEvent { event: 9, kind: 0, data: [id as i32, cause, 0] };
    let site_observers: Vec<(u16, u8)> = if role(world, id).is_none() { site_observers(world, id).into_iter().map(|o| (o, 0)).collect() } else { Vec::new() };
    notify(world, id, ev);
    for &(o, _) in &site_observers {
        match world.entities.by_index(o as u32).map(|e| e.obj_type) {
            Some(crate::entity::ObjType::Pile) => crate::pile::set_event(world, o, ev),
            Some(crate::entity::ObjType::Settler) => crate::jobs::core::set_event(world, o, ev),
            _ => {}
        }
    }
    // 0x56e130: every observer's vtbl+0x44 `SetObserverTarget(0, 0)` at once, then the list is
    // cleared. A pile's (0x56fd10) is its role's `SubjectDie`: out of the offer while the land
    // is still the owner's, then a free (anywhere) pile. A settler's (0x581710) is its role's
    // vtbl+0x50 with target 0 (0x586ff0, the carrier's 0x574450 too: home +0x20 = 0) and
    // ATTACHED cleared; the queued event 9 then finds no home (`check_home` sets it free).
    let mut observers = role(world, id).map(|r| r.base.observers.clone()).unwrap_or_default();
    observers.extend(site_observers);
    if let Some(r) = role_mut(world, id) {
        r.base.observers.clear();
    }
    for (o, _) in observers {
        match world.entities.by_index(o as u32).map(|e| e.obj_type) {
            Some(crate::entity::ObjType::Pile) => crate::pile::set_building(world, o, 0),
            Some(crate::entity::ObjType::Settler) => military::set_home(world, o, 0),
            _ => {}
        }
    }
    world.unregister_from_logic_update(id);
    // `CheckOutBuilding` (0x53c680): out of its list (0x53c6b0); a finished building (role
    // vtbl+0x70 ≠ 0) leaves the fighting strength and the finished counts (0x4f8730, 0x14ef624).
    list_remove(world, id);
    if !world.sites.roles.contains_key(&id)
        && role(world, id).is_some()
        && let Some(e) = world.entities.by_index(id as u32)
    {
        let (owner, kind) = (e.owner, e.kind);
        crate::statistic::building(world, owner, kind, -1);
    }
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    crate::ai::post_event(world, 4, owner as i32, id as i32, 0);
}

/// `CPileMgr::AddPile` for the building + `CPile::SetBuildingId` (the pile is attached to the
/// building): what roles' `Init` do for their piles.
pub fn add_building_pile(world: &mut World, id: u16, x: i32, y: i32, good: u8, pile_role: u8) -> u16 {
    let Some(p) = crate::pile::add_pile(world, x, y, good, 0, pile_role) else { return 0 };
    attach(world, id, p);
    if let Some(r) = role_mut(world, id) {
        r.base.piles.push(p);
    }
    p
}

/// Whether `entity` is alive and not flagged dead.
pub fn alive(world: &World, entity: u16) -> bool {
    world.entities.by_index(entity as u32).is_some_and(|e| e.flags & flag::DIED == 0)
}

/// A military role restored from a savegame's `IBuildingRole +0x1d` alone, without `Init`
/// (oracle tools that rebuild a world from the saved building headers).
pub fn restore_military_inhabited(world: &mut World, id: u16, inhabited: bool) {
    let base = Base { house_kind: house_kind::MILITARY, worker_in_house: inhabited, ..Default::default() };
    world.buildings.roles.insert(id, Role { base, kind: RoleKind::Military(Default::default()) });
}

/// `CBuilding::HaveInhabitant` (`IBuildingRole +0x1d`): the role's worker is home; a military
/// building is inhabited from its first soldier (`FirstSettlerEnter` 0x5484f0) until the last
/// one is gone (0x54a120).
pub fn have_inhabitant(world: &World, id: u16) -> bool {
    crate::buildings::role(world, id).is_some_and(|r| r.base.worker_in_house)
}
