//! The Dark Tribe sector AI's agents (`CAIDarkTribeSectorAI`, ctor 0x440600) other than the
//! mushroom farms ([`super::dark::mushroom_farms`]). Decompiles in
//! `tools/re/decompiled/script/ai/darktribe2.c` and `dtagents.c`.

use crate::World;

/// The `AI` section's `CAIConfigInt`s the Dark Tribe agents read (value at object +8; static
/// initialisers in 0x410000..0x420000 register each with its section, name and default; the
/// install's config files set none of these; `DTMaxSoldiersPerTaskForce`, which Balancing.cfg
/// sets, is [`crate::magic::DarkMagicCfg::max_soldiers_per_task_force`]).
pub mod config {
    /// `DTBowmanToSwordsmanRatio256` (0x117d074).
    pub const BOWMAN_TO_SWORDSMAN_RATIO_256: i32 = 0x80;
    /// `DTFightersMin` (0x117d080).
    pub const FIGHTERS_MIN: i32 = 10;
    /// `DTGardenerMax1A256`, `…1B`, `…2A256`, `…2B`, `DTGardenerMax` (0x117d08c …).
    pub const GARDENER_MAX: [i32; 5] = [0x10, 0, 8, 5, 0x15];
    /// `DTShamanMax1A256`, `…1B`, `…2A256`, `…2B`, `DTShamanMax` (0x117d0c8 …).
    pub const SHAMAN_MAX: [i32; 5] = [0x10, 0, 8, 5, 0x15];
}

/// The settler types the Dark Tribe AI recruits (the dark temple's order slots,
/// [`crate::buildings::dark_temple::UNIT_TYPES`]).
const SWORDSMAN: u16 = 29;
const BOWMAN: u16 = 32;
const GARDENER: u16 = 52;
const SHAMAN: u16 = 54;

/// The 5047 amount that replaces an order instead of adding to it (0x545d60).
const SET: i32 = 10000;

/// 0x42ffb0 `(fighters, a1, b1, a2, b2, max)`: the smaller of `a1 · fighters / 256 + b1` and
/// `a2 · fighters / 256 + b2` (the `a`s clamped to ±0x1000, the `b`s to ±0x400), then at most
/// `max` (≤ 0x400); a negative `max` keeps only a result below 1, else 0.
fn limit(fighters: i32, [a1, b1, a2, b2, max]: [i32; 5]) -> i32 {
    let l1 = (a1.clamp(-0x1000, 0x1000).wrapping_mul(fighters) >> 8) + b1.clamp(-0x400, 0x400);
    let l2 = (a2.clamp(-0x1000, 0x1000).wrapping_mul(fighters) >> 8) + b2.clamp(-0x400, 0x400);
    let m = l1.min(l2);
    if max < 0 {
        return if m < 1 { m } else { 0 };
    }
    m.min(max.min(0x400))
}

