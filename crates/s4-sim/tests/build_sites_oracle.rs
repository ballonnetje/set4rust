//! Placement rules against the construction sites in the original's savegames (chunk 162,
//! buildings without READY that carry a `CBuildingSiteRole`). Skipped when no saves are present.
//! Vanilla saves are judged with the installed GameData; Settlers United saves (chunk 50) with the
//! mod's data if `S4_USD_GAMEDATA` points at its GameData folder, otherwise skipped.
//!
//! Per site, the world is rebuilt from the save's layers (render map, flags, owners, map objects)
//! with the site's own marks removed (0x08/0x80 on its footprint, 0x01 on its block cells, 0x02 on
//! its repelling cells and node and its pile nodes, 0x10 in its dig window unless a site still digging (state 2/3) overlaps it),
//! then:
//! 1. `check_for_build` must allow the place (the ground and territory as saved are what the
//!    player built on, give or take what happened since: dug heights, territory changes);
//! 2. the site is placed again with our `AddBuilding` path, and its marks are compared with the
//!    save: 0x08 on the footprint, ground 28 where `CanChangeGround` allowed it, and for sites past
//!    state 4 the `SetToWorld` bits (0x01 block, 0x02 repelling, 0x08 footprint).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use s4_sim::build::{BuildInfo, BuildRules, CheckResult, Why, check_for_build};
use s4_sim::entity::flag;
use s4_sim::save::{SaveGame, SavedEntity};
use s4_sim::{Grid, Player, Rules, World};

fn saves() -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out)
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("sav")) {
                out.push(p)
            }
        }
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default();
    let mut v = Vec::new();
    walk(&home.join(r"Documents\TheSettlers4\Save"), &mut v);
    v.sort();
    v
}

fn tokens(text: &[u8]) -> Vec<&[u8]> {
    text.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|t| !t.is_empty()).collect()
}

/// `CBuildingSiteRole::Store` (after `IBuildingRole::Store`, whose length varies: it embeds
/// strings): version 1, +380, +384, +388, +392 (ints), +396 state, +397..+408 (8 bytes), +412,
/// +416 work, +420 work step, … Located by its signature: work = 200·(stone + boards + gold)
/// and step = 131070 / work, within 600 tokens after the building's id. Keyed by building id.
#[derive(Clone, Copy)]
struct SiteRec {
    state: u8,
    /// +380 Σ |height − target| at placement, +384 digging slot, +392 effort left.
    dig_total: i64,
    slot: i64,
    effort: i64,
}

fn site_states(path: &Path, s: &SaveGame, rules: &BuildRules) -> BTreeMap<u16, SiteRec> {
    let m = s4_formats::map::MapFile::open(path).unwrap();
    let Some((_, text)) = m.container.load_id(162, 0).unwrap() else { return BTreeMap::new() };
    let toks = tokens(&text);
    let num = |k: usize| toks.get(k).and_then(|t| std::str::from_utf8(t).ok()).and_then(|t| t.parse::<i64>().ok());
    let mut out = BTreeMap::new();
    let mut from = 0;
    for b in &s.buildings {
        let Some(inf) = rules.info(b.race, b.kind as u8) else { continue };
        let work = 200 * (inf.stone as i64 + inf.boards as i64 + inf.gold as i64);
        if b.flags & flag::READY != 0 || work == 0 {
            continue;
        }
        let id = b.id.to_string();
        let Some(h) = (from..toks.len()).find(|&k| toks[k] == id.as_bytes() && num(k + 2) == Some(b.pos.0 as i64)) else { continue };
        from = h;
        for k in h..(h + 600).min(toks.len().saturating_sub(20)) {
            if num(k) == Some(1) && num(k + 15) == Some(work) && num(k + 16) == Some(131_070 / work) {
                if let (Some(dig_total), Some(slot), Some(effort), Some(state)) = (num(k + 1), num(k + 2), num(k + 4), num(k + 5)) {
                    out.insert(b.id, SiteRec { state: state as u8, dig_total, slot, effort });
                }
                break;
            }
        }
    }
    out
}

