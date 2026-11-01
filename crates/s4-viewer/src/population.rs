//! What the sprite layer draws, independent of where it came from: either an `s4_sim::World`
//! (the default, the simulation's state right after loading the map) or the raw map chunks
//! (`--raw-map`, for comparison).
//!
//! The viewer only *reads* the world. The one exception is [`world_from_map`], which builds a
//! world and runs the start towers' `SetOwner` as a stand-in for the post-load steps the sim has
//! not ported yet (`s4_sim::load::new_game` TODO: `CLogic::PostLoadMap`).

use s4_formats::map::MapFile;
use s4_sim::{EntityData, ObjType, World};

/// A deco object at a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deco {
    pub x: u16,
    pub y: u16,
    /// Object id (chunk 6 byte 0 / the deco entity's type; stone heaps are 41 with `amount`).
    pub id: u16,
    /// Gfx job offset from the type's first job: the growth / felling stage of trees and planted
    /// objects (`CTree` / `CPlant` +0x26 = `GetObjectFirstJob(type) + stage`, set by their
    /// constructors, `LogicUpdate` and `Decrease`, release 0x593880 / 0x593c50 / 0x593e50), 0 for
    /// other objects. 3 = grown, 4 = falling, 5..9 = the trunk being chopped, 10 = the stump.
    pub stage: u8,
    /// Frame within the job.
    pub frame: u8,
    /// Stone amount (type 41).
    pub amount: u8,
    /// Frames of the animation cycle (`+74`); 0 = the job's frame count.
    pub cycle: u8,
}

impl Deco {
    /// Objects whose gfx job follows a stage: trees (`CTree`, 1..=22) and the planted objects.
    pub fn staged(id: u16) -> bool {
        matches!(id, 1..=22 | 162 | 209..=214 | 255)
    }

    /// The frame advances with the ticks (`GetGfxInfo`: `frame = (frame + Δtick) % cycle`), except
    /// for a tree's stump (`CTree` 0x593b80: stage 10, faded by `LogicUpdate`) and a plant past
    /// its grown stages (`CPlant` 0x566430: stage 4 and above), and a shadow herb's growing job
    /// (`CShadowHerb` drawn by 0x566a40, which skips its stages 1 and 2: job 1, frames set by
    /// `LogicUpdate` 0x566b00).
    pub fn animated(&self) -> bool {
        match self.id {
            1..=22 => self.stage != 10,
            216 => self.stage != 1,
            162 | 209..=211 | 255 => self.stage < 4,
            _ => true,
        }
    }
}

/// A soldier in a military building (`CMilitaryBuildingRole` +0x1a4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inhabitant {
    /// Settler type.
    pub kind: u16,
    /// The soldier's own race (a captured building keeps its builder's race; its new owner's
    /// soldiers keep theirs: playtest P2).
    pub race: u8,
    /// Its slot (the index into buildingInfo's soldier list); `None` for raw map data (the
    /// drawing takes the first free slot of its kind, as `SettlerEnter` does).
    pub slot: Option<u8>,
    /// The soldier as the simulation has it (its job, frame and direction: `CSettler::
    /// GetPatchGfx` draws those, so a shooting bowman animates: playtest B2); `None` for raw
    /// map data.
    pub unit: Option<Unit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Building {
    /// Entity index in the world (0 for raw map data).
    pub id: u16,
    pub x: u16,
    pub y: u16,
    /// Building type (generic: ports/shipyards as 32/31).
    pub kind: u8,
    pub owner: u8,
    pub race: u8,
    /// The soldiers inside (garrison), in slot order.
    pub inhabitants: Vec<Inhabitant>,
    /// A construction site (`CBuildingSiteRole`).
    pub site: Option<SiteGfx>,
    /// The house worker the building draws as part of itself (see [`house_worker`]).
    pub worker: Option<Unit>,
    /// `+0x44` the door (s4-sim `BuildingData::door`; the building's eco-sector is the door node's, e.g. 0x53f170); the
    /// position for raw map data.
    pub door: (u16, u16),
    /// The tiling sector (`m_cNormalTileIdMap` → tile record, low half; 0x12f1a24) of the node at
    /// +0x48 (s4-sim `BuildingData::ensign`): the trade-target check's "same land" (0x552960).
    /// 0 for raw map data.
    pub land: u16,
    /// Entity flag 0x2000000 (`READY`).
    pub ready: bool,
    /// The entity's kind (+0xc): `kind` with ports and shipyards as 0x20 / 0x1f.
    pub generic: u16,
    /// The small player flag (0x540300): `Some(working)` when the role draws one and its worker
    /// is home (role +0x1d), `working` = entity flag 0x1000. See [`flag_of`].
    pub flag: Option<bool>,
    /// An animal or donkey ranch (`CAnimalRanchRole`, house kind 3): the animals in its pen
    /// (role +0x18c), which pick its FX slot 0 (s4-game `Patches::pen`).
    pub pen: Option<u8>,
    /// A Dark Tribe mushroom farm (`CDarkMushroomFarmRole`, house kind 20): what its patches and
    /// pull effects show ([`MushFarm`]).
    pub mushroom: Option<MushFarm>,
}

/// The display side of a mushroom farm (s4-sim `buildings::mushfarm::State`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MushFarm {
    /// The patch playing: 0 none, 1 OPEN (slot 0), 2 FLOW (slot 1); since tick `patch_tick`.
    pub patch: u8,
    pub patch_tick: u32,
    /// +0x188: spheres pulled (GROW, slot 2, shows frame `spheres − 1`).
    pub spheres: i32,
    /// +0x194 the tick of the last pull, the mushroom it came from and the tick its flight
    /// reaches the farm (+0x198).
    pub last_pull: i32,
    pub pull_from: (u16, u16),
    pub pull_end: u32,
}