/// `CAIAgentDarkTribeRecruitment::Execute` (0x430070), with the sector AI's anchor building
/// (+0xf0, the dark temple): nothing below 20 mana (returns cost 1). Otherwise it sets the
/// temple's orders (5047, amount + 10000 replaces the order):
/// * with swordsmen + bowmen at least `DTFightersMin` (0..=1000): dark gardeners (unless the
///   script flag 0x10000000) and shamans (unless 0x20000000) up to their [`limit`] of the
///   fighters, at most 5 more each; otherwise none;
/// * the bowmen wanted are `DTBowmanToSwordsmanRatio256` (0..=0x1000) · swordsmen / 256: with
///   too few, that many more bowmen; with too many, swordsmen up to bowmen · 256 / ratio (each
///   at most 10);
/// * only while the fighters are below `SoldierLimitAbsolute`: swordsmen and bowmen, with 50 mana
///   or more 4 swordsmen and 4 · ratio / 256 bowmen more.
pub fn recruitment(world: &mut World, player: u8, temple: u16) -> u32 {
    use config::*;
    let mana = crate::magic::mana(world, player);
    if mana < 0x14 {
        return 1;
    }
    let flags = world.ai.vars.records.get(player as usize).map_or(0, |r| r.flags);
    let count = |k: u16| world.settler_lists.settlers(player, k);
    let (swords, bows, gardeners, shamans) = (count(SWORDSMAN), count(BOWMAN), count(GARDENER), count(SHAMAN));
    let fighters = swords + bows;
    let (mut more_gardeners, mut more_shamans) = (0, 0);
    if fighters >= FIGHTERS_MIN.clamp(0, 1000) {
        if flags & super::vars::flag::DT_DARK_GARDENERS == 0 {
            more_gardeners = (limit(fighters, GARDENER_MAX) - gardeners).clamp(0, 5);
        }
        if flags & super::vars::flag::DT_SHAMANS == 0 {
            more_shamans = (limit(fighters, SHAMAN_MAX) - shamans).clamp(0, 5);
        }
    }
    let ratio = BOWMAN_TO_SWORDSMAN_RATIO_256.clamp(0, 0x1000);
    let (mut more_swords, mut more_bows) = (0, 0);
    let wanted = (ratio * swords + 0x80) >> 8;
    if wanted < bows {
        if ratio > 0 {
            more_swords = ((bows << 8) / ratio - swords).clamp(0, 10);
        }
    } else {
        more_bows = (wanted - bows).clamp(0, 10);
    }
    let w = (temple as u32) << 16;
    super::send(world, 5047, player, w | GARDENER as u32, (more_gardeners + SET) as u32);
    super::send(world, 5047, player, w | SHAMAN as u32, (more_shamans + SET) as u32);
    if fighters < world.ai.vars.get(player, 5) {
        if mana >= 0x32 {
            more_swords += 4;
            more_bows += (ratio * 4 + 0x80) >> 8;
        }
        super::send(world, 5047, player, w | SWORDSMAN as u32, (more_swords + SET) as u32);
        super::send(world, 5047, player, w | BOWMAN as u32, (more_bows + SET) as u32);
    }
    0
}

// ---- the task-force agents ---------------------------------------------------------------

use super::player::TfRef;
use super::taskforce::{self as tfs, class, flag, kind};
use crate::grid::Y16X16;

fn unpack(v: u32) -> (i32, i32) {
    ((v & 0xffff) as i32, (v as i32) >> 16)
}

fn pack(x: i32, y: i32) -> i32 {
    (y << 16) + x
}

/// The sector AI's anchor node (+0xf4 / +0xf8).
fn anchor(world: &World, player: u8, s: u8) -> (i32, i32) {
    world.ai.players[player as usize].as_ref().and_then(|a| a.body.sectors.get(s as usize)).map_or((0, 0), |sec| sec.anchor())
}

/// 0x4360f0 `(&x, &y)`: around `(x, y)` (on the map, on a sector) the first node within the
/// spiral's radius 10 that is free (flags 0x25 clear, ground not 0x2X / 0x3X) on the same sector;
/// from there on the first such node beside dark land (flag 4 on one of its six neighbours)
/// replaces `(x, y)` (1). Without one `(x, y)` moves to the first free node (−1); none at all: 0.
fn dark_spot(world: &World, x: &mut i32, y: &mut i32) -> i32 {
    let size = world.grid.size() as i32;
    if !(0..size).contains(x) || !(0..size).contains(y) {
        return 0;
    }
    let sector = world.tiling.sector_of((*y * size + *x) as usize);
    if sector == 0 {
        return 0;
    }
    let free = |nx: i32, ny: i32| -> Option<usize> {
        if !(0..size).contains(&nx) || !(0..size).contains(&ny) {
            return None;
        }
        let n = (ny * size + nx) as usize;
        let g = world.layers.ground[n] & 0xf0;
        (world.layers.flags[n] & 0x25 == 0 && g != 0x30 && g != 0x20 && world.tiling.sector_of(n) == sector).then_some(n)
    };
    let last = crate::spiral::last(10);
    let Some(first) = (0..=last).find(|&k| free(*x + crate::spiral::dx(k), *y + crate::spiral::dy(k)).is_some()) else { return 0 };
    let near_dark = |n: usize| {
        let s = size as isize;
        [1, s + 1, s, -1, -s - 1, -s].iter().any(|&d| {
            let m = n as isize + d;
            m >= 0 && (m as usize) < world.layers.flags.len() && world.layers.flags[m as usize] & 4 != 0
        })
    };
    for k in first..=last {
        let (nx, ny) = (*x + crate::spiral::dx(k), *y + crate::spiral::dy(k));
        if let Some(n) = free(nx, ny)
            && near_dark(n)
        {
            *x = nx;
            *y = ny;
            return 1;
        }
    }
    *x += crate::spiral::dx(first);
    *y += crate::spiral::dy(first);
    -1
}

