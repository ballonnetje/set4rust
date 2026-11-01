//! The normal sector AI's squads: the squad management (`CAIAgentSquads`, slot 10, period 100,
//! Execute 0x4339d0), the tower occupation (slot 12, 0x432930), the moving home (slot 6,
//! 0x432a80) and the suicide missions (slot 9, 0x4329d0). Own reverse engineering of the shipped
//! exe (disassembly in `tools/re/decompiled/script/ai/s2/`); no AI save exists to check against.
//!
//! Squads (group task forces of type 5) guard the player's own military buildings on the sector:
//! each is ordered (2) to one building of the "desire list" built from goal caches 0 (threat) and
//! 1 (exposure); a squad whose building left the list is disbanded into the slots. Squads are
//! made and topped up from the slots (swordsmen 2, bowmen 3, specialists 4, leaders 5) up to
//! `SquadMinFighters + SquadMinExtraFighters · squads`; one may move to a more threatened
//! building. The run ends with the soldier wishes for the eco-sector AIs (event 0x1e) and the
//! attack agent's soldier count and strength.

use super::attack::{self, kind};
use super::player::TfRef;
use super::sector::{self, cache};
use super::taskforce::{self as tfs, class};
use crate::World;

/// The `AI` section's `CAIConfigInt`s (static initialisers 0x414ee0 / 0x414fa0 / 0x415040; no
/// config file of the install sets them).
pub mod config {
    /// `SquadMinFighters` (0x117d10c).
    pub const MIN_FIGHTERS: i32 = 10;
    /// `SquadMinExtraFighters` (0x117d118).
    pub const MIN_EXTRA_FIGHTERS: i32 = 2;
    /// `SquadMaxRecruits` (0x117d124).
    pub const MAX_RECRUITS: i32 = 5;
}

/// One building of the desire list (0x439110 entries, 0x14 bytes, at most 48).
#[derive(Clone, Copy, Debug)]
struct Desire {
    id: u16,
    /// +2: 1 a squad guards it, 2 a new squad was sent.
    flags: u8,
    /// +4 exposure, +8 threat, +0xc the other warriors' strength at it.
    a: i32,
    b: i32,
    c: i32,
    /// +0x10.
    tf: Option<TfRef>,
}

/// 0x439110 `(id, a, b, c)`: an entry already listed keeps the larger of each value; a new one is
/// added while there are fewer than 0x30.
fn merge(list: &mut Vec<Desire>, id: i32, a: i32, b: i32, c: i32) {
    if let Some(e) = list.iter_mut().find(|e| e.id as i32 == id) {
        e.a = e.a.max(a);
        e.b = e.b.max(b);
        e.c = e.c.max(c);
        return;
    }
    if list.len() < 0x30 {
        list.push(Desire { id: id as u16, flags: 0, a, b, c, tf: None });
    }
}

fn slot_count(world: &World, p: u8, s: u8, k: u8) -> i32 {
    tfs::count(world, TfRef::Slot(p, s, k))
}

/// 0x43fa50 `(task force)`: each member with a warrior type goes to the slot of its type.
pub(crate) fn back_to_slots(world: &mut World, p: u8, s: u8, t: TfRef) {
    for m in tfs::members(world, t) {
        let wt = world.entities.by_index(m as u32).map_or(0, |e| e.flags & 0xf) as u8;
        if wt != 0 && super::player::tf(world, TfRef::Slot(p, s, wt)).is_some() {
            super::player::add_to(world, m, TfRef::Slot(p, s, wt));
        }
    }
}

fn entity_pos(world: &World, id: i32) -> i32 {
    world.entities.by_index(id.max(0) as u32).map_or(0, |e| e.pos.0 as i32)
}

