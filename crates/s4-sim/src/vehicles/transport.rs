//! `CTransportShip` (vtable 0x105c81c, 0x590b90..0x592000): the ports' trade ships. Decompiles
//! `tools/re/vehicles/transship.c`, `transship2.c`; the ports' side is in
//! `buildings/trading.rs` (0x554da0 order, 0x555800 arrival, 0x5551b0 load, 0x5554d0 leave,
//! 0x5555e0 unload).
//!
//! * **Members** (ctor 0x590ba0, load 0x590d80 versions 1..=4): +0x98 the target port, +0x9c the
//!   source port, +0xa0 home (where it stood when ordered; the source's ensign once it leaves
//!   loaded), +0xa4 / +0xa5 sailing to the source / target, +0xa8 the state (2 to the source, 4
//!   loading, 1 to the target, 3 unloading, 0 none), +0xac the task of the last update, +0xad
//!   foreign, +0x110 the foreign leg (1 there, 2 back), +0x114 going home, eight slots +0xb0
//!   `{good, amount}` of 8 (+8 never written).
//! * **Update** (vtbl+0x90, 0x591c30): nothing without hit points; the ships' repair; walking
//!   (6) or drifting (0x11, not while loading / unloading: then it is not scheduled again until
//!   an order comes); a walk that ended (0x591950) reports to the port it sailed to (0x555800);
//!   a failed step (0x40) counts as the target gone (0x591740).
//! * **Target gone** (0x591740, also event 7 / 9 from the target): the target forgets it; with
//!   a living source it turns back there (recorded there) with its goods, else it drops them and
//!   goes home. **Source gone** (0x591830, event 7 / 9 from the source): on the way there it is
//!   forgotten and goes home.

use super::{get, get_mut, logic};
use crate::World;
use crate::entity::{ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// Goods per slot.
pub const SLOT_SIZE: i32 = 8;
/// Goods types the cargo loops run over (0..0x2b).
const GOODS: i32 = 0x2b;

/// The `CTransportShip` members (see the module doc).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransportData {
    pub target: u16,
    pub source: u16,
    pub home: i32,
    pub to_source: bool,
    pub to_target: bool,
    pub state: i32,
    pub last_task: u8,
    pub foreign: bool,
    pub leg: i32,
    pub going_home: bool,
    /// `{good, amount}`.
    pub slots: [[i32; 2]; 8],
}

impl TransportData {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u16(self.target);
        h.u16(self.source);
        for v in [self.home, self.state, self.leg] {
            h.u32(v as u32);
        }
        h.u8(self.to_source as u8 | (self.to_target as u8) << 1 | (self.foreign as u8) << 2 | (self.going_home as u8) << 3);
        h.u8(self.last_task);
        for s in &self.slots {
            h.u32(s[0] as u32);
            h.u32(s[1] as u32);
        }
    }

    /// 0x591290: the amount of `good` aboard.
    pub fn amount_of(&self, good: i32) -> i32 {
        self.slots.iter().filter(|s| s[0] == good).map(|s| s[1]).sum()
    }

    /// 0x591620: a slot is empty or holds `good` with room.
    pub fn has_room(&self, good: i32) -> bool {
        self.slots.iter().any(|s| s[0] == 0 || (s[0] == good && s[1] < SLOT_SIZE))
    }

    /// 0x591660: full. Domestic: every slot holds a good and 8. Foreign: more than 15 goods or
    /// more than one slot with goods (counted slot by slot).
    pub fn is_full(&self) -> bool {
        if !self.foreign {
            return self.slots.iter().all(|s| s[0] != 0 && SLOT_SIZE - s[1] < 1);
        }
        let (mut total, mut used) = (0, 0);
        for s in &self.slots {
            if s[1] >= 1 {
                used += 1;
            }
            total += s[1];
            if total > 15 || used > 1 {
                return true;
            }
        }
        false
    }

    /// 0x591700: a slot holds `good` with goods.
    pub fn has_good(&self, good: i32) -> bool {
        self.slots.iter().any(|s| s[1] >= 1 && s[0] == good)
    }

    /// The goods aboard.
    pub fn total(&self) -> i32 {
        self.slots.iter().map(|s| s[1]).sum()
    }
}

pub fn data(world: &World, id: u16) -> Option<&TransportData> {
    get(world, id)?.transport.as_deref()
}

