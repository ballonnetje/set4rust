//! The normal sector AI's attack (`CAIAgentAttack`, slot 0, period 100, Execute 0x42faf0) and the
//! target picks the squads share. Own reverse engineering of the shipped exe; decompiles in
//! `tools/re/decompiled/script/ai/s2/` (`agents.c`, `more1.c`, `more2.c`, `core.c`). No AI save
//! exists to check against.
//!
//! The agent's ints (+0x28.., saved, [`super::model::AgentKind::Attack`]):
//! 0 the step, 1 the soldiers of the sector AI's squads and slots and 2 their strength (both set by
//! the squad agent each run), 3 the earliest tick of the next attack, 4 the chance (out of 65536)
//! to try one, 5..9 the target: entity, unique id, owner, ensign x, y.
//!
//! Steps: 0 wait — after `AttackDelay` minutes and the earliest tick, with the chance (growing by
//! 0x51e up to 0x28f5 at each miss) a target from goal cache 3 whose owner's fighters are weaker
//! than ours, either "partial" (`AttackChancePartial` %, one squad, `AttackThresholdPartial`) or
//! "total" (all but a fifth of the squads, `AttackThresholdTotal`; `AttackMode` 2 or more); the
//! squads (type 6 from then on) walk to the edge of the enemy land (order 3); 1 wait until none
//! is walking; 3 all attack (order 4); 2 every squad picks the nearest enemy military building
//! (else the target) until none has swordsmen left; then they go home and the agent starts over.

use super::player::TfRef;
use super::sched::AgentRef;
use super::sector::{self, cache};
use super::taskforce::{self as tfs, class, flag};
use crate::World;
use crate::alliances::player_bit;

/// The AI variables (`AI.SetPlayerVar`) these read.
mod var {
    pub const ATTACK_MODE: usize = 0;
    pub const ATTACK_DELAY: usize = 1;
    pub const ATTACK_CHANCE_PARTIAL: usize = 2;
    pub const ATTACK_THRESHOLD_PARTIAL: usize = 3;
    pub const ATTACK_THRESHOLD_TOTAL: usize = 4;
}

/// The group task-force types of the normal sector AI.
pub mod kind {
    /// Going home (0x443d90).
    pub const HOME: i32 = 3;
    /// A suicide mission (`AI.NewSquad` 9).
    pub const SUICIDE: i32 = 4;
    /// A squad guarding the player's buildings.
    pub const SQUAD: i32 = 5;
    /// A squad attacking (0x42f620).
    pub const ATTACKER: i32 = 6;
    /// Priests (class 3).
    pub const PRIESTS: i32 = 8;
}

fn ints(world: &World, p: u8, s: u8) -> Vec<i32> {
    world.ai.players[p as usize].as_ref().and_then(|a| super::sched::Agents::get(&a.body, AgentRef::Sector(s, 0))).map_or(vec![0; 10], |a| a.extra.clone())
}

fn set_ints(world: &mut World, p: u8, s: u8, v: &[i32]) {
    if let Some(a) = world.ai.players[p as usize].as_mut().and_then(|a| super::sched::Agents::agent(&mut a.body, AgentRef::Sector(s, 0))) {
        a.extra = v.to_vec();
    }
}

/// Sets attack int `k`.
pub(crate) fn set_int(world: &mut World, p: u8, s: u8, k: usize, v: i32) {
    let mut a = ints(world, p, s);
    if let Some(x) = a.get_mut(k) {
        *x = v;
    }
    set_ints(world, p, s, &a);
}

fn var(world: &World, p: u8, k: usize) -> i32 {
    world.ai.vars.records.get(p as usize).map_or(0, |r| r.get(k))
}

/// 0x42f260: step 0, the counts 0, the earliest attack 0x200 + 0..127 ticks from now (one RNG
/// draw), the chance 0x51e, no target. The sector AI's ctor runs it.
pub fn reset(world: &mut World, p: u8, s: u8) {
    let now = world.ai.tick;
    let r = world.rng.rand();
    set_ints(world, p, s, &[0, 0, 0, now.wrapping_add(0x200).wrapping_add(r & 0x7f) as i32, 0x51e, 0, 0, 0, 0, 0]);
}

