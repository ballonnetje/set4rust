//! The spell functions (.rdata 0x1000838 `[race·8 + spell]`, `this` = the [`SpellCtx`]; own
//! reverse engineering of the shipped exe, SIM.md §20). Each returns what the original returns:
//! 0 = nothing happened (the cast fails and costs nothing), otherwise usually how much it did.
//!
//! Most walk `CSpiralWalk(x, y, radius)` (0x4312e0: spiral entries `0..=Last(radius)` on the
//! map, [`crate::spiral::walk`]). The effects they start (`CEffects::AddEffect`, vtbl+0x10 of
//! `GoodDistributionInfoHolder +0x24`) are [`SimEvent::Effect`]; the random numbers the original
//! draws for an effect's start frame are drawn here as well.

use super::SpellCtx;
use crate::World;
use crate::entity::{EntityData, ObjType};
use crate::event::SimEvent;

fn effect(world: &mut World, effect: u8, sound: u16, x: i32, y: i32) {
    world.events.push(SimEvent::Effect { effect, sound, x, y });
}

fn nodes(world: &World, ctx: &SpellCtx) -> impl Iterator<Item = (i32, i32)> + use<> {
    crate::spiral::walk(world.grid, ctx.x, ctx.y, ctx.radius.clamp(0, 255) as u8)
}

fn cfg_at(table: &[[i32; super::SPELLS]; super::RACES], race: u8, i: usize) -> i32 {
    table.get(race as usize).and_then(|t| t.get(i)).copied().unwrap_or(0)
}

/// The deco-object layer entry of node `n + off` (a flat offset; off the array → none).
fn deco_at(world: &World, n: usize, off: i64) -> u16 {
    usize::try_from(n as i64 + off).ok().and_then(|m| world.layers.deco_object.get(m).copied()).unwrap_or(0)
}

fn is_pile(world: &World, id: u16) -> bool {
    id != 0 && world.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Pile)
}

/// 0x4f35d0, spell 0 of every race but the Dark Tribe: gifts. On free land (no flags 0x33, no
/// map or deco object, ground 0x1X, 0x4X or 0x9X, no pile on the six neighbours) of the spiral a
/// pile of a drawn present: `Rand() % PresentChances[7]`, the first `i` with the draw below
/// `PresentChances[i]` gives `PresentGoods[i]` (a good 1..42) and `PresentAmounts[i]` (from 2 on
/// less `Rand() % ((n + 2) / 2)`, at most 8). The pile (an anywhere pile) is MAGIC_INVISIBLE
/// until future event 1 after `4 + Rand() % 4` ticks; effect 0x18. Up to the limit; returns how
/// many piles.
pub fn gift(world: &mut World, ctx: &SpellCtx) -> i32 {
    let race = ctx.race;
    let size = world.grid.size() as i64;
    let neighbours = [1, size + 1, size, -1, -size - 1, -size];
    let mut count = 0;
    for (x, y) in nodes(world, ctx) {
        let n = world.grid.index(x as u32, y as u32);
        let l = &world.layers;
        let g = l.ground[n] & 0xf0;
        if l.flags[n] & 0x33 != 0 || l.map_object[n] != 0 || l.deco_object[n] != 0 || !matches!(g, 0x10 | 0x40 | 0x90) {
            continue;
        }
        if neighbours.iter().any(|&o| is_pile(world, deco_at(world, n, o))) {
            continue;
        }
        let cfg = &world.rules.magic;
        let total = cfg_at(&cfg.present_chances, race, 7);
        let r = if total > 0 { (world.rng.rand() % total as u32) as i32 } else { 0 };
        let cfg = &world.rules.magic;
        let i = (0..8).find(|&i| r < cfg_at(&cfg.present_chances, race, i)).unwrap_or(8);
        // Index 8 reads past the table in the original; with any sane config it is never reached.
        let good = if i < 8 { cfg_at(&cfg.present_goods, race, i) } else { 0 };
        if !(1..0x2b).contains(&good) {
            continue;
        }
        let mut amount = cfg_at(&cfg.present_amounts, race, i);
        if amount < 2 {
            amount = 1;
        } else {
            let r = world.rng.rand();
            amount -= (r % ((amount + 2) >> 1) as u32) as i32;
            amount = amount.min(8);
        }
        let Some(pile) = crate::pile::add_pile(world, x, y, good as u8, amount.clamp(0, 255) as u8, crate::pile::role::ANYWHERE) else { continue };
        if let Some(e) = world.entities.by_index_mut(pile as u32) {
            e.flags |= crate::entity::flag::MAGIC_INVISIBLE;
        }
        let r = world.rng.rand() & 3;
        effect(world, 0x18, 0, x, y);
        crate::future::add(world, 1, r + 4, pile, 0, crate::entity::flag::MAGIC_INVISIBLE as i32);
        count += 1;
        if ctx.limit <= count {
            return count;
        }
    }
    count
}

