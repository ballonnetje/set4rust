//! The user's playtest saves (`%APPDATA%\s4rs\saves`, `S4RS_SAVE_DIR`), loaded with the sim only
//! (no script) and ticked; each test shows one reported bug. Skipped without the save. One file
//! per save / mission: a new topic goes into a new file.

mod afterwin;
mod towerbug;
mod viking01;

use s4_sim::World;

/// The save `name` from the save folder, sim only.
pub(super) fn load(name: &str) -> Option<World> {
    let path = crate::save::dir()?.join(name);
    if !path.exists() {
        return None;
    }
    Some(crate::save::read(&path, s4_sim::Rules::load_default().ok()?).ok()?.world)
}

/// The user's saves of version 29 (before `GameSetup::out`) load through the migration
/// (`save::read`: the body in its own layout, the world hash checked) and run on.
#[test]
fn version_29_saves_load_and_run() {
    for name in ["towerbug.s4rs", "afterwin.s4rs"] {
        let Some(path) = crate::save::dir().map(|d| d.join(name)).filter(|p| p.exists()) else { continue };
        let h = crate::save::read_header(&path).expect("header");
        if h.version != 29 {
            continue;
        }
        let mut w = crate::save::read(&path, s4_sim::Rules::load_default().unwrap()).unwrap_or_else(|e| panic!("{name}: {e:#}")).world;
        assert_eq!(w.setup.out, 0);
        for _ in 0..200 {
            s4_sim::tick(&mut w, &[]);
        }
        eprintln!("{name}: version 29 loaded and ran to tick {}", w.tick);
    }
}

/// Late-game tick cost on the user's saves (sim only): `S4RS_BENCH_TICKS` ticks (2000) timed,
/// the state hashes every 500 ticks, and with `S4RS_BENCH_PROFILE` where the time went.
/// `cargo test --release -p s4-game --bin s4-game playtest::tick_bench -- --ignored --nocapture`
#[test]
#[ignore]
fn tick_bench() {
    let ticks = std::env::var("S4RS_BENCH_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
    let profile = std::env::var_os("S4RS_BENCH_PROFILE").is_some();
    for name in ["towerbug.s4rs", "afterwin.s4rs"] {
        let Some(mut w) = load(name) else { continue };
        let start = w.tick;
        let b = s4_sim::debug::bench(&mut w, ticks, 500, profile);
        println!("{name}: ticks {}..{} ({} entities): {:.3} ms/tick, slowest {:.2} ms at {}", start + 1, w.tick, w.entities.iter().count(), b.wall.as_secs_f64() * 1000.0 / ticks as f64, b.worst.1.as_secs_f64() * 1000.0, b.worst.0);
        for ((k, h), d) in b.hashes.iter().zip(&b.blocks) {
            println!("hash {k} {h:016x} {}", d.as_micros());
        }
        if let Some(p) = b.profile {
            print!("{p}");
        }
    }
}