/// The squads (class 2) of the sector AI with members and the order `order` (0x42bc10), in list
/// order.
pub(crate) fn squads_with_order(world: &World, p: u8, s: u8, order: u32) -> Vec<TfRef> {
    tfs::list(world, p, s, class::SQUAD).into_iter().filter(|&t| tfs::count(world, t) >= 1 && super::player::tf(world, t).is_some_and(|x| x.command() == order)).collect()
}

/// The group task forces of `kind`.
pub(crate) fn count_of_kind(world: &World, p: u8, s: u8, kind: i32) -> i32 {
    world.ai.players[p as usize].as_ref().and_then(|a| a.body.sectors.get(s as usize)).map_or(0, |x| x.group.items.iter().filter(|i| i.kind == kind).count() as i32)
}

/// `CAIAgentAttack::Execute` (0x42faf0).
pub fn execute(world: &mut World, p: u8, s: u8) -> u32 {
    let a = ints(world, p, s);
    match a[0] {
        0 => {
            let mode = var(world, p, var::ATTACK_MODE);
            if mode < 1 {
                return 0;
            }
            let now = world.ai.tick;
            if now <= a[3] as u32 || now <= (var(world, p, var::ATTACK_DELAY) as u32).wrapping_mul(0x348) {
                return 0;
            }
            if world.rng.rand() < a[4] as u32 {
                let r = world.rng.rand();
                let partial = r < ((var(world, p, var::ATTACK_CHANCE_PARTIAL) as u32) << 16) / 100;
                if (partial || mode > 1) && find_target(world, p, s, 0, 2 - partial as i32) && send_squads(world, p, s, partial as i32) {
                    set_int(world, p, s, 0, 1);
                    return 0;
                }
            }
            let mut a = ints(world, p, s);
            if (a[4] as u32) < 0x28f5 {
                a[4] += 0x51e;
                set_ints(world, p, s, &a);
            }
            let r = world.rng.rand();
            ((r >> 8) & 1) << 20 | 0x20_0000
        }
        1 => {
            if check_target(world, p, s) {
                let n = staged(world, p, s);
                if n != 0 {
                    if n >= 1 {
                        set_int(world, p, s, 0, 3);
                    }
                    return 0;
                }
            }
            recall(world, p, s, 3);
            reset(world, p, s);
            0
        }
        3 => {
            if check_target(world, p, s) && charge(world, p, s) > 0 {
                set_int(world, p, s, 0, 2);
                return 0;
            }
            recall(world, p, s, 3);
            reset(world, p, s);
            0
        }
        2 => {
            check_target(world, p, s);
            if fight(world, p, s) > 0 {
                return 0;
            }
            recall(world, p, s, 4);
            reset(world, p, s);
            0
        }
        _ => {
            reset(world, p, s);
            0
        }
    }
}

/// 0x42f1e0 `(player, n)` (`AI.AttackNow` via event 0x1b, and event 0x1f): a player 1..8 in the
/// game and not of our team, or 0 for any; while the agent waits (step 0), a target of that player
/// (mode 0: no strength test) and `n` squads ([`send_squads`]) start an attack (step 1) and the
/// agent is due at once.
pub fn attack_now(world: &mut World, p: u8, s: u8, player: i32, n: i32) -> bool {
    if player != 0 {
        if !(1..=8).contains(&player) {
            return false;
        }
        let team = world.alliances.team(player as u8);
        if team < 1 || world.alliances.team(p) == team {
            return false;
        }
    }
    if ints(world, p, s)[0] == 0 && find_target(world, p, s, player, 0) && send_squads(world, p, s, n) {
        set_int(world, p, s, 0, 1);
        if let Some(a) = world.ai.players[p as usize].as_mut() {
            let r = AgentRef::Sector(s, sector::agent::ATTACK);
            a.sched.reschedule(&mut a.body, r, 0);
        }
        return true;
    }
    false
}

