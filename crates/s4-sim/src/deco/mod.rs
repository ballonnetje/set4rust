//! Landscape objects: `CDecoObjMgr` (MapObjects/DecoObj/DecoObjMgr.cpp) and the object classes
//! with behaviour (`CTree`, `CStone`, …). The deco-object layer (`CWorldManager::ObjectId`) holds
//! the entity of each node's object; grown trees, plants and stone heaps share static instances
//! (`IDecoObject +68`) until a worker turns one into a dynamic instance of its own.

use crate::entity::{DecoData, Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::rules::OBJECT_MAX;
use crate::world::World;

pub mod hive;
pub mod mushroom;
pub mod plant;
pub mod shadow_herb;
pub mod stone;
pub mod tree;

/// Ids per type in `CDecoObjMgr`'s instance table. The table starts at `this + 32` and holds 291
/// records of 132 bytes (`CDecoObjMgr::Clear` = `memset(this + 32, 0, 0x960C)`, 0x960C = 291 * 132):
/// limit, cursor, count, pad, then 32 `u32` ids. The largest pool any object asks for is 32
/// (frame counts from `Gfx.gfx`, e.g. object 254 DARKSPITTER).
pub const POOL_IDS: usize = 32;

/// One `m_vInstanceId[type]` record of `CDecoObjMgr` (at `this + 32 + 132 * type`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecoPool {
    /// +0: number of shared instances to create before reusing.
    pub limit: u8,
    /// +1: last reused instance.
    pub cursor: u8,
    /// +2: instances created.
    pub count: u8,
    /// +4: instance entity ids.
    #[cfg_attr(feature = "serde", serde(with = "serde_big_array::BigArray"))]
    pub ids: [u16; POOL_IDS],
}

/// `CDecoObjMgr`'s shared static instances, per object type.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DecoPools {
    pub pools: Vec<DecoPool>,
}

impl Default for DecoPools {
    fn default() -> Self {
        DecoPools { pools: vec![DecoPool::default(); OBJECT_MAX] }
    }
}

impl DecoPools {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for p in &self.pools {
            h.u8(p.limit);
            h.u8(p.cursor);
            h.u8(p.count);
            for &id in &p.ids {
                h.u16(id);
            }
        }
    }
}

/// Why a deco object was not created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DecoSkip {
    /// Types 41 (`OBJECT_MINESET1`) / 253 placed with amount 0: `Create` would index its pool at -1
    /// and write a garbage id into the node. Not reproduced.
    StoneWithoutAmount,
    /// Pool index past the 32-entry table (would overwrite the next type's record; does not occur
    /// with the shipped data).
    PoolOverflow,
    /// No free entity slot.
    NoSlot,
}

fn new_deco(world: &World, kind: u16, x: i32, y: i32, data: DecoData) -> Entity {
    // `IDecoObject` ctor: static instances sit at (0, 0); visible flag.
    let pos = if data.static_instance { Y16X16(0) } else { Y16X16::pack(x, y) };
    let _ = world;
    let mut e = Entity::new(ObjType::Deco, kind, 0, 0, pos, EntityData::Deco(data));
    e.flags |= flag::VISIBLE;
    e
}

