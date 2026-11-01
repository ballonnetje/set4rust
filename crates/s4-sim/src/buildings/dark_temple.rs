//! `CDarkTempleRole` (house kind 18, release vtable 0x1059284): the Dark Tribe's temple and
//! fortress (both kind 18). Decompiles in `tools/re/decompiled/script/dark/temple.c`,
//! `temple2.c`, `pull_effect.c` (LogicUpdate), `mushfarm2.c` (0x5456b0).
//!
//! Ported: the parts the mushroom farms need — making a settler at the temple
//! ([`make_settler`]), taking farmers, slaves and dark gardeners in ([`settler_enter`]), letting
//! the gardeners out again and sending idle slaves to the farms.
//!
//! The unit production from mana and the aura (sim's: [`take_order`] 0x545d60, [`produce`]
//! 0x545760, [`aura`] 0x5458c0).

use super::{Role, RoleKind};
use crate::World;
use crate::entity::{EntityData, flag};
use crate::grid::Y16X16;
use crate::hash::StateHasher;

/// `CDarkTempleRole` +0x17c .. +0x1ac.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct State {
    /// +0x17c: where new settlers appear, the door + (1, 1).
    pub exit: u32,
    /// +0x180 .. +0x194: unit orders per slot of [`UNIT_TYPES`] (100 = without end); +0x198 the
    /// next slot; +0x19c the tick of the last mushroom farmer (−0x400 at Init).
    pub orders: [i32; 6],
    pub next: i32,
    pub last_unit: i32,
    /// +0x1a0: slaves taken in.
    pub slaves_in: i32,
    /// +0x1a4: dark gardeners inside, let out last in first.
    pub inside: Vec<u16>,
}

impl State {
    pub(crate) fn hash(&self, h: &mut StateHasher) {
        h.u32(self.exit);
        self.orders.iter().for_each(|&o| h.u32(o as u32));
        h.u32(self.next as u32);
        h.u32(self.last_unit as u32);
        h.u32(self.slaves_in as u32);
        h.u16s(&self.inside);
    }
}

fn role(world: &mut World, id: u16) -> Option<(&mut super::Base, &mut State)> {
    match world.buildings.roles.get_mut(&id)? {
        Role { base, kind: RoleKind::DarkTemple(s) } => Some((base, s)),
        _ => None,
    }
}

pub fn get(world: &World, id: u16) -> Option<&State> {
    match world.buildings.roles.get(&id)? {
        Role { kind: RoleKind::DarkTemple(s), .. } => Some(s),
        _ => None,
    }
}

/// `CDarkTempleRole::Init` (0x545560), after `InitCommon`: the inhabitant flag +0x1d set, the exit
/// at the door + (1, 1), the counters cleared, +0x19c = −0x400; first update in 31 ticks.
pub fn init(world: &mut World, id: u16) {
    let door = match world.entities.by_index(id as u32).map(|e| &e.data) {
        Some(EntityData::Building(d)) => d.door,
        _ => return,
    };
    if let Some((base, s)) = role(world, id) {
        base.worker_in_house = true;
        *s = State { exit: Y16X16::pack(door.x() + 1, door.y() + 1).0, last_unit: -0x400, ..State::default() };
    }
    world.register_for_logic_update(id, 31);
}

/// `CDarkTempleRole::LogicUpdate` (0x5455e0).
/// * Building flags: with 0x800000 that bit goes, else 0x200000 (the aura display bits the aura
///   sets as 0xa00000).
/// * The unit production ([`produce`], 0x545760).
/// * The last dark gardener inside comes out at the exit with his default list. The release
///   writes his id into the map-object layer at the *packed* exit used as a node index (an
///   out-of-range write); the layer is left alone here.
/// * The aura ([`aura`], 0x5458c0), then the idle slaves (0x545e10); next update in 31 ticks.
pub fn logic_update(world: &mut World, id: u16) {
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= if e.flags & 0x80_0000 != 0 { !0x80_0000 } else { !0x20_0000 };
    }
    produce(world, id);
    let out = role(world, id).and_then(|(_, s)| s.inside.pop().map(|g| (g, s.exit)));
    if let Some((g, exit)) = out {
        if let Some(e) = world.entities.by_index_mut(g as u32) {
            e.pos = Y16X16(exit);
            e.flags |= flag::VISIBLE;
        }
        crate::jobs::core::take_wait_list(world, g);
    }
    aura(world, id);
    send_slaves(world, id);
    world.register_for_logic_update(id, 31);
}