/// `CAIAgentSquads::Execute` (0x4339d0).
pub fn squads(world: &mut World, p: u8, s: u8) -> u32 {
    sector::cache_clean(world, p, s, cache::THREAT);
    sector::cache_clean(world, p, s, cache::EXPOSED);
    let mut list: Vec<Desire> = Vec::new();
    let c0 = sector::caches(world, p, s).map_or(Vec::new(), |c| c[cache::THREAT].entries());
    for (id, _, v) in c0 {
        // The building's AI info and its `CAIEntityInfoTower`, made when missing: +0x10.
        let other = sector::info_ext(world, id as u16).map_or(0, |i| i.tower.get_or_insert([0; 3])[2]);
        if other > 0 || v > 0 {
            merge(&mut list, id, i32::MIN, v, other);
        }
    }
    let c1 = sector::caches(world, p, s).map_or(Vec::new(), |c| c[cache::EXPOSED].entries());
    for (id, _, v) in c1 {
        merge(&mut list, id, v, i32::MIN, 0);
    }
    let n5 = attack::count_of_kind(world, p, s, kind::SQUAD);
    let base = config::MIN_FIGHTERS + config::MIN_EXTRA_FIGHTERS * n5;
    let (mut assigned, mut leaderless, mut maxm, mut deficit, mut under) = (0i32, 0i32, 0i32, 0i32, 0i32);
    let mut largest: Option<TfRef> = None;
    let (mut sw, mut bw, mut sp, mut ld) = (0, 0, 0, 0);
    for t in tfs::list(world, p, s, class::SQUAD) {
        if tfs::kind_of_ref(world, t) != kind::SQUAD {
            continue;
        }
        let m = tfs::count(world, t);
        if m > 0 {
            let x = super::player::tf(world, t).cloned().unwrap_or_default();
            let k = (x.flags() & tfs::flag::ENTITY != 0 && x.target() > 0).then(|| list.iter().position(|e| e.id as i32 == x.target())).flatten();
            if let Some(k) = k {
                assigned += 1;
                list[k].flags |= 1;
                list[k].tf = Some(t);
                sw += tfs::warriors(world, t, 2);
                bw += tfs::warriors(world, t, 3);
                sp += tfs::warriors(world, t, 4);
                let l = tfs::warriors(world, t, 5);
                ld += l;
                if l <= 0 {
                    leaderless += 1;
                }
                if maxm < m {
                    maxm = m;
                    largest = Some(t);
                }
                if m < base {
                    under += 1;
                    deficit += base - m;
                }
                continue;
            }
            tfs::order(world, t, 0, 0, 0);
            back_to_slots(world, p, s, t);
            assigned -= 1;
        }
        tfs::destroy(world, t);
    }
    let unassigned = list.iter().filter(|e| e.flags & 1 == 0).count() as i32;
    let threatened = list.iter().filter(|e| e.flags & 1 == 0 && e.c > 0).count() as i32;
    let (s2, s3, s4, s5) = (slot_count(world, p, s, 2), slot_count(world, p, s, 3), slot_count(world, p, s, 4), slot_count(world, p, s, 5));
    let sw_t = sw + s2;
    let free = s2 + s3;
    let bw_t = bw + s3;
    let sp_t = s4 + sp;
    let ld_t = ld + s5;
    if free < base && maxm > 2 * base && unassigned > 0 && let Some(t) = largest {
        back_to_slots(world, p, s, t);
        tfs::destroy(world, t);
        return 0x5000;
    }
    let need = if n5 == 0 && free > 0 { free.min(base) } else { base };
    if free - deficit >= need
        && unassigned > 0
        && let Some(t) = tfs::create(world, p, s, kind::SQUAD)
    {
        let k = list.iter().position(|e| e.flags & 1 == 0).unwrap_or(0);
        tfs::order(world, t, 2, list[k].id as i32, 0);
        deficit += base;
        under += 1;
        list[k].flags |= 2;
        list[k].tf = Some(t);
    }
    let (mut base2, mut deficit2, mut under2) = (base, deficit, under);
    if free > 0 && defence_state(world, p, s) >= 2 && deficit == 0 {
        under2 = assigned;
        base2 = 99999;
        deficit2 = free;
    }
    if free > 0 && under2 > 0 {
        let q = deficit2.max(free) / under2;
        if q > 0 {
            let q = q.min(config::MAX_RECRUITS);
            for t in tfs::list(world, p, s, class::SQUAD) {
                if tfs::kind_of_ref(world, t) != kind::SQUAD || tfs::count(world, t) >= base2 {
                    continue;
                }
                let x = super::player::tf(world, t).cloned().unwrap_or_default();
                let target = if x.flags() & tfs::flag::ENTITY != 0 { x.target() } else { 0 };
                fill_squad(world, p, s, t, entity_pos(world, target), q);
                tfs::order(world, t, 2, target, 0);
            }
        }
    }
    reassign(world, &list, threatened);
    priests(world, p, s);
    escorts(world, p, s);
    let total = ld_t + sp_t + bw_t + sw_t;
    let ev = super::evaluation::of(world, p);
    let strength = if total > 0 { single_mul_div(ev[15], ev[16], total) } else { 0 };
    attack::set_int(world, p, s, 1, total);
    attack::set_int(world, p, s, 2, strength);
    for k in [cache::WEAK, cache::NEAR, cache::LAND] {
        sector::cache_clean(world, p, s, k);
    }
    let diff = sw_t - 2 * bw_t;
    let (mut sw_wish, mut bw_wish) = match diff {
        d if d < -12 => (16, 0),
        d if d < -6 => (12, 4),
        d if d <= 6 => (12, 12),
        d if d <= 12 => (4, 12),
        _ => (0, 16),
    };
    let level = world.ai.vars.records.get(p as usize).map_or(0, |r| r.level);
    let mut spec = 0;
    if level != 1 && sw_t > 10 && sp_t < (sw_t + bw_t) / 8 {
        spec = (world.rng.rand() & 3) as i32 + 1;
    }
    let lead = ((leaderless - s5) / 2).clamp(0, 2);
    let fighters = sw_t + bw_t;
    let ad = diff.abs();
    if sw_t > 15 && bw_t > 10 && (ad <= 30 || fighters > 120) && (ad <= 45 || fighters > 240) && ad <= 60 {
        let rec = world.ai.vars.records.get(p as usize).copied().unwrap_or_default();
        let (abs, rel) = (rec.get(5), rec.get(6));
        if abs > 0 && total > abs {
            let stop = if rel > 0 {
                let team = world.alliances.team(p);
                let sum: i32 = (1..=8u8).filter(|&q| team != 0 && world.alliances.team(q) == team).map(|q| {
                    let e = super::evaluation::of(world, q);
                    e[13] + e[4]
                }).sum();
                total > rel.wrapping_mul(sum) / 100
            } else {
                true
            };
            if stop {
                sw_wish = 0;
                bw_wish = 0;
                spec = 0;
            }
        }
    }
    if world.race_of(p) == 0 {
        // The recruitment agent's +0x28 (Roman medics).
        if let Some(a) = world.ai.players[p as usize].as_mut().and_then(|a| super::sched::Agents::agent(&mut a.body, super::sched::AgentRef::Sector(s, sector::agent::RECRUITMENT))) {
            a.state = spec;
        }
        if spec != 0 {
            sw_wish += spec;
            spec = 0;
        }
    }
    sector::post_eco(world, p, s, bw_wish << 16 | sw_wish, lead << 16 | spec);
    if slot_count(world, p, s, 2) + slot_count(world, p, s, 3) > 10 { 0xa000 } else { 0 }
}

