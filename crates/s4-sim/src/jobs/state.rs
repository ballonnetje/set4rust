//! Per-settler job state: the job-relevant members of `IAnimatedEntity`, `IMovingEntity`,
//! `CSettler` and the settler's `ISettlerRole` (offsets of the decompiled debug build).

use super::task::EntityTask;
use super::walking::WalkState;
use crate::grid::Y16X16;
use crate::hash::StateHasher;

/// `ISettlerRole::GetSettlerRole()` ids (vtbl+72).
pub mod role {
    /// No role ported / attached yet: `logic_update` does nothing.
    pub const NONE: u8 = 0;
    /// `CCarrierRole`.
    pub const CARRIER: u8 = 1;
    /// `CDiggerRole`.
    pub const DIGGER: u8 = 2;
    /// `CBuilderRole`.
    pub const BUILDER: u8 = 3;
    /// `CHouseWorkerRole`.
    pub const HOUSE_WORKER: u8 = 4;
    /// `CFreeWorkerRole` (woodcutter, stonecutter, forester, …).
    pub const FREE_WORKER: u8 = 5;
    /// `CSoldierRole` (swordsmen, bowmen, …; `GetSettlerRole` 0x588a60).
    pub const SOLDIER: u8 = 7;
    /// `CHunterRole` (`GetSettlerRole` 0x57ce50).
    pub const HUNTER: u8 = 8;
    /// `CPriestRole` (`GetSettlerRole` 0x57f0e0).
    pub const PRIEST: u8 = 9;
    /// `CSaboteurRole` (a `CSoldierRole`; vtable 0x105bdbc, `GetSettlerRole` 0x57fbe0).
    pub const SABOTEUR: u8 = 10;
    /// `CGardenerRole` (`GetSettlerRole` 0x578f20).
    pub const GARDENER: u8 = 13;
    /// `CPioneerRole` (`GetSettlerRole` 0x57e080).
    pub const PIONEER: u8 = 11;
    /// `CGeologistRole` (`GetSettlerRole` 0x57a1d0).
    pub const GEOLOGIST: u8 = 12;
    /// `CDarkGardenerRole` (`GetSettlerRole` 0x574630).
    pub const DARK_GARDENER: u8 = 14;
    /// `CMushroomFarmerRole` (`GetSettlerRole` 0x57d950).
    pub const MUSHROOM_FARMER: u8 = 15;
    /// `CDonkeyRole` (`GetSettlerRole` 0x575ef0).
    pub const DONKEY: u8 = 20;
    /// `CDoorRole`, the invisible door settler of a military building (0x5776c0).
    pub const DOOR: u8 = 21;
    /// `CTowerSoldier`, a soldier inside a military building (0x58bf30).
    pub const TOWER_SOLDIER: u8 = 22;
    /// `CFleeRole`.
    pub const FLEE: u8 = 18;
    /// `CSquadLeaderRole` (a `CSoldierRole`; ctor 0x58ab30, `GetSettlerRole` 0x57bb20).
    pub const SQUAD_LEADER: u8 = 23;
    /// `CShamanRole` (a `CSoldierRole`; vtable 0x105c14c, `GetSettlerRole` 0x587640).
    pub const SHAMAN: u8 = 16;
    /// `CSlaveRole` (vtable 0x105c254, `GetSettlerRole` 0x587c50).
    pub const SLAVE: u8 = 17;
    /// `CThiefRole` (vtable 0x105c4b4, `GetSettlerRole` 0x58b100).
    pub const THIEF: u8 = 19;
}

/// `CSettler::m_pEntityTasks` (+84): the list the to-do iterator walks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ToDo {
    #[default]
    None,
    /// A shared list of `CEntityToDoListMgr` (`SettlerJobList(race, list)`).
    Shared { race: u8, list: u16 },
    /// A list the role built for this settler (e.g. `ISettlerRole::NewDestinationEx`).
    Own(Vec<EntityTask>),
}

