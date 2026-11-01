//! The player AI at run time (stage S1): its start (`Init`, vtbl+8), the per-tick update and the
//! event dispatch to its task forces and sector AIs. Decompiles in
//! `tools/re/decompiled/script/ai/` (`s1.c`, `save.c`, `taskforce.c`, `sectorai.c`,
//! `dtsector.c`).
//!
//! The schedulers run ([`run_scheduler`], [`super::sched`]). What most agents do when they run
//! (their `Execute`), the sector AIs' event handlers (vtbl+0x28) and the per-eco-sector economy
//! AIs (`CAIEcoSectorAIEx` with its `CEcoManager`, 0x4346d0 / 0x4439f0) are stages S2 / S3.
//! Until then those agents return "nothing" (period, default cost; see [`execute`]) and the
//! sector AIs' queues are emptied each update.

use std::collections::BTreeMap;

use super::model::{SectorAi, SectorKind, TaskForce};
use super::sched::{self, AgentRef};
use super::{AiEvent, PlayerAi};
use crate::entity::flag;
use crate::world::World;

/// Where an entity's AI info (entity +0x10, `CAIEntityInfoEx`, made by 0x435370) points: the
/// task force it belongs to (info +8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TfRef {
    /// The player AI's reservoir 0..4 (+0x80, +0xd0, +0x120, +0x170, +0x1c0).
    Reservoir(u8, u8),
    /// The player AI's squad task force (+0x238).
    Squad(u8),
    /// Sector AI `s`'s slot `k` (+0x1c + 4k, by warrior type).
    Slot(u8, u8, u8),
    /// Sector AI `s`'s reservoir `k` (normal sector AIs: +0x8c0 …).
    Force(u8, u8, u8),
    /// The task force `uid` of sector AI `s`'s group (+0x58).
    Group(u8, u8, u32),
}

impl TfRef {
    pub fn player(self) -> u8 {
        match self {
            TfRef::Reservoir(p, _) | TfRef::Squad(p) | TfRef::Slot(p, ..) | TfRef::Force(p, ..) | TfRef::Group(p, ..) => p,
        }
    }
}

/// Entity → task force.
pub type Infos = BTreeMap<u16, TfRef>;

/// The rest of an entity's AI info (`CAIEntityInfoEx`, 0x435370): +2 a partner entity (a tower
/// and the swordsman sent to it, 0x441de0; read by the military buildings' soldier search
/// 0x582f70), +4 a tick (cleared when the entity joins a task force), and for a building its
/// `CAIEntityInfoTower` (+0x14: +8 own, +0xc allied, +0x10 other warriors' strength, 0x43f510).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InfoExt {
    pub partner: u16,
    pub tick: u32,
    pub tower: Option<[i32; 3]>,
}

fn player_mut(world: &mut World, p: u8) -> Option<&mut PlayerAi> {
    world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut())
}

pub(crate) fn tf_mut(world: &mut World, r: TfRef) -> Option<&mut TaskForce> {
    match r {
        TfRef::Reservoir(p, k) => player_mut(world, p)?.body.reservoirs.get_mut(k as usize),
        TfRef::Squad(p) => Some(&mut player_mut(world, p)?.body.squad),
        TfRef::Slot(p, s, k) => player_mut(world, p)?.body.sectors.get_mut(s as usize)?.slots.get_mut(k as usize)?.as_mut().map(|x| &mut x.1),
        TfRef::Force(p, s, k) => match &mut player_mut(world, p)?.body.sectors.get_mut(s as usize)?.kind {
            SectorKind::Normal { forces, .. } => forces.get_mut(k as usize),
            SectorKind::Dark { .. } => None,
        },
        TfRef::Group(p, s, uid) => player_mut(world, p)?.body.sectors.get_mut(s as usize)?.group.get_mut(uid).map(|i| &mut i.tf),
    }
}

/// The task force `r` points at.
pub fn tf(world: &World, r: TfRef) -> Option<&TaskForce> {
    let a = world.ai.players.get(r.player() as usize)?.as_ref()?;
    match r {
        TfRef::Reservoir(_, k) => a.body.reservoirs.get(k as usize),
        TfRef::Squad(_) => Some(&a.body.squad),
        TfRef::Slot(_, s, k) => a.body.sectors.get(s as usize)?.slots.get(k as usize)?.as_ref().map(|x| &x.1),
        TfRef::Force(_, s, k) => match &a.body.sectors.get(s as usize)?.kind {
            SectorKind::Normal { forces, .. } => forces.get(k as usize),
            SectorKind::Dark { .. } => None,
        },
        TfRef::Group(_, s, uid) => a.body.sectors.get(s as usize)?.group.get(uid).map(|i| &i.tf),
    }
}