/// 0x42f3a0 `(player, mode)` "CheckStrengthAndFindAnyTarget": the target is cleared; with mode 0
/// or more than 14 soldiers, the first goal of cache 3 whose entity is the one noted, whose owner
/// is not excluded (our team; with `player` everyone else) and, for mode 1 / 2, whose owner's
/// fighters (evaluation word 14) are below our strength scaled by `100 / AttackThresholdPartial`
/// or `/ AttackThresholdTotal` becomes the target (0x42f300); a too strong owner is excluded.
fn find_target(world: &mut World, p: u8, s: u8, player: i32, mode: i32) -> bool {
    let mut a = ints(world, p, s);
    a[5..10].fill(0);
    set_ints(world, p, s, &a);
    if mode != 0 && a[1] <= 0xe {
        return false;
    }
    let entries = sector::caches(world, p, s).and_then(|c| c.get(cache::WEAK)).map_or(Vec::new(), |c| c.entries());
    if entries.is_empty() {
        return false;
    }
    let mut excl = world.alliances.ally_bits[p.min(8) as usize] as u32;
    if player > 0 {
        excl = !(player_bit(player as u8) as u32);
    }
    let mut strength = a[2];
    if mode > 0 {
        let thr = var(world, p, if mode != 1 { var::ATTACK_THRESHOLD_TOTAL } else { var::ATTACK_THRESHOLD_PARTIAL });
        if thr > 0 {
            strength = strength.wrapping_mul(100) / thr;
        }
    }
    for (id, uid, _) in entries {
        if sector::uid_of(world, id) != uid {
            continue;
        }
        let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
        let bit = player_bit(owner) as u32;
        if excl & bit != 0 {
            continue;
        }
        if mode < 1 || super::evaluation::of(world, owner)[14] < strength {
            if set_target(world, p, s, id) {
                return true;
            }
        } else {
            excl |= bit;
        }
    }
    false
}

/// 0x42f300 `(entity)`: with a unique id and an ensign above 0 the entity becomes the target
/// (id, unique id, owner, ensign); otherwise the target is cleared.
fn set_target(world: &mut World, p: u8, s: u8, id: i32) -> bool {
    let uid = sector::uid_of(world, id);
    let pos = if id > 0 { sector::ensign(world, id as u16).map_or(-1, |e| e.0 as i32) } else { -1 };
    let mut a = ints(world, p, s);
    if uid > 0 && pos > 0 {
        let owner = world.entities.by_index(id as u32).map_or(0, |e| e.owner);
        a[5] = id;
        a[6] = uid as i32;
        a[7] = owner as i32;
        a[8] = pos & 0xffff;
        a[9] = pos >> 16;
        set_ints(world, p, s, &a);
        return true;
    }
    a[5..10].fill(0);
    set_ints(world, p, s, &a);
    false
}

/// 0x42f580: the target still stands (unique id, alive) and belongs to another team; otherwise a
/// new target of its owner (mode 0).
fn check_target(world: &mut World, p: u8, s: u8) -> bool {
    let a = ints(world, p, s);
    let ok = world.entities.by_index(a[5].max(0) as u32).is_some_and(|e| {
        a[5] > 0 && e.uid == a[6] as u32 && e.flags & crate::entity::flag::ALIVE_MASK != 0 && world.alliances.team(p) != world.alliances.team(e.owner)
    });
    ok || find_target(world, p, s, a[7], 0)
}