/// 0x4f3850, spell 1 of every race but the Dark Tribe: goods of `ConvertGood<race>Src` on the
/// spiral's piles become `ConvertGood<race>Dst` (both 1..42), pile by pile while the converted
/// total stays at most `limit + 2` (10 below a limit of 8) — the pile's role decides (vtbl+0x48:
/// anywhere and trade piles, [`crate::pile::change_good_type`]); effect 0x1b. Stops once the total
/// reaches `limit − 2` (8 below a limit of 10). Returns the total.
pub fn convert(world: &mut World, ctx: &SpellCtx) -> i32 {
    let cfg = &world.rules.magic;
    let src = cfg.convert_good_src.get(ctx.race as usize).copied().unwrap_or(0);
    let dst = cfg.convert_good_dst.get(ctx.race as usize).copied().unwrap_or(0);
    if !(1..0x2b).contains(&src) || !(1..0x2b).contains(&dst) {
        return 0;
    }
    let lo = if ctx.limit < 10 { 8 } else { ctx.limit - 2 };
    let hi = if ctx.limit < 8 { 10 } else { ctx.limit + 2 };
    let mut total = 0;
    for (x, y) in nodes(world, ctx) {
        let n = world.grid.index(x as u32, y as u32);
        let p = world.layers.deco_object[n];
        let Some(d) = crate::pile::get(world, p).filter(|_| is_pile(world, p)) else { continue };
        if d.good as i32 != src {
            continue;
        }
        let t = d.amount as i32 + total;
        if t > hi || !crate::pile::change_good_type(world, p, dst as u8) {
            continue;
        }
        let _ = world.rng.rand() & 3;
        effect(world, 0x1b, 0, x, y);
        total = t;
        if lo <= total {
            return total;
        }
    }
    total
}

/// The five effects the Roman resource spells start around the target (delays 0, 3, 6, 10, 13).
fn five_effects(world: &mut World, ctx: &SpellCtx, e: u8) {
    for (dx, dy) in [(0, 0), (-2, 1), (-1, -3), (1, -1), (3, 2)] {
        effect(world, e, 0, ctx.x + dx, ctx.y + dy);
    }
}

/// 0x4f3b60, the Romans' spell 2: fish. Water nodes (ground 0x0X) of the spiral whose resource is
/// fish (type 0) or empty and below 15 get `1 + (r & 1) + ((r & 3) >> 1)` more (one draw, at most
/// 15; the type becomes fish), until as much as the limit was added. Full water still counts as 1
/// when nothing else was added. Effects 0x30. Returns the amount added.
pub fn fish(world: &mut World, ctx: &SpellCtx) -> i32 {
    let mut added = 0;
    for (x, y) in nodes(world, ctx) {
        let n = world.grid.index(x as u32, y as u32);
        if world.layers.ground[n] & 0xf0 != 0 {
            continue;
        }
        let b = world.layers.resource[n];
        let amount = (b & 0xf) as u32;
        if (b & 0xf0 == 0 || amount == 0) && amount < 15 {
            let r = world.rng.rand();
            let v = ((r & 1) + 1 + ((r & 3) >> 1) + amount).min(15);
            world.layers.resource[n] = v as u8;
            added += (v - amount) as i32;
            if ctx.limit <= added {
                break;
            }
        } else if added == 0 {
            added = 1;
        }
    }
    if added > 0 {
        five_effects(world, ctx, 0x30);
    }
    added
}

/// 0x4f3fd0, the Romans' spell 7: mountains (ground 0x2X) with a resource of 1..14 get
/// `amount + amount / 4 + 1 + (Rand() & 3)` (at most 15, the type kept) until as much as the
/// limit was added; a node with none or 15 counts as 1 when nothing else was added. Effects 0x24.
pub fn mountains(world: &mut World, ctx: &SpellCtx) -> i32 {
    let mut added = 0;
    for (x, y) in nodes(world, ctx) {
        let n = world.grid.index(x as u32, y as u32);
        if world.layers.ground[n] & 0xf0 != 0x20 {
            continue;
        }
        let b = world.layers.resource[n];
        let amount = (b & 0xf) as u32;
        if amount == 0 || amount > 14 {
            added += (added == 0) as i32;
            continue;
        }
        let r = world.rng.rand();
        let v = ((r & 3) + 1 + amount + (amount >> 2)).min(15);
        world.layers.resource[n] = (b & 0xf0) | v as u8;
        added += (v - amount) as i32;
        if ctx.limit <= added {
            break;
        }
    }
    if added > 0 {
        five_effects(world, ctx, 0x24);
    }
    added
}

/// 0x4f4190, the Vikings' spell 2: animals. On grass (ground 0x1X) without blocked land, map or
/// deco object, and with no settler or other map object on unblocked nodes of hex rings 1 and 2
/// (0x145414c: `g_sSurroundingHexPoints8` entries 1..18), an animal of type 5 (`CAnimalMgr`
/// 0x52e6e0) appears, MAGIC_INVISIBLE until future event 1 after `4 + (count & 7)` ticks. Up to
/// the limit; returns how many.
pub fn animals(world: &mut World, ctx: &SpellCtx) -> i32 {
    let size = world.grid.size() as i64;
    let ring: Vec<i64> = crate::tiling::hex_points(2).iter().skip(1).map(|&(dx, dy)| dy as i64 * size + dx as i64).collect();
    let mut count = 0;
    for (x, y) in nodes(world, ctx) {
        let n = world.grid.index(x as u32, y as u32);
        let l = &world.layers;
        if l.ground[n] & 0xf0 != 0x10 || l.flags[n] & 1 != 0 || l.map_object[n] != 0 || l.deco_object[n] != 0 {
            continue;
        }
        let busy = ring.iter().any(|&o| {
            usize::try_from(n as i64 + o).ok().filter(|&m| m < l.flags.len()).is_some_and(|m| l.flags[m] & 1 == 0 && l.map_object[m] != 0)
        });
        if busy {
            continue;
        }
        let Some(a) = crate::animals::add_at(world, x, y, 5) else { continue };
        if let Some(e) = world.entities.by_index_mut(a as u32) {
            e.flags |= crate::entity::flag::MAGIC_INVISIBLE;
        }
        crate::future::add(world, 1, (count as u32 & 7) + 4, a, 0, crate::entity::flag::MAGIC_INVISIBLE as i32);
        count += 1;
        if ctx.limit <= count {
            return count;
        }
    }
    count
}

