//! The world build's steps timed (`s4_sim::debug` on): `load_times MAP [runs]`, MAP under
//! `Map/` (default `Multiplayer/UnitedNations.map`); every computer slot at control 3.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let map = args.get(1).cloned().unwrap_or("Multiplayer/UnitedNations.map".into());
    let runs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let m = s4_formats::map::MapFile::open(s4_formats::game_dir().join("Map").join(&map)).expect("map");
    let rules = s4_sim::Rules::load_default().unwrap();
    let mut setup = s4_sim::NewGame::default();
    setup.control.iter_mut().for_each(|c| *c = 3);
    for run in 0..runs {
        s4_sim::debug::enable(true);
        let t = std::time::Instant::now();
        let (w, _) = s4_sim::new_game(&m, rules.clone(), &setup).unwrap();
        let total = t.elapsed();
        let steps = s4_sim::debug::take_load_times();
        s4_sim::debug::enable(false);
        println!("run {run}: {total:?} (hash {:016x})", w.state_hash());
        for (s, d) in steps {
            println!("  {s:28} {d:?}");
        }
    }
}
