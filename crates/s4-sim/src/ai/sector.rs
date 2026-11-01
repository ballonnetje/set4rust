//! The normal sector AI at run time (`CAINormalSectorAI`, vtable 0x102b218, ctor 0x442180; stage
//! S2): its event handler (vtbl+0x28, 0x443520), its update (0x443b50), the eco-sector AIs it
//! keeps ([`super::eco`]), its goal caches, the two building cursors and the two evaluations
//! (defence 0x441eb0, offence 0x442050). Own reverse engineering of the shipped exe; decompiles
//! in `tools/re/decompiled/script/ai/s2/` (`sector.c`, `core.c`, `more1.c`, `more2.c`). No AI save
//! exists to check against; choices the code alone does not settle are marked [inf].
//!
//! Goal caches (+0x104 + 0x18c·k, [`super::model::GoalCache`]):
//! * 0, 1: the player's own military buildings on the sector, by threat and by exposure (defence);
//! * 2, 3, 4: enemy military buildings on the sector, by path length to the anchor, by weakness
//!   and by the land around them (offence). Cache 3 is the attack agent's target list.

use std::collections::BTreeMap;

use super::model::{Cursor, GoalCache, SectorKind};
use super::sched::AgentRef;
use super::AiEvent;
use crate::World;
use crate::alliances::player_bit;
use crate::entity::{EntityData, flag};

/// The goal caches.
pub mod cache {
    /// Own buildings by threat: `(enemy strength · 4 − own strength) << 12 + min(enemy, 0xfff)`.
    pub const THREAT: usize = 0;
    /// Own buildings by exposure: `enemy squares · 4 − own squares` (0x435d10).
    pub const EXPOSED: usize = 1;
    /// Enemy buildings by the negated path length from them to the anchor.
    pub const NEAR: usize = 2;
    /// Enemy buildings by the negated enemy strength around them.
    pub const WEAK: usize = 3;
    /// Enemy buildings by the land value of their 16×16 cell (0x43b680).
    pub const LAND: usize = 4;
}

/// The normal sector AI's agent slots (+0xaf0 …, [`super::model::NORMAL_AGENTS`]).
pub mod agent {
    pub const ATTACK: u8 = 0;
    pub const DEFENCE: u8 = 1;
    pub const OFFENCE: u8 = 2;
    pub const GEOLOGISTS: u8 = 3;
    pub const GARDENERS: u8 = 4;
    pub const INTRUDERS: u8 = 5;
    pub const MOVING_HOME: u8 = 6;
    pub const PIONEERS: u8 = 7;
    pub const RECRUITMENT: u8 = 8;
    pub const SUICIDE: u8 = 9;
    pub const SQUADS: u8 = 10;
    pub const THIEVES: u8 = 11;
    pub const TOWERS: u8 = 12;
}

/// The normal sector AI's reservoirs (+0x8c0 + 0x50·k).
pub mod force {
    /// +0xa00: swordsmen taken from the slots to occupy towers (0x441de0).
    pub const OCCUPY: u8 = 4;
    /// +0x9b0: the units of event 0x14 (new units on the sector).
    pub const NEW: u8 = 3;
}

/// Military buildings: small tower, large tower, castle.
const FIRST_TOWER: u16 = 46;
const LAST_TOWER: u16 = 48;

fn sec(world: &World, p: u8, s: u8) -> Option<&super::model::SectorAi> {
    world.ai.players.get(p as usize)?.as_ref()?.body.sectors.get(s as usize)
}

fn sec_mut(world: &mut World, p: u8, s: u8) -> Option<&mut super::model::SectorAi> {
    world.ai.players.get_mut(p as usize)?.as_mut()?.body.sectors.get_mut(s as usize)
}

/// The sector AI's walking sector (+0x100).
pub fn sector_of(world: &World, p: u8, s: u8) -> u16 {
    sec(world, p, s).map_or(0, |x| x.sector())
}

/// The anchor (+0xf4 / +0xf8).
pub fn anchor(world: &World, p: u8, s: u8) -> (i32, i32) {
    sec(world, p, s).map_or((0, 0), |x| x.anchor())
}

pub fn caches(world: &World, p: u8, s: u8) -> Option<&Vec<GoalCache>> {
    match &sec(world, p, s)?.kind {
        SectorKind::Normal { caches, .. } => Some(caches),
        SectorKind::Dark { .. } => None,
    }
}

pub fn caches_mut(world: &mut World, p: u8, s: u8) -> Option<&mut Vec<GoalCache>> {
    match &mut sec_mut(world, p, s)?.kind {
        SectorKind::Normal { caches, .. } => Some(caches),
        SectorKind::Dark { .. } => None,
    }
}

/// Eco-sector AI `k` of the sector AI.
pub fn eco_mut(world: &mut World, p: u8, s: u8, k: usize) -> Option<&mut super::eco::EcoSectorAi> {
    match &mut sec_mut(world, p, s)?.kind {
        SectorKind::Normal { eco, .. } => eco.get_mut(k),
        SectorKind::Dark { .. } => None,
    }
}

fn eco_list(world: &World, p: u8, s: u8) -> Vec<(u16, u32, u32)> {
    match sec(world, p, s).map(|x| &x.kind) {
        Some(SectorKind::Normal { eco, .. }) => eco.iter().map(|e| (e.eco, e.uid, e.made)).collect(),
        _ => Vec::new(),
    }
}

/// The walking sector of a node (tile +4), 0 off the map.
pub(crate) fn sector_at(world: &World, x: i32, y: i32) -> u16 {
    world.grid.checked_index(x, y).map_or(0, |n| world.tiling.sector_of(n))
}


