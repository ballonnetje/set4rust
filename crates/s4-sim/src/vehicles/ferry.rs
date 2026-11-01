//! `CFerryShip` (vtable 0x105c60c, 0x58cc00..0x58f000): a ship that carries settlers and land
//! vehicles. Decompiles `tools/re/vehicles/ferry.c`, `ferry2.c`, `ferry3.c`, `shipctors.c`.
//!
//! * **Members** (ctor 0x58cda0, load 0x58cf50): +0x98 the land search radius (10), +0x9c the
//!   farthest a passenger may stand (50), +0xa0 the passenger records (0x18 bytes: obj type,
//!   type, count, id, the node it was put ashore on, +0x14 arrived, +0x15 aboard), +0xac the
//!   weight aboard, +0xb0 the weight reserved, +0xb4 the capacity (15), +0xb8 the state (0
//!   loaded, 1 sailing to the passengers, 2 waiting there), +0xbc the task of the last update.
//!   A settler weighs 1, a land vehicle of type 4 / 5 5 (0x58cd20).
//! * **Boarding** (0x58e760): a move order onto a ferry (a settler role through 0x5800b0, a
//!   `CWheeler` 0x59a200) makes the passenger a record (reserved), sends it event 0x18 with the
//!   ferry and its exit place (0x58ea60: a free land node near the land nearest to the ferry),
//!   and sends the ferry to the first water node of the spiral around that place. The passenger
//!   walks there and checks in (vtbl+0x80, 0x58e6b0): arrived; a ferry that waits or stands
//!   takes everyone who arrived aboard (0x58e0a0: off the map, ON_BOARD, detached), else it does
//!   so when its walk ends (0x58e060).
//! * **Sailing**: when the ferry starts walking (its task becomes 6, 0x58d2e0), every passenger
//!   not aboard yet gets event 7 and is dropped — including the ones whose boarding just sent it
//!   off, unless the ferry already stood on that water node (state 2; the original's order).
//! * **Unloading** (5009 `VehicleUnload`, vtbl+0x98 0x58d8e0): around the land node nearest to
//!   the ferry (within 10) every passenger aboard gets a free node (0x58dd70) and event 0x19
//!   (0x58da60) and leaves the list. A land vehicle that cannot be put there comes back aboard
//!   (0x58ecd0).
//! * **Deleted** (vtbl+0x10, 0x58d560): passengers aboard die with it (vehicles also when not
//!   aboard), settlers still on their way get event 7.

use super::{get, get_mut, logic};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// +0x98: the radius of the land searches.
pub const RADIUS: u8 = 10;
/// +0x9c: a passenger must stand nearer than this.
pub const REACH: i32 = 50;
/// +0xb4: the capacity.
pub const CAPACITY: i32 = 15;

/// One passenger record (+0xa0, 0x18 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Passenger {
    /// +0 obj type, +4 type, +8 count (always 1 while listed), +0xc id.
    pub obj_type: u8,
    pub kind: u16,
    pub count: i32,
    pub id: u16,
    /// +0x10 the node it was put ashore on (0 before).
    pub pos: i32,
    /// +0x14 checked in at the exit place, +0x15 aboard.
    pub arrived: bool,
    pub aboard: bool,
}

/// The `CFerryShip` members (see the module doc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FerryData {
    pub passengers: Vec<Passenger>,
    /// +0xac / +0xb0 the weight aboard and reserved.
    pub aboard: i32,
    pub reserved: i32,
    /// +0xb8, +0xbc.
    pub state: i32,
    pub last_task: u8,
}

impl FerryData {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u32(self.passengers.len() as u32);
        for p in &self.passengers {
            h.u8(p.obj_type);
            h.u16(p.kind);
            h.u32(p.count as u32);
            h.u16(p.id);
            h.u32(p.pos as u32);
            h.u8(p.arrived as u8 | (p.aboard as u8) << 1);
        }
        for v in [self.aboard, self.reserved, self.state] {
            h.u32(v as u32);
        }
        h.u8(self.last_task);
    }
}

pub fn data(world: &World, id: u16) -> Option<&FerryData> {
    get(world, id)?.ferry.as_deref()
}