/// 0x4f4450, the Maya's spell 2: crops. Every plant (`CPlant`: grain, wine, agave, sunflower) on
/// ground 0x1X or 0x4X of the spiral gets entity flag 0x100000 (it then grows every
/// `(FasterCropsDecrease256 · 31 + 127) / 256` ticks instead of 31, `deco::plant`); another deco
/// object there counts 1 if nothing else did. No limit. Effect 0x25 at the target. Shared static
/// plant instances get the flag for every node that shows them.
pub fn crops(world: &mut World, ctx: &SpellCtx) -> i32 {
    let mut count = 0;
    for (x, y) in nodes(world, ctx) {
        let n = world.grid.index(x as u32, y as u32);
        if !matches!(world.layers.ground[n] & 0xf0, 0x10 | 0x40) {
            continue;
        }
        let d = world.layers.deco_object[n];
        if d == 0 {
            continue;
        }
        match world.entities.by_index_mut(d as u32) {
            Some(e) if matches!(e.data, EntityData::Deco(_)) && crate::deco::plant::is_plant(e.kind) => {
                e.flags |= crate::deco::plant::FLAG_FAST;
                count += 1;
            }
            _ => count += (count == 0) as i32,
        }
    }
    if count > 0 {
        effect(world, 0x25, 0, ctx.x, ctx.y);
    }
    count
}

/// 0x4f4e40, the Trojans' spell 2: every finished building of the caster within the radius
/// (the war map's building lists of the cells around, 0x5b0310 / 0x5b0420: READY, hex distance
/// at most the radius) that is a mine gets effect 0x6a (sound 0x3b; one draw) and its first food
/// pile filled up (`8 − amount − coming`, `CPile` 0x56f870). No limit. Returns how many mines.
pub fn mines(world: &mut World, ctx: &SpellCtx) -> i32 {
    let mut count = 0;
    for b in buildings_within(world, ctx.x, ctx.y, ctx.radius) {
        let Some(e) = world.entities.by_index(b as u32) else { continue };
        if e.owner != ctx.player {
            continue;
        }
        let Some(food) = crate::buildings::mine::get(world, b).map(|s| s.food.first().copied().unwrap_or(0)) else { continue };
        let (bx, by) = (e.x(), e.y());
        let _ = world.rng.rand() & 7;
        effect(world, 0x6a, 0x3b, bx, by);
        if let Some(p) = crate::pile::get(world, food).filter(|_| is_pile(world, food)) {
            let n = 8 - p.coming as i32 - p.amount as i32;
            if n > 0 {
                crate::pile::increase_unforeseen(world, food, n);
            }
        }
        count += 1;
    }
    count
}

/// 0x5b0310 + 0x5b0420: the READY buildings of the war map's building lists (category 2) of the
/// cells around `(x, y)` within `r` (cell order of [`crate::warmap::cells_within`], list order),
/// within hex distance `r`.
pub fn buildings_within(world: &World, x: i32, y: i32, r: i32) -> Vec<u16> {
    let mut out = Vec::new();
    for (cx, cy) in crate::warmap::cells_within(world, x, y, r) {
        for id in world.warmap.cell(cx as usize, cy as usize, 2) {
            let Some(e) = world.entities.by_index(id as u32) else { continue };
            if e.obj_type == ObjType::Building
                && e.flags & crate::entity::flag::READY != 0
                && crate::spiral::grid_distance(e.x() - x, e.y() - y) <= r
            {
                out.push(id);
            }
        }
    }
    out
}

/// The ground of conversion level 0..3 per kind (.rdata 0x1000798, 5 per kind): kind 0 the lava
/// family (found on 0x80 / 0x81 / 0x23 ground) → rock, kinds 1..5 = race + 1 (Roman desert →
/// grass, Viking 0x4X → grass, Maya grass → 0x40, Trojan grass → 0x90); kind 4 is dark land
/// (made light instead).
const TERRAIN: [[u8; 5]; 6] = [
    [0x80, 0x81, 0x23, 0x20, 0],
    [0x50, 0x51, 0x15, 0x10, 0],
    [0x40, 0x41, 0x14, 0x10, 0],
    [0x10, 0x14, 0x41, 0x40, 0],
    [0, 0, 0, 0, 0],
    [0x10, 0x17, 0x91, 0x90, 0],
];

/// 0x4e4e40 `(ground, kind)`: the ground's level in the kind's family (0..3), 4 for ground not
/// in it; every ground is level 0 for dark land (kind 4).
fn level(ground: u8, kind: usize) -> usize {
    if kind == 4 {
        return 0;
    }
    TERRAIN.get(kind).and_then(|t| t[..4].iter().position(|&g| g == ground)).unwrap_or(4)
}

/// 0x4f39d0, spell 3 of every race but the Dark Tribe: terrain. The spiral of `radius − 2`
/// around the target is searched for the first node that decides the kind: ground 0x80, 0x81 or
/// 0x23 → kind 0; dark land (flag 0x04) → kind 4 over 5 rings; ground of `TerrainSrc`'s class
/// (`& 0xf0`; none when that is 0 or the source equals `TerrainDst`) → kind `race + 1`. None →
/// −1 (which counts as cast). Otherwise effect 0x1e and future event 4 for every ring
/// `0..=radius`, ring `r` due after `r + 1` ticks ([`ring`]). Returns the kind — 0 for the lava
/// family, so that cast fails (no mana taken) although its events run.
pub fn terrain(world: &mut World, ctx: &SpellCtx) -> i32 {
    let race = ctx.race as usize;
    let cfg = &world.rules.magic;
    let (src, dst) = (cfg.terrain_src.get(race).copied().unwrap_or(0), cfg.terrain_dst.get(race).copied().unwrap_or(0));
    let class = if src & 0xf0 == 0 || src == dst { -1 } else { src & 0xf0 };
    let r = (ctx.radius - 2).clamp(0, 255) as u8;
    let mut found = None;
    for (x, y) in crate::spiral::walk(world.grid, ctx.x, ctx.y, r) {
        let n = world.grid.index(x as u32, y as u32);
        let g = world.layers.ground[n];
        if matches!(g, 0x80 | 0x81 | 0x23) {
            found = Some((0, ctx.radius));
            break;
        }
        if world.layers.flags[n] & 4 != 0 {
            found = Some((4, 5));
            break;
        }
        if (g & 0xf0) as i32 == class {
            found = Some((ctx.race as i32 + 1, ctx.radius));
            break;
        }
    }
    let Some((kind, rings)) = found else { return -1 };
    effect(world, 0x1e, 0, ctx.x, ctx.y);
    let a = ctx.y << 16 | (ctx.x & 0xffff);
    for u in 0..=rings.max(-1) {
        crate::future::add(world, 4, u as u32 + 1, 0, a, kind << 16 | u);
    }
    kind
}