/// A building's ensign (+0x48), `None` for anything else (0x431100 / 0x435980 give −1).
pub(crate) fn ensign(world: &World, id: u16) -> Option<crate::grid::Y16X16> {
    match &world.entities.by_index(id as u32)?.data {
        EntityData::Building(b) => Some(b.ensign),
        _ => None,
    }
}

/// An entity's unique id (+4), 0 when it is gone.
pub(crate) fn uid_of(world: &World, id: i32) -> u32 {
    if id <= 0 {
        return 0;
    }
    world.entities.by_index(id as u32).map_or(0, |e| e.uid)
}

/// 0x435320: alive (flags 0x3000000) and of warrior type 0xc (a building).
pub(crate) fn building_alive(world: &World, id: i32) -> bool {
    id > 0 && world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0 && e.flags & 0xf == 0xc)
}

/// 0x438c30 for an entity: added when it is above 0, exists and has a unique id above 0.
pub(crate) fn cache_add(world: &mut World, p: u8, s: u8, k: usize, id: i32, value: i32) {
    let uid = uid_of(world, id);
    if id <= 0 || uid == 0 {
        // 0x438bb0 runs first all the same.
        if let Some(c) = caches_mut(world, p, s).and_then(|c| c.get_mut(k)) {
            c.remove(id);
        }
        return;
    }
    if let Some(c) = caches_mut(world, p, s).and_then(|c| c.get_mut(k)) {
        c.add(id, uid, value);
    }
}

/// 0x438d10 on cache `k`.
pub(crate) fn cache_clean(world: &mut World, p: u8, s: u8, k: usize) {
    let now = world.ai.tick;
    let Some(c) = caches(world, p, s).and_then(|c| c.get(k)) else { return };
    let mut c = c.clone();
    c.clean(|id| uid_of(world, id), now);
    if let Some(slot) = caches_mut(world, p, s).and_then(|v| v.get_mut(k)) {
        *slot = c;
    }
}

// ---- building cursors ---------------------------------------------------------------------

/// `CBuildingMgr`'s lists (heads 0x14eea74 + 4 · (83 · player + type), linked through entity
/// +0x2a, [`World::building_list`]) of the military types for the players of a mask, copied once
/// per use.
pub(crate) struct Lists {
    lists: BTreeMap<(u8, u16), Vec<u16>>,
}

impl Lists {
    pub(crate) fn new(world: &World, mask: u16) -> Lists {
        let mut lists: BTreeMap<(u8, u16), Vec<u16>> = BTreeMap::new();
        for p in (1..=8u8).filter(|&p| mask & player_bit(p) != 0) {
            for t in FIRST_TOWER..=LAST_TOWER {
                let v = crate::buildings::list(world, p, t);
                if !v.is_empty() {
                    lists.insert((p, t), v.to_vec());
                }
            }
        }
        Lists { lists }
    }

    fn head(&self, p: u8, t: u16) -> u16 {
        self.lists.get(&(p, t)).and_then(|v| v.first()).copied().unwrap_or(0)
    }

    /// Entity +0x2a: the next in its list (0 at the end, or [inf] when it left the list).
    fn next(&self, p: u8, t: u16, id: u16) -> u16 {
        let Some(v) = self.lists.get(&(p, t)) else { return 0 };
        v.iter().position(|&x| x == id).and_then(|k| v.get(k + 1)).copied().unwrap_or(0)
    }
}

/// 0x4392a0 `(mask)`: the lowest and the highest player of the mask (0 none).
fn players_of(mask: u16) -> (u8, u8) {
    let first = (1..=8u8).find(|&p| mask & player_bit(p) != 0).unwrap_or(0);
    let last = (1..=8u8).rev().find(|&p| mask & player_bit(p) != 0).unwrap_or(0);
    (first, last)
}

/// 0x439470: the head of the first player's first type; an empty list goes on with `next`.
pub(crate) fn cursor_first(c: &mut Cursor, mask: u16, l: &Lists, world: &World) -> u16 {
    let (first, _) = players_of(mask);
    c.player = first;
    if first == 0 {
        c.kind = 0;
        c.entity = 0;
        return 0;
    }
    c.kind = FIRST_TOWER;
    c.entity = l.head(first, FIRST_TOWER);
    if c.entity == 0 {
        return cursor_next(c, mask, l, world);
    }
    c.entity
}

/// 0x4394c0: the next entity of the list, else the next type's head, else the next player's
/// (in the mask) first type; entities with flag 0x4000000 (died) are passed over. At the end the
/// player goes back to 0 and the result is 0.
pub(crate) fn cursor_next(c: &mut Cursor, mask: u16, l: &Lists, world: &World) -> u16 {
    let (_, last) = players_of(mask);
    loop {
        if c.entity != 0 {
            c.entity = l.next(c.player, c.kind, c.entity);
        }
        if c.entity == 0 {
            loop {
                c.kind += 1;
                if c.kind > LAST_TOWER {
                    loop {
                        if c.player >= last {
                            c.player = 0;
                            return 0;
                        }
                        c.player += 1;
                        if mask & player_bit(c.player) != 0 {
                            break;
                        }
                    }
                    c.kind = FIRST_TOWER;
                }
                c.entity = l.head(c.player, c.kind);
                if c.entity != 0 {
                    break;
                }
            }
        }
        let died = world.entities.by_index(c.entity as u32).is_some_and(|e| e.flags & flag::DIED != 0);
        if c.entity == 0 || !died {
            return c.entity;
        }
    }
}

/// The cursors' masks: 0 the player's own bit (+0xd4c), 1 its enemies (+0xd70, 0x12dcd54).
pub(crate) fn cursor_mask(world: &World, p: u8, which: usize) -> u16 {
    if which == 0 { player_bit(p) } else { world.alliances.enemy_bits[p.min(8) as usize] }
}