fn data_mut(world: &mut World, id: u16) -> Option<&mut FerryData> {
    get_mut(world, id)?.ferry.as_deref_mut()
}

pub fn is_ferry(world: &World, id: u16) -> bool {
    data(world, id).is_some()
}

/// 0x58cd20 / 0x58cd60: a land vehicle of type 4 / 5 weighs 5, anything else 1.
pub fn weight(obj_type: u8, kind: u16) -> i32 {
    if obj_type == ObjType::LandVehicle as u8 && matches!(kind, 4 | 5) { 5 } else { 1 }
}

/// 0x58eb00 `(obj type, type)`: room for a settler (1) or a land vehicle of type 4 / 5 (5) in
/// both the weight aboard and the reserved one.
pub fn room(world: &World, id: u16, obj_type: u8, kind: u16) -> bool {
    let w = match obj_type {
        1 => 1,
        4 if matches!(kind, 4 | 5) => 5,
        _ => return false,
    };
    data(world, id).is_some_and(|f| f.aboard + w <= CAPACITY && f.reserved + w <= CAPACITY)
}

/// The ctor 0x58cda0 (after `CShip`'s): no passengers, the current task as the last one.
pub fn init(world: &mut World, id: u16) {
    if let Some(v) = get_mut(world, id) {
        v.ferry = Some(Box::new(FerryData { last_task: v.task, ..FerryData::default() }));
    }
}

fn pos_of(world: &World, id: u16) -> Y16X16 {
    world.entities.by_index(id as u32).map_or(Y16X16(0), |e| e.pos)
}

/// The node's ground type (`[0x15630d4 + n·4 + 1]`); water is ≤ 8. Outside the map: none.
fn ground(world: &World, x: i32, y: i32) -> Option<u8> {
    let size = world.grid.size() as i32;
    ((x as u32) < size as u32 && (y as u32) < size as u32).then(|| world.layers.ground[(y * size + x) as usize])
}

fn node(world: &World, x: i32, y: i32) -> usize {
    (y * world.grid.size() as i32 + x) as usize
}

/// 0x58df70: the land node (ground type > 8) nearest to the ferry on the spiral up to radius 10
/// (the first of equal distance); `(distance, x, y)`, distance 11 and `(x, y)` untouched without
/// one.
fn nearest_land(world: &World, id: u16, x: &mut i32, y: &mut i32) -> i32 {
    let p = pos_of(world, id);
    let mut best = RADIUS as i32 + 1;
    for k in 0..=crate::spiral::last(RADIUS) {
        let (dx, dy) = (crate::spiral::dx(k), crate::spiral::dy(k));
        if ground(world, p.x() + dx, p.y() + dy).is_some_and(|g| g > 8) {
            let d = crate::spiral::grid_distance(dx, dy);
            if d < best {
                *x = p.x() + dx;
                *y = p.y() + dy;
                best = d;
            }
        }
    }
    best
}

/// 0x58dba0 `(entity, x, y)`: on the spiral up to radius 10 around `(x, y)` the first land node
/// without a map object and not blocked (`m_cFlagMap` bit 1) — for a vehicle also one it can
/// stand on (0x597020 with its type) — that `CAStarTiling::FindPath` reaches from the entity's
/// node (flags 0 for a settler, 2 for anything else: the land mode either way).
fn free_node_for(world: &World, entity: u16, x: i32, y: i32) -> Option<(i32, i32)> {
    let e = world.entities.by_index(entity as u32)?;
    let (obj, kind, from) = (e.obj_type, e.kind, e.pos);
    let side = world.grid.size() as usize / 16;
    for k in 0..=crate::spiral::last(RADIUS) {
        let (cx, cy) = (x + crate::spiral::dx(k), y + crate::spiral::dy(k));
        if !ground(world, cx, cy).is_some_and(|g| g > 8) {
            continue;
        }
        let n = node(world, cx, cy);
        let ok = if obj == ObjType::Settler {
            world.layers.flags[n] & 1 == 0 && world.layers.map_object[n] == 0
        } else {
            super::can_place(world, cx, cy, kind) && world.layers.map_object[n] == 0
        };
        if !ok || world.layers.flags[n] & 1 != 0 {
            continue;
        }
        let mut wp = crate::pathing::Waypoints::default();
        let flags = if obj == ObjType::Settler { 0 } else { 2 };
        if crate::pathing::astar_tiling(&world.tiling, &world.alliances, &world.warmap, side, from, Y16X16::pack(cx, cy), flags, &mut wp) != 0 {
            return Some((cx, cy));
        }
    }
    None
}