pub fn data_mut(world: &mut World, id: u16) -> Option<&mut TransportData> {
    get_mut(world, id)?.transport.as_deref_mut()
}

pub fn is_transport(world: &World, id: u16) -> bool {
    data(world, id).is_some()
}

/// The ctor 0x590ba0 (after `CShip`'s): empty, the current task as the last one.
pub fn init(world: &mut World, id: u16) {
    if let Some(v) = get_mut(world, id) {
        v.transport = Some(Box::new(TransportData { last_task: v.task, ..TransportData::default() }));
    }
}

/// A building entity (the original's `obj type == 8` casts).
fn building(world: &World, id: u16) -> Option<&crate::entity::BuildingData> {
    match world.entities.by_index(id as u32).filter(|_| id != 0).map(|e| &e.data) {
        Some(crate::entity::EntityData::Building(d)) => Some(d),
        _ => None,
    }
}

/// 0x591310 `(good, n)`: up to `n` of `good` into the slots of the good, then into empty slots
/// (8 each). Returns what did not fit.
pub fn load(world: &mut World, id: u16, good: i32, n: i32) -> i32 {
    let Some(t) = data_mut(world, id) else { return n };
    let mut n = n;
    for s in t.slots.iter_mut().filter(|s| s[0] == good) {
        let room = SLOT_SIZE - s[1];
        if room != 0 {
            let k = n.min(room);
            n -= k;
            s[1] += k;
        }
    }
    for s in t.slots.iter_mut() {
        if n == 0 {
            break;
        }
        if s[0] == 0 {
            s[0] = good;
            let k = n.min(SLOT_SIZE);
            s[1] += k;
            n -= k;
        }
    }
    // (A ship with flag 0x400 refreshes its dialog: the client's.)
    n
}

/// 0x5915b0 `(good, n)`: up to `n` of `good` off the slots; an emptied slot loses its good.
/// Returns what was not there.
pub fn unload(world: &mut World, id: u16, good: i32, n: i32) -> i32 {
    let Some(t) = data_mut(world, id) else { return n };
    let mut n = n;
    for s in t.slots.iter_mut().filter(|s| s[0] == good) {
        let k = n.min(s[1]);
        s[1] -= k;
        if s[1] == 0 {
            s[0] = 0;
        }
        n -= k;
    }
    n
}

/// 0x591a80: with cargo, every good goes (0x591ae0) and VULNERABLE (0x30000) is off.
fn drop_cargo(world: &mut World, id: u16) {
    if data(world, id).is_none_or(|t| t.slots.iter().all(|s| s[1] == 0)) {
        return;
    }
    for g in 0..GOODS {
        let n = data(world, id).map_or(0, |t| t.amount_of(g));
        if n != 0 {
            unload(world, id, g, n);
        }
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::VULNERABLE_MASK;
    }
}

/// 0x591f80 `(foreign)`: +0xad; domestic clears the leg; foreign drops VULNERABLE and steps the
/// leg (0 / 2 → 1, 1 → 2).
pub fn set_foreign(world: &mut World, id: u16, foreign: bool) {
    if foreign && let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::VULNERABLE_MASK;
    }
    let Some(t) = data_mut(world, id) else { return };
    t.foreign = foreign;
    if !foreign {
        t.leg = 0;
    } else {
        t.leg = match t.leg {
            0 | 2 => 1,
            1 => 2,
            l => l,
        };
    }
}

fn send_to(world: &mut World, id: u16, to: i32) {
    logic::set_event(world, id, EntityEvent { event: 0x11, kind: 0xd, data: [0, to, 0] });
}

/// 0x5919d0: with a target, sail to its working area (+0x4c): to the target (+0xa5), not the
/// source.
pub fn sail_to_target(world: &mut World, id: u16) {
    let target = data(world, id).map_or(0, |t| t.target);
    if target == 0 {
        return;
    }
    // (The original reads +0x4c of a null building when the target is no building.)
    let Some(to) = building(world, target).map(|d| d.working_area) else { return };
    if let Some(t) = data_mut(world, id) {
        t.to_source = false;
        t.to_target = true;
    }
    send_to(world, id, to.0 as i32);
}