/// 0x441290 `(&x, &y, radius)`: where a squad or shamans should stand. Of the player's mushroom
/// farms (the `CBuildingMgr` list 0x14eeb38) whose ensign lies on the sector of `(x, y)`, the
/// one with the best score (one game RNG draw each): `rand & 31 − own strength / 2 −
/// a / 8 + 4 · foreign strength − d` (the war map cell of the ensign; `d` the distance to
/// `(x, y)`; `a` the "distance" to the anchor as the release computes it, `|ex − ax|` and
/// `|ex − ay|` — the ensign's x twice), `+ 8d − 120` when `d < 15`, `+ 50 (+ 50 − a when
/// a < 50)` with foreign strength and own strength at most 16, `+ 32 · (radius − a)` past the
/// radius. The ensign replaces `(x, y)`.
fn station(world: &mut World, player: u8, s: u8, x: &mut i32, y: &mut i32, radius: i32) -> bool {
    let size = world.grid.size() as i32;
    let Some(n) = world.grid.checked_index(*x, *y) else { return false };
    let sector = world.tiling.sector_of(n);
    if sector == 0 {
        return false;
    }
    let team = crate::warmap::team(world, player);
    let (ax, ay) = anchor(world, player, s);
    let mut best: Option<u16> = None;
    let mut best_score = -0x10_0000;
    for b in world.building_list(player as i32, 0x31) {
        let en = match world.entities.by_index(b as u32).map(|e| &e.data) {
            Some(crate::entity::EntityData::Building(d)) => d.ensign,
            _ => Y16X16(u32::MAX),
        };
        let (ex, ey) = (en.x(), (en.0 as i32) >> 16);
        if !(0..size).contains(&ex) || !(0..size).contains(&ey) || world.tiling.sector_of((ey * size + ex) as usize) != sector {
            continue;
        }
        let d = crate::spiral::grid_distance(ex - *x, ey - *y);
        let c = world.warmap.cell_strength(ex >> 4, ey >> 4);
        let own = c.get(team).copied().unwrap_or(0) as i32;
        let foreign = c[0] as i32 - own;
        let a = crate::spiral::grid_distance(ex - ax, ex - ay);
        let r = world.rng.rand();
        let mut score = (r & 0x1f) as i32 - (own >> 1) - a / 8 + foreign * 4 - d;
        if d < 0xf {
            score += d * 8 - 0x78;
        }
        if foreign > 0 && own <= 0x10 {
            score += 0x32;
            if a < 0x32 {
                score += 0x32 - a;
            }
        }
        if radius < a {
            score += (radius - a) * 0x20;
        }
        if best_score < score {
            best_score = score;
            best = Some(b);
        }
    }
    let Some(b) = best else { return false };
    let en = match world.entities.by_index(b as u32).map(|e| &e.data) {
        Some(crate::entity::EntityData::Building(d)) => d.ensign.0,
        _ => u32::MAX,
    };
    *x = (en & 0xffff) as i32;
    *y = (en as i32) >> 16;
    true
}