/// The defence evaluation agent's saved int (+0x28, sector AI +0xb68). [inf] No writer found in
/// the normal AI code; 0 unless a load brings another value.
fn defence_state(world: &World, p: u8, s: u8) -> i32 {
    world.ai.players[p as usize]
        .as_ref()
        .and_then(|a| super::sched::Agents::get(&a.body, super::sched::AgentRef::Sector(s, sector::agent::DEFENCE)))
        .and_then(|a| a.extra.first().copied())
        .unwrap_or(0)
}

/// The tail of 0x4339d0: one squad may move to another building. With threatened buildings
/// unguarded: from the guarded building with the least other strength (the last of equals) to the
/// unguarded one with the highest threat (the first of equals). Otherwise: from a guarded one
/// whose squad is in phase 4 (arrived; not since this tick) with the lowest `(rand + exposure +
/// threat) & 0xff` (the last of equals), to an unguarded one within 100 with the highest such
/// score (the first). The squad goes (order 2) when the other strength there is at least twice
/// its own building's and — without threatened ones — it is not walking (phase 2) and, in phase
/// 4, has been so for more than 0x4b ticks.
fn reassign(world: &mut World, list: &[Desire], threatened: i32) {
    let now = world.ai.tick;
    let mut from: i32 = -1;
    let mut to: i32 = -1;
    if threatened > 0 {
        let (mut minc, mut maxb) = (i32::MAX, i32::MIN);
        for (k, e) in list.iter().enumerate() {
            if e.flags & 3 != 0 {
                if e.c <= minc {
                    minc = e.c;
                    from = k as i32;
                }
            } else if e.b > maxb {
                maxb = e.b;
                to = k as i32;
            }
        }
    } else {
        let mut min = i32::MAX;
        for (k, e) in list.iter().enumerate() {
            let Some(t) = e.tf.filter(|_| e.flags & 3 != 0) else { continue };
            let Some(x) = super::player::tf(world, t) else { continue };
            if x.phase() != 4 || (x.ints[4] as u32).wrapping_sub(now) <= 0x4b {
                continue;
            }
            let score = (world.rng.rand() as i32).wrapping_add(e.a).wrapping_add(e.b) & 0xff;
            if min >= score {
                min = score;
                from = k as i32;
            }
        }
        if from < 0 {
            return;
        }
        let pos = crate::grid::Y16X16(entity_pos(world, list[from as usize].id as i32) as u32);
        let mut max = i32::MIN;
        for (k, e) in list.iter().enumerate() {
            if e.flags & 3 != 0 {
                continue;
            }
            let q = crate::grid::Y16X16(entity_pos(world, e.id as i32) as u32);
            if crate::spiral::grid_distance(q.x() - pos.x(), q.y() - pos.y()) < 100 {
                let score = (world.rng.rand() as i32).wrapping_add(e.a).wrapping_add(e.b) & 0xff;
                if max < score {
                    max = score;
                    to = k as i32;
                }
            }
        }
    }
    if from < 0 || to < 0 || from == to {
        return;
    }
    let (f, t) = (list[from as usize], list[to as usize]);
    let Some(tf) = f.tf else { return };
    let ok = threatened > 0 || {
        let x = super::player::tf(world, tf).cloned().unwrap_or_default();
        match x.phase() {
            2 => false,
            4 => (x.ints[4] as u32).wrapping_add(0x4b) < now,
            _ => true,
        }
    };
    if ok && t.c >= f.c.wrapping_mul(2) {
        tfs::order(world, tf, 2, t.id as i32, 0);
    }
}

