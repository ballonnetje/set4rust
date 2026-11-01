//! The normal (non Dark Tribe) sector AI's agents (`CAINormalSectorAI`, ctor 0x442180; their
//! order and periods in [`super::model::NORMAL_AGENTS`]). Own reverse engineering of the shipped
//! exe; decompiles in `tools/re/decompiled/script/ai/s2/`. No AI save exists to check them
//! against: every choice the code alone does not settle is marked [inf].

use crate::World;

/// The sector AI's walking sector (+0x100).
fn sector_of(world: &World, p: u8, s: u8) -> u16 {
    world.ai.players.get(p as usize).and_then(|a| a.as_ref()).and_then(|a| a.body.sectors.get(s as usize)).map_or(0, |x| x.sector())
}

/// Building type of the barracks.
const BARRACKS: i32 = 24;

/// `CAIAgentRecruitment::Execute` (0x432cd0, period 250): one game RNG draw (its bit 0 picks the
/// first barracks' archer / swordsman mix; the next barracks gets the other). For each READY
/// barracks of the player (`CBuildingMgr` list, 0x14eead4) whose ensign lies on the sector AI's
/// sector the soldier wishes of its eco-sector are rewritten by orders 5047 (`ChangeSoldierWish`:
/// type 0x43 with 0 clears them, a type with 0 then `n` sets `n`): swordsmen 1 (0x1d) 12,
/// bowmen 1 (0x20) 12, squad leaders (0x2c) 12; above AI level 1 also swordsmen 2 and bowmen 2 and
/// 3 by the mix (8 / 8 / 2, or 2 / 8 / 8 with swordsmen 3), and the tribe's specialists:
/// Romans (only with the squad agent's count +0x28 > 0) medics 0x23 / 0x24 / 0x25 at 1, Vikings
/// axe warriors 0x26 12, 0x27 4, Maya blowgun warriors 0x29 12, 0x2a 4, Trojans backpack
/// catapultists 0x3d 12, 0x3e 4. Each such barracks flips the mix. Returns 0 (its period).
pub fn recruitment(world: &mut World, p: u8, s: u8, romans_extra: i32) -> u32 {
    let sector = sector_of(world, p, s);
    let level = world.ai.vars.records.get(p as usize).map_or(0, |r| r.level);
    let race = world.race_of(p);
    let mut mix = world.rng.rand() & 1 != 0;
    for b in world.building_list(p as i32, BARRACKS) {
        let Some(e) = world.entities.by_index(b as u32) else { continue };
        let ensign = match &e.data {
            crate::entity::EntityData::Building(d) => d.ensign,
            _ => continue,
        };
        let ready = e.flags & crate::entity::flag::READY != 0;
        let on = world.grid.checked_index(ensign.x(), ensign.y()).is_some_and(|n| world.tiling.sector_of(n) == sector);
        if !ready || !on {
            continue;
        }
        let w = (b as u32) << 16;
        let mut send = |t: u32, n: u32| super::send(world, 5047, p, w | t, n);
        send(0x43, 0);
        for t in [0x1d, 0x20, 0x2c] {
            send(t, 0);
            send(t, 0xc);
        }
        if level != 1 {
            send(0x1e, 0);
            if !mix {
                send(0x1e, 8);
                send(0x1f, 0);
                send(0x1f, 8);
                send(0x21, 0);
                send(0x21, 2);
            } else {
                send(0x1e, 2);
                send(0x21, 0);
                send(0x21, 8);
                send(0x22, 0);
                send(0x22, 8);
            }
            match race {
                0 if romans_extra >= 1 => {
                    for t in [0x23, 0x24, 0x25] {
                        send(t, 0);
                        send(t, 1);
                    }
                }
                1 | 2 | 4 => {
                    let (a, b2) = match race {
                        1 => (0x26, 0x27),
                        2 => (0x29, 0x2a),
                        _ => (0x3d, 0x3e),
                    };
                    send(a, 0);
                    send(a, 0xc);
                    send(b2, 0);
                    send(b2, 4);
                }
                _ => {}
            }
        }
        mix = !mix;
    }
    0
}

/// Settler +0x54 (the to-do list iterator `NewToDoList` sets, 0x580fb0) → +0xc: its task type is
/// 0x11 (WAIT): the settler idles. [inf] read as the current task ([`crate::jobs::core::actual_task`]).
fn idle(world: &World, id: u16) -> bool {
    crate::jobs::core::actual_task(world, id).is_some_and(|t| t.task == crate::jobs::task::task_type::WAIT)
}

/// The tick of an entity's AI info (+4), 0 without one.
fn info_tick(world: &World, id: u16) -> u32 {
    world.ai.ext.get(&id).map_or(0, |x| x.tick)
}

fn set_info_tick(world: &mut World, id: u16, now: u32) {
    if let Some(i) = super::sector::info_ext(world, id) {
        i.tick = now;
    }
}