/// In-building offset of `CHouseWorkerRole` (+36/+40 position, +44/+48 step per tick). The
/// original keeps these as `float` pixels; here they are fixed point with [`OFFSET_ONE`] per
/// pixel. They only place the sprite; the move's tick count is computed from the integer
/// targets (see `house_worker::take_job`).
pub const OFFSET_ONE: i32 = 1 << 12;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct JobState {
    /// Which `ISettlerRole` subclass ([`role`]).
    pub role: u8,
    pub todo: ToDo,
    /// `CSettler +72`: list id of `todo` when it is a shared list, −1 otherwise.
    pub list_id: i16,
    /// `CSettler +88`: index of the actual task in `todo` (the original keeps a list iterator).
    pub cursor: u16,

    /// `IAnimatedEntity::m_cFrame` (+36).
    pub frame: u8,
    /// `IAnimatedEntity::m_wJobPart` (+38): settler gfx job being shown.
    pub job_part: u16,
    /// `IAnimatedEntity::m_uLastUpdateTick`.
    pub last_update_tick: u32,
    /// `IAnimatedEntity::m_iEventQueue` (`SetEvent` / `ProcessAllEvents`).
    pub events: Vec<EntityEvent>,

    /// `IMovingEntity +69` (distance to the next node in 1/256, 0xFF = just stepped).
    pub distance: u8,
    /// `IMovingEntity::m_cDisplacementCosts`.
    pub displacement_costs: u8,
    /// `IMovingEntity::m_pWalking` (the pathing state; owned by the pathing implementation).
    pub walking: Option<Box<WalkState>>,

    /// `ISettlerRole +4`: the running task (`CEntityTask::*`).
    pub task: u8,
    /// `ISettlerRole +5`: last walk result (`CSettler::Walk`), 72 after `InitCommonTaskValues`.
    pub walk: u8,
    /// `ISettlerRole +6`: the task's tick counter; the walk speed (7/8/9) while walking.
    pub counter: i8,
    /// `ISettlerRole +7` (`m_iCycleFrames`).
    pub cycle_frames: u8,
    /// `ISettlerRole +8`: ticks since the last `Update`.
    pub elapsed: u16,
    /// `ISettlerRole +10`: play forwards.
    pub forward: bool,
    /// `ISettlerRole +11`: good carried / produced.
    pub good: u8,
    /// `ISettlerRole +12`.
    pub b12: u8,
    /// `ISettlerRole +14/+16`: the task's cell offset.
    pub x: i16,
    pub y: i16,
    /// `ISettlerRole +18` (the carrier's `Init` stores its own entity id here).
    pub w18: u16,
    /// `ISettlerRole +20` `m_uSourcePileId` (observer target 2).
    pub source_pile: u16,
    /// `ISettlerRole +24` destination, `+28` start position.
    pub destination: Y16X16,
    pub start: Y16X16,
    /// `ISettlerRole +32` `m_uHomeEntityId`.
    pub home: u16,
    /// `ISettlerRole +34` `m_uEntityId`.
    pub entity: u16,
    /// `CHouseWorkerRole` offset / step, see [`OFFSET_ONE`].
    pub offset: (i32, i32),
    pub step: (i32, i32),
    /// `CCarrierRole` members.
    pub carrier: CarrierData,
    /// `CDiggerRole` / `CBuilderRole` members.
    pub digger: DiggerData,
    pub builder: BuilderData,
    pub free_worker: FreeWorkerData,
    /// `CSoldierRole` / `CTowerSoldier` members.
    pub soldier: SoldierData,
    pub tower: TowerData,
    /// `CGardenerRole` / `CDarkGardenerRole` members.
    #[cfg_attr(feature = "serde", serde(default))]
    pub gardener: GardenerData,
    /// `CGeologistRole` +0x48 / +0x4c (its centre and search are in [`JobState::gardener`]).
    #[cfg_attr(feature = "serde", serde(default))]
    pub geologist: GeologistData,
    /// `CPriestRole` members.
    #[cfg_attr(feature = "serde", serde(default))]
    pub priest: PriestData,
    /// `CSlaveRole` members.
    #[cfg_attr(feature = "serde", serde(default))]
    pub slave: SlaveData,
    /// `CDonkeyRole` members (made by the role's first use; boxed: only donkeys have them).
    #[cfg_attr(feature = "serde", serde(default))]
    pub donkey: Option<Box<DonkeyData>>,
    /// `CThiefRole` +0x38 / +0x3c.
    #[cfg_attr(feature = "serde", serde(default))]
    pub thief: ThiefData,
    /// `ISelectableSettlerRole +0x2c`: the group word (see [`crate::group`]); 0 for roles
    /// without one.
    pub groups: u16,
}

