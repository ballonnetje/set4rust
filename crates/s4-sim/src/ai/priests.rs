//! The normal sector AI's priest task forces (`CAITaskForcePriests` and its tribe subclasses
//! Roman / Viking / Maya / Trojan, group type 8, class 3; made by the squad agent's 0x4337e0 and
//! linked to a squad by 0x433930). Own reverse engineering of the shipped exe; decompiles and
//! notes in `tools/re/decompiled/script/ai/s2/priests.c` and `more3.c`. No AI save exists to check
//! against.
//!
//! * Order (vtbl+0x20, 0x446e50): the common part (target, tick, order bits, goal −1, states 0,
//!   phase 1, timer 0) and the next cast time +0x60 = 0; order 0, or 6 (follow the linked squad:
//!   state 0x6a, phase 7). Other orders do the common part and fail.
//! * Update (vtbl+0x28, 0x446f20): from +0x60 on, the spells the player may cast (`MagicMode` 0,
//!   `SpellMask`, `SpellFlags`: the mana minus the cost — 0 with flag bit 0, halved with bit 1 — is
//!   not 0; the release tests `!= 0`, not `>= 0`); each priest in turn: the warriors within 20 of
//!   it (0x5b0f80), the tribe's pick (vtbl+0x30), the tribe's place (vtbl+0x34); the first that
//!   finds both is told to cast (0x4351a0: event 3) and the next look is 0x2d + 0..31 ticks later,
//!   else 0x1e. Then the shared states (0x4464a0, following the squad).

use super::player::TfRef;
use crate::World;
use crate::alliances::player_bit;
use crate::entity::flag;

/// The `AI` section's `CAIConfigInt`s (static initialisers 0x416c5c..0x4175dc; no config file of
/// the install sets them): per tribe the weights of the military spells' picks.
pub mod config {
    /// `…MilitarySpellChanceDefence` / `…Attack` / `…Soldier` / `…None` (all 5).
    pub const CHANCE: i32 = 5;
    /// `VikingMilitarySpellThorsHammer` (0x11844c4, 15): percent.
    pub const THORS_HAMMER: u32 = 0xf;
}

/// 0x5b0f80 `(x, y, player)`: the warriors (warrior type 2..5) of the war-map cells within 20
/// (0x5b0310) and within 20 of `(x, y)`: `[own bowmen, own, allied bowmen, allied, other bowmen,
/// other]` (allied includes own).
fn warriors_near(world: &World, x: i32, y: i32, p: u8) -> [i32; 6] {
    let allies = world.alliances.ally_bits[p.min(8) as usize];
    let mut c = [0i32; 6];
    for (cx, cy) in crate::warmap::cells_within(world, x, y, 0x14) {
        for id in world.warmap.cell(cx as usize, cy as usize, 0) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            let bit = 1u32 << (e.flags & 0xf);
            if bit & 0x3c == 0 || crate::spiral::grid_distance(e.x() - x, e.y() - y) >= 0x15 {
                continue;
            }
            let k = (bit != 8) as usize;
            c[k] += (e.owner == p) as i32;
            c[k + 2 + (allies & player_bit(e.owner) == 0) as usize * 2] += 1;
        }
    }
    [c[0], c[1] + c[0], c[2], c[3] + c[2], c[4], c[5] + c[4]]
}

/// 0x5b0770 `(x, y, r, warrior types, players)`: of the settlers in the war-map cells within `r`
/// whose warrior type is in the mask and whose owner is a player of the mask, the nearest (hex
/// distance, the first of equals): its position, −1 for none.
fn nearest_unit(world: &World, x: i32, y: i32, r: i32, types: u32, players: u16) -> i32 {
    let (mut best, mut pos) = (0x4000, -1);
    for (cx, cy) in crate::warmap::cells_within(world, x, y, r) {
        for id in world.warmap.cell(cx as usize, cy as usize, 0) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            if types & 1 << (e.flags & 0xf) == 0 || players & player_bit(e.owner) == 0 {
                continue;
            }
            let d = crate::spiral::grid_distance(e.x() - x, e.y() - y);
            if d < best {
                best = d;
                pos = e.pos.0 as i32;
            }
        }
    }
    pos
}