/// 0x435370 `(entity, create)`: an entity may get an AI info when it exists and has flag
/// 0x1000000 or 0x2000000.
fn can_have_info(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| e.flags & 0x0300_0000 != 0)
}

/// `CAITaskForceReservoir` vtbl+0x14 (0x444ab0) with mode 0 (to the back): the entity leaves its
/// task force (vtbl+0x18 of that one, 0x444ba0) and joins `to`.
pub fn add_to(world: &mut World, id: u16, to: TfRef) {
    if !can_have_info(world, id) {
        return;
    }
    if let Some(&old) = world.ai.infos.get(&id) {
        if old == to {
            return;
        }
        if let Some(tf) = tf_mut(world, old) {
            tf.remove_member(id);
        }
    }
    if let Some(tf) = tf_mut(world, to) {
        tf.add_member(id, false);
        world.ai.infos.insert(id, to);
        if let Some(x) = world.ai.ext.get_mut(&id) {
            x.tick = 0;
        }
    } else {
        world.ai.infos.remove(&id);
    }
}

/// An entity is deleted: it leaves its task force (the AI info goes with the entity, 0x566d80).
pub fn on_delete(world: &mut World, id: u16) {
    world.ai.ext.remove(&id);
    if let Some(r) = world.ai.infos.remove(&id)
        && let Some(tf) = tf_mut(world, r)
    {
        tf.remove_member(id);
    }
}

/// The walking sector of a node (tile +4).
fn sector_at(world: &World, x: i32, y: i32) -> u16 {
    world.grid.checked_index(x, y).map_or(0, |n| world.tiling.sector_of(n))
}

/// 0x436020: from `(x, y)` along the whole spiral (table 0x1169968..0x117cf2c) the first node on
/// the map without flag 1.
fn free_node(world: &World, x: i32, y: i32) -> Option<(i32, i32)> {
    free_node_dist(world, x, y).map(|f| f.1)
}

/// 0x436020 with its result: the node's distance (`Grid::Distance`) and the node; `None` (-1)
/// when the spiral holds none.
pub(crate) fn free_node_dist(world: &World, x: i32, y: i32) -> Option<(i32, (i32, i32))> {
    let (nx, ny) = (0..crate::spiral::LEN)
        .map(|k| (x + crate::spiral::dx(k), y + crate::spiral::dy(k)))
        .find(|&(nx, ny)| world.grid.checked_index(nx, ny).is_some_and(|n| world.layers.flags[n] & 1 == 0))?;
    Some((crate::spiral::grid_distance(x - nx, y - ny), (nx, ny)))
}

/// The player's buildings of `kind` that have not died, in the order of `CBuildingMgr`'s
/// per-player list (0x14eea74 + 4 · (83 · player + kind), [`World::building_list`]).
fn building_list(world: &World, player: u8, kind: u16) -> Vec<u16> {
    let mut v = world.building_list(player as i32, kind as i32);
    v.retain(|&b| world.entities.by_index(b as u32).is_some_and(|e| e.flags & flag::DIED == 0));
    v
}

fn ensign(world: &World, id: u16) -> Option<(i32, i32)> {
    match &world.entities.by_index(id as u32)?.data {
        crate::entity::EntityData::Building(b) => Some((b.ensign.x(), b.ensign.y())),
        _ => None,
    }
}

/// The AI's settler types per tribe (0xff052c → 0x11633d8 …): soldiers, priests, thieves,
/// pioneers, geologists, gardeners. (Its vehicle types, 0xff0518, need the vehicle lists.)
fn settler_types(race: u8) -> &'static [u16] {
    match race {
        0 => &[29, 30, 31, 32, 33, 34, 35, 36, 37, 44, 45, 49, 47, 46],
        1 => &[29, 30, 31, 32, 33, 34, 38, 39, 40, 44, 45, 49, 47, 46],
        2 => &[29, 30, 31, 32, 33, 34, 41, 42, 43, 44, 45, 49, 47, 46],
        3 => &[52, 29, 32, 54],
        _ => &[29, 30, 31, 32, 33, 34, 61, 62, 63, 44, 45, 49, 47, 46],
    }
}