/// 0x42f620 `(n)`: towards the target's ensign: of the squads (type 5) with swordsmen, the most
/// swordsmen first (a binary max-heap, unsigned counts), up to `n` (0: all but a fifth of the
/// squads (`count − count / 5 − 1`), negative: that many fewer than all, at most that and at most
/// 50) become attackers (type 6) and walk to the edge of the enemy land (order 3). True when any
/// went.
fn send_squads(world: &mut World, p: u8, s: u8, n: i32) -> bool {
    let a = ints(world, p, s);
    let pos = a[9].wrapping_mul(0x10000).wrapping_add(a[8]);
    if pos < 1 {
        return false;
    }
    let total = count_of_kind(world, p, s, kind::SQUAD);
    let cap = total - total / 5 - 1;
    let k = if n > 0 { n.min(cap) } else if n < 0 { (n + total).min(cap) } else { cap };
    if k <= 0 {
        return false;
    }
    let k = k.min(0x32);
    // 1-based heap, entry 0 the sentinel 0x7fffffff.
    let mut heap: Vec<(Option<TfRef>, u32)> = vec![(None, 0x7fff_ffff)];
    for t in tfs::list(world, p, s, class::SQUAD) {
        if tfs::kind_of_ref(world, t) != kind::SQUAD {
            continue;
        }
        let c = tfs::warriors(world, t, 2) as u32;
        if c == 0 {
            continue;
        }
        heap.push((Some(t), c));
        let mut i = heap.len() - 1;
        while heap[i >> 1].1 < c {
            heap[i] = heap[i >> 1];
            i >>= 1;
        }
        heap[i] = (Some(t), c);
    }
    let mut sent = 0;
    let mut n = heap.len() - 1;
    while n != 0 && sent < k {
        if let Some(t) = heap[1].0 {
            if tfs::kind_of_ref(world, t) != kind::ATTACKER {
                sector::set_kind(world, t, kind::ATTACKER);
            }
            tfs::order(world, t, 3, pos, 0);
            sent += 1;
        }
        // 0x42fdd0: the last to the top and down.
        let x = heap[n];
        n -= 1;
        let mut i = 1;
        while i <= n >> 1 {
            let mut c = 2 * i;
            if c < n && heap[c].1 < heap[c + 1].1 {
                c += 1;
            }
            if heap[c].1 <= x.1 {
                break;
            }
            heap[i] = heap[c];
            i = c;
        }
        heap[i] = x;
        heap.truncate(n + 1);
    }
    sent > 0
}

/// 0x42f830: the squads walking to the edge (order 3): −1 while one of them is moving (phase 2),
/// else their number.
fn staged(world: &World, p: u8, s: u8) -> i32 {
    let mut n = 0;
    for t in squads_with_order(world, p, s, 3) {
        n += 1;
        if super::player::tf(world, t).is_some_and(|x| x.phase() == 2) {
            return -1;
        }
    }
    n
}

/// 0x42f8d0: every squad at the edge (order 3) attacks the target (order 4); their number.
fn charge(world: &mut World, p: u8, s: u8) -> i32 {
    let target = ints(world, p, s)[5];
    let mut n = 0;
    for t in squads_with_order(world, p, s, 3) {
        n += 1;
        tfs::order(world, t, 4, target, 0);
    }
    n
}

/// 0x42f880 `(order)`: the squads with that order go home (0x443d90).
fn recall(world: &mut World, p: u8, s: u8, order: u32) {
    for t in squads_with_order(world, p, s, order) {
        sector::send_home(world, p, s, t);
    }
}

/// 0x42f9a0: each attacking squad (order 4) without swordsmen goes home; the others count, and —
/// unless it is walking (phase 2) and its order is younger than 0xb9 + 0..127 ticks (one RNG
/// draw) — attack the enemy military building nearest their first member (0x5b1610 within 0x20)
/// or else the target, ordered anew when that differs or the order is 1000 ticks old or flagged
/// 0x4000000; without any such building they go home.
fn fight(world: &mut World, p: u8, s: u8) -> i32 {
    let now = world.ai.tick;
    let mut n = 0;
    for t in squads_with_order(world, p, s, 4) {
        if tfs::warriors(world, t, 2) <= 0 {
            sector::send_home(world, p, s, t);
            continue;
        }
        let Some(x) = super::player::tf(world, t).cloned() else { continue };
        if x.phase() == 2 {
            let r = world.rng.rand();
            if (r & 0x7f).wrapping_add(x.order_tick()).wrapping_add(0xb9) >= now {
                n += 1;
                continue;
            }
        }
        let mut target = 0;
        if let Some(pos) = tfs::first_pos(world, t) {
            let player = (x.flags() & 0xf) as u8;
            let mask = world.alliances.enemy_bits[player.min(8) as usize];
            target = nearest_enemy_building(world, pos.x(), pos.y(), 0x20, mask).0 as i32;
        }
        if target <= 0 {
            target = ints(world, p, s)[5];
        }
        if target <= 0 {
            sector::send_home(world, p, s, t);
            continue;
        }
        n += 1;
        if x.flags() & 0xff0 != 0x40 || x.target() != target || x.flags() & flag::RETARGET != 0 || x.order_tick().wrapping_add(1000) <= now {
            tfs::order(world, t, 4, target, 0);
        }
    }
    n
}

