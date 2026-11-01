//! `CAIShaman` (the one static object at 0x1184440, vtable 0x102b2fc: vtbl+0 0x4440d0, +4
//! 0x444340, +8 0x4443a0, +0xc 0x4444d0; +0x10 on are not filled — pure calls): where the Dark
//! Tribe sends its shamans and manakopters (vtbl+4 0x444340, the nearest enemy civilian, is not
//! ported: no caller yet). Own reverse engineering of the shipped exe;
//! decompiles in `tools/re/decompiled/script/manakopter/aishaman.c`.
//!
//! [`target`] (vtbl+0) picks what to go for: the boards or iron the manakopter hall still lacks
//! (the larger shortfall; iron on a tie), otherwise with a 70 % chance (one game RNG draw below
//! 0xb333) slaves, otherwise kind 0. Each kind has four building types (.rdata 0xff0768):
//! kind 0 `[0x29, 0x2a, 4, 1]`, boards `[3, 0x22, 0x1f, 0x17]`, iron `[0x16, 0x14, 0x22, 0x15]`,
//! slaves `[0xb, 0x24, 0x52, 0x23]`. From a random one of them (one game RNG draw) it scores the
//! enemy players' buildings of those types in turn ([`score`], 0x443fe0) and takes the first
//! enemy player with a positive score: the best building's ensign, moved for boards / iron to
//! the nearest pile of that good on its eco-sector ([`pile_near`], vtbl+8) and for slaves to
//! the first field within 25 ([`field_near`], vtbl+0xc).

use crate::World;
use crate::entity::{EntityData, ObjType};

/// .rdata 0xff0768: the building types of each target kind (0xff0744: four each).
const TYPES: [[u16; 4]; 4] = [[0x29, 0x2a, 4, 1], [3, 0x22, 0x1f, 0x17], [0x16, 0x14, 0x22, 0x15], [0xb, 0x24, 0x52, 0x23]];

/// The `CAIShaman` members a search sets (+4 player, +8 team, +0xc enemies, +0x10 / +0x14 the
/// start, +0x18 its sector).
struct Ctx {
    team: usize,
    enemies: u32,
    sector: u16,
}

fn sector_at(world: &World, x: i32, y: i32) -> u16 {
    // The tile map 0x12f1a18 (set by the tiling init 0x5b7230) → tile +0 [inf]: the sector of
    // `ITiling`, as 0x15630c4 gives it.
    world.grid.checked_index(x, y).map_or(0, |n| world.tiling.sector_of(n))
}

/// 0x443fe0 `(player, type, &x, &y, &best, any_sector)`: the buildings of `player` and `type`
/// (`CBuildingMgr` list) with their ensign on the search's sector (or any, with `any_sector`):
/// `100 (when any) + 8 · civilians of the enemies within 24 cells (0x5b0dd0) + foreign
/// strength of the ensign's cell − 0`, i.e. `+ total − own team`; a better one than `best`
/// becomes it and its ensign the position.
fn score(world: &World, c: &Ctx, player: u8, kind: u16, pos: &mut (i32, i32), best: &mut i32, any_sector: bool) {
    for b in world.building_list(player as i32, kind as i32) {
        let ensign = match world.entities.by_index(b as u32).map(|e| &e.data) {
            Some(EntityData::Building(d)) => d.ensign,
            _ => continue,
        };
        let (ex, ey) = (ensign.x(), ensign.y());
        if sector_at(world, ex, ey) as u32 != c.sector as u32 && !any_sector {
            continue;
        }
        let cell = world.warmap.cell_strength(ex >> 4, ey >> 4);
        let own = cell.get(c.team).copied().unwrap_or(0) as i32;
        let (n, _) = crate::warmap::civilians_near(world, ex, ey, 0x18, c.enemies);
        let s = (if n > 0 { 100 } else { 0 }) + n * 8 - (own - cell[0] as i32);
        if s > *best {
            *best = s;
            *pos = (ex, ey);
        }
    }
}