/// 0x43fd00 `(task force, pos, q)`: up to `q` (at most the free swordsmen and bowmen) more
/// fighters from the slots nearest `pos` (0x43fbd0): bowmen `max(0, (swordsmen / 2 − bowmen + q)
/// / 2)` and swordsmen the rest, each limited by its slot (the shortfall goes to the other kind);
/// specialists up to `(swordsmen + bowmen) / 6` minus those it has (at most 4 and the slot's);
/// a leader (the nearest, 0x43fab0) when it has none.
fn fill_squad(world: &mut World, p: u8, s: u8, t: TfRef, pos: i32, q: i32) {
    let c2 = tfs::warriors(world, t, 2);
    let c3 = tfs::warriors(world, t, 3);
    let c4 = tfs::warriors(world, t, 4);
    let c5 = tfs::warriors(world, t, 5);
    let (s2, s3, s4, s5) = (slot_count(world, p, s, 2), slot_count(world, p, s, 3), slot_count(world, p, s, 4), slot_count(world, p, s, 5));
    let q = q.min(s2 + s3);
    let a = ((c2 / 2 - c3 + q) / 2).max(0);
    let b = q - a;
    let (n2, n3) = if a > s2 {
        (s2, b + (a - s2))
    } else if b > s3 {
        (a + (b - s3), s3)
    } else {
        (a, b)
    };
    super::dark_agents::fill(world, t, p, s, pos, 2, n2);
    super::dark_agents::fill(world, t, p, s, pos, 3, n3);
    let k = (c2 + c3) / 6 - c4;
    if k > 0 && s4 > 0 {
        super::dark_agents::fill(world, t, p, s, pos, 4, k.min(4).min(s4));
    }
    if c5 < 1 && s5 >= 1 {
        sector::nearest_to(world, p, s, crate::grid::Y16X16(pos as u32), 5, t);
    }
}

