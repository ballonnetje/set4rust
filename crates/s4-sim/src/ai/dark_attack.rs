//! `CAIAgentDarkTribeAttack` (vtable 0xff0068, the first agent of a Dark Tribe sector AI): the
//! Dark Tribe's raids with shamans, their escort squads and manakopters. Own reverse engineering
//! of the shipped exe; decompiles in `tools/re/decompiled/script/ai/dtagents.c` (0x42cf00) and
//! `taskforce4.c` (the helpers; their task-force iterators were read from the disassembly).
//!
//! The agent's saved ints (version 2): +0x28 the step 0..7, +0x2c the players to raid (a bit
//! mask, bit `p − 1`; 1 = player 1 unless `AI.AttackNow` says otherwise), +0x30 the strength
//! asked for, +0x34 / +0x38 the target, +0x40 how many manakopters fly, +0x3c the tick before
//! which no new raid starts.
//!
//! It works on the sector AI's shaman task forces (class 7, their escort squads linked at +0x2c)
//! and manakopter task forces (class 8). The task-force iterator (0x42bc10) visits the task
//! forces of one class list with members whose order (+0x38 bits 4..11) is the one asked for.
//!
//! **Steps** (`Execute` 0x42cf00, returns the cost):
//! 0. with `AttackMode` ≥ 1, past +0x3c and past `AttackDelay` · 840 ticks, and a shaman task
//!    force with an escort (0x42c080): unless a strength is asked for already, players 1 and
//!    strength 1; with manakopter task forces a free manakopter is sought for the strongest
//!    shaman force (0x42c750 `(empty, false)`: order 10 board for it and its escort, which
//!    becomes type 7 and keeps at most `DTMaxSoldiersPerTaskForce`; none → no raid) and those
//!    forces (order 10) are marked (0x42c9f0); step 1. Cost 0xa000.
//! 1. a target for the raid (`CAIShaman` vtbl+0 from the anchor, [`super::shaman::target`];
//!    any sector when manakopters fly). With manakopters: once every marked force is done
//!    (0x42c160), they fly there (0x42c3e0 with spread, order 1) — step 2 with their count;
//!    without: the escorted shaman forces go to the edge of the enemy land (0x42c250, order 3),
//!    are marked (0x42c9f0 with order 3), step 2. No target: the raid ends.
//! 2. flying: when all of them are idle again (0x42c110) they unload (order 0xc), step 3 (cost
//!    0x10000); on foot: marked forces still on the way wait (0x42c970 < 0), the others with
//!    order 3 cast at the target (0x42c2e0, order 9), step 4; else the raid ends.
//! 3. the marked forces cast there (0x42c370: order 0xe, their escorts follow, order 6); step 4.
//! 4. per marked shaman force a target near it (0x42cb30: a pile of the goods the hall lacks, an
//!    enemy civilian within 32, else home); with manakopters when that is done they board again
//!    (0x42c750 `(false, true)`), step 5; without, step 6.
//! 5. once done (0x42c160) the manakopters fly home (0x42c3e0 without spread); none: the marked
//!    forces walk home (order 7); step 6.
//! 6. flying: when idle they land (order 0xb), step 7 (cost 0x10000); on foot: wait while a
//!    marked force is on the way, else the raid ends.
//! 7. the end: marks off, reset (0x42ca40, 0x42c010), the shaman forces with order 10 stop
//!    (order 0), their escorts become squads (type 2) again and follow (order 6).
//!
//! The reset (0x42c010): step 0, no players, strength −1, no manakopters, the next raid not
//! before `DTAttackCheckDelay` · 840 + 0..255 ticks (one game RNG draw).

use super::player::TfRef;
use super::taskforce::{self as tfs, class};
use crate::World;
use crate::grid::Y16X16;

/// The agent's index among the Dark Tribe sector AI's agents.
const AGENT: usize = 0;

/// The agent's saved ints by their offset (version 2 order: +0x28, +0x2c, +0x30, +0x34, +0x38,
/// +0x40, +0x3c).
#[derive(Clone, Copy, Debug)]
enum F {
    Step = 0,
    Players = 1,
    Strength = 2,
    X = 3,
    Y = 4,
    Flying = 5,
    Next = 6,
}

fn get(world: &World, p: u8, s: u8, f: F) -> i32 {
    super::dark_agents::agent(world, p, s, AGENT).and_then(|a| a.extra.get(f as usize).copied()).unwrap_or(0)
}

