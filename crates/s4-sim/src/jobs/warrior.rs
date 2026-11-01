//! `CWarriorBehavior` (the soldier role's part at +0x30, vtable 0x105c394): the fighting half of
//! `CSoldierRole`. Own reverse engineering of the shipped exe (release VAs).
//!
//! Each soldier uses one row of the table at .data 0x11cdc18 (25 dwords; row = its warrior type,
//! or 15..20 for medics, axe warriors, blowgun warriors, the shaman 0x36, the saboteur 0x2e and
//! the backpack catapultists 0x3d..0x3f, see `SetMode` 0x598290): [0] "enemy strength in my war-map cell" (0x5983f0), [1]
//! "this target is still a valid enemy in range" (0x5984a0), [2] the attack search, [3] the
//! approach search, [4] the attack kind (1: heals allies), [5] the target mask for 0x598180,
//! [6] a reach, [7] / [8] the attack range, [9] the approach radius, [10..24] a priority per
//! warrior type of the target. Swordsmen (row 2), the squad leader (5) and row 13 attack the six
//! neighbours (0x598eb0); bowmen (row 3) shoot from 4 to 13 (0x599010 → 0x599150) and approach
//! within 15 (0x599400 → 0x599550).
//!
//! `Update` (0x598610) states: 0/1 idle — every 30 ticks a search: an enemy in attack range is
//! attacked, else one within the approach radius is walked to (`bflags` bit 1), else (ranged
//! rows) an enemy too close makes the soldier step back (0x599800); otherwise one walking step.
//! 4 walks to a position, 6 / 7 are the attack-move modes (0x500000 / 0x600000), 5 and ≥ 8 end
//! the behaviour. `Attack` (vtbl+0xc 0x58a750) makes the first task of the soldier's anim list
//! 1 (2 when a healing row meets an ally) the role's override task (+0x50) with the target as its
//! entity and takes it: the WORK task of the fight animation. When it ends, `LogicUpdateJob`
//! (0x589250) deals the damage ([`hit`]) and takes the list's task again.
//!
//! Medics (row 15) always search; with healings left they heal a hurt ally on the 18 nodes of
//! rings 1–2 (0x5992c0, anim list 2, task 0x22 → [`heal`]), approach hurt allies as well as
//! enemies (0x599650) and otherwise fight like swordsmen. Axe warriors (row 16) attack enemy
//! military buildings from a builder spot within 2 (0x5993a0 / 0x597fd0) before enemies on rings
//! 1..20, and approach a builder spot within 8 (0x5997a0) before enemies; their target is never
//! "still valid" (0x5985f0), so every update searches. Backpack catapultists' hits splash
//! (0x589250, table .data 0x1165374: 55/65/75 % of the damage within 2 of the target, half that
//! up to 4), blowgun hits may freeze a swordsman / bowman (0x3c, 0xc against the Dark Tribe:
//! warrior types 2..5 / 2..3) with chance 0x11cdad0[kind] / 65536 (36044 / 42598 / 49152; both
//! tables are filled from the Maya settler info by the loader 0x583910, see `Rules`) for
//! 20..35 ticks (entity flag 0x800000, cleared by future event 1).
//!
//! Saboteurs (row 19, `CSaboteurRole`, [`super::soldier::saboteur_logic_update`]) always search,
//! never keep a target (0x598600), attack any enemy building (not of the Dark Tribe) whose builder
//! spot they stand on (0x5993d0) and walk to the best free builder spot within 12 of one within 21
//! (0x5997e0); their hit is the role's own (a chance, no fighting strength).
//!
//! Not ported (TODO): row 18 (shaman, `CShamanRole`: its `LogicUpdateJob` 0x587820 casts spells
//! through 0x4f5570 with a mana pool, i.e. the magic system); the Dark Tribe manakopters bowmen also shoot at (0x599010 / 0x599400
//! with the list 0x153fca0, 0x569e60); spells (entity flag 0x100000). The shot / throw effects
//! `Attack` starts (`CEffects` vtbl+0x18) are [`crate::projectile`]'s: only drawn, the damage
//! comes from the task's end.
//!
//! Squad leaders (`CSquadLeaderRole`, [`super::soldier::lead`]) fight with row 5; soldiers grouped
//! with one hit harder ([`hit`]).

use super::core::{job, job_mut};
use super::state::SoldierData;
use super::walking::{ActivePathing, Pathing};
use crate::World;
use crate::entity::{EntityData, ObjType, flag};
use crate::event::SimEvent;
use crate::grid::Y16X16;
use crate::spiral::grid_distance;