/// House kinds whose `FillGfxInfo` calls the flag helper 0x540300: simple 0x54f420, work-up and
/// animal ranch 0x55a3a0, production 0x54d0e0, workshop 0x5592f0, gather 0x546ea0, mine
/// 0x54b790, farm 0x546780, lookout tower 0x547520, trading 0x5528b0, small temple 0x54fce0.
/// Military buildings and castles draw their own standarte (0x548f70); storage, residence,
/// barrack, big temple, eye-catcher and the Dark Tribe roles draw none.
const FLAG_HOUSE_KINDS: [u8; 11] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 13, 16];

/// 0x540300: with the role's +0x1d (`HaveInhabitant`) set, the flag; its direction is 0 while
/// the building works (entity flag 0x1000), else 1.
fn flag_of(w: &World, idx: u16, e: &s4_sim::entity::Entity, type_ex: u8) -> Option<bool> {
    if w.sites.roles.contains_key(&idx) || !FLAG_HOUSE_KINDS.contains(&w.rules.building(e.race, type_ex).house_kind) {
        return None;
    }
    s4_sim::buildings::have_inhabitant(w, idx).then_some(e.flags & 0x1000 != 0)
}

/// House kind 3, `CAnimalRanchRole` (animal and donkey ranch): the animals of the finished
/// ranch (`buildings::workup`, +0x18c).
fn pen_of(w: &World, idx: u16, e: &s4_sim::entity::Entity, type_ex: u8) -> Option<u8> {
    if w.sites.roles.contains_key(&idx) || w.rules.building(e.race, type_ex).house_kind != 3 {
        return None;
    }
    s4_sim::buildings::workup::get(w, idx).map(|s| s.animals)
}

/// What `CBuildingSiteRole::FillGfxInfo` reads of a site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiteGfx {
    /// +396: 2 digging … 5 building.
    pub state: u8,
    /// +412 work done, +416 work to do, +420 `131070 / work`.
    pub work_done: i32,
    pub work: i32,
    pub step: i32,
    /// +401 builders at work.
    pub builders: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unit {
    /// Entity index in the world (0 for raw map data).
    pub id: u16,
    /// `IEntity +4` (`m_iUniqueId`; 0 for raw map data).
    pub uid: u32,
    /// `IEntity::ObjType`: 1 settler, 2 ship, 4 land vehicle.
    pub obj: u8,
    /// `EntityFlag` bits (low 4 bits: the warrior type); 0 for raw map data.
    pub flags: u32,
    /// `IEntity::Hitpoints` and the type's maximum (`SettlerInfo +2`; 0 = none).
    pub health: u8,
    pub max_health: u8,
    pub x: u16,
    pub y: u16,
    pub kind: u16,
    pub owner: u8,
    pub race: u8,
    /// `CSettler +0x40`: the direction faced, and of the last step.
    pub dir: u8,
    /// What `CSettler::GetGfxInfo` (release 0x5810a0) draws: gfx job `+0x26` (`m_wJobPart`, an
    /// index into the race's settler file) and frame `+0x24`. Raw map data has none (job 0,
    /// task 0); see [`Unit::gfx_job`].
    pub job: u16,
    pub frame: u8,
    /// The role (`ISettlerRole::GetSettlerRole`, `s4_sim::jobs::state::role`), whose `UpdateJob`
    /// animates the tasks the base `Update` does not.
    pub role: u8,
    /// The role's task (`ISettlerRole +4`), its counter (`+6`, the walk speed while walking),
    /// cycle frames (`+7`), last walk result (`+5`) and play direction (`+10`): what the
    /// draw-time `ISettlerRole::Update` (0x586a00) advances the frame and distance with.
    pub task: u8,
    pub counter: i8,
    pub cycle: u8,
    pub walk: u8,
    pub forward: bool,
    /// `IMovingEntity +0x41` (release): distance still to go to the node in 1/256 (0xFF = just
    /// stepped onto it).
    pub distance: u8,
    /// The virtual-task offset (`CHouseWorkerRole` / `CFreeWorkerRole` floats `+0x24/+0x28`) and
    /// its step per tick (`+0x2c/+0x30`), both fixed point with `s4_sim::jobs::state::OFFSET_ONE`
    /// per unit: 1/256 of a node column / row for a free worker (drawn instead of the walk
    /// displacement while flag 0x800 is set), pixels from the building's anchor for a house
    /// worker drawn by its building (see [`crate::animate`]).
    pub offset: (i32, i32),
    pub step: (i32, i32),
    /// A vehicle's own drawing state ([`VehicleGfx`]).
    pub vehicle: Option<VehicleGfx>,
    /// The tiling sector of the unit's node (`ITiling::SectorId`: `m_cNormalTileIdMap` 0x15630c4
    /// → the tile record's low half, 0x12f1a24), as Shift + click compares it (0x4ed810); 0 for
    /// raw map data.
    pub land: u16,
}

/// What the vehicles' GetGfxInfo (`CWheeler` 0x59a5c0, `CCart` 0x55ba20, `CShip` 0x590590; see
/// [`crate::vehicle_gfx`]) and the draw-time update 0x594db0 read beyond the unit fields (`job`
/// +0x26, `frame` +0x24, `dir` +0x40, `distance` +0x41).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VehicleGfx {
    /// +0x2c the tick of the last update (the simulation's `LogicUpdate` runs it too).
    pub anim_tick: u32,
    /// +0x5f the task (6 walking, 0x10 animation, 0x11 standing), +0x5c the last step (bit 3:
    /// none), +0x60 half frames, +0x62 ticks per node.
    pub task: u8,
    pub step: u8,
    pub half_frame: u8,
    pub speed: u8,
    /// +0x5d turn steps left (non-zero: the turn sprites, vtbl+0xb8) and +0x5e the 16-way
    /// direction (bit 7 turning clockwise).
    pub turn: u8,
    pub dir16: u8,
    /// The vehicle record's gfx jobs +0x18 (moving), +0x1c and +0x20 (the turn sprites' bases).
    pub jobs: [u16; 3],
    /// The clip (gfx info +0x5c): BIRTH (0x1000000) for land vehicles, not READY (0x2000000)
    /// for ships, `0xfffe − (+0x74) · (+0x70)`.
    pub crop: Option<u16>,
    /// A cart that carries something (0x55ca10, `CartData::has_cargo`: a slot amount, an arrived
    /// good, a settler inside): the loaded cart's sprites.
    pub loaded: bool,
    /// The icons shown with the selection icon: gfx info +0x15 (ammo) and +0x14 (health), 0 =
    /// none; shown while `icon_flag` (0x200 selected, 0x400 "other") is set.
    pub icons: [u8; 2],
    pub icon_flag: u32,
}