/// `Init` (vtbl+8) of a new player AI: `CAINormalPlayerAI` 0x43b5f0 = 0x43a7f0 then 0x439e70;
/// `CAIDarkTribePlayerAI` 0x43b3a0 = 0x43a7f0 then its one sector AI at the temple or fortress.
pub fn init(world: &mut World, player: u8) {
    let Some(dark) = world.ai.players.get(player as usize).and_then(|a| a.as_ref()).map(|a| a.dark) else { return };
    collect_units(world, player);
    if dark {
        dark_sector(world, player);
    } else {
        make_sectors(world, player);
    }
}

/// 0x43a7f0: the player's units of the AI's settler types (in each type's list order), then its
/// vehicles of the AI's vehicle types (+0x18: 0xff0518 → .rdata 0x1163334, types 1..5; the Dark
/// Tribe's table 0x1266b1c is [inf] empty), each list from `CVehicleMgr`'s head 0x1549bd0, that
/// are READY, not flagged 0x8000 and in no task force join reservoir 0.
fn collect_units(world: &mut World, player: u8) {
    let race = world.race_of(player);
    let join = |world: &mut World, mut id: u16| {
        while id != 0 {
            let Some(e) = world.entities.by_index(id as u32) else { break };
            let next = e.next;
            if e.flags & 0x0200_8000 == 0x0200_0000 && !world.ai.infos.contains_key(&id) {
                add_to(world, id, TfRef::Reservoir(player, 0));
            }
            id = next;
        }
    };
    for &kind in settler_types(race) {
        let head = world.settler_lists.head(player, kind);
        join(world, head);
    }
    if race != 3 {
        for kind in 1..=5 {
            let head = world.vehicles.head(player, kind);
            join(world, head);
        }
    }
}

/// 0x439e70: at most 8 sector AIs; for the player's castles (48), then large (47) and small
/// (46) towers, one per walking sector of their ensign.
fn make_sectors(world: &mut World, player: u8) {
    let mut have: Vec<u16> = world.ai.players[player as usize].as_ref().map_or(Vec::new(), |a| a.body.sectors.iter().map(|s| s.sector()).collect());
    if have.len() >= 8 {
        return;
    }
    for kind in [48u16, 47, 46] {
        for b in building_list(world, player, kind) {
            let Some((x, y)) = ensign(world, b) else { continue };
            let sector = sector_at(world, x, y);
            if have.contains(&sector) {
                continue;
            }
            have.push(sector);
            let at = free_node(world, x, y).unwrap_or((x, y));
            let index = at.1 * world.grid.size() as i32 + at.0;
            let s = SectorAi::new_normal(player, (x, y), at, index, sector_at(world, at.0, at.1));
            let Some(a) = world.ai.players[player as usize].as_mut() else { return };
            a.body.sectors.push(s);
            let k = a.body.sectors.len() - 1;
            let full = a.body.sectors.len() > 7;
            // The ctor 0x442180: the attack agent's reset (0x42f260, one RNG draw), the agents
            // registered, the task forces made, the eco-sector AIs (0x4439f0).
            super::attack::reset(world, player, k as u8);
            let Some(a) = world.ai.players[player as usize].as_mut() else { return };
            sched::register_sector(&mut a.sched, &mut a.body, k);
            super::taskforce::register_sector(&mut world.ai, player, k as u8, false);
            super::sector::make_eco(world, player, k as u8);
            if full {
                return;
            }
        }
    }
}

/// 0x43b3a0: the player's first dark temple (50; +0x2ac = 0x32) or else dark fortress (51,
/// 0x33) becomes +0x2a8 and gets the one `CAIDarkTribeSectorAI`.
fn dark_sector(world: &mut World, player: u8) {
    let b = building_list(world, player, 50).first().copied().or_else(|| building_list(world, player, 51).first().copied());
    let Some(a) = world.ai.players[player as usize].as_mut() else { return };
    a.body.dark = Some((a.body.dark.map_or(0, |d| d.0), b.unwrap_or(0) as i32));
    let Some(b) = b else { return };
    let Some(e) = world.entities.by_index(b as u32) else { return };
    let pos = (e.x(), e.y());
    let Some(en) = ensign(world, b) else { return };
    let index = en.1 * world.grid.size() as i32 + en.0;
    let s = SectorAi::new_dark(player, b, pos, en, index, sector_at(world, en.0, en.1));
    // The ctor's `CAIDarkTribe` (0x44b1c0).
    let plan = Box::new(super::dark::DarkTribe::new(world, player));
    if let Some(a) = world.ai.players[player as usize].as_mut() {
        a.body.sectors.push(s);
        a.dark_ai = Some(plan);
        let k = a.body.sectors.len() - 1;
        sched::register_sector(&mut a.sched, &mut a.body, k);
        super::taskforce::register_sector(&mut world.ai, player, k as u8, true);
    }
}