/// `CAIAgentDarkTribeDarkGardeners::Execute` (0x430930): per dark gardener task force (the
/// group's class 6 list) — an empty one is destroyed. One with a darkening order (5) less than
/// 1000 ticks after its last look (+0x4c, not 0) and no re-target flag waits for 250 ticks
/// since that look and since the order; then with a dark spot by its target (0x4360f0) or, if
/// none, a target beside the target from the plan (vtbl+8 — the release then still orders the
/// old target) the order 5 is repeated at the target. Otherwise: flag 0x4000000 cleared, the look
/// noted, the plan's next target (vtbl+0xc) ordered (5), and when the first member is more than 5
/// from it (its dark spot) the next look waits `12 · distance` ticks.
pub fn gardeners(world: &mut World, player: u8, s: u8, now: u32) -> u32 {
    for r in tfs::list(world, player, s, class::DARK_GARDENERS) {
        if tfs::count(world, r) <= 0 {
            tfs::destroy(world, r);
            continue;
        }
        let (mx, my) = tfs::first_pos(world, r).map_or((-1, -1), |p| (p.x(), (p.0 as i32) >> 16));
        let Some(t) = tfs::get(world, r) else { continue };
        let (fl, b4c, target, ordered) = (t.flags(), t.b4c(), t.target(), t.order_tick());
        if fl & flag::RETARGET == 0 && b4c.wrapping_add(1000) > now && b4c != 0 && fl & 0xff0 == 0x50 {
            if b4c.wrapping_add(0xfa) > now || ordered.wrapping_add(0xfa) > now {
                continue;
            }
            let (mut x, mut y) = unpack(target as u32);
            if dark_spot(world, &mut x, &mut y) != 0 {
                tfs::order(world, r, 5, pack(x, y), 0);
                continue;
            }
            let near = take_plan(world, player).map_or(0, |(w, d)| {
                let v = d.target_near(w, target as u32);
                put_plan(w, player, d);
                v
            });
            if (near as i32) > 0 {
                // 0x430a38: the new point goes through 0x4360f0, but the order takes the old
                // target's coordinates (0x430a06) [release behaviour].
                let (mut nx, mut ny) = unpack(near);
                dark_spot(world, &mut nx, &mut ny);
                tfs::order(world, r, 5, pack(x, y), 0);
                continue;
            }
        }
        if let Some(t) = super::player::tf_mut(world, r) {
            t.set_flags(t.flags() & !flag::RETARGET);
            t.ints[5] = now as i32;
        }
        let next = take_plan(world, player).map_or(0, |(w, mut d)| {
            let v = d.take_target();
            put_plan(w, player, d);
            v.unwrap_or(0)
        });
        if (next as i32) > 0 {
            let (mut x, mut y) = unpack(next);
            dark_spot(world, &mut x, &mut y);
            let d = crate::spiral::grid_distance(x - mx, y - my);
            if d > 5
                && let Some(t) = super::player::tf_mut(world, r)
            {
                t.ints[5] = now.wrapping_add(d as u32 * 12) as i32;
            }
            tfs::order(world, r, 5, next as i32, 0);
        }
    }
    0
}

fn take_plan(world: &mut World, player: u8) -> Option<(&mut World, Box<super::dark::DarkTribe>)> {
    let d = world.ai.players.get_mut(player as usize)?.as_mut()?.dark_ai.take()?;
    Some((world, d))
}

fn put_plan(world: &mut World, player: u8, d: Box<super::dark::DarkTribe>) {
    if let Some(Some(a)) = world.ai.players.get_mut(player as usize) {
        a.dark_ai = Some(d);
    }
}

/// `CAIAgentDarkTribeShamans::Execute` (0x430820): per shaman task force (class 7) — an empty
/// one is destroyed; one ordered 3 or 9 or reserved by the attack (0x20000000) is left alone.
/// One moving (order 1) without the re-target flag draws the game RNG and keeps going until
/// `rand & 511 + 200` ticks after its order; then, or right away otherwise, flag 0x4000000 is
/// cleared and it moves (1) to the farm [`station`] picks within 120 of its first member.
pub fn shamans(world: &mut World, player: u8, s: u8, now: u32) -> u32 {
    for r in tfs::list(world, player, s, class::SHAMANS) {
        if tfs::count(world, r) <= 0 {
            tfs::destroy(world, r);
            continue;
        }
        let Some(t) = tfs::get(world, r) else { continue };
        let (fl, ordered) = (t.flags(), t.order_tick());
        let cmd = fl >> 4 & 0xff;
        if cmd == 3 || cmd == 9 || fl & flag::ATTACK != 0 {
            continue;
        }
        if fl & flag::RETARGET == 0 && fl & 0xff0 == 0x10 {
            let r16 = world.rng.rand();
            if (r16 & 0x1ff) + 200 + ordered > now {
                continue;
            }
        }
        if let Some(t) = super::player::tf_mut(world, r) {
            t.set_flags(t.flags() & !flag::RETARGET);
        }
        let Some(p) = tfs::first_pos(world, r) else { continue };
        let (mut x, mut y) = (p.x(), (p.0 as i32) >> 16);
        if station(world, player, s, &mut x, &mut y, 0x78) {
            tfs::order(world, r, 1, pack(x, y), 0);
        }
    }
    0
}

