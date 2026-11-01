//! `CResidenceBuildingRole` (house kind 14, release vtable 0x1059010): the small, medium and big
//! residences. A finished residence adds its beds (buildingInfo `dummyValue`, info +0x1f0) to the
//! player's beds and brings new settlers of its inhabitant type (info +0x1de, the carrier) into
//! the world near its door, one per work cycle, until it has filled its beds (+0x17c) or the
//! player has reached the settler limit. Evidence: `tools/re/decompiled/script/residence/`.
//!
//! States: 2 production delay (updates of 14 ticks), 3 bring one settler.

use super::work::{self, WorkBase};
use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, ObjType};
use crate::hash::StateHasher;

/// `CResidenceBuildingRole` (+0x17c).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub work: WorkBase,
    /// +0x17c: settlers this residence has brought (its beds taken).
    pub residents: u8,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.work.hash(h);
        h.u8(self.residents);
    }
}

fn role(world: &mut World, id: u16) -> Option<&mut State> {
    match world.buildings.roles.get_mut(&id)? {
        Role { kind: RoleKind::Residence(s), .. } => Some(s),
        _ => None,
    }
}

/// The settlers residence `id` has brought (+0x17c); its dialog (0x54df70) shows
/// `beds − residents` free beds (at least 0).
pub fn residents(world: &World, id: u16) -> Option<u8> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Residence(s), .. } => Some(s.residents),
        _ => None,
    }
}

/// The residence's beds (info +0x1f0) and inhabitant type (+0x1de).
fn beds_and_type(world: &World, id: u16) -> (i32, u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return (0, 0) };
    let EntityData::Building(d) = &e.data else { return (0, 0) };
    let beds = world.rules.building(e.race, d.type_ex).beds;
    let kind = world.rules.work.info(e.race, d.type_ex).map_or(0, |i| i.inhabitant);
    (beds, kind)
}

/// `CResidenceBuildingRole::Init` (0x54db60), after `IBuildingRole::InitCommon`: state 3, no
/// residents. If the player has strikers (the carriers without a bed, `CEcoSectorMgr`'s list
/// at 0x147c8cc), they take the new beds first: fewer strikers than beds become its residents;
/// as many or more fill it (+0x17c = beds + 1, as the release writes). Then
/// `UpdateStrikeSettlers` (0x4df2f0) and, with beds left, the first update in 31 ticks.
/// A full residence sends the AI notice (8, owner, residence, 0). The beds count into the
/// player's bed total in `CStatistic` (+0xa00, 0x54db94; the end screen reads it), [`crush`]
/// takes them off.
pub fn init(world: &mut World, id: u16) {
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    let (beds, _) = beds_and_type(world, id);
    crate::statistic::beds(world, owner, beds);
    let strikers = world.economy.strikers.get(owner as usize).map_or(0, |s| s.len()) as i32;
    let residents = if strikers == 0 {
        0
    } else if strikers < beds {
        strikers as u8
    } else {
        (beds + 1) as u8
    };
    if let Some(s) = role(world, id) {
        *s = State { work: WorkBase { state: 3, ..WorkBase::default() }, residents };
    }
    if residents as i32 > beds {
        crate::ai::post_event(world, 8, owner as i32, id as i32, 0);
    }
    crate::eco::update_strike_settlers(world);
    if (residents as i32) < beds {
        world.register_for_logic_update(id, 31);
    }
}

/// `CResidenceBuildingRole` vtbl+0x2c (0x54df40), the residence crushed or destroyed: its beds
/// leave the owner's bed total (+0xa00).
pub fn crush(world: &mut World, id: u16) {
    let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
    let (beds, _) = beds_and_type(world, id);
    crate::statistic::beds(world, owner, -beds);
}

/// The player's settlers (`CSettlerMgr`'s counts of types 1..=66, summed).
fn settlers_of(world: &World, player: u8) -> i32 {
    world.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == player && (1..=66).contains(&e.kind)).count() as i32
}

/// `CResidenceBuildingRole::LogicUpdate` (0x54dca0; with building flag 0x200 it first refreshes the
/// dialog, GUI only).
/// * State 2: the production delay (info +0x1e0) in updates of 14 ticks; then, with beds left,
///   state 3 in 1 tick (else it stays asleep).
/// * State 3, with beds left: while the player has fewer settlers than the limit (2,500 with up
///   to 4 players, else 10,000 / players), a settler of the inhabitant type is added
///   (`AddSettler` 0x582600) on the first node of the spiral of radius 15 around the door
///   (0x5857c0: node flags 0x21 clear, no map object, the door's eco-sector). A full residence
///   sends the AI notice (8) and stops here. Otherwise the delay starts again (state 2, 14 ticks), or without a delay the
///   next try is in 31 ticks.
pub fn logic_update(world: &mut World, id: u16) {
    let delay = work::production_delay(world, id);
    let (beds, kind) = beds_and_type(world, id);
    let Some(s) = role(world, id) else { return };
    match s.work.state {
        2 => {
            if s.work.delay <= delay {
                s.work.delay += 1;
                world.register_for_logic_update(id, 14);
            } else if (s.residents as i32) < beds {
                s.work.state = 3;
                s.work.delay = 0;
                world.register_for_logic_update(id, 1);
            }
        }
        3 if (s.residents as i32) < beds => {
            let Some(e) = world.entities.by_index(id as u32) else { return };
            let owner = e.owner;
            let EntityData::Building(d) = &e.data else { return };
            let door = d.door;
            let players = world.number_of_players();
            let limit = if players < 5 { 2500 } else { 10_000 / players };
            if settlers_of(world, owner) < limit
                && let Some((x, y)) = free_node_near(world, door.x(), door.y())
                && crate::create::add_settler(world, x, y, owner, kind).is_some()
            {
                // 0x54ddc6: one more carrier made (statistic +0x114 and the total +0x21c).
                crate::statistic::settler_produced(world, owner, 1);
                let Some(s) = role(world, id) else { return };
                s.residents += 1;
                if s.residents as i32 >= beds {
                    crate::ai::post_event(world, 8, owner as i32, id as i32, 0);
                    return;
                }
            }
            if delay == 0 {
                world.register_for_logic_update(id, 31);
            } else if let Some(s) = role(world, id) {
                s.work.delay = 0;
                s.work.state = 2;
                world.register_for_logic_update(id, 14);
            }
        }
        _ => {}
    }
}

/// 0x5857c0: the first node of `CSpiralWalk(x, y, 15)` with node flags 0x21 clear, no map
/// object and the eco-sector of (x, y).
fn free_node_near(world: &World, x: i32, y: i32) -> Option<(i32, i32)> {
    let eco = crate::eco::eco_of(world, world.grid.checked_index(x, y)?);
    if eco == 0 {
        return None;
    }
    crate::spiral::walk(world.grid, x, y, 15).find(|&(px, py)| {
        let n = world.grid.index(px as u32, py as u32);
        world.layers.flags[n] & 0x21 == 0 && world.layers.map_object[n] == 0 && crate::eco::eco_of(world, n) == eco
    })
}
