//! `CLookoutTowerRole` (house kind 9, release vtable 0x1058d7c): the lookout tower. Its
//! inhabitant (a carrier) watches for enemy soldiers. Decompiles in
//! `tools/re/decompiled/script/eyelook/`.
//!
//! * `Init` (0x547190): state 1, first update in 2 ticks, no alarm.
//! * `LogicUpdate` (0x547210), every 31 ticks:
//!   - state 1 orders the inhabitant (`OrderInhabitant` 0x53ff40). Once it is ordered, state 3
//!     (without a new update: the inhabitant's arrival brings it).
//!   - state 3, with the inhabitant home: an enemy warrior within 32 (`CWarMap` 0x5b0a20) raises
//!     the alarm once until none is left. The alarm is building trigger 16 (`TakeJobTrigger`'s
//!     table at 0x1298f10: the patch and its sound, client side) and, for the owner, the filtered
//!     message 0x992 at the tower.
//!   - Any other state becomes 3.
//! * `SettlerEnter` (0x5471e0): the inhabitant is home, state 3, next update in 31 ticks.
//!
//! The eye catchers (house kind 19, `CEyeCatcherRole` 0x1058b6c) need nothing beyond
//! `InitCommon`: their `Init` (0x545ff0) only adds the selection refresh and their `LogicUpdate`
//! (0x546020) is empty.

use super::{Role, RoleKind};
use crate::World;
use crate::hash::StateHasher;

/// `CLookoutTowerRole` +4 and +0x17c.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    pub state: u8,
    /// +0x17c: the alarm was raised for the enemies in sight.
    pub alarm: bool,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.state);
        h.bool(self.alarm);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::Lookout(s) } => Some((base, s)),
        _ => None,
    }
}

pub fn get(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::Lookout(s), .. } => Some(s),
        _ => None,
    }
}

/// The alarm trigger (`TakeJobTrigger` row 16).
const ALARM_TRIGGER: u8 = 16;
/// "Enemy spotted" (string 0x992).
const ALARM_TEXT: u16 = 0x992;
/// The watch radius.
const RADIUS: i32 = 0x20;

/// `CLookoutTowerRole::Init` (0x547190), after `InitCommon`.
pub fn init(world: &mut World, id: u16) {
    if let Some((_, s)) = role(world, id) {
        *s = State { state: 1, alarm: false };
    }
    world.register_for_logic_update(id, 2);
}

/// `CLookoutTowerRole::SettlerEnter` (0x5471e0).
pub fn settler_enter(world: &mut World, id: u16, _settler: u16) -> bool {
    if let Some((base, s)) = role(world, id) {
        base.worker_in_house = true;
        s.state = 3;
    }
    world.register_for_logic_update(id, 31);
    true
}

/// `CLookoutTowerRole::LogicUpdate` (0x547210). (A selected tower first refreshes its dialog,
/// vtbl+0x58, GUI only.)
pub fn logic_update(world: &mut World, id: u16) {
    let Some((base, s)) = role(world, id) else { return };
    let (state, home) = (s.state, base.worker_in_house);
    match state {
        1 => {
            if super::work::order_inhabitant(world, id) {
                if let Some((_, s)) = role(world, id) {
                    s.state = 3;
                }
                return;
            }
        }
        3 => {
            if home {
                let Some(e) = world.entities.by_index(id as u32) else { return };
                let (x, y, owner) = (e.x(), e.y(), e.owner);
                if !crate::warmap::enemy_warriors_near(world, x, y, RADIUS, owner) {
                    if let Some((_, s)) = role(world, id) {
                        s.alarm = false;
                    }
                } else if role(world, id).is_some_and(|(_, s)| !s.alarm) {
                    super::take_job_trigger(world, id, ALARM_TRIGGER);
                    world.events.push(crate::event::SimEvent::Message { player: owner, text: ALARM_TEXT, x, y, filtered: true });
                    if let Some((_, s)) = role(world, id) {
                        s.alarm = true;
                    }
                }
            }
        }
        _ => {
            if let Some((_, s)) = role(world, id) {
                s.state = 3;
            }
        }
    }
    world.register_for_logic_update(id, 31);
}
