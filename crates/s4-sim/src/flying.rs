//! Flying objects: `IFlyingEntity` (obj type 0x40, vtable 0x105a258, ctor 0x569100), the
//! Dark Tribe's manakopter `CManakopter` (vtable 0x105a3b8, ctor 0x56ae30) and the
//! `CFlyingMgr` lists. Own reverse engineering of the shipped exe; decompiles in
//! `tools/re/decompiled/script/manakopter/` (`manakopter.c`, `flyingmgr.c`, `effects*.c`,
//! `flying2.c`, `board*.c`).
//!
//! **The manager** (.bss 0x153fc98..0x153fd2c, cleared by `CLogic`'s dtor 0x4f25b0 and the
//! loader 0x569a30): per `player · 2 + type` a list head (0x153fc9c, linked through entity +0x28
//! previous / +0x2a next, new ones at the head) and a count (0x153fce4), and the last flying
//! object created (0x153fc98) — the one the scripts' `DarkTribe.FlyTo` / `LeaveMk` mean. The
//! manakopter is type 1; nothing creates type 0.
//!
//! **Creation** (0x569ca0 `(x, y, player)`: the manakopter hall 0x547bb3, `DarkTribe.AddManakopter`
//! 0x5cb370): `GetFreeSlot`, the ctors, the list, then the AI hears event 0x14 `(player, id, 0)`.
//! The entity is ON_BOARD (0x8000: never on a node), READY | 0x20000 | VISIBLE, warrior type 9
//! (war-map category 4, strength 1), race 3, `m_nType` 1, its hit points
//! `DTManakopterHitpoints` (250 from 256 up), state +0x4c = 2 (empty), the goal its position; its
//! first update 10 ticks later.
//!
//! **Flight.** The position belongs to a `CEffects` moving effect (type 8: 0x4e3d60 → 0x4e2800,
//! speed +0x30 = 60 by vtbl+0x38 0x4e3f20, clamped 10..=500), which the update reads
//! (vtbl+0x34 0x4e3e70) and steers (vtbl+0x3c 0x4e3f70 → 0x4e28e0), see [`Flight`]. The update
//! (vtbl+0x64 0x56b4e0): the entity takes the effect's position (war map `NotifyMove`), then
//! * at the goal: still in the air (+0x7c, the drawn height offset, below −50) it sinks 5 a
//!   tick and clears state bit 1 (the order); landed (+0x7c ≥ −50) it updates again in 10 ticks
//!   and on dark land (node flag 4) gets its full hit points back;
//! * away from the goal: below the cruising height (+0x7c ≥ −199) it climbs 5 a tick; up there
//!   it flies one node toward the goal (x and y each one step toward it, so diagonals both
//!   ways) and updates when it arrives.
//!
//! It is drawn as three effects (Gfx 37): 0x6d the body (job 9, 16 directions), 0x6e — the
//! moving effect itself — the shadow on the ground (job 10), 0x6f the rotor (job 11, one direction,
//! 6 frames). The +0x78 / +0x7c offsets (−10 / −50 at the start) lift the body and the rotor; the
//! rotor sounds 99 / 100 follow them: the client's. The attached effects' start (+8) is the
//! creation tick ([`Flyer::born`]); the rotor's frame is `(now − born) % 6`.
//!
//! **Crew.** Settlers come by event 0x1c `(manakopter)` (`CHJBRole` 0x589f70: they walk to it
//! with `SetMode(pos, 0x200000)`; the shaman's 0x587ac0 and the slave's 0x5882f0 likewise):
//! vtbl+0x70 (0x56bb00, [`attach`]) puts them on the observer list (+0x50) and makes it their
//! home; at CHECKIN (0x589b30 case 0x18; the slave's 0x588080 only within distance 4)
//! [`board`] (0x56b890) takes them into the passenger vector (+0x58: unique id, id, type) and
//! off the war map. State bits: 1 an order to fly, 2 empty, 4 crew, 8 full (when +0x6c, +0x70
//! and +0x74 are all 0 — [inf] nothing in the code that tests obj type 0x40 lowers them from
//! 999 / 3 / 999). [`unload`] (0x56b990; the task force's state 0x6e, `DarkTribe.LeaveMk`)
//! sends every passenger out: event 0x19 `[0, place, 0]` and a move order 0x11 / 0xd to the
//! same place (the manakopter's position + spiral entry `8 + (dir + 3) % 6`; dir +0x4e stays 0).
//!
//! **Damage** (vtbl+0x20 0x56b490): above `DTManakopterArmor` the armor comes off, otherwise 1;
//! at or below 0 hit points it dies (`Kill(id, attacker)`). **Delete** (vtbl+0x10 0x56b330):
//! the passengers die (`Kill(id, 0)`), the explosion effect 0x72 (sound 0x65), event 9 `[id,
//! +0x25]` to the observers (whose list empties, 0x537430 → 0x56e130), out of the list.
//!
//! Targets: [`nearest`] (0x569e60) — the bowmen (0x599010 / 0x599400), the tower soldiers
//! (0x58c9d0) and the Dark Tribe AI (0x42c750, 0x435710) look for enemy manakopters with it.