/// 0x545e10: the first three of the player's slaves (`CSettlerMgr`'s list of type 55, newest
/// first) that are free (`CSettler` vtbl+0x74 → role vtbl+0x4c, `CSlaveRole` 0x588710: no home
/// and not in task 0x1b, [`crate::jobs::slave::is_free`]) go to a farm
/// ([`crate::dark::slave_to_farm`]).
fn send_slaves(world: &mut World, id: u16) {
    let Some(player) = world.entities.by_index(id as u32).map(|e| e.owner) else { return };
    let mut s = world.settler_lists.head(player, crate::dark::SLAVE);
    let mut sent = 0;
    while s != 0 {
        let next = world.entities.by_index(s as u32).map_or(0, |e| e.next);
        if crate::jobs::slave::is_free(world, s) {
            crate::dark::slave_to_farm(world, s);
            sent += 1;
            if sent > 2 {
                return;
            }
        }
        s = next;
    }
}

/// `CDarkTempleRole::SettlerEnter` (0x545c50):
/// * a dark gardener (52) waits inside: hidden, off the map-object layer, at the temple's
///   position, last on the list [`logic_update`] lets out;
/// * a mushroom farmer (53) brings the player mana ([`crate::magic::add_sphere`]), a slave (55)
///   counts +0x1a0; both leave the observer list, lose their home (`SetObserverTarget(0, 0)`),
///   are hidden and killed.
///
/// Returns true.
pub fn settler_enter(world: &mut World, id: u16, settler: u16) -> bool {
    let Some(e) = world.entities.by_index(settler as u32) else { return true };
    let (kind, pos) = (e.kind, e.pos);
    let Some(t) = world.entities.by_index(id as u32) else { return true };
    let (owner, tpos) = (t.owner, t.pos);
    match kind {
        crate::dark::DARK_GARDENER => {
            if let Some(n) = world.grid.checked_index(pos.x(), pos.y()) {
                world.layers.map_object[n] = 0;
            }
            if let Some(e) = world.entities.by_index_mut(settler as u32) {
                e.flags &= !flag::VISIBLE;
                e.pos = tpos;
            }
            if let Some((_, s)) = role(world, id) {
                s.inside.push(settler);
            }
            return true;
        }
        crate::dark::MUSHROOM_FARMER => crate::magic::add_sphere(world, owner),
        crate::dark::SLAVE => {
            if let Some((_, s)) = role(world, id) {
                s.slaves_in += 1;
            }
        }
        _ => return true,
    }
    super::detach(world, id, settler);
    if let Some(j) = crate::jobs::core::job_mut(world, settler) {
        j.home = 0;
    }
    if let Some(e) = world.entities.by_index_mut(settler as u32) {
        e.flags &= !(flag::ATTACHED | flag::VISIBLE);
    }
    world.kill(settler, 0);
    true
}

/// 0x5456b0 (`this` = the temple's role): a new settler of `kind` for `player` at the exit, or —
/// when the exit is off the map, blocked (flag 0x01) or taken — at the first node of the spiral
/// up to radius 15 around it (0x5856e0) with no flags 0x23, no map object and the exit's sector.
/// `CSettlerMgr::AddSettler(x, y, player, kind, how)` (0x582600; `how` is 0 from the mushroom
/// farmer orders 0x4cb7a5 / 0x545e02, 1 from the temple's unit production 0x54585c). Returns the
/// settler, 0 if none.
pub fn make_settler(world: &mut World, temple: u16, player: u8, kind: u16, how: i32) -> u16 {
    let Some(exit) = get(world, temple).map(|s| Y16X16(s.exit)) else { return 0 };
    let (x, y) = (exit.x(), exit.y());
    let free = world.grid.checked_index(x, y).filter(|&n| world.layers.flags[n] & 1 == 0 && world.layers.map_object[n] == 0);
    let (x, y) = match free {
        Some(_) => (x, y),
        None => {
            let sector = world.grid.checked_index(x, y).map_or(0, |n| world.tiling.sector_of(n));
            if sector == 0 {
                return 0;
            }
            let found = crate::spiral::walk(world.grid, x, y, 15).find(|&(px, py)| {
                let n = world.grid.index(px as u32, py as u32);
                world.layers.flags[n] & 0x23 == 0 && world.layers.map_object[n] == 0 && world.tiling.sector_of(n) == sector
            });
            match found {
                Some(p) => p,
                None => return 0,
            }
        }
    };
    crate::create::add_settler_ex(world, x, y, player, kind, how).unwrap_or(0)
}