/// vtbl+0x8c (0x58ea60) `(obj type, entity)`: where `entity` boards: with land within 10 of the
/// ferry and the entity nearer than 50, a node for it near that land (0x58dba0); else 0.
pub fn exit_place(world: &World, id: u16, entity: u16) -> i32 {
    let (mut x, mut y) = (0, 0);
    if nearest_land(world, id, &mut x, &mut y) > RADIUS as i32 {
        return 0;
    }
    let Some(e) = world.entities.by_index(entity as u32) else { return 0 };
    let p = pos_of(world, id);
    if crate::spiral::grid_distance(e.pos.x() - p.x(), e.pos.y() - p.y()) >= REACH {
        return 0;
    }
    free_node_for(world, entity, x, y).map_or(0, |(x, y)| Y16X16::pack(x, y).0 as i32)
}

/// The home of a settler (role +0x20, vtbl+0x48(0)) or a land vehicle (`CWheeler` +0x98).
fn home_of(world: &World, id: u16) -> i32 {
    match world.entities.by_index(id as u32).map(|e| e.obj_type) {
        Some(ObjType::Settler) => crate::jobs::core::job(world, id).map_or(0, |j| j.home as i32),
        Some(ObjType::LandVehicle) => get(world, id).map_or(0, |v| v.target_entity),
        _ => 0,
    }
}

/// 0x58ebf0: `entity` has a record.
fn listed(world: &World, id: u16, entity: u16) -> bool {
    data(world, id).is_some_and(|f| f.passengers.iter().any(|p| p.id == entity))
}

/// 0x58e760 `(entity)`: `entity` (a settler or a land vehicle) boards the ferry (module doc).
/// 0 on success; 2 the ferry is not READY / not a passenger kind / no exit place, 3 no room, 4
/// another player's, already with a home or already listed.
pub fn board(world: &mut World, id: u16, entity: u16) -> u32 {
    let Some(f) = world.entities.by_index(id as u32) else { return 2 };
    if f.flags & flag::READY == 0 {
        return 2;
    }
    let (fpos, fowner) = (f.pos, f.owner);
    let Some(e) = world.entities.by_index(entity as u32) else { return 2 };
    let (obj, kind, owner) = (e.obj_type, e.kind, e.owner);
    if !matches!(obj, ObjType::Settler | ObjType::LandVehicle) {
        return 2;
    }
    if !room(world, id, obj as u8, kind) {
        return 3;
    }
    if owner != fowner || home_of(world, entity) != 0 || listed(world, id, entity) {
        return 4;
    }
    let place = exit_place(world, id, entity);
    if place == 0 {
        return 2;
    }
    let place = Y16X16(place as u32);
    super::cart::send(world, entity, EntityEvent { event: 0x18, kind: 0, data: [id as i32, place.x(), place.y()] });
    // The first water node (ground type < 9) on the spiral up to radius 10 around the place.
    let mut to = place;
    for k in 0..=crate::spiral::last(10) {
        let (x, y) = (place.x() + crate::spiral::dx(k), place.y() + crate::spiral::dy(k));
        if ground(world, x, y).is_some_and(|g| g < 9) {
            to = Y16X16::pack(x, y);
            break;
        }
    }
    logic::set_event(world, id, EntityEvent { event: 0x11, kind: 0xd, data: [0, to.0 as i32, 0] });
    if let Some(f) = data_mut(world, id) {
        f.state = if fpos == to { 2 } else { 1 };
        f.passengers.push(Passenger { obj_type: obj as u8, kind, count: 1, id: entity, pos: 0, arrived: false, aboard: false });
        f.reserved += weight(obj as u8, kind);
    }
    0
}