/// `CDecoObjMgr::AddDecoObjWithoutFlags(x, y, type, amount, grown)` (release 0x563940) →
/// `CDecoObjMgr::Create` (inlined there, from 0x563d27): picks or creates the entity for a deco object and writes it
/// into the node's deco slot. Only `grown == true` (the map loader's case) is implemented.
///
/// Per type group (`a4`):
/// * 1..=22 trees, 255 / 162 / 209..=211 plants, 212..=214 mushrooms: shared instances, pool size
///   = frames of job `first + 3`; each new instance draws `Rand() % cycle` for its start frame;
///   trees also set resource `0x70 | 1`. Reuse advances the cursor (wrapping to 0).
/// * 124..=136 stone heaps: instance `CStone(41, type - 123)` kept in type 41's pool at
///   index `amount - 1`; resource `0x60 | amount`.
/// * 216 shadow herb, 217 hive: own (non-static) entity, registered for logic updates.
/// * everything else: `CDecoObject` shared instances, pool size = frames of the first job, start
///   frame = creation index; reuse skips index 0 after the first wrap (copied as is).
pub fn add_deco_obj_without_flags(world: &mut World, x: i32, y: i32, kind: u16, amount: u8) -> Result<u16, DecoSkip> {
    let node = world.grid.checked_index(x, y).expect("deco position in world");
    let slot = world.entities.free_slot();
    let t = kind as usize;
    let frames = |w: &World, k| w.rules.object_frames(kind, k);
    let id = match kind {
        1..=22 | 162 | 209..=211 | 255 | 212..=214 => {
            if world.deco_pools.pools[t].count == 0 {
                world.deco_pools.pools[t].limit = frames(world, 3);
            }
            let p = &mut world.deco_pools.pools[t];
            let id = if p.count >= p.limit {
                p.cursor = p.cursor.wrapping_add(1);
                if p.cursor >= p.limit {
                    p.cursor = 0;
                }
                let id = *p.ids.get(p.cursor as usize).ok_or(DecoSkip::PoolOverflow)?;
                world.layers.deco_object[node] = id;
                id
            } else {
                if p.count as usize >= POOL_IDS {
                    return Err(DecoSkip::PoolOverflow);
                }
                let slot = slot.ok_or(DecoSkip::NoSlot)?;
                // CTree / CPlant / CMushroom ctor with stage 3: job = first + 3, static → random frame.
                let cycle = frames(world, 3);
                let frame = if cycle == 0 { 0 } else { (world.rng.rand() % cycle as u32) as u8 };
                // CPlant keeps its good in +0x47 (here `amount`) and a static plant is READY
                // (release ctor 0x566030; goods from `CDecoObjMgr::Create` 0x563d27).
                let good = plant::good_of(kind);
                let data = DecoData { static_instance: true, frame, stage: 3, cycle_frames: cycle, amount: good, counter: 0, ..DecoData::default() };
                let mut e = new_deco(world, kind, x, y, data);
                // A grown mushroom is READY too (ctor 0x565a20 with stage 3).
                if plant::is_plant(kind) || mushroom::is_mushroom(kind) {
                    e.flags |= flag::READY;
                }
                world.entities.insert(slot, e);
                world.layers.deco_object[node] = slot;
                let p = &mut world.deco_pools.pools[t];
                p.ids[p.count as usize] = slot;
                p.count += 1;
                slot
            };
            if kind <= 22 {
                world.layers.set_resource(node, 0x70, 1);
            }
            id
        }
        41 | 253 if amount == 0 => return Err(DecoSkip::StoneWithoutAmount),
        41 | 253 | 124..=136 => {
            let (pool_type, amount) = if kind >= 124 { (41, (kind - 123) as u8) } else { (t, amount) };
            let idx = amount as usize - 1;
            if idx >= POOL_IDS {
                return Err(DecoSkip::PoolOverflow);
            }
            if world.deco_pools.pools[pool_type].count == 0 {
                world.deco_pools.pools[pool_type].limit = if kind >= 124 { 12 } else { 15 };
            }
            let existing = world.deco_pools.pools[pool_type].ids[idx];
            let id = if existing != 0 {
                world.layers.deco_object[node] = existing;
                existing
            } else {
                let slot = slot.ok_or(DecoSkip::NoSlot)?;
                let data = DecoData { static_instance: true, frame: 0, stage: 0, cycle_frames: 0, amount, counter: 0, ..DecoData::default() };
                let e = new_deco(world, pool_type as u16, x, y, data);
                world.entities.insert(slot, e);
                world.layers.deco_object[node] = slot;
                let p = &mut world.deco_pools.pools[pool_type];
                p.ids[idx] = slot;
                p.count += 1;
                slot
            };
            if kind != 253 {
                world.layers.set_resource(node, 0x60, amount);
            }
            id
        }
        217 => hive::create(world, x, y).ok_or(DecoSkip::NoSlot)?,
        216 => {
            let slot = slot.ok_or(DecoSkip::NoSlot)?;
            // CShadowHerb: job = first, registered with delta = its cycle length.
            let (cycle, delta) = (frames(world, 0), frames(world, 0) as i32);
            let data = DecoData { static_instance: false, frame: 0, stage: 0, cycle_frames: cycle, amount: 0, counter: 0, ..DecoData::default() };
            let e = new_deco(world, kind, x, y, data);
            world.entities.insert(slot, e);
            world.layers.deco_object[node] = slot;
            world.register_for_logic_update(slot, delta);
            slot
        }
        _ => {
            if world.deco_pools.pools[t].count == 0 {
                world.deco_pools.pools[t].limit = frames(world, 0);
            }
            let p = &mut world.deco_pools.pools[t];
            if p.count >= p.limit {
                if p.cursor as i8 >= (p.limit as i8).wrapping_sub(1) {
                    p.cursor = 0;
                }
                if p.limit as i8 > 1 {
                    p.cursor += 1;
                }
                let id = *p.ids.get(p.cursor as usize).ok_or(DecoSkip::PoolOverflow)?;
                world.layers.deco_object[node] = id;
                id
            } else {
                if p.count as usize >= POOL_IDS {
                    return Err(DecoSkip::PoolOverflow);
                }
                let slot = slot.ok_or(DecoSkip::NoSlot)?;
                let index = p.count;
                let cycle = frames(world, 0);
                // CDecoObject ctor 0x562460: frame = creation index % cycle. Its last argument, the
                // object record's byte 7 (0x153f383 + 8 * type: `pingPong != 0`), sets +0x45 =
                // (byte 7 == 0), the ping-pong mode, which only the draw call 0x562700 reads (it
                // also advances the frame there, by game ticks since the last draw): not sim
                // state (s4-viewer `sprites::DecoAnim`).
                let data = DecoData { static_instance: true, frame: index % cycle.max(1), stage: 0, cycle_frames: cycle, amount: 0, counter: 0, ..DecoData::default() };
                let e = new_deco(world, kind, x, y, data);
                world.entities.insert(slot, e);
                world.layers.deco_object[node] = slot;
                let p = &mut world.deco_pools.pools[t];
                p.ids[p.count as usize] = slot;
                p.count += 1;
                slot
            }
        }
    };
    Ok(id)
}