fn cursor(world: &World, p: u8, s: u8, which: usize) -> Cursor {
    match sec(world, p, s).map(|x| &x.kind) {
        Some(SectorKind::Normal { cursors, .. }) => cursors[which],
        _ => Cursor::default(),
    }
}

fn set_cursor(world: &mut World, p: u8, s: u8, which: usize, c: Cursor) {
    if let Some(SectorKind::Normal { cursors, .. }) = sec_mut(world, p, s).map(|x| &mut x.kind) {
        cursors[which] = c;
    }
}

// ---- the AI infos beyond the task force ---------------------------------------------------

/// 0x435370 `(entity, create)` for the extra fields: the info of an entity that may have one
/// (flags 0x3000000), made when missing.
pub(crate) fn info_ext(world: &mut World, id: u16) -> Option<&mut super::player::InfoExt> {
    if !world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0) {
        return None;
    }
    Some(world.ai.ext.entry(id).or_default())
}

/// 0x435370 `(entity, 0)` / entity +0x10: whether the entity has an AI info (it goes with the
/// entity, 0x566d80).
pub(crate) fn has_info(world: &World, id: u16) -> bool {
    world.entities.by_index(id as u32).is_some() && (world.ai.ext.contains_key(&id) || world.ai.infos.contains_key(&id))
}

pub(crate) fn partner(world: &World, id: u16) -> u16 {
    world.ai.ext.get(&id).map_or(0, |x| x.partner)
}

// ---- the update and the events ------------------------------------------------------------

/// 0x443b50: the queue (0x43f2f0, each event by vtbl+0x28 [`handle`]), then, unless the player's
/// script flag bit 0 is set, each eco-sector AI's `Execute`; with the bit their queues are
/// dropped.
pub fn update(world: &mut World, p: u8, s: u8) {
    // 0x43f2f0 reads from the read index to the end, an event at a time, so events the handler
    // adds to the queue are handled in the same pass.
    loop {
        let Some(x) = sec_mut(world, p, s) else { return };
        let k = x.queue.read as usize;
        let Some(&ev) = x.queue.events.get(k) else {
            x.queue = Default::default();
            break;
        };
        x.queue.read += 1;
        handle(world, p, s, ev);
    }
    let n = eco_list(world, p, s).len();
    let skip = world.ai.vars.records.get(p as usize).is_some_and(|r| r.flags & 1 != 0);
    for k in 0..n {
        if skip {
            if let Some(e) = eco_mut(world, p, s, k) {
                e.queue.clear();
            }
        } else {
            super::eco::execute(world, p, s, k);
        }
    }
}

/// vtbl+0x1c (0x4434a0): the event goes to the sector AI's queue and to each eco-sector AI's.
pub fn post(world: &mut World, p: u8, s: u8, ev: AiEvent) {
    let Some(x) = sec_mut(world, p, s) else { return };
    x.queue.events.push(ev);
    if let SectorKind::Normal { eco, .. } = &mut x.kind {
        for e in eco {
            e.queue.push(ev);
        }
    }
}

/// 0x443440 `(a, b)`: event 0x1e `(a, b, 0)` to each eco-sector AI (from the squad agent).
pub fn post_eco(world: &mut World, p: u8, s: u8, a: i32, b: i32) {
    if let Some(SectorKind::Normal { eco, .. }) = sec_mut(world, p, s).map(|x| &mut x.kind) {
        for e in eco {
            e.queue.push(AiEvent { kind: 0x1e, player: a, a: b, b: 0 });
        }
    }
}

/// The handler (vtbl+0x28, 0x443520) by event type:
/// * 3 (a building of `b` captured): when `b` is this player, event 0x1f `(player, building, 10)`
///   through vtbl+0x1c;
/// * 0xd / 0xe / 0x10: the eco-sector AIs (new, deleted, split);
/// * 0x14 (a new unit): it joins reservoir 3 (0x444b70);
/// * 0x1b (`AI.AttackNow`): the attack agent's 0x42f1e0 `(a, b)`;
/// * 0x1c (`AI.NewSquad` `a`): see [`new_squad`];
/// * 0x1f (we lost a building): the building into goal cache 3 with value 0x7fffffff, the attack
///   agent's 0x42f1e0 `(its new owner, 10)` and then `(a, b)` as for 0x1b (the release falls
///   through).
fn handle(world: &mut World, p: u8, s: u8, ev: AiEvent) {
    match ev.kind {
        3 => {
            if ev.b == p as i32 {
                post(world, p, s, AiEvent { kind: 0x1f, player: p as i32, a: ev.a, b: 10 });
            }
        }
        0xd => new_eco(world, p, s, ev.a as u32, ev.b as u32),
        0xe => drop_dead_eco(world, p, s),
        0x10 => {
            // `(new << 16 | old, the new one's unique id)`: with the new eco-sector alive, the old
            // one's AI (made before this tick) is made anew for its current unique id.
            let id = (ev.a & 0xffff) as u32;
            if !eco_alive(world, (ev.a >> 16) as u32 & 0xffff, ev.b as u32) {
                return;
            }
            drop_dead_eco(world, p, s);
            let list = eco_list(world, p, s);
            let Some(k) = list.iter().position(|e| e.0 as u32 == id) else { return };
            if list[k].2 < world.ai.tick {
                let uid = world.economy.get(id as u16).map_or(0, |e| e.unique_id);
                remove_eco(world, p, s, k);
                new_eco(world, p, s, id, uid);
            }
        }
        0x14 => super::player::add_to(world, ev.a as u16, super::player::TfRef::Force(p, s, force::NEW)),
        0x1b => {
            super::attack::attack_now(world, p, s, ev.a, ev.b);
        }
        0x1c => new_squad(world, p, s, ev.a),
        0x1f => {
            let owner = world.entities.by_index(ev.a as u32).map_or(0, |e| e.owner);
            cache_add(world, p, s, cache::WEAK, ev.a, 0x7fff_ffff);
            super::attack::attack_now(world, p, s, owner as i32, 10);
            super::attack::attack_now(world, p, s, ev.a, ev.b);
        }
        _ => {}
    }
}