/// 0x4337e0: empty priest task forces (class 3) are destroyed; the priests of slot 6 join priest
/// task forces (type 8) of fewer than 2 that are unlinked or escort a squad (type 5), then new
/// ones of up to 2.
fn priests(world: &mut World, p: u8, s: u8) {
    for t in tfs::list(world, p, s, 3) {
        if tfs::count(world, t) == 0 {
            tfs::destroy(world, t);
        }
    }
    let mut members = tfs::members(world, TfRef::Slot(p, s, 6)).into_iter().peekable();
    let tfl = tfs::list(world, p, s, 3);
    let mut i = 0;
    let mut cur: Option<TfRef> = tfl.first().copied();
    while let (Some(t), Some(&m)) = (cur, members.peek()) {
        let link_ok = tfs::linked(world, t).is_none_or(|l| tfs::kind_of_ref(world, l) == kind::SQUAD);
        if tfs::kind_of_ref(world, t) != kind::PRIESTS || tfs::count(world, t) > 1 || !link_ok {
            i += 1;
            cur = tfl.get(i).copied();
            continue;
        }
        super::player::add_to(world, m, t);
        members.next();
    }
    for m in members {
        if cur.is_none_or(|t| tfs::count(world, t) > 1) {
            cur = tfs::create(world, p, s, kind::PRIESTS);
        }
        if let Some(t) = cur {
            super::player::add_to(world, m, t);
        }
    }
}

/// 0x433930: each unlinked priest task force escorts an unlinked squad (type 5): linked, ordered
/// to follow (6, [`super::priests`]).
fn escorts(world: &mut World, p: u8, s: u8) {
    loop {
        let pr = tfs::list(world, p, s, 3).into_iter().find(|&t| tfs::linked(world, t).is_none());
        let sq = tfs::list(world, p, s, class::SQUAD).into_iter().find(|&t| tfs::kind_of_ref(world, t) == kind::SQUAD && tfs::linked(world, t).is_none());
        let (Some(TfRef::Group(_, _, a)), Some(TfRef::Group(_, _, b))) = (pr, sq) else { return };
        if let Some(sec) = world.ai.players[p as usize].as_mut().and_then(|x| x.body.sectors.get_mut(s as usize)) {
            sec.group.link(a, b);
        }
        let Some(pr) = pr else { return };
        tfs::order(world, pr, 6, -1, 0);
    }
}

/// `CAIAgentTowerOccupation::Execute` (0x432930): each member of reservoir 4 (swordsmen sent to
/// towers by 0x441de0) whose partner building no longer names it (no partner, the building gone
/// or without an AI info, or another partner) goes back to the slot of its warrior type. Returns 0,
/// or cost 0x20 when the reservoir is empty.
pub fn towers(world: &mut World, p: u8, s: u8) -> u32 {
    let r = TfRef::Force(p, s, sector::force::OCCUPY);
    let members = tfs::members(world, r);
    if members.is_empty() {
        return 0x20;
    }
    for m in members {
        let b = sector::partner(world, m);
        let ok = b != 0 && sector::has_info(world, b) && sector::partner(world, b) == m;
        if !ok {
            let wt = world.entities.by_index(m as u32).map_or(0, |e| e.flags & 0xf) as u8;
            if wt != 0 && super::player::tf(world, TfRef::Slot(p, s, wt)).is_some() {
                super::player::add_to(world, m, TfRef::Slot(p, s, wt));
            }
        }
    }
    0
}

/// `CAIAgentMovingHome::Execute` (0x432a80): without task forces of type 3, its period · 10 at
/// cost 1. Otherwise each one of type 3: empty ones are destroyed; a member (from the last) not
/// yet on the anchor's eco-sector moves to the back and the force is sent home again (0x443d90);
/// when all are home it becomes a squad (type 5) and its order is cleared.
pub fn moving_home(world: &mut World, p: u8, s: u8) -> u32 {
    if attack::count_of_kind(world, p, s, kind::HOME) == 0 {
        let period = super::model::NORMAL_AGENTS[sector::agent::MOVING_HOME as usize].2;
        return period.wrapping_mul(0xa000) | 1;
    }
    let (ax, ay) = sector::anchor(world, p, s);
    let home = world.grid.checked_index(ax, ay).map_or(0, |n| world.tiling.eco_sector_of(n));
    for t in tfs::list(world, p, s, class::SQUAD) {
        if tfs::kind_of_ref(world, t) != kind::HOME {
            continue;
        }
        if tfs::count(world, t) == 0 {
            tfs::destroy(world, t);
            continue;
        }
        let away = tfs::members(world, t).into_iter().rev().find(|&m| {
            world.entities.by_index(m as u32).is_some_and(|e| world.grid.checked_index(e.x(), e.y()).map_or(0, |n| world.tiling.eco_sector_of(n)) != home)
        });
        if let Some(m) = away {
            if let Some(x) = super::player::tf_mut(world, t)
                && x.remove_member(m)
            {
                x.members.push(m);
            }
            sector::send_home(world, p, s, t);
            continue;
        }
        sector::set_kind(world, t, kind::SQUAD);
        tfs::order(world, t, 0, 0, 0);
    }
    0
}