/// .data 0x11cdc18, 21 rows of 25 dwords (read from the shipped exe).
#[rustfmt::skip]
pub(crate) const ROWS: [[u32; 25]; 21] = [
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x598eb0, 0x599550, 4, 0x2019c03, 0, 1, 1, 13, 0, 0, 6, 6, 5, 4, 5, 2, 3, 3, 0, 0, 0, 6, 1],
    [0x5983f0, 0x5984a0, 0x599010, 0x599400, 0, 0x202b003, 0, 4, 13, 15, 0, 0, 6, 6, 5, 4, 5, 2, 3, 3, 0, 0, 6, 0, 1],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x598eb0, 0x599550, 4, 0x201bc03, 0, 1, 1, 13, 0, 0, 6, 6, 5, 4, 5, 2, 3, 3, 0, 0, 0, 0, 1],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0x202e0ff, 0, 1, 20, 20, 0, 0, 0, 0, 0, 0, 0, 0, 8, 8, 6, 2, 3, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0x202e0ff, 0, 1, 20, 30, 0, 0, 0, 0, 0, 0, 0, 0, 6, 6, 8, 2, 3, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x598eb0, 0x599550, 4, 0x201ff03, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x598480, 0x598570, 0x5992c0, 0x599650, 1, 0x201bf03, 0, 1, 1, 13, 0, 0, 6, 6, 5, 4, 5, 2, 0, 0, 0, 0, 0, 6, 1],
    [0x5983f0, 0x5985f0, 0x5993a0, 0x5997a0, 4, 0x201bc03, 0, 1, 2, 13, 0, 0, 6, 6, 5, 4, 5, 2, 3, 3, 0, 0, 0, 6, 1],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 3, 0x202bf03, 0, 3, 8, 13, 0, 0, 6, 6, 5, 4, 5, 2, 0, 0, 0, 0, 0, 0, 1],
    [0x598440, 0x5984a0, 0x599150, 0x599550, 4, 0x200fffe, 0, 1, 3, 10, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x598490, 0x598600, 0x5993d0, 0x5997e0, 4, 0x200fffe, 0, 1, 3, 10, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0x5983f0, 0x5984a0, 0x599150, 0x599550, 3, 0x202bf03, 0, 4, 10, 15, 0, 0, 6, 6, 5, 4, 5, 2, 0, 0, 0, 0, 0, 0, 1],
];

/// The six direction vectors (.rdata 0x101bd88, `{dx, dy}` dwords).
const DIRS: [(i32, i32); 6] = [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)];

fn row(world: &World, i: u16) -> &'static [u32; 25] {
    let t = job(world, i).map_or(0, |j| j.soldier.table) as usize;
    &ROWS[t.min(ROWS.len() - 1)]
}

fn sd(world: &mut World, i: u16) -> Option<&mut SoldierData> {
    job_mut(world, i).map(|j| &mut j.soldier)
}

fn unported(world: &mut World, what: &'static str, entity: u16) {
    world.events.push(SimEvent::Unported { what, entity });
}

/// The hex points in rings around a node (.rdata 0x101bdb8; the same order as the node-offset
/// table 0x1454148 the melee search reads). Their fourth byte, which 0x599800 adds to the step
/// direction, is 0 for every entry the searches reach.
pub(crate) fn hex() -> &'static [(i32, i32)] {
    use std::sync::OnceLock;
    static H: OnceLock<Vec<(i32, i32)>> = OnceLock::new();
    H.get_or_init(|| crate::tiling::hex_points(30))
}

/// 0x598180 `(entity, mask)`: a flying type-1 entity counts when READY; otherwise a settler or an
/// entity with a warrior type, without flag 0x8000000, whose warrior-type bit is not in the mask
/// and which has every flag of 0x2030100 (READY, VULNERABLE, VISIBLE) the mask asks for.
pub fn valid_target(world: &World, id: u16, mask: u32) -> bool {
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    if e.kind == 1 && e.obj_type == ObjType::Flying {
        return e.flags & flag::READY != 0;
    }
    let wt = e.flags & 0xF;
    if (wt != 0 || e.obj_type == ObjType::Settler) && e.flags & 0x800_0000 == 0 {
        return mask & ((1 << wt) | (!e.flags & 0x203_0100)) == 0;
    }
    false
}

/// The enemy-player mask (0x12dcd54) check and 0x598180.
fn enemy_valid(world: &World, me: u8, id: u16, mask: u32) -> bool {
    world.entities.by_index(id as u32).is_some_and(|e| crate::warmap::hostile(world, me, e.owner)) && valid_target(world, id, mask)
}

fn priority(world: &World, r: &[u32; 25], id: u16) -> i32 {
    let wt = world.entities.by_index(id as u32).map_or(0, |e| e.flags & 0xF) as usize;
    r[10 + wt] as i32
}

fn me(world: &World, i: u16) -> Option<(Y16X16, u8)> {
    world.entities.by_index(i as u32).map(|e| (e.pos, e.owner))
}

fn object_at(world: &World, p: Y16X16, (dx, dy): (i32, i32)) -> u16 {
    world.grid.checked_index(p.x() + dx, p.y() + dy).map_or(0, |n| world.layers.map_object[n])
}

/// Row [0].
fn f_cell_enemy(world: &mut World, i: u16) -> bool {
    match row(world, i)[0] {
        0x5983f0 => me(world, i).is_some_and(|(p, o)| crate::warmap::foreign_strength(world, p.x(), p.y(), o) > 0),
        // Medics (0x598480) and saboteurs (0x598490) always search.
        0x598480 | 0x598490 => true,
        // 0x598440 (the shaman): an enemy owns land in the shaman's square or one next to it
        // (byte 1 of the owner map, 0x12ef999, against the enemy mask 0x12dcd54).
        0x598440 => me(world, i).is_some_and(|(p, o)| {
            let bits = world.tiling.owner_bits(p.x() >> 4, p.y() >> 4)[1] as u16;
            world.alliances.enemy_bits.get(o as usize).is_some_and(|&m| m & bits != 0)
        }),
        _ => {
            unported(world, "CWarriorBehavior search row [0]", i);
            false
        }
    }
}

