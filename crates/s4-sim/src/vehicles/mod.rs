//! Vehicles: `CVehicle` (vtable 0x105cf58, ctor 0x593f60), `CWheeler` (land vehicles, ctor
//! 0x599ec0) and `CVehicleMgr` (0x596300..0x597530), with the classes `CCart`, `C*Catapult`,
//! `CFerryShip`, `CTransportShip`, `C*WarShip` (see `docs/VEHICLES.md`). Own reverse
//! engineering; decompiles in `tools/re/vehicles/`.
//!
//! **Types** (`VEHICLE_*`): 1 warship, 2 ferry, 3 transport ship (obj type 2, ships), 4 war
//! machine (catapult), 5 cart (obj type 4, land vehicles, `CWheeler`). The Dark Tribe has no
//! warship and no catapult (`Create` 0x596b80 reports "can't use this vehicle").
//!
//! **The manager** keeps per `[player · 6 + type]` an intrusive list of the vehicles (head
//! 0x1549bd0, newest first, through `IEntity +0x28 / +0x2a`) and three counters (0x1549a8c: +2 under
//! construction, +4 finished), and the statistic's per-type counts (`CStatistic` +0xb64 created,
//! +0xb80 finished).
//!
//! **A land vehicle on the map** (`CWheeler` 0x59a4c0) holds its node in the map-object layer
//! and marks the six neighbours with the placeholder ids 1..=6 (their direction + 1); moving or
//! dying clears the seven nodes (0x59a520).

pub mod ammo;
pub mod cart;
pub mod catapult;
pub mod ferry;
pub mod info;
pub mod logic;
pub mod save;
pub mod scripted;
pub mod ship;
pub mod transport;
pub mod warship;

use crate::World;
use crate::entity::{Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;

pub const PLAYERS: usize = 9;
pub const TYPES: usize = 6;

/// `VEHICLE_*`.
pub mod kind {
    pub const WARSHIP: u16 = 1;
    pub const FERRY: u16 = 2;
    pub const TRANSPORT_SHIP: u16 = 3;
    pub const WAR_MACHINE: u16 = 4;
    pub const CART: u16 = 5;
    /// `AddVehicle`'s cart variant (0x406): a foundation cart that comes filled (`CCart`
    /// 0x55aae0 then 0x55e020); its type is 5.
    pub const TRADE_CART: u16 = 0x406;
}

/// One `CEntityTask` of a vehicle's own list (+0x80, 0x18 bytes each): the stored fields
/// +4..=+0x15 in store order (0x567400).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VehicleTask {
    pub task: u8,
    pub good: u8,
    pub dir: i8,
    pub duration: i8,
    pub frames: i8,
    pub trigger: u8,
    pub x: i16,
    pub y: i16,
    pub job_part: u16,
    pub entity: u16,
    pub w12: u16,
    pub visible: bool,
    pub forward: bool,
}