/// `CAIAgentSuicideMissions::Execute(now)` (0x4329d0): without task forces of type 4, its period
/// · 10 at cost 1. Otherwise each one of type 4: empty ones are destroyed; with a target
/// ([`attack::suicide_target`]) it attacks it (order 4) unless already doing so (same target, not
/// flagged, ordered less than 1000 ticks ago); without one it goes home.
pub fn suicide_missions(world: &mut World, p: u8, s: u8, now: u32) -> u32 {
    if attack::count_of_kind(world, p, s, kind::SUICIDE) == 0 {
        let period = super::model::NORMAL_AGENTS[sector::agent::SUICIDE as usize].2;
        return period.wrapping_mul(0xa000) | 1;
    }
    for t in tfs::list(world, p, s, class::SQUAD) {
        if tfs::kind_of_ref(world, t) != kind::SUICIDE {
            continue;
        }
        if tfs::count(world, t) == 0 {
            tfs::destroy(world, t);
            continue;
        }
        let target = attack::suicide_target(world, p, s, t);
        if target < 1 {
            sector::send_home(world, p, s, t);
            continue;
        }
        let x = super::player::tf(world, t).cloned().unwrap_or_default();
        if x.flags() & 0xff0 != 0x40 || x.target() != target || x.flags() & tfs::flag::RETARGET != 0 || x.order_tick().wrapping_add(1000) <= now {
            tfs::order(world, t, 4, target, 0);
        }
    }
    0
}

/// The settler types the intruder combat watches, in its order (.rdata 0xff0438): saboteur,
/// thief, geologist, pioneer, gardener, priest.
const INTRUDERS: [u16; 6] = [46, 48, 49, 47, 50, 45];

/// 0x5b17e0 `(player, x, y)`: 0 when the node is the player's land; 1 when, from a node of a
/// 16×16 square whose neighbourhood holds the player's land (owner-map byte 1) and that is the
/// player's (byte 0) or lies within 4 of the square's edge, the player's land is at most 4 steps
/// away in one of the six directions (0x1454108 flat offsets inside a margin of 4, 0x101bd88 near
/// the map edge); −1 otherwise.
pub(crate) fn near_own_land(world: &World, p: u8, x: i32, y: i32) -> i32 {
    let owner = |x: i32, y: i32| world.grid.checked_index(x, y).map(|n| world.tiling.owner_of(n));
    if owner(x, y) == Some(p) {
        return 0;
    }
    let bit = crate::alliances::player_bit(p) as u8;
    let o = world.tiling.owner_bits(x >> 4, y >> 4);
    let inner = ((y & 0xf) - 4) as u32 <= 7 && ((x & 0xf) - 4) as u32 <= 7;
    if o[1] & bit != 0 && (o[0] & bit != 0 || !inner) {
        for (dx, dy) in [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)] {
            for k in 1..=4 {
                match owner(x + dx * k, y + dy * k) {
                    None => break,
                    Some(o) if o == p => return 1,
                    _ => {}
                }
            }
        }
    }
    -1
}

fn set_partner(world: &mut World, id: u16, v: u16) {
    if let Some(i) = sector::info_ext(world, id) {
        i.partner = v;
    }
}