impl Unit {
    /// `GetGfxInfo` returns nothing unless `(flags & 0x80100) == 0x100` (VISIBLE without
    /// MAGIC_INVISIBLE) and DIED (0x4000000) is clear: settlers inside buildings (CHECKIN clears
    /// VISIBLE) are not drawn. Raw map units (id 0) have no flags and are always drawn.
    pub fn drawn(&self) -> bool {
        self.id == 0 || (self.flags & 0x0408_0100) == 0x100
    }

    /// The settler gfx job to draw: `+0x26`, or job 0 when it is out of range (`>= 0x16f`, as
    /// `GetSettlerGfxInfo` 0x461d00 does). `None` for units the simulation never gave a task
    /// (raw map data): the caller draws the type's walk job.
    pub fn gfx_job(&self) -> Option<u16> {
        (self.id != 0 && (self.job != 0 || self.task != 0)).then_some(if self.job < 0x16f { self.job } else { 0 })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pile {
    pub x: u16,
    pub y: u16,
    pub good: u8,
    pub amount: u8,
}

/// A land animal (`CAnimal` / `CLandAnimal`, `s4_sim::animals::AnimalData`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Animal {
    pub id: u16,
    /// `IEntity +4` (`m_iUniqueId`).
    pub uid: u32,
    pub x: u16,
    pub y: u16,
    /// `ANIMAL_*`.
    pub kind: u16,
    /// +0x58: 0 waiting, 1 idle animation, 2 walking, 3 fleeing, 4 dying, 5 dead.
    pub state: u8,
    /// +0x6a: the task type (0x11 stand, 0x10 idle animation, 6 walk, 0x1b die).
    pub task_type: u8,
    /// +0x26 the gfx job (`Gfx\8.gfx`), +0x40 the direction.
    pub job: u16,
    pub dir: u8,
    /// Entity flags (`GetGfxInfo` 0x52bb30 draws only `flags & 0x80100 == 0x100`).
    pub flags: u32,
    /// +0x6c the task's frame count, +0x68 the last step (0x08: blocked), +0x69 ticks per step,
    /// +0x24 the frame the simulation holds (only the death animation's end sets it).
    pub frames: u8,
    pub moved: u8,
    pub speed: u8,
    pub frame: u8,
    /// The ticks of the last `TakeJob` and of the last step: the draw-time update (0x52bb90)
    /// counts frames and the walk distance from them ([`crate::animate`]).
    pub job_tick: u32,
    pub step_tick: u32,
}

/// A flying object as the client draws it (s4-sim `flying.rs`, flyer_gfx.rs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flyer {
    pub id: u16,
    pub owner: u8,
    /// Its `CEffects` moving effect (+0x44): where it is between two nodes.
    pub flight: s4_sim::flying::Flight,
    /// +0x78 / +0x7c: the offset of the two effects drawn above it (x, height; −10 / −50 on
    /// the ground, −200 up in the air).
    pub offset: (i32, i32),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Population {
    /// The world tick this snapshot shows (deco frames are valid at this tick).
    pub tick: u32,
    pub size: usize,
    pub height: Vec<u8>,
    pub ground: Vec<u8>,
    pub gradient: Vec<u8>,
    pub gfx_bits: Vec<u8>,
    /// `m_cFlagMap` per node (`s4_sim::grid::flag`; 0x04 dark land, set and cleared at run time by
    /// `CDarkTribe` 0x4cbb60). Empty for raw map data.
    pub flags: Vec<u8>,
    /// The resource layer per node (`s4_sim` `layers.resource`: type in the high nibble, amount
    /// in the low one; type 0 = fish). The client's jumping-fish pass (0x5301c0) reads it.
    /// Empty for raw map data.
    pub resource: Vec<u8>,
    /// `CWaterFlags` per node (s4-sim `tiling.water_flags`, 0x14438ac). The client's
    /// water-animal pass (0x52ffe0) reads it. Empty for raw map data.
    pub water: Vec<u16>,
    /// Territory owner per node (`ITiling::OwnerId`), 0 = nobody.
    pub owner: Vec<u8>,
    pub decos: Vec<Deco>,
    pub buildings: Vec<Building>,
    pub settlers: Vec<Unit>,
    /// Vehicles: `kind` is the vehicle type (1..=5).
    pub vehicles: Vec<Unit>,
    /// Land animals.
    pub animals: Vec<Animal>,
    /// The flying objects (manakopters) that have not died: drawn as `CEffects` records
    /// (flyer_gfx.rs).
    pub flyers: Vec<Flyer>,
    pub piles: Vec<Pile>,
    /// Race per player id (index 1..=8; 0 unused).
    pub races: [u8; 9],
    /// Start position per player id.
    pub starts: [(u32, u32); 9],
    /// The team of each player id (`CAlliances` 0x12dcd0c, 0 = not in the game).
    pub teams: [u8; 9],
    /// What the GUI's statistics pages read (empty for raw map data).
    pub economy: Economy,
    /// Per node, what the minimap's object layer can show (`MinimapIndexedColorLookup` 0x60da80
    /// over `m_cMapObjectMap`): the entity's type as the lookup takes it (a settler with a
    /// warrior type, flags & 0xf without 0x1000_0000, becomes 0x16) in the low byte, its owner
    /// (entity +0x20 >> 4) in the high byte, 0xff for "the local player" (flag 0x1000_0000:
    /// `CurrentTribeOffset`); 0 = no entity or no owner. Empty for raw map data.
    pub minimap_obj: Vec<u16>,
    /// The players' unit groups (`CGroupMgr` 0x153fd30, s4-sim `group.rs`): the member ids of
    /// player `p`'s group `g` at `p * 11 + g`. Empty for raw map data.
    pub groups: Vec<Vec<u16>>,
}

/// The economy as the GUI pages read it (`CEcoSectorMgr` records, `CSettlerMgr` counts).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Economy {
    /// The eco-sector records.
    pub sectors: Vec<EcoInfo>,
    /// Eco-sector id per node (`ITiling::EcoSectorId`), for "the eco-sector at the screen centre".
    pub eco_of: Vec<u16>,
    /// Living settlers per player id (0..=8) and settler type (`CSettlerMgr::GetNumberOfSettlers`).
    pub settlers: Vec<[u32; 67]>,
    /// The tool good of each settler type, per race (`SettlerInfo` byte 1, 0 = none).
    pub settler_tool: Vec<[u8; 67]>,
    /// `CSettlerMgr`'s lists (0x1544cec, `s4_sim::settler_lists`) by (player, settler type), head
    /// first, the empty ones left out: (entity id, node x, node y) (`+0x18`, also for settlers
    /// inside buildings).
    pub lists: std::collections::BTreeMap<(u8, u16), Vec<Listed>>,
    /// `CVehicleMgr`'s lists (0x1549bd0, `s4_sim::vehicles::iter`) by (player, vehicle type), head
    /// first (newest first), the empty ones left out: what the "next vehicle" button walks
    /// (0x4e9cc0).
    pub vehicle_lists: std::collections::BTreeMap<(u8, u16), Vec<u16>>,
    /// `CBuildingMgr`'s lists (heads 0x14eea74, s4-sim `BuildingRoles::lists`) by (player,
    /// building type), head first (sites included, new ones at the head, a captured building
    /// at the head of its new owner's list): what the build pages' go-to walks (0x53d0b0).
    pub building_lists: std::collections::BTreeMap<(u8, u16), Vec<u16>>,
    /// Per ferry, the counts its dialog shows (`CFerryShip` vtbl+0x94 0x58e450,
    /// `s4_sim::vehicles::ferry::dialog`).
    pub ferries: std::collections::BTreeMap<u16, [i32; 12]>,
    /// Per player id (0..=8): what the statistic pages read of `CStatistic` (per player 0x1128
    /// bytes from 0x11abe34) and `CEcoSectorMgr`.
    pub players: Vec<PlayerInfo>,
    /// `CStatisticArchive` (the statistics graph pages, stat_graphs.rs).
    pub history: s4_sim::statistic::Archive,
    /// `GlobalGameStateObject +0x58` (`s4_sim::world::GameSetup::game_type`) and, for game type
    /// 3, its seven goods (+0x78, `race_goods`): the extras' goods race dialog (goods_race.rs).
    pub game_type: u32,
    pub race_goods: [u8; 7],
}