/// The `CVehicle` / `CWheeler` members (the `IEntity` part is the [`Entity`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Vehicle {
    /// +0x21 hit points (`VehicleInfo` +0x30).
    pub health: u8,
    /// +0x24 frame, +0x26 gfx job.
    pub frame: u8,
    pub job_part: u16,
    /// +0x2c the tick of the last animation update.
    pub anim_tick: u32,
    /// +0x40 direction 0..=5, +0x41 distance to the next node, +0x42 displacement costs.
    pub dir: i8,
    pub distance: u8,
    pub displacement: u8,
    /// The walker (+0x4c: `CWalkingCatapult` for land vehicles, `CWalkingShip` for ships).
    pub walking: Option<Box<crate::pathing::walking::WalkState>>,
    /// +0x5c (word, 8 after the ctor), +0x5d turn steps left, +0x5e the 16-way direction (bit 7
    /// turning clockwise), +0x5f the task (6 walking, 0x10 animation, 0x11 idle), +0x60 frame
    /// half-steps, +0x61 frames of the job, +0x62 walk speed (`WalkSteps`), +0x63 cargo / ammo,
    /// +0x64 a task list is running, +0x65 tasks of the list.
    pub b5c: u8,
    pub turn: u8,
    pub dir16: u8,
    pub task: u8,
    pub half_frame: u8,
    pub frames: u8,
    pub speed: u8,
    pub cargo: u8,
    pub listed: u8,
    pub list_len: u8,
    /// +0x66 flags (vtbl+0x54 / +0x5c / +0x60).
    pub s66: u16,
    /// +0x68 the tick of the next action.
    pub next_tick: i32,
    /// +0x6c / +0x6d boards needed / still to bring, +0x6e / +0x6f ironbars, +0x70 the build
    /// progress, +0x72 its end (boards + ironbars), +0x74 `0xfffe / +0x72`, +0x76, +0x78
    /// (`InitBuildInfo` 0x594750).
    pub build: [u8; 4],
    pub progress: [u16; 5],
    /// +0x7c (a float in the original; kept as its millionths).
    pub f7c: i64,
    /// +0x80 the task list, +0x8c the passengers.
    pub tasks: Vec<VehicleTask>,
    pub passengers: Vec<u16>,
    /// `CWheeler` +0x94 target position, +0x98 entity, +0x9c.
    pub target_pos: i32,
    pub target_entity: i32,
    pub x9c: i32,
    /// `IAnimatedEntity::m_iEventQueue`.
    pub events: Vec<crate::jobs::state::EntityEvent>,
    /// The `CCart` members (carts only).
    pub cart: Option<Box<cart::CartData>>,
    /// The `CCatapult` members of war machines and the behaviour members of war ships (+0x98,
    /// +0xb0; their timer is +0x94, `x9c`).
    pub catapult: Option<Box<catapult::CatapultData>>,
    /// The `CFerryShip` members (ferries only).
    pub ferry: Option<Box<ferry::FerryData>>,
    /// The `CTransportShip` members (transport ships only).
    pub transport: Option<Box<transport::TransportData>>,
}

impl Vehicle {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for v in [self.health, self.frame, self.dir as u8, self.distance, self.displacement, self.b5c, self.turn, self.dir16, self.task, self.half_frame, self.frames, self.speed, self.cargo, self.listed, self.list_len] {
            h.u8(v);
        }
        h.u16(self.job_part);
        h.u16(self.s66);
        h.u32(self.anim_tick);
        h.u32(self.next_tick as u32);
        h.bytes(&self.build);
        h.u16s(&self.progress);
        h.u32(self.f7c as u32);
        h.u32(self.tasks.len() as u32);
        for t in &self.tasks {
            for v in [t.task, t.good, t.dir as u8, t.duration as u8, t.frames as u8, t.trigger, t.visible as u8, t.forward as u8] {
                h.u8(v);
            }
            for v in [t.x as u16, t.y as u16, t.job_part, t.entity, t.w12] {
                h.u16(v);
            }
        }
        h.u16s(&self.passengers);
        for v in [self.target_pos, self.target_entity, self.x9c] {
            h.u32(v as u32);
        }
        h.u32(self.events.len() as u32);
        for e in &self.events {
            for v in [e.event, e.kind, e.data[0], e.data[1], e.data[2]] {
                h.u32(v as u32);
            }
        }
        match &self.walking {
            None => h.u8(0),
            Some(w) => {
                h.u8(1);
                w.hash(h);
            }
        }
        if let Some(c) = &self.cart {
            c.hash(h);
        }
        if let Some(c) = &self.catapult {
            c.hash(h);
        }
        if let Some(f) = &self.ferry {
            f.hash(h);
        }
        if let Some(t) = &self.transport {
            t.hash(h);
        }
    }
}

/// `CVehicleMgr`'s lists and counters, and the statistic's vehicle counts.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VehicleMgr {
    /// 0x1549bd0 `[player · 6 + type]`: the list heads.
    pub heads: Vec<u16>,
    /// 0x1549a8c `[player · 6 + type]`: (+2) under construction, (+4) finished.
    pub building: Vec<u16>,
    pub finished: Vec<u16>,
    /// `CStatistic` +0xb64 / +0xb80 per player and type: made, finished.
    pub stat_made: Vec<i32>,
    pub stat_finished: Vec<i32>,
    /// The ship cells (0x1443cb8, `ship`).
    pub ship_cells: ship::ShipCells,
}

