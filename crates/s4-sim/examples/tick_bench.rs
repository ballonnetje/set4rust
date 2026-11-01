//! Late-game tick cost of a map: `new_game` (every player a computer with `--ai`), run to
//! `--from` ticks, then [`s4_sim::debug::bench`] over `--ticks` ticks: ms a tick, the slowest,
//! the state hashes every 500 ticks with the block's wall time in µs and its thread cycles in
//! millions (`QueryThreadCycleTime`: not counting the time the thread waits for a CPU, so steadier
//! on a busy machine) — to compare before / after a change the hashes must match — and with
//! `--profile` where the time went; with `--paths` instead the path searches a tick (through the
//! debug channel); with `--hash` the cost of `state_hash` / `subsystem_hashes` and of each part.
//! `cargo run --release -p s4-sim --example tick_bench -- [--map NAME] [--from 20000] [--ticks 2000] [--ai] [--profile | --paths | --hash]`
use s4_formats::map::MapFile;
use std::time::Instant;

#[cfg(windows)]
fn cycles() -> u64 {
    unsafe extern "system" {
        fn GetCurrentThread() -> isize;
        fn QueryThreadCycleTime(thread: isize, cycles: *mut u64) -> i32;
    }
    let mut c = 0u64;
    // SAFETY: the pseudo handle of the current thread and a valid out pointer.
    unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut c) };
    c
}

#[cfg(not(windows))]
fn cycles() -> u64 {
    0
}

/// `ticks` ticks in blocks of 500: one line per block (tick, hash, µs, M cycles); total wall.
fn blocks(w: &mut s4_sim::World, ticks: u32) -> std::time::Duration {
    let mut wall = std::time::Duration::ZERO;
    let mut left = ticks;
    while left > 0 {
        let n = left.min(500);
        let c = cycles();
        let b = s4_sim::debug::bench(w, n, n, false);
        let c = cycles() - c;
        wall += b.wall;
        for ((k, h), d) in b.hashes.iter().zip(&b.blocks) {
            println!("hash {k} {h:016x} {} {}", d.as_micros(), c / 1_000_000);
        }
        left -= n;
    }
    wall
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let flag = |k: &str| args.iter().any(|a| a == k);
    let name = arg("--map").unwrap_or_else(|| "UnitedNations".into());
    let from: u32 = arg("--from").map_or(20_000, |v| v.parse().expect("--from"));
    let ticks: u32 = arg("--ticks").map_or(2_000, |v| v.parse().expect("--ticks"));
    let root = s4_formats::game_dir().join("Map");
    let path = ["Multiplayer", "Singleplayer", "Campaign"].iter().map(|d| root.join(d).join(format!("{name}.map"))).find(|p| p.exists()).expect("map");
    let map = MapFile::open(&path).expect("map file");
    let mut ng = s4_sim::NewGame::default();
    if flag("--ai") {
        ng.control = [3; 8];
        ng.control[0] = 0;
    }
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default().expect("rules"), &ng).expect("new game");
    let t = Instant::now();
    blocks(&mut w, from);
    println!("{name}: to tick {} in {:.1} s ({} entities)", w.tick, t.elapsed().as_secs_f64(), w.entities.iter().count());
    if flag("--hash") {
        let best = |f: &dyn Fn() -> u64| (0..20).map(|_| {
            let t = Instant::now();
            std::hint::black_box(f());
            t.elapsed()
        }).min().unwrap_or_default();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        println!("state_hash {:016x}: {:.3} ms; subsystem_hashes: {:.3} ms (fastest of 20); grid {}", w.state_hash(), ms(best(&|| w.state_hash())), ms(best(&|| w.subsystem_hashes().len() as u64)), w.grid.size());
        println!("hashes (both in one pass): {:.3} ms", ms(best(&|| w.hashes().0)));
        for (name, d) in s4_sim::debug::hash_times(&w, 20) {
            if d.as_micros() >= 10 {
                println!("  {name:<14} {:.3} ms", ms(d));
            }
        }
        return;
    }
    if flag("--paths") {
        s4_sim::debug::enable(true);
        let mut sum = s4_sim::debug::PathCounts::default();
        for k in 0..ticks {
            s4_sim::tick(&mut w, &[]);
            w.events.clear();
            if k % 500 == 499 || k + 1 == ticks {
                for t in s4_sim::debug::take_ticks() {
                    let p = t.paths;
                    sum.coarse += p.coarse;
                    sum.coarse_failed += p.coarse_failed;
                    sum.fine += p.fine;
                    sum.fine_failed += p.fine_failed;
                    sum.fine_steps += p.fine_steps;
                }
            }
        }
        let per = |v: u32| v as f64 / ticks as f64;
        println!("a tick: {:.1} coarse searches ({:.1} failed), {:.1} fine ({:.1} failed, {:.1} steps)", per(sum.coarse), per(sum.coarse_failed), per(sum.fine), per(sum.fine_failed), per(sum.fine_steps));
        return;
    }
    let (wall, profile) = if flag("--profile") {
        // One profiled run over all the ticks (the probes cost time of their own).
        let b = s4_sim::debug::bench(&mut w, ticks, 500, true);
        (b.wall, b.profile)
    } else {
        (blocks(&mut w, ticks), None)
    };
    println!("ticks {}..{}: {:.3} ms/tick", w.tick - ticks + 1, w.tick, wall.as_secs_f64() * 1000.0 / ticks as f64);
    if let Some(p) = profile {
        print!("{p}");
    }
}