use crate::World;
use crate::entity::{Entity, EntityData, ObjType, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;
use crate::jobs::state::EntityEvent;

/// `m_nType` of the manakopter.
pub const MANAKOPTER: u16 = 1;
/// The moving effect's gfx (+0x40).
const GFX: u8 = 0x6e;

/// State +0x4c bits.
pub mod state {
    /// Ordered to fly to the goal (the task force / `FlyTo`); cleared when it starts to land.
    pub const ORDERED: u16 = 1;
    /// No crew.
    pub const EMPTY: u16 = 2;
    /// Crew aboard or on the way.
    pub const CREW: u16 = 4;
    /// Full.
    pub const FULL: u16 = 8;
}

/// `CFlyingMgr`'s lists (.bss 0x153fc98..).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FlyingMgr {
    /// 0x153fc9c `[player · 2 + type]`: the list heads.
    pub heads: [u16; 18],
    /// 0x153fce4 `[player · 2 + type]`: the counts.
    pub counts: [i32; 18],
    /// 0x153fc98: the last flying object created.
    pub last: u16,
}

impl FlyingMgr {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        self.heads.iter().for_each(|&x| h.u16(x));
        self.counts.iter().for_each(|&x| h.u32(x as u32));
        h.u16(self.last);
    }
}

/// A `CEffects` moving effect (type 8; the record at `CEffects` +0x41b0 + id · 0x3c), as far as
/// it decides the position: map nodes +0x10 / +0x14 (from) and +0x18 / +0x1c (to), whole screen
/// pixels +0x20 / +0x24 and +0x28 / +0x2c (0x65b130, [`crate::projectile::screen`]), the start
/// tick +8, the ticks of the move +3 (a byte), the speed +0x30. (Frame +7, frame count +6 and
/// the sprite direction +5, an `f32` octant 0x4e1f30, only draw.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Flight {
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub screen_from: (i32, i32),
    pub screen_to: (i32, i32),
    pub start: u32,
    pub ticks: u8,
    pub speed: i32,
}

impl Flight {
    /// 0x4e2800 (vtbl+0x28 `(gfx, x, y)` → 0x4e3d60): standing at `(x, y)`, speed 100.
    fn new(world: &World, x: i32, y: i32) -> Flight {
        let s = crate::projectile::screen(world, x, y);
        Flight { from: (x, y), to: (x, y), screen_from: s, screen_to: s, start: world.tick, ticks: 0, speed: 100 }
    }

    /// Ticks since the start (0 before it, unsigned compare).
    fn elapsed(&self, now: u32) -> i32 {
        if self.start <= now { now.wrapping_sub(self.start) as i32 } else { 0 }
    }

    /// vtbl+0x34 (0x4e3e70): the node the effect is at, `from + (to − from) · elapsed / ticks`
    /// (truncated) during the move, `to` after it.
    pub fn position(&self, now: u32) -> (i32, i32) {
        let (el, n) = (self.elapsed(now), self.ticks as i32);
        if el >= n {
            return self.to;
        }
        if el <= 0 {
            return self.from;
        }
        ((self.to.0 - self.from.0) * el / n + self.from.0, (self.to.1 - self.from.1) * el / n + self.from.1)
    }