impl Default for VehicleMgr {
    fn default() -> Self {
        let n = PLAYERS * TYPES;
        VehicleMgr {
            heads: vec![0; n],
            building: vec![0; n],
            finished: vec![0; n],
            stat_made: vec![0; n],
            stat_finished: vec![0; n],
            ship_cells: ship::ShipCells::default(),
        }
    }
}

impl VehicleMgr {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u16s(&self.heads);
        h.u16s(&self.building);
        h.u16s(&self.finished);
        self.stat_made.iter().chain(&self.stat_finished).for_each(|&v| h.u32(v as u32));
        self.ship_cells.hash(h);
    }

    fn slot(player: u8, kind: u16) -> Option<usize> {
        ((player as usize) < PLAYERS && (kind as usize) < TYPES).then(|| player as usize * TYPES + kind as usize)
    }

    /// The vehicles of `player` and `kind`, newest first.
    pub fn head(&self, player: u8, kind: u16) -> u16 {
        Self::slot(player, kind).map_or(0, |i| self.heads[i])
    }
}

/// The vehicle data of entity `id`.
pub fn get(world: &World, id: u16) -> Option<&Vehicle> {
    match &world.entities.by_index(id as u32)?.data {
        EntityData::Vehicle(v) => Some(&v.0),
        _ => None,
    }
}

pub fn get_mut(world: &mut World, id: u16) -> Option<&mut Vehicle> {
    match &mut world.entities.by_index_mut(id as u32)?.data {
        EntityData::Vehicle(v) => Some(&mut v.0),
        _ => None,
    }
}

/// The vehicles of `player`'s list of `kind`, head first.
pub fn iter(world: &World, player: u8, kind: u16) -> impl Iterator<Item = u16> + '_ {
    let mut cur = world.vehicles.head(player, kind);
    std::iter::from_fn(move || {
        if cur == 0 {
            return None;
        }
        let id = cur;
        cur = world.entities.by_index(id as u32).map_or(0, |e| e.next);
        Some(id)
    })
}

/// The six neighbour offsets as node index deltas (`DAT_01454108`, the grid's neighbour table in
/// direction order).
fn neighbours(world: &World, node: usize) -> [Option<usize>; 6] {
    let (x, y) = ((node % world.grid.size() as usize) as i32, (node / world.grid.size() as usize) as i32);
    std::array::from_fn(|d| {
        let (dx, dy) = crate::jobs::walking::NEIGHBOR[d];
        world.grid.checked_index(x + dx, y + dy)
    })
}

/// `CWheeler` 0x59a4c0: the vehicle `id` takes `pos` in the map-object layer, its six neighbours
/// the placeholders 1..=6.
pub fn put_on_map(world: &mut World, id: u16, pos: Y16X16) {
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return };
    world.layers.map_object[n] = id;
    for (d, m) in neighbours(world, n).into_iter().enumerate() {
        if let Some(m) = m {
            world.layers.map_object[m] = d as u16 + 1;
        }
    }
}

/// `CWheeler` 0x59a520: the seven nodes of a land vehicle at `pos` are cleared.
pub fn take_off_map(world: &mut World, pos: Y16X16) {
    let Some(n) = world.grid.checked_index(pos.x(), pos.y()) else { return };
    world.layers.map_object[n] = 0;
    for m in neighbours(world, n).into_iter().flatten() {
        world.layers.map_object[m] = 0;
    }
}