/// `IAnimatedEntity::LogicUpdate` of a deco object, by class.
pub fn logic_update(world: &mut World, id: u16) {
    let Some(kind) = world.entities.by_index(id as u32).map(|e| e.kind) else { return };
    match kind {
        k if tree::is_tree(k) => tree::logic_update(world, id),
        k if plant::is_plant(k) => plant::logic_update(world, id),
        k if hive::is_hive(k) => hive::logic_update(world, id),
        k if shadow_herb::is_shadow_herb(k) => shadow_herb::logic_update(world, id),
        k if mushroom::is_mushroom(k) => mushroom::logic_update(world, id),
        _ => {}
    }
}

/// `CSpiralOffsets::First(radius)` (release accessor 0x4330a0, `{first, last}` table 0x117cf38),
/// which Set/ClearFlagsForObject (0x563510, 0x563670) and IsAddDecoObjOk (0x5631b0) read inline.
/// Not `Last(radius − 1) + 1`: the table's rings are not contiguous for radius >= 8.
fn spiral_first(radius: u8) -> usize {
    crate::spiral::first(radius)
}

/// `CDecoObjMgr::ClearFlagsForObject(x, y, type, false)` (release 0x563670): undo `SetFlagsForObject`.
/// A repelling object clears 0x02 / 0x40 on its node; a blocking one clears blocked land and
/// 0x40 on its `rings`; the rest of its `total` rings lose 0x40.
pub fn clear_flags_for_object(world: &mut World, x: i32, y: i32, kind: u16) {
    let f = world.rules.build.deco_flags.get(kind as usize).copied().unwrap_or_default();
    if f.repelling {
        if let Some(n) = world.grid.checked_index(x, y) {
            world.layers.flags[n] &= !(2 | 0x40);
        }
    } else {
        for i in 0..spiral_first(f.rings) {
            let (px, py) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
            world.clear_blocked_land(px, py);
            if let Some(n) = world.grid.checked_index(px, py) {
                world.layers.flags[n] &= !0x40;
            }
        }
    }
    for i in spiral_first(f.rings)..spiral_first(f.total) {
        if let Some(n) = world.grid.checked_index(x + crate::spiral::dx(i), y + crate::spiral::dy(i)) {
            world.layers.flags[n] &= !0x40;
        }
    }
}

/// `CMapObjectMgr::Destroy` of a dynamic deco object: the `IDecoObject` destructor empties its
/// node's deco slot.
fn destroy(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, kind) = (e.x(), e.y(), e.kind);
    if let Some(n) = world.grid.checked_index(x, y) {
        // `CStone::~CStone`: a dynamic heap clears its node's resource.
        if stone::is_stone(kind) {
            world.layers.set_resource(n, 0, 0);
        }
        if world.layers.deco_object[n] == id {
            world.layers.deco_object[n] = 0;
        }
    }
    world.unregister_from_logic_update(id);
    world.entities.destroy(id);
}

/// `CDecoObjMgr::DeleteWithoutFlags(id)` (release 0x563450): a deco object (entity type 0x20) is
/// destroyed; anything else is left alone.
pub fn delete_without_flags(world: &mut World, id: u16) {
    if id != 0 && world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == crate::entity::ObjType::Deco) {
        destroy(world, id);
    }
}