/// `CAIPlayerAI` vtbl+4: `CAINormalPlayerAI` 0x43aac0, `CAIDarkTribePlayerAI` 0x43b270.
pub fn update(world: &mut World, player: u8) {
    let Some(a) = world.ai.players.get(player as usize).and_then(|a| a.as_ref()) else { return };
    if a.dark {
        // 0x43b270: without its temple / fortress (gone, not READY or not its own) the queue is
        // dropped and nothing else happens.
        let b = a.body.dark.map_or(0, |d| d.1) as u16;
        let ok = world.entities.by_index(b as u32).is_some_and(|e| e.flags & 0x0300_0000 != 0 && e.owner == player);
        if !ok {
            if let Some(a) = world.ai.players[player as usize].as_mut() {
                a.queue = Default::default();
            }
            return;
        }
        extra_mana(world, player);
    }
    let n = world.ai.players[player as usize].as_ref().map_or(0, |a| a.body.sectors.len());
    for s in 0..n {
        anchor(world, player, s);
    }
    dispatch(world, player);
    distribute(world, player);
    let now = world.ai.tick;
    run_scheduler(world, Some(player), now, sched::BUDGET);
    for s in 0..n {
        sector_update(world, player, s);
    }
}

/// 0x43b2c0..0x43b33e: with `DTExtraManaBaseAmount` or `DTExtraManaAmountPerFarm2` above 0 and
/// `DTExtraManaDelay` above 0, every `DTExtraManaDelay · 14` ticks (AI tick, unsigned remainder)
/// while the mana is below `DTExtraManaLimit`, the Dark Tribe gets `base + per farm ·
/// ⌊√finished mushroom farms⌋` mana (0x436380; the farms of `CBuildingMgr` 0x14ef7ac + 0x298·p,
/// the root by the CRT `sqrt` 0xea3580, truncated).
fn extra_mana(world: &mut World, player: u8) {
    let v = |k| world.ai.vars.get(player, k);
    let (base, per_farm, delay, limit) = (v(14), v(15), v(16), v(17));
    if (base <= 0 && per_farm <= 0) || delay <= 0 {
        return;
    }
    if !world.ai.tick.is_multiple_of((delay as u32).wrapping_mul(14)) || crate::magic::mana(world, player) >= limit {
        return;
    }
    let farms = world.statistic.players.get(player as usize).map_or(0, |s| s.count[0x31]);
    // The CRT `sqrt` of a non-negative count truncated: the integer square root.
    let n = (farms.max(0) as u32).isqrt() as i32;
    crate::magic::add_mana(world, player, n.wrapping_mul(per_farm).wrapping_add(base));
}

/// 0x43f430: a sector AI whose anchor node lost its sector goes back to its base node, or the
/// nearest free node from there (0x436020).
fn anchor(world: &mut World, player: u8, s: usize) {
    let Some(sec) = world.ai.players[player as usize].as_ref().and_then(|a| a.body.sectors.get(s)) else { return };
    let size = world.grid.size() as i32;
    let index = sec.raw[6] as i32;
    let cur = if index >= 0 { sector_at(world, index % size, index / size) } else { 0 };
    let (bx, by) = (sec.raw[1] as i32, sec.raw[2] as i32);
    let (at, sector) = if cur != 0 {
        ((sec.raw[4] as i32, sec.raw[5] as i32), cur)
    } else if world.grid.checked_index(bx, by).is_some_and(|n| world.layers.flags[n] & 1 == 0) {
        ((bx, by), sector_at(world, bx, by))
    } else {
        let at = free_node(world, bx, by).unwrap_or((bx, by));
        (at, sector_at(world, at.0, at.1))
    };
    if let Some(sec) = world.ai.players[player as usize].as_mut().and_then(|a| a.body.sectors.get_mut(s)) {
        sec.raw[4] = at.0 as u32;
        sec.raw[5] = at.1 as u32;
        sec.raw[6] = (at.1 * size + at.0) as u32;
        sec.raw[7] = sector as u32;
    }
}

fn warrior_type(world: &World, id: u16) -> u32 {
    world.entities.by_index(id as u32).map_or(0, |e| e.flags & 0xf)
}

fn entity_sector(world: &World, id: u16) -> u16 {
    world.entities.by_index(id as u32).map_or(0, |e| sector_at(world, e.x(), e.y()))
}

fn sector_index(world: &World, player: u8, sector: u16) -> Option<usize> {
    world.ai.players[player as usize].as_ref()?.body.sectors.iter().position(|s| s.sector() == sector)
}