/// 0x431f60 `(type)`: the enemies' settlers of `type` (`CSettlerMgr`'s warrior lists 0x1544cec of
/// the players not in our team, 0x12dcee0; alive, not aboard, on the sector; 0x436660) that are
/// on or near our land (0x5b17e0). Each gets a hunter from reservoir 2: the one already after it;
/// else the nearest free one within 39 (counted); else, while the reservoir has fewer than 10, the
/// nearest swordsman of a squad (type 5) with more than 5 members (at most 5 swordsmen looked at
/// per squad, a squad's search ends at one within 30, the squads' search at one within 39), who
/// joins the reservoir's front. The hunter is told (its info's partner) and walks to the
/// intruder. Returns the intruders that got a free hunter of the reservoir.
fn hunt(world: &mut World, p: u8, s: u8, kind: u16) -> i32 {
    let r2 = TfRef::Force(p, s, 2);
    let sector = sector::sector_of(world, p, s);
    let enemies = world.alliances.enemy_bits[p.min(8) as usize];
    let mut targets = Vec::new();
    for q in 1..=8u8 {
        if enemies & crate::alliances::player_bit(q) == 0 {
            continue;
        }
        let mut id = world.settler_lists.head(q, kind);
        while id != 0 {
            let Some(e) = world.entities.by_index(id as u32) else { break };
            if e.flags & crate::entity::flag::ALIVE_MASK != 0 && e.flags & crate::entity::flag::ON_BOARD == 0 && sector::sector_at(world, e.x(), e.y()) == sector {
                targets.push(id);
            }
            id = e.next;
        }
    }
    let mut n = 0;
    for e in targets {
        let Some(ep) = world.entities.by_index(e as u32).map(|x| x.pos) else { continue };
        if near_own_land(world, p, ep.x(), ep.y()) < 0 {
            continue;
        }
        let mut hunter = 0u16;
        let (mut cand, mut best) = (0u16, 0x4000);
        for m in tfs::members(world, r2) {
            let partner = sector::partner(world, m);
            if partner == 0 {
                let Some(mp) = world.entities.by_index(m as u32).map(|x| x.pos) else { continue };
                let d = crate::spiral::grid_distance(ep.x() - mp.x(), ep.y() - mp.y());
                if d < best {
                    best = d;
                    cand = m;
                }
            } else if partner == e {
                hunter = m;
                break;
            }
        }
        if hunter == 0 {
            if cand != 0 && best < 0x28 {
                n += 1;
                hunter = cand;
            } else {
                if tfs::count(world, r2) >= 10 {
                    continue;
                }
                let (mut best, mut pick) = (0x4000, 0u16);
                for t in tfs::list(world, p, s, class::SQUAD) {
                    if tfs::kind_of_ref(world, t) == kind::SQUAD && tfs::count(world, t) > 5 {
                        let mut k = 0;
                        for m in tfs::members(world, t) {
                            let Some(me) = world.entities.by_index(m as u32) else { continue };
                            if me.flags & 0xf != 2 {
                                continue;
                            }
                            let d = crate::spiral::grid_distance(me.x() - ep.x(), me.y() - ep.y());
                            if d < best {
                                best = d;
                                pick = m;
                            }
                            k += 1;
                            if d < 0x1e || k >= 5 {
                                break;
                            }
                        }
                    }
                    if best < 0x28 {
                        break;
                    }
                }
                if pick == 0 {
                    continue;
                }
                // vtbl+0x14 mode 1: to the front.
                super::player::add_to(world, pick, r2);
                if let Some(x) = super::player::tf_mut(world, r2)
                    && x.remove_member(pick)
                {
                    x.members.insert(0, pick);
                }
                hunter = pick;
            }
        }
        if hunter != 0 && sector::has_info(world, hunter) {
            set_partner(world, hunter, e);
            tfs::move_entity(world, hunter, ep.x(), ep.y(), true);
        }
    }
    n
}

/// 0x431e20 `(entity)`: back to the player's nearest READY military building within 0x20 (its
/// ensign; 0x5b1610 with the own bit), else the anchor; data0 4.
fn go_back(world: &mut World, p: u8, s: u8, id: u16) {
    let Some(pos) = world.entities.by_index(id as u32).map(|e| e.pos) else { return };
    let (mut x, mut y) = sector::anchor(world, p, s);
    let (b, _) = attack::nearest_enemy_building(world, pos.x(), pos.y(), 0x20, crate::alliances::player_bit(p));
    if b > 0 {
        (x, y) = sector::ensign(world, b).map_or((0xffff, -1), |e| (e.x(), e.y()));
    }
    tfs::move_entity(world, id, x, y, false);
}

fn intruder_step(world: &mut World, p: u8, s: u8) -> u16 {
    let r = super::sched::AgentRef::Sector(s, sector::agent::INTRUDERS);
    let Some(a) = world.ai.players[p as usize].as_mut().and_then(|a| super::sched::Agents::agent(&mut a.body, r)) else { return INTRUDERS[0] };
    a.state += 1;
    if a.state as u32 > 5 {
        a.state = 0;
    }
    INTRUDERS[a.state as usize]
}