/// 0x445ca0 / 0x445c00: a weighted pick of the military spells: `a` with `wa` when the mask has
/// its bit, `b` with `wb` likewise, spell 4 with `w4` when the mask has 0x10, none (−1) with
/// `wn`; one RNG draw `% sum` (none drawn and −1 when the sum is 0).
fn pick(world: &mut World, w4: i32, (a, wa): (u8, i32), (b, wb): (u8, i32), wn: i32, mask: u32) -> i32 {
    let ids = [4, a as i32, b as i32, -1];
    let w = [if mask & 0x10 != 0 { w4 } else { 0 }, if mask & 1 << a != 0 { wa } else { 0 }, if mask & 1 << b != 0 { wb } else { 0 }, wn];
    let mut cum = [0u32; 4];
    let mut sum = 0u32;
    for k in 0..4 {
        if w[k] > 0 {
            sum += w[k] as u32;
        }
        cum[k] = sum;
    }
    if sum == 0 {
        return -1;
    }
    let r = world.rng.rand() % sum;
    let k = cum.iter().position(|&c| c > r).unwrap_or(3);
    ids[k]
}

/// `r < n · 0x3333` (n/5) etc.: one RNG draw.
fn chance(world: &mut World, n: u32) -> bool {
    world.rng.rand() < n
}

/// The tribe's spell pick (vtbl+0x30) on the warriors `e` near a priest with the castable `mask`:
/// Roman 0x4472d0, Viking 0x447400, Maya 0x447590, Trojan 0x447740. −1 none.
fn tribe_pick(world: &mut World, race: u8, e: [i32; 6], mut mask: u32) -> i32 {
    let c = config::CHANCE;
    match race {
        0 => {
            if e[5] < 1 || (e[5] < 5 && !chance(world, e[5].wrapping_mul(0x3333) as u32)) {
                return -1;
            }
            if e[3] < 10 && !chance(world, e[3].wrapping_mul(0x1999) as u32) {
                mask &= !0x10;
            }
            if mask & 0x70 == 0 {
                return -1;
            }
            pick(world, c, (5, c), (6, c), c, mask)
        }
        1 => {
            if mask & 0x80 != 0 && world.rng.rand() < (config::THORS_HAMMER << 16) / 100 {
                return 7;
            }
            if e[5] < 1 || (e[5] < 5 && !chance(world, e[5].wrapping_mul(0x3333) as u32)) {
                return -1;
            }
            if e[3] < 10 && !chance(world, e[3].wrapping_mul(0x1999) as u32) {
                mask &= !0x20;
            }
            if mask & 0x70 == 0 {
                return -1;
            }
            pick(world, c, (5, c), (6, c), c, mask)
        }
        2 => {
            if e[5] < 1 || (e[5] < 5 && !chance(world, e[5].wrapping_mul(0x3333) as u32)) {
                return -1;
            }
            if e[4] < 10 && !chance(world, e[4].wrapping_mul(0x1999) as u32) {
                mask &= !0x20;
            }
            if e[3] < 10 && !(e[3] > 4 && chance(world, (e[3] - 5).wrapping_mul(0x3333) as u32)) {
                mask &= !0x40;
            }
            if mask & 0x70 == 0 {
                return -1;
            }
            pick(world, c, (5, c), (6, c), c, mask)
        }
        4 => {
            if e[5] < 1 || (e[5] < 5 && !chance(world, e[5].wrapping_mul(0x3333) as u32)) {
                return -1;
            }
            if e[5] == e[4] {
                mask &= !0x10;
            }
            if e[5] < 10 && !chance(world, (e[5] - 5).wrapping_mul(0x3333) as u32) {
                mask &= !0x40;
            }
            if mask & 0x70 == 0 {
                return -1;
            }
            pick(world, c, (6, c), (5, 0), c, mask)
        }
        _ => -1,
    }
}