/// 0x430500 `(settler type, slot)`: the player's settlers of the type (its `CSettlerMgr` list,
/// alive ones: flags 0x3000000) that are in no task force join the sector AI's slot. The release
/// starts with the first alive settler but steps on from the list head, so a dead head makes it
/// look at the first alive one twice (harmless); at most 9999 steps.
fn collect(world: &mut World, player: u8, s: u8, settler: u16, slot: u8) {
    let has = world.ai.players[player as usize].as_ref().is_some_and(|a| a.body.sectors.get(s as usize).is_some_and(|x| x.slots.get(slot as usize).is_some_and(|k| k.is_some())));
    if !has {
        return;
    }
    let alive = |w: &World, id: u16| w.entities.by_index(id as u32).is_some_and(|e| e.flags & 0x0300_0000 != 0);
    let next = |w: &World, id: u16| w.entities.by_index(id as u32).map_or(0, |e| e.next);
    let next_alive = |w: &World, mut id: u16| loop {
        id = next(w, id);
        if id == 0 || alive(w, id) {
            break id;
        }
    };
    let mut list = world.settler_lists.head(player, settler);
    let mut cur = if list != 0 && !alive(world, list) { next_alive(world, list) } else { list };
    let mut steps = 0;
    while cur != 0 {
        steps += 1;
        if steps > 9999 {
            return;
        }
        if !world.ai.infos.contains_key(&cur) {
            tfs::add(world, TfRef::Slot(player, s, slot), cur);
        }
        list = next_alive(world, list);
        cur = list;
    }
}

/// 0x430300 `(slot, type, max)`: the empty task forces of the type's class are destroyed; the
/// slot's members fill the first task forces of the type (from the list head) with fewer than
/// `max` members and not reserved by the attack, the rest go into new ones of the type.
fn distribute(world: &mut World, player: u8, s: u8, slot: u8, kind: i32, max: i32) {
    let class = super::model::class_of(kind);
    for r in tfs::list(world, player, s, class) {
        if tfs::count(world, r) == 0 {
            tfs::destroy(world, r);
        }
    }
    let slot_ref = TfRef::Slot(player, s, slot);
    let mut members = tfs::members(world, slot_ref).into_iter();
    let mut pending = members.next();
    let list = tfs::list(world, player, s, class);
    let mut it = list.into_iter();
    let mut cur = it.next();
    while let (Some(m), Some(r)) = (pending, cur) {
        let ok = tfs::kind_of_ref(world, r) == kind && tfs::count(world, r) < max && tfs::get(world, r).is_some_and(|t| t.flags() & flag::ATTACK == 0);
        if !ok {
            cur = it.next();
            continue;
        }
        pending = members.next();
        tfs::add(world, r, m);
    }
    while let Some(m) = pending {
        pending = members.next();
        if cur.is_none_or(|r| tfs::count(world, r) >= max) {
            cur = tfs::create(world, player, s, kind);
        }
        if let Some(r) = cur {
            tfs::add(world, r, m);
        }
    }
}

/// 0x43fbd0 `(task force, position, slot, n)`: up to `n` members of the slot, nearest first —
/// bucketed by distance to the position (`d / 8` below 312, else bucket 38; 40 per bucket, the
/// rest dropped; each bucket in slot order) — join the task force.
pub(crate) fn fill(world: &mut World, r: TfRef, player: u8, s: u8, pos: i32, slot: u8, n: i32) {
    if n <= 0 {
        return;
    }
    let slot_ref = TfRef::Slot(player, s, slot);
    if tfs::get(world, slot_ref).is_none() {
        return;
    }
    let (x, y) = unpack(pos as u32);
    let mut buckets: Vec<Vec<u16>> = vec![Vec::new(); 0x4f];
    for m in tfs::members(world, slot_ref) {
        let Some(e) = world.entities.by_index(m as u32) else { continue };
        let d = crate::spiral::grid_distance(x - e.x(), y - e.y());
        let b = if d < 0x138 { (d >> 3) as usize } else { 0x26 };
        if buckets[b].len() < 0x28 {
            buckets[b].push(m);
        }
    }
    for m in buckets.into_iter().flatten().take(n as usize) {
        tfs::add(world, r, m);
    }
}