/// Future event 4 (0x4e5320): ring `b & 0xffff` (spiral entries `First..=Last`) around the
/// target `a`: every node at least 3 from the map's edge converts towards level 3 of kind
/// `b >> 16` ([`convert_node`]).
pub fn ring(world: &mut World, a: i32, b: i32) {
    let (x, y) = (a & 0xffff, (a as u32 >> 16) as i32);
    let size = world.grid.size() as i32;
    if x >= size || y >= size {
        return;
    }
    let (r, kind) = ((b & 0xffff) as usize, (b as u32 >> 16) as usize);
    let (first, last) = (crate::spiral::first(r.min(255) as u8), crate::spiral::last(r.min(255) as u8));
    for i in first..=last {
        let (px, py) = (x + crate::spiral::dx(i), y + crate::spiral::dy(i));
        if ((px - 3) as u32) < (size - 6) as u32 && ((py - 3) as u32) < (size - 6) as u32 {
            convert_node(world, px, py, 3, kind);
        }
    }
}

/// 0x4e5100 `(x, y, level, kind)`: at level 3 nothing happens near ground outside the family
/// (level 4 on the spiral of radius 3). A node below `level` becomes the family's ground of that
/// level (`CWorldManager` 0x60cbd0: the ground byte; ground of the no-dark set (water, 0x5X,
/// 0x8X) blocks the land, other ground clears it and gets move cost 2; the deco object there
/// goes, 0x564cc0) — dark land is made light instead (`CDarkTribe` vtbl+0x10 with player 1) —
/// and its six neighbours follow at `level − 1`.
fn convert_node(world: &mut World, x: i32, y: i32, lvl: usize, kind: usize) {
    if lvl == 3 {
        let near_foreign = crate::spiral::walk(world.grid, x, y, 3).any(|(a, b)| level(world.layers.ground[world.grid.index(a as u32, b as u32)], kind) == 4);
        if near_foreign {
            return;
        }
    }
    let Some(n) = world.grid.checked_index(x, y) else { return };
    if !(1..=3).contains(&lvl) || level(world.layers.ground[n], kind) >= lvl {
        return;
    }
    if kind == 4 {
        crate::dark::lighten(world, x, y, 1);
    } else {
        set_ground(world, x, y, TERRAIN[kind.min(5)][lvl]);
        crate::deco::remove_at(world, x, y, false);
        world.update_world_position(x, y); // 0x4e522a
    }
    for (dx, dy) in crate::jobs::walking::NEIGHBOR {
        convert_node(world, x + dx, y + dy, lvl - 1, kind);
    }
}

/// `CWorldManager` 0x60cbd0 `(x, y, ground)`.
fn set_ground(world: &mut World, x: i32, y: i32, g: u8) {
    let Some(n) = world.grid.checked_index(x, y) else { return };
    world.layers.ground[n] = g;
    if matches!(g, 0..=8 | 0x50 | 0x51 | 0x80 | 0x81) {
        world.set_blocked_land(x, y);
        return;
    }
    world.clear_blocked_land(x, y);
    world.layers.walk_move[n] = (world.layers.walk_move[n] & 0xfa) | 2;
}

// ---- soldier spells ----------------------------------------------------------------------------

/// Entity flag 0x100000: bloodlust (the Vikings' spell 5): a soldier's hit grows by
/// `BloodlustDmgIncrease256` (0x589328, `jobs::warrior::hit`).
pub const FLAG_BLOODLUST: u32 = 0x10_0000;
/// Entity flag 0x200000: shield (the Romans' spell 4): damage taken times
/// `ShieldDmgDecrease256 / 256` (0x580051, `combat`).
pub const FLAG_SHIELD: u32 = 0x20_0000;
/// Entity flag 0x400000 (the Vikings' spell 6): the soldier only idles about (0x589217).
pub const FLAG_CONFUSED: u32 = 0x40_0000;
/// Entity flag 0x800000 (the Vikings' spell 4, also the blowgun): the soldier stands (0x5891ee).
pub const FLAG_FROZEN: u32 = 0x80_0000;
/// Entity flag 0x8000000 (the Trojans' spell 5): no enemy targets the soldier (0x5981cc,
/// 0x58cba1) until it attacks (0x58a75e).
pub const FLAG_HIDDEN: u32 = 0x800_0000;

fn allies(world: &World, player: u8) -> u32 {
    world.alliances.ally_bits.get(player as usize).copied().unwrap_or(0) as u32
}

fn enemies(world: &World, player: u8) -> u32 {
    world.alliances.enemy_bits.get(player as usize).copied().unwrap_or(0) as u32
}

