//! `CLandAnimal` behaviour: `LogicUpdate` (`CAnimal` vtbl+0x64, 0x52bcd0), the decision
//! (vtbl+0x7c, 0x531850), `TakeJob` (vtbl+0x80, 0x52bd40), the step (0x52be40) and the direction
//! choices (0x531fa0, 0x531dd0, 0x531b70). Every random number is the game RNG's.

use super::{AnimalData, cells, data, data_mut};
use crate::entity::flag;
use crate::grid::Y16X16;
use crate::pathing::walking;
use crate::world::World;

/// `Y16X16::g_iNeighborY16X16Modifiers` (.rdata 0x101bd58): direction → packed delta.
const MODIFIER: [i32; 6] = [1, 0x10001, 0x10000, -1, -0x10001, -0x10000];
/// .rdata 0x101bd88: direction → (dx, dy), the same order (used for cells and nodes alike).
pub(super) const NEIGHBOR: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];

/// Task indices of the type's list (`CAnimalMgr` 0x52f8c0).
mod task {
    pub const STAND: u8 = 0;
    pub const IDLE: u8 = 1;
    pub const WALK: u8 = 2;
    pub const FLEE: u8 = 3;
    pub const DIE: u8 = 4;
}

/// `CWorldManager::Index(packed)` without a range check, as the original computes it.
fn raw_index(world: &World, packed: i32) -> i64 {
    let p = Y16X16(packed as u32);
    p.y() as i64 * world.grid.size() as i64 + p.x() as i64
}

fn in_map(world: &World, i: i64) -> Option<usize> {
    usize::try_from(i).ok().filter(|&i| i < world.grid.len())
}

/// No blocked land, no deco object, no map object (off the map: taken).
fn free(world: &World, i: i64) -> bool {
    let l = &world.layers;
    in_map(world, i).is_some_and(|i| l.flags[i] & 1 == 0 && l.deco_object[i] == 0 && l.map_object[i] == 0)
}

fn pos_of(world: &World, id: u16) -> i32 {
    world.entities.by_index(id as u32).map_or(0, |e| e.pos.0 as i32)
}

fn kind_of(world: &World, id: u16) -> u16 {
    world.entities.by_index(id as u32).map_or(0, |e| e.kind)
}

/// `CAnimal::TakeJob(register)` (0x52bd40): the current task's type, frames and job; frame and
/// step progress 0, step flags 0x48. Registering waits the task's frames (walk, idle animation),
/// one frame less (death) or one tick (stand).
pub(super) fn take_job(world: &mut World, id: u16, register: bool) {
    let kind = kind_of(world, id);
    let tick = world.tick;
    let Some(index) = data(world, id).map(|d| d.task) else { return };
    let (ty, job, frames) = super::task(&world.rules.animals, kind, index);
    let Some(d) = data_mut(world, id) else { return };
    d.task_type = ty;
    d.job = job;
    d.frames = frames;
    d.frame = 0;
    d.moved = 0x48;
    d.job_tick = tick;
    if register {
        let delay = match ty {
            6 | 0x10 => frames as i32,
            0x1b if frames > 1 => frames as i32 - 1,
            _ => 1,
        };
        world.register_for_logic_update(id, delay);
    }
}

/// `CAnimal::LogicUpdate` (0x52bcd0). Entity events (0x532be0) are only consumed by animals
/// (vtbl+0x6c 0x52c000 sets flag 0x80000000) and nothing sends them yet. The dead lie 450 ticks.
/// Otherwise `IdleWalk` first (it steps out of a crowd, game RNG), then the decision.
pub fn logic_update(world: &mut World, id: u16) {
    let Some(state) = data(world, id).map(|d| d.state) else { return };
    if state == 5 {
        if world.entities.by_index(id as u32).is_some_and(|e| e.flags & flag::DIED == 0) {
            let Some(d) = data_mut(world, id) else { return };
            d.dead_ticks += 15;
            if d.dead_ticks < 0x1c2 {
                world.register_for_logic_update(id, 15);
            } else {
                super::delete(world, id);
            }
        }
        return;
    }
    let pos = Y16X16(pos_of(world, id) as u32);
    let Some(mut ws) = data_mut(world, id).map(|d| std::mem::take(&mut d.walk)) else { return };
    let r = walking::idle_walk(world, &mut ws, pos.0 as i32, 0);
    if let Some(d) = data_mut(world, id) {
        d.walk = ws;
    }
    if r & 8 == 0 {
        walk(world, id, r as u8);
    } else {
        decide(world, id);
    }
}

