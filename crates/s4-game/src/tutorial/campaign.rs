//! Campaign missions with their real scripts and computer players, as the briefing starts them
//! (`frontend::setup::campaign_setup`, difficulty 0): every first mission runs a while without a
//! script warning and the same way twice; every mission can be run for longer (ignored); and a
//! mission can be watched with player 1 a computer player too (ignored, exploration).

use crate::sim::{Clock, SimHandle, SimSetup};
use s4_sim::entity::ObjType;

/// The military buildings `DefaultPlayersLostCheck` counts (`World::has_military_presence`).
const MILITARY: [i32; 5] = [46, 47, 48, 50, 51];

/// Soldiers (swordsmen, bowmen, the races' third kind, levels 1..3).
fn is_soldier(kind: u16) -> bool {
    matches!(kind, 29..=34 | 38..=43)
}

/// The mission with player 1 a computer player too (control 3; `CAMP_P1_AI=0` leaves it idle),
/// reported every 3000 ticks: buildings, military buildings, soldiers, carriers, settlers and residences
/// (0x28..0x2a) per player. `CAMP_MAP` (under `Map/`, default `Campaign/roman01`), `CAMP_TICKS`
/// (150 000).
#[test]
#[ignore = "exploration"]
fn campaign_ai_vs_ai() {
    let ticks: u32 = std::env::var("CAMP_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(150_000);
    let map = std::env::var("CAMP_MAP").unwrap_or_else(|_| "Campaign/roman01".into());
    let map_path = s4_viewer::scene::resolve_map(&map);
    if !map_path.exists() {
        return;
    }
    let mut players = crate::frontend::setup::campaign_setup(&map_path, 0).unwrap();
    let me = players.local.max(1);
    if std::env::var("CAMP_P1_AI").map_or(true, |v| v != "0") {
        players.control[me as usize - 1] = 3;
    }
    let setup = SimSetup { map_path, prefer_file_script: true, run_script: true, fog: false, outcome: Default::default(), tutorial: Default::default(), net: None, players: Some(players), load: None };
    let sim = SimHandle::spawn(setup, Clock::Manual).unwrap();
    loop {
        let t = sim.step(3000);
        let (tx, rx) = std::sync::mpsc::channel();
        sim.with_world(move |w| {
            let r = (1..=8)
                .filter(|&p| w.is_used_player(p))
                .map(|p| {
                    let b: i32 = (1..83).map(|k| w.buildings_amount(p, k, 2)).sum();
                    let mil: i32 = MILITARY.iter().map(|&k| w.buildings_amount(p, k, 2)).sum();
                    let sol = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == p as u8 && is_soldier(e.kind)).count();
                    let car = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == p as u8 && e.kind == 1).count();
                    let res: Vec<i32> = (0x28..=0x2a).map(|k| w.buildings_amount(p, k, 2)).collect();
                    let all = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == p as u8).count();
                    format!("p{p}: buildings {b} military {mil} soldiers {sol} carriers {car} settlers {all} residences {res:?}")
                })
                .collect::<Vec<_>>()
                .join(" | ");
            let _ = tx.send(r);
        });
        let o = sim.outcome.load(std::sync::atomic::Ordering::Relaxed);
        eprintln!("{map} tick {t}: {}, outcome {o}", rx.recv().unwrap());
        if o != 0 || t >= ticks {
            break;
        }
    }
}