/// 0x5984a0: the target is a valid enemy and `min ≤ distance ≤ max` (a land vehicle one more).
fn target_ok(world: &World, i: u16, target: u16) -> bool {
    let r = row(world, i);
    let Some((p, o)) = me(world, i) else { return false };
    if target == 0 || !enemy_valid(world, o, target, r[5]) {
        return false;
    }
    let Some(t) = world.entities.by_index(target as u32) else { return false };
    let d = grid_distance(t.pos.x() - p.x(), t.pos.y() - p.y());
    let max = r[8] as i32 + (t.obj_type == ObjType::LandVehicle) as i32;
    d <= max && r[7] as i32 <= d
}

/// Row [1].
fn f_target_ok(world: &mut World, i: u16, target: u16) -> bool {
    match row(world, i)[1] {
        0x5984a0 => target_ok(world, i, target),
        // 0x598570 (medics): with healings left, a hurt ally or an enemy in range.
        0x598570 => healings(world, i) > 0 && (me(world, i).is_some_and(|(_, o)| hurt_ally(world, o, target)) || target_ok(world, i, target)),
        // 0x5985f0 (axe warriors), 0x598600 (saboteurs): never.
        _ => false,
    }
}

/// Row [2]: the target (> 0) or ≤ 0.
fn f_attack(world: &mut World, i: u16) -> i32 {
    let r = row(world, i);
    match r[2] {
        0x598eb0 => melee(world, i, r),
        0x599150 => rings(world, i, r),
        0x599010 => rings_or_manakopter(world, i, r),
        0x5992c0 => medic_attack(world, i, r),
        // 0x5993a0: an enemy military building with a builder spot within 2, else the rings.
        0x5993a0 => match building_search(world, i, 15, 2, true, None) {
            0 => rings(world, i, r),
            b => b as i32,
        },
        // 0x5993d0 (saboteurs): any enemy building within 15 with a builder spot under the
        // saboteur (reach 0), else −1.
        0x5993d0 => match building_search(world, i, 15, 0, false, None) {
            0 => -1,
            b => b as i32,
        },
        _ => -1,
    }
}

/// Row [3]: the target, its node in `pos`.
fn f_approach(world: &mut World, i: u16, pos: &mut Y16X16) -> u16 {
    let r = row(world, i);
    match r[3] {
        0x599550 => approach_search(world, i, r, pos),
        0x599400 => approach_or_manakopter(world, i, r, pos),
        0x599650 => medic_approach(world, i, r, pos),
        // 0x5997a0: a builder spot within 8 of an enemy military building, else enemies.
        0x5997a0 => match building_search(world, i, 21, 8, true, Some(pos)) {
            0 => approach_search(world, i, r, pos),
            b => b,
        },
        // 0x5997e0 (saboteurs): the best builder spot within 12 of any enemy building within
        // 21, occupied spots scoring worse (reach −12).
        0x5997e0 => building_search(world, i, 21, -12, false, Some(pos)),
        _ => 0,
    }
}

/// 0x598eb0: of the six neighbours the valid enemy of the highest priority. Each placeholder
/// (`map_object` 1..6) found while nothing better is known lowers the result by one from −1;
/// exactly −2 (one placeholder, no enemy) searches ring 2 for the first valid enemy.
fn melee(world: &World, i: u16, r: &[u32; 25]) -> i32 {
    let Some((p, o)) = me(world, i) else { return -1 };
    let (mut best, mut best_p) = (-1i32, 0);
    for &d in &hex()[1..7] {
        let m = object_at(world, p, d);
        if m == 0 {
            continue;
        }
        if m < 7 {
            best += best >> 31;
        } else if enemy_valid(world, o, m, r[5]) {
            let pr = priority(world, r, m);
            if best_p < pr {
                best = m as i32;
                best_p = pr;
            }
        }
    }
    if best != -2 {
        return best;
    }
    for &d in &hex()[7..19] {
        let m = object_at(world, p, d);
        if m != 0 && enemy_valid(world, o, m, r[5]) {
            return m as i32;
        }
    }
    -1
}

/// 0x599150: hex entries `(min−1)·3·min + 1 .. (max+1)·max·3 + 1` (rings `min..=max`), the
/// valid enemy of the highest priority; above 7 ends at once, above 3 cuts the search to the
/// 200 entries after the start.
fn rings(world: &World, i: u16, r: &[u32; 25]) -> i32 {
    let (min, max) = (r[7] as i32, r[8] as i32);
    if min < 1 {
        return -1;
    }
    let Some((p, o)) = me(world, i) else { return -1 };
    let first = (min - 1) * 3 * min + 1;
    let cap = first + 200;
    let mut end = (max + 1) * max * 3 + 1;
    let (mut best, mut best_p) = (-1i32, 0);
    let mut k = first;
    while k < end {
        let m = object_at(world, p, hex()[k as usize]);
        if m != 0 && enemy_valid(world, o, m, r[5]) {
            let pr = priority(world, r, m);
            if best_p < pr {
                if pr > 7 {
                    return m as i32;
                }
                best_p = pr;
                best = m as i32;
                if pr > 3 && cap < end {
                    end = cap;
                }
            }
        }
        k += 1;
    }
    best
}