/// `CLandAnimal` vtbl+0x7c (0x531850): what to do next.
/// * Dying (4): the last frame, dead (5), next update in a tick.
/// * Wrong land: a dark bug off dark land, any other animal on it, is deleted at once.
/// * A RUNAWAY type looks for the nearest enemy settler every second decision (every one while
///   fleeing) and flees (3) while one is in sight, if it finds a direction.
/// * Off its kind of land it walks back (a direction from [`choose_dir`]).
/// * Otherwise (game RNG) 30 %: the idle animation; 40 %: stands 10..13 ticks; 30 %: a step,
///   or stands 5 ticks when no direction is free.
/// * Standing or idling 151 decisions in a row on the same node deletes it.
fn decide(world: &mut World, id: u16) {
    let Some(d) = data(world, id) else { return };
    let state = d.state;
    let kind = kind_of(world, id);
    if state == 4 {
        let Some(d) = data_mut(world, id) else { return };
        d.frame = d.frames.saturating_sub(1);
        d.state = 5;
        d.dead_ticks = 0;
        world.register_for_logic_update(id, 1);
        return;
    }
    let pos = pos_of(world, id);
    let n = raw_index(world, pos);
    let dark = in_map(world, n).is_some_and(|n| world.layers.flags[n] & 0x04 != 0);
    let darkbug = kind == 2 || kind == 3;
    if dark != darkbug {
        let Some(d) = data_mut(world, id) else { return };
        d.wrong_land += 1;
        if d.wrong_land > 0 {
            super::delete(world, id);
            // `CEffects::AddEffect(0x40, 0x45, x, y)`: the vanishing effect.
            let p = Y16X16(pos as u32);
            world.events.push(crate::event::SimEvent::Effect { effect: 0x40, sound: 0x45, x: p.x(), y: p.y() });
            return;
        }
    } else if let Some(d) = data_mut(world, id) {
        d.wrong_land = 0;
    }
    let runaway = world.animals.types.get(kind as usize).is_some_and(|t| t.runaway);
    if runaway {
        let Some(d) = data_mut(world, id) else { return };
        d.search += 1;
        if d.state == 3 || d.search > 1 {
            let found = find_enemy(world, id);
            let Some(d) = data_mut(world, id) else { return };
            d.search = 0;
            d.state = if found { 3 } else { 2 };
        }
        if data(world, id).is_some_and(|d| d.state == 3)
            && let Some(dir) = choose_dir(world, id)
        {
            if let Some(d) = data_mut(world, id) {
                d.task = task::FLEE;
            }
            take_job(world, id, false);
            if let Some(d) = data_mut(world, id) {
                d.state = 3;
            }
            step_and_wait(world, id, dir);
            return;
        }
    }
    if !land_ok(world, id, n)
        && let Some(dir) = choose_dir(world, id)
    {
        walk(world, id, dir);
        if let Some(d) = data_mut(world, id) {
            d.roam += 1;
        }
        return;
    }
    let r = world.rng.rand();
    if r < 0x4ccc {
        if let Some(d) = data_mut(world, id) {
            d.task = task::IDLE;
        }
        take_job(world, id, true);
        if let Some(d) = data_mut(world, id) {
            d.state = 1;
        }
    } else if r < 0xb333 {
        let wait = (world.rng.rand() & 3) as i32 + 10;
        stand(world, id, wait);
    } else if let Some(dir) = choose_dir(world, id) {
        walk(world, id, dir);
        if let Some(d) = data_mut(world, id) {
            d.roam += 1;
        }
        return;
    } else {
        stand(world, id, 5);
    }
    let Some(d) = data_mut(world, id) else { return };
    if d.last_pos == pos {
        d.stuck += 1;
        if d.stuck > 0x96 {
            super::delete(world, id);
            if let Some(d) = data_mut(world, id) {
                d.last_pos = pos;
            }
            return;
        }
    } else {
        d.stuck = 0;
    }
    d.last_pos = pos;
}