/// 0x5b1c60 with 0x547f20: the next settler on the spiral `0..=Last(r)` around `(x, y)` (from
/// entry `*k` on) that is READY, has a warrior type in `types` (bit per type) and an owner in
/// `players` (`CAlliances` bits); `*k` moves past it. Evaluated one at a time, as the original
/// does, so what a spell does to one settler counts for the next.
fn next_settler(world: &World, x: i32, y: i32, r: i32, k: &mut usize, players: u32, types: u32) -> Option<(u16, i32, i32)> {
    let last = crate::spiral::last(r.clamp(0, 255) as u8);
    while *k <= last {
        let (px, py) = (x + crate::spiral::dx(*k), y + crate::spiral::dy(*k));
        *k += 1;
        let Some(n) = world.grid.checked_index(px, py) else { continue };
        let m = world.layers.map_object[n];
        let Some(e) = world.entities.by_index(m as u32).filter(|e| m != 0 && e.obj_type == ObjType::Settler) else { continue };
        let bit = crate::alliances::player_bit(e.owner) as u32;
        if e.flags & crate::entity::flag::READY != 0 && types & (1 << (e.flags & 0xf)) != 0 && players & bit != 0 {
            return Some((m, px, py));
        }
    }
    None
}

/// 0x4f34f0 `(players, types, flag, effect)`: every matching settler in the radius without
/// `flag` gets it for the spell's duration (future event 1 clears it) and the effect; up to the
/// limit. Returns how many. Used by the Romans' spell 4 (shield: allies, types 2..5), the
/// Vikings' spells 4 (freeze: enemies 2..5), 5 (bloodlust: allies 2, 4, 5) and 6 (confusion:
/// enemies 2..5) and the Trojans' spell 5 (hidden: own soldiers 2..5).
fn flag_settlers(world: &mut World, ctx: &SpellCtx, players: u32, types: u32, flag: u32, eff: u8) -> i32 {
    let (mut k, mut count) = (0, 0);
    while let Some((s, x, y)) = next_settler(world, ctx.x, ctx.y, ctx.radius, &mut k, players, types) {
        let Some(e) = world.entities.by_index_mut(s as u32) else { continue };
        if e.flags & flag != 0 {
            continue;
        }
        e.flags |= flag;
        crate::future::add(world, 1, ctx.duration.max(0) as u32, s, 0, flag as i32);
        effect(world, eff, 0, x, y);
        count += 1;
        if ctx.limit <= count {
            break;
        }
    }
    count
}

/// 0x4f3d00, the Romans' spell 4.
pub fn shield(world: &mut World, ctx: &SpellCtx) -> i32 {
    flag_settlers(world, ctx, allies(world, ctx.player), 0x3c, FLAG_SHIELD, 0x36)
}

/// 0x4f42e0, the Vikings' spell 4.
pub fn freeze(world: &mut World, ctx: &SpellCtx) -> i32 {
    flag_settlers(world, ctx, enemies(world, ctx.player), 0x3c, FLAG_FROZEN, 0x2d)
}

/// 0x4f4300, the Vikings' spell 5.
pub fn bloodlust(world: &mut World, ctx: &SpellCtx) -> i32 {
    flag_settlers(world, ctx, allies(world, ctx.player), 0x34, FLAG_BLOODLUST, 0x22)
}

/// 0x4f4320, the Vikings' spell 6.
pub fn confuse(world: &mut World, ctx: &SpellCtx) -> i32 {
    flag_settlers(world, ctx, enemies(world, ctx.player), 0x3c, FLAG_CONFUSED, 0x38)
}

/// 0x4f5180, the Trojans' spell 5: the caster's own soldiers only.
pub fn hide(world: &mut World, ctx: &SpellCtx) -> i32 {
    flag_settlers(world, ctx, crate::alliances::player_bit(ctx.player) as u32, 0x3c, FLAG_HIDDEN, 0x38)
}

/// 0x4f3d20, the Romans' spell 5: lightning. Every enemy settler of warrior type 2..7 in the
/// radius takes `min + Rand() % (max − min + 1)` damage (`LightningBoltDamageMin` at least 1,
/// `…Max`; settler vtbl+0x20) after the local player's "under attack" notice; effect 0x36. Up to
/// the limit; returns how many.
pub fn lightning(world: &mut World, ctx: &SpellCtx) -> i32 {
    let cfg = &world.rules.magic;
    let lo = cfg.lightning_bolt_damage_min.max(1);
    let span = if cfg.lightning_bolt_damage_max < lo { 1 } else { (cfg.lightning_bolt_damage_max - lo + 1) as u32 };
    let players = enemies(world, ctx.player);
    let (mut k, mut count) = (0, 0);
    while let Some((s, x, y)) = next_settler(world, ctx.x, ctx.y, ctx.radius, &mut k, players, 0xfc) {
        let d = lo + (world.rng.rand() % span) as i32;
        if let Some(ev) = SimEvent::attacked(world, s) {
            world.events.push(ev);
        }
        crate::combat::take_damage(world, s, d, ctx.player);
        effect(world, 0x36, 0, x, y);
        count += 1;
        if ctx.limit <= count {
            break;
        }
    }
    count
}

/// The settler's role id (`GetSettlerRole`, vtbl+0x48).
fn role_of(world: &World, s: u16) -> u8 {
    match world.entities.by_index(s as u32).map(|e| &e.data) {
        Some(EntityData::Settler(d)) => d.job.role,
        _ => 0,
    }
}

/// Settler `s` dies and in its place (the node `(x, y)`) a settler of `kind` for `player`
/// appears (`AddSettler(x, y, player, kind, 2)`): the visible flag goes first, then
/// `Kill(s, 0)` (0x56d030).
fn replace(world: &mut World, s: u16, x: i32, y: i32, player: u8, kind: u16) {
    if let Some(e) = world.entities.by_index_mut(s as u32) {
        e.flags &= !crate::entity::flag::VISIBLE;
    }
    world.kill(s, 0);
    crate::create::add_settler_ex(world, x, y, player, kind, 2);
}