/// `CVehicleMgr::AddVehicle(x, y, player, type, dir, under_construction)` (0x596ad0) with
/// `Create` (0x596b80) and the ctors (`CVehicle` 0x593f60, `CWheeler` 0x599ec0): the vehicle
/// joins the head of its list, the counters (+2 when under construction, else +4) and the
/// statistic count. `dir` < 0 draws a direction from the game RNG (`% 6`). Types without a class
/// (Dark Tribe warship / catapult, types outside 1..=5) make nothing.
pub fn add_vehicle(world: &mut World, x: i32, y: i32, player: u8, kind_in: u16, dir: i32, under_construction: bool) -> Option<u16> {
    let kind = if kind_in == kind::TRADE_CART { kind::CART } else { kind_in };
    let race = world.race_of(player);
    if !(1..=5).contains(&kind) || (race == 3 && matches!(kind, kind::WARSHIP | kind::WAR_MACHINE)) {
        return None;
    }
    let slot_i = VehicleMgr::slot(player, kind)?;
    let id = world.entities.free_slot()?;
    let info = world.rules.vehicles.get(race, kind as u8);
    // CVehicle ctor 0x593f60.
    let dir = if dir < 0 { (world.rng.rand() % 6) as i8 } else { dir as i8 };
    let obj_type = if info.obj_type == 2 { ObjType::Ship } else { ObjType::LandVehicle };
    let mut v = Vehicle {
        health: info.hitpoints.clamp(0, 255) as u8,
        frame: 0,
        // +0x26 the type's first gfx job, +0x61 its frames (both branches of the ctor).
        job_part: world.rules.vehicles.first_job(race, kind as u8),
        anim_tick: 0,
        dir,
        distance: 0,
        displacement: 10,
        walking: None,
        b5c: 8,
        turn: 0,
        dir16: 0,
        task: 0,
        half_frame: 0,
        frames: world.rules.vehicles.frame_count(race, world.rules.vehicles.first_job(race, kind as u8)),
        speed: info.walk_steps.clamp(0, 255) as u8,
        cargo: 0,
        listed: 0,
        list_len: 0,
        s66: 0,
        next_tick: 0,
        build: [0; 4],
        progress: [0; 5],
        f7c: 0,
        tasks: Vec::new(),
        passengers: Vec::new(),
        target_pos: 0,
        target_entity: 0,
        x9c: 0,
        events: Vec::new(),
        cart: None,
        catapult: None,
        ferry: None,
        transport: None,
    };
    let mut flags = info.flags | 0x0203_0100;
    if under_construction {
        // InitBuildInfo 0x594750: READY off, BIRTH and VISIBLE on.
        let (b, i) = (info.boards.clamp(0, 255) as u8, info.ironbars.clamp(0, 255) as u8);
        v.build = [b, b, i, i];
        let sum = b as u16 + i as u16;
        v.progress = [0, sum, 0xfffe / sum.max(1), 1, 0];
        flags = 0x0100_0100;
    }
    flags |= (info.x10 as u32) & flag::WARRIOR_TYPE_MASK;
    let mut e = Entity::new(obj_type, kind, player, race, Y16X16::pack(x, y), EntityData::Vehicle(crate::entity::VehicleData(Box::new(v))));
    e.flags = (e.flags & !0x2800) | flags;
    world.entities.insert(id, e);
    if under_construction {
        world.register_for_logic_update(id, 16);
    }
    // The walker: CWalkingCatapult (3) / CWalkingShip (4), cancelled (+8(−1, 0)).
    let mut w = crate::pathing::walking::WalkState::new(info.walker.clamp(0, 255) as u8, player);
    w.init(world, -1, 0);
    if let Some(v) = get_mut(world, id) {
        v.walking = Some(Box::new(w));
    }
    if obj_type == ObjType::LandVehicle {
        // CWheeler ctor: on the map, into the war map.
        put_on_map(world, id, Y16X16::pack(x, y));
        crate::warmap::readd(world, id);
    }
    if kind == kind::CART {
        cart::init(world, id, under_construction, kind_in == kind::TRADE_CART);
    }
    if kind == kind::WAR_MACHINE && obj_type == ObjType::LandVehicle {
        catapult::init(world, id, under_construction);
    }
    if obj_type == ObjType::Ship {
        ship::init(world, id, under_construction);
        if kind == kind::FERRY {
            ferry::init(world, id);
        }
        if kind == kind::TRANSPORT_SHIP {
            transport::init(world, id);
        }
        if kind == kind::WARSHIP {
            warship::init(world, id);
        }
    }
    // AddVehicle: counters, list head, statistic.
    let m = &mut world.vehicles;
    if under_construction {
        m.building[slot_i] += 1;
    } else {
        m.finished[slot_i] += 1;
    }
    let old = m.heads[slot_i];
    m.heads[slot_i] = id;
    m.stat_made[slot_i] += 1;
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.prev = 0;
        e.next = old;
    }
    if old != 0
        && let Some(o) = world.entities.by_index_mut(old as u32)
    {
        o.prev = id;
    }
    Some(id)
}