/// One player's statistics as the GUI pages read them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerInfo {
    /// `CStatistic` +0xa04 + 4 · good: the goods the player has (kept by `CPile::AdjustStatistic`
    /// 0x56fe80 with the eco-sector's owner; the sim's `statistic.players[p].goods`).
    pub goods: [i32; 43],
    /// `GetNrOfCurrentTotalBeds` (0x4df730) and `GetNrOfInitialFreeBeds` (0x147c8a8).
    pub beds: i32,
    pub initial_beds: i32,
    /// The striking carriers (`CEcoSectorMgr` 0x147c8cc vector).
    pub strikers: i32,
    /// `CStatistic` +0xbbc / +0xbc0: the fighting strength on own / foreign land (256 = 100 %).
    pub strength: (i32, i32),
    /// The mana pool (`s4_sim::magic`): the mana, and per spell (casts the mana pays for,
    /// 0x4f5a50 `s4_sim::magic::affordable`; the current cost, +0x10 + 4 · i).
    pub mana: i32,
    pub spells: [(i32, i32); 8],
    /// `CStatistic` +0x1060: the copy of the pool's mana `Statistic::Update` makes (0x4f87f0,
    /// `s4_sim::statistic::copy_mana`), which the extension bar shows.
    pub stat_mana: i32,
    /// `CStatistic` +0x1078 + 4 · good: the goods as the start snapshot froze them (the sim's
    /// `goods_start`, mirroring `goods` until then); the goods race fill 0x4f8da0 sums these.
    pub goods_start: [i32; 43],
}

impl Default for PlayerInfo {
    fn default() -> Self {
        PlayerInfo { goods: [0; 43], beds: 0, initial_beds: 0, strikers: 0, strength: (0, 0), mana: 0, spells: [(0, 0); 8], stat_mana: 0, goods_start: [0; 43] }
    }
}

/// A settler of a `CSettlerMgr` list: (entity id, node x, node y).
pub type Listed = (u16, u16, u16);

/// One `CEcoSector` as the pages read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EcoInfo {
    pub id: u16,
    pub owner: u8,
    /// `NrOfSettler` per type, +32 `MinCarrier`.
    pub settlers: [u16; 67],
    pub min_carrier: u16,
    /// `GetNumberOfGoods` categories 0 (piles) and 1 (carriers) per good.
    pub goods_piles: [u16; 43],
    pub goods_carriers: [u16; 43],
    /// 0x4d4280 per good: the amounts of the piles in the good's offer list (+0x130).
    pub offered: [i32; 43],
    /// `m_cWorkerDeltaAmount` (diggers, builders as `s4_sim::eco::WORKERS`: builder, digger) and
    /// `m_cSpecialistWishes` (`s4_sim::eco::SPECIALISTS` order).
    pub worker_delta: [i8; 2],
    pub specialist_wish: [i8; 5],
    /// `CGoodTransportPriority` (eco +0x224): rank per good (1 = first, 0 = none) and +0x5c
    /// (eco +0x280) the number of ranks.
    pub transport_prio: [u16; 43],
    pub transport_count: i32,
    /// `CBuildingSupplyPriority` (eco +0x218): building type → good → share.
    pub supply: s4_sim::eco::SupplyMap,
    /// 0x4ddf40: the owner's striking carriers standing in this eco-sector.
    pub strikers: u32,
}