/// The tribe's place for spell `spell` cast by a priest at `(x, y)` (vtbl+0x34): Roman 0x447370,
/// Viking 0x4474d0, Maya 0x447660, Trojan 0x4477f0. A packed position, −1 none.
fn tribe_place(world: &mut World, race: u8, p: u8, spell: i32, x: i32, y: i32) -> i32 {
    let own = player_bit(p);
    let enemies = world.alliances.enemy_bits[p.min(8) as usize];
    match (race, spell) {
        (0, 4) => {
            let v = nearest_unit(world, x, y, 0xf, 0x24, own);
            if v < 1 { nearest_unit(world, x, y, 0x14, 0x3c, own) } else { v }
        }
        (0, 5 | 6) => nearest_unit(world, x, y, 0x14, 0x3c, enemies),
        (1, 4 | 6) => nearest_unit(world, x, y, 0x14, 0x3c, enemies),
        (1, 5) => nearest_unit(world, x, y, 0x14, 0x3c, own),
        (1, 7) => {
            let (b, _) = super::attack::nearest_enemy_building(world, x, y, 0x14, enemies);
            if b == 0 {
                return -1;
            }
            super::sector::ensign(world, b).map_or(-1, |e| e.0 as i32)
        }
        (2, 4) => {
            let side = world.grid.size() as i32 - 0x20;
            if side < 1 {
                return -1;
            }
            let side = side as u32;
            for _ in 0..5 {
                let r1 = world.rng.rand();
                let r2 = world.rng.rand();
                let (nx, ny) = ((r1 % side + 0x10) as i32, (r2 % side + 0x10) as i32);
                if super::player::free_node_dist(world, nx, ny).is_some() {
                    return (ny << 16) + nx;
                }
            }
            -1
        }
        (2, 5) => nearest_unit(world, x, y, 0x14, 8, enemies),
        (2, 6) => nearest_unit(world, x, y, 0x14, 0x3c, own),
        (4, 4) => {
            let (b, _) = super::attack::nearest_enemy_building(world, x, y, 0x14, own);
            world.entities.by_index(b as u32).filter(|_| b != 0).map_or(-1, |e| e.pos.0 as i32)
        }
        (4, 6) => nearest_unit(world, x, y, 0x14, 0x3c, enemies),
        _ => -1,
    }
}

/// The castable spells (0x446f20..0x447202): `MagicMode` 0 (var 8), then per bit of `SpellMask`
/// (var 9) the spell whose cost — 0 with `SpellFlags` (var 10) bit 0, `cost >> 1` with bit 1 — is
/// not exactly the mana (`mana − cost != 0`).
fn castable(world: &World, p: u8) -> u32 {
    let rec = world.ai.vars.records.get(p as usize).copied().unwrap_or_default();
    if rec.get(8) != 0 {
        return 0;
    }
    let (mask, flags) = (rec.get(9) as u32, rec.get(10) as u32);
    let Some(pool) = world.magic.pool(p) else { return 0 };
    let mut out = 0;
    for i in 0..8 {
        if mask & 1 << i == 0 {
            continue;
        }
        let cost = if flags & 1 != 0 { 0 } else { pool.costs[i] >> (flags >> 1 & 1) };
        if pool.mana.wrapping_sub(cost) != 0 {
            out |= 1 << i;
        }
    }
    out
}

/// 0x4351a0 `(priest, spell, pos)`: a READY priest (settler type 0x2d) gets event 3 `(spell,
/// owner, 0, pos)` — the cast order.
fn order_cast(world: &mut World, id: u16, spell: i32, pos: i32) {
    let Some(e) = world.entities.by_index(id as u32) else { return };
    if e.obj_type != crate::entity::ObjType::Settler || e.flags & flag::READY == 0 || e.kind != 0x2d {
        return;
    }
    let owner = e.owner as i32;
    super::taskforce::send_event(world, id, crate::jobs::state::EntityEvent { event: 3, kind: spell, data: [owner, 0, pos] });
}

/// `CAITaskForcePriests` update (0x446f20) before the shared states.
pub(crate) fn update(world: &mut World, r: TfRef) {
    let now = world.ai.tick;
    let Some(t) = super::player::tf(world, r) else { return };
    if now < t.next_cast {
        return;
    }
    let p = (t.flags() & 0xf) as u8;
    let race = world.race_of(p);
    let mask = castable(world, p);
    let mut next = now.wrapping_add(0x1e);
    for m in super::taskforce::members(world, r) {
        let Some(pos) = world.entities.by_index(m as u32).map(|e| e.pos) else { continue };
        let e = warriors_near(world, pos.x(), pos.y(), p);
        let spell = tribe_pick(world, race, e, mask);
        if spell < 0 {
            continue;
        }
        let at = tribe_place(world, race, p, spell, pos.x(), pos.y());
        if at < 0 {
            continue;
        }
        order_cast(world, m, spell, at);
        let r2 = world.rng.rand();
        next = now.wrapping_add(0x2d).wrapping_add(r2 & 0x1f);
        break;
    }
    if let Some(t) = super::player::tf_mut(world, r) {
        t.next_cast = next;
    }
}