/// 0x591b40: home: the cargo goes (0x591a80), it sails to +0xa0, no source / target, going home.
pub fn go_home(world: &mut World, id: u16) {
    let home = data(world, id).map_or(0, |t| t.home);
    drop_cargo(world, id);
    send_to(world, id, home);
    if let Some(t) = data_mut(world, id) {
        t.to_source = false;
        t.to_target = false;
        t.target = 0;
        t.source = 0;
        t.going_home = true;
    }
}

/// 0x591740: the target is gone (or unreachable): it forgets the ship; a living source port
/// becomes the target (recorded there) and the ship sails back with its goods; a source that is
/// no building: the cargo goes; then (without a source too) state 0 and home.
pub fn target_gone(world: &mut World, id: u16) {
    let Some(t) = data(world, id) else { return };
    let (target, source, foreign) = (t.target, t.source, t.foreign);
    if target != 0 && building(world, target).is_some() {
        crate::buildings::trading::remove_transport(world, target, id, crate::buildings::trading::transport::SHIP);
    }
    if let Some(t) = data_mut(world, id) {
        t.target = 0;
    }
    if source != 0 {
        if building(world, source).is_some() {
            if let Some(t) = data_mut(world, id) {
                t.source = 0;
                t.target = source;
            }
            crate::buildings::trading::add_transport(world, source, id, crate::buildings::trading::transport::SHIP, foreign);
            sail_to_target(world, id);
            return;
        }
        drop_cargo(world, id);
    }
    if let Some(t) = data_mut(world, id) {
        t.state = 0;
    }
    go_home(world, id);
}

/// 0x591830: the source is gone: forgotten; on the way there the source's record goes, state 0,
/// home.
pub fn source_gone(world: &mut World, id: u16) {
    let Some(t) = data(world, id) else { return };
    let (source, to_source) = (t.source, t.to_source);
    if let Some(t) = data_mut(world, id) {
        t.source = 0;
    }
    if !to_source {
        return;
    }
    if building(world, source).is_some() {
        crate::buildings::trading::remove_transport(world, source, id, crate::buildings::trading::transport::SHIP);
    }
    if let Some(t) = data_mut(world, id) {
        t.state = 0;
    }
    go_home(world, id);
}

/// 0x591950: the walk ended: at the source / target it sailed to that port takes it (0x555800);
/// going home: no longer, the cargo goes.
fn arrived(world: &mut World, id: u16) {
    let Some(t) = data(world, id) else { return };
    let (to_source, source) = (t.to_source, t.source);
    if to_source && source != 0 && building(world, source).is_some() {
        crate::buildings::trading::ship_arrived(world, source, id);
    }
    let Some(t) = data(world, id) else { return };
    let (to_target, target) = (t.to_target, t.target);
    if to_target && target != 0 && building(world, target).is_some() {
        crate::buildings::trading::ship_arrived(world, target, id);
    }
    if data(world, id).is_some_and(|t| t.going_home) {
        if let Some(t) = data_mut(world, id) {
            t.going_home = false;
        }
        drop_cargo(world, id);
    }
}

/// vtbl+0x90 (0x591c30), see the module doc.
pub fn update(world: &mut World, id: u16) {
    if super::get(world, id).is_none_or(|v| v.health == 0) {
        return;
    }
    super::ship::repair(world, id);
    // (A ship with flag 0x400 refreshes its dialog, vtbl+0x94: the client's.)
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return };
    let (task, state) = (get(world, id).map_or(0, |v| v.task), data(world, id).map_or(0, |t| t.state));
    match task {
        logic::task::GO => {
            if let Some(t) = data_mut(world, id) {
                t.last_task = logic::task::GO;
            }
            super::ship::walk(world, id, pos, true);
        }
        logic::task::IDLE if state != 3 && state != 4 => super::ship::walk(world, id, pos, false),
        0x1b => logic::next_task(world, id),
        _ => {}
    }
    let (task, last) = (get(world, id).map_or(0, |v| v.task), data(world, id).map_or(0, |t| t.last_task));
    if task != last && last == logic::task::GO {
        arrived(world, id);
    }
    if get(world, id).is_some_and(|v| v.b5c & 0x40 != 0) {
        target_gone(world, id);
    }
    let task = get(world, id).map_or(0, |v| v.task);
    if let Some(t) = data_mut(world, id) {
        t.last_task = task;
    }
}