/// vtbl+0x80 (0x58e6b0) `(entity)`: `entity` checked in at its exit place; a ferry that waits
/// there (2), is loaded (0) or stands takes everyone who arrived aboard.
pub fn settler_entered(world: &mut World, id: u16, entity: u16) {
    let Some(f) = data_mut(world, id) else { return };
    let Some(k) = f.passengers.iter().rposition(|p| p.id == entity) else { return };
    f.passengers[k].arrived = true;
    let state = f.state;
    let task = get(world, id).map_or(0, |v| v.task);
    if state == 2 || state == 0 || task == logic::task::IDLE {
        load_arrived(world, id);
        if let Some(f) = data_mut(world, id) {
            f.state = 0;
        }
    }
}

/// 0x58e0a0: everyone who arrived and is not aboard comes aboard: a land vehicle leaves the
/// map (0x59a520) with position 0 and is unregistered, a settler leaves the war map and its
/// node; ON_BOARD, detached from the ferry (vtbl+0x40), the weight aboard grows. False (and the
/// rest stays) at a record whose entity is neither.
fn load_arrived(world: &mut World, id: u16) -> bool {
    let mut k = 0;
    while let Some(p) = data(world, id).and_then(|f| f.passengers.get(k).copied()) {
        k += 1;
        if !p.arrived || p.aboard {
            continue;
        }
        let Some(e) = world.entities.by_index(p.id as u32) else { return false };
        let (obj, kind, pos) = (e.obj_type, e.kind, e.pos);
        match obj {
            ObjType::LandVehicle => {
                super::take_off_map(world, pos);
                if let Some(e) = world.entities.by_index_mut(p.id as u32) {
                    e.pos = Y16X16(0);
                }
                world.unregister_from_logic_update(p.id);
                if let Some(e) = world.entities.by_index_mut(p.id as u32) {
                    e.flags |= flag::ON_BOARD;
                }
            }
            ObjType::Settler => {
                if world.warmap.contains(p.id) {
                    crate::warmap::remove(world, p.id);
                }
                if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
                    world.layers.map_object[n] = 0;
                }
                if let Some(e) = world.entities.by_index_mut(p.id as u32) {
                    e.flags |= flag::ON_BOARD;
                }
                // (Deselected if selected, 0x4eda10: the client's.)
            }
            _ => return false,
        }
        super::detach_settler(world, id, p.id);
        if let Some(f) = data_mut(world, id) {
            f.aboard += weight(obj as u8, kind);
            if let Some(r) = f.passengers.get_mut(k - 1) {
                r.aboard = true;
            }
        }
    }
    // (A selected ferry refreshes its dialog, vtbl+0x94: the client's.)
    true
}

/// 0x58e060: the walk ended: sailing to the passengers (1), everyone who arrived comes aboard
/// (state 0); a refusal waits (2, update in 16 ticks).
fn arrived(world: &mut World, id: u16) {
    if data(world, id).is_none_or(|f| f.state != 1) {
        return;
    }
    let loaded = load_arrived(world, id);
    if let Some(f) = data_mut(world, id) {
        f.state = if loaded { 0 } else { 2 };
    }
    if !loaded {
        world.register_for_logic_update(id, 16);
    }
}

/// vtbl+0x7c (0x58d720) `(entity)`: `entity`'s record goes (its weight off the reserved one), the
/// ferry stops where it is (event 0x11 / 0xd) and is loaded (state 0).
pub fn remove_passenger(world: &mut World, id: u16, entity: u16) {
    let Some(f) = data(world, id) else { return };
    let Some(p) = f.passengers.iter().rev().find(|p| p.id == entity).copied() else { return };
    let here = pos_of(world, id);
    if let Some(f) = data_mut(world, id) {
        f.state = 0;
    }
    logic::set_event(world, id, EntityEvent { event: 0x11, kind: 0xd, data: [0, here.0 as i32, 0] });
    if let Some(f) = data_mut(world, id) {
        f.reserved -= p.count * weight(p.obj_type, p.kind);
        if let Some(k) = f.passengers.iter().position(|r| r.id == p.id) {
            f.passengers.remove(k);
        }
    }
}