/// `CDecoObjMgr::Delete(id)` (release 0x563410): clear the object's flags, then destroy it.
pub fn delete(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (x, y, kind) = (e.x(), e.y(), e.kind);
    clear_flags_for_object(world, x, y, kind);
    destroy(world, id);
}

/// `CDecoObjMgr` 0x564cc0 `(x, y, keep)`: the deco object on the node goes — a shared instance
/// leaves the node (its flags cleared), a dynamic one is deleted (unless `keep`) — and the
/// node's resource is cleared. Returns 0 for an empty node, −1 for a non-deco entity there.
pub fn remove_at(world: &mut World, x: i32, y: i32, keep: bool) -> i32 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let id = world.layers.deco_object[n];
    if id == 0 {
        return 0;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return -1 };
    let EntityData::Deco(d) = &e.data else { return -1 };
    if d.static_instance {
        let kind = e.kind;
        world.layers.deco_object[n] = 0;
        clear_flags_for_object(world, x, y, kind);
    } else if !keep {
        delete(world, id);
    }
    world.layers.set_resource(n, 0, 0);
    1
}

/// `CDecoObjMgr::ChangeToStaticInstance(x, y, type, amount)` (release 0x564390): the node's dynamic
/// object goes back to a shared static instance (`AddDecoObjWithoutFlags(…, grown)`).
pub fn change_to_static_instance(world: &mut World, x: i32, y: i32, kind: u16, amount: u8) {
    let Some(n) = world.grid.checked_index(x, y) else { return };
    let old = world.layers.deco_object[n];
    if old != 0 {
        delete_without_flags(world, old);
    }
    let _ = add_deco_obj_without_flags(world, x, y, kind, amount);
}

fn unported(world: &mut World, what: &'static str, id: u16) {
    world.events.push(crate::event::SimEvent::Unported { what, entity: id });
}

/// `IDecoObject::GetGoodType` (vtbl+60): what a worker gets from the object.
pub fn good_type(world: &World, id: u16) -> u8 {
    match world.entities.by_index(id as u32).map(|e| e.kind) {
        Some(k) if tree::is_tree(k) => tree::GOOD,
        Some(k) if stone::is_stone(k) => stone::GOOD,
        // CPlant::GetGoodType (release 0x566010): +0x47.
        Some(k) if plant::is_plant(k) => match world.entities.by_index(id as u32).map(|e| &e.data) {
            Some(EntityData::Deco(d)) => d.amount,
            _ => 0,
        },
        // CDecoObject, CMushroom, CHive, CShadowHerb: 0x52c4f0 (`xor eax, eax`).
        _ => 0,
    }
}

/// `IDecoObject::Decrease(1)` (vtbl+32): one step of work on the object.
pub fn decrease(world: &mut World, id: u16) {
    match world.entities.by_index(id as u32).map(|e| e.kind) {
        Some(k) if tree::is_tree(k) => tree::decrease(world, id),
        Some(k) if stone::is_stone(k) => {} // CStone::Decrease is empty
        Some(k) if plant::is_plant(k) => {} // CPlant::Decrease (release 0x566690) is empty
        Some(k) if hive::is_hive(k) => hive::decrease(world, id),
        _ => unported(world, "IDecoObject::Decrease", id),
    }
}

/// `IDecoObject::Take(1)` (vtbl+36): the worker takes the good (a tree: `Decrease(1)`).
pub fn take(world: &mut World, id: u16) {
    match world.entities.by_index(id as u32).map(|e| e.kind) {
        Some(k) if tree::is_tree(k) => tree::decrease(world, id),
        Some(k) if stone::is_stone(k) => stone::take(world, id),
        Some(k) if plant::is_plant(k) => plant::take(world, id),
        _ => unported(world, "IDecoObject::Take", id),
    }
}

/// `CFreeWorkerRole::SetFree` for a target the worker had not started on: a dynamic instance
/// goes back to the static one (`ChangeToStaticInstance(x, y, type, vtbl+40())`).
pub fn release(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let EntityData::Deco(d) = &e.data else { return };
    if d.static_instance {
        return;
    }
    let (x, y, kind, amount) = (e.x(), e.y(), e.kind, d.amount);
    change_to_static_instance(world, x, y, kind, amount);
}