/// 0x431430: the spiral from `idx` to `max`, the index growing by `idx / 32 + 1` each step; the
/// next node on the map.
fn sparse_spiral(world: &World, x: i32, y: i32, idx: &mut usize, max: usize) -> Option<(i32, i32)> {
    let size = world.grid.size() as i32;
    while *idx <= max {
        let k = *idx;
        *idx = k + (k >> 5) + 1;
        let (nx, ny) = (x + crate::spiral::dx(k), y + crate::spiral::dy(k));
        if (nx as u32) < size as u32 && (ny as u32) < size as u32 {
            return Some((nx, ny));
        }
    }
    None
}

/// `CAIAgentGeologists::Execute(now)` (0x431750, period 100): the geologists (type 49) of slot 7
/// join reservoir 1; with none there, cost 0x80. Up to 10 of them that idle (their current task is
/// WAIT) or were last sent 500 ticks ago are sent again: along the sparse spiral 0x431430 from
/// `First(12)` to `Last(50)` around each, the first mountain node (ground 0x20) on its sector whose
/// tile is not an enemy's — or, from a mountain node that fails, the first such node of the next
/// 13 to its right — and they walk there (data0 0); without one, back to the anchor (data0 4).
pub fn geologists(world: &mut World, p: u8, s: u8, now: u32) -> u32 {
    use super::player::TfRef;
    for m in super::taskforce::members(world, TfRef::Slot(p, s, 7)) {
        if world.entities.by_index(m as u32).is_some_and(|e| e.kind == 49) {
            super::player::add_to(world, m, TfRef::Force(p, s, 1));
        }
    }
    let r1 = TfRef::Force(p, s, 1);
    if super::taskforce::count(world, r1) < 1 {
        return 0x80;
    }
    let enemies = world.alliances.enemy_bits[p.min(8) as usize];
    let size = world.grid.size() as i32;
    let mut k = 0;
    for m in super::taskforce::members(world, r1) {
        if k >= 10 {
            break;
        }
        if !(info_tick(world, m).wrapping_add(500) <= now || idle(world, m)) {
            continue;
        }
        set_info_tick(world, m, now);
        let Some(pos) = world.entities.by_index(m as u32).map(|e| e.pos) else { continue };
        let sector = super::sector::sector_at(world, pos.x(), pos.y());
        let ok = |world: &World, x: i32, y: i32| -> bool {
            let n = (y * size + x) as usize;
            if world.layers.ground[n] != 0x20 {
                return false;
            }
            let t = world.tiling.tile(world.tiling.normal[n]);
            t.sector == sector && enemies & crate::alliances::player_bit(t.owner()) == 0
        };
        let mut idx = crate::spiral::first(12);
        let max = crate::spiral::last(50);
        let mut found = None;
        while let Some((nx, ny)) = sparse_spiral(world, pos.x(), pos.y(), &mut idx, max) {
            if world.layers.ground[(ny * size + nx) as usize] != 0x20 {
                continue;
            }
            if ok(world, nx, ny) {
                found = Some((nx, ny));
                break;
            }
            if let Some(xx) = (nx + 1..=(nx + 13).min(size - 1)).find(|&xx| ok(world, xx, ny)) {
                found = Some((xx, ny));
                break;
            }
        }
        match found {
            Some((x, y)) => super::taskforce::move_entity(world, m, x, y, true),
            None => {
                let (ax, ay) = super::sector::anchor(world, p, s);
                super::taskforce::move_entity(world, m, ax, ay, false);
            }
        }
        k += 1;
    }
    0
}

/// .rdata 0x1163000: the 49 squares (7×7, by rings) around a square.
const SQUARES: [(i32, i32); 49] = [
    (0, 0), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1), (2, 0), (2, 1), (2, 2), (1, 2), (0, 2), (-1, 2), (-2, 2), (-2, 1),
    (-2, 0), (-2, -1), (-2, -2), (-1, -2), (0, -2), (1, -2), (2, -2), (2, -1), (3, 0), (3, 1), (3, 2), (3, 3), (2, 3), (1, 3), (0, 3), (-1, 3),
    (-2, 3), (-3, 3), (-3, 2), (-3, 1), (-3, 0), (-3, -1), (-3, -2), (-3, -3), (-2, -3), (-1, -3), (0, -3), (1, -3), (2, -3), (3, -3), (3, -2),
    (3, -1),
];

/// 0x4323f0 `(sx, sy)` with the search state (sector, team, best): the square's first tile (its
/// tile list 0x1391b20) on the sector that nobody owns; when the square's war-map cell has less
/// foreign strength than the best so far its centre is the best. True when the square had such a
/// tile.
fn square_pick(world: &World, sx: i32, sy: i32, sector: u16, p: u8, best: &mut (i32, i32, i32)) -> bool {
    let side = (world.grid.size() >> 4) as usize;
    let mut id = world.tiling.first_tile_of_square(sx as usize, sy as usize, side);
    while id != 0 {
        let t = world.tiling.tile(id);
        if t.sector == sector && t.ty & 0xf == 0 {
            let c = world.warmap.cell_strength(sx, sy);
            let foreign = c[0] as i32 - c.get(world.alliances.team(p)).copied().unwrap_or(0) as i32;
            if foreign < best.2 {
                *best = (t.center.x(), t.center.y(), foreign);
            }
            return true;
        }
        id = t.sq_next;
    }
    false
}