fn set(world: &mut World, p: u8, s: u8, f: F, v: i32) {
    if let Some(x) = super::dark_agents::agent_mut(world, p, s, AGENT).and_then(|a| a.extra.get_mut(f as usize)) {
        *x = v;
    }
}

fn sector(world: &World, p: u8, s: u8) -> Option<&super::model::SectorAi> {
    world.ai.players.get(p as usize)?.as_ref()?.body.sectors.get(s as usize)
}

/// The sector AI's anchor (+0xf4 / +0xf8) and its sector (+0x100).
fn anchor(world: &World, p: u8, s: u8) -> ((i32, i32), u16) {
    sector(world, p, s).map_or(((0, 0), 0), |x| (x.anchor(), x.sector()))
}

fn shamans(world: &World, p: u8, s: u8) -> Vec<TfRef> {
    tfs::list(world, p, s, class::SHAMANS)
}

fn manakopters(world: &World, p: u8, s: u8) -> Vec<TfRef> {
    tfs::list(world, p, s, class::MANAKOPTERS)
}

fn tf(world: &World, r: TfRef) -> Option<&super::model::TaskForce> {
    tfs::get(world, r)
}

fn count(world: &World, r: TfRef) -> i32 {
    tfs::count(world, r)
}

fn phase(world: &World, r: TfRef) -> u8 {
    tf(world, r).map_or(0, |t| t.phase())
}

fn marked(world: &World, r: TfRef) -> bool {
    tf(world, r).is_some_and(|t| t.flags() & tfs::flag::ATTACK != 0)
}

/// 0x42bc10 over a list: the task forces with members and the order `cmd`.
fn with_order(world: &World, list: Vec<TfRef>, cmd: u32) -> Vec<TfRef> {
    list.into_iter().filter(|&r| count(world, r) >= 1 && tf(world, r).is_some_and(|t| t.command() == cmd)).collect()
}

fn pack(x: i32, y: i32) -> i32 {
    y.wrapping_shl(16).wrapping_add(x)
}

/// A manakopter task force with members.
fn any_flying(world: &World, p: u8, s: u8) -> bool {
    manakopters(world, p, s).into_iter().any(|r| count(world, r) >= 1)
}

/// 0x42c080: a shaman task force with members and an escort.
fn escorted_shamans(world: &World, p: u8, s: u8) -> bool {
    shamans(world, p, s).into_iter().any(|r| count(world, r) >= 1 && tfs::linked(world, r).is_some())
}

/// 0x42c160: every marked shaman force with members is done (phase 8), its escort too.
fn marked_done(world: &World, p: u8, s: u8) -> bool {
    shamans(world, p, s).into_iter().filter(|&r| count(world, r) >= 1 && marked(world, r)).all(|r| phase(world, r) == 8 && tfs::linked(world, r).is_none_or(|l| phase(world, l) == 8))
}

/// 0x42c9b0: every marked shaman force with members is in phase 10.
fn marked_flying(world: &World, p: u8, s: u8) -> bool {
    shamans(world, p, s).into_iter().filter(|&r| count(world, r) >= 1 && marked(world, r)).all(|r| phase(world, r) == 10)
}

/// 0x42c970: the marked shaman forces with members, −1 as soon as one is moving (phase 2).
fn marked_walking(world: &World, p: u8, s: u8) -> i32 {
    let mut n = 0;
    for r in shamans(world, p, s) {
        if count(world, r) >= 1 && marked(world, r) {
            n += 1;
            if phase(world, r) == 2 {
                return -1;
            }
        }
    }
    n
}

/// 0x42c110: every manakopter force with order 1 is idle (phase 1).
fn flights_idle(world: &World, p: u8, s: u8) -> bool {
    with_order(world, manakopters(world, p, s), 1).into_iter().all(|r| phase(world, r) == 1)
}

/// 0x42c0b0 `(flying)`: a manakopter force with members (in phase 10 with `flying`).
fn manakopter_force(world: &World, p: u8, s: u8, flying: bool) -> bool {
    manakopters(world, p, s).into_iter().any(|r| count(world, r) >= 1 && (!flying || phase(world, r) == 10))
}

/// 0x42c9f0 `(cmd)`: the shaman forces with order `cmd` are marked.
fn mark(world: &mut World, p: u8, s: u8, cmd: u32) {
    for r in with_order(world, shamans(world, p, s), cmd) {
        if let Some(t) = super::player::tf_mut(world, r) {
            t.set_flags(t.flags() | tfs::flag::ATTACK);
        }
    }
}