/// `CSoldierRole` (ctor 0x588ac0, size 0x60) and its `CWarriorBehavior` (at role +0x30). Only the
/// members the ported states use; the behaviour's search table pointer (+0x34) is the row
/// [`SoldierData::table`] of the table at .data 0x11cdc18 (25 dwords per row).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SoldierData {
    /// Behaviour +0x34 row: the settler's warrior type, or 15..20 for medics, axe warriors,
    /// blowgun warriors, the shaman (0x36), the saboteur (0x2e) and the catapultists 0x3d..0x3f
    /// (`SetMode` 0x598290).
    pub table: u8,
    /// Behaviour +0x38 state (0/1 idle, 4 walking to a target, 5..7 fighting, 8 done).
    pub state: u8,
    /// Behaviour +0x39 flags (bit 1: a target was found by the second search).
    pub bflags: u8,
    /// Behaviour +0x3a: the enemy being attacked (0 = none).
    pub target: u16,
    /// Behaviour +0x3c: target position (−1 unless the mode walks somewhere).
    pub target_pos: i32,
    /// Behaviour +0x40: mode flags (owner in the low nibble, 0x100000..0x700000 the mode).
    pub mode: u32,
    /// Behaviour +0x44: tick of the next enemy search.
    pub next_search: u32,
    /// Role +0x49, initialised to `Logic/MedicNumberOfHealings` (0x11cdbbc).
    pub healings: u8,
    /// Role +0x4c: tick of the last regeneration (medics).
    pub regen_tick: u32,
    /// Role +0x50: the fight task (`Attack` 0x58a750) that overrides the list's until the next
    /// `LogicUpdateJob`.
    pub fight_task: Option<super::EntityTask>,
    /// Role +0x60: `CSquadLeaderRole` the tick of the next lead (0x58aea0), `CSaboteurRole` the
    /// tick of the next camouflage check (0x57fd80).
    pub lead_tick: u32,
    /// `CSquadLeaderRole` +0x68: the place the group was last sent to (0 none).
    pub lead_target: i32,
    /// `CSquadLeaderRole` +0x6c: where the leader stands its ground (−1 / 0 none).
    pub anchor: i32,
    /// `CShamanRole` +0x60 / +0x64: the manakopter it belongs to (event 0x1c) and that entity's
    /// unique id; 0 none.
    #[cfg_attr(feature = "serde", serde(default))]
    pub shaman_ride: u16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub shaman_ride_uid: u32,
    /// `CShamanRole` +0x68: the shaman's own mana (`DTShamanManaPool` from the ctor 0x5876b0,
    /// refilled on dark land).
    #[cfg_attr(feature = "serde", serde(default))]
    pub shaman_mana: i32,
    /// `CShamanRole` +0x6c: where to cast (event 3), 0 none.
    #[cfg_attr(feature = "serde", serde(default))]
    pub shaman_cast_at: i32,
}

/// `CSlaveRole` (size 0x38): +0x2c / +0x30 the node it reserved to work at (node flag 0x20;
/// (−1, −1) none), +0x34 the spiral cursor of its search (0x588730).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SlaveData {
    pub spot: (i32, i32),
    pub cursor: i32,
}