/// Event 0x1c `(command)`: with units in reservoir 3, a new group task force of type 4 (command
/// 9) or 3 takes its swordsmen, bowmen and specialists (warrior types 2..4). With members: the
/// offence evaluation runs (0x442050); command 9 with a target (0x443bc0) orders an attack on it
/// (order 4) and brings the suicide agent forward (0x441d40); otherwise 0x443d90 sends it home.
/// Without members it is destroyed.
fn new_squad(world: &mut World, p: u8, s: u8, command: i32) {
    let r3 = super::player::TfRef::Force(p, s, force::NEW);
    if super::taskforce::count(world, r3) <= 0 {
        return;
    }
    let Some(t) = super::taskforce::create(world, p, s, if command == 9 { 4 } else { 3 }) else { return };
    for m in super::taskforce::members(world, r3) {
        let wt = world.entities.by_index(m as u32).map_or(0, |e| e.flags & 0xf);
        if (2..=4).contains(&wt) {
            super::player::add_to(world, m, t);
        }
    }
    if super::taskforce::count(world, t) > 0 {
        eval_offence(world, p, s);
        if command == 9 {
            let target = super::attack::suicide_target(world, p, s, t);
            if target > 0 {
                super::taskforce::order(world, t, 4, target, 0);
                bring_forward(world, p, s, agent::SUICIDE);
                return;
            }
        }
        send_home(world, p, s, t);
        return;
    }
    super::taskforce::destroy(world, t);
}

/// 0x441d40 on agent `k`: due after its period from now when that is earlier than planned.
pub(crate) fn bring_forward(world: &mut World, p: u8, s: u8, k: u8) {
    let now = world.ai.tick;
    let Some(a) = world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut()) else { return };
    let r = AgentRef::Sector(s, k);
    let Some(ag) = super::sched::Agents::get(&a.body, r) else { return };
    let at = now.wrapping_add(ag.period);
    if a.sched.is_registered(r) && at < ag.next {
        a.sched.reschedule(&mut a.body, r, at);
    }
}

/// 0x42e950 after a check (`registered && at < next`) as the callers do it: agent `k` due at `at`.
pub(crate) fn due_at(world: &mut World, p: u8, s: u8, k: u8, at: u32) {
    let Some(a) = world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut()) else { return };
    let r = AgentRef::Sector(s, k);
    let Some(ag) = super::sched::Agents::get(&a.body, r) else { return };
    if a.sched.is_registered(r) && at < ag.next {
        a.sched.reschedule(&mut a.body, r, at);
    }
}

/// 0x443d90 `(task force)`: a group task force of class 2 whose type is not 3 becomes type 3 (the
/// group's per-type counts follow); it is ordered to move (1) to the anchor and the moving-home
/// agent is brought forward.
pub(crate) fn send_home(world: &mut World, p: u8, s: u8, t: super::player::TfRef) {
    set_kind(world, t, 3);
    let (x, y) = anchor(world, p, s);
    super::taskforce::order(world, t, 1, y * 0x10000 + x, 0);
    bring_forward(world, p, s, agent::MOVING_HOME);
}

/// A group task force of class 2 (squads) changes its type (+8; the group's counts +0x88 + 4·type
/// follow).
pub(crate) fn set_kind(world: &mut World, t: super::player::TfRef, kind: i32) {
    let super::player::TfRef::Group(p, s, uid) = t else { return };
    if super::taskforce::class_of_ref(world, t) != super::taskforce::class::SQUAD {
        return;
    }
    if let Some(it) = sec_mut(world, p, s).and_then(|x| x.group.get_mut(uid)) {
        it.kind = kind;
    }
}

// ---- eco-sector AIs ----------------------------------------------------------------------

/// 0x436410 `(id, uid)`: the eco-sector exists with that unique id.
fn eco_alive(world: &World, id: u32, uid: u32) -> bool {
    id < 0x4000 && world.economy.get(id as u16).is_some_and(|e| e.unique_id == uid)
}

/// 0x4437c0: an eco-sector of the player with more than 4 carriers (+0x26).
fn eco_qualifies(world: &World, p: u8, id: u16) -> bool {
    world.economy.get(id).is_some_and(|e| e.owner == p && e.settlers[1] > 4)
}

/// `CTiling` vtbl+0x2c (0x5b7bf0) then the sector: the walking sector of a tile of the
/// eco-sector. [inf] The release caches one tile per eco-sector and scans from a cursor; any tile
/// of an eco-sector lies on the same walking sector, so the lowest tile id is taken.
fn eco_walking_sector(world: &World, id: u16) -> u16 {
    world.tiling.tiles.iter().find(|t| t.used() && t.eco_sector == id).map_or(0, |t| t.sector)
}