/// 0x42ca40: the marks off (shaman forces with members and their escorts).
fn unmark(world: &mut World, p: u8, s: u8) {
    for r in shamans(world, p, s) {
        if count(world, r) < 1 {
            continue;
        }
        let l = tfs::linked(world, r);
        for x in std::iter::once(r).chain(l) {
            if let Some(t) = super::player::tf_mut(world, x) {
                t.set_flags(t.flags() & !tfs::flag::ATTACK);
            }
        }
    }
}

/// 0x42c010: the reset (see the module doc).
pub fn reset(world: &mut World, p: u8, s: u8) {
    set(world, p, s, F::Step, 0);
    set(world, p, s, F::Players, 0);
    set(world, p, s, F::Strength, -1);
    set(world, p, s, F::Flying, 0);
    let delay = world.ai.vars.get(p, 22);
    let r = world.rng.rand();
    let next = (delay.wrapping_mul(0x348) as u32).wrapping_add(r & 0xff).wrapping_add(world.ai.tick);
    set(world, p, s, F::Next, next as i32);
}

/// 0x444a70 `(type)`: the escort squad takes type `kind` when that is of its class.
fn set_kind(world: &mut World, r: TfRef, kind: i32) {
    let TfRef::Group(p, s, uid) = r else { return };
    let Some(item) = world.ai.players.get_mut(p as usize).and_then(|a| a.as_mut()).and_then(|a| a.body.sectors.get_mut(s as usize)).and_then(|x| x.group.get_mut(uid)) else { return };
    if item.kind != kind && super::model::class_of(kind) == super::model::class_of(item.kind) {
        item.kind = kind;
    }
}

/// 0x42c750 `(empty, marked)`: a manakopter for a shaman force — the strongest shaman force of
/// type 0xc (members, + 10 with an escort that has members; the first of equals), or with
/// `marked` the last marked one. The player's manakopter nearest its first member (with `empty`
/// only empty ones, state bit 2) that is not under way (state bit 1) — and with `marked` whose
/// own task force is in phase 10 and the shaman force still marked — gets the force and its
/// escort aboard (order 10); the escort becomes type 7 and gives up its first members beyond
/// `DTMaxSoldiersPerTaskForce`. True when that happened.
fn board(world: &mut World, p: u8, s: u8, empty: bool, marked_only: bool) -> bool {
    let mut best: Option<TfRef> = None;
    let mut score = 0;
    for r in shamans(world, p, s) {
        if count(world, r) < 1 || tfs::kind_of_ref(world, r) != 0xc {
            continue;
        }
        if marked_only {
            if marked(world, r) {
                best = Some(r);
            }
            continue;
        }
        let mut v = count(world, r);
        if tfs::linked(world, r).is_some_and(|l| count(world, l) > 0) {
            v += 10;
        }
        if score < v {
            score = v;
            best = Some(r);
        }
    }
    let Some(r) = best else { return false };
    let Some(first) = tfs::members(world, r).first().copied() else { return false };
    let Some(at) = world.entities.by_index(first as u32).map(|e| e.pos) else { return false };
    let mk = crate::flying::nearest(world, p, at.x(), at.y(), if empty { crate::flying::state::EMPTY } else { 0 });
    if mk == 0 {
        return false;
    }
    let ok = !marked_only
        || (world.ai.infos.get(&mk).is_some_and(|&m| phase(world, m) == 10) && marked(world, r));
    let ordered = crate::flying::get(world, mk).is_some_and(|f| f.state & crate::flying::state::ORDERED != 0);
    if ordered || !ok {
        return false;
    }
    tfs::order(world, r, 10, mk as i32, 0);
    if let Some(l) = tfs::linked(world, r) {
        tfs::order(world, l, 10, mk as i32, 0);
        set_kind(world, l, 7);
        let max = world.rules.magic.dark.max_soldiers_per_task_force;
        let n = count(world, l);
        if n > max {
            for m in tfs::members(world, l).into_iter().take((n - max) as usize) {
                tfs::remove(world, l, m);
            }
        }
    }
    true
}

/// The catapult tiling's sector of a node (tile ids from 0x8000 on count as 0).
fn catapult_sector(world: &World, n: usize) -> u16 {
    let id = world.tiling.catapult[n];
    world.tiling.tile(if id < 0x8000 { id } else { 0 }).sector
}