/// `CVehicleMgr` 0x597150 (the vehicle is deleted): out of its list, one less in its counter
/// (+2 when BIRTH, or neither alive flag but 0x100000; else +4) and the statistic.
pub fn on_delete(world: &mut World, id: u16) {
    if ferry::is_ferry(world, id) {
        ferry::on_delete(world, id);
    }
    cart::on_delete(world, id);
    if catapult::is_catapult(world, id) {
        catapult::on_delete(world, id);
    }
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (player, kind, flags, prev, next, pos, obj) = (e.owner, e.kind, e.flags, e.prev, e.next, e.pos, e.obj_type);
    let Some(i) = VehicleMgr::slot(player, kind) else { return };
    if prev == 0 {
        world.vehicles.heads[i] = next;
    } else if let Some(p) = world.entities.by_index_mut(prev as u32) {
        p.next = next;
    }
    if next != 0
        && let Some(n) = world.entities.by_index_mut(next as u32)
    {
        n.prev = prev;
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.prev = 0;
        e.next = 0;
    }
    let birth = flags & flag::BIRTH != 0 || (flags & flag::ALIVE_MASK == 0 && flags & 0x10_0000 != 0);
    let m = &mut world.vehicles;
    let c = if birth { &mut m.building[i] } else { &mut m.finished[i] };
    *c = c.saturating_sub(1);
    m.stat_made[i] -= m.stat_made[i].min(1);
    // `CVehicle` 0x594a10 (Delete, vtbl+0x10): vtbl+0xc8 only for a vehicle with a position (one
    // aboard a ferry has none), and the position is then 0; out of the logic update (0x56d720
    // with its slot +0x30). Without that a killed ship walked on for its last 31 ticks and left
    // its cell entry and marks (0x800) behind, a rock for every ship after it.
    if pos.0 != 0 && pos.0 <= 0x7fff_ffff {
        if obj == ObjType::LandVehicle {
            take_off_map(world, pos);
        }
        if obj == ObjType::Ship {
            // vtbl+0xc8: a transport ship's ports forget it first (0x591890).
            transport::off_map(world, id);
            ship::off_map(world, id);
        }
        if let Some(e) = world.entities.by_index_mut(id as u32) {
            e.pos = Y16X16(0);
        }
    }
    world.unregister_from_logic_update(id);
    // 0x594ac3..0x594ae8: last, the AI notice 0x17 (owner, vehicle, 0): out of its task force
    // (0x4456b0), then to every player AI.
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    crate::ai::post_event(world, 0x17, owner as i32, id as i32, 0);
}

fn unlink(world: &mut World, id: u16, i: usize) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (prev, next) = (e.prev, e.next);
    if prev == 0 {
        world.vehicles.heads[i] = next;
    } else if let Some(p) = world.entities.by_index_mut(prev as u32) {
        p.next = next;
    }
    if next != 0
        && let Some(n) = world.entities.by_index_mut(next as u32)
    {
        n.prev = prev;
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.prev = 0;
        e.next = 0;
    }
}

fn link_head(world: &mut World, id: u16, i: usize) {
    let old = world.vehicles.heads[i];
    world.vehicles.heads[i] = id;
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.prev = 0;
        e.next = old;
    }
    if old != 0
        && let Some(o) = world.entities.by_index_mut(old as u32)
    {
        o.prev = id;
    }
}