/// The order slots' settler types (.rdata 0x101a230; slot 0 none): mushroom farmer, dark
/// gardener, shaman, swordsman, bowman. The type → slot map 0x15ab040 is built from it (ctors
/// 0x545140 / 0x5452f0).
pub const UNIT_TYPES: [u16; 6] = [0, 53, 52, 54, 29, 32];

/// `CDarkTempleRole::TakeOrder(type, amount)` (0x545d60, command 5047 on a Dark Tribe building):
/// for a type of [`UNIT_TYPES`] (1..=0x42) — an amount above 9999 first clears the order and
/// counts 10000 less; below −99 clears it; otherwise, while both the amount and the order are
/// below 100, the order grows by the amount within 0..=99, else it becomes 100 (no end).
pub fn take_order(world: &mut World, id: u16, kind: u16, amount: i32) {
    if !(1..=0x42).contains(&kind) {
        return;
    }
    let Some(slot) = UNIT_TYPES.iter().position(|&t| t == kind).filter(|&k| k != 0) else { return };
    let Some((_, s)) = role(world, id) else { return };
    let mut a = amount;
    if a > 9999 {
        a -= 10000;
        s.orders[slot] = 0;
    }
    if a < -99 {
        s.orders[slot] = 0;
        return;
    }
    if a < 100 && s.orders[slot] < 100 {
        s.orders[slot] = (s.orders[slot] + a).clamp(0, 99);
    } else {
        s.orders[slot] = 100;
    }
}

/// 0x545760, from [`logic_update`]: one unit bought with the player's mana, when it has at least
/// 10. Slot 1 (mushroom farmers) comes first when ordered and more than 30 ticks after the last
/// one; otherwise from the next slot (+0x198) on, the first of slots 2..=5 with an order. The
/// unit costs its `DTCost*` ([`crate::magic::DarkMagicCfg::unit_cost`]); with enough mana it is
/// paid, a limited order counts down, the settler appears at the temple ([`make_settler`]),
/// slot 1 notes the tick and the next slot follows; without, the slot stays next. No RNG.
/// The statistic counts the unit (0x54586a) whether it appeared or not.
pub fn produce(world: &mut World, id: u16) {
    let Some(player) = world.entities.by_index(id as u32).map(|e| e.owner) else { return };
    if crate::magic::mana(world, player) <= 9 {
        return;
    }
    let now = world.tick as i32;
    let Some((_, s)) = role(world, id) else { return };
    let first = if now.wrapping_sub(s.last_unit) > 30 && s.orders[1] > 0 { 1 } else { 0 };
    let mut i = s.next.rem_euclid(6) as usize;
    let mut pick = first;
    for _ in 0..6 {
        if i > 1 && s.orders[i] > 0 {
            pick = i;
            break;
        }
        i = (i + 1) % 6;
        pick = first;
    }
    if pick == 0 {
        return;
    }
    let kind = UNIT_TYPES[pick];
    let cost = world.rules.magic.dark.unit_cost(kind).unwrap_or(0);
    if !crate::magic::spend_dark_unit(world, player, cost) {
        if let Some((_, s)) = role(world, id) {
            s.next = pick as i32;
        }
        return;
    }
    if let Some((_, s)) = role(world, id)
        && s.orders[pick] < 100
    {
        s.orders[pick] -= 1;
    }
    make_settler(world, id, player, kind, 1);
    crate::statistic::settler_produced(world, player, kind);
    if let Some((_, s)) = role(world, id) {
        if pick == 1 {
            s.last_unit = now;
        }
        s.next = ((pick + 1) % 6) as i32;
    }
}