/// 0x430450 `(class)`: each squad (class 2) without a link is linked with the next task force of
/// the class without a link and not reserved by the attack, and ordered to follow it (6).
fn escort(world: &mut World, player: u8, s: u8, class: u8) {
    let squads = tfs::list(world, player, s, class::SQUAD);
    let others = tfs::list(world, player, s, class);
    let (mut i, mut j) = (0, 0);
    loop {
        while j < others.len() && (tfs::linked(world, others[j]).is_some() || tfs::get(world, others[j]).is_some_and(|t| t.flags() & flag::ATTACK != 0)) {
            j += 1;
        }
        while i < squads.len() && tfs::linked(world, squads[i]).is_some() {
            i += 1;
        }
        let (Some(&sq), Some(&o)) = (squads.get(i), others.get(j)) else { return };
        let (TfRef::Group(_, _, a), TfRef::Group(_, _, b)) = (sq, o) else { return };
        if let Some(sec) = world.ai.players[player as usize].as_mut().and_then(|p| p.body.sectors.get_mut(s as usize)) {
            sec.group.link(a, b);
        }
        tfs::order(world, sq, 6, -1, 0);
    }
}

/// `CAIAgentDarkTribeSquadManager::Execute` (0x4305d0), step +0x28 (0..7, not saved):
/// 1. at steps 1 / 3 / 5 / 6 the free swordsmen / bowmen / dark gardeners / shamans join the
///    slots 2 / 3 / 7 / 4 ([`collect`]);
/// 2. the slots go into task forces ([`distribute`]): dark gardeners (0xb) by 3, shamans (0xc) by 3,
///    manakopters (0xd) one each; empty squads are destroyed;
/// 3. with fighters in slots 2 / 3: fewer squads than gardener and shaman task forces + 1 make a
///    new squad (type 2) of up to 10 swordsmen nearest the anchor, else the smallest type-2 squad
///    takes up to 10 nearest its first member; then up to 5 bowmen ([`fill`]);
/// 4. unless the script flag 0x40000000: free squads escort free gardener and shaman task forces
///    ([`escort`]); each other squad draws the game RNG and, `rand & 511 + 500` ticks after its
///    order, moves (1) to the farm [`station`] picks within 50 of its first member.
pub fn squad_manager(world: &mut World, player: u8, s: u8, now: u32) -> u32 {
    let k = 5u8;
    let step = world.ai.players[player as usize].as_ref().and_then(|a| match &a.body.sectors.get(s as usize)?.kind {
        super::model::SectorKind::Dark { agents } => agents.get(k as usize).map(|a| a.state),
        _ => None,
    });
    let Some(step) = step else { return 0 };
    match step {
        1 => collect(world, player, s, SWORDSMAN, 2),
        3 => collect(world, player, s, BOWMAN, 3),
        5 => collect(world, player, s, GARDENER, 7),
        6 => collect(world, player, s, SHAMAN, 4),
        _ => {}
    }
    if let Some(super::model::SectorKind::Dark { agents }) = world.ai.players[player as usize].as_mut().and_then(|a| a.body.sectors.get_mut(s as usize)).map(|x| &mut x.kind)
        && let Some(a) = agents.get_mut(k as usize)
    {
        a.state = (a.state + 1) & 7;
    }
    distribute(world, player, s, 7, kind::DARK_GARDENERS, 3);
    distribute(world, player, s, 4, kind::SHAMANS, 3);
    distribute(world, player, s, 9, kind::MANAKOPTERS, 1);
    for r in tfs::list(world, player, s, class::SQUAD) {
        if tfs::count(world, r) == 0 {
            tfs::destroy(world, r);
        }
    }
    if tfs::count(world, TfRef::Slot(player, s, 2)) + tfs::count(world, TfRef::Slot(player, s, 3)) > 0 {
        let squads = tfs::list(world, player, s, class::SQUAD);
        let mut smallest = None;
        let mut min = 0x10000;
        for &r in &squads {
            if tfs::kind_of_ref(world, r) == kind::SQUAD {
                let c = tfs::count(world, r);
                if c < min {
                    min = c;
                    smallest = Some(r);
                }
            }
        }
        let others = tfs::list(world, player, s, class::DARK_GARDENERS).len() + tfs::list(world, player, s, class::SHAMANS).len();
        let target = if squads.len() < others + 1 {
            let (ax, ay) = anchor(world, player, s);
            let pos = pack(ax, ay);
            let r = tfs::create(world, player, s, kind::SQUAD);
            if let Some(r) = r {
                fill(world, r, player, s, pos, 2, 10);
            }
            r.map(|r| (r, pos))
        } else {
            smallest.map(|r| {
                let pos = tfs::first_pos(world, r).map_or(-1, |p| p.0 as i32);
                fill(world, r, player, s, pos, 2, 10);
                (r, pos)
            })
        };
        if let Some((r, pos)) = target {
            fill(world, r, player, s, pos, 3, 5);
        }
    }
    let flags = world.ai.vars.records.get(player as usize).map_or(0, |r| r.flags);
    if flags & super::vars::flag::DT_IGNORE_FIGHTERS == 0 {
        escort(world, player, s, class::DARK_GARDENERS);
        escort(world, player, s, class::SHAMANS);
        for r in tfs::list(world, player, s, class::SQUAD) {
            if tfs::linked(world, r).is_some() {
                continue;
            }
            let ordered = tfs::get(world, r).map_or(0, |t| t.order_tick());
            let r16 = world.rng.rand();
            if (r16 & 0x1ff) + 500 + ordered > now {
                continue;
            }
            let Some(p) = tfs::first_pos(world, r) else { continue };
            let (mut x, mut y) = (p.x(), (p.0 as i32) >> 16);
            if station(world, player, s, &mut x, &mut y, 0x32) {
                tfs::order(world, r, 1, pack(x, y), 0);
            }
        }
    }
    0
}