/// 0x43a170 `(entity, warrior type, sector)`: into slot `warrior type` of the sector AI on
/// that sector.
fn to_sector_slot(world: &mut World, player: u8, id: u16, wt: u32, sector: u16) {
    let Some(s) = sector_index(world, player, sector) else { return };
    let has_slot = wt != 0 && world.ai.players[player as usize].as_ref().is_some_and(|a| a.body.sectors[s].slots.get(wt as usize).is_some_and(|k| k.is_some()));
    if has_slot {
        add_to(world, id, TfRef::Slot(player, s as u8, wt as u8));
    }
}

/// 0x43a420: the events queued for the player AI, from the read index to the end; then the
/// queue is empty. Per type (`a` is an entity unless said otherwise):
/// * 0x12: a warrior joins reservoir 3; 0x13 (own player) and 0x15 / 0x16: reservoir 0.
/// * 0x14 (a new unit, `b` how it came): `b` 1 on a sector with a sector AI and other `b` (not 2)
///   on a sector go to the sector AIs of that sector; `b` 1 on a sector without one: reservoir 0
///   (0x444b70); otherwise reservoir 0. These are not passed on.
/// * 0x19: warrior types 10 / 11 join reservoir 1 (`b` ≠ 1) or 2; others the sector AI's slot.
/// * 0x1c (`AI.NewSquad`, normal player AIs only): commands 8 / 13 send reservoir 2's units to
///   `b` (0x43a1e0); others hand reservoir 4 out to the sector AIs (0x43a360) and start the
///   global suicide mission (0x42b820, S3) and are then passed on to every sector AI (their
///   `new_squad`); 8 / 13 and the Dark Tribe's are not passed on.
/// * 0x1d: passed on only by the Dark Tribe.
/// * Everything else (and the above unless said otherwise) goes to every sector AI
///   (vtbl+0x1c).
pub fn dispatch(world: &mut World, player: u8) {
    let Some(a) = world.ai.players.get_mut(player as usize).and_then(|a| a.as_mut()) else { return };
    let start = a.queue.read as usize;
    let events: Vec<AiEvent> = a.queue.events.get(start..).map_or(Vec::new(), |e| e.to_vec());
    let dark = a.dark;
    let sectors = a.body.sectors.len();
    for ev in events {
        let id = ev.a as u16;
        // `None`: not passed on; `Some(0)`: to every sector AI; `Some(s)`: those on sector `s`.
        let forward: Option<u16> = match ev.kind {
            0x12 => {
                if warrior_type(world, id) != 0 {
                    add_to(world, id, TfRef::Reservoir(player, 3));
                }
                Some(0)
            }
            0x13 | 0x15 | 0x16 => {
                if (ev.kind != 0x13 || ev.player == player as i32) && warrior_type(world, id) != 0 {
                    add_to(world, id, TfRef::Reservoir(player, 0));
                }
                Some(0)
            }
            0x14 => {
                let sector = entity_sector(world, id);
                let with_ai = sector != 0 && sector_index(world, player, sector).is_some();
                if sector != 0 && ((ev.b == 1 && with_ai) || (ev.b != 1 && ev.b != 2)) {
                    Some(sector)
                } else {
                    add_to(world, id, TfRef::Reservoir(player, 0));
                    None
                }
            }
            0x19 => {
                let wt = warrior_type(world, id);
                let sector = entity_sector(world, id);
                if wt == 0xb || wt == 0xa {
                    add_to(world, id, TfRef::Reservoir(player, if ev.b != 1 { 1 } else { 2 }));
                } else if wt != 0 && sector != 0 {
                    to_sector_slot(world, player, id, wt, sector);
                }
                Some(0)
            }
            0x1c => {
                if !dark {
                    if ev.a == 8 || ev.a == 0xd {
                        send_squad(world, player, ev.b & 0xffff, ev.b >> 16, ev.a == 8);
                    } else {
                        hand_out(world, player);
                        // 0x444d60: with a first unit in the squad task force, the global
                        // suicide mission is due now (0x42b820).
                        if !world.ai.players[player as usize].as_ref().is_some_and(|a| a.body.squad.members.is_empty()) {
                            let now = world.ai.tick;
                            if let Some(a) = world.ai.players[player as usize].as_mut()
                                && a.sched.is_registered(AgentRef::GlobalSuicide)
                                && now < a.body.suicide.next
                            {
                                a.sched.reschedule(&mut a.body, AgentRef::GlobalSuicide, now);
                            }
                        }
                    }
                }
                // 0x43a420: commands 8 / 0xd and the Dark Tribe are not passed on (goto 0x43a4f0);
                // any other command (7 MOVE_HOME, 9 SUICIDE) breaks into the forward loop with
                // sector 0: every sector AI's vtbl+0x1c (`sector::new_squad`).
                (!dark && ev.a != 8 && ev.a != 0xd).then_some(0)
            }
            0x1d => dark.then_some(0),
            _ => Some(0),
        };
        if let Some(f) = forward {
            for s in 0..sectors {
                let on = world.ai.players[player as usize].as_ref().and_then(|a| a.body.sectors.get(s)).map(|x| x.sector());
                if on.is_some_and(|on| f == 0 || on == f) {
                    // vtbl+0x1c: the normal sector AI queues it for itself and its eco-sector
                    // AIs (0x4434a0).
                    super::sector::post(world, player, s as u8, ev);
                }
            }
        }
    }
    if let Some(a) = world.ai.players[player as usize].as_mut() {
        a.queue = Default::default();
    }
}