impl Default for SlaveData {
    fn default() -> Self {
        SlaveData { spot: (-1, -1), cursor: 0 }
    }
}

/// `CPriestRole` (size 0x3c): +0x38 the spell ordered (event 3), −1 none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PriestData {
    pub spell: i32,
}

impl Default for PriestData {
    fn default() -> Self {
        PriestData { spell: -1 }
    }
}

/// `CTowerSoldier` (size 0x34): +0x2c slot, +0x2d state (0 at the door wall, 1 the first top
/// slot (watches the door), 2 other top slots, 3 guarding the door outside), +0x2e direction,
/// +0x2f shot strength, +0x30 tick of the last shot. The shot target (+0x22) is
/// [`JobState::entity`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TowerData {
    pub slot: u8,
    pub state: u8,
    pub dir: u8,
    pub strength: u8,
    pub tick: u32,
}

/// `CDonkeyRole` +0x2c..+0x78 (ctor 0x575f20, store version 5 0x5761b0 in this order; the own
/// settler id +0x30 is the entity). +0x2d and +0x48 are not set by the ctor (garbage in the
/// saves); here they start at 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DonkeyData {
    /// +0x2c: the task at the end of the last `LogicUpdateJob`.
    pub last_task: u8,
    /// +0x2d: a vehicle order (event 11) is running.
    pub vehicle_order: bool,
    /// +0x2e / +0x2f: walking to the source / the target (the arrival goes to that market).
    pub to_source: bool,
    pub to_target: bool,
    /// +0x34 target market, +0x38 source market.
    pub target: u16,
    pub source: u16,
    /// +0x3c: the vehicle of a vehicle order.
    pub vehicle: i32,
    /// +0x40: 0 free, 2 to the source, 4 loading, 1 to the target, 3 unloading, 5 vehicle order.
    pub state: u32,
    /// +0x44: goods of a vehicle order.
    pub x44: i32,
    /// +0x48: the vehicle order's good; +0x4a its pile, +0x4c its vehicle.
    pub x48: u8,
    pub pile: u16,
    pub vehicle2: u16,
    /// The two load slots (+0x50 + 12·i good, +0x54 amount; +0x58 is never written).
    pub slots: [(u8, i32); 2],
    /// +0x68: a foreign trade transport.
    pub foreign: bool,
    /// +0x6c: goods unloaded since the last arrival; +0x70 goods loaded into empty slots (not
    /// stored).
    pub unloaded: i32,
    pub loaded: i32,
    /// +0x74: the foreign leg (0, 1 there, 2 back).
    pub leg: u32,
    /// +0x78: walking home after an order (`0x577010`).
    pub going_home: bool,
}

/// `CDarkGardenerRole` +0x34..+0x48 (`CGardenerRole` the same 4 bytes further on): the work
/// centre the player sent it to, the node it reserved (flag 0x20), and its search through the
/// spiral around itself (64 entries a round, at most 57 rounds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GardenerData {
    /// +0x34 / +0x38.
    pub centre: (i32, i32),
    /// +0x3c / +0x40, (−1, −1) when none.
    pub spot: (i32, i32),
    /// +0x44: rounds without a free node.
    pub rounds: i32,
    /// +0x48: the spiral entry the next round starts at.
    pub spiral: i32,
}

impl Default for GardenerData {
    fn default() -> Self {
        GardenerData { centre: (0, 0), spot: (-1, -1), rounds: 0, spiral: 0 }
    }
}

/// `CGeologistRole` +0x48 (counted 0..5 by each search task) and +0x4c (the sign type of the last
/// `CThiefRole` (ctor 0x58b120, size 0x40): +0x38 the tick of the next visibility check
/// ([`super::thief`]), +0x3c the good stolen and carried (0 = none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ThiefData {
    pub check_at: u32,
    pub loot: u8,
}

/// "found" message; 0xc1 = none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GeologistData {
    pub tick: i32,
    pub last_sign: u16,
}

impl Default for GeologistData {
    fn default() -> Self {
        GeologistData { tick: 0, last_sign: 0xc1 }
    }
}