/// A mission's computer players up close (`CAMP_MAP`, `CAMP_PLAYERS` e.g. `2,3`, `CAMP_TICKS`,
/// `CAMP_P1_AI=0` as above): every 3000 ticks the settlers by type and the buildings by kind
/// (finished / all); at `CAMP_DUMP` the carriers by (job list, task) and the eco engines' open
/// entries.
#[test]
#[ignore = "exploration"]
fn campaign_ai_dump() {
    let env = |k: &str| std::env::var(k).ok();
    let ticks: u32 = env("CAMP_TICKS").and_then(|v| v.parse().ok()).unwrap_or(60_000);
    let dump: u32 = env("CAMP_DUMP").and_then(|v| v.parse().ok()).unwrap_or(45_000);
    let players: Vec<u8> = env("CAMP_PLAYERS").unwrap_or_else(|| "2".into()).split(',').filter_map(|v| v.parse().ok()).collect();
    let map_path = s4_viewer::scene::resolve_map(&env("CAMP_MAP").unwrap_or_else(|| "Campaign/roman01".into()));
    if !map_path.exists() {
        return;
    }
    let mut setup_players = crate::frontend::setup::campaign_setup(&map_path, 0).unwrap();
    let local = setup_players.local.max(1);
    if env("CAMP_P1_AI").is_none_or(|v| v != "0") {
        setup_players.control[local as usize - 1] = 3;
    }
    let setup = SimSetup { map_path: map_path.clone(), prefer_file_script: true, run_script: true, fog: false, outcome: Default::default(), tutorial: Default::default(), net: None, players: Some(setup_players), load: None };
    let (mut world, mut script) = crate::sim::build_world(&setup, &setup.map_path, local).unwrap();
    for t in 1..=ticks {
        match script.as_mut() {
            Some(s) => s4_sim::tick_with_script(&mut world, &[], s),
            None => s4_sim::tick(&mut world, &[]),
        }
        let w = &world;
        for &p in &players {
            if t % 3000 == 0 {
                let kinds: Vec<String> = (1..83)
                    .filter_map(|k| {
                        let all = w.buildings_amount(p as i32, k, 2);
                        (all > 0).then(|| format!("{k}:{}/{all}", w.buildings_amount(p as i32, k, 1)))
                    })
                    .collect();
                let mut settlers = std::collections::BTreeMap::new();
                for (_, e) in w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == p) {
                    *settlers.entry(e.kind).or_insert(0) += 1;
                }
                eprintln!("tick {t} p{p}: settlers {settlers:?}
  buildings {kinds:?}");
            }
            if t == dump {
                let mut tasks = std::collections::BTreeMap::new();
                for (_, e) in w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == p && e.kind == 1) {
                    if let s4_sim::entity::EntityData::Settler(st) = &e.data {
                        *tasks.entry((st.job.list_id, st.job.task)).or_insert(0) += 1;
                    }
                }
                eprintln!("p{p} carriers (list, task): {tasks:?}");
                // Nodes of the player's land by resource type (0x60 stone, 0x70 trees, ...).
                let mut res = std::collections::BTreeMap::new();
                for n in 0..w.layers.resource.len() {
                    if w.owner_of(n) == p && w.layers.resource[n] != 0 {
                        *res.entry(w.layers.resource_type(n)).or_insert(0) += 1;
                    }
                }
                eprintln!("p{p} own nodes by resource type: {res:?}");
                if let Some(a) = s4_sim::ai::player::get(w, p) {
                    for sec in &a.body.sectors {
                        if let s4_sim::ai::model::SectorKind::Normal { eco, .. } = &sec.kind {
                            for e in eco {
                                let en = &e.engine;
                                eprintln!("p{p} eco {}: board lock {} waiting {} focus {} steps {}", e.eco, en.board.lock, en.board.waiting, en.s.focus, en.s.steps);
                                let all = env("CAMP_ALL").is_some();
                                for x in en.s.list.iter().filter(|x| all || !matches!(x.kind, 2 | 5 | 9)) {
                                    eprintln!("  entry id {} kind {} state {} link {} go_on {} {:?}", x.id, x.kind, x.state, x.link, x.go_on, x.d);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Counts the script host's warnings (Lua errors in `new_game`, a tick or an event).
#[derive(Clone, Default)]
struct ScriptWarnings(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for ScriptWarnings {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let m = event.metadata();
        if *m.level() == tracing::Level::WARN && m.target().starts_with("s4_game::script") {
            struct Msg(String);
            impl tracing::field::Visit for Msg {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    if f.name() == "message" {
                        self.0 = format!("{v:?}");
                    }
                }
            }
            let mut msg = Msg(String::new());
            event.record(&mut msg);
            self.0.lock().unwrap().push(msg.0);
        }
    }
}

/// `ticks` of a campaign map with its script and its computer players, headless: the final
/// state hash, the script's warnings and the sim's unported-code events.
fn run_mission(map: &std::path::Path, ticks: u32) -> (u64, Vec<String>, Vec<&'static str>) {
    use tracing_subscriber::layer::SubscriberExt;
    let players = crate::frontend::setup::campaign_setup(map, 0);
    let local = players.map_or(1, |p| p.local.max(1));
    let setup = SimSetup { map_path: map.to_path_buf(), prefer_file_script: true, run_script: true, fog: false, outcome: Default::default(), tutorial: Default::default(), net: None, players, load: None };
    let warnings = ScriptWarnings::default();
    let subscriber = tracing_subscriber::registry().with(warnings.clone());
    let (hash, unported) = tracing::subscriber::with_default(subscriber, || {
        let (mut world, mut script) = crate::sim::build_world(&setup, &setup.map_path, local).unwrap();
        let mut unported = Vec::new();
        for _ in 0..ticks {
            match script.as_mut() {
                Some(s) => s4_sim::tick_with_script(&mut world, &[], s),
                None => s4_sim::tick(&mut world, &[]),
            }
            for e in &world.events {
                if let s4_sim::event::SimEvent::Unported { what, .. } = e
                    && !unported.contains(what)
                {
                    unported.push(*what);
                }
            }
        }
        (world.state_hash(), unported)
    });
    let w = warnings.0.lock().unwrap().clone();
    (hash, w, unported)
}

fn missions() -> Vec<std::path::PathBuf> {
    maps("Campaign")
}

/// The maps in `Map/<dir>`.
fn maps(dir: &str) -> Vec<std::path::PathBuf> {
    let dir = s4_formats::game_dir().join("Map").join(dir);
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map"))).collect();
    v.sort();
    v
}

/// The first mission of each campaign: 600 ticks with its script and computer players, no
/// script warning, and the same state twice.
#[test]
fn campaign_missions_start() {
    let first = ["roman01", "viking01", "maya01", "dark01", "MCD2_roman1", "MD_roman1", "AO_roman1", "AO_trojan01", "XMD3_roman1", "MD_conflict1"];
    for m in missions().into_iter().filter(|p| p.file_stem().is_some_and(|s| first.iter().any(|f| s.eq_ignore_ascii_case(f)))) {
        let t = std::time::Instant::now();
        let (a, wa, ua) = run_mission(&m, 600);
        assert!(wa.is_empty(), "{}: script warnings {wa:?}", m.display());
        let (b, ..) = run_mission(&m, 600);
        assert_eq!(a, b, "{}: two runs differ", m.display());
        eprintln!("{}: {:.1} s, unported {ua:?}", m.display(), t.elapsed().as_secs_f32());
    }
}

/// Every campaign map, 3000 ticks: no script warning (`CAMP_TICKS` for longer).
#[test]
#[ignore = "about 10 minutes"]
fn campaign_missions_run() {
    let ticks = std::env::var("CAMP_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let mut bad = Vec::new();
    for m in missions() {
        let t = std::time::Instant::now();
        let (_, w, u) = run_mission(&m, ticks);
        eprintln!("{}: {} warnings, {:.1} s, unported {u:?}", m.display(), w.len(), t.elapsed().as_secs_f32());
        if !w.is_empty() {
            bad.push(format!("{}: {:?}", m.display(), &w[..w.len().min(3)]));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Every single-player and multiplayer map with its script (if it has one) and every free or
/// computer slot a computer, 3000 ticks: no script warning (`CAMP_TICKS` for longer).
#[test]
#[ignore = "about 2 minutes"]
fn other_maps_run() {
    let ticks = std::env::var("CAMP_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let mut bad = Vec::new();
    for m in maps("Singleplayer").into_iter().chain(maps("Multiplayer")) {
        let t = std::time::Instant::now();
        let (_, w, u) = run_mission(&m, ticks);
        eprintln!("{}: {} warnings, {:.1} s, unported {u:?}", m.display(), w.len(), t.elapsed().as_secs_f32());
        if !w.is_empty() {
            bad.push(format!("{}: {:?}", m.display(), &w[..w.len().min(3)]));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