/// 0x42c3e0 `(spread)`: every manakopter force flies (order 1) to the target, first moved to
/// the nearest node without flag 1 (0x436020). With `spread` and the target on a sector, each
/// force draws an offset (two game RNG draws: `x − 0..15`, `y + 0..15`) and takes the first of
/// 1000 spiral nodes from there that is on the target's sector, on a catapult-tiling sector,
/// without node flag 8, with the four nodes (−4, −2), (2, −2), (−4, 4), (2, 4) from it on the
/// map likewise; none: (−2, 2) when `(x − 2, x + 2)` [sic] is on the sector, else the target.
/// Returns the number of forces.
fn fly_all(world: &mut World, p: u8, s: u8, spread: bool) -> i32 {
    let (tx, ty) = (get(world, p, s, F::X), get(world, p, s, F::Y));
    let (x, y) = super::player::free_node_pub(world, tx, ty).unwrap_or((tx, ty));
    let size = world.grid.size() as i32;
    let on = |w: &World, nx: i32, ny: i32| -> Option<bool> {
        let n = w.grid.checked_index(nx, ny)?;
        Some(w.tiling.sector_of(n))
            .map(|sec| sec as i32 == w.grid.checked_index(x, y).map_or(-1, |m| w.tiling.sector_of(m) as i32) && catapult_sector(w, n) != 0)
    };
    let target_sector = world.grid.checked_index(x, y).map_or(0, |n| world.tiling.sector_of(n));
    let mut n = 0;
    for r in manakopters(world, p, s) {
        let (mut ox, mut oy) = (0, 0);
        if spread && target_sector != 0 {
            let rx = (world.rng.rand() & 0xf) as i32;
            let ry = (world.rng.rand() & 0xf) as i32;
            let (bx, by) = (x - rx, y + ry);
            let mut found = false;
            for k in 0..1000usize {
                let (nx, ny) = (bx + crate::spiral::dx(k), by + crate::spiral::dy(k));
                if !(0..size).contains(&nx) || !(0..size).contains(&ny) {
                    continue;
                }
                let node = world.grid.index(nx as u32, ny as u32);
                if on(world, nx, ny) != Some(true) || world.layers.flags[node] & 8 != 0 {
                    continue;
                }
                let corners = [(-4, -2), (2, -2), (-4, 4), (2, 4)];
                if corners.iter().all(|&(dx, dy)| on(world, nx + dx, ny + dy) != Some(false)) {
                    (ox, oy) = (nx - x, ny - y);
                    found = true;
                    break;
                }
            }
            if !found {
                let keep = (0..size).contains(&(x - 2)) && (0..size).contains(&(x + 2)) && world.tiling.sector_of(world.grid.index((x - 2) as u32, (x + 2) as u32)) == target_sector;
                (ox, oy) = if keep { (-2, 2) } else { (0, 0) };
            }
        }
        tfs::order(world, r, 1, pack(x + ox, y + oy), 0);
        n += 1;
    }
    n
}

/// 0x42c250: the target reachable (0x435a00, a node of the anchor's sector within spiral radius
/// 15 of it), the escorted shaman forces with members go to it (order 3) — at most +0x30 of
/// them. Returns their number.
fn send_on_foot(world: &mut World, p: u8, s: u8) -> i32 {
    let ((_, _), home) = anchor(world, p, s);
    let (x, y) = (get(world, p, s, F::X), get(world, p, s, F::Y));
    if !reachable(world, home, x, y) {
        return 0;
    }
    let limit = get(world, p, s, F::Strength);
    let mut n = 0;
    for r in shamans(world, p, s) {
        if count(world, r) < 1 || tfs::linked(world, r).is_none() {
            continue;
        }
        tfs::order(world, r, 3, pack(x, y), 0);
        n += 1;
        if n >= limit {
            break;
        }
    }
    n
}

/// 0x435a00 `(&x, &y, sector)`: a node of `sector` (> 0) on the spiral `0..=Last(15)` around
/// `(x, y)` (on the map).
fn reachable(world: &World, sector: u16, x: i32, y: i32) -> bool {
    sector > 0 && crate::spiral::walk(world.grid, x, y, 15).any(|(nx, ny)| world.tiling.sector_of(world.grid.index(nx as u32, ny as u32)) == sector)
}