/// `CVehicle` 0x594830 (vtbl+0xa0): the vehicle under construction is done: out of and back
/// into its list (now counted finished), BIRTH off, the class flags and 0x2030100 on, into the
/// war map, its first task (vtbl+0xbc), the statistic's finished count; the local player hears
/// of a ship or war machine (`0x52a320`: 1 0x7ab, 2 0x77c, 3 0x785, catapults by race 0x777 /
/// 0x7aa / 0x784 / Trojan 0x122; carts none).
pub fn finish(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (player, k, race, pos) = (e.owner, e.kind, e.race, e.pos);
    let Some(i) = VehicleMgr::slot(player, k) else { return };
    unlink(world, id, i);
    let info = world.rules.vehicles.get(race, k as u8);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags = (e.flags & !flag::BIRTH) | info.flags | 0x0203_0100;
    }
    let m = &mut world.vehicles;
    m.building[i] = m.building[i].saturating_sub(1);
    m.finished[i] += 1;
    link_head(world, id, i);
    crate::warmap::readd(world, id);
    logic::take_next_task(world, id);
    world.vehicles.stat_finished[i] += 1;
    let text = match k {
        kind::WARSHIP => Some(0x7ab),
        kind::FERRY => Some(0x77c),
        kind::TRANSPORT_SHIP => Some(0x785),
        kind::WAR_MACHINE => Some(match race {
            1 => 0x7aa,
            2 => 0x784,
            4 => 0x122,
            _ => 0x777,
        }),
        _ => None,
    };
    if let Some(text) = text {
        world.events.push(crate::event::SimEvent::Message { player, text, x: pos.x(), y: pos.y(), filtered: false });
    }
}

/// `CVehicleMgr` 0x597020 `(x, y, type)`: whether a vehicle of `type` can be made at `(x, y)`.
/// Land vehicles (4, 5): the node lies in a catapult-tiling sector (tile id below 0x8000 whose
/// sector is not 0, 0x12f1a1c / 0x12f1a24) and the node and its six neighbours hold no map
/// object. Ships (1..=3, 0x5bc2f0): open water (`& 0xcb00 == 0xc000`) with a ship-cell slot
/// (`ship::can_place`). The original reads the neighbours unchecked; outside the map they count
/// as taken here.
pub fn can_place(world: &World, x: i32, y: i32, kind: u16) -> bool {
    let Some(n) = world.grid.checked_index(x, y) else { return false };
    match kind {
        kind::WAR_MACHINE | kind::CART => {
            let t = world.tiling.catapult.get(n).copied().filter(|&t| t < 0x8000).unwrap_or(0);
            if world.tiling.tiles.get(t as usize).is_none_or(|t| t.sector == 0) {
                return false;
            }
            world.layers.map_object[n] == 0 && neighbours(world, n).iter().all(|m| m.is_some_and(|m| world.layers.map_object[m] == 0))
        }
        kind::WARSHIP | kind::FERRY | kind::TRANSPORT_SHIP => ship::can_place(world, x, y),
        _ => false,
    }
}

/// `CVehicleMgr` 0x597100: a vehicle (obj type 2 / 4) is killed (deselecting it first, 0x4eda10,
/// is the client's).
pub fn delete(world: &mut World, id: u16) {
    if id != 0 && world.entities.by_index(id as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle)) {
        world.kill(id, 0);
    }
}

/// A cart (`CCart`): the class whose passengers / goods calls are ported.
fn is_cart(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::LandVehicle && e.kind == kind::CART)
}

/// vtbl+0x8c `(obj type, entity)`: where `entity` coming to the vehicle goes (`CCart` 0x55ae60,
/// the catapults' 0x561580, `CShip` 0x590b80, `CFerryShip` 0x58ea60).
pub fn exit_place(world: &mut World, id: u16, entity: u16) -> i32 {
    if is_cart(world, id) {
        return cart::exit_place(world, id);
    }
    if catapult::is_catapult(world, id) {
        return catapult::exit_place(world, id);
    }
    if ferry::is_ferry(world, id) {
        return ferry::exit_place(world, id, entity);
    }
    if ship::is_ship(world, id) {
        return ship::exit_place(world, id);
    }
    0
}