/// vtbl+0xc0 (0x58d8b0) `(entity)` from the detach (0x595600): an entity that is no longer
/// alive (neither BIRTH nor READY) leaves the list.
pub fn detached(world: &mut World, id: u16, entity: u16) {
    if world.entities.by_index(entity as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK == 0) {
        remove_passenger(world, id, entity);
    }
}

/// 0x58eb60: records with a count below 1 go.
fn purge(world: &mut World, id: u16) {
    if let Some(f) = data_mut(world, id) {
        f.passengers.retain(|p| p.count >= 1);
    }
}

/// The ferry's update (vtbl+0x90, 0x58d2e0): the repair and moving part of the ships (with a
/// standing 0x1b taking the next task and then drifting), then, unless DIED, on a change of
/// task: a walk that ended (6) loads (0x58e060); a walk that starts drops everyone not aboard
/// (event 7 `[ferry]`, detached, the reserved weight back; a passenger whose entity is gone stops
/// the update before the last task is kept).
pub fn update(world: &mut World, id: u16) {
    super::ship::repair(world, id);
    // (A ferry with flag 0x400 refreshes its dialog, vtbl+0x94: the client's.)
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return };
    let t = get(world, id).map_or(logic::task::IDLE, |v| v.task);
    match t {
        logic::task::GO => super::ship::walk(world, id, pos, true),
        logic::task::IDLE => super::ship::walk(world, id, pos, false),
        0x1b => {
            logic::next_task(world, id);
            let pos = pos_of(world, id);
            super::ship::walk(world, id, pos, false);
        }
        _ => {}
    }
    if world.entities.by_index(id as u32).is_none_or(|e| e.flags & flag::DIED != 0) {
        return;
    }
    let (task, last) = (get(world, id).map_or(0, |v| v.task), data(world, id).map_or(0, |f| f.last_task));
    if task != last {
        if last == logic::task::GO {
            arrived(world, id);
        }
        if get(world, id).is_some_and(|v| v.task == logic::task::GO) {
            let mut k = 0;
            while let Some(p) = data(world, id).and_then(|f| f.passengers.get(k).copied()) {
                if p.count > 0 && !p.aboard {
                    let Some(e) = world.entities.by_index(p.id as u32) else { return };
                    let moving = matches!(e.obj_type, ObjType::Settler | ObjType::Ship | ObjType::LandVehicle);
                    if moving {
                        super::cart::send(world, p.id, EntityEvent { event: 7, kind: 0, data: [id as i32, 0, 0] });
                    }
                    super::detach_settler(world, id, p.id);
                    if let Some(f) = data_mut(world, id)
                        && let Some(r) = f.passengers.get_mut(k)
                    {
                        f.reserved -= weight(r.obj_type, r.kind) * r.count;
                        r.count = 0;
                        r.arrived = false;
                    }
                    if !moving {
                        return;
                    }
                }
                k += 1;
            }
            purge(world, id);
        }
    }
    let task = get(world, id).map_or(0, |v| v.task);
    if let Some(f) = data_mut(world, id) {
        f.last_task = task;
    }
}

/// 0x58dd70 `(obj type, x, y)`: on the spiral up to radius 10 around `(x, y)` the first node for
/// a passenger: a settler's not blocked and without a map object, a vehicle's one it can stand
/// on (0x597020 with the obj type as its type); without a map object and with no neighbour in
/// common with the six neighbours of any record's landing node (0 for those not put ashore).
fn unload_node(world: &World, id: u16, obj_type: u8, x: i32, y: i32) -> Option<(i32, i32)> {
    let side = world.grid.size() as i64;
    let nb: Vec<i64> = crate::jobs::walking::NEIGHBOR.iter().map(|&(dx, dy)| dy as i64 * side + dx as i64).collect();
    let recs: Vec<i64> = data(world, id).map_or(Vec::new(), |f| f.passengers.iter().map(|p| (p.pos >> 16) as i64 * side + (p.pos & 0xffff) as i64).collect());
    for k in 0..=crate::spiral::last(RADIUS) {
        let (cx, cy) = (x + crate::spiral::dx(k), y + crate::spiral::dy(k));
        if ground(world, cx, cy).is_none() {
            continue;
        }
        let n = node(world, cx, cy);
        let ok = if obj_type == 1 {
            world.layers.flags[n] & 1 == 0 && world.layers.map_object[n] == 0
        } else {
            super::can_place(world, cx, cy, obj_type as u16)
        };
        if !ok {
            continue;
        }
        let c = n as i64;
        let overlap = recs.iter().any(|&r| nb.iter().any(|&a| nb.iter().any(|&b| r + a == c + b)));
        if world.layers.map_object[n] == 0 && !overlap {
            return Some((cx, cy));
        }
    }
    None
}