/// 0x52bfc0: stand (task 0, `TakeJob` only when it changes) for `ticks`.
fn stand(world: &mut World, id: u16, ticks: i32) {
    let Some(d) = data_mut(world, id) else { return };
    let changed = d.task != task::STAND;
    d.task = task::STAND;
    if changed {
        take_job(world, id, false);
    }
    if let Some(d) = data_mut(world, id) {
        d.state = 0;
    }
    world.register_for_logic_update(id, ticks);
}

/// 0x52bf50: walk (task 2, state 2) one step in `dir`.
fn walk(world: &mut World, id: u16, dir: u8) {
    if let Some(d) = data_mut(world, id) {
        d.task = task::WALK;
    }
    take_job(world, id, false);
    if let Some(d) = data_mut(world, id) {
        d.state = 2;
    }
    step_and_wait(world, id, dir);
}

/// 0x52bf10: the step, then the next update after a step's ticks (+0x69) or, blocked, next tick.
fn step_and_wait(world: &mut World, id: u16, dir: u8) {
    step(world, id, dir);
    let Some(d) = data(world, id) else { return };
    let delay = if d.moved & 0xf < 6 { d.speed as i32 } else { 1 };
    world.register_for_logic_update(id, delay);
}

/// 0x52be40: one step in direction `dir & 0xf` when the walking's `IsNotBlocked` and
/// `IsNotOccupied` allow the node: the map-object layer, position, direction and war map follow.
/// The step flags keep the direction, or'ed with 0x88 when it did not move.
fn step(world: &mut World, id: u16, dir: u8) {
    let d4 = (dir & 0xf) as usize;
    let mut moved = dir;
    if d4 < 6 {
        let pos = pos_of(world, id);
        let to = pos.wrapping_add(MODIFIER[d4]);
        let n = in_map(world, raw_index(world, to));
        let ws = data(world, id).map(|d| d.walk.as_ref().clone()).unwrap_or_default();
        if walking::is_not_blocked(world, &ws, n) && walking::is_not_occupied(world, n) {
            let n = n.expect("checked by is_not_blocked");
            if let Some(old) = in_map(world, raw_index(world, pos)) {
                world.layers.map_object[old] = 0;
            }
            world.layers.map_object[n] = id;
            if let Some(e) = world.entities.by_index_mut(id as u32) {
                e.pos = Y16X16(to as u32);
            }
            let tick = world.tick;
            if let Some(d) = data_mut(world, id) {
                d.dir = d4 as u8;
                d.step_tick = tick;
                d.moved = dir;
            }
            crate::warmap::moved(world, id);
            return;
        }
        moved |= 0x88;
    }
    if let Some(d) = data_mut(world, id) {
        d.moved = moved;
    }
}

/// `CLandAnimal` vtbl+0x20 (0x5327a0, the hunter's kill): unless dead, it stops being alive
/// (flags 0x3000000 off), plays its death (task 4, state 4) and is updated when it ends.
pub fn start_dying(world: &mut World, id: u16) {
    if data(world, id).is_none_or(|d| d.state == 5) {
        return;
    }
    if let Some(e) = world.entities.by_index_mut(id as u32) {
        e.flags &= !flag::ALIVE_MASK;
    }
    if let Some(d) = data_mut(world, id) {
        d.task = task::DIE;
        d.state = 4;
    }
    take_job(world, id, true);
}