/// vtbl+0 (0x4440d0) `(player, enemies, &x, &y, any_sector)`: see the module doc. `None`
/// (false) when `(x, y)` is on no sector or no enemy player in `enemies` (bit `p − 1`) gives a
/// positive score.
pub fn target(world: &mut World, player: u8, enemies: u32, x: i32, y: i32, any_sector: bool) -> Option<(i32, i32)> {
    let c = Ctx { team: world.alliances.team(player), enemies, sector: sector_at(world, x, y) };
    let mut kind = 0usize;
    let mut need = false;
    if let Some(hall) = crate::buildings::manakopter::hall_of(world, player)
        && world.entities.by_index(hall as u32).is_some_and(|e| e.flags & crate::entity::flag::READY != 0)
        && let Some(s) = crate::buildings::manakopter::get(world, hall)
    {
        let req = world.rules.magic.dark.manakopter_required;
        let (boards, iron) = (req[1] - s.stock[1], req[2] - s.stock[2]);
        let (good, left) = if iron < boards { (7, boards) } else { (20, iron) };
        if left > 0 {
            kind = if good == 7 { 1 } else { 2 };
            need = true;
        }
    }
    if !need && world.rng.rand() < 0xb333 {
        kind = 3;
    }
    if c.sector == 0 {
        return None;
    }
    let n = TYPES[kind].len();
    let r = world.rng.rand() as usize % n;
    let mut pos = (0, 0);
    let mut best = 0;
    for p in 1..=world.player_count() {
        if enemies & crate::alliances::player_bit(p) as u32 == 0 {
            continue;
        }
        for j in 0..n - 1 {
            score(world, &c, p, TYPES[kind][(r + j) % n], &mut pos, &mut best, any_sector);
        }
        if best <= 0 {
            score(world, &c, p, TYPES[kind][(r + n - 1) % n], &mut pos, &mut best, any_sector);
        }
        if best > 0 {
            match kind {
                1 => {
                    pile_near(world, &mut pos, 7);
                }
                2 => {
                    pile_near(world, &mut pos, 20);
                }
                3 => {
                    if let Some(p) = field_near(world, pos.0, pos.1, 0x19) {
                        pos = p;
                    }
                }
                _ => {}
            }
            return Some(pos);
        }
    }
    None
}

/// vtbl+8 (0x4443a0) `(&x, &y, good)`: of the piles offering `good` on the eco-sector of
/// `(x, y)`, the nearest (hex distance, the first of equals) with goods not yet reserved
/// (+0x41 amount > +0x42 leaving); `(x, y)` becomes its position. Returns the pile (0 = none).
pub fn pile_near(world: &World, pos: &mut (i32, i32), good: u8) -> u16 {
    let Some(n) = world.grid.checked_index(pos.0, pos.1) else { return 0 };
    let eco = crate::eco::eco_of(world, n);
    let Some(es) = world.economy.get(eco).filter(|_| eco != 0) else { return 0 };
    let mut i = es.good_offer.get(good as usize).copied().unwrap_or(0);
    let (mut best, mut found) = (0xff_ffff, 0u16);
    let mut at = *pos;
    while let Some(e) = world.entities.by_index(i as u32).filter(|e| e.obj_type == ObjType::Pile) {
        let d = crate::spiral::grid_distance(e.x() - pos.0, e.y() - pos.1);
        if let EntityData::Pile(p) = &e.data
            && d < best
            && p.leaving < p.amount
        {
            best = d;
            found = i;
            at = (e.x(), e.y());
        }
        i = e.next;
    }
    if found != 0 {
        *pos = at;
    }
    found
}

/// vtbl+0xc (0x4444d0) `(&x, &y, radius)`: the first node of the spiral `0..=Last(radius)`
/// around `(x, y)` (on the map) whose deco object is a grain (209, 210), agave (211), hive (217)
/// or sunflower (255) field — not a vine.
pub fn field_near(world: &World, x: i32, y: i32, radius: u8) -> Option<(i32, i32)> {
    crate::spiral::walk(world.grid, x, y, radius).find(|&(px, py)| {
        let n = world.grid.index(px as u32, py as u32);
        let id = world.layers.deco_object[n];
        world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Deco && matches!(e.kind, 209 | 210 | 211 | 217 | 255))
    })
}
