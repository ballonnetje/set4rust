//! The Dark Tribe's manakopter hall positions (`CBuildingMgr` +0x46fdc / +0x472ac / +0x472d0,
//! release 0x1535a34 / 0x1535d04 / 0x1535d28; own reverse engineering of the shipped exe).
//!
//! * `AddBuilding` (0x53c350) for type 0x50 first adds the position to the player's list
//!   (`AddPositionToList` 0x53f810: at most 20, "Too many ManakopterHallPositions" otherwise) —
//!   whether or not the building then stands. If the player already had a hall (its per-type
//!   building list is not empty), the new one is destroyed at once (`DestroyBuilding(x, y,
//!   player)` 0x53ce10) and the hall tick kept as it was.
//! * `DestroyBuilding` sets the owner's hall tick to the game tick for every hall its search
//!   spiral meets (0x53cec7..0x53cef7), covering the node or not.
//! * The Dark Tribe AI (0x44b260, from 0x4490e0) orders a new hall (5025, type 0x50) when the
//!   player has positions and no hall and `(tick − hall tick) / 14` exceeds its record's +0x4c,
//!   at the first listed position where `CheckForBuildDarkTribe` passes ([`build_position`],
//!   0x53f750). Another agent (0x44a9f3) asks whether a node is within 24 of a listed position
//!   ([`near_position`], 0x53f860).
//! * Reset with the building manager (0x53bb10); saved in chunk 162 (0x53bba0 / 0x53bfa0, version
//!   2: per player the tick, the count and 20 positions).

use crate::World;
use crate::grid::Y16X16;
use crate::hash::StateHasher;

const PLAYERS: usize = 9;
/// The list's capacity per player.
pub const MAX: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HallList {
    /// Per player 0..=8, in the order added.
    pub positions: Vec<Vec<Y16X16>>,
    /// Per player 0..=8: the tick a hall was last destroyed.
    pub tick: Vec<u32>,
}

impl Default for HallList {
    fn default() -> Self {
        HallList { positions: vec![Vec::new(); PLAYERS], tick: vec![0; PLAYERS] }
    }
}

impl HallList {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        for (p, t) in self.positions.iter().zip(&self.tick) {
            h.u32(*t);
            h.u32(p.len() as u32);
            p.iter().for_each(|q| h.u32(q.0));
        }
    }
}

/// `AddPositionToList(pos, player)` (0x53f810): false when the player's list is full.
pub fn add_position(world: &mut World, player: u8, pos: Y16X16) -> bool {
    let Some(list) = world.sites.halls.positions.get_mut(player as usize) else { return false };
    if list.len() >= MAX {
        return false; // "Too many ManakopterHallPositions. Player %d"
    }
    list.push(pos);
    true
}

/// Whether `player` has a hall (its per-type building list for 0x50 is not empty; sites count).
pub fn has_hall(world: &World, player: u8) -> bool {
    world.entities.iter().any(|(_, e)| e.obj_type == crate::entity::ObjType::Building && e.owner == player && e.kind == 80 && !e.is_dead())
}

/// 0x53f750 `(player)`: the first listed position where a hall may be built
/// (`CheckForBuildDarkTribe` mode 0 passes), or None.
pub fn build_position(world: &World, player: u8) -> Option<Y16X16> {
    let list = world.sites.halls.positions.get(player as usize)?;
    list.iter()
        .copied()
        .find(|p| matches!(super::check::check_for_build(world, p.x(), p.y(), player, 80), super::CheckResult::Possible { .. }))
}

/// 0x53f860 `(pos, player)`: a listed position within hex distance 24 of `pos`.
pub fn near_position(world: &World, player: u8, pos: Y16X16) -> bool {
    let Some(list) = world.sites.halls.positions.get(player as usize) else { return false };
    list.iter().any(|q| {
        let (dx, dy) = (q.x() - pos.x(), q.y() - pos.y());
        let d = if (dx ^ dy) < 0 { dx.abs() + dy.abs() } else { dx.abs().max(dy.abs()) };
        d < 25
    })
}

/// The hall part of `AddBuilding` before the building is made: the position joins the list.
/// Returns whether the player already had a hall (the new one is then destroyed by
/// [`destroy_second`] once it exists).
pub(crate) fn before_add(world: &mut World, player: u8, x: i32, y: i32) -> bool {
    let had = has_hall(world, player);
    add_position(world, player, Y16X16::pack(x, y));
    had
}

/// The second hall goes: `DestroyBuilding(x, y, player)`, the hall tick kept (0x53c4f0..).
pub(crate) fn destroy_second(world: &mut World, player: u8, x: i32, y: i32) {
    let keep = world.sites.halls.tick.get(player as usize).copied().unwrap_or(0);
    crate::buildings::crush::destroy_building_at(world, x, y, player as i32);
    if let Some(t) = world.sites.halls.tick.get_mut(player as usize) {
        *t = keep;
    }
}

/// `DestroyBuilding`'s spiral meeting a hall of `owner` (0x53cec7): its hall tick becomes the
/// game tick.
pub(crate) fn met_in_destroy(world: &mut World, owner: u8) {
    let tick = world.tick;
    if let Some(t) = world.sites.halls.tick.get_mut(owner as usize) {
        *t = tick;
    }
}