/// The nearest READY manakopter (0x569e60) of each Dark Tribe player (race 3) whose team does
/// not hold `me` (0x12dcd30[p]), nearer than `limit`: the nearest of them and its distance.
fn enemy_manakopter(world: &World, me: u8, at: Y16X16, mut limit: i32) -> Option<(u16, i32)> {
    let mut best = None;
    for p in 1..=world.player_count() {
        if world.race_of(p) != 3 || crate::warmap::allied(world, p, me) {
            continue;
        }
        let m = crate::flying::nearest(world, p, at.x(), at.y(), 0);
        let Some(e) = world.entities.by_index(m as u32).filter(|e| m != 0 && e.flags & flag::READY != 0) else { continue };
        let d = grid_distance(e.x() - at.x(), e.y() - at.y());
        if d < limit {
            best = Some((m, d));
            limit = d;
        }
    }
    best
}

/// 0x599010: the rings (0x599150), then the enemy manakopters nearer than the row's maximum
/// ([`enemy_manakopter`]): a manakopter wins unless the rings found a target whose priority
/// (row [10 + warrior type]) is above 3.
fn rings_or_manakopter(world: &World, i: u16, r: &[u32; 25]) -> i32 {
    let found = rings(world, i, r);
    let Some((p, o)) = me(world, i) else { return found };
    match enemy_manakopter(world, o, p, r[8] as i32) {
        Some(_) if found != -1 && priority(world, r, found as u16) > 3 => found,
        Some((m, _)) => m as i32,
        None => found,
    }
}

/// 0x599400: the approach search (0x599550), then the nearest enemy manakopter nearer than the
/// row's radius, which replaces it (its node goes to `pos`).
fn approach_or_manakopter(world: &World, i: u16, r: &[u32; 25], pos: &mut Y16X16) -> u16 {
    let found = approach_search(world, i, r, pos);
    let Some((p, o)) = me(world, i) else { return found };
    match enemy_manakopter(world, o, p, r[9] as i32) {
        Some((m, _)) => {
            *pos = world.entities.by_index(m as u32).map_or(*pos, |e| e.pos);
            m
        }
        None => found,
    }
}

/// 0x599550: rings `min + 1..=radius`, the first valid enemy; its node goes to `pos`.
fn approach_search(world: &World, i: u16, r: &[u32; 25], pos: &mut Y16X16) -> u16 {
    let (min, radius) = (r[7] as i32, r[9] as i32);
    let Some((p, o)) = me(world, i) else { return 0 };
    for k in (min + 1) * min * 3 + 1..(radius + 1) * radius * 3 + 1 {
        let d = hex()[k as usize];
        let m = object_at(world, p, d);
        if m != 0 && enemy_valid(world, o, m, r[5]) {
            *pos = Y16X16::pack(p.x() + d.0, p.y() + d.1);
            return m;
        }
    }
    0
}

/// Role +0x49: the medic's healings left.
fn healings(world: &World, i: u16) -> u8 {
    job(world, i).map_or(0, |j| j.soldier.healings)
}

/// 0x597dc0 `(ally mask 0x12dcd30[me], id)`: a READY settler of an allied player below its
/// maximum health (vtbl+0x2c 0x581830: settler info +2).
fn hurt_ally(world: &World, me: u8, id: u16) -> bool {
    if id == 0 {
        return false;
    }
    let Some(e) = world.entities.by_index(id as u32) else { return false };
    let EntityData::Settler(s) = &e.data else { return false };
    e.obj_type == ObjType::Settler && e.flags & flag::READY != 0 && crate::warmap::allied(world, me, e.owner) && s.health < world.rules.settler_health(e.race, e.kind)
}

/// 0x5992c0 (medics): with healings left, the first settler of an allied player below its
/// maximum health on the 18 nodes of rings 1 and 2 (no READY check here); else the melee search.
fn medic_attack(world: &World, i: u16, r: &[u32; 25]) -> i32 {
    let Some((p, o)) = me(world, i) else { return -1 };
    if healings(world, i) > 0 {
        for &d in &hex()[1..19] {
            let m = object_at(world, p, d);
            let Some(e) = world.entities.by_index(m as u32).filter(|_| m != 0) else { continue };
            let EntityData::Settler(s) = &e.data else { continue };
            if e.obj_type == ObjType::Settler && crate::warmap::allied(world, o, e.owner) && s.health < world.rules.settler_health(e.race, e.kind) {
                return m as i32;
            }
        }
    }
    melee(world, i, r)
}

/// 0x599650 (medics): as 0x599550, also taking a hurt ally while healings are left.
fn medic_approach(world: &World, i: u16, r: &[u32; 25], pos: &mut Y16X16) -> u16 {
    let (min, radius) = (r[7] as i32, r[9] as i32);
    let Some((p, o)) = me(world, i) else { return 0 };
    let heals = healings(world, i) > 0;
    for k in (min + 1) * min * 3 + 1..(radius + 1) * radius * 3 + 1 {
        let d = hex()[k as usize];
        let m = object_at(world, p, d);
        if m != 0 && (enemy_valid(world, o, m, r[5]) || (heals && hurt_ally(world, o, m))) {
            *pos = Y16X16::pack(p.x() + d.0, p.y() + d.1);
            return m;
        }
    }
    0
}