/// 0x42c2e0: with the target reachable, the shaman forces with order 3 cast there (order 9);
/// otherwise those go home (0x42c1e0 with 3). Returns their number.
fn cast_on_foot(world: &mut World, p: u8, s: u8) -> i32 {
    let ((_, _), home) = anchor(world, p, s);
    let (x, y) = (get(world, p, s, F::X), get(world, p, s, F::Y));
    if !reachable(world, home, x, y) {
        go_home(world, p, s, 3);
        return 0;
    }
    let list = with_order(world, shamans(world, p, s), 3);
    for &r in &list {
        tfs::order(world, r, 9, pack(x, y), 0);
    }
    list.len() as i32
}

/// 0x42c370: the marked shaman forces with members cast at the target (order 0xe); their
/// escorts follow (order 6).
fn cast_from_air(world: &mut World, p: u8, s: u8) -> i32 {
    let (x, y) = (get(world, p, s, F::X), get(world, p, s, F::Y));
    let mut n = 0;
    for r in shamans(world, p, s) {
        if count(world, r) >= 1 && marked(world, r) {
            n += 1;
            tfs::order(world, r, 0xe, pack(x, y), 0);
            if let Some(l) = tfs::linked(world, r) {
                tfs::order(world, l, 6, 0, 0);
            }
        }
    }
    n
}

/// 0x42c1e0 `(cmd)`: the shaman forces with order `cmd` go home (order 7 to the anchor).
fn go_home(world: &mut World, p: u8, s: u8, cmd: u32) {
    let ((ax, ay), _) = anchor(world, p, s);
    for r in with_order(world, shamans(world, p, s), cmd) {
        tfs::order(world, r, 7, pack(ax, ay), 0);
    }
}

/// 0x42cb30: the next target of each marked shaman force; see the module doc. Returns the number
/// of forces visited that did not turn home (−1 when it stops the step).
fn retarget(world: &mut World, p: u8, s: u8) -> i32 {
    let now = world.ai.tick;
    if marked_flying(world, p, s) {
        if get(world, p, s, F::Flying) > 0 && board(world, p, s, false, true) {
            set(world, p, s, F::Step, 5);
            return 1;
        }
        let mk10 = manakopters(world, p, s).into_iter().any(|r| count(world, r) >= 1 && phase(world, r) == 10);
        if mk10 && marked_done(world, p, s) {
            let ((ax, ay), _) = anchor(world, p, s);
            set(world, p, s, F::X, ax);
            set(world, p, s, F::Y, ay);
            fly_all(world, p, s, false);
            set(world, p, s, F::Step, 6);
            return 1;
        }
        if get(world, p, s, F::Flying) == 0 {
            go_home(world, p, s, 9);
            return -1;
        }
        if !mk10 {
            go_home(world, p, s, 0xe);
            return -1;
        }
    }
    let cmd = if get(world, p, s, F::Flying) < 1 { 9 } else { 0xe };
    let mut n = 0;
    for r in with_order(world, shamans(world, p, s), cmd) {
        let prev = n;
        n += 1;
        let first = tfs::members(world, r).first().copied().and_then(|m| world.entities.by_index(m as u32)).map(|e| (e.x(), e.y()));
        let (fx, fy) = first.unwrap_or((-1, -1));
        let (ph, tick, target) = tf(world, r).map_or((0, 0, 0), |t| (t.phase(), t.order_tick(), t.target()));
        let flying = get(world, p, s, F::Flying);
        let late = if ph == 2 || flying != 0 { (world.rng.rand() & 0x7f).wrapping_add(tick).wrapping_add(0xb9) < now } else { true };
        // Is the agent's target still worth it: a pile of boards or iron with goods not reserved,
        // or (no deco object) a field within 2 (which moves the target)?
        let (ax, ay) = (get(world, p, s, F::X), get(world, p, s, F::Y));
        let deco = world.grid.checked_index(ax, ay).map_or(0, |n| world.layers.deco_object[n]);
        let keep = if deco == 0 {
            match super::shaman::field_near(world, ax, ay, 2) {
                Some((fx2, fy2)) => {
                    set(world, p, s, F::X, fx2);
                    set(world, p, s, F::Y, fy2);
                    true
                }
                None => false,
            }
        } else {
            match world.entities.by_index(deco as u32) {
                Some(e) if e.obj_type == crate::entity::ObjType::Pile => match &e.data {
                    crate::entity::EntityData::Pile(pl) => matches!(pl.good, 7 | 20) && pl.amount > pl.leaving,
                    _ => false,
                },
                _ => true,
            }
        };
        if keep && !late {
            continue;
        }
        // A new target from the force's first member: a pile of what the hall lacks, an enemy
        // civilian within 32 (from there, or from the order's target when that is more than 8
        // away), else home.
        let mut at = (fx, fy);
        let mut found = false;
        if let Some(hall) = crate::buildings::manakopter::hall_of(world, p)
            && world.entities.by_index(hall as u32).is_some_and(|e| e.flags & crate::entity::flag::READY != 0)
            && let Some(st) = crate::buildings::manakopter::get(world, hall)
        {
            let req = world.rules.magic.dark.manakopter_required;
            let (boards, iron) = (req[1] - st.stock[1], req[2] - st.stock[2]);
            let (good, left) = if iron < boards { (7, boards) } else { (20, iron) };
            if left > 0 {
                found = super::shaman::pile_near(world, &mut at, good) != 0;
            }
        }
        if !found {
            found = civilian(world, p, &mut at);
            if !found {
                let t = Y16X16(target as u32);
                at = (t.x(), t.y());
                let d = crate::spiral::grid_distance(at.0 - fx, at.1 - fy);
                found = d > 8 && civilian(world, p, &mut at);
            }
        }
        if found {
            tfs::order(world, r, cmd, pack(at.0, at.1), 0);
            continue;
        }
        let ((hx, hy), _) = anchor(world, p, s);
        if get(world, p, s, F::Flying) > 0 && manakopter_force(world, p, s, false) {
            board(world, p, s, false, true);
            set(world, p, s, F::Step, 5);
        } else {
            tfs::order(world, r, 7, pack(hx, hy), 0);
            set(world, p, s, F::Step, 6);
        }
        n = prev;
    }
    n
}