/// 0x532690: whether node `n` is the animal's kind of land: grass not dark for plain and forests
/// (land types 1..4), dark land for the dark bugs (8), not dark for town animals (9), grass for
/// the rest.
fn land_ok(world: &World, id: u16, n: i64) -> bool {
    let Some(n) = in_map(world, n) else { return false };
    let land = world.animals.types.get(kind_of(world, id) as usize).map_or(0, |t| t.land);
    let grass = world.layers.ground[n] & 0xf0 == 0x10;
    let dark = world.layers.flags[n] & 0x04 != 0;
    match land {
        1..=4 => grass && !dark,
        8 => dark,
        9 => !dark,
        _ => grass,
    }
}

/// 0x532740: a map object on one of the six neighbours of `packed` other than the animal itself.
fn crowded(world: &World, id: u16, packed: i32) -> bool {
    let own = pos_of(world, id);
    MODIFIER.iter().any(|&m| {
        let p = packed.wrapping_add(m);
        p != own && in_map(world, raw_index(world, p)).is_some_and(|i| world.layers.map_object[i] != 0)
    })
}

/// 0x532590: the direction from the animal towards `target` by the signs of dx and dy (−1 when
/// there is none, e.g. the same node).
fn dir_towards(world: &World, id: u16, target: i32) -> i32 {
    let p = Y16X16(pos_of(world, id) as u32);
    let t = Y16X16(target as u32);
    let sx = (t.x() - p.x()).signum();
    let sy = (t.y() - p.y()).signum();
    match (sx, sy) {
        (-1, 1) => 3,
        (1, -1) => 0,
        _ => NEIGHBOR.iter().position(|&n| n == (sx, sy)).map_or(-1, |d| d as i32),
    }
}

/// 0x532500: the nearest settler of warrior type 0..7 (not the door settler 0xd nor a donkey 14)
/// of a player the animal fears — every player,
/// or for a type with a race the players of other races — in the war-map cells within SEARCH_RAD
/// (0x5b0770); remembered as the enemy.
fn find_enemy(world: &mut World, id: u16) -> bool {
    let kind = kind_of(world, id);
    let (race, radius) = world.animals.types.get(kind as usize).map_or((-1, 0), |t| (t.race, t.search_rad));
    let players: u32 = if race < 0 {
        0xff
    } else {
        (1..=8u8).filter(|&p| world.player(p).is_none_or(|pl| pl.race as i32 != race)).fold(0, |m, p| m | 1 << (p - 1))
    };
    let p = Y16X16(pos_of(world, id) as u32);
    let (x, y) = (p.x(), p.y());
    let mut best = (0x4000, -1);
    for (cx, cy) in crate::warmap::cells_within(world, x, y, radius) {
        for s in world.warmap.cell(cx as usize, cy as usize, 0) {
            let Some(e) = world.entities.by_index(s as u32) else { continue };
            if (1u32 << (e.flags & 0xf)) & 0xff == 0 || (1u32 << (e.owner as u32)) >> 1 & players == 0 {
                continue;
            }
            let dist = crate::spiral::grid_distance(e.x() - x, e.y() - y);
            if dist < best.0 {
                best = (dist, e.pos.0 as i32);
            }
        }
    }
    let enemy = if best.0 <= radius { best.1 } else { -1 };
    if let Some(d) = data_mut(world, id) {
        d.enemy = enemy;
    }
    enemy != -1
}

/// 0x531af0: whether cell `(cx, cy)` suits the type: a town animal a cell with buildings and no
/// dark land; others a tree count in their range (when they have one) and more than 120 dark
/// (dark bugs) or grass (the rest) nodes.
fn cell_ok(world: &World, kind: u16, flat: i32) -> bool {
    let Some(t) = world.animals.types.get(kind as usize) else { return false };
    let c = world.animals.cells.at(flat);
    if t.land == super::land_type::TOWN {
        return super::warmap_head(world, flat, 2) != 0 && c.dark() == 0;
    }
    let trees = c.trees() as i32;
    if t.min_trees > 0 && (trees < t.min_trees || trees >= t.max_trees) {
        return false;
    }
    if t.land == super::land_type::DARK { c.dark() > 120 } else { c.grass() > 120 }
}