/// 0x597fd0 `(settler, radius, reach, military only, &pos)`: the enemy buildings of the war-map
/// cells within `radius` (0x5b0310, category 2 lists, iterator 0x5b05a0: READY, hostile owner,
/// with the flag only military ones (flags nibble 0xc), at most `radius` away), not of the Dark
/// Tribe, are scored by 0x597e10; the last building that improved the best score is returned
/// (0 = none), the best spot goes to `pos` (0 without buildings).
fn building_search(world: &World, i: u16, radius: i32, reach: i32, military: bool, pos: Option<&mut Y16X16>) -> u16 {
    let Some((p, o)) = me(world, i) else { return 0 };
    let (mut best, mut spot, mut found) = (0x4000, 0u32, 0u16);
    for (cx, cy) in crate::warmap::cells_within(world, p.x(), p.y(), radius) {
        for b in world.warmap.cell(cx as usize, cy as usize, 2) {
            let Some(e) = world.entities.by_index(b as u32) else { continue };
            if !crate::warmap::hostile(world, o, e.owner) || e.flags & flag::READY == 0 || (military && e.flags & 0xF != 0xC) {
                continue;
            }
            if grid_distance(e.pos.x() - p.x(), e.pos.y() - p.y()) > radius || e.race == 3 {
                continue;
            }
            if builder_spot(world, p, b, reach, &mut best, &mut spot) {
                found = b;
            }
        }
    }
    if let Some(pos) = pos {
        *pos = Y16X16(spot);
    }
    found
}

/// 0x597e10 `(x, y, reach, building, &best, &pos)`: the building's builder spots (info +7
/// `builderNumber`, +8 three bytes each). A spot under the soldier wins at once (score 0);
/// otherwise spots within `|reach|` score their distance (for a negative reach plus 8 for an
/// occupied spot and 2 per occupied neighbour) and a score below `best` replaces it.
fn builder_spot(world: &World, me: Y16X16, b: u16, reach: i32, best: &mut i32, spot: &mut u32) -> bool {
    let Some(e) = world.entities.by_index(b as u32) else { return false };
    let Some(info) = world.rules.build.info(e.race, e.kind as u8) else { return false };
    let bp = e.pos;
    let mut found = false;
    for &(dx, dy, _) in &info.builders {
        let (px, py) = (bp.x() + dx as i32, bp.y() + dy as i32);
        let mut d = grid_distance(px - me.x(), py - me.y());
        if d == 0 {
            *best = 0;
            *spot = Y16X16::pack(px, py).0;
            return true;
        }
        if d > reach.abs() {
            continue;
        }
        if reach < 0 {
            let occupied = |x: i32, y: i32| world.grid.checked_index(x, y).is_some_and(|n| world.layers.map_object[n] != 0);
            if occupied(px, py) {
                d += 8;
            }
            for (nx, ny) in super::walking::NEIGHBOR {
                if occupied(px + nx, py + ny) {
                    d += 2;
                }
            }
        }
        if d < *best {
            *best = d;
            *spot = Y16X16::pack(px, py).0;
            found = true;
        }
    }
    found
}

/// 0x599800 (rows with a minimum range above 1): a valid enemy closer than the range makes the
/// soldier step back. One game-RNG draw picks direction `(Rand() % 3 + 2) % 6` (relative to the
/// enemy's hex entry, whose direction byte is 0); six nodes that way, the first node of the
/// radius-4 spiral (`CSpiralWalk`, `0..=Last(4)`) in the soldier's sector is the walk target.
fn step_back(world: &mut World, i: u16, pos: &mut Y16X16) -> u16 {
    let r = row(world, i);
    let min = r[7] as i32;
    let Some((p, o)) = me(world, i) else { return 0 };
    for k in 1..(min - 1) * 3 * min + 1 {
        let m = object_at(world, p, hex()[k as usize]);
        if m == 0 || !enemy_valid(world, o, m, r[5]) {
            continue;
        }
        let sector = world.grid.checked_index(p.x(), p.y()).map_or(0, |n| world.tiling.sector_of(n));
        let dir = ((world.rng.rand() % 3 + 2) % 6) as usize;
        let (cx, cy) = (p.x() + 6 * DIRS[dir].0, p.y() + 6 * DIRS[dir].1);
        let found = crate::spiral::walk(world.grid, cx, cy, 4).find(|&(x, y)| world.tiling.sector_of(world.grid.index(x as u32, y as u32)) == sector);
        let Some((x, y)) = found else { return 0 };
        *pos = Y16X16::pack(x, y);
        return m;
    }
    0
}

/// The walking object's target (vtbl+0x2c).
pub fn walk_target(world: &World, i: u16) -> i32 {
    job(world, i).and_then(|j| j.walking.as_ref()).map_or(-1, |w| w.walk_to)
}

fn walk_to(world: &mut World, i: u16, pos: Y16X16) {
    let mode = job(world, i).map_or(0, |j| j.soldier.mode);
    ActivePathing::walk_to_xy(world, i, pos, ((mode & 0x1060) | 0x2000) as i32);
}

/// 0x598df0: the distance to `pos`; the walk is pointed there unless (more than 4 away) the
/// current walk already ends within 2 of it.
fn approach(world: &mut World, i: u16, pos: Y16X16) -> i32 {
    let Some((p, _)) = me(world, i) else { return 0 };
    let d = grid_distance(pos.x() - p.x(), pos.y() - p.y());
    let wt = walk_target(world, i);
    if wt >= 0 && d > 4 {
        let w = Y16X16(wt as u32);
        if grid_distance(pos.x() - w.x(), pos.y() - w.y()) < 3 {
            return d;
        }
    }
    walk_to(world, i, pos);
    d
}

