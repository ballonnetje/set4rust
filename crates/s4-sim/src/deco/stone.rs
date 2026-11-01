//! `CStone` (MapObjects/Stone.cpp, object types 41 `OBJECT_MINESET1` and 253
//! `OBJECT_DARKMINESET1`): a heap of 1..13 stones, a shared static instance per amount with
//! resource `0x60 | amount` on its node. A stonecutter's search gives it a dynamic instance;
//! every `Take` removes one stone, turning the heap back into the static instance of the smaller
//! amount, until the last one deletes it. Hacking (`Decrease`) changes nothing.

use super::{change_to_static_instance, delete};
use crate::entity::{DecoData, Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::world::World;

/// `CStone::GetGoodType`: `GOOD_STONE`.
pub const GOOD: u8 = 32;
/// Resource type of a stone heap (`CWorldManager::SetResource(x, y, 96, amount)`).
pub const RESOURCE_STONE: u8 = 0x60;

pub fn is_stone(kind: u16) -> bool {
    kind == 41 || kind == 253
}

/// `CStone::CStone(const CStone& static, id, x, y)` (release 0x593420) via `CSearchRoutines::SearchStone`
/// (release 0x54e310): a dynamic copy of the static heap `static_id` at (x, y), same amount.
pub fn make_dynamic(world: &mut World, static_id: u16, x: i32, y: i32) -> Option<u16> {
    let e = world.entities.by_index(static_id as u32)?;
    let EntityData::Deco(d) = &e.data else { return None };
    let (kind, amount) = (e.kind, d.amount);
    let node = world.grid.checked_index(x, y)?;
    let slot = world.entities.free_slot()?;
    let data = DecoData { static_instance: false, frame: 0, stage: 0, cycle_frames: 0, amount, counter: 0, ..DecoData::default() };
    let mut s = Entity::new(ObjType::Deco, kind, 0, 0, Y16X16::pack(x, y), EntityData::Deco(data));
    s.flags |= flag::VISIBLE;
    world.entities.insert(slot, s);
    world.layers.deco_object[node] = slot;
    Some(slot)
}

/// `CStone::Take(1)` (release vtbl+0x24, 0x593690): one stone off. The heap becomes the static instance of the
/// remaining amount (`ChangeToStaticInstance`), or is deleted when it was the last stone.
pub fn take(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let EntityData::Deco(d) = &e.data else { return };
    // "!IsStaticInstance()" and "m_uAmount == resource amount" asserted.
    let (x, y, kind, amount) = (e.x(), e.y(), e.kind, d.amount.saturating_sub(1));
    if amount != 0 {
        if let Some(n) = world.grid.checked_index(x, y) {
            let t = world.layers.resource_type(n);
            world.layers.set_resource(n, t, amount);
        }
        change_to_static_instance(world, x, y, kind, amount);
    } else {
        delete(world, id);
    }
}