/// `CFreeWorkerRole` +44..+47 (store version 1: +44, +45, floats +48/+52 (the step, here
/// [`JobState::step`]), +46). The in-node offset +36/+40 is [`JobState::offset`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FreeWorkerData {
    /// +44: the good carried home.
    pub good: u8,
    /// +45: working on the entity (`WORK_ON_ENTITY` taken); `SetFree` leaves it alone then.
    pub on_entity: bool,
    /// +46: object type to plant (event 6).
    pub plant: u16,
}

/// `CDiggerRole` +44..+46 (store version 1: +44 u16, +46 u8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiggerData {
    /// +44: target height of the node being dug (0 = none).
    pub target: u16,
    /// +46: set while digging (WORK taken), cleared by event 1 and `SetFree`.
    pub working: bool,
}

/// `CBuilderRole` +44..+45 (store version 1: two `signed char`s).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuilderData {
    /// +44: direction to face while building (`SetDir`, from `GetBuilderPos`).
    pub dir: i8,
    /// +45: 1 once the builder started working at its position (WORK taken).
    pub at_work: i8,
}

/// `CEntityEvent` (persistence class 66): `m_iEvent`, `m_iType`, three data words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct EntityEvent {
    pub event: i32,
    pub kind: i32,
    pub data: [i32; 3],
}

/// `CCarrierRole` +44..+55 (store version 3 order: +44, +45, +20 (base), +46, +48, +50, +52, +54).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CarrierData {
    /// +44 good carried.
    pub good: u8,
    /// +45 settler type to become (`ChangeToNextType`), 0 = none.
    pub next_type: u8,
    /// +46 destination pile (observer target 1).
    pub dest_pile: u16,
    /// +48 / +50 second and third source pile (targets 3, 4: tools for a new specialist).
    pub source_pile2: u16,
    pub source_pile3: u16,
    /// +52 idle counter, +54 idle threshold (`Rand() % 300 + 300`, drawn in the constructor and
    /// after every idle animation).
    pub idle_count: u16,
    pub idle_after: u16,
}

impl Default for JobState {
    /// `ISettlerRole::ISettlerRole` + `IMovingEntity` / `IAnimatedEntity` constructors.
    fn default() -> Self {
        JobState {
            role: role::NONE,
            todo: ToDo::None,
            list_id: -1,
            cursor: 0,
            frame: 0,
            job_part: 0,
            last_update_tick: 0,
            events: Vec::new(),
            distance: 0,
            displacement_costs: 0,
            walking: None,
            task: 0,
            walk: 0,
            counter: 1,
            cycle_frames: 1,
            elapsed: 0,
            forward: true,
            good: 0,
            b12: 0,
            x: 0,
            y: 0,
            w18: 0,
            source_pile: 0,
            destination: Y16X16(0),
            start: Y16X16(0),
            home: 0,
            entity: 0,
            offset: (0, 0),
            step: (0, 0),
            carrier: CarrierData::default(),
            digger: DiggerData::default(),
            builder: BuilderData::default(),
            free_worker: FreeWorkerData::default(),
            soldier: SoldierData::default(),
            tower: TowerData::default(),
            gardener: GardenerData::default(),
            geologist: GeologistData::default(),
            priest: PriestData::default(),
            slave: SlaveData::default(),
            donkey: None,
            thief: ThiefData::default(),
            groups: 0,
        }
    }
}