impl EcoInfo {
    /// `UpdateGoodDistributionValues` (0x4c5290): (building type, share) of every building type
    /// that has a share of `good`, in type order.
    pub fn supply_of(&self, good: i32) -> Vec<(i32, i32)> {
        self.supply.iter().filter_map(|(&b, g)| u8::try_from(good).ok().and_then(|k| g.get(&k)).map(|&s| (b as i32, s))).collect()
    }
}

impl Economy {
    pub fn sector(&self, id: u16) -> Option<&EcoInfo> {
        self.sectors.iter().find(|e| e.id == id)
    }

    /// 0x585870 / 0x4ebfe0: the first eco-sector of player `local` on the spiral (entries 0..100,
    /// .data 0x1169968 = `s4_sim::spiral`) around the screen centre node `(x, y)`; 0 if none.
    /// The pages of the free carriers (32) and the specialists (30) and their commands use it.
    pub fn own_at_centre(&self, size: usize, (x, y): (usize, usize), local: u8) -> u16 {
        for k in 0..100 {
            let (nx, ny) = (x as i32 + s4_sim::spiral::dx(k), y as i32 + s4_sim::spiral::dy(k));
            if nx < 0 || ny < 0 || nx as usize >= size || ny as usize >= size {
                continue;
            }
            let id = self.eco_of.get(ny as usize * size + nx as usize).copied().unwrap_or(0);
            if id != 0 && self.sector(id).is_some_and(|e| e.owner == local) {
                return id;
            }
        }
        0
    }
}

impl Population {
    /// `GetCurrentEcoSectorIndex` (0x4dfde0): the first node of the spiral entries
    /// `0..=CSpiralOffsets::Last(15)` (word 0x117cf76) around the screen-centre node `(x, y)` whose
    /// land belongs to `local` (`EcoSectorPool` record of the partition map, low nibble) and has an
    /// eco-sector; its id, 0 if none. The transport priority (0x4dfae0) and goods distribution
    /// (0x4df9d0) fills use it.
    pub fn current_eco_at_centre(&self, (x, y): (usize, usize), local: u8) -> u16 {
        for k in 0..=s4_sim::spiral::last(15) {
            let (nx, ny) = (x as i32 + s4_sim::spiral::dx(k), y as i32 + s4_sim::spiral::dy(k));
            if nx < 0 || ny < 0 || nx as usize >= self.size || ny as usize >= self.size {
                continue;
            }
            let i = ny as usize * self.size + nx as usize;
            if self.owner.get(i) == Some(&local) {
                let id = self.economy.eco_of.get(i).copied().unwrap_or(0);
                if id != 0 {
                    return id;
                }
            }
        }
        0
    }
}

/// 0x4d4280: the amounts of the piles in eco-sector `eco`'s offer list of `good`.
pub fn offered(w: &World, eco: u16, good: u8) -> i32 {
    let mut id = w.economy.get(eco).map_or(0, |e| e.good_offer.get(good as usize).copied().unwrap_or(0));
    let mut n = 0;
    while id != 0 {
        n += s4_sim::pile::get(w, id).map_or(0, |p| p.amount as i32);
        id = w.entities.by_index(id as u32).map_or(0, |e| e.next);
    }
    n
}

/// `s4_sim::new_game` plus, for every inhabited military building, `CWorldManager::SetOwner` at its
/// position (the start territory; not yet run by the sim's loader).
pub fn world_from_map(map: &MapFile) -> anyhow::Result<World> {
    world_from_map_with(map, &s4_sim::NewGame::default())
}

/// [`world_from_map`] with a game setup (races chosen in a lobby, seed).
pub fn world_from_map_with(map: &MapFile, setup: &s4_sim::NewGame) -> anyhow::Result<World> {
    world_from_map_start(map, setup, &mut |_, _| {})
}

/// [`world_from_map_with`] whose game start makes the start resources through `start`
/// ([`s4_sim::new_game_with`]).
pub fn world_from_map_start(map: &MapFile, setup: &s4_sim::NewGame, start: &mut dyn FnMut(&mut World, &s4_sim::StartResources)) -> anyhow::Result<World> {
    let rules = s4_sim::Rules::load_default()?;
    let (mut world, report) = s4_sim::new_game_with(map, rules, setup, start)?;
    tracing::debug!("sim load: {report:?}");
    let towers: Vec<(i32, i32)> = world
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&world, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        let c = s4_sim::territory::set_owner(&mut world, x, y);
        tracing::debug!("SetOwner({x},{y}): {} nodes changed", c.changed);
    }
    Ok(world)
}