/// 0x58da60 `(entity, x, y)`: event 0x19 to a land vehicle `[ferry, x, y]` or a settler
/// `[0, packed]`.
fn send_ashore(world: &mut World, id: u16, entity: u16, x: i32, y: i32) -> bool {
    match world.entities.by_index(entity as u32).map(|e| e.obj_type) {
        Some(ObjType::LandVehicle) => {
            logic::set_event(world, entity, EntityEvent { event: 0x19, kind: 0, data: [id as i32, x, y] });
            true
        }
        Some(ObjType::Settler) => {
            crate::jobs::core::set_event(world, entity, EntityEvent { event: 0x19, kind: 0, data: [0, Y16X16::pack(x, y).0 as i32, 0] });
            true
        }
        _ => false,
    }
}

/// 5009 `VehicleUnload` (vtbl+0x98, 0x58d8e0): unless DIED, with land within 10, every passenger
/// aboard that gets a node (0x58dd70 around the nearest land) is sent ashore (0x58da60): its
/// weights off, count 0, the node kept; then those records go.
pub fn unload(world: &mut World, id: u16) {
    if world.entities.by_index(id as u32).is_none_or(|e| e.flags & flag::DIED != 0) {
        return;
    }
    let (mut lx, mut ly) = (0, 0);
    if nearest_land(world, id, &mut lx, &mut ly) <= RADIUS as i32 {
        let mut k = 0;
        while let Some(p) = data(world, id).and_then(|f| f.passengers.get(k).copied()) {
            k += 1;
            if p.count <= 0 || !p.aboard {
                continue;
            }
            let Some((x, y)) = unload_node(world, id, p.obj_type, lx, ly) else { continue };
            if !send_ashore(world, id, p.id, x, y) {
                continue;
            }
            if let Some(f) = data_mut(world, id) {
                let w = weight(p.obj_type, p.kind) * p.count;
                f.aboard -= w;
                f.reserved -= w;
                if let Some(r) = f.passengers.get_mut(k - 1) {
                    r.count = 0;
                    r.aboard = false;
                    r.pos = Y16X16::pack(x, y).0 as i32;
                }
            }
        }
        purge(world, id);
    }
    // (vtbl+0x94: the dialog, the client's.)
}

/// 0x58ecd0 `(vehicle)`: a land vehicle that could not go ashore is a passenger aboard again
/// (ON_BOARD; only the weight aboard grows).
pub fn reboard(world: &mut World, id: u16, entity: u16) -> bool {
    let Some(e) = world.entities.by_index(entity as u32) else { return false };
    let (obj, kind) = (e.obj_type as u8, e.kind);
    if let Some(f) = data_mut(world, id) {
        f.passengers.push(Passenger { obj_type: obj, kind, count: 1, id: entity, pos: 0, arrived: true, aboard: true });
        f.aboard += weight(obj, kind);
    }
    if let Some(e) = world.entities.by_index_mut(entity as u32) {
        e.flags |= flag::ON_BOARD;
    }
    true
}