impl JobState {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.role);
        match &self.todo {
            ToDo::None => h.u8(0),
            ToDo::Shared { race, list } => {
                h.u8(1);
                h.u8(*race);
                h.u16(*list);
            }
            ToDo::Own(v) => {
                h.u8(2);
                h.u32(v.len() as u32);
                v.iter().for_each(|t| t.hash(h));
            }
        }
        h.u16(self.list_id as u16);
        h.u16(self.cursor);
        h.u8(self.frame);
        h.u16(self.job_part);
        h.u32(self.last_update_tick);
        h.u32(self.events.len() as u32);
        for e in &self.events {
            h.u32(e.event as u32);
            h.u32(e.kind as u32);
            e.data.iter().for_each(|&d| h.u32(d as u32));
        }
        h.u8(self.distance);
        h.u8(self.displacement_costs);
        match &self.walking {
            None => h.u8(0),
            Some(w) => {
                h.u8(1);
                w.hash(h);
            }
        }
        h.u8(self.task);
        h.u8(self.walk);
        h.u8(self.counter as u8);
        h.u8(self.cycle_frames);
        h.u16(self.elapsed);
        h.bool(self.forward);
        h.u8(self.good);
        h.u8(self.b12);
        h.u16(self.w18);
        h.u16(self.source_pile);
        h.u16(self.x as u16);
        h.u16(self.y as u16);
        h.u32(self.destination.0);
        h.u32(self.start.0);
        h.u16(self.home);
        h.u16(self.entity);
        h.u32(self.offset.0 as u32);
        h.u32(self.offset.1 as u32);
        h.u32(self.step.0 as u32);
        h.u32(self.step.1 as u32);
        let c = &self.carrier;
        h.u8(c.good);
        h.u8(c.next_type);
        h.u16(c.dest_pile);
        h.u16(c.source_pile2);
        h.u16(c.source_pile3);
        h.u16(c.idle_count);
        h.u16(c.idle_after);
        h.u16(self.digger.target);
        h.bool(self.digger.working);
        h.u8(self.builder.dir as u8);
        h.u8(self.builder.at_work as u8);
        h.u8(self.free_worker.good);
        h.bool(self.free_worker.on_entity);
        h.u16(self.free_worker.plant);
        let s = &self.soldier;
        for v in [s.table, s.state, s.bflags, s.healings] {
            h.u8(v);
        }
        h.u16(s.target);
        h.u32(s.target_pos as u32);
        h.u32(s.mode);
        h.u32(s.next_search);
        h.u32(s.regen_tick);
        h.bool(s.fight_task.is_some());
        if let Some(t) = &s.fight_task {
            h.u8(t.task);
            h.u16(t.entity);
            h.u16(t.job_part);
            h.u8(t.duration as u8);
            h.u8(t.frames);
        }
        let t = &self.tower;
        for v in [t.slot, t.state, t.dir, t.strength] {
            h.u8(v);
        }
        h.u32(t.tick);
        h.u16(self.groups);
        h.u32(s.lead_tick);
        h.u32(s.lead_target as u32);
        h.u32(s.anchor as u32);
        h.u16(s.shaman_ride);
        h.u32(s.shaman_ride_uid);
        h.u32(s.shaman_mana as u32);
        h.u32(s.shaman_cast_at as u32);
        let g = &self.gardener;
        for v in [g.centre.0, g.centre.1, g.spot.0, g.spot.1, g.rounds, g.spiral] {
            h.u32(v as u32);
        }
        h.u32(self.geologist.tick as u32);
        h.u16(self.geologist.last_sign);
        h.u32(self.priest.spell as u32);
        h.u32(self.slave.spot.0 as u32);
        h.u32(self.slave.spot.1 as u32);
        h.u32(self.slave.cursor as u32);
        let d = self.donkey.as_deref().copied().unwrap_or_default();
        for v in [d.last_task, d.vehicle_order as u8, d.to_source as u8, d.to_target as u8, d.x48, d.foreign as u8, d.going_home as u8] {
            h.u8(v);
        }
        for v in [d.target, d.source, d.pile, d.vehicle2] {
            h.u16(v);
        }
        for v in [d.vehicle, d.state as i32, d.x44, d.unloaded, d.loaded, d.leg as i32] {
            h.u32(v as u32);
        }
        for (g, n) in d.slots {
            h.u8(g);
            h.u32(n as u32);
        }
        h.u32(self.thief.check_at);
        h.u8(self.thief.loot);
    }
}