/// Snapshot of a world for drawing.
pub fn from_world(w: &World) -> Population {
    let l = &w.layers;
    let size = w.grid.size() as usize;
    let mut p = Population {
        tick: w.tick,
        size,
        height: l.height.clone(),
        ground: l.ground.clone(),
        gradient: l.gradient.clone(),
        gfx_bits: l.gfx_bits.clone(),
        flags: l.flags.clone(),
        resource: l.resource.clone(),
        water: w.tiling.water_flags.clone(),
        owner: (0..l.height.len()).map(|i| w.owner_of(i)).collect(),
        decos: Vec::new(),
        buildings: Vec::new(),
        settlers: Vec::new(),
        vehicles: Vec::new(),
        animals: Vec::new(),
        flyers: Vec::new(),
        piles: Vec::new(),
        races: [0; 9],
        starts: [(0, 0); 9],
        teams: [0; 9],
        economy: economy(w),
        minimap_obj: l
            .map_object
            .iter()
            .map(|&id| {
                let Some(e) = (id != 0).then(|| w.entities.by_index(id as u32)).flatten() else { return 0 };
                if e.owner == 0 {
                    return 0;
                }
                let ty = e.obj_type as u16;
                let local = e.flags & 0x1000_0000 != 0;
                let ty = if !local && e.flags & 0xf != 0 && ty == 1 { 0x16 } else { ty };
                let owner = if local { 0xff } else { e.owner as u16 };
                owner << 8 | ty
            })
            .collect(),
        groups: (0..9u8).flat_map(|pl| (0..s4_sim::group::GROUPS).map(move |g| (pl, g))).map(|(pl, g)| w.groups.members(pl, g).to_vec()).collect(),
    };
    for (i, pl) in w.players.iter().enumerate() {
        if let (Some(pl), Some(r)) = (pl, p.races.get_mut(i)) {
            *r = pl.race;
            p.starts[i] = (pl.start_x, pl.start_y);
            p.teams[i] = w.alliances.alliance_id.get(i).copied().unwrap_or(0);
        }
    }
    // Deco instances are shared between nodes: draw them from the node layer.
    for (i, &d) in l.deco_object.iter().enumerate() {
        if d == 0 {
            continue;
        }
        let Some(e) = w.entities.by_index(d as u32) else { continue };
        if let EntityData::Deco(dd) = &e.data {
            // `CHive` and `CShadowHerb` keep their job apart from their stage (`DecoData::job`).
            let stage = if matches!(e.kind, 216 | 217) { dd.job } else if Deco::staged(e.kind) { dd.stage } else { 0 };
            p.decos.push(Deco { x: (i % size) as u16, y: (i / size) as u16, id: e.kind, stage, frame: dd.frame, amount: dd.amount, cycle: dd.cycle_frames });
        }
    }
    for (idx, e) in w.entities.iter() {
        if e.is_dead() {
            continue;
        }
        let (x, y) = (e.x() as u16, e.y() as u16);
        match (&e.data, e.obj_type) {
            (EntityData::Building(d), _) => p.buildings.push(Building {
                id: idx,
                x,
                y,
                kind: d.type_ex,
                owner: e.owner,
                race: e.race,
                inhabitants: s4_sim::buildings::military::slots(w, idx)
                    .iter()
                    .enumerate()
                    .filter(|&(_, &s)| s != 0)
                    .filter_map(|(k, &s)| {
                        let e = w.entities.by_index(s as u32)?;
                        let unit = match &e.data {
                            EntityData::Settler(sd) => Some(settler_unit(w, s, e, sd)),
                            _ => None,
                        };
                        Some(Inhabitant { kind: e.kind, race: e.race, slot: Some(k as u8), unit })
                    })
                    .collect(),
                door: (d.door.x() as u16, d.door.y() as u16),
                land: w.grid.checked_index(d.ensign.x(), d.ensign.y()).map_or(0, |n| w.tiling.sector_of(n)),
                ready: e.flags & s4_sim::entity::flag::READY != 0,
                generic: e.kind,
                site: w.sites.roles.get(&idx).map(|s| SiteGfx {
                    state: s.state,
                    work_done: s.work_done,
                    work: s.work,
                    step: s.work_step,
                    builders: s.builders_working,
                }),
                worker: house_worker(w, idx),
                flag: flag_of(w, idx, e, d.type_ex),
                pen: pen_of(w, idx, e, d.type_ex),
                mushroom: s4_sim::buildings::mushfarm::get(w, idx).filter(|_| !w.sites.roles.contains_key(&idx)).map(|m| MushFarm {
                    patch: m.patch,
                    patch_tick: m.patch_tick,
                    spheres: m.spheres,
                    last_pull: m.last_pull,
                    pull_from: ((m.pull_from & 0xffff) as u16, (m.pull_from >> 16) as u16),
                    pull_end: m.pull_end,
                }),
            }),
            // Soldiers inside a military building are drawn with it; the door settler is invisible.
            (EntityData::Settler(s), ObjType::Settler)
                if !(e.flags & s4_sim::entity::flag::ON_BOARD != 0 && s.job.role == s4_sim::jobs::role::TOWER_SOLDIER)
                    && e.kind != s4_sim::create::DOOR_SETTLER =>
            {
                p.settlers.push(settler_unit(w, idx, e, s))
            }
            (EntityData::Vehicle(v), _) => {
                let v = &v.0;
                let info = w.rules.vehicles.get(e.race, e.kind as u8);
                let ship = e.obj_type == ObjType::Ship;
                let clip = if ship { e.flags & 0x0200_0000 == 0 } else { e.flags & 0x0100_0000 != 0 };
                let ammo = |warship| {
                    let mana = s4_sim::magic::mana(w, e.owner);
                    crate::vehicle_gfx::ammo_icon(warship, e.race, mana, w.rules.magic.viking_warmachine_shot_cost, v.cargo, info.max_ammo)
                };
                // +0x15 / +0x14 by class (0x560c60, 0x58ec70, 0x591f20, 0x5924c0).
                let (icons, icon_flag) = match (ship, e.kind) {
                    (false, 4) => ([ammo(false), crate::vehicle_gfx::health_icon(v.health, info.hitpoints)], 0x200),
                    (true, 1) => ([ammo(true), crate::vehicle_gfx::ship_health_icon(v.health, info.hitpoints)], 0x200),
                    (true, 2) => ([0, crate::vehicle_gfx::ship_health_icon(v.health, info.hitpoints)], 0x200),
                    (true, 3) => ([0, crate::vehicle_gfx::ship_health_icon(v.health, info.hitpoints)], 0x400),
                    _ => ([0, 0], 0),
                };
                let gfx = VehicleGfx {
                    anim_tick: v.anim_tick,
                    task: v.task,
                    step: v.b5c,
                    half_frame: v.half_frame,
                    speed: v.speed,
                    turn: v.turn,
                    dir16: v.dir16,
                    jobs: [info.job, info.job2, info.job3].map(|j| j.clamp(0, 0xffff) as u16),
                    crop: clip.then(|| (0xfffe_u32.wrapping_sub(v.progress[2] as u32 * v.progress[0] as u32) & 0xffff) as u16),
                    loaded: v.cart.as_ref().is_some_and(|c| c.has_cargo()),
                    icons,
                    icon_flag,
                };
                p.vehicles.push(Unit {
                    id: idx,
                    uid: e.uid,
                    obj: e.obj_type as u8,
                    flags: e.flags,
                    x,
                    y,
                    kind: e.kind,
                    owner: e.owner,
                    race: e.race,
                    dir: v.dir.rem_euclid(6) as u8,
                    job: v.job_part,
                    frame: v.frame,
                    distance: v.distance,
                    health: v.health,
                    vehicle: Some(gfx),
                    land: w.grid.checked_index(x as i32, y as i32).map_or(0, |n| w.tiling.sector_of(n)),
                    ..Default::default()
                })
            }
            (EntityData::Pile(d), _) => p.piles.push(Pile { x, y, good: d.good, amount: d.amount }),
            (EntityData::Flying(f), _) => p.flyers.push(Flyer { id: idx, owner: e.owner, flight: f.flight, offset: f.offset }),
            (EntityData::Animal(a), _) => {
                p.animals.push(Animal {
                    id: idx,
                    uid: e.uid,
                    x,
                    y,
                    kind: e.kind,
                    state: a.state,
                    task_type: a.task_type,
                    job: a.job,
                    dir: a.dir,
                    flags: e.flags,
                    frames: a.frames,
                    moved: a.moved,
                    speed: a.speed,
                    frame: a.frame,
                    job_tick: a.job_tick,
                    step_tick: a.step_tick,
                })
            }
            _ => {}
        }
    }
    p
}