/// 0x531b70: a random one (game RNG) of the six neighbour cells that suit the type, as a
/// direction; −1 when none does (no draw then).
fn cell_dir(world: &mut World, id: u16) -> i32 {
    let kind = kind_of(world, id);
    let p = pos_of(world, id);
    let (cx, cy) = ((p >> 4) & 0xfff, p >> 20);
    let ok: Vec<i32> = (0..6)
        .filter(|&d| {
            let (dx, dy) = NEIGHBOR[d as usize];
            cell_ok(world, kind, (cy + dy) * cells::SIDE + cx + dx)
        })
        .collect();
    if ok.is_empty() {
        return -1;
    }
    ok[(world.rng.rand() % ok.len() as u32) as usize]
}

/// 0x531dd0: the direction away from the enemy, or one to either side of it, whichever of those
/// nodes are free (game RNG among them); straight away when none is.
fn flee_direction(world: &mut World, id: u16) -> i32 {
    let enemy = data(world, id).map_or(-1, |d| d.enemy);
    let mut away = dir_towards(world, id, enemy) - 3;
    if away < 0 {
        away += 6;
    }
    let left = if away - 1 < 0 { away + 5 } else { away - 1 };
    let right = if away + 1 >= 6 { away - 5 } else { away + 1 };
    let pos = pos_of(world, id);
    let ok: Vec<i32> = [away, left, right].into_iter().filter(|&d| free(world, raw_index(world, pos.wrapping_add(MODIFIER[d as usize])))).collect();
    if ok.is_empty() {
        return away;
    }
    ok[(world.rng.rand() % ok.len() as u32) as usize]
}