/// `CAIAgentDarkTribeManakopterHalls::Execute` (0x430c20): at tick 0 nothing. The player's hall
/// (the `CBuildingMgr` cursor of type 80, 0x14f0eb8, else the next one 0x53d0b0) READY, no
/// manakopter of the player (0x42ff20) and more than `DTManakopterDelay` (AI variable 20) since
/// the last one was started (+0x28, in units of 14 ticks): [`start`] — when it starts, +0x28 =
/// now / 14 and the cost `DTManakopterDelay · 0xe000`; otherwise the hall's AI fill 0x547e30.
///
/// [`start`]: crate::buildings::manakopter::start
pub fn manakopter_halls(world: &mut World, player: u8, s: u8, now: u32) -> u32 {
    if now == 0 {
        return 0;
    }
    let Some(hall) = crate::buildings::manakopter::hall_of(world, player) else { return 0 };
    let ready = world.entities.by_index(hall as u32).is_some_and(|e| e.flags & crate::entity::flag::READY != 0);
    let last = agent(world, player, s, 7).map_or(0, |a| a.extra.first().copied().unwrap_or(0));
    let delay = world.ai.vars.get(player, 20);
    if !ready || crate::flying::count(world, player) != 0 || delay >= (now / 0xe) as i32 - last {
        return 0;
    }
    if crate::buildings::manakopter::start(world, hall) {
        if let Some(a) = agent_mut(world, player, s, 7)
            && let Some(x) = a.extra.first_mut()
        {
            *x = (now / 0xe) as i32;
        }
        return (delay as u32).wrapping_mul(0xe000);
    }
    crate::buildings::manakopter::fill_for_ai(world, hall);
    0
}

/// The Dark Tribe sector AI's agent `k`.
pub(crate) fn agent(world: &World, player: u8, s: u8, k: usize) -> Option<&super::model::Agent> {
    match &world.ai.players.get(player as usize)?.as_ref()?.body.sectors.get(s as usize)?.kind {
        super::model::SectorKind::Dark { agents } => agents.get(k),
        _ => None,
    }
}

pub(crate) fn agent_mut(world: &mut World, player: u8, s: u8, k: usize) -> Option<&mut super::model::Agent> {
    match &mut world.ai.players.get_mut(player as usize)?.as_mut()?.body.sectors.get_mut(s as usize)?.kind {
        super::model::SectorKind::Dark { agents } => agents.get_mut(k),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_of_the_fighters() {
        // 0x10 · 40 / 256 + 0 = 2; 8 · 40 / 256 + 5 = 6: 2.
        assert_eq!(limit(40, config::GARDENER_MAX), 2);
        // 0x10 · 400 / 256 = 25; 8 · 400 / 256 + 5 = 17; at most 0x15.
        assert_eq!(limit(400, config::GARDENER_MAX), 17);
        assert_eq!(limit(4000, config::SHAMAN_MAX), 0x15);
        assert_eq!(limit(10, [0, 3, 0, 4, -1]), 0);
        assert_eq!(limit(10, [0, -3, 0, 4, -1]), -3);
    }
}