/// 0x598da0: walk to the behaviour's target position.
fn rewalk(world: &mut World, i: u16) {
    let Some(s) = job(world, i).map(|j| j.soldier) else { return };
    walk_to(world, i, Y16X16(s.target_pos as u32));
}

fn set_target(world: &mut World, i: u16, t: u16) {
    if let Some(s) = sd(world, i) {
        s.target = t;
    }
}

/// `CWarriorBehavior::Update(settler, tick, 0)` (0x598610).
pub fn update(world: &mut World, i: u16) {
    update_flags(world, i, 0);
}

/// `Update`'s flag 0x10 (the shaman, 0x587820): in the idle states no attack or approach search
/// and a kept target is not attacked.
pub const NO_SEARCH: u32 = 0x10;

/// 0x598610 `(settler, tick, flags)`, see [`update`].
pub fn update_flags(world: &mut World, i: u16, flags: u32) {
    let tick = world.tick;
    let Some(s) = job(world, i).map(|j| j.soldier) else { return };
    match s.state {
        4 => {
            if super::soldier::walk_step(world, i) & 0x70 != 0
                && let Some(s) = sd(world, i)
            {
                s.state = 8;
            }
        }
        0 | 1 => idle(world, i, tick, flags),
        6 => {
            if tick < s.next_search {
                if s.target == 0 {
                    super::soldier::walk_step(world, i);
                    return;
                }
                if target_ok(world, i, s.target) {
                    attack(world, i, s.target);
                    return;
                }
            }
            if let Some(s) = sd(world, i) {
                s.next_search = tick + 30;
            }
            if f_cell_enemy(world, i) {
                let t = f_attack(world, i);
                if t > 0 {
                    set_target(world, i, t as u16);
                    attack(world, i, t as u16);
                    return;
                }
            }
            set_target(world, i, 0);
            super::soldier::walk_step(world, i);
        }
        7 => {
            if f_cell_enemy(world, i) {
                let t = f_attack(world, i);
                if t > 0 {
                    set_target(world, i, t as u16);
                    attack(world, i, t as u16);
                    return;
                }
            }
            // Mode 0x600000 follows the entity at `target_pos` while it is READY: walking when
            // farther than both rows' reach [6] + 1, else standing (vtbl+8).
            let follow = (s.target_pos > 0).then(|| world.entities.by_index(s.target_pos as u32)).flatten();
            if let Some(t) = follow.filter(|t| t.flags & flag::READY != 0) {
                let (tpos, twt) = (t.pos, (t.flags & 0xF) as usize);
                let Some((p, _)) = me(world, i) else { return };
                let d = grid_distance(tpos.x() - p.x(), tpos.y() - p.y());
                if row(world, i)[6] as i32 + 1 + ROWS[twt][6] as i32 >= d {
                    super::soldier::stand_step(world, i);
                } else {
                    approach(world, i, tpos);
                    super::soldier::walk_step(world, i);
                }
                return;
            }
            if let Some(s) = sd(world, i) {
                s.target_pos = -1;
                s.state = 8;
            }
        }
        _ => {
            if let Some(s) = sd(world, i) {
                s.state = 0;
            }
            super::soldier::behaviour_done(world, i);
        }
    }
}

/// States 0/1 of 0x598610.
fn idle(world: &mut World, i: u16, tick: u32, flags: u32) {
    let Some(s) = job(world, i).map(|j| j.soldier) else { return };
    let mut search = s.next_search <= tick;
    if tick < s.next_search && s.target != 0 && flags & NO_SEARCH != 0 {
        if let Some(s) = sd(world, i) {
            s.bflags &= !2;
        }
    } else if tick < s.next_search && s.target != 0 {
        if f_target_ok(world, i, s.target) {
            if let Some(s) = sd(world, i) {
                s.bflags &= !2;
            }
            attack(world, i, s.target);
            return;
        }
        if s.bflags & 2 == 0 {
            search = true;
        }
    }
    if search {
        if let Some(s) = sd(world, i) {
            s.bflags &= !2;
            s.next_search = tick + 30;
        }
        if f_cell_enemy(world, i) && flags & NO_SEARCH == 0 {
            let t = f_attack(world, i);
            if t > 0 {
                set_target(world, i, t as u16);
                attack(world, i, t as u16);
                return;
            }
            let mut pos = Y16X16(0);
            let mut t = f_approach(world, i, &mut pos);
            if t == 0 && row(world, i)[7] > 1 {
                t = step_back(world, i, &mut pos);
            }
            if t > 0 {
                if let Some(s) = sd(world, i) {
                    s.bflags |= 2;
                    s.target = t;
                }
                let d = approach(world, i, pos);
                if d < 9
                    && let Some(s) = sd(world, i)
                {
                    s.next_search = (tick as i32 + (d - 1) * 4) as u32;
                }
            }
        }
    }
    let Some(s) = job(world, i).map(|j| j.soldier) else { return };
    if s.target != 0 && s.bflags & 2 == 0 {
        set_target(world, i, 0);
        if s.target_pos != walk_target(world, i) {
            rewalk(world, i);
        }
    }
    let r = super::soldier::walk_step(world, i);
    let Some(s) = job(world, i).map(|j| j.soldier) else { return };
    if r & 0x70 == 0 || s.target_pos == -1 {
        return;
    }
    if walk_target(world, i) != s.target_pos {
        rewalk(world, i);
        return;
    }
    if let Some(s) = sd(world, i) {
        s.state = 8;
    }
}