/// vtbl+0xbc (0x591da0): the ships' next task (0x5907a0), then the task is the last one.
pub fn next_task(world: &mut World, id: u16) {
    super::ship::next_task(world, id);
    let task = get(world, id).map_or(0, |v| v.task);
    if let Some(t) = data_mut(world, id) {
        t.last_task = task;
    }
}

/// vtbl+0x6c (0x591d20): event 7 / 9 from the target (data 0) → 0x591740, from the source →
/// 0x591830; then a move order (0xd: data 1 / 2; 0x11 / 0xd: packed data 1, flags data 0) makes
/// the task list. Returns whether a new list started.
pub fn convert_event(world: &mut World, id: u16, ev: &EntityEvent) -> bool {
    if matches!(ev.event, 7 | 9) {
        if data(world, id).is_some_and(|t| ev.data[0] == t.target as i32) {
            target_gone(world, id);
        }
        if data(world, id).is_some_and(|t| ev.data[0] == t.source as i32) {
            source_gone(world, id);
        }
    }
    match ev.event {
        0xd => logic::new_task_list(world, id, ev.data[1], ev.data[2], ev.data[0] as u32),
        0x11 if ev.kind == 0xd => {
            let p = Y16X16(ev.data[1] as u32);
            logic::new_task_list(world, id, p.x(), p.y(), ev.data[0] as u32)
        }
        _ => false,
    }
}

/// vtbl+0xc8 (0x591890): off the map: the target and source ports forget it, then the ships'
/// part (0x590a20). (The original prints "CTransportShip destroyed".)
pub fn off_map(world: &mut World, id: u16) {
    let Some(t) = data(world, id) else { return };
    let (target, source) = (t.target, t.source);
    for b in [target, source] {
        if b != 0 && building(world, b).is_some() {
            crate::buildings::trading::remove_transport(world, b, id, crate::buildings::trading::transport::SHIP);
        }
    }
    if let Some(t) = data_mut(world, id) {
        t.target = 0;
        t.source = 0;
    }
}

/// The free transport ship nearest to (x, y) on its sector (0x4fa050 for vehicle type 3; vtbl+0x74
/// 0x5955c0: standing (task 0x11 or 0) and READY).
pub fn nearest_free(world: &World, player: u8, x: i32, y: i32) -> u16 {
    let Some(n) = world.grid.checked_index(x, y) else { return 0 };
    let sector = world.tiling.sector_of(n);
    let (mut best, mut best_d) = (0u16, 0xffff);
    for id in super::iter(world, player, super::kind::TRANSPORT_SHIP) {
        let Some(e) = world.entities.by_index(id as u32) else { continue };
        let Some(en) = world.grid.checked_index(e.x(), e.y()) else { continue };
        let free = e.flags & flag::READY != 0 && get(world, id).is_some_and(|v| matches!(v.task, logic::task::IDLE | 0));
        if world.tiling.sector_of(en) != sector || !free {
            continue;
        }
        let d = crate::spiral::grid_distance(x - e.x(), y - e.y());
        if d < best_d {
            best_d = d;
            best = id;
        }
    }
    best
}

/// The first ship of the war-map cell (16 × 16 nodes) of `pos`, head of the cell's vehicle list
/// (0x142f294 / 0x142f296), if the list is not empty.
pub fn ship_in_cell(world: &World, pos: Y16X16) -> Option<u16> {
    let (cx, cy) = (((pos.0 & 0xffff) >> 4) as usize, (pos.0 >> 20) as usize);
    let head = world.warmap.cell(cx, cy, 1).next()?;
    world.entities.by_index(head as u32).filter(|e| e.obj_type == ObjType::Ship || e.obj_type == ObjType::LandVehicle).map(|_| head)
}

/// The D41 fill (vtbl+0x94, 0x591dc0, event 0x25a; 0x25c refreshes): info code 0x16 at packet
/// (0x11aa918) +4, the eight slots `{good, amount}` at +8 + 12·i / +0xc + 12·i, +0xf9 = 0; the
/// rest of the packet keeps what an earlier fill left (no clear, unlike the donkey's 0x577240).
pub fn dialog(world: &World, id: u16) -> Option<[[i32; 2]; 8]> {
    data(world, id).map(|t| t.slots)
}