/// 0x4f3e60, the Romans' spell 6: enemy soldiers (types 2..5, soldier or squad-leader role) join
/// the caster: swordsmen, bowmen and squad leaders as they are, the other specialists as medics
/// of their level (0x23..0x25); effect 0x37. Up to the limit; returns how many.
pub fn convert_soldiers(world: &mut World, ctx: &SpellCtx) -> i32 {
    let players = enemies(world, ctx.player);
    let (mut k, mut count) = (0, 0);
    while let Some((s, x, y)) = next_settler(world, ctx.x, ctx.y, ctx.radius, &mut k, players, 0x3c) {
        if !matches!(role_of(world, s), 7 | 0x17) {
            continue;
        }
        let kind = world.entities.by_index(s as u32).map_or(0, |e| e.kind);
        let new = match kind {
            0x1d..=0x22 | 0x2c => kind,
            0x23 | 0x26 | 0x29 | 0x3d => 0x23,
            0x24 | 0x27 | 0x2a | 0x3e => 0x24,
            0x25 | 0x28 | 0x2b | 0x3f => 0x25,
            _ => continue,
        };
        replace(world, s, x, y, ctx.player, new);
        effect(world, 0x37, 0, x, y);
        count += 1;
        if ctx.limit <= count {
            break;
        }
    }
    count
}

/// 0x4f4340, the Vikings' spell 7: the first READY enemy building (not the Dark Tribe's) on the
/// spiral takes 300 damage 25 ticks later (future event 2, attacker 0); effect 0x17. Returns 1,
/// or 0 without one.
pub fn strike(world: &mut World, ctx: &SpellCtx) -> i32 {
    let players = enemies(world, ctx.player);
    for (x, y) in nodes(world, ctx) {
        let m = world.layers.map_object[world.grid.index(x as u32, y as u32)];
        let Some(e) = world.entities.by_index(m as u32).filter(|e| m != 0 && e.obj_type == ObjType::Building) else { continue };
        if e.flags & crate::entity::flag::READY != 0 && e.race != 3 && players & crate::alliances::player_bit(e.owner) as u32 != 0 {
            effect(world, 0x17, 0, x, y);
            crate::future::add(world, 2, 25, m, 300, 0);
            return 1;
        }
    }
    0
}

/// 0x4f4550, the Maya's spell 4 (any range): enemy soldiers (types 2..5) around the *caster*
/// are moved to the target: each to the first node of the radius-4 spiral around its place
/// relative to the target that its walking object may enter and that is not its own position —
/// a new settler of its owner and type there (`AddSettler(…, 2)`), the old one killed; effects
/// 0x37 (one draw). Up to the limit. Then sounds 0x3f at both ends. Without a caster (a script's
/// cast) nothing happens. Returns how many.
pub fn teleport(world: &mut World, ctx: &SpellCtx) -> i32 {
    use crate::jobs::walking::Pathing;
    let Some(c) = world.entities.by_index(ctx.caster as u32).filter(|_| ctx.caster != 0) else { return 0 };
    let (cx, cy) = (c.x(), c.y());
    let players = enemies(world, ctx.player);
    let (mut k, mut count) = (0, 0);
    'outer: while let Some((s, _, _)) = next_settler(world, cx, cy, ctx.radius, &mut k, players, 0x3c) {
        let Some(e) = world.entities.by_index(s as u32) else { continue };
        let (owner, kind, pos) = (e.owner, e.kind, e.pos);
        let (tx, ty) = (pos.x() - cx + ctx.x, pos.y() - cy + ctx.y);
        for (px, py) in crate::spiral::walk(world.grid, tx, ty, 4) {
            let n = world.grid.index(px as u32, py as u32);
            let ok = crate::jobs::walking::ActivePathing::is_not_blocked(world, s, n)
                && crate::jobs::walking::ActivePathing::is_not_occupied(world, s, n)
                && pos != crate::grid::Y16X16::pack(px, py);
            if !ok {
                continue;
            }
            let _ = world.rng.rand() & 3;
            effect(world, 0x37, 0, pos.x(), pos.y());
            effect(world, 0x37, 0, px, py);
            crate::create::add_settler_ex(world, px, py, owner, kind, 2);
            if let Some(e) = world.entities.by_index_mut(s as u32) {
                e.flags &= !crate::entity::flag::VISIBLE;
            }
            world.kill(s, 0);
            count += 1;
            if ctx.limit <= count {
                break 'outer;
            }
            break;
        }
    }
    if count > 0 {
        effect(world, 0, 0x3f, cx, cy);
        effect(world, 0, 0x3f, ctx.x, ctx.y);
    }
    count
}

/// 0x4f4780, the Maya's spell 5: enemy bowmen (type 3) in the radius vanish (effect 0x38, one
/// draw; killed); each becomes a butterfly ([`SimEvent::Butterfly`], made by the client's
/// ambient manager with its own RNG). Up to the limit.
pub fn bowmen(world: &mut World, ctx: &SpellCtx) -> i32 {
    let players = enemies(world, ctx.player);
    let (mut k, mut count) = (0, 0);
    while let Some((s, x, y)) = next_settler(world, ctx.x, ctx.y, ctx.radius, &mut k, players, 8) {
        let _ = world.rng.rand() & 3;
        effect(world, 0x38, 0, x, y);
        if let Some(e) = world.entities.by_index_mut(s as u32) {
            e.flags &= !crate::entity::flag::VISIBLE;
        }
        world.kill(s, 0);
        world.events.push(SimEvent::Butterfly { x, y });
        count += 1;
        if ctx.limit <= count {
            break;
        }
    }
    count
}