/// vtbl+0xa4 (`CVehicle` 0x5955e0 → 0x56e0a0): `entity` joins the observer list, the vehicle is
/// its home.
pub fn attach(world: &mut World, id: u16, entity: u16) {
    cart::attach(world, id, entity);
}

/// vtbl+0x40: an entity is detached (`CCart` 0x55d310; `CVehicle` 0x595600 for the others: off
/// the observer list, no home, then vtbl+0xc0 — the ferry's 0x58d8b0, empty for the others).
pub fn detach_settler(world: &mut World, id: u16, settler: u16) {
    if is_cart(world, id) {
        cart::detach_settler(world, id, settler);
    } else {
        cart::detach_observer(world, id, settler);
        if ferry::is_ferry(world, id) {
            ferry::detached(world, id, settler);
        }
    }
}

/// vtbl+0x7c `(entity)`: a passenger that is not inside yet leaves (`CCart` 0x55d350,
/// `CFerryShip` 0x58d720; `CVehicle` 0x594b20 does nothing).
pub fn remove_passenger(world: &mut World, id: u16, entity: u16) {
    if is_cart(world, id) {
        cart::remove_settler(world, id, entity);
    } else if ferry::is_ferry(world, id) {
        ferry::remove_passenger(world, id, entity);
    }
}

/// vtbl+0x80: a settler checks in (`CCart` 0x55d0c0, `CFerryShip` 0x58e6b0; 0x594b00 does
/// nothing).
pub fn settler_entered(world: &mut World, id: u16, settler: u16) {
    if is_cart(world, id) {
        cart::settler_entered(world, id, settler);
    } else if ferry::is_ferry(world, id) {
        ferry::settler_entered(world, id, settler);
    }
}

/// vtbl+0x78 / +0x84 / +0x88 `(good, n)`: goods ordered / arrived / not coming (`CCart`
/// 0x55d760 / 0x55d7e0 / 0x55d880).
pub fn good_ordered(world: &mut World, id: u16, good: i32, n: i32) {
    if is_cart(world, id) {
        cart::good_ordered(world, id, good, n);
    }
    // Catapults and ships: 0x594b10, nothing.
}

pub fn good_arrived(world: &mut World, id: u16, good: i32, n: i32) {
    if is_cart(world, id) {
        cart::good_arrived(world, id, good, n);
    } else if catapult::is_catapult(world, id) {
        catapult::ammo_arrived(world, id, n);
    }
    // Ships: 0x594b30, nothing.
}

pub fn good_cancelled(world: &mut World, id: u16, good: i32, n: i32) {
    if is_cart(world, id) {
        cart::good_cancelled(world, id, good, n);
    } else if catapult::is_catapult(world, id) {
        catapult::carrier_gone(world, id);
    }
    // Ships: 0x594b40, nothing.
}

/// vtbl+0x20 `TakeDamage(n)` of a vehicle: the cart's 0x55c1d0, else `CVehicle` 0x5956d0
/// (ships: the same base).
pub fn take_damage(world: &mut World, id: u16, n: i32, attacker: u8) {
    if is_cart(world, id) {
        cart::hit(world, id, n);
    } else {
        catapult::take_damage(world, id, n, attacker);
    }
}

/// The house worker's PRODUCE_VIRTUAL into a vehicle under construction (inline in
/// `CHouseWorkerRole::LogicUpdateJob`, 0x57c7f3..0x57c809): boards (7) lower +0x6d, ironbars
/// (20) +0x6f, and either raises the progress +0x70; any other good changes nothing.
pub fn add_building_material(world: &mut World, id: u16, good: u8) {
    let Some(v) = get_mut(world, id) else { return };
    match good {
        7 => v.build[1] = v.build[1].wrapping_sub(1),
        20 => v.build[3] = v.build[3].wrapping_sub(1),
        _ => return,
    }
    v.progress[0] = v.progress[0].wrapping_add(1);
}