/// `CAIShaman` vtbl+4 (0x444340) `(player, 32, &x, &y)`: the nearest civilian of the player's
/// enemies around `at` (0x5b0dd0 with radius 32); `at` becomes its node.
fn civilian(world: &World, p: u8, at: &mut (i32, i32)) -> bool {
    let enemies = world.alliances.enemy_bits[p.min(8) as usize] as u32;
    let (n, pos) = crate::warmap::civilians_near(world, at.0, at.1, 0x20, enemies);
    if n > 0 {
        let q = Y16X16(pos as u32);
        *at = (q.x(), q.y());
    }
    n > 0
}

/// `Execute` (0x42cf00) of the attack agent of Dark Tribe sector AI `s`.
pub fn execute(world: &mut World, p: u8, s: u8) -> u32 {
    let now = world.ai.tick;
    match get(world, p, s, F::Step) {
        0 => {
            if world.ai.vars.get(p, 0) < 1 || now <= get(world, p, s, F::Next) as u32 {
                return 0;
            }
            if (now as i32) <= world.ai.vars.get(p, 1).wrapping_mul(0x348) || !escorted_shamans(world, p, s) {
                return 0;
            }
            if get(world, p, s, F::Strength) < 1 {
                set(world, p, s, F::Players, 1);
                set(world, p, s, F::Strength, 1);
                if any_flying(world, p, s) {
                    if !board(world, p, s, true, false) {
                        set(world, p, s, F::Players, 0);
                        set(world, p, s, F::Strength, -1);
                        return 0xa000;
                    }
                    mark(world, p, s, 10);
                }
                set(world, p, s, F::Step, 1);
            }
            0xa000
        }
        1 => {
            let flying = any_flying(world, p, s);
            if !escorted_shamans(world, p, s) {
                return 0;
            }
            let ((ax, ay), _) = anchor(world, p, s);
            let players = get(world, p, s, F::Players) as u32;
            let Some((x, y)) = super::shaman::target(world, p, players, ax, ay, flying) else {
                set(world, p, s, F::X, ax);
                set(world, p, s, F::Y, ay);
                return 0;
            };
            set(world, p, s, F::X, x);
            set(world, p, s, F::Y, y);
            if flying {
                if !marked_done(world, p, s) {
                    return 0;
                }
                let n = fly_all(world, p, s, true);
                set(world, p, s, F::Flying, n);
                if n >= 1 {
                    set(world, p, s, F::Step, 2);
                }
                return 0;
            }
            if send_on_foot(world, p, s) > 0 {
                mark(world, p, s, 3);
                set(world, p, s, F::Step, 2);
                return 0;
            }
            end(world, p, s)
        }
        2 => {
            if get(world, p, s, F::Flying) > 0 && any_flying(world, p, s) {
                if !flights_idle(world, p, s) {
                    return 0;
                }
                for r in with_order(world, manakopters(world, p, s), 1) {
                    tfs::order(world, r, 0xc, 0, 0);
                }
                set(world, p, s, F::Step, 3);
                return 0x10000;
            }
            let n = marked_walking(world, p, s);
            if n < 0 {
                return 0;
            }
            if n > 0 && cast_on_foot(world, p, s) != 0 {
                set(world, p, s, F::Step, 4);
                return 0;
            }
            end(world, p, s)
        }
        3 => {
            cast_from_air(world, p, s);
            set(world, p, s, F::Step, 4);
            0
        }
        4 => {
            if get(world, p, s, F::Flying) > 0 && any_flying(world, p, s) {
                if retarget(world, p, s) > 0 {
                    return 0;
                }
                board(world, p, s, false, true);
                set(world, p, s, F::Step, 5);
                return 0;
            }
            set(world, p, s, F::Flying, 0);
            if retarget(world, p, s) > 0 {
                return 0;
            }
            set(world, p, s, F::Step, 6);
            0
        }
        5 => {
            if get(world, p, s, F::Flying) < 1 || !marked_done(world, p, s) {
                return 0;
            }
            let ((ax, ay), _) = anchor(world, p, s);
            set(world, p, s, F::X, ax);
            set(world, p, s, F::Y, ay);
            if fly_all(world, p, s, false) == 0 {
                go_home(world, p, s, 10);
            }
            set(world, p, s, F::Step, 6);
            0
        }
        6 => {
            if get(world, p, s, F::Flying) > 0 {
                if !flights_idle(world, p, s) {
                    return 0;
                }
                for r in with_order(world, manakopters(world, p, s), 1) {
                    tfs::order(world, r, 0xb, 0, 0);
                }
                set(world, p, s, F::Step, 7);
                return 0x10000;
            }
            if marked_walking(world, p, s) < 0 {
                return 0;
            }
            end(world, p, s)
        }
        7 => {
            unmark(world, p, s);
            reset(world, p, s);
            set(world, p, s, F::X, 0);
            set(world, p, s, F::Y, 0);
            for r in with_order(world, shamans(world, p, s), 10) {
                tfs::order(world, r, 0, 0, 0);
                if let Some(l) = tfs::linked(world, r) {
                    if tfs::kind_of_ref(world, l) != 2 {
                        set_kind(world, l, 2);
                    }
                    tfs::order(world, l, 6, 0, 0);
                }
            }
            0
        }
        _ => end(world, p, s),
    }
}