/// `CDecoObjMgr::AddDecoObjWithoutFlags(x, y, type, 0, 0)`: a planted object (not grown).
/// Trees, plants and mushrooms get a dynamic instance at stage 0; a shadow herb or hive its own.
pub fn plant(world: &mut World, x: i32, y: i32, kind: u16) {
    if tree::is_tree(kind) {
        tree::plant(world, x, y, kind);
    } else if plant::is_plant(kind) {
        plant::plant(world, x, y, kind);
    } else if hive::is_hive(kind) {
        hive::create(world, x, y);
    } else if mushroom::is_mushroom(kind) {
        mushroom::plant(world, x, y, kind);
    } else {
        unported(world, "CDecoObjMgr::AddDecoObjWithoutFlags (planted)", kind);
    }
}

/// `CDecoObjMgr::SetFlagsForObject(x, y, type, false)` (release 0x563510): a repelling object sets 0x02 | 0x40 on
/// its node, a blocking one blocked land and 0x40 on its `rings`; the rest of its `total` rings
/// get 0x40 (FLAG_DECO_RING).
pub fn set_flags_for_object(world: &mut World, x: i32, y: i32, kind: u16) {
    let f = world.rules.build.deco_flags.get(kind as usize).copied().unwrap_or_default();
    if f.repelling {
        if let Some(n) = world.grid.checked_index(x, y) {
            world.layers.flags[n] |= 2 | 0x40;
        }
    } else {
        for i in 0..spiral_first(f.rings) {
            let (px, py) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
            world.set_blocked_land(px, py);
            if let Some(n) = world.grid.checked_index(px, py) {
                world.layers.flags[n] |= 0x40;
            }
        }
    }
    for i in spiral_first(f.rings)..spiral_first(f.total) {
        if let Some(n) = world.grid.checked_index(x + crate::spiral::dx(i), y + crate::spiral::dy(i)) {
            world.layers.flags[n] |= 0x40;
        }
    }
}

/// `CDecoObjMgr::IsAddDecoObjOk(x, y, type, _, extra)` (release 0x5631b0): the node is in the
/// world, has none of flags 0x7B and no deco object; a blocking object's rings hold no settler
/// and none of 0x7B; the further rings up to `total + extra` none of 0x7B. (The debug build's
/// "allow an object on the node" flag is not used by the release.)
pub fn is_add_deco_obj_ok(world: &World, x: i32, y: i32, kind: u16, extra: u8) -> bool {
    const BUSY: u8 = 0x7B;
    let Some(n) = world.grid.checked_index(x, y) else { return false };
    if world.layers.flags[n] & BUSY != 0 || world.layers.deco_object[n] != 0 {
        return false;
    }
    let f = world.rules.build.deco_flags.get(kind as usize).copied().unwrap_or_default();
    if !f.repelling {
        for i in 0..spiral_first(f.rings) {
            let Some(m) = world.grid.checked_index(x + crate::spiral::dx(i), y + crate::spiral::dy(i)) else { return false };
            if world.layers.map_object[m] != 0 || world.layers.flags[m] & BUSY != 0 {
                return false;
            }
        }
    }
    for i in spiral_first(f.rings)..spiral_first(f.total.saturating_add(extra)) {
        let Some(m) = world.grid.checked_index(x + crate::spiral::dx(i), y + crate::spiral::dy(i)) else { return false };
        if world.layers.flags[m] & BUSY != 0 {
            return false;
        }
    }
    true
}

/// `CDecoObjMgr::GetObjectType(kind, race, ground)` (release 0x5637d0): the object a planter of
/// `kind` (`SettlerInfo` `other`, `CDecoObjMgr::*`) puts on `ground`, drawn from the game RNG.
pub fn get_object_type(world: &mut World, kind: u8, race: u8, ground: u8) -> u16 {
    use crate::rng::Random16;
    let mut rand = || world.rng.rand();
    match kind {
        0 => 162,
        1 => 211,
        // TREE
        2 => match race {
            2 => {
                if ground & 0xF0 == 0x40 { (rand() & 1) as u16 + 11 } else { (rand() & 1) as u16 + 13 }
            }
            4 => {
                if ground & 0xF0 == 0x40 {
                    (rand() & 1) as u16 + 11
                } else {
                    let r = rand() as u8;
                    (if r & 6 != 0 { 15 } else { 17 }) + (r & 1) as u16
                }
            }
            _ => (rand() & 1) as u16 + 9,
        },
        3 => {
            if rand() >= Random16::percent_value(50) { 210 } else { 209 }
        }
        4 => {
            let r = rand();
            if r < Random16::percent_value(30) {
                212
            } else if r >= Random16::percent_value(70) {
                214
            } else {
                213
            }
        }
        5 => 216,
        6 => 217,
        7 => 255,
        _ => 0,
    }
}