/// 0x5458c0, from [`logic_update`]: the temple's aura grows with the player's finished mushroom
/// farms `n` (`CBuildingMgr` 0x14ef7ac: type 49). With `n > 0` (all draws on the game RNG):
/// * one draw `r0` caps the far kills at `(r0 & 7) + 8`;
/// * 0x545ba0: `min(3n, 40)` effects 0x4e around `(x + 1, y + 2)` at spiral entries
///   `first(16) + draw % (last(16) − first(16))` (sound 0x30 for every `n`-th), client only;
/// * the spiral `0..=Last(n < 4 ? 2n + 16 : 24)` around `(x + 1, y + 2)`: on dark land (node
///   flags & 5 = 4) in the sector of the temple's ensign, every enemy warrior (a settler or land
///   vehicle with a warrior type) draws `r`; it is struck when `r < chance` (`n < 4 ?
///   n · 0x190000 / 100 : 0x10000`) — beyond ring 16 only while the far count stays within the
///   cap and a second draw is below `(8 − ring) << 13` (unsigned) — with effect 0x4e (sound 0x30)
///   and future event 2 (255 damage, attacker 0) `6 + (draw & 15)` ticks later. After `n < 4 ?
///   16 n² : 0x4000` warriors met the aura ends. The release also sets the temple's display bits
///   0xa00000 when the victim belongs to the local player (client state, left out).
pub fn aura(world: &mut World, id: u16) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let (pos, player) = (e.pos, e.owner);
    let ensign = match &e.data {
        crate::entity::EntityData::Building(d) => d.ensign,
        _ => return,
    };
    let n = world.statistic.players.get(player as usize).map_or(0, |p| p.count.get(49).copied().unwrap_or(0));
    if n <= 0 {
        return;
    }
    let (limit, chance, rings) = if n < 4 { (n * n * 16, (n as u32 * 0x19_0000) / 100, n * 2 + 16) } else { (0x4000, 0x1_0000, 24) };
    let cap = (world.rng.rand() & 7) as i32 + 8;
    let sector = world.grid.checked_index(ensign.x(), ensign.y()).map_or(0, |n| world.tiling.sector_of(n));
    let (cx, cy) = (pos.x() + 1, pos.y() + 2);
    // 0x545ba0: the aura's effects.
    let (f, span) = (crate::spiral::first(16), (crate::spiral::last(16) - crate::spiral::first(16)) as u32);
    for i in 0..(n * 3).min(40) {
        let k = f + (world.rng.rand() % span) as usize;
        let sound = if i % n == 0 { 0x30 } else { 0 };
        world.events.push(crate::event::SimEvent::Effect { effect: 0x4e, sound, x: cx + crate::spiral::dx(k), y: cy + crate::spiral::dy(k) });
    }
    let enemies = world.alliances.enemy_bits.get(player as usize).copied().unwrap_or(0) as u32;
    let (mut met, mut far) = (0, 0);
    for k in 0..=crate::spiral::last(rings as u8) {
        let (x, y) = (cx + crate::spiral::dx(k), cy + crate::spiral::dy(k));
        let Some(node) = world.grid.checked_index(x, y) else { continue };
        if world.layers.flags[node] & 5 != 4 {
            continue;
        }
        let m = world.layers.map_object[node];
        if m == 0 || world.tiling.sector_of(node) != sector {
            continue;
        }
        let Some(t) = world.entities.by_index(m as u32) else { continue };
        let warrior = matches!(t.obj_type, crate::entity::ObjType::Settler | crate::entity::ObjType::LandVehicle) && t.flags & 0xf != 0;
        if t.is_dead() || enemies & ((1u32 << t.owner) >> 1) == 0 || !warrior {
            continue;
        }
        let r = world.rng.rand();
        let hit = r < chance;
        let ring = crate::spiral::radius(k) as u32;
        let strike = if ring > 16 {
            far += 1;
            if far > cap {
                return;
            }
            hit && world.rng.rand() < (8u32.wrapping_sub(ring)) << 13
        } else {
            hit
        };
        if strike {
            let d = world.rng.rand() & 0xf;
            world.events.push(crate::event::SimEvent::Effect { effect: 0x4e, sound: 0x30, x, y });
            crate::future::add(world, 2, d + 6, m, 0xff, 0);
        }
        met += 1;
        if met >= limit {
            return;
        }
    }
}