/// 0x443960 `(id, uid)`: with the eco-sector alive under that unique id, the dead ones go
/// (0x4438e0); one not yet listed that lies on this sector (0x4364d0) and qualifies (0x4437c0)
/// gets its eco-sector AI.
fn new_eco(world: &mut World, p: u8, s: u8, id: u32, uid: u32) {
    if !eco_alive(world, id, uid) {
        return;
    }
    drop_dead_eco(world, p, s);
    if eco_list(world, p, s).iter().any(|e| e.0 as u32 == id) {
        return;
    }
    if eco_walking_sector(world, id as u16) == sector_of(world, p, s) && eco_qualifies(world, p, id as u16) {
        let e = super::eco::new(world, p, s, id as u16);
        if let Some(SectorKind::Normal { eco, .. }) = sec_mut(world, p, s).map(|x| &mut x.kind) {
            eco.push(e);
        }
    }
}

/// 0x4438e0: every eco-sector AI whose eco-sector is gone or has another unique id is destroyed
/// (the last one takes its place).
fn drop_dead_eco(world: &mut World, p: u8, s: u8) {
    let mut k = 0;
    loop {
        let list = eco_list(world, p, s);
        let Some(&(id, uid, _)) = list.get(k) else { break };
        if eco_alive(world, id as u32, uid) {
            k += 1;
        } else {
            remove_eco(world, p, s, k);
        }
    }
}

/// Removal by swapping in the last one (0x443860 / 0x4438e0).
fn remove_eco(world: &mut World, p: u8, s: u8, k: usize) {
    if let Some(SectorKind::Normal { eco, .. }) = sec_mut(world, p, s).map(|x| &mut x.kind)
        && k < eco.len()
    {
        eco.swap_remove(k);
    }
}

/// 0x4439f0 (end of the ctor and of a load): every eco-sector on the sector (0x436510: ids from 1
/// up, existing, on the sector) that is not listed, is the player's and qualifies gets an
/// eco-sector AI.
pub fn make_eco(world: &mut World, p: u8, s: u8) {
    let sector = sector_of(world, p, s);
    if sector == 0 {
        return;
    }
    let ids: Vec<u16> =
        (1..crate::eco::ECO_MAX as u16).filter(|&id| world.economy.get(id).is_some() && eco_walking_sector(world, id) == sector).collect();
    for id in ids {
        if eco_list(world, p, s).iter().any(|e| e.0 == id) || !eco_qualifies(world, p, id) {
            continue;
        }
        let e = super::eco::new(world, p, s, id);
        if let Some(SectorKind::Normal { eco, .. }) = sec_mut(world, p, s).map(|x| &mut x.kind) {
            eco.push(e);
        }
    }
}

// ---- evaluations ---------------------------------------------------------------------------

/// 0x435d10 `(player, x, y)`: the owners of the 16×16 squares around (COwnerMapEx byte 0 at
/// 0x12ef998): `[own in the centre square, own in its 8 neighbours, own in the 16 squares two
/// away (0xff0498), own score, the same four for the player's enemies]`, a score being
/// `ring2 + (centre · 16 + ring1) · 4`. Squares are checked against 64, not the map side.
fn square_owners(world: &World, p: u8, x: i32, y: i32) -> [i32; 8] {
    const RING2: [(i32, i32); 16] =
        [(-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2), (-2, -1), (2, -1), (-2, 0), (2, 0), (-2, 1), (2, 1), (-2, 2), (-1, 2), (0, 2), (1, 2), (2, 2)];
    let own = player_bit(p) as u8;
    let enemy = world.alliances.enemy_bits[p.min(8) as usize] as u8;
    let (sx, sy) = (x >> 4, y >> 4);
    let bits = |dx: i32, dy: i32| -> u8 {
        let (a, b) = (sx + dx, sy + dy);
        if (0..64).contains(&a) && (0..64).contains(&b) { world.tiling.owner_bits(a, b)[0] } else { 0 }
    };
    let c = bits(0, 0);
    let (c_own, c_enemy) = ((c & own != 0) as i32, (c & enemy != 0) as i32);
    let (mut r1_own, mut r1_enemy) = (0, 0);
    for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
        let b = bits(dx, dy);
        r1_own += (b & own != 0) as i32;
        r1_enemy += (b & enemy != 0) as i32;
    }
    let (mut r2_own, mut r2_enemy) = (0, 0);
    for (dx, dy) in RING2 {
        let b = bits(dx, dy);
        r2_own += (b & own != 0) as i32;
        r2_enemy += (b & enemy != 0) as i32;
    }
    [c_own, r1_own, r2_own, r2_own + (c_own * 16 + r1_own) * 4, c_enemy, r1_enemy, r2_enemy, r2_enemy + (c_enemy * 16 + r1_enemy) * 4]
}

/// .rdata 0x101c990 copied to 0x1443790 (the war map's strength per settler type).
fn strength(kind: u16) -> i32 {
    crate::warmap::settler_strength(kind)
}

/// 0x5b1120 `(x, y, r, player)`: the settlers of warrior type 2..5 in the war-map cells within `r`
/// ([`crate::warmap::cells_within`]) on the sector of `(x, y)` and within `r`: per side (0 the
/// player's allies, 1 the player, 2 the others) the count and the strength (type strength << 4,
/// divided by the distance beyond 12). Returns `[own count, ally count, other count, own
/// strength, ally strength, other strength]`.
pub(crate) fn warriors_around(world: &World, x: i32, y: i32, r: i32, p: u8) -> [i32; 6] {
    let sector = sector_at(world, x, y);
    let allies = world.alliances.ally_bits[p.min(8) as usize];
    let mut sum = [0i32; 3];
    let mut count = [0i32; 3];
    for (cx, cy) in crate::warmap::cells_within(world, x, y, r) {
        for id in world.warmap.cell(cx as usize, cy as usize, 0) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            if !(2..=5).contains(&(e.flags & 0xf)) || sector_at(world, e.x(), e.y()) != sector {
                continue;
            }
            let d = crate::spiral::grid_distance(e.x() - x, e.y() - y);
            if d > r {
                continue;
            }
            let side = if allies & player_bit(e.owner) != 0 { (e.owner == p) as usize } else { 2 + (e.owner == p) as usize };
            let side = side.min(2);
            let mut v = strength(e.kind) << 4;
            if d > 12 {
                v /= d;
            }
            count[side] += 1;
            sum[side] += v;
        }
    }
    [count[1], count[0], count[2], sum[1], sum[0], sum[2]]
}