/// 0x58e2a0 `(entity)` (the script's ferry loading, 0x5c006f / 0x5c1600): with room, a settler or
/// vehicle is aboard at once: a record arrived and aboard, both weights grow; a settler takes its
/// wait list, leaves the map (vtbl+0x38), is unregistered and neither VISIBLE nor SELECTABLE; a
/// vehicle the same (off the map only if not ON_BOARD yet). Returns whether it came aboard.
pub fn load_direct(world: &mut World, id: u16, entity: u16) -> bool {
    let Some(e) = world.entities.by_index(entity as u32) else { return false };
    let (obj, kind, flags) = (e.obj_type, e.kind, e.flags);
    if !room(world, id, obj as u8, kind) || !matches!(obj, ObjType::Settler | ObjType::Ship | ObjType::LandVehicle) {
        return false;
    }
    if let Some(f) = data_mut(world, id) {
        f.passengers.push(Passenger { obj_type: obj as u8, kind, count: 1, id: entity, pos: 0, arrived: true, aboard: true });
        let w = weight(obj as u8, kind);
        f.aboard += w;
        f.reserved += w;
    }
    if obj == ObjType::Settler {
        crate::jobs::core::take_wait_list(world, entity);
    }
    if obj == ObjType::Settler || flags & flag::ON_BOARD == 0 {
        leave_map(world, entity);
    }
    world.unregister_from_logic_update(entity);
    if let Some(e) = world.entities.by_index_mut(entity as u32) {
        e.flags &= !0x2100;
    }
    true
}

/// `IEntity` vtbl+0x38 (0x5670e0): ON_BOARD, the map-object entry cleared.
fn leave_map(world: &mut World, entity: u16) {
    let Some(e) = world.entities.by_index_mut(entity as u32) else { return };
    if e.flags & flag::ON_BOARD != 0 {
        return;
    }
    e.flags |= flag::ON_BOARD;
    let pos = e.pos;
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
        world.layers.map_object[n] = 0;
    }
}

/// vtbl+0x10 (0x58d560): the ferry is deleted: a settler on its way gets event 7 `[ferry]` and is
/// detached, one aboard dies; a vehicle passenger dies either way. The list is emptied.
pub fn on_delete(world: &mut World, id: u16) {
    let mut k = 0;
    while let Some(p) = data(world, id).and_then(|f| f.passengers.get(k).copied()) {
        k += 1;
        if p.count <= 0 {
            continue;
        }
        if p.obj_type == ObjType::Settler as u8 {
            if p.aboard {
                world.kill(p.id, 0);
            } else if world.entities.by_index(p.id as u32).is_some() {
                crate::jobs::core::set_event(world, p.id, EntityEvent { event: 7, kind: 0, data: [id as i32, 0, 0] });
                super::detach_settler(world, id, p.id);
            }
        } else if p.id != 0 && world.entities.by_index(p.id as u32).is_some_and(|e| matches!(e.obj_type, ObjType::Ship | ObjType::LandVehicle)) {
            // (Deselected first if selected, 0x4eda10: the client's.)
            world.kill(p.id, 0);
        }
    }
    if let Some(f) = data_mut(world, id) {
        f.passengers.clear();
    }
}

/// 0x5973e0 `(x, y)`: the first map object on the spiral up to radius 2 around `(x, y)`, if it is
/// a ferry.
pub fn ferry_near(world: &World, x: i32, y: i32) -> u16 {
    for k in 0..=crate::spiral::last(2) {
        let Some(n) = world.grid.checked_index(x + crate::spiral::dx(k), y + crate::spiral::dy(k)) else { continue };
        let m = world.layers.map_object[n];
        if m != 0 {
            return if is_ferry(world, m) { m } else { 0 };
        }
    }
    0
}

/// 0x5800b0 `(settler, event)`, the settler roles' move order (0x11 / 0xd): onto a ferry of the
/// settler's player near the clicked node (data 2 > 0, `SendTo`'s click) the settler boards
/// (true). Otherwise a settler on its way to a ferry (its home) leaves it (vtbl+0x7c, vtbl+0x40);
/// false.
pub fn settler_move_order(world: &mut World, settler: u16, ev: &EntityEvent) -> bool {
    if ev.data[2] <= 0 {
        return false;
    }
    let to = Y16X16(ev.data[2] as u32);
    let ferry = ferry_near(world, to.x(), to.y());
    let owner = world.entities.by_index(settler as u32).map_or(0, |e| e.owner);
    if ferry != 0 && world.entities.by_index(ferry as u32).is_some_and(|f| f.owner == owner) && board(world, ferry, settler) == 0 {
        return true;
    }
    let home = crate::jobs::core::job(world, settler).map_or(0, |j| j.home);
    if home != 0 && is_ferry(world, home) {
        remove_passenger(world, home, settler);
        super::detach_settler(world, home, settler);
    }
    false
}