fn settler_unit(w: &World, idx: u16, e: &s4_sim::entity::Entity, s: &s4_sim::entity::SettlerData) -> Unit {
    let j = &s.job;
    Unit {
        id: idx,
        uid: e.uid,
        obj: 1,
        flags: e.flags,
        health: s.health,
        max_health: w.rules.settler_health(e.race, e.kind),
        x: e.x() as u16,
        y: e.y() as u16,
        kind: e.kind,
        owner: e.owner,
        race: e.race,
        dir: s.direction,
        job: j.job_part,
        frame: j.frame,
        role: j.role,
        task: j.task,
        counter: j.counter,
        cycle: j.cycle_frames,
        walk: j.walk,
        forward: j.forward,
        distance: j.distance,
        offset: j.offset,
        step: j.step,
        vehicle: None,
        land: w.grid.checked_index(e.x(), e.y()).map_or(0, |n| w.tiling.sector_of(n)),
    }
}

/// The house worker that building `b`'s `FillGfxInfo` draws: `CWorkUpBuildingRole` (release
/// 0x55a3a0), `CProductionBuildingRole` (0x54d0e0) and `CMineRole` (0x54b790) hand their
/// inhabitant to the building's gfx info (the first sub-sprite, +200) when the worker is in the
/// house (`+0x1d`) and the
/// settler has VISIBLE (0x100): its job `+0x26`, frame `+0x24` and direction `+0x40`, at the
/// role floats `+0x24/+0x28` as a **pixel** offset (zoom 1) from the building's anchor. The
/// settler itself carries MAGIC_INVISIBLE from `SettlerEnter` (0x55a050 / 0x54ca90), so
/// `CSettler::GetGfxInfo` never draws it. (`CWorkshopBuildingRole` 0x5592f0 does the same at a
/// buildingInfo slot; that role is not ported.) The healer's hut (`CSimpleBuildingRole` 0x54f420,
/// type 27) draws its healer the same way (0x581490).
fn house_worker(w: &World, b: u16) -> Option<Unit> {
    use s4_sim::buildings::{Role, RoleKind};
    let Role { base, kind } = w.buildings.roles.get(&b)?;
    let healer = matches!(kind, RoleKind::Simple(_)) && w.entities.by_index(b as u32).is_some_and(|e| e.kind == 27);
    if !(matches!(kind, RoleKind::WorkUp(_) | RoleKind::Production(_) | RoleKind::Mine(_)) || healer) {
        return None;
    }
    if !base.worker_in_house {
        return None;
    }
    let e = w.entities.by_index(base.inhabitant as u32)?;
    let EntityData::Settler(s) = &e.data else { return None };
    (e.flags & s4_sim::entity::flag::VISIBLE != 0 && !e.is_dead()).then(|| settler_unit(w, base.inhabitant, e, s))
}