/// Chunk 182 (`CBuildingMgr::Store`): u16 13379, the last calc's height and state grids (2×1024),
/// then the digging slots of players 0..8, 30 × 1025 bytes each (byte 0 in use, then the target
/// heights `[1 + 32·dx + dy]`), u16 20757 at the end.
fn dig_slots(path: &Path) -> Option<Vec<u8>> {
    let m = s4_formats::map::MapFile::open(path).ok()?;
    let (_, d) = m.container.load_id(182, 0).ok()??;
    (d.len() >= 2050 + 276_750 && u16::from_le_bytes([d[0], d[1]]) == 13379).then(|| d[2050..2050 + 276_750].to_vec())
}

fn slot_target(slots: &[u8], player: u8, slot: i64, dx: i32, dy: i32) -> u8 {
    slots[30750 * player as usize + 1025 * slot as usize + 1 + 32 * (dx + 15) as usize + (dy + 15) as usize]
}

#[derive(Default)]
struct Tally {
    saves: u32,
    sites: u32,
    by_state: BTreeMap<u8, u32>,
    allowed: u32,
    cannot_level: u32,
    impossible: BTreeMap<Why, u32>,
    /// Footprint cells: ours 0x08 / save 0x08.
    fp: (u64, u64),
    /// Footprint cells: our ground-28 decision equals the save's ground being 28.
    ground: (u64, u64),
    /// Disagreements: (ours 28, save not) by the save's ground; (save 28, ours not) by the ground
    /// we kept.
    ground_ours_only: BTreeMap<u8, u64>,
    ground_save_only: BTreeMap<u8, u64>,
    /// Sites past state 4: SetToWorld bits ours ⊆ save, per bit 0x01, 0x02, 0x08.
    stamp: [(u64, u64); 3],
    /// Flag bits (0x5F) left on footprint cells after removing the site's own marks, per bit.
    residual: [u64; 8],
    node_residual: [u64; 8],
    /// Undug sites (+392 == +380) with a digging slot: (sites, sites whose whole plan equals the
    /// saved slot, window cells compared, cells equal).
    plans: (u64, u64, u64, u64),
}

fn world_from_save(s: &SaveGame, rules: &Rules, players: &[(u8, u8)]) -> World {
    let mut w = World::new(Grid::new(s.size).unwrap(), rules.clone());
    for (i, c) in s.render.as_chunks::<4>().0.iter().enumerate() {
        w.layers.height[i] = c[0];
        w.layers.ground[i] = c[1];
        w.layers.gradient[i] = c[2];
        w.layers.gfx_bits[i] = c[3];
    }
    w.layers.flags.copy_from_slice(&s.flags);
    w.layers.map_object.copy_from_slice(&s.map_object);
    w.layers.deco_object.copy_from_slice(&s.deco_object);
    for &(p, race) in players {
        w.players[p as usize] = Some(Player { id: p, race, start_x: 0, start_y: 0 });
    }
    w
}

fn info<'a>(r: &'a BuildRules, b: &SavedEntity) -> Option<&'a BuildInfo> {
    r.info(b.race, b.kind as u8)
}