/// 0x43a1e0 `(x, y, vanish)`: every unit of reservoir 2 gets the move order (event 0x11 kind
/// 0xd to `(x, y)`, data0 0) and leaves the reservoir; with `vanish` (`AI.NewSquad` 8) a ship or
/// land vehicle also gets the event of .rdata 0x10710a0: 0x11 kind 0xd with data0 0x12, data1 0.
fn send_squad(world: &mut World, player: u8, x: i32, y: i32, vanish: bool) {
    let members = world.ai.players[player as usize].as_ref().map_or(Vec::new(), |a| a.body.reservoirs[2].members.clone());
    for id in members {
        let place = crate::grid::Y16X16::pack(x, y).0 as i32;
        super::taskforce::send_event(world, id, crate::jobs::state::EntityEvent { event: 0x11, kind: 0xd, data: [0, place, 0] });
        let vehicle = world.entities.by_index(id as u32).is_some_and(|e| matches!(e.obj_type, crate::entity::ObjType::Ship | crate::entity::ObjType::LandVehicle));
        if vanish && vehicle {
            super::taskforce::send_event(world, id, crate::jobs::state::EntityEvent { event: 0x11, kind: 0xd, data: [0x12, 0, 0] });
        }
        if let Some(tf) = tf_mut(world, TfRef::Reservoir(player, 2)) {
            tf.remove_member(id);
        }
        world.ai.infos.remove(&id);
    }
}

/// 0x43a360: reservoir 4's units go to the sector AI on their sector (slot by warrior type),
/// or else to the squad task force.
fn hand_out(world: &mut World, player: u8) {
    let members = world.ai.players[player as usize].as_ref().map_or(Vec::new(), |a| a.body.reservoirs[4].members.clone());
    for id in members {
        let sector = entity_sector(world, id);
        if sector_index(world, player, sector).is_some() {
            to_sector_slot(world, player, id, warrior_type(world, id), sector);
        } else {
            add_to(world, id, TfRef::Squad(player));
        }
    }
}

/// 0x43a9f0: reservoir 0's units on a sector with a sector AI go to its slot for their warrior
/// type.
fn distribute(world: &mut World, player: u8) {
    let members = world.ai.players[player as usize].as_ref().map_or(Vec::new(), |a| a.body.reservoirs[0].members.clone());
    for id in members {
        let sector = entity_sector(world, id);
        if sector_index(world, player, sector).is_some() {
            to_sector_slot(world, player, id, warrior_type(world, id), sector);
        }
    }
}