/// `CAIAgentIntruderCombat::Execute` (0x431bd0, period 0x25): the hunters of reservoir 2, from
/// the last: one whose partner is gone or no longer on or near our land (0x5b17e0) is freed; one
/// still after its partner walks to it; a free one moves to the back (vtbl+0x14 mode 2) and
/// counts. Then three intruder types (the agent's +0x28 cycles over [`INTRUDERS`]) are hunted
/// ([`hunt`]); when the free hunters minus those the first two types took exceed those of the
/// third, every free hunter goes back (0x431e20) and returns to its slot. Returns 0.
pub fn intruders(world: &mut World, p: u8, s: u8) -> u32 {
    let r2 = TfRef::Force(p, s, 2);
    let mut free = 0;
    for m in tfs::members(world, r2).into_iter().rev() {
        let partner = sector::partner(world, m);
        if partner != 0 {
            let alive = world.entities.by_index(partner as u32).filter(|e| e.flags & crate::entity::flag::ALIVE_MASK != 0).map(|e| e.pos);
            match alive {
                Some(pp) if near_own_land(world, p, pp.x(), pp.y()) >= 0 => {
                    tfs::move_entity(world, m, pp.x(), pp.y(), true);
                    continue;
                }
                _ => set_partner(world, m, 0),
            }
        }
        if sector::has_info(world, m)
            && let Some(x) = super::player::tf_mut(world, r2)
            && x.remove_member(m)
        {
            x.members.push(m);
        }
        free += 1;
    }
    let k = intruder_step(world, p, s);
    let a = hunt(world, p, s, k);
    let k = intruder_step(world, p, s);
    let b = hunt(world, p, s, k);
    let d = free - a - b;
    let k = intruder_step(world, p, s);
    let c = hunt(world, p, s, k);
    if d != c && d - c >= 0 {
        for m in tfs::members(world, r2) {
            if sector::partner(world, m) == 0 {
                go_back(world, p, s, m);
                back_to_slots_one(world, p, s, m);
            }
        }
    }
    0
}

/// A member with a warrior type to the slot of its type (vtbl+0x14 mode 0).
fn back_to_slots_one(world: &mut World, p: u8, s: u8, m: u16) {
    let wt = world.entities.by_index(m as u32).map_or(0, |e| e.flags & 0xf) as u8;
    if wt != 0 && super::player::tf(world, TfRef::Slot(p, s, wt)).is_some() {
        super::player::add_to(world, m, TfRef::Slot(p, s, wt));
    }
}

/// A value of IEEE single precision as `(negative, mantissa below 2^24, exponent)`.
type Single = (bool, u128, i32);

/// Rounds `m · 2^e` (with `sticky`: a bit of value below the last one of `m` is set) to 24
/// significant bits, to nearest, ties to even.
fn round24(neg: bool, m: u128, e: i32, sticky: bool) -> Single {
    let len = 128 - m.leading_zeros() as i32;
    if len <= 24 {
        return (neg, m, e);
    }
    let s = len - 24;
    let mut q = m >> s;
    let rem = m & ((1u128 << s) - 1);
    let half = 1u128 << (s - 1);
    if rem > half || (rem == half && (sticky || q & 1 == 1)) {
        q += 1;
    }
    if q == 1 << 24 {
        return (neg, q >> 1, e + s + 1);
    }
    (neg, q, e + s)
}

/// `cvtdq2ps`.
fn single(v: i32) -> Single {
    round24(v < 0, v.unsigned_abs() as u128, 0, false)
}

/// `cvttss2si(single(a) · single(b) / single(c))` as the squad agent computes the strength with
/// SSE single precision (0x43428b..0x4342b0), in integers: each step rounded to nearest even, the
/// result truncated; out of range gives 0x80000000. `c` is not 0.
pub fn single_mul_div(a: i32, b: i32, c: i32) -> i32 {
    let (na, ma, ea) = single(a);
    let (nb, mb, eb) = single(b);
    let (nc, mc, ec) = single(c);
    if ma == 0 || mb == 0 {
        return 0;
    }
    let (np, mp, ep) = round24(na != nb, ma * mb, ea + eb, false);
    let n = mp << 60;
    let (q, r) = (n / mc, n % mc);
    let (neg, m, e) = round24(np != nc, q, ep - 60 - ec, r != 0);
    let v = if e >= 0 { if e > 40 { u128::MAX } else { m << e } } else if -e >= 128 { 0 } else { m >> -e };
    if v > i32::MAX as u128 {
        return i32::MIN;
    }
    if neg { -(v as i32) } else { v as i32 }
}