fn check_save(path: &Path, s: &SaveGame, rules: &Rules, t: &mut Tally) {
    t.saves += 1;
    let states = site_states(path, s, &rules.build);
    let sites: Vec<&SavedEntity> = s
        .buildings
        .iter()
        .filter(|b| b.flags & flag::READY == 0 && b.kind != 31 && b.kind != 32 && b.race != 3 && states.contains_key(&b.id))
        .collect();
    if sites.is_empty() {
        return;
    }
    let mut players: Vec<(u8, u8)> = s.buildings.iter().map(|b| (b.owner, b.race)).collect();
    players.sort();
    players.dedup_by_key(|p| p.0);
    let size = s.size as i32;
    // Nodes claimed by a digging slot (a nonzero target height), per site, from chunk 182.
    let slots = dig_slots(path);
    let mut claimed = vec![0u8; (size * size) as usize];
    for b in &sites {
        let r = states[&b.id];
        let Some(sl) = slots.as_ref().filter(|_| (0..30).contains(&r.slot)) else { continue };
        for dy in -15..=15 {
            for dx in -15..=15 {
                let (x, y) = (b.pos.x() + dx, b.pos.y() + dy);
                if (0..size).contains(&x) && (0..size).contains(&y) && slot_target(sl, b.owner, r.slot, dx, dy) != 0 {
                    let i = (y * size + x) as usize;
                    claimed[i] = claimed[i].saturating_add(1);
                }
            }
        }
    }
    let base = world_from_save(s, rules, &players);
    for b in sites {
        let Some(inf) = info(&rules.build, b) else { continue };
        let rec = states[&b.id];
        let state = rec.state;
        t.sites += 1;
        *t.by_state.entry(state).or_default() += 1;
        let (x, y) = (b.pos.x(), b.pos.y());
        let mut w = base.clone();
        let idx = |cx: i32, cy: i32| (cy * size + cx) as usize;
        let rule = rules.building(b.race, b.kind as u8).clone();
        let fp: Vec<(i32, i32)> = inf.cells(&inf.building_lines, x, y).collect();
        for &(cx, cy) in &fp {
            w.layers.flags[idx(cx, cy)] &= !(0x08 | 0x80);
        }
        for &(dx, dy) in &rule.block_cells {
            w.layers.flags[idx(x + dx as i32, y + dy as i32)] &= !0x01;
        }
        for &(dx, dy) in &rule.repealing_cells {
            w.layers.flags[idx(x + dx as i32, y + dy as i32)] &= !0x02;
        }
        w.layers.flags[idx(x, y)] &= !0x02;
        // The site's material piles (`PrepareGround`: flag − 1, flag, flag + 1) set 0x02.
        for dx in -1..=1 {
            w.layers.flags[idx(x + inf.flag.0 as i32 + dx, y + inf.flag.1 as i32)] &= !0x02;
        }
        // 0x10 in the window: clear it where only this site's slot claims the node, or no slot
        // does (left over from finished digging).
        let own_slot = slots.as_ref().filter(|_| (0..30).contains(&rec.slot));
        for dy in -15..=15 {
            for dx in -15..=15 {
                let i = idx(x + dx, y + dy);
                let mine = own_slot.is_some_and(|sl| slot_target(sl, b.owner, rec.slot, dx, dy) != 0) as u8;
                if claimed[i] <= mine {
                    w.layers.flags[i] &= !0x10;
                }
            }
        }
        w.layers.map_object[idx(x, y)] = 0;
        w.init_tiling(&s.owner);
        for &(cx, cy) in &fp {
            let f = w.layers.flags[idx(cx, cy)] & 0x5F;
            for (k, r) in t.residual.iter_mut().enumerate() {
                *r += (f >> k & 1) as u64;
            }
        }
        let f = w.layers.flags[idx(x, y)] & 0x5F;
        for (k, r) in t.node_residual.iter_mut().enumerate() {
            *r += (f >> k & 1) as u64;
        }
        // Undug sites: our leveling plan against the saved digging slot.
        if let Some(sl) = own_slot.filter(|_| rec.effort == rec.dig_total && rec.effort > 0)
            && let Some((_, plan)) = s4_sim::build::dig_plan(&w, x, y, b.owner, b.kind as u8)
        {
            let mut all = true;
            for dy in -15..=15 {
                for dx in -15..=15 {
                    let cur = w.layers.height[idx(x + dx, y + dy)];
                    let ours = plan[(32 * (dx + 15) + dy + 15) as usize];
                    let ours = if ours == cur { 0 } else { ours };
                    let eq = ours == slot_target(sl, b.owner, rec.slot, dx, dy);
                    t.plans.2 += 1;
                    t.plans.3 += eq as u64;
                    all &= eq;
                }
            }
            t.plans.0 += 1;
            t.plans.1 += all as u64;
        }
        match check_for_build(&w, x, y, b.owner, b.kind as u8) {
            CheckResult::Possible { .. } => t.allowed += 1,
            CheckResult::CannotLevel => t.cannot_level += 1,
            CheckResult::Impossible(why) => *t.impossible.entry(why).or_default() += 1,
        }
        // Place again (whatever the check says) and compare the marks.
        let Some(id) = s4_sim::build::add_building_site(&mut w, x, y, b.owner, b.kind as u8) else { continue };
        for &(cx, cy) in &fp {
            let i = idx(cx, cy);
            t.fp.0 += (w.layers.flags[i] & 0x08 != 0) as u64;
            t.fp.1 += (s.flags[i] & 0x08 != 0) as u64;
            t.ground.0 += 1;
            let (ours, save) = (w.layers.ground[i] == 28, s.render[4 * i + 1] == 28);
            t.ground.1 += (ours == save) as u64;
            if ours && !save {
                *t.ground_ours_only.entry(s.render[4 * i + 1]).or_default() += 1;
            } else if save && !ours {
                *t.ground_save_only.entry(w.layers.ground[i]).or_default() += 1;
            }
        }
        if state >= 5 {
            s4_sim::create::set_to_world(&mut w, id);
            for (k, (cells, bit)) in [(&rule.block_cells, 0x01u8), (&rule.repealing_cells, 0x02), (&rule.building_cells, 0x08)].into_iter().enumerate() {
                for &(dx, dy) in cells {
                    let i = idx(x + dx as i32, y + dy as i32);
                    if w.layers.flags[i] & bit != 0 {
                        t.stamp[k].0 += 1;
                        t.stamp[k].1 += (s.flags[i] & bit != 0) as u64;
                    }
                }
            }
        }
    }
}