    /// 0x4e28e0 `(x, y, stop)` (vtbl+0x3c 0x4e3f70): the move so far is kept (from and the
    /// screen position advance by `elapsed / ticks`, or to the end), then the effect heads for
    /// `(x, y)`: `ticks = clamp(isqrt(dx² + dy²) · speed >> 9, 1, 255)` of the screen distance.
    /// `stop` (bit 0) halts it where it is (1 tick). Returns the ticks.
    fn move_to(&mut self, world: &World, x: i32, y: i32, stop: bool) -> u32 {
        let now = world.tick;
        let (el, n) = (self.elapsed(now), self.ticks as i32);
        if el < n {
            if el > 0 {
                let step = |a: i32, b: i32| a + (b - a) * el / n;
                self.from = (step(self.from.0, self.to.0), step(self.from.1, self.to.1));
                self.screen_from = (step(self.screen_from.0, self.screen_to.0), step(self.screen_from.1, self.screen_to.1));
            }
        } else {
            self.from = self.to;
            self.screen_from = self.screen_to;
        }
        let ticks = if stop {
            self.to = self.from;
            self.screen_to = self.screen_from;
            1
        } else {
            self.to = (x, y);
            self.screen_to = crate::projectile::screen(world, x, y);
            let (dx, dy) = (self.screen_to.0 - self.screen_from.0, self.screen_to.1 - self.screen_from.1);
            let d2 = (dy.wrapping_mul(dy)).wrapping_add(dx.wrapping_mul(dx)) as u32;
            (isqrt(d2) as i32).wrapping_mul(self.speed) >> 9
        }
        .clamp(1, 255);
        self.ticks = ticks as u8;
        self.start = now;
        ticks as u32
    }
}

/// 0x4e1fe0: `floor(sqrt(n))` of an unsigned 32-bit value, bit by bit.
fn isqrt(mut n: u32) -> u32 {
    let mut bit: u32 = if n < 0x10000 { 0x4000 } else { 0x4000_0000 };
    let mut r: u32 = 0;
    while bit != 0 {
        let t = bit.wrapping_add(r);
        if t <= n {
            n -= t;
            r = t.wrapping_add(bit);
        }
        bit >>= 2;
        r >>= 1;
    }
    r
}

/// One record of the passenger vector +0x58 (12 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Passenger {
    pub uid: u32,
    pub id: u16,
    pub kind: u16,
}

/// The `IFlyingEntity` / `CManakopter` members (entity +0x21, +0x40..+0x7c).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Flyer {
    /// +0x21 hit points.
    pub health: u8,
    /// +0x44 the moving effect.
    pub flight: Flight,
    /// +0x48 / +0x4a the goal.
    pub goal: (u16, u16),
    /// +0x4c [`state`] bits.
    pub state: u16,
    /// +0x4e direction (0 from the ctor; nothing changes it).
    pub dir: i8,
    /// +0x50 the observer list (`std::list<u16>`).
    pub observers: Vec<u16>,
    /// +0x58 the passengers.
    pub passengers: Vec<Passenger>,
    /// +0x6c / +0x70 / +0x74 (999 / 3 / 999).
    pub room: [i32; 3],
    /// +0x78 / +0x7c the drawn offset of the body and rotor effects (x, height).
    pub offset: (i32, i32),
    /// The attached effects' start (+8): the creation tick ([inf] the logic tick).
    #[cfg_attr(feature = "serde", serde(default))]
    pub born: u32,
}

impl Flyer {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u8(self.health);
        let f = &self.flight;
        for v in [f.from.0, f.from.1, f.to.0, f.to.1, f.screen_from.0, f.screen_from.1, f.screen_to.0, f.screen_to.1, f.start as i32, f.speed] {
            h.u32(v as u32);
        }
        h.u8(f.ticks);
        h.u16(self.goal.0);
        h.u16(self.goal.1);
        h.u16(self.state);
        h.u8(self.dir as u8);
        h.u32(self.observers.len() as u32);
        self.observers.iter().for_each(|&o| h.u16(o));
        h.u32(self.passengers.len() as u32);
        for p in &self.passengers {
            h.u32(p.uid);
            h.u16(p.id);
            h.u16(p.kind);
        }
        self.room.iter().for_each(|&r| h.u32(r as u32));
        h.u32(self.offset.0 as u32);
        h.u32(self.offset.1 as u32);
        h.u32(self.born);
    }
}

/// The flying object `id` (0x569d90: obj type 0x40), living or not.
pub fn get(world: &World, id: u16) -> Option<&Flyer> {
    match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Flying(f)) => Some(f),
        _ => None,
    }
}