/// 0x4f48f0, the Maya's spell 6: allied swordsmen, bowmen and specialists (types 2..4) of level
/// 1 or 2 rise a level (a new settler of the next type, the old one killed; the catapultists
/// 0x3d..0x3f stay what they are); effect 0x29. Up to the limit; then sound 0x51 at the target.
pub fn upgrade(world: &mut World, ctx: &SpellCtx) -> i32 {
    let players = allies(world, ctx.player);
    let (mut k, mut count) = (0, 0);
    while let Some((s, x, y)) = next_settler(world, ctx.x, ctx.y, ctx.radius, &mut k, players, 0x1c) {
        let Some(e) = world.entities.by_index(s as u32) else { continue };
        let (kind, owner) = (e.kind, e.owner);
        let new = match kind {
            0x1d | 0x1e | 0x20 | 0x21 | 0x23 | 0x24 | 0x26 | 0x27 | 0x29 | 0x2a => kind + 1,
            0x3d..=0x3f => kind,
            _ => continue,
        };
        replace(world, s, x, y, owner, new);
        effect(world, 0x29, 0, x, y);
        count += 1;
        if ctx.limit <= count {
            break;
        }
    }
    if count > 0 {
        effect(world, 0, 0x51, ctx.x, ctx.y);
    }
    count
}

/// 0x4f5000, the Trojans' spell 4: the caster's military buildings in the radius (0x5b0310 /
/// 0x5b0420) — effect 0x68 with sound 0x62 — get a door settler that cannot be hurt (entity flag
/// 0x10000 cleared, back after the spell's duration by future event 1). Up to the limit.
pub fn doors(world: &mut World, ctx: &SpellCtx) -> i32 {
    let mut count = 0;
    for b in buildings_within(world, ctx.x, ctx.y, ctx.radius) {
        let Some(e) = world.entities.by_index(b as u32) else { continue };
        if e.owner != ctx.player {
            continue;
        }
        let Some(door) = crate::buildings::military::state(world, b).map(|s| s.door) else { continue };
        let (x, y) = (e.x(), e.y());
        effect(world, 0x68, 0x62, x, y);
        if door != 0 {
            if let Some(d) = world.entities.by_index_mut(door as u32) {
                d.flags &= !0x1_0000;
            }
            crate::future::add(world, 1, ctx.duration.max(0) as u32, door, 0x1_0000, 0);
        }
        count += 1;
        if ctx.limit <= count {
            break;
        }
    }
    count
}

/// 0x4f51a0, the Trojans' spell 6: enemy soldiers (types 2..5) in the radius are disarmed: the
/// weapon lies on their node (an anywhere pile of 1: sword 0x22 for swordsmen, bow 8 for bowmen;
/// type 4 by race: Roman 0x22, Viking 5, Maya 6, Trojan 0x26) and a carrier of their owner stands
/// there instead; effect 0x37. A Dark Tribe swordsman or bowman just dies (not counted). Up to the
/// limit.
pub fn disarm(world: &mut World, ctx: &SpellCtx) -> i32 {
    let players = enemies(world, ctx.player);
    let (mut k, mut count) = (0, 0);
    while let Some((s, x, y)) = next_settler(world, ctx.x, ctx.y, ctx.radius, &mut k, players, 0x3c) {
        let Some(e) = world.entities.by_index(s as u32) else { continue };
        let (race, wt, owner) = (e.race, e.flags & 0xf, e.owner);
        if race == 3 {
            if wt == 2 || wt == 3 {
                world.kill(s, 0);
            }
            continue;
        }
        effect(world, 0x37, 0, x, y);
        if let Some(e) = world.entities.by_index_mut(s as u32) {
            e.flags &= !crate::entity::flag::VISIBLE;
        }
        world.kill(s, 0);
        let good = match (wt, race) {
            (2, _) | (4, 0) => 0x22,
            (3, _) => 8,
            (4, 1) => 5,
            (4, 2) => 6,
            (4, 4) => 0x26,
            _ => 0,
        };
        if good != 0 {
            crate::pile::add_pile(world, x, y, good, 1, crate::pile::role::ANYWHERE);
        }
        crate::create::add_settler(world, x, y, owner, 1);
        count += 1;
        if ctx.limit <= count {
            break;
        }
    }
    count
}

/// 0x4f5380, the Trojans' spell 7: the fog lifts at the target for the caster until
/// `tick + SpellDurationsTrojan[7]` (the raw value, not ×14): `CFogging::LiftFogAtSpot` — the
/// client's fog ([`World::fog`]), so it changes no simulation state. Returns 1.
pub fn vision(world: &mut World, ctx: &SpellCtx) -> i32 {
    let until = world.tick.wrapping_add(world.rules.magic.durations[4][7] as u32);
    if let Some(f) = world.fog.as_mut() {
        f.lift_fog_at_spot(ctx.x, ctx.y, until, ctx.player);
    }
    1
}