/// 0x531fa0: the next step's direction, `None` when there is none.
///
/// The preferred direction: fleeing, the flee direction (a new one every 5 steps); after more than
/// 8 roaming steps, towards a suitable neighbour cell or else back home (the cell check also moves
/// home here when the cell suits); otherwise straight on. One game RNG draw `r`. Candidates, each
/// needing a free node of its land with no other map object around it: left of the preferred
/// direction, the preferred one (only when `r < 0xb333`; taken at once when its node's move cost
/// is below 7), right of it. Without any: the next three directions turning right (free, own land),
/// then any of the six the walking may enter. The preferred one stays when it is a candidate and
/// `r < 0xb333`, else candidate `r % count`. A target node with another map object around it gives
/// `None`.
fn choose_dir(world: &mut World, id: u16) -> Option<u8> {
    let pos = pos_of(world, id);
    let n = raw_index(world, pos);
    // Per direction: 3 blocked land, 2 a map object, 1 move cost 7, 0 otherwise.
    let class: [u8; 6] = std::array::from_fn(|d| {
        let (dx, dy) = NEIGHBOR[d];
        match in_map(world, n + dy as i64 * world.grid.size() as i64 + dx as i64) {
            None => 3,
            Some(i) if world.layers.flags[i] & 1 != 0 => 3,
            Some(i) if world.layers.map_object[i] != 0 => 2,
            Some(i) => (world.layers.move_cost(i) == 7) as u8,
        }
    });
    let mut pref = -1;
    let (state, roam, flee_steps, flee_dir) = data(world, id).map(|d| (d.state, d.roam, d.flee_steps, d.flee_dir))?;
    if state == 3 {
        if flee_steps < 0 || flee_dir == -1 {
            let f = flee_direction(world, id);
            data_mut(world, id)?.flee_dir = f;
        }
        let d = data_mut(world, id)?;
        d.flee_steps += 1;
        pref = d.flee_dir;
        if d.flee_steps > 4 {
            d.flee_steps = -1;
        }
    } else if roam > 8 {
        let flat = ((pos >> 20) * cells::SIDE) + ((pos >> 4) & 0xfff);
        if cell_ok(world, kind_of(world, id), flat) {
            data_mut(world, id)?.home = pos;
        } else {
            pref = cell_dir(world, id);
            if pref < 0 {
                let home = data(world, id)?.home;
                pref = dir_towards(world, id, home);
            }
        }
        data_mut(world, id)?.roam = 0;
    }
    if pref == -1 {
        pref = data(world, id)?.dir as i8 as i32;
    }
    let left = if pref == 0 { 5 } else { pref - 1 };
    let mut right = if pref == 5 { 0 } else { pref + 1 };
    let r = world.rng.rand();
    let target = |d: i32| pos.wrapping_add(MODIFIER.get(d as usize).copied().unwrap_or(0));
    let good = |world: &World, d: i32| {
        let t = target(d);
        let i = raw_index(world, t);
        free(world, i) && land_ok(world, id, i) && !crowded(world, id, t)
    };
    let mut mask = 0u32;
    let mut cands: Vec<i32> = Vec::new();
    if good(world, left) {
        mask |= 1 << left;
        cands.push(left);
    }
    if (0..6).contains(&pref) && good(world, pref) && r < 0xb333 {
        if class[pref as usize] == 0 {
            return Some(pref as u8);
        }
        if mask & 1 << pref == 0 {
            mask |= 1 << pref;
            cands.push(pref);
        }
    }
    if good(world, right) && mask & 1 << right == 0 {
        mask |= 1 << right;
        cands.push(right);
    }
    if cands.is_empty() {
        let size = world.grid.size() as i64;
        let p = Y16X16(pos as u32);
        let at = |d: i32| {
            let (dx, dy) = NEIGHBOR[d as usize];
            (p.y() + dy) as i64 * size + (p.x() + dx) as i64
        };
        for _ in 0..3 {
            right = if right == 5 { 0 } else { right + 1 };
            let i = at(right);
            if free(world, i) && land_ok(world, id, i) && mask & 1 << right == 0 {
                mask |= 1 << right;
                cands.push(right);
            }
        }
        if cands.is_empty() {
            let ws = data(world, id)?.walk.as_ref().clone();
            for d in 0..6 {
                let i = at(d);
                let n = in_map(world, i);
                if free(world, i) && walking::is_not_blocked(world, &ws, n) && walking::is_not_occupied(world, n) && mask & 1 << d == 0 {
                    mask |= 1 << d;
                    cands.push(d);
                }
            }
        }
    }
    if cands.is_empty() {
        return None;
    }
    if r >= 0xb333 || !(0..6).contains(&pref) || mask & 1 << pref == 0 {
        pref = cands[(r % cands.len() as u32) as usize];
    }
    let t = target(pref);
    if MODIFIER.iter().any(|&m| {
        let q = t.wrapping_add(m);
        q != pos && in_map(world, raw_index(world, q)).is_some_and(|i| world.layers.map_object[i] != 0)
    }) {
        return None;
    }
    Some(pref as u8)
}

impl AnimalData {
    /// The step distance still to go that the original draws with (`IMovingEntity` +0x41, in
    /// 1/256 of a step): `TakeJob` (0x52bd40) clears it, a step (0x52be40) sets 0xff, and every
    /// drawn frame (0x52bb90 → 0x56df70) takes `(ticks << 8) / speed` off it, 0xff counting as
    /// 256. Here from the ticks since the step (the original's per-frame rounding aside).
    pub fn step_left(&self, tick: u32) -> u8 {
        if self.moved & 8 != 0 || self.task_type != 6 || self.step_tick < self.job_tick {
            return 0;
        }
        let t = tick.wrapping_sub(self.step_tick) as u64;
        match (t << 8) / self.speed.max(1) as u64 {
            0 => 0xff,
            done => 256u64.saturating_sub(done) as u8,
        }
    }
}