pub fn get_mut(world: &mut World, id: u16) -> Option<&mut Flyer> {
    match world.entities.by_index_mut(id as u32).map(|e| &mut e.data) {
        Some(EntityData::Flying(f)) => Some(f),
        _ => None,
    }
}

/// A flying object that has not died (entity flag 0x4000000 clear).
pub fn alive(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Flying && e.flags & flag::DIED == 0)
}

fn slot(player: u8, kind: u16) -> usize {
    (player as usize * 2 + kind as usize).min(17)
}

/// 0x569ca0 `(x, y, player)`: a new manakopter (see the module doc). `None` without a free
/// entity slot.
pub fn add_manakopter(world: &mut World, x: i32, y: i32, player: u8) -> Option<u16> {
    let id = world.entities.free_slot()?;
    // IFlyingEntity ctor 0x569100 (the effect 0x4e3d60, ON_BOARD, state 2, the goal), then
    // CManakopter ctor 0x56ae30 (speed 60, offsets, room, type 1, hit points, flags).
    let mut flight = Flight::new(world, x, y);
    flight.speed = 60;
    let hp = world.rules.magic.dark.manakopter_hitpoints;
    let f = Flyer {
        health: if hp < 0x100 { hp as u8 } else { 250 },
        flight,
        goal: (x as u16, y as u16),
        state: state::EMPTY,
        dir: 0,
        observers: Vec::new(),
        passengers: Vec::new(),
        room: [999, 3, 999],
        offset: (-10, -50),
        born: world.tick,
    };
    let mut e = Entity::new(ObjType::Flying, MANAKOPTER, player, 3, Y16X16::pack(x, y), EntityData::Flying(Box::new(f)));
    e.flags = flag::ON_BOARD | 0x0202_0109;
    world.entities.insert(id, e);
    crate::warmap::readd(world, id);
    world.register_for_logic_update(id, 10);
    // 0x569ca0: the list, the count, the last one, the AI.
    let k = slot(player, MANAKOPTER);
    let head = world.flying.heads[k];
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.prev = 0;
        e.next = head;
    }
    if head != 0
        && let Some(h) = world.entities.by_index_mut(head as u32)
    {
        h.prev = id;
    }
    world.flying.last = id;
    world.flying.counts[k] += 1;
    world.flying.heads[k] = id;
    crate::ai::post_event(world, 0x14, player as i32, id as i32, 0);
    let _ = GFX;
    Some(id)
}