/// 0x5b1610 `(x, y, r, players)`: of the war map's buildings in the cells within `r` (0x5b0310)
/// that are READY military buildings (warrior type 0xc) of a player in the mask, with their ensign
/// on the sector of `(x, y)`, the nearest by ensign (hex distance, below 0x4000; the first of
/// equals): (its id, the distance), (0, 0x4000) for none.
pub(crate) fn nearest_enemy_building(world: &World, x: i32, y: i32, r: i32, mask: u16) -> (u16, i32) {
    let sector = sector::sector_at(world, x, y);
    let (mut best, mut bd) = (0u16, 0x4000);
    for (cx, cy) in crate::warmap::cells_within(world, x, y, r) {
        for id in world.warmap.cell(cx as usize, cy as usize, 2) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            if e.flags & crate::entity::flag::READY == 0 || e.flags & 0xf != 0xc || mask & player_bit(e.owner) == 0 {
                continue;
            }
            let Some(en) = sector::ensign(world, id) else { continue };
            if sector::sector_at(world, en.x(), en.y()) != sector {
                continue;
            }
            let d = crate::spiral::grid_distance(en.x() - x, en.y() - y);
            if d < bd {
                bd = d;
                best = id;
            }
        }
    }
    (best, bd)
}

/// 0x443bc0 `(task force)`: the building a suicide mission (or event 0x1c 9) attacks. None
/// without members or swordsmen. An entity target of its order 4 that still stands and is a
/// military building is kept when within 10 of the first member; otherwise the nearest enemy
/// military building within 0x20 ([`nearest_enemy_building`]) if it is more than 10 nearer;
/// otherwise that target. Without one: the first goal of cache 3 (cleaned once per tick) if it
/// is a standing building.
pub fn suicide_target(world: &mut World, p: u8, s: u8, t: TfRef) -> i32 {
    if tfs::count(world, t) == 0 || tfs::warriors(world, t, 2) <= 0 {
        return 0;
    }
    let (x, y) = tfs::first_pos(world, t).map_or((-1, -1), |p| (p.x(), p.y()));
    let Some(tf) = super::player::tf(world, t).cloned() else { return 0 };
    let mut best = 0;
    let mut d = 0x4000;
    let mut near = false;
    if tf.flags() & 0xff0 == 0x40 && entity_target_ok(world, &tf) {
        best = tf.target();
        if sector::building_alive(world, best) {
            let (ex, ey) = sector::ensign(world, best as u16).map_or((0xffff, -1), |e| (e.x(), e.y()));
            d = crate::spiral::grid_distance(ex - x, ey - y);
            near = d <= 10;
        }
    }
    if !near {
        let mask = world.alliances.enemy_bits[p.min(8) as usize];
        let (b, bd) = nearest_enemy_building(world, x, y, 0x20, mask);
        if b != 0 && bd < d - 10 {
            best = b as i32;
        }
    }
    if best > 0 {
        return best;
    }
    let now = world.ai.tick;
    if sector::caches(world, p, s).and_then(|c| c.get(cache::WEAK)).is_some_and(|c| c.cleaned != now) {
        sector::cache_clean(world, p, s, cache::WEAK);
    }
    let first = sector::caches(world, p, s).and_then(|c| c.get(cache::WEAK)).and_then(|c| c.entries().first().copied()).map_or(best, |e| e.0);
    if sector::building_alive(world, first) { first } else { 0 }
}

/// 0x444d20: a task force's order target is an entity (flag 0x1000) that still has the noted
/// unique id and is alive.
pub(crate) fn entity_target_ok(world: &World, t: &super::model::TaskForce) -> bool {
    if t.flags() & flag::ENTITY == 0 {
        return false;
    }
    world.entities.by_index(t.target().max(0) as u32).is_some_and(|e| e.uid == t.ints[2] as u32 && e.flags & crate::entity::flag::ALIVE_MASK != 0)
}

