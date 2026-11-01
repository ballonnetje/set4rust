//! The eco-sector AI (docs/ECOAI.md) on MCD2_roman1 with player 2 a computer player: its one
//! eco-sector AI is made with the sector AI, plans and builds; the run is deterministic. The
//! release comparison is `tools/re/script/ecoai_replay.py` on the record of [`eco_ai_trace`].
//! Skipped when the game data is not present.

use s4_sim::World;

fn world() -> Option<World> {
    world_on("Campaign/MCD2_roman1.map", &[2], &[])
}

/// A new game on the map (under `Map/`) with `ai` computer players, their races `races` (the
/// map's where none is given).
fn world_on(map: &str, ai: &[u8], races: &[u8]) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join(map);
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let mut setup = s4_sim::NewGame::default();
    for (k, &p) in ai.iter().enumerate() {
        setup.control[p as usize - 1] = 3;
        setup.races[p as usize - 1] = races.get(k).copied();
    }
    // A map without placed settlers: the map settings' start goods (1..=3).
    setup.start_resources = std::env::var("S4RS_ECO_START").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    s4_sim::new_game(&map, rules, &setup).ok().map(|(w, _)| w)
}

/// The player's buildings and sites.
fn buildings(w: &World, player: u8) -> usize {
    (1..83).map(|t| w.building_list(player as i32, t).len()).sum()
}

fn eco_ais(w: &World, player: u8) -> usize {
    s4_sim::ai::player::get(w, player).map_or(0, |a| a.body.sectors.iter().map(|s| match &s.kind { s4_sim::ai::model::SectorKind::Normal { eco, .. } => eco.len(), _ => 0 }).sum())
}

#[test]
fn eco_ai_builds() {
    let Some(mut w) = world() else { return };
    assert_eq!(eco_ais(&w, 2), 1, "one eco-sector with carriers: one eco AI");
    assert_eq!(eco_ais(&w, 1), 0, "the human player has none");
    let start = buildings(&w, 2);
    for _ in 0..30000 {
        s4_sim::tick(&mut w, &[]);
    }
    let end = buildings(&w, 2);
    eprintln!("player 2: {start} buildings at the start, {end} after 30000 ticks");
    assert!(end >= start + 8, "the AI's economy grows ({start} -> {end})");
}

/// The monitor's view (`ai::eco::view::eco_view`): read only, the engine's list as it stands.
#[test]
fn eco_view_reads_the_engine() {
    let Some(mut w) = world() else { return };
    assert!(s4_sim::ai::eco::view::eco_view(&w, 1).is_none_or(|v| v.sectors.is_empty()));
    let mut seen_plan = false;
    let mut seen_planned = false;
    for t in 1..=6000 {
        s4_sim::tick(&mut w, &[]);
        if t % 250 == 0 {
            let hash = w.state_hash();
            let v = s4_sim::ai::eco::view::eco_view(&w, 2).expect("player 2 is a computer player");
            assert_eq!(w.state_hash(), hash);
            assert_eq!(v.sectors.len(), 1);
            let s = &v.sectors[0];
            let ids: Vec<i32> = s.entries.iter().map(|e| e.id).collect();
            let a = s4_sim::ai::player::get(&w, 2).unwrap();
            let eng = a.body.sectors.iter().find_map(|x| match &x.kind { s4_sim::ai::model::SectorKind::Normal { eco, .. } => eco.first(), _ => None }).unwrap();
            assert_eq!(ids, eng.engine.s.list.iter().map(|e| e.id).collect::<Vec<_>>());
            seen_plan |= s.entries.iter().any(|e| e.problem.as_ref().is_some_and(|p| p.plan.is_some()));
            seen_planned |= !s.planned.is_empty();
        }
    }
    assert!(seen_plan && seen_planned, "plans {seen_plan}, planned buildings {seen_planned}");
    if std::env::var_os("S4RS_ECO_VIEW").is_some() {
        eprintln!("{:#?}", s4_sim::ai::eco::view::eco_view(&w, 2));
    }
}

#[test]
fn eco_ai_is_deterministic() {
    let run = || {
        let mut w = world()?;
        for _ in 0..6000 {
            s4_sim::tick(&mut w, &[]);
        }
        Some(w.state_hash())
    };
    let Some(a) = run() else { return };
    assert_eq!(run(), Some(a));
}