/// Behaviour vtbl+0xc (0x58a750) `Attack(target, kind)`: clears entity flag 0x8000000, puts the
/// first task of anim list 1 (2 for kind 1 against a member of the own team) with the target as
/// entity into the role's override task (+0x50) and takes it.
pub fn attack(world: &mut World, i: u16, target: u16) {
    crate::debug::log(world, i, "attack", target as i32, 0);
    // The squad leader's vtbl+0xc (0x58ae40) leads its group first.
    if super::soldier::is_leader(world, i) {
        super::soldier::lead(world, i);
    }
    // The saboteur's (0x57fe40) shows itself first.
    if super::soldier::is_saboteur(world, i) {
        super::soldier::saboteur_attack(world, i);
    }
    // The shaman's (0x587b60) enslaves.
    if super::shaman::is_shaman(world, i) {
        super::shaman::attack(world, i, target);
        return;
    }
    let kind = row(world, i)[4];
    let Some(e) = world.entities.by_index_mut(i as u32) else { return };
    e.flags &= !0x800_0000;
    let (race, settler, owner) = (e.race, e.kind, e.owner);
    let ally = world.entities.by_index(target as u32).is_some_and(|t| crate::warmap::team(world, t.owner) == crate::warmap::team(world, owner));
    let idx = if kind == 1 && ally { 2 } else { 1 };
    let Some(list) = world.rules.jobs.anim_list(race, settler, idx) else { return };
    let Some(mut t) = world.rules.jobs.list(race, list).and_then(|l| l.first().copied()) else { return };
    t.entity = target;
    if let Some(s) = sd(world, i) {
        s.fight_task = Some(t);
    }
    super::soldier::take_job(world, i);
    // The flying effect (0x58a7e9..): bowmen an arrow (the Dark Tribe's its own), the axe
    // throwers (settler types 0x29..0x2b) an axe, types 0x3d..0x3f theirs (0x3e two); 15 ticks
    // from now (0x3e: 10 and 20).
    let wt = world.entities.by_index(i as u32).map_or(0, |e| e.flags & 0xf);
    let shots: &[(u8, i32)] = match (wt, settler) {
        (3, _) if race & 0xf == 3 => &[(7, 15)],
        (3, _) => &[(0, 15)],
        (4, 0x29..=0x2b) => &[(1, 15)],
        (4, 0x3d) => &[(8, 15)],
        (4, 0x3e) => &[(8, 10), (8, 20)],
        (4, 0x3f) => &[(9, 15)],
        _ => &[],
    };
    for &(kind, delay) in shots {
        crate::projectile::launch_at(world, kind, i, target, delay, (0, 0));
    }
}

/// `LogicUpdateJob` (0x589250) at the end of a WORK task (0x10): the target (role +0x22), if
/// READY and vulnerable, takes `(strength · damage + 127) >> 8` (at least 1) from the soldier's
/// player; `strength` is the player's fighting strength on own-team land or on foreign land
/// (the owner of the eco-sector at the soldier's node), `damage` the settler info +3. A soldier
/// grouped with a squad leader adds `leader_bonus(race) · d / 256`, randomly rounded
/// ([`crate::rules::Rules::leader_bonus`], role +0x2c bit 0x800, 0x12dbc40).
pub fn hit(world: &mut World, i: u16) {
    let Some(target) = job(world, i).map(|j| j.entity) else { return };
    let ok = world.entities.by_index(target as u32).is_some_and(|t| t.flags & flag::READY != 0 && t.flags & flag::VULNERABLE_MASK != 0);
    if !ok {
        return;
    }
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (race, kind, owner, pos, flags) = (e.race, e.kind, e.owner, e.pos, e.flags);
    let land = world.grid.checked_index(pos.x(), pos.y()).map_or(0, |n| world.economy.owner(world.tiling.eco_sector_of(n)));
    let strength = crate::statistic::strength(world, owner, land);
    let d = (strength * world.rules.settler_damage(race, kind) as i32 + 0x7f) >> 8;
    let mut d = d + (d == 0) as i32;
    // Bloodlust (entity flag 0x100000, the Vikings' spell 5): `+ (d · BloodlustDmgIncrease256
    // + 127) >> 8`.
    if flags & crate::magic::spells::FLAG_BLOODLUST != 0 {
        d += (world.rules.magic.bloodlust_dmg_increase256.wrapping_mul(d) + 0x7f) >> 8;
    }
    // A member of a group with a squad leader (group word bit 0x800): one game-RNG draw rounds
    // `bonus · d / 256` up or down at random (up when its low byte exceeds the draw's), added.
    if job(world, i).is_some_and(|j| j.groups & crate::group::LEADER_BONUS != 0) {
        let b = world.rules.leader_bonus(race) * d;
        let r = world.rng.rand() & 0xff;
        d += (if (b & 0xff) as u32 <= r { b } else { b + 0x100 }) >> 8;
    }
    let tpos = world.entities.by_index(target as u32).map_or(pos, |t| t.pos);
    if let Some(ev) = crate::event::SimEvent::attacked(world, target) {
        world.events.push(ev);
    }
    crate::combat::take_damage(world, target, d, owner);
    if (0x3d..=0x3f).contains(&kind) {
        splash(world, i, target, tpos, d, owner, kind);
    }
    if (0x29..=0x2b).contains(&kind) {
        freeze(world, target, kind);
    }
}