/// 0x433120 `(type, x, y)`: of player 1's buildings of `type` (`CBuildingMgr` 0x14eebc0 + 4 ·
/// type: the list of player 1, hard-coded), the READY one nearest `(x, y)` (hex distance of its
/// ensign, below 0x4000, the first of equals) whose ensign lies on the sector of `(x, y)`; 0 none.
fn nearest_of_player_1(world: &World, kind: i32, x: i32, y: i32) -> u16 {
    let sector = sector::sector_at(world, x, y);
    let (mut best, mut bd) = (0u16, 0x4000);
    for b in world.building_list(1, kind) {
        let Some(e) = world.entities.by_index(b as u32) else { continue };
        if e.flags & crate::entity::flag::READY == 0 {
            continue;
        }
        let Some(en) = sector::ensign(world, b) else { continue };
        if sector::sector_at(world, en.x(), en.y()) != sector {
            continue;
        }
        let d = crate::spiral::grid_distance(en.x() - x, en.y() - y);
        if d < bd {
            bd = d;
            best = b;
        }
    }
    best
}

/// `CAIAgentGlobalSuicideMission::Execute(now)` (0x433210, the player AI's agent, period 0x96) on
/// the player AI's squad task force (+0x238, filled by `AI.NewSquad`): without members its period
/// · 10 at cost 1. The target is picked as [`suicide_target`] does (the order-4 target within 10,
/// else the nearest enemy military building within 0x20 if more than 10 nearer); without one,
/// player 1's nearest READY small tower, else large tower, else castle on the sector (0x433120,
/// three tries); without any the squad stops (order 1 to its first member) and the agent waits
/// its period · 10 at cost 0x200. With a target it attacks (order 4) unless already doing so
/// (same target, not flagged, ordered less than 1000 ticks ago); after a fallback pick it also
/// waits period · 10 at cost 0x200, else 0.
pub fn global_suicide(world: &mut World, p: u8, now: u32) -> u32 {
    let r = TfRef::Squad(p);
    let period = world.ai.players[p as usize].as_ref().map_or(super::sched::SUICIDE_PERIOD, |a| a.body.suicide.period);
    if tfs::count(world, r) < 1 {
        return period.wrapping_mul(0xa000) | 1;
    }
    let (x, y) = tfs::first_pos(world, r).map_or((-1, -1), |q| (q.x(), q.y()));
    let Some(tf) = super::player::tf(world, r).cloned() else { return 0 };
    let mut best = 0;
    let mut d = 0x4000;
    let mut near = false;
    if tf.flags() & 0xff0 == 0x40 && entity_target_ok(world, &tf) {
        best = tf.target();
        if sector::building_alive(world, best) {
            let (ex, ey) = sector::ensign(world, best as u16).map_or((0xffff, -1), |e| (e.x(), e.y()));
            d = crate::spiral::grid_distance(ex - x, ey - y);
            near = d < 0xb;
        }
    }
    if !near {
        let mask = world.alliances.enemy_bits[p.min(8) as usize];
        let (b, bd) = nearest_enemy_building(world, x, y, 0x20, mask);
        if b != 0 && bd < d - 10 {
            best = b as i32;
        }
    }
    let mut fallback = false;
    if best < 1 {
        fallback = true;
        for kind in [0x2e, 0x2f, 0x30] {
            best = nearest_of_player_1(world, kind, x, y) as i32;
            if best != 0 {
                break;
            }
        }
        if best <= 0 {
            tfs::order(world, r, 1, (y << 16).wrapping_add(x), 0);
            return period.wrapping_mul(0xa000) | 0x200;
        }
    }
    if tf.flags() & 0xff0 != 0x40 || tf.target() != best || tf.flags() & flag::RETARGET != 0 || tf.order_tick().wrapping_add(1000) <= now {
        tfs::order(world, r, 4, best, 0);
    }
    if fallback { period.wrapping_mul(0xa000) | 0x200 } else { 0 }
}