/// The sector AI's update: `CAINormalSectorAI` 0x443b50 handles its queue (0x43f2f0, each event
/// by vtbl+0x28) and runs its eco-sector AIs ([`super::sector::update`]); `CAIDarkTribeSectorAI`
/// 0x4411c0 handles its queue (0x4411e0) and runs its `CAIDarkTribe` ([`super::dark::execute`]).
/// The Dark Tribe's events: 0x14 (a new unit `a`) puts the unit into the slot of its warrior type
/// (when the sector AI has one); 0x1d (dark land changed at `(player, a)`, dark when `b` ≠ 0) goes
/// to the plan; 0x1b (`AI.AttackNow`) to the dark attack's 0x42ca80.
fn sector_update(world: &mut World, player: u8, s: usize) {
    let Some(a) = world.ai.players[player as usize].as_mut() else { return };
    let dark = a.dark;
    if !dark {
        super::sector::update(world, player, s as u8);
        return;
    }
    let Some(a) = world.ai.players[player as usize].as_mut() else { return };
    let Some(sec) = a.body.sectors.get_mut(s) else { return };
    let queue = std::mem::take(&mut sec.queue);
    for ev in queue.events.iter().skip(queue.read as usize) {
        match ev.kind {
            // A new unit joins the slot of its warrior type.
            0x14 => {
                let wt = warrior_type(world, ev.a as u16);
                let has = world.ai.players[player as usize].as_ref().is_some_and(|a| a.body.sectors.get(s).is_some_and(|x| x.slots.get(wt as usize).is_some_and(|k| k.is_some())));
                if wt != 0 && has {
                    add_to(world, ev.a as u16, TfRef::Slot(player, s as u8, wt as u8));
                }
            }
            0x1d => super::dark::land_changed(world, player, ev.player, ev.a, ev.b != 0),
            // `AI.AttackNow` (0x4411e0 → the attack agent's 0x42ca80).
            0x1b => {
                let players = if ev.a < 1 { 0 } else { crate::alliances::player_bit(ev.a.min(16) as u8) as u32 };
                super::dark_attack::attack_now(world, player, s as u8, players, ev.b);
            }
            _ => {}
        }
    }
    super::dark::execute(world, player);
}

/// `CAIScheduler::Update(now, budget)` (vtbl+4, 0x42e9b0) of `CAIMain` (`owner` `None`) or of
/// player AI `owner`: while the first agent is due (`now` ≥ its next tick) and the budget covers
/// its cost (+0xc), it runs (vtbl+4 `Execute(now, budget)`), goes back into the list (see
/// [`sched::Scheduler::after_run`]) and its cost leaves the budget (not below 0). With the default
/// cost 0x100 and the budget 0x1ff, one agent that returns no cost of its own uses up a tick.
pub fn run_scheduler(world: &mut World, owner: Option<u8>, now: u32, budget: u32) {
    let mut budget = budget;
    loop {
        let r = match owner {
            None => world.ai.sched.due(&world.ai.evaluation, now, budget),
            Some(p) => world.ai.players.get(p as usize).and_then(|a| a.as_ref()).and_then(|a| a.sched.due(&a.body, now, budget)),
        };
        let Some(r) = r else { break };
        let ret = execute(world, owner, r, now, budget);
        let cost = match owner {
            None => {
                let ai = &mut world.ai;
                ai.sched.after_run(&mut ai.evaluation, r, now, ret)
            }
            Some(p) => match world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut()) {
                Some(a) => a.sched.after_run(&mut a.body, r, now, ret),
                None => return,
            },
        };
        budget = budget.saturating_sub(cost);
    }
    match owner {
        None => {
            let ai = &mut world.ai;
            ai.sched.finish(&ai.evaluation, now);
        }
        Some(p) => {
            if let Some(a) = world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut()) {
                a.sched.finish(&a.body, now);
            }
        }
    }
}

/// The value an agent's `Execute` returns when the agent does nothing: 0, "again after the
/// period, at the default cost" (also `CAIAgentGardeners` 0x431740 and `CAIAgentThieves`
/// 0x432910, whose `Execute` is `return 0`).
const UNPORTED: u32 = 0;