/// The counts the ferry's dialog D42 shows (vtbl+0x94, 0x58e450: message 0x25a, 0x25c with the
/// argument; packet 0x11ab078, all cleared first, code 0x17 at +4, `[k]` at +8 + 4·k), of the
/// passengers aboard by type: [0] 0x32, [1] 0x31,
/// [2] 0x30, [3] 0x2f, [4] 0x2e, [5] 0x1d..=0x1f, [6] 0x20..=0x22, [7] 0x23..=0x2b and
/// 0x3d..=0x3f, [8] 0x2c, [9] 0x2d, [10] type 4, [11] type 5 (the switch reads the type only).
pub fn dialog(world: &World, id: u16) -> [i32; 12] {
    let mut c = [0; 12];
    for p in data(world, id).map_or(&[][..], |f| &f.passengers[..]).iter().filter(|p| p.aboard) {
        let i = match p.kind {
            0x32 => 0,
            0x31 => 1,
            0x30 => 2,
            0x2f => 3,
            0x2e => 4,
            0x1d..=0x1f => 5,
            0x20..=0x22 => 6,
            0x23..=0x2b | 0x3d..=0x3f => 7,
            0x2c => 8,
            0x2d => 9,
            4 => 10,
            5 => 11,
            _ => continue,
        };
        c[i] += p.count;
    }
    c
}

/// The settler roles' event 0x18 `[vehicle]` (the gardener 0x579a80, geologist 0x57ab80 and
/// priest 0x57f880 cases; the pioneer's and thief's are theirs): with the vehicle's place for him
/// (vtbl+0x8c) and the vehicle alive, he walks there (start here, his anim list 0: GO_TO_TARGET,
/// CHECKIN), the vehicle attaches him, SELECTABLE / SELECTED go, he leaves his groups and the AI
/// hears (0x12 with his home, the vehicle); else SELECTABLE and the wait list. Returns whether he
/// goes.
pub fn settler_called(world: &mut World, i: u16, vehicle: u16) -> bool {
    let place = super::exit_place(world, vehicle, i);
    let alive = world.entities.by_index(vehicle as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0);
    if place == 0 || !alive {
        if let Some(e) = world.entities.by_index_mut(i as u32) {
            e.flags |= flag::SELECTABLE;
        }
        crate::jobs::core::take_wait_list(world, i);
        return false;
    }
    let here = pos_of(world, i);
    if let Some(j) = crate::jobs::core::job_mut(world, i) {
        j.destination = Y16X16(place as u32);
        j.start = here;
    }
    crate::jobs::core::take_anim_list(world, i, 0);
    // (Deselected if he is the local player's, 0x4eda10: the client's.)
    super::attach(world, vehicle, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags &= !0x2200;
    }
    if crate::group::word(world, i) != 0 {
        crate::group::remove_all(world, i);
    }
    let (owner, home) = (world.entities.by_index(i as u32).map_or(0, |e| e.owner), crate::jobs::core::job(world, i).map_or(0, |j| j.home));
    crate::ai::post_event(world, 0x12, owner as i32, i as i32, home as i32);
    true
}

/// The settler roles' event 0x19 `[0, at]`, its common part: on the map near `at` with his
/// default list (0x582a40), out of and into the war map (0x5bb440 / 0x5bb330), off board,
/// VISIBLE and SELECTABLE.
pub fn settler_ashore(world: &mut World, i: u16, at: Y16X16) {
    crate::create::put_settler_near(world, i, at.x(), at.y());
    crate::warmap::readd(world, i);
    if let Some(e) = world.entities.by_index_mut(i as u32) {
        e.flags = (e.flags & !flag::ON_BOARD) | 0x2100;
    }
}