/// The raid ends (0x42ca40, 0x42c010).
fn end(world: &mut World, p: u8, s: u8) -> u32 {
    unmark(world, p, s);
    reset(world, p, s);
    0
}

/// 0x42ca80 `(players, strength, now)`: `AI.AttackNow` (the sector AI's event 0x1b, 0x4411e0:
/// players = bit of `a` (0 for `a` < 1), strength = `b`). A negative strength cancels the
/// request; otherwise (0 → 2), when no raid runs and it asks for more than the last one: the
/// players (0 → 1), the agent due at once (0x42b820), and with manakopters a free one for the
/// strongest shaman force (as step 0) — none cancels it — then step 1.
pub fn attack_now(world: &mut World, p: u8, s: u8, players: u32, strength: i32) {
    if strength < 0 {
        set(world, p, s, F::Players, 0);
        set(world, p, s, F::Strength, -1);
        return;
    }
    let v = if strength == 0 { 2 } else { strength };
    if get(world, p, s, F::Step) != 0 || v <= get(world, p, s, F::Strength) {
        return;
    }
    set(world, p, s, F::Strength, v);
    set(world, p, s, F::Players, if players == 0 { 1 } else { players as i32 });
    super::player::run_agent_now(world, p, s, AGENT as u8);
    if any_flying(world, p, s) {
        if !board(world, p, s, true, false) {
            set(world, p, s, F::Players, 0);
            set(world, p, s, F::Strength, -1);
            return;
        }
        mark(world, p, s, 10);
    }
    set(world, p, s, F::Step, 1);
}