/// An agent's `Execute(now, budget)` (vtbl+4): bits 0..11 the cost it used, the rest the delay to
/// its next run (0: the scheduler's defaults).
fn execute(world: &mut World, owner: Option<u8>, r: AgentRef, now: u32, _budget: u32) -> u32 {
    let Some(p) = owner else {
        // `CAIAgentEvaluation` (0x430fb0): the next player (+0x24, 1..=player count) is
        // evaluated (0x4378b0).
        world.ai.evaluated += 1;
        if world.ai.evaluated > world.player_count() as i32 {
            world.ai.evaluated = 1;
        }
        super::evaluation::evaluate(world, world.ai.evaluated as u8);
        return 0;
    };
    let dark = world.ai.players.get(p as usize).and_then(|a| a.as_ref()).is_some_and(|a| a.dark);
    match r {
        // `CAIAgentGardeners` 0x431740 and `CAIAgentThieves` 0x432910: `return 0`.
        AgentRef::Sector(_, 4 | 11) if !dark => 0,
        // `CAIAgentEvalOffence` 0x431700: the sector AI's 0x442050, then 0.
        AgentRef::Sector(s, 2) if !dark => {
            super::sector::eval_offence(world, p, s);
            0
        }
        // `CAIAgentEvalDefence` 0x4315c0: the sector AI's 0x441eb0, then 0.
        AgentRef::Sector(s, 1) if !dark => {
            super::sector::eval_defence(world, p, s);
            0
        }
        // `CAIAgentDarkTribeManakopters` 0x4402d0: `return 1`.
        AgentRef::Sector(_, 6) if dark => 1,
        // `CAIAgentDarkTribeMushroomFarms` 0x430b00, with the sector AI's anchor building (+0xf0).
        AgentRef::Sector(s, 2) if dark => super::dark::mushroom_farms(world, p, sector_anchor(world, p, s), now),
        // `CAIAgentDarkTribeRecruitment` 0x430070.
        AgentRef::Sector(s, 3) if dark => super::dark_agents::recruitment(world, p, sector_anchor(world, p, s)),
        // `CAIAgentDarkTribeDarkGardeners` 0x430930, `…Shamans` 0x430820, `…SquadManager` 0x4305d0.
        AgentRef::Sector(s, 1) if dark => super::dark_agents::gardeners(world, p, s, now),
        AgentRef::Sector(s, 4) if dark => super::dark_agents::shamans(world, p, s, now),
        AgentRef::Sector(s, 5) if dark => super::dark_agents::squad_manager(world, p, s, now),
        // `CAIAgentRecruitment` 0x432cd0 (+0x28: the Roman specialists' count the squad agent
        // sets).
        AgentRef::Sector(s, 8) if !dark => {
            let extra = super::sched::Agents::get(&world.ai.players[p as usize].as_ref().expect("AI").body, r).map_or(0, |a| a.state);
            super::normal_agents::recruitment(world, p, s, extra)
        }
        // `CAIAgentAttack` 0x42faf0.
        AgentRef::Sector(s, 0) if !dark => super::attack::execute(world, p, s),
        // `CAIAgentGeologists` 0x431750, `CAIAgentIntruderCombat` 0x431bd0, `CAIAgentPioneers`
        // 0x432620.
        AgentRef::Sector(s, 3) if !dark => super::normal_agents::geologists(world, p, s, now),
        AgentRef::Sector(s, 5) if !dark => super::squads::intruders(world, p, s),
        AgentRef::Sector(s, 7) if !dark => super::normal_agents::pioneers(world, p, s, now),
        // `CAIAgentGlobalSuicideMission` 0x433210 (normal player AIs; the Dark Tribe's has the
        // same class).
        AgentRef::GlobalSuicide => super::attack::global_suicide(world, p, now),
        // `CAIAgentMovingHome` 0x432a80, `CAIAgentSuicideMissions` 0x4329d0, `CAIAgentSquads`
        // 0x4339d0, `CAIAgentTowerOccupation` 0x432930.
        AgentRef::Sector(s, 6) if !dark => super::squads::moving_home(world, p, s),
        AgentRef::Sector(s, 9) if !dark => super::squads::suicide_missions(world, p, s, now),
        AgentRef::Sector(s, 10) if !dark => super::squads::squads(world, p, s),
        AgentRef::Sector(s, 12) if !dark => super::squads::towers(world, p, s),
        // `CAIAgentDarkTribeAttack` 0x42cf00.
        AgentRef::Sector(s, 0) if dark => super::dark_attack::execute(world, p, s),
        // `CAIAgentDarkTribeManakopterHalls` 0x430c20.
        AgentRef::Sector(s, 7) if dark => super::dark_agents::manakopter_halls(world, p, s, now),
        _ => UNPORTED,
    }
}

/// 0x42b820 `(0)`: a registered agent with a next tick above 0 is due at once (0x42e950).
pub(crate) fn run_agent_now(world: &mut World, p: u8, s: u8, k: u8) {
    let Some(a) = world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut()) else { return };
    let r = AgentRef::Sector(s, k);
    if super::sched::Agents::get(&a.body, r).is_some_and(|x| x.next > 0) {
        a.sched.reschedule(&mut a.body, r, 0);
    }
}

/// [`free_node_dist`]'s node.
pub(crate) fn free_node_pub(world: &World, x: i32, y: i32) -> Option<(i32, i32)> {
    free_node(world, x, y)
}

/// The sector AI's anchor building (+0xf0; the Dark Tribe's temple or fortress).
fn sector_anchor(world: &World, player: u8, s: u8) -> u16 {
    world.ai.players[player as usize].as_ref().and_then(|a| a.body.sectors.get(s as usize)).map_or(0, |sec| sec.raw[3] as u16)
}

/// The player AI as a whole (for tests).
pub fn get(world: &World, player: u8) -> Option<&PlayerAi> {
    world.ai.players.get(player as usize)?.as_ref()
}