fn economy(w: &World) -> Economy {
    let sectors = w
        .economy
        .sectors
        .iter()
        .flatten()
        .map(|e| EcoInfo {
            id: e.id,
            owner: e.owner,
            settlers: e.settlers,
            min_carrier: e.min_carrier,
            goods_piles: e.goods_on_piles,
            goods_carriers: e.goods_on_carriers,
            offered: std::array::from_fn(|g| offered(w, e.id, g as u8)),
            worker_delta: e.worker_delta,
            specialist_wish: e.specialist_wish,
            transport_prio: e.transport_prio,
            transport_count: e.transport_prio_count,
            supply: e.supply.clone(),
            strikers: w.economy.strikers.get(e.owner as usize).map_or(0, |v| {
                v.iter()
                    .filter(|&&s| {
                        w.entities.by_index(s as u32).and_then(|x| w.grid.checked_index(x.x(), x.y())).is_some_and(|n| w.tiling.eco_sector_of(n) == e.id)
                    })
                    .count() as u32
            }),
        })
        .collect::<Vec<EcoInfo>>();
    let players = (0..9u8)
        .map(|p| {
            let st = w.statistic.players.get(p as usize);
            PlayerInfo {
                goods: st.map_or([0; 43], |s| std::array::from_fn(|g| s.goods.get(g).copied().unwrap_or(0))),
                goods_start: st.map_or([0; 43], |s| std::array::from_fn(|g| s.goods_start.get(g).copied().unwrap_or(0))),
                beds: s4_sim::eco::current_total_beds(w, p),
                initial_beds: if (1..=8).contains(&p) { w.economy.initial_free_beds[p as usize] } else { 0 },
                strikers: w.economy.strikers.get(p as usize).map_or(0, |v| v.len() as i32),
                strength: st.map_or((0, 0), |s| (s.own, s.foreign)),
                mana: s4_sim::magic::mana(w, p),
                stat_mana: st.map_or(0, |s| s.mana),
                spells: std::array::from_fn(|i| {
                    let cost = w.magic.pool(p).map_or(0, |m| m.costs[i]);
                    (s4_sim::magic::affordable(w, p, i as u8), cost)
                }),
            }
        })
        .collect();
    let eco_of = (0..w.grid.len()).map(|i| w.tiling.eco_sector_of(i)).collect();
    let mut settlers = vec![[0u32; 67]; 9];
    for (_, e) in w.entities.iter() {
        if e.obj_type == ObjType::Settler && !e.is_dead() && (e.owner as usize) < 9 && (e.kind as usize) < 67 {
            settlers[e.owner as usize][e.kind as usize] += 1;
        }
    }
    let settler_tool = (0..5u8).map(|r| std::array::from_fn(|t| w.rules.settler_tool(r, t as u16))).collect();
    let mut lists = std::collections::BTreeMap::new();
    for player in 1..=8u8 {
        for kind in 0..67u16 {
            let l: Vec<Listed> = s4_sim::settler_lists::iter(w, player, kind)
                .map(|id| {
                    let (x, y) = w.entities.by_index(id as u32).map_or((0, 0), |e| (e.x(), e.y()));
                    (id, x as u16, y as u16)
                })
                .collect();
            if !l.is_empty() {
                lists.insert((player, kind), l);
            }
        }
    }
    let mut vehicle_lists = std::collections::BTreeMap::new();
    for player in 1..=8u8 {
        for kind in 1..s4_sim::vehicles::TYPES as u16 {
            let l: Vec<u16> = s4_sim::vehicles::iter(w, player, kind).collect();
            if !l.is_empty() {
                vehicle_lists.insert((player, kind), l);
            }
        }
    }
    let ferries = vehicle_lists
        .iter()
        .filter(|((_, kind), _)| *kind == s4_sim::vehicles::kind::FERRY)
        .flat_map(|(_, ids)| ids.iter().map(|&id| (id, s4_sim::vehicles::ferry::dialog(w, id))))
        .collect();
    Economy { sectors, eco_of, settlers, settler_tool, lists, vehicle_lists, building_lists: w.buildings.lists.clone(), ferries, players, history: w.statistic.archive.clone(), game_type: w.setup.game_type, race_goods: w.setup.race_goods }
}

/// The map chunks as they are (no simulation): what the viewer drew before the sim existed.
pub fn from_map(map: &MapFile) -> anyhow::Result<Population> {
    let land = map.landscape()?;
    let players = map.players().unwrap_or_default();
    let race = |owner: u8| {
        players.iter().find(|p| p.id == owner).and_then(|p| u8::try_from(p.race).ok()).filter(|&r| r <= 4).unwrap_or(0)
    };
    let size = land.width as usize;
    let mut p = Population {
        tick: 0,
        size,
        height: land.nodes.iter().map(|n| n.height).collect(),
        ground: land.nodes.iter().map(|n| n.ground).collect(),
        gradient: land.nodes.iter().map(|n| n.gradient).collect(),
        gfx_bits: land.nodes.iter().map(|n| n.gfx_bits).collect(),
        flags: Vec::new(),
        resource: Vec::new(),
        water: Vec::new(),
        owner: land.nodes.iter().map(|n| if n.is_water() { 0 } else { n.owner }).collect(),
        decos: Vec::new(),
        buildings: Vec::new(),
        settlers: Vec::new(),
        vehicles: Vec::new(),
        animals: Vec::new(),
        flyers: Vec::new(),
        piles: Vec::new(),
        races: [0; 9],
        starts: [(0, 0); 9],
        teams: [0; 9],
        economy: Economy::default(),
        minimap_obj: Vec::new(),
        groups: Vec::new(),
    };
    for pl in &players {
        if let Some(r) = p.races.get_mut(pl.id as usize) {
            *r = race(pl.id);
            p.starts[pl.id as usize] = (pl.start_x, pl.start_y);
        }
    }
    for (i, n) in land.nodes.iter().enumerate() {
        let id = n.object as u16;
        if id == 0 || matches!(id, 162 | 209 | 210 | 211) {
            continue;
        }
        let (x, y) = ((i % size) as u16, (i / size) as u16);
        let deco = match id {
            124..=136 => Deco { x, y, id: 41, stage: 0, frame: 0, amount: (id - 123) as u8, cycle: 1 },
            _ => Deco { x, y, id, stage: if matches!(id, 1..=22 | 255 | 212..=214) { 3 } else { 0 }, frame: 0, amount: 0, cycle: 0 },
        };
        p.decos.push(deco);
    }
    for b in map.buildings()? {
        let mut inhabitants = Vec::new();
        if b.has_garrison != 0 {
            for (j, &n) in b.soldiers.iter().enumerate() {
                inhabitants.extend(std::iter::repeat_n(Inhabitant { kind: 29 + j as u16, race: race(b.owner), slot: None, unit: None }, n as usize));
            }
        }
        p.buildings.push(Building { id: 0, x: b.x, y: b.y, kind: b.kind, owner: b.owner, race: race(b.owner), inhabitants, site: None, worker: None, door: (b.x, b.y), land: 0, ready: true, generic: b.kind as u16, flag: None, pen: None, mushroom: None });
    }
    for s in map.settlers()? {
        let u = Unit { obj: 1, x: s.x, y: s.y, kind: s.kind as u16, owner: s.owner, race: race(s.owner), ..Default::default() };
        match s.vehicle_type() {
            Some(vt) => p.vehicles.push(Unit { kind: vt as u16, ..u }),
            None if s.kind < 67 => p.settlers.push(u),
            None => {}
        }
    }
    for pl in map.piles()? {
        p.piles.push(Pile { x: pl.x, y: pl.y, good: pl.good, amount: pl.amount });
    }
    Ok(p)
}