/// 0x432480 `(x, y)`: the pioneers' target near `(x, y)` on its sector: over the 49 squares around
/// (0x1163000) first those without enemy land whose neighbourhood holds land of our team, then —
/// when that gave nothing or only a place with more than 0x14 foreign strength — those with enemy
/// land: the unowned tile (see [`square_pick`]) with the least foreign strength.
fn pioneer_target(world: &World, p: u8, x: i32, y: i32) -> Option<(i32, i32)> {
    let sector = super::sector::sector_at(world, x, y);
    if sector == 0 {
        return None;
    }
    let side = world.grid.size() as i32 >> 4;
    let allies = world.alliances.ally_bits[p.min(8) as usize] as u8;
    let enemies = world.alliances.enemy_bits[p.min(8) as usize] as u8;
    let mut best = (0, 0, i32::MAX);
    let mut later = [false; 49];
    for (k, &(dx, dy)) in SQUARES.iter().enumerate() {
        let (sx, sy) = ((x >> 4) + dx, (y >> 4) + dy);
        if (sx as u32) < side as u32 && (sy as u32) < side as u32 {
            let o = world.tiling.owner_bits(sx, sy);
            if o[0] & enemies == 0 {
                if o[1] & allies != 0 {
                    square_pick(world, sx, sy, sector, p, &mut best);
                }
            } else {
                later[k] = true;
            }
        }
    }
    if best.2 > 0x14 || best.0 < 1 {
        for (k, &(dx, dy)) in SQUARES.iter().enumerate() {
            if later[k] {
                square_pick(world, (x >> 4) + dx, (y >> 4) + dy, sector, p, &mut best);
            }
        }
    }
    (best.0 > 0).then_some((best.0, best.1))
}

/// `CAIAgentPioneers::Execute(now)` (0x432620, period 100): with `UsePioneers` below 1, period · 4
/// at cost 1. The pioneers (type 47) of slot 7 join reservoir 5's back; with none there the target
/// (+0x28 / +0x2c, since +0x30) is cleared and the cost is 0x80. A target set less than 0x578 ticks
/// ago is kept — moved to the first node on the sector within 8 (0x431270) — while that node has
/// less than 0x10 foreign strength; otherwise a new one from the anchor (0x432480), else from the
/// first pioneer, else the anchor itself, and every pioneer goes; with a kept target only the
/// idle ones go (walk, data0 0; their info's tick noted). The three ints are not saved (a load
/// starts with none).
pub fn pioneers(world: &mut World, p: u8, s: u8, now: u32) -> u32 {
    use super::player::TfRef;
    use super::sched::{AgentRef, Agents};
    let r = AgentRef::Sector(s, super::sector::agent::PIONEERS);
    let period = world.ai.players[p as usize].as_ref().and_then(|a| a.body.get(r)).map_or(100, |a| a.period);
    if world.ai.vars.records.get(p as usize).map_or(0, |x| x.get(18)) < 1 {
        return period << 14 | 1;
    }
    let r5 = TfRef::Force(p, s, 5);
    for m in super::taskforce::members(world, TfRef::Slot(p, s, 7)) {
        if world.entities.by_index(m as u32).is_some_and(|e| e.kind == 47) {
            super::player::add_to(world, m, r5);
        }
    }
    let get = |world: &World| -> [i32; 3] {
        let e = world.ai.players[p as usize].as_ref().and_then(|a| a.body.get(r)).map_or(Vec::new(), |a| a.extra.clone());
        [e.first().copied().unwrap_or(0), e.get(1).copied().unwrap_or(0), e.get(2).copied().unwrap_or(0)]
    };
    let put = |world: &mut World, v: [i32; 3]| {
        if let Some(a) = world.ai.players[p as usize].as_mut().and_then(|a| a.body.agent(r)) {
            a.extra = v.to_vec();
        }
    };
    if super::taskforce::count(world, r5) < 1 {
        put(world, [0; 3]);
        return 0x80;
    }
    let mut a = get(world);
    let mut keep = false;
    if a[0] > 0 && a[1] > 0 && a[2] != 0 && now < (a[2] as u32).wrapping_add(0x578) {
        let sector = super::sector::sector_of(world, p, s);
        if let Some((x, y)) = super::sector::spiral_on_sector(world, a[0], a[1], 8, sector) {
            a[0] = x;
            a[1] = y;
            keep = crate::warmap::foreign_strength(world, x, y, p) < 0x10;
        }
    }
    if !keep {
        let (ax, ay) = super::sector::anchor(world, p, s);
        a = [ax, ay, 0];
        if let Some((x, y)) = pioneer_target(world, p, ax, ay) {
            a = [x, y, now as i32];
        } else if let Some(fp) = super::taskforce::first_pos(world, r5)
            && let Some((x, y)) = pioneer_target(world, p, fp.x(), fp.y())
        {
            a = [x, y, now as i32];
        }
    }
    put(world, a);
    for m in super::taskforce::members(world, r5) {
        if !keep || idle(world, m) {
            set_info_tick(world, m, now);
            super::taskforce::move_entity(world, m, a[0], a[1], true);
        }
    }
    0
}