/// 0x569f00: out of its list, one fewer counted.
fn unlist(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (k, prev, next) = (slot(e.owner, e.kind), e.prev, e.next);
    world.flying.counts[k] -= 1;
    if prev == 0 {
        world.flying.heads[k] = next;
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

/// The flying objects of `player` of type `kind`, in list order (0x547710 reads the head).
pub fn list(world: &World, player: u8, kind: u16) -> Vec<u16> {
    let mut out = Vec::new();
    let mut i = world.flying.heads[slot(player, kind)];
    while i != 0 && out.len() < 0x10000 {
        out.push(i);
        i = world.entities.by_index(i as u32).map_or(0, |e| e.next);
    }
    out
}

/// 0x42ff20: how many manakopters `player` has.
pub fn count(world: &World, player: u8) -> i32 {
    world.flying.counts[slot(player, MANAKOPTER)]
}

/// 0x569e60 `(player, x, y, mask)`: the manakopter of `player` nearest to `(x, y)` (hex
/// distance, the first of equals in list order) whose state has a bit of `mask` (any with 0);
/// 0 = none.
pub fn nearest(world: &World, player: u8, x: i32, y: i32, mask: u16) -> u16 {
    let mut best = (u32::MAX, 0);
    for id in list(world, player, MANAKOPTER) {
        let Some(e) = world.entities.by_index(id as u32) else { continue };
        let d = crate::spiral::grid_distance(x - e.x(), y - e.y()) as u32;
        let st = get(world, id).map_or(0, |f| f.state);
        if d < best.0 && (mask == 0 || mask & st != 0) {
            best = (d, id);
        }
    }
    best.1
}

/// `CManakopter::LogicUpdate` (vtbl+0x64, 0x56b4e0); see the module doc. Every path but the
/// dead one ends in the rotor sounds (0x56b71a, `SimEvent::Rotor`).
pub fn logic_update(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.flags & flag::DIED != 0 || e.obj_type != ObjType::Flying {
        return;
    }
    update(world, id);
    if let (Some(e), Some(f)) = (world.entities.by_index(id as u32), get(world, id)) {
        let ev = crate::event::SimEvent::Rotor { id, x: e.x(), y: e.y(), ordered: f.state & state::ORDERED != 0 };
        world.events.push(ev);
    }
}

fn update(world: &mut World, id: u16) {
    let now = world.tick;
    let Some(f) = get(world, id) else { return };
    let (x, y) = f.flight.position(now);
    let (goal, offset) = (f.goal, f.offset);
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.pos = Y16X16::pack(x, y);
    }
    crate::warmap::moved(world, id);
    if (x, y) == (goal.0 as i32, goal.1 as i32) {
        if offset.0 > -11 && offset.1 > -51 {
            world.register_for_logic_update(id, 10);
            let dark = world.grid.checked_index(x, y).is_some_and(|n| world.layers.flags[n] & 4 != 0);
            if dark {
                let hp = world.rules.magic.dark.manakopter_hitpoints as u8;
                if let Some(f) = get_mut(world, id) {
                    f.health = hp;
                }
            }
            return;
        }
        if let Some(f) = get_mut(world, id) {
            f.offset.1 += 5;
            f.state &= !state::ORDERED;
        }
        world.register_for_logic_update(id, 1);
        return;
    }
    if offset.0 < -9 && offset.1 < -199 {
        let step = |c: i32, g: u16| if c == g as i32 { 0 } else if c <= g as i32 { 1 } else { -1 };
        let (nx, ny) = (x + step(x, goal.0), y + step(y, goal.1));
        let mut flight = get(world, id).map(|f| f.flight).unwrap_or_default();
        let ticks = flight.move_to(world, nx, ny, false);
        if let Some(f) = get_mut(world, id) {
            f.flight = flight;
        }
        world.register_for_logic_update(id, ticks as i32);
        return;
    }
    if let Some(f) = get_mut(world, id) {
        f.offset.1 -= 5;
    }
    world.register_for_logic_update(id, 1);
}

/// What the task force's state 0x6d (0x448120) and `DarkTribe.FlyTo` (0x5cb4a0) do: the goal,
/// state bit 1, an update next tick.
pub fn fly_to(world: &mut World, id: u16, x: i32, y: i32) {
    let Some(f) = get_mut(world, id) else { return };
    f.state |= state::ORDERED;
    f.goal = (x as u16, y as u16);
    // 0x5cb4a0 registers it whether it died or not.
    world.register_for_logic_update(id, 1);
}

/// vtbl+0x20 (0x56b490) `(n)`: the hit points it loses, `attacker` the global of
/// `TakeDamage` (0x129b0b8).
pub fn damage(world: &mut World, id: u16, n: i32, attacker: u8) {
    if n <= 0 {
        return;
    }
    let armor = world.rules.magic.dark.manakopter_armor;
    let d = if armor < n { n - armor } else { 1 };
    if d < 1 {
        return;
    }
    let Some(f) = get_mut(world, id) else { return };
    if f.health as i32 <= d {
        f.health = 0;
        world.kill(id, attacker);
    } else {
        f.health -= d as u8;
    }
}

/// `IAnimatedEntity::SetEvent` (vtbl+0x50) of an observer.
fn send_event(world: &mut World, id: u16, ev: EntityEvent) {
    match world.entities.by_index(id as u32).map(|e| e.obj_type) {
        Some(ObjType::Settler) => crate::jobs::core::set_event(world, id, ev),
        Some(ObjType::Building) => crate::buildings::set_event(world, id, ev),
        Some(ObjType::Ship | ObjType::LandVehicle) => crate::vehicles::logic::set_event(world, id, ev),
        Some(ObjType::Pile) => crate::pile::set_event(world, id, ev),
        _ => {}
    }
}

/// `CManakopter::Delete` (vtbl+0x10, 0x56b330), from `Kill`.
pub fn on_delete(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let pos = e.pos;
    let passengers = get_mut(world, id).map(|f| std::mem::take(&mut f.passengers)).unwrap_or_default();
    for p in passengers.iter().rev() {
        let same = world.entities.by_index(p.id as u32).is_some_and(|e| e.obj_type == ObjType::Settler && e.uid == p.uid);
        if same {
            world.kill(p.id, 0);
        }
    }
    world.events.push(crate::event::SimEvent::RotorStop { id });
    world.events.push(crate::event::SimEvent::Effect { effect: 0x72, sound: 0x65, x: pos.x(), y: pos.y() });
    // Event 9 [id, +0x25] to the observers, then their list empties.
    let observers = get_mut(world, id).map(|f| std::mem::take(&mut f.observers)).unwrap_or_default();
    for o in observers {
        send_event(world, o, EntityEvent { event: 9, kind: 0, data: [id as i32, 0, 0] });
    }
    unlist(world, id);
}

/// `SetObserverTarget(0, home)` (settler vtbl+0x44, 0x581710): the role's home (+0x20) and
/// ATTACHED with one.
fn set_home(world: &mut World, settler: u16, home: u16) {
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.home = home;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        if home != 0 {
            e.flags |= flag::ATTACHED;
        } else {
            e.flags &= !flag::ATTACHED;
        }
    }
}