/// 0x589250 for types 0x3d..0x3f: every other enemy settler (0x547f20: settlers on the spiral
/// `0..=Last(4)` around the target's node) that is a valid target of the row's mask takes
/// `damage · pct / 100` within 2 of the target, `/ 200` beyond.
fn splash(world: &mut World, i: u16, target: u16, at: Y16X16, d: i32, owner: u8, kind: u16) {
    let base = world.rules.splash_percent(kind) * d;
    let (full, half) = (base / 100, base / 200);
    let mask = row(world, i)[5];
    let hits: Vec<(u16, i32)> = crate::spiral::walk(world.grid, at.x(), at.y(), 4)
        .filter_map(|(x, y)| {
            let m = world.layers.map_object[world.grid.index(x as u32, y as u32)];
            let e = world.entities.by_index(m as u32).filter(|e| m != 0 && e.obj_type == ObjType::Settler)?;
            (m != target && enemy_valid(world, owner, m, mask)).then(|| (m, grid_distance(e.pos.x() - at.x(), e.pos.y() - at.y())))
        })
        .collect();
    for (m, dist) in hits {
        crate::combat::take_damage(world, m, if (dist - 1) & !1 > 0 { half } else { full }, owner);
    }
}

/// 0x589250 for types 0x29..0x2b: a target not yet frozen (entity flag 0x800000) whose warrior
/// type is in 0x3c (0xc for a Dark Tribe target) is frozen with chance
/// [`crate::rules::Rules::freeze_chance`] out of 65536
/// (one game-RNG draw, a second for the duration): flag 0x800000 set, future event 1 clears it
/// after `20 + (r & 15)` ticks (`GoodDistributionInfoHolder +0x20` vtbl+0xc = AddEvent).
fn freeze(world: &mut World, target: u16, kind: u16) {
    let Some(t) = world.entities.by_index(target as u32) else { return };
    let mask = if t.race == 3 { 0xc } else { 0x3c };
    if t.flags & 0x80_0000 != 0 || mask & (1 << (t.flags & 0xF)) == 0 {
        return;
    }
    if world.rng.rand() >= world.rules.freeze_chance(kind) {
        return;
    }
    let delay = (world.rng.rand() & 0xF) + 0x14;
    crate::future::add(world, 1, delay, target, 0, 0x80_0000);
    if let Some(t) = world.entities.by_index_mut(target as u32) {
        t.flags |= 0x80_0000;
    }
}

/// `LogicUpdateJob` (0x589250) at the end of task 0x22 (a medic's heal): the target, if READY,
/// gains `(strength · info+7 + 127) >> 8` (at least 1; the strength as in [`hit`]) through
/// vtbl+0x18 (settlers 0x5817d0: up to the maximum health); one healing is used up.
pub fn heal(world: &mut World, i: u16) {
    let Some(target) = job(world, i).map(|j| j.entity) else { return };
    let Some(t) = world.entities.by_index(target as u32).filter(|t| t.flags & flag::READY != 0) else { return };
    let (trace, tkind, tobj) = (t.race, t.kind, t.obj_type);
    let Some(e) = world.entities.by_index(i as u32) else { return };
    let (race, kind, owner, pos) = (e.race, e.kind, e.owner, e.pos);
    let land = world.grid.checked_index(pos.x(), pos.y()).map_or(0, |n| world.economy.owner(world.tiling.eco_sector_of(n)));
    let strength = crate::statistic::strength(world, owner, land);
    let h = (strength * world.rules.settler_misc(race, kind) as i32 + 0x7f) >> 8;
    let h = h + (h == 0) as i32;
    if tobj == ObjType::Settler {
        let max = world.rules.settler_health(trace, tkind) as i32;
        if let Some(EntityData::Settler(s)) = world.entities.by_index_mut(target as u32).map(|e| &mut e.data) {
            s.health = (s.health as i32 + h).min(max) as u8;
        }
    } else {
        unported(world, "TakeHeal of a non-settler", target);
    }
    if let Some(s) = sd(world, i)
        && s.healings > 0
    {
        s.healings -= 1;
    }
}

/// `TakeJob` WORK / 0x22 (0x589b30): displacement costs 10, the task's duration, the settler
/// turned towards the target (`Y16X16::DirectionFast` 0x575370).
pub fn take_work(world: &mut World, i: u16) {
    let Some(j) = job_mut(world, i) else { return };
    j.displacement_costs = 10;
    let (counter, target) = (j.counter, j.entity);
    super::core::register(world, i, counter as i32);
    let from = world.entities.by_index(i as u32).map(|e| e.pos);
    let to = world.entities.by_index(target as u32).map(|e| e.pos);
    if let (Some(a), Some(b)) = (from, to) {
        let dir = crate::pathing::walking::direction(a.0 as i32, b.0 as i32) as u8;
        if let Some(s) = super::core::settler_mut(world, i) {
            s.direction = dir;
        }
    }
}