/// The manager calls of the first ticks for the replay harness: `S4RS_ECO_TRACE=file`
/// (`S4RS_ECO_TICKS`, 3000 by default; `S4RS_ECO_MAP` a map under `Map/` and `S4RS_ECO_AI` its
/// computer players, e.g. `2,3,4`, MCD2_roman1 and player 2 by default; `S4RS_ECO_RACES` their
/// races in that order; `S4RS_ECO_START` the start goods of a map without placed settlers).
/// `S4RS_ECO_FUZZ=seed:permille:from[:slots[:until]]` changes that many of the manager's scalar
/// answers from tick `from` (0: also those of the engines made with the map) to `until` (to 0,
/// ±1, twice the answer or 100 more); the replay then checks the release engine on paths the
/// world does not reach. `slots` (hex, `/`-separated, e.g. `90/11c/120`) limits it to those
/// manager slots.
#[test]
fn eco_ai_trace() {
    let Some(path) = std::env::var_os("S4RS_ECO_TRACE") else { return };
    s4_sim::ai::eco::trace::start();
    let map = std::env::var("S4RS_ECO_MAP").unwrap_or_else(|_| "Campaign/MCD2_roman1.map".into());
    let list = |name: &str, default: &str| -> Vec<u8> { std::env::var(name).unwrap_or_else(|_| default.into()).split(',').filter_map(|v| v.trim().parse().ok()).collect() };
    let ai: Vec<u8> = list("S4RS_ECO_AI", "2").into_iter().filter(|p| (1..=8).contains(p)).collect();
    let ticks: u32 = std::env::var("S4RS_ECO_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let spec = std::env::var("S4RS_ECO_FUZZ").unwrap_or_default();
    let fuzz: Vec<u64> = spec.split(':').take(3).filter_map(|v| v.parse().ok()).collect();
    let slots: Vec<i32> = spec.split(':').nth(3).unwrap_or("").split('/').filter_map(|v| i32::from_str_radix(v, 16).ok()).collect();
    let until: u64 = spec.split(':').nth(4).and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
    let from = fuzz.get(2).copied().unwrap_or(0);
    // On from the game's start (the engines made with the map) when `from` is 0.
    let on = std::rc::Rc::new(std::cell::Cell::new(fuzz.len() >= 2 && from == 0));
    // `S4RS_ECO_SET=rule;rule…`, each `slot[@arg]=v` (the answer v), `slot[@arg]+d` (d more) or
    // `slot[@arg]*k`, optionally `,from,until` in ticks (0: also the engines made with the map):
    // answers crafted for a path, applied before the fuzz.
    let rules: Vec<(i32, Option<i32>, char, i32, u64, u64)> = std::env::var("S4RS_ECO_SET")
        .unwrap_or_default()
        .split(';')
        .filter_map(|r| {
            let mut parts = r.split(',');
            let head = parts.next()?;
            let at = head.find(['=', '+', '*'])?;
            let (target, op, val) = (&head[..at], head.as_bytes()[at] as char, head[at + 1..].parse().ok()?);
            let (slot, arg) = match target.split_once('@') {
                Some((s, a)) => (i32::from_str_radix(s, 16).ok()?, Some(i32::from_str_radix(a, 16).ok()?)),
                None => (i32::from_str_radix(target, 16).ok()?, None),
            };
            let from = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            let until = parts.next().and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
            Some((slot, arg, op, val, from, until))
        })
        .collect();
    let now = std::rc::Rc::new(std::cell::Cell::new(0u64));
    if !fuzz.is_empty() || !rules.is_empty() {
        let (on, now, mut x) = (on.clone(), now.clone(), fuzz.first().copied().unwrap_or(1) | 1);
        let permille = fuzz.get(1).copied().unwrap_or(0);
        s4_sim::ai::eco::trace::perturb(Some(Box::new(move |slot, args, v| {
            let t = now.get();
            let v = rules.iter().filter(|r| r.0 == slot && r.1.is_none_or(|a| args.first() == Some(&a)) && (r.4..r.5).contains(&t)).fold(v, |v, r| match r.2 {
                '=' => r.3,
                '+' => v.wrapping_add(r.3),
                _ => v.wrapping_mul(r.3),
            });
            // xorshift64
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            // 0x7c, a type's working radius, is data: below 2 the release's planting-area
            // search (0x652290) never ends.
            if !on.get() || x % 1000 >= permille || slot == 0x7c || !(slots.is_empty() || slots.contains(&slot)) {
                return v;
            }
            match (x >> 32) % 5 {
                0 => 0,
                1 => v.wrapping_add(1),
                2 => v.wrapping_sub(1),
                3 => v.wrapping_mul(2),
                _ => v.wrapping_add(100),
            }
        })));
    }
    let Some(mut w) = world_on(&map, &ai, &list("S4RS_ECO_RACES", "")) else { return };
    for t in 0..ticks {
        now.set(t as u64);
        on.set(fuzz.len() >= 2 && (from..until).contains(&(t as u64)));
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::ai::eco::trace::perturb(None);
    std::fs::write(path, s4_sim::ai::eco::trace::take().unwrap()).unwrap();
}