/// vtbl+0x70 (0x56bb00) `(settler)`: crew on the way (state bit 2 off, 4 on); the settler joins
/// the observer list (0x56e0a0, at the end) and the manakopter becomes its home.
pub fn attach(world: &mut World, id: u16, settler: u16) {
    let Some(f) = get_mut(world, id) else { return };
    f.state = (f.state & !state::EMPTY) | state::CREW;
    f.observers.push(settler);
    set_home(world, settler, id);
}

/// vtbl+0x40 (`IFlyingEntity` 0x569390) `(settler)`: off the observer list (0x56e270, the first
/// entry), no home.
pub fn detach(world: &mut World, id: u16, settler: u16) {
    if let Some(f) = get_mut(world, id)
        && let Some(k) = f.observers.iter().position(|&o| o == settler)
    {
        f.observers.remove(k);
    }
    set_home(world, settler, 0);
}

/// 0x56b890 `(settler)`: a living settler boards — into the passenger vector unless there,
/// out of the war map, off its node, ON_BOARD; state bit 2 off, 4 on (and 8 when the room
/// counters are all 0). False for a dead or missing settler.
pub fn board(world: &mut World, id: u16, settler: u16) -> bool {
    let Some(e) = world.entities.by_index(settler as u32) else { return false };
    if e.obj_type != ObjType::Settler || e.flags & flag::DIED != 0 {
        return false;
    }
    let (uid, kind, pos) = (e.uid, e.kind, e.pos);
    let Some(f) = get_mut(world, id) else { return false };
    if !f.passengers.iter().any(|p| p.id == settler) {
        f.passengers.push(Passenger { uid, id: settler, kind });
    }
    if world.warmap.contains(settler) {
        crate::warmap::remove(world, settler);
    }
    if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
        world.layers.map_object[n] = 0;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags |= flag::ON_BOARD;
    }
    if let Some(f) = get_mut(world, id) {
        f.state = (f.state & !state::EMPTY) | state::CREW;
        if f.room == [0; 3] {
            f.state |= state::FULL;
        }
    }
    true
}

/// 0x56b990: everyone out (see the module doc); room 999 / 3 / 999, state bits 4 and 8 off, 2
/// on.
pub fn unload(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let pos = e.pos;
    let Some(f) = get_mut(world, id) else { return };
    let k = 8 + (f.dir as i32 + 3).rem_euclid(6) as usize;
    let passengers = std::mem::take(&mut f.passengers);
    let place = Y16X16::pack(pos.x() + crate::spiral::dx(k), pos.y() + crate::spiral::dy(k)).0 as i32;
    for p in passengers.iter().rev() {
        let same = world.entities.by_index(p.id as u32).is_some_and(|e| e.obj_type == ObjType::Settler && e.uid == p.uid);
        if same {
            send_event(world, p.id, EntityEvent { event: 0x19, kind: 0, data: [0, place, 0] });
            send_event(world, p.id, EntityEvent { event: 0x11, kind: 0xd, data: [0, place, 0] });
        }
    }
    if let Some(f) = get_mut(world, id) {
        f.room = [999, 3, 999];
        f.state = (f.state & !(state::CREW | state::FULL)) | state::EMPTY;
    }
}