/// 0x4f4ad0, the Maya's spell 7: stones. Random spiral entries (`Rand() % (Last(radius) + 1)`)
/// are tried while an attempt counter stays below that count (it grows by one per try and by
/// `Rand() & 7` per stone): a node of a catapult tile (with a centre), on ground 0x1X, 0x4X or
/// 0x9X, where `IsAddDecoObjOk(stone, 1 extra ring)` holds, gets a stone heap of `8 + Rand() & 3`
/// (deco 41) with its flags; effect 0x2f. Up to the limit; returns how many.
pub fn stones(world: &mut World, ctx: &SpellCtx) -> i32 {
    let entries = crate::spiral::last(ctx.radius.clamp(0, 255) as u8) as u32 + 1;
    let (mut tries, mut count) = (0u32, 0);
    loop {
        let r = (world.rng.rand() % entries) as usize;
        let (x, y) = (ctx.x + crate::spiral::dx(r), ctx.y + crate::spiral::dy(r));
        if let Some(n) = world.grid.checked_index(x, y) {
            let cat = world.tiling.catapult.get(n).copied().unwrap_or(0);
            let tile = if cat < 0x8000 { cat } else { 0 };
            let centred = world.tiling.tiles.get(tile as usize).is_some_and(|t| t.center.0 & 0xffff != 0);
            if centred && matches!(world.layers.ground[n] & 0xf0, 0x10 | 0x40 | 0x90) && crate::deco::is_add_deco_obj_ok(world, x, y, 41, 1) {
                let amount = (world.rng.rand() & 3) as u8 + 8;
                let made = crate::deco::add_deco_obj_without_flags(world, x, y, 41, amount).is_ok();
                crate::deco::set_flags_for_object(world, x, y, 41);
                if made {
                    let _ = world.rng.rand() & 3;
                    effect(world, 0x2f, 0, x, y);
                    count += 1;
                    tries += world.rng.rand() & 7;
                    if ctx.limit <= count {
                        return count;
                    }
                }
            }
        }
        tries += 1;
        if tries >= entries {
            return count;
        }
    }
}

// ---- the Dark Tribe (stage C) ----------------------------------------------------------------

/// 0x4f4c70, the Dark Tribe's spell 2 (the shaman's rot): effect 0x65 (sound 0x3b) at the target,
/// then every node of the spiral `0..=Last(radius)` on the map rots ([`rot_at`]). Returns how many
/// nodes (no limit).
pub fn rot(world: &mut World, ctx: &SpellCtx) -> i32 {
    effect(world, 0x65, 0x3b, ctx.x, ctx.y);
    let mut count = 0;
    for (x, y) in nodes(world, ctx) {
        rot_at(world, x, y);
        count += 1;
    }
    count
}

/// `CDecoObjMgr` 0x564430 `(node, rot = 1)`: a plant on the node rots ([`plant_step`]).
pub fn rot_at(world: &mut World, x: i32, y: i32) {
    plant_step(world, x, y, true);
}

/// `CDecoObjMgr` 0x564430 `(node, rot)`: the plant on the node ages.
/// * Its own (dynamic) instance, with `rot` only, jumps to the rotten stage (`CPlant::Take(final
///   − stage)`) unless it stands grown (stage 3, a farmer's target): vine / agave stage 4, grain
///   (209, 210) and sunflower stage 5. The hive (217) has the rule too, but its `Take` (vtbl+0x24
///   0x533cd0) does nothing.
/// * A shared static instance (with or without `rot`: the world step 0x4f2d00 passes 0) is
///   replaced by its own copy (0x5661e0, stage 3) that takes `final − 3`; one already showing
///   the final stage is removed from the node (0x564cc0).
pub fn plant_step(world: &mut World, x: i32, y: i32, rot: bool) {
    let Some(n) = world.grid.checked_index(x, y) else { return };
    let id = world.layers.deco_object[n];
    let Some(e) = world.entities.by_index(id as u32) else { return };
    let EntityData::Deco(d) = &e.data else { return };
    let (kind, stage, shared) = (e.kind, d.stage, d.static_instance);
    let last = match kind {
        162 | 211 => 4u8,
        209 | 210 | 255 => 5,
        _ => return,
    };
    if !shared {
        if rot && stage != 3 {
            crate::deco::plant::take_n(world, id, last.wrapping_sub(stage) as i8);
        }
        return;
    }
    if stage == last {
        crate::deco::remove_at(world, x, y, false);
        return;
    }
    if let Some(copy) = crate::deco::plant::make_dynamic(world, id, x, y) {
        crate::deco::plant::take_n(world, copy, (last - 3) as i8);
    }
}

/// 0x4f4d10, the Dark Tribe's spell 7 (the shaman's teleport): the pile on the target node
/// (effect 0x66, sound 0x3f, whenever there is one) gives all its goods not already reserved
/// (amount − leaving, `CPile` +0x41 / +0x42) to the caster (`AttachAndIncAmountLeaving` with
/// target 2, then taken at once, 0x56fc10) unless it has entity flag 0x10; the player's
/// manakopter hall counts boards and iron ([`crate::buildings::manakopter::add_goods`]). Returns
/// 1 when goods moved. The caster must be a settler (the release reads it unchecked).
pub fn teleport_goods(world: &mut World, ctx: &SpellCtx) -> i32 {
    let Some(n) = world.grid.checked_index(ctx.x, ctx.y) else { return 0 };
    let pile = world.layers.deco_object[n];
    let Some(p) = world.entities.by_index(pile as u32).filter(|p| p.obj_type == ObjType::Pile) else { return 0 };
    let pflags = p.flags;
    let EntityData::Pile(d) = &p.data else { return 0 };
    let (good, amount, leaving) = (d.good, d.amount, d.leaving);
    let Some(c) = world.entities.by_index(ctx.caster as u32).filter(|c| c.obj_type == ObjType::Settler) else { return 0 };
    let player = c.owner;
    effect(world, 0x66, 0x3f, ctx.x, ctx.y);
    if pflags & 0x10 != 0 || amount <= leaving {
        return 0;
    }
    let k = amount as i32 - leaving as i32;
    crate::pile::attach_and_inc_amount_leaving(world, pile, ctx.caster, k, crate::pile::target::PILE_SRC);
    crate::pile::change_amount_and_detach(world, pile, ctx.caster);
    if let Some(hall) = crate::buildings::manakopter::hall_of(world, player) {
        crate::buildings::manakopter::add_goods(world, hall, good, k);
    }
    1
}