/// 0x5b1320 `(x, y, r, player)`: the buildings of warrior type 0xc in the war-map cells within
/// `r` whose ensign is on the sector of `(x, y)` and within `r`; for the side "other" (not of the
/// player's team): `[count, swordsmen inside, bowmen inside, strength + weight]`, the weight 1 /
/// 2 / 4 for a small tower / large tower / castle, the strength of the READY ones' soldiers
/// (role +0x1a4 list; bowmen (3) count double, each divided by the distance beyond 12).
pub(crate) fn buildings_around(world: &World, x: i32, y: i32, r: i32, p: u8) -> [i32; 4] {
    let sector = sector_at(world, x, y);
    let allies = world.alliances.ally_bits[p.min(8) as usize];
    let (mut count, mut weight, mut swords, mut bows, mut sum) = (0, 0, 0, 0, 0);
    for (cx, cy) in crate::warmap::cells_within(world, x, y, r) {
        for id in world.warmap.cell(cx as usize, cy as usize, 2) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            if e.flags & 0xf != 0xc {
                continue;
            }
            let Some(en) = ensign(world, id) else { continue };
            if sector_at(world, en.x(), en.y()) != sector {
                continue;
            }
            let d = crate::spiral::grid_distance(en.x() - x, en.y() - y);
            if d > r || allies & player_bit(e.owner) != 0 {
                continue;
            }
            count += 1;
            weight += match e.kind {
                47 => 2,
                48 => 4,
                _ => 1,
            };
            if e.flags & flag::READY != 0 {
                for &sid in crate::buildings::military::slots(world, id) {
                    let Some(se) = world.entities.by_index(sid as u32).filter(|_| sid != 0) else { continue };
                    let mut v = strength(se.kind) << 4;
                    if d > 12 {
                        v /= d;
                    }
                    match se.flags & 0xf {
                        3 => {
                            sum += v * 2;
                            bows += 1;
                        }
                        2 => {
                            sum += v;
                            swords += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    [count, swords, bows, sum + weight]
}

/// 0x43b680 on the land cell of `(x, y)` (0x1267bc0 + 0x2a · cell): `Σ (a·S + b)·S + c` over the
/// 3×3 resource sums S (+0x10..+0x1e) with the .data coefficients 0x1163418.. ({c, b, a} per
/// field, all 0 except c = 1 for +0x18, b = 2 for +0x1a, a = 3 for +0x1c): `1 + 2·S5 + 3·S6²`
/// (32-bit wrapping).
fn land_value(world: &World, x: i32, y: i32) -> i32 {
    let s = crate::animals::cells::neighbour_sums(&world.animals.cells, x >> 4, y >> 4);
    let (s5, s6) = (s[5] as u32, s[6] as u32);
    1u32.wrapping_add(s5.wrapping_mul(2)).wrapping_add(s6.wrapping_mul(3).wrapping_mul(s6)) as i32
}

/// 0x43f510 `(building, out)`: [`square_owners`] at the building's ensign and the warriors around
/// it within 0x20 (own strength +0x20, other strength +0x24); the building's AI info gets the
/// tick (+4) and its `CAIEntityInfoTower` (+0x14) the strengths (+8 own, +0xc allies, +0x10
/// others).
fn tower_eval(world: &mut World, p: u8, b: u16) -> [i32; 10] {
    let Some(en) = ensign(world, b) else { return [0; 10] };
    let (x, y) = (en.x(), en.y());
    let o = square_owners(world, p, x, y);
    let w = warriors_around(world, x, y, 0x20, p);
    let now = world.ai.tick;
    if let Some(info) = info_ext(world, b) {
        info.tick = now;
        info.tower = Some([w[3], w[4], w[5]]);
    }
    [o[0], o[1], o[2], o[3], o[4], o[5], o[6], o[7], w[3], w[5]]
}

/// `CAIAgentEvalDefence::Execute` (0x4315c0 → 0x441eb0, period 5): from the cursor of the own
/// military buildings (restarted when its building is gone, not a READY building or not the
/// player's), up to 50 steps (each building on the sector counts 11): each building on the
/// sector goes into goal cache 0 (threat) and 1 (exposure) and gets a tower occupant
/// (0x441de0); then the cursor moves on. A building on another sector does not move the cursor
/// (release behaviour: the evaluation stays at it). With enemy warriors near any of them the squad
/// agent is due within 15 ticks. Returns whether it started from the beginning (unused).
pub fn eval_defence(world: &mut World, p: u8, s: u8) -> bool {
    let sector = sector_of(world, p, s);
    if sector == 0 {
        return false;
    }
    let mask = cursor_mask(world, p, 0);
    let lists = Lists::new(world, mask);
    let mut c = cursor(world, p, s, 0);
    let mut restarted = false;
    let mut cur = c.entity;
    let valid = |world: &World, id: u16| world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0 && e.flags & 0xf == 0xc && e.owner == p);
    if cur == 0 {
        restarted = true;
        cur = cursor_first(&mut c, mask, &lists, world);
    } else if !valid(world, cur) {
        cur = cursor_first(&mut c, mask, &lists, world);
    }
    let mut threat = false;
    let mut n = 0;
    if cur != 0 {
        loop {
            n += 1;
            let on = ensign(world, cur).is_some_and(|en| sector_at(world, en.x(), en.y()) == sector);
            if on {
                n += 10;
                let o = tower_eval(world, p, cur);
                cache_add(world, p, s, cache::THREAT, cur as i32, ((o[9] * 4 - o[8]) << 12) + o[9].min(0xfff));
                cache_add(world, p, s, cache::EXPOSED, cur as i32, o[7] * 4 - o[3]);
                threat |= o[9] > 0;
                occupant(world, p, s, cur);
                cur = cursor_next(&mut c, mask, &lists, world);
            }
            if cur == 0 || n >= 50 {
                break;
            }
        }
        if threat {
            let at = world.ai.tick.wrapping_add(15);
            due_at(world, p, s, agent::SQUADS, at);
        }
    }
    set_cursor(world, p, s, 0, c);
    restarted
}

/// 0x441de0 `(building)`: the building's AI info names a swordsman (+2), who names it back. Not
/// for a building whose worker is in the house (role +0x1d: the info's partner is cleared). A
/// partner that is still linked back is kept. Otherwise: slot 2 is refilled from the squads when
/// empty (0x43f700), the swordsman of slot 2 nearest the ensign (0x43fab0) goes to reservoir 4
/// and becomes the partner, and walks (event 0x11 kind 0xd, data0 0) to the spot 0x43f800 finds
/// from the ensign. The military building's soldier search takes him (0x582f70).
fn occupant(world: &mut World, p: u8, s: u8, b: u16) {
    if info_ext(world, b).is_none() {
        return;
    }
    if crate::buildings::role(world, b).is_some_and(|r| r.base.worker_in_house) {
        if let Some(i) = info_ext(world, b) {
            i.partner = 0;
        }
        return;
    }
    let cur = partner(world, b);
    if cur > 0 && has_info(world, cur) && partner(world, cur) == b {
        return;
    }
    let Some(pos) = ensign(world, b) else { return };
    refill_swordsmen(world, p, s, pos);
    let soldier = nearest_to(world, p, s, pos, 2, super::player::TfRef::Force(p, s, force::OCCUPY));
    if soldier > 0 {
        if let Some(i) = info_ext(world, b) {
            i.partner = soldier;
        }
        if let Some(i) = info_ext(world, soldier) {
            i.partner = b;
        }
        let spot = crate::grid::Y16X16(spot_near(world, p, s, pos.0 as i32) as u32);
        super::taskforce::move_entity(world, soldier, spot.x(), spot.y(), true);
    }
}

/// 0x43f700 `(pos)`: when slot 2 (swordsmen) is empty, the swordsman (warrior type 2) of the
/// group's squads (type 5) nearest `pos` — by distance / 8 (38 for 0x138 and more), at most 40 a
/// bucket, the first of the nearest bucket — joins slot 2.
fn refill_swordsmen(world: &mut World, p: u8, s: u8, pos: crate::grid::Y16X16) {
    let slot = super::player::TfRef::Slot(p, s, 2);
    if super::player::tf(world, slot).is_none_or(|t| !t.members.is_empty()) {
        return;
    }
    let mut buckets: Vec<Vec<u16>> = vec![Vec::new(); 0x4f];
    for t in group_of_kind(world, p, s, 5) {
        for m in super::taskforce::members(world, t) {
            let Some(e) = world.entities.by_index(m as u32) else { continue };
            if e.flags & 0xf != 2 {
                continue;
            }
            let d = crate::spiral::grid_distance(pos.x() - e.x(), pos.y() - e.y());
            let k = if d < 0x138 { (d >> 3) as usize } else { 0x26 };
            if buckets[k].len() < 0x28 {
                buckets[k].push(m);
            }
        }
    }
    // 0x439070 from (bucket 0, index −1); none left reads past the buckets (uninitialised stack
    // in the release) [inf: nothing].
    if let Some(&m) = buckets.iter().find_map(|b| b.first()) {
        super::player::add_to(world, m, slot);
    }
}

/// The group task forces of `kind` (+8) of the sector AI, newest first (all classes' lists from
/// +0xc8, the squad class).
pub(crate) fn group_of_kind(world: &World, p: u8, s: u8, kind: i32) -> Vec<super::player::TfRef> {
    super::taskforce::list(world, p, s, super::taskforce::class::SQUAD)
        .into_iter()
        .filter(|&t| super::taskforce::kind_of_ref(world, t) == kind)
        .collect()
}

/// 0x43fab0 `(task force, pos, warrior type)`: of slot `wt`'s members the nearest to `pos` (hex
/// distance; one within 8 at once, else the nearest, the first of equals, below 10000) joins the
/// task force `to` (vtbl+0x14 mode 0); its id, 0 when none.
pub(crate) fn nearest_to(world: &mut World, p: u8, s: u8, pos: crate::grid::Y16X16, wt: u8, to: super::player::TfRef) -> u16 {
    let slot = super::player::TfRef::Slot(p, s, wt);
    let Some(t) = super::player::tf(world, slot) else { return 0 };
    let (mut best, mut bd) = (0u16, 9999);
    for &m in &t.members {
        let Some(e) = world.entities.by_index(m as u32) else { continue };
        let d = crate::spiral::grid_distance(pos.x() - e.x(), pos.y() - e.y());
        if d < 9 {
            best = m;
            bd = d;
            break;
        }
        if d < bd {
            best = m;
            bd = d;
        }
    }
    if best != 0 && bd < 10000 {
        super::player::add_to(world, best, to);
        return best;
    }
    0
}

/// 0x43f800 `(pos)`: of the six directions (.rdata 0x101bd88) the one with the most land of the
/// player (walking up to 8 steps while on the map, on land the player owns and on this sector or
/// none: 2 a node on a sector, 1 without) scored `4 · its count + its two neighbours'`, the
/// first of equals; 6 steps that way, then the first node of the spiral to radius 8 (0x431270)
/// on the sector. `pos` itself when no direction scores or no such node exists.
pub(crate) fn spot_near(world: &World, p: u8, s: u8, pos: i32) -> i32 {
    const DIRS: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];
    let sector = sector_of(world, p, s);
    let size = world.grid.size() as i32;
    let (x0, y0) = (pos & 0xffff, pos >> 16);
    let mut c = [0i32; 6];
    for (k, &(dx, dy)) in DIRS.iter().enumerate() {
        let (mut x, mut y) = (x0, y0);
        for _ in 0..8 {
            x += dx;
            y += dy;
            if !(0..size).contains(&x) || !(0..size).contains(&y) {
                break;
            }
            let n = (y * size + x) as usize;
            let t = world.tiling.tile(world.tiling.normal[n]);
            if t.owner() != p {
                break;
            }
            let ts = t.sector;
            if ts != 0 && ts as u32 != sector as u32 {
                break;
            }
            c[k] += (ts != 0) as i32 + 1;
        }
    }
    let mut best: i32 = -1;
    let mut score = 0;
    for k in 0..6 {
        let v = c[k] * 4 + c[(k + 5) % 6] + c[(k + 1) % 6];
        if c[k] > 0 && v > score {
            score = v;
            best = k as i32;
        }
    }
    if best < 0 {
        return pos;
    }
    let (dx, dy) = DIRS[best as usize];
    let (cx, cy) = (x0 + dx * 6, y0 + dy * 6);
    spiral_on_sector(world, cx, cy, 8, sector).map_or(pos, |(x, y)| y * 0x10000 + x)
}

/// 0x431270 with 0x4312e0: the spiral from `(x, y)` to `Last(radius)` (inclusive), the first node
/// on the map whose sector is `sector`.
pub(crate) fn spiral_on_sector(world: &World, x: i32, y: i32, radius: u8, sector: u16) -> Option<(i32, i32)> {
    let size = world.grid.size() as i32;
    (0..=crate::spiral::last(radius))
        .map(|k| (x + crate::spiral::dx(k), y + crate::spiral::dy(k)))
        .filter(|&(a, b)| (a as u32) < size as u32 && (b as u32) < size as u32)
        .find(|&(a, b)| sector_at(world, a, b) as u32 == sector as u32)
}

/// 0x43f5e0 `(building)`: for an enemy building, the tile path (0x5ac2f0 mode 6, flags player |
/// 0x2c000600) from its ensign to the anchor; with a path of length above 0: `[−length, −(the
/// other side's tower strength and weight within 0x20 + its warriors' strength within 0x20),
/// the land value of its cell]`.
fn target_eval(world: &World, p: u8, s: u8, b: u16) -> Option<[i32; 3]> {
    let en = ensign(world, b)?;
    let (ax, ay) = anchor(world, p, s);
    let mut wp = crate::pathing::Waypoints::default();
    let side = (world.grid.size() >> 4) as usize;
    let r = crate::pathing::tiling_astar::astar_tiling(&world.tiling, &world.alliances, &world.warmap, side, en, crate::grid::Y16X16::pack(ax, ay), p as u32 | 0x2c00_0600, &mut wp);
    if r == 0 || wp.goal_value <= 0 {
        return None;
    }
    let w = warriors_around(world, en.x(), en.y(), 0x20, p);
    let t = buildings_around(world, en.x(), en.y(), 0x20, p);
    Some([-wp.goal_value, -(t[3] + w[5]), land_value(world, en.x(), en.y())])
}

/// `CAIAgentEvalOffence::Execute` (0x431700 → 0x442050, period 10): from the cursor of the enemy
/// military buildings (restarted when its building is gone, not a READY building or of the
/// player's team), up to 10 steps (each READY building on the sector counts 14): each READY one
/// on the sector with a path goes into goal caches 2, 3 and 4 ([`target_eval`]).
pub fn eval_offence(world: &mut World, p: u8, s: u8) {
    let sector = sector_of(world, p, s);
    if sector == 0 {
        return;
    }
    let mask = cursor_mask(world, p, 1);
    let lists = Lists::new(world, mask);
    let mut c = cursor(world, p, s, 1);
    let mut cur = c.entity;
    let team = world.alliances.team(p);
    let valid = cur != 0
        && world.entities.by_index(cur as u32).is_some_and(|e| e.flags & flag::ALIVE_MASK != 0 && e.flags & 0xf == 0xc && world.alliances.team(e.owner) != team);
    if !valid {
        cur = cursor_first(&mut c, mask, &lists, world);
    }
    let mut n = 0;
    while cur != 0 {
        n += 1;
        let on = ensign(world, cur).is_some_and(|en| sector_at(world, en.x(), en.y()) == sector);
        let ready = world.entities.by_index(cur as u32).is_some_and(|e| e.flags & flag::READY != 0);
        if on && ready {
            n += 0xd;
            if let Some(v) = target_eval(world, p, s, cur) {
                cache_add(world, p, s, cache::NEAR, cur as i32, v[0]);
                cache_add(world, p, s, cache::WEAK, cur as i32, v[1]);
                cache_add(world, p, s, cache::LAND, cur as i32, v[2]);
            }
        }
        cur = cursor_next(&mut c, mask, &lists, world);
        if n >= 10 {
            break;
        }
    }
    set_cursor(world, p, s, 1, c);
}