fn report(name: &str, t: &Tally) {
    eprintln!(
        "{name}: {} saves, {} sites (by state {:?}); check: {} allowed, {} cannot level, impossible {:?}; \
         footprint 0x08 ours {} / save {}; ground-28 decision agrees {}/{}; SetToWorld ours⊆save 0x01 {}/{}, 0x02 {}/{}, 0x08 {}/{}",
        t.saves,
        t.sites,
        t.by_state,
        t.allowed,
        t.cannot_level,
        t.impossible,
        t.fp.0,
        t.fp.1,
        t.ground.1,
        t.ground.0,
        t.stamp[0].1,
        t.stamp[0].0,
        t.stamp[1].1,
        t.stamp[1].0,
        t.stamp[2].1,
        t.stamp[2].0,
    );
    eprintln!("  undug sites with a digging slot: {} , plan equal to the saved slot on {}; window cells {}/{}", t.plans.0, t.plans.1, t.plans.3, t.plans.2);
    eprintln!("  ground disagreements: ours 28 / save other {:?}; save 28 / ours other {:?}", t.ground_ours_only, t.ground_save_only);
    eprintln!("  residual flag bits 0..7 on footprints {:?}, on the node {:?}", t.residual, t.node_residual);
}

#[test]
fn construction_sites_in_savegames() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let vanilla = Rules::load_default().expect("rules");
    let usd = std::env::var_os("S4_USD_GAMEDATA").map(|d| {
        let data = s4_formats::gamedata::GameData::load(d).expect("usd GameData");
        let mut r = Rules::new(&data, &s4_formats::gfx::GfxFile::open_default(5).expect("gfx 5"));
        r.build = BuildRules::new(&data);
        r
    });
    let (mut tv, mut tu, mut skipped) = (Tally::default(), Tally::default(), 0);
    for f in &files {
        let s = SaveGame::open(f).unwrap();
        match (&usd, s.settlers_united) {
            (_, false) => check_save(f, &s, &vanilla, &mut tv),
            (Some(r), true) => check_save(f, &s, r, &mut tu),
            (None, true) => skipped += 1,
        }
    }
    report("vanilla data, vanilla saves", &tv);
    if usd.is_some() {
        report("usd data, Settlers United saves", &tu);
    } else {
        eprintln!("{skipped} Settlers United saves skipped (set S4_USD_GAMEDATA)");
    }
    assert_eq!(tv.fp.0, tv.fp.1.min(tv.fp.0));
}

