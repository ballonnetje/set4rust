//! Determinism and performance soak over the shipped maps (sim only: no map script).
//!
//! Every map of `Map\Campaign`, `Map\Multiplayer` and `Map\Singleplayer` is set up twice with
//! `new_game` (two separate worlds, not a clone) and both are ticked in lockstep for `--ticks` ticks with the
//! same commands; the state hashes are compared every `--every` ticks. On a mismatch the
//! subsystems that differ are named and the map stops there. Each world's tick time is measured
//! on its own.
//!
//! ```text
//! cargo run --release -p s4-sim --example sim_soak -- [--ticks 20000] [--every 500]
//!     [--ai] [--fuzz] [--only SUBSTR] [--jobs N] [--profile]
//! ```
//! * `--ai`: every player is a computer player (control 3, `ai::add_players_at_start`).
//! * `--fuzz`: random player commands from a fixed-seed generator (built from world A's state,
//!   given to both): builds near own buildings, building commands on own buildings, soldiers
//!   sent at enemy buildings, spells, trading, vehicles, orders — to drive the subsystems an
//!   idle game leaves alone.
//! * `--jobs N` runs N maps at once (faster, but the timings then share the CPU).
//! * `--watch SECS`: one world per map, ticked through `tick_probed`; an update that runs SECS
//!   seconds is reported (tick, entity type, kind and id) and ends the process. With `--check`
//!   the eco-sectors' offer lists are checked after every tick (`eco::check_offer_lists`).
//! * `--profile`: one world per map, ticked through `tick_probed`; prints where the time went
//!   (tick steps, entity updates by object type and kind). The clock reads add about 20–40 ns
//!   per entity update.
//!
//! Output: one line per map, then the maps sorted by mean ms/tick, then world A's
//! `SimEvent::Unported` calls by kind (on how many maps, how many calls). The events are drained
//! every tick (the client's job; left in the world they pile up).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use s4_formats::map::MapFile;
use s4_sim::{Command, NewGame, NoScript, ObjType, PlayerCommand, Probe, Rules, TickProbe, World, new_game, tick, tick_probed};

struct Opts {
    ticks: u32,
    every: u32,
    ai: bool,
    fuzz: bool,
    only: Option<String>,
    jobs: usize,
    profile: bool,
    watch: u64,
    check: bool,
}

/// `--watch SECS`: the last probe and tick, shared with a watchdog thread that reports an update
/// still running after SECS seconds (a hang) and ends the process.
struct Watch<'a> {
    now: &'a Mutex<(u32, Option<Probe>, Instant)>,
    tick: u32,
}

impl TickProbe for Watch<'_> {
    fn at(&mut self, what: Probe) {
        *self.now.lock().unwrap() = (self.tick, Some(what), Instant::now());
    }
}

fn watch(path: &PathBuf, rules: &Rules, o: &Opts) {
    let Ok(m) = MapFile::open(path) else { return };
    let Ok(mut a) = setup(&m, rules, o.ai) else { return };
    let now = Mutex::new((0u32, None, Instant::now()));
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                let (t, p, at) = *now.lock().unwrap();
                if at.elapsed().as_secs() >= o.watch {
                    println!("{}: HANG at tick {t}, in {p:?} for {:?}", path.display(), at.elapsed());
                    std::process::exit(2);
                }
            }
        });
        let mut fuzz = Fuzz { rng: 0x9e37_79b9 };
        for t in 1..=o.ticks {
            let cmds = if o.fuzz { fuzz.commands(&a) } else { Vec::new() };
            let mut w = Watch { now: &now, tick: t };
            tick_probed(&mut a, &cmds, &mut NoScript, &mut w);
            a.events.clear();
            if o.check
                && let Err(e) = s4_sim::eco::check_offer_lists(&a)
            {
                println!("{}: tick {t}: {e}; commands this tick {cmds:?}", path.display());
                std::process::exit(3);
            }
        }
        done.store(true, Ordering::Relaxed);
    });
    println!("{}: {} ticks, no hang", path.display(), o.ticks);
}

/// `--profile`: time per [`Probe`] (the time until the next call is charged to the last one).
#[derive(Default)]
struct Profiler {
    last: Option<(Probe, Instant)>,
    spent: HashMap<Probe, (Duration, u64)>,
}

impl TickProbe for Profiler {
    fn at(&mut self, what: Probe) {
        let now = Instant::now();
        // Per type and kind, not per entity.
        let what = match what {
            Probe::Entity(t, k, _) => Probe::Entity(t, k, 0),
            Probe::Site(k, _) => Probe::Site(k, 0),
            p => p,
        };
        if let Some((p, t)) = self.last.take() {
            let e = self.spent.entry(p).or_default();
            e.0 += now - t;
            e.1 += 1;
        }
        self.last = (!matches!(what, Probe::Phase("end"))).then_some((what, now));
    }
}

fn profile(path: &PathBuf, rules: &Rules, o: &Opts) {
    let Ok(m) = MapFile::open(path) else { return };
    let Ok(mut a) = setup(&m, rules, o.ai) else { return };
    let mut prof = Profiler::default();
    let mut fuzz = Fuzz { rng: 0x9e37_79b9 };
    let start = Instant::now();
    for _ in 0..o.ticks {
        let cmds = if o.fuzz { fuzz.commands(&a) } else { Vec::new() };
        tick_probed(&mut a, &cmds, &mut NoScript, &mut prof);
        a.events.clear();
    }
    let wall = start.elapsed();
    // Entities grouped by type and kind; steps by name.
    let mut rows: Vec<(String, Duration, u64)> = prof
        .spent
        .iter()
        .map(|(p, &(d, n))| {
            let name = match p {
                Probe::Phase(s) => format!("step {s}"),
                Probe::Entity(t, k, _) => format!("{t:?} kind {k}"),
                Probe::Site(k, _) => format!("Site kind {k}"),
            };
            (name, d, n)
        })
        .collect();
    let mut by_type: HashMap<String, (Duration, u64)> = HashMap::new();
    for (p, &(d, n)) in &prof.spent {
        let k = match p {
            Probe::Phase(_) => continue,
            Probe::Entity(t, _, _) => format!("{t:?}"),
            Probe::Site(..) => "Site".into(),
        };
        let e = by_type.entry(k).or_default();
        e.0 += d;
        e.1 += n;
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!("{}: {} ticks, {:.3} ms/tick (with probes)", path.display(), o.ticks, ms(wall) / o.ticks as f64);
    let mut types: Vec<_> = by_type.into_iter().collect();
    types.sort_by_key(|t| std::cmp::Reverse(t.1.0));
    for (k, (d, n)) in types {
        println!("  {:<24} {:>9.4} ms/tick {:>6.1} %  {:>10} updates  {:>7.2} us each", k, ms(d) / o.ticks as f64, 100.0 * d.as_secs_f64() / wall.as_secs_f64(), n, d.as_secs_f64() * 1e6 / n.max(1) as f64);
    }
    println!("  -- top 30 --");
    for (name, d, n) in rows.iter().take(30) {
        println!("  {:<32} {:>9.4} ms/tick {:>6.1} %  {:>10} calls  {:>7.2} us each", name, ms(*d) / o.ticks as f64, 100.0 * d.as_secs_f64() / wall.as_secs_f64(), n, d.as_secs_f64() * 1e6 / (*n).max(1) as f64);
    }
}

#[derive(Default)]
struct Report {
    name: String,
    entities: usize,
    commands: u32,
    /// Mean ms per tick of one world, and the worst block of `every` ticks.
    mean_ms: f64,
    worst_block_ms: f64,
    worst_block_at: u32,
    /// The first checked tick whose hashes differ, with the subsystems that differ.
    diverged: Option<(u32, Vec<&'static str>)>,
    error: Option<String>,
    /// World A's `SimEvent::Unported` calls by what (the events are drained every tick).
    unported: HashMap<&'static str, u32>,
}

fn maps(only: &Option<String>) -> Vec<PathBuf> {
    let root = s4_formats::game_dir().join("Map");
    let mut v = Vec::new();
    for dir in ["Campaign", "Multiplayer", "Singleplayer"] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map")) {
                v.push(p);
            }
        }
    }
    v.sort();
    if let Some(s) = only {
        let s = s.to_ascii_lowercase();
        v.retain(|p| p.to_string_lossy().to_ascii_lowercase().contains(&s));
    }
    v
}

fn setup(map: &MapFile, rules: &Rules, ai: bool) -> Result<World, String> {
    let mut ng = NewGame::default();
    if ai {
        ng.control = [3; 8];
        ng.control[0] = 0;
    }
    new_game(map, rules.clone(), &ng).map(|(w, _)| w).map_err(|e| e.to_string())
}

fn soak(path: &PathBuf, rules: &Rules, o: &Opts) -> Report {
    let dir = path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut r = Report { name: format!("{dir}/{stem}"), ..Default::default() };
    let worlds = MapFile::open(path).map_err(|e| e.to_string()).and_then(|m| Ok((setup(&m, rules, o.ai)?, setup(&m, rules, o.ai)?)));
    let (mut a, mut b) = match worlds {
        Ok(w) => w,
        Err(e) => {
            r.error = Some(e);
            return r;
        }
    };
    if a.state_hash() != b.state_hash() {
        r.diverged = Some((0, differing(&a, &b)));
        return r;
    }
    let (mut total, mut block) = (Duration::ZERO, Duration::ZERO);
    let mut run = 0;
    let mut fuzz = Fuzz { rng: 0x9e37_79b9 ^ r.name.bytes().fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32)) };
    for t in 1..=o.ticks {
        let cmds = if o.fuzz { fuzz.commands(&a) } else { Vec::new() };
        r.commands += cmds.len() as u32;
        let s = Instant::now();
        tick(&mut a, &cmds);
        let ta = s.elapsed();
        let s = Instant::now();
        tick(&mut b, &cmds);
        let tb = s.elapsed();
        for e in a.events.drain(..) {
            if let s4_sim::event::SimEvent::Unported { what, .. } = e {
                *r.unported.entry(what).or_default() += 1;
            }
        }
        b.events.clear();
        total += ta + tb;
        block += ta + tb;
        run = t;
        if t % o.every == 0 || t == o.ticks {
            let n = if t % o.every == 0 { o.every } else { t % o.every };
            let ms = block.as_secs_f64() * 1000.0 / (2.0 * n as f64);
            if ms > r.worst_block_ms {
                r.worst_block_ms = ms;
                r.worst_block_at = t;
            }
            block = Duration::ZERO;
            if a.state_hash() != b.state_hash() {
                r.diverged = Some((t, differing(&a, &b)));
                break;
            }
        }
    }
    r.entities = a.entities.iter().filter(|(_, e)| !e.is_dead()).count();
    r.mean_ms = total.as_secs_f64() * 1000.0 / (2.0 * run.max(1) as f64);
    r
}

fn differing(a: &World, b: &World) -> Vec<&'static str> {
    a.subsystem_hashes().into_iter().zip(b.subsystem_hashes()).filter(|(x, y)| x.1 != y.1).map(|(x, _)| x.0).collect()
}

fn main() {
    let mut o = Opts { ticks: 20_000, every: 500, ai: false, fuzz: false, only: None, jobs: 1, profile: false, watch: 0, check: false };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().expect("value");
        match a.as_str() {
            "--ticks" => o.ticks = val().parse().expect("ticks"),
            "--every" => o.every = val().parse().expect("every"),
            "--ai" => o.ai = true,
            "--fuzz" => o.fuzz = true,
            "--only" => o.only = Some(val()),
            "--jobs" => o.jobs = val().parse().expect("jobs"),
            "--profile" => o.profile = true,
            "--watch" => o.watch = val().parse().expect("watch seconds"),
            "--check" => o.check = true,
            other => panic!("unknown argument {other}"),
        }
    }
    let rules = Rules::load_default().expect("rules");
    let list = maps(&o.only);
    if o.profile || o.watch > 0 {
        for p in &list {
            if o.watch > 0 {
                let o = &o;
                std::thread::scope(|s| std::thread::Builder::new().stack_size(64 << 20).spawn_scoped(s, || watch(p, &rules, o)).unwrap().join().ok());
                continue;
            }
            let o = &o;
            std::thread::scope(|s| std::thread::Builder::new().stack_size(64 << 20).spawn_scoped(s, || profile(p, &rules, o)).unwrap().join().ok());
        }
        return;
    }
    eprintln!("{} maps, {} ticks, hashes every {}, ai {}, fuzz {}, jobs {}", list.len(), o.ticks, o.every, o.ai, o.fuzz, o.jobs);
    let next = AtomicUsize::new(0);
    let reports = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..o.jobs.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(p) = list.get(i) else { break };
                    // A large stack, as the game's sim thread has.
                    let r = std::thread::scope(|s2| {
                        std::thread::Builder::new().stack_size(64 << 20).spawn_scoped(s2, || soak(p, &rules, &o)).unwrap().join()
                    });
                    let r = r.unwrap_or_else(|_| Report { name: p.display().to_string(), error: Some("panicked".into()), ..Default::default() });
                    println!("{}", line(&r));
                    reports.lock().unwrap().push(r);
                }
            });
        }
    });
    let mut reports = reports.into_inner().unwrap();
    reports.sort_by(|a, b| b.mean_ms.total_cmp(&a.mean_ms));
    println!("\n== by mean ms/tick ==");
    for r in &reports {
        println!("{}", line(r));
    }
    // Unported calls: total and on how many maps.
    let mut un: HashMap<&'static str, (u64, u32)> = HashMap::new();
    for r in &reports {
        for (&w, &n) in &r.unported {
            let e = un.entry(w).or_default();
            e.0 += n as u64;
            e.1 += 1;
        }
    }
    let mut un: Vec<_> = un.into_iter().collect();
    un.sort_by(|a, b| b.1.1.cmp(&a.1.1).then(b.1.0.cmp(&a.1.0)));
    println!("
== unported calls (maps, calls) ==");
    for (w, (n, maps)) in un {
        println!("{maps:>4} {n:>9}  {w}");
    }
    let bad: Vec<&Report> = reports.iter().filter(|r| r.diverged.is_some() || r.error.is_some()).collect();
    println!("\n{} maps, {} diverged or failed", reports.len(), bad.len());
    for r in bad {
        println!("  {}", line(r));
    }
}

fn line(r: &Report) -> String {
    let state = match (&r.error, &r.diverged) {
        (Some(e), _) => format!("ERROR {e}"),
        (None, Some((t, parts))) => format!("DIVERGED at tick {t}: {parts:?}"),
        (None, None) => "same".into(),
    };
    format!(
        "{:<32} {:>8.3} ms/tick mean {:>8.3} worst block (to tick {:>5}) {:>6} entities {:>6} commands  {state}",
        r.name, r.mean_ms, r.worst_block_ms, r.worst_block_at, r.entities, r.commands
    )
}

/// The command generator of `--fuzz` (xorshift32; a command on about one tick in four).
struct Fuzz {
    rng: u32,
}

impl Fuzz {
    fn next(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { self.next() as usize % n }
    }

    fn pick<T: Copy>(&mut self, v: &[T]) -> Option<T> {
        if v.is_empty() { None } else { Some(v[self.below(v.len())]) }
    }

    fn near(&mut self, size: i32, x: i32, y: i32, r: i32) -> (u16, u16) {
        let dx = self.below(2 * r as usize + 1) as i32 - r;
        let dy = self.below(2 * r as usize + 1) as i32 - r;
        ((x + dx).clamp(1, size - 2) as u16, (y + dy).clamp(1, size - 2) as u16)
    }

    fn commands(&mut self, w: &World) -> Vec<PlayerCommand> {
        if !self.next().is_multiple_of(4) {
            return Vec::new();
        }
        let players: Vec<u8> = (1..=8u8).filter(|&p| w.players.get(p as usize).is_some_and(|x| x.is_some())).collect();
        let Some(p) = self.pick(&players) else { return Vec::new() };
        let (mut own, mut enemy, mut soldiers, mut priests, mut specialists, mut vehicles) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for (id, e) in w.entities.iter() {
            if e.is_dead() {
                continue;
            }
            let (x, y) = (e.pos.x(), e.pos.y());
            match e.obj_type {
                ObjType::Building if e.owner == p => own.push((id, x, y)),
                ObjType::Building if e.owner != 0 => enemy.push((x, y)),
                ObjType::Settler if e.owner == p => match (e.kind, w.rules.warrior_type(e.kind)) {
                    (45, _) => priests.push(id),
                    (_, 7) => specialists.push(id),
                    (_, 0 | 1 | 6 | 14) => {}
                    _ => soldiers.push(id),
                },
                ObjType::Ship | ObjType::LandVehicle if e.owner == p => vehicles.push(id),
                _ => {}
            }
        }
        let size = w.grid.size() as i32;
        let Some((b, bx, by)) = self.pick(&own) else { return Vec::new() };
        let b32 = b as u32;
        let target = match self.pick(&enemy) {
            Some(t) => t,
            None => (self.below(size as usize) as i32, self.below(size as usize) as i32),
        };
        let good = 1 + self.below(43) as u16;
        let c = match self.below(26) {
            0..=7 => {
                let (x, y) = self.near(size, bx, by, 14);
                Command::Build { building: 1 + self.below(82) as u32, x, y }
            }
            8..=10 => {
                let n = (1 + self.below(12)).min(soldiers.len());
                let start = self.below(soldiers.len());
                let entities: Vec<u16> = soldiers.iter().cycle().skip(start).take(n).copied().collect();
                let (x, y) = self.near(size, target.0, target.1, 3);
                Command::SendTo { x, y, mode: self.below(3) as u32, entities }
            }
            11 => {
                let Some(priest) = self.pick(&priests) else { return Vec::new() };
                let (x, y) = if self.next().is_multiple_of(2) { self.near(size, target.0, target.1, 4) } else { self.near(size, bx, by, 6) };
                Command::CastSpell { priest, spell: self.below(10) as u16, x, y }
            }
            12 => {
                let (x, y) = self.near(size, bx, by, 8);
                Command::SetWorkingArea { building: b32, x, y }
            }
            13 => Command::SwitchBuilding { building: b32 },
            14 => Command::PrioritizeBuildingSite { building: b32 },
            15 => Command::SoldierOrder { building: b, kind: self.below(70) as u16, amount: 1 + self.below(5) as u32 },
            16 => Command::ProductionOrder { building: b, good, amount: 1 + self.below(4) as u32 },
            17 => Command::WorkshopOrder { building: b, item: self.below(8) as u16, amount: 1 + self.below(3) as u32 },
            18 => match self.below(4) {
                0 => Command::TransportGood { building: b, good, amount: 1 + self.below(8) as u32 },
                1 => Command::TradeWith { building: b32, value: self.below(9) as u32 },
                2 => Command::TradeGood { building: b32, value: self.below(44) as u32 },
                _ => {
                    let Some((t, _, _)) = self.pick(&own) else { return Vec::new() };
                    Command::SetTradeTarget { building: b32, target: t as u32 }
                }
            },
            19 => {
                let Some(v) = self.pick(&vehicles) else { return Vec::new() };
                match self.below(6) {
                    0 => Command::VehicleUnload { vehicle: v as u32 },
                    1 => Command::CatapultLoad { vehicle: v as u32 },
                    2 => Command::CreateSettlement { cart: v as u32 },
                    3 => Command::CreateNormalCart { cart: v as u32 },
                    4 => Command::DestroyVehicle { vehicle: v as u32 },
                    _ => {
                        let (x, y) = self.near(size, target.0, target.1, 3);
                        Command::SendTo { x, y, mode: 0, entities: vec![v] }
                    }
                }
            }
            20 => {
                if self.next().is_multiple_of(2) {
                    Command::FillTower { building: b32, value: u32::MAX }
                } else {
                    Command::EmptyTower { building: b, kind: self.below(3) as u16, amount: 1 }
                }
            }
            21 => Command::SetSpecialistWish { eco_sector: 1 + self.below(4) as u16, specialist: self.below(8) as u16, value: self.below(6) as u32 },
            22 => Command::StorageGoodSwitch { building: b, good, value: self.below(3) as u32 },
            23 => Command::SetTransportPriority { eco_sector: 1 + self.below(4) as u32, good, delta: self.below(3) as i16 - 1 },
            24 => {
                let n = 1 + self.below(4);
                Command::MorphToCarrier { entities: specialists.iter().take(n).copied().collect() }
            }
            _ => {
                if self.below(20) == 0 {
                    Command::CrushBuilding { building: b32 }
                } else {
                    Command::DefineGroup { group: self.below(10) as u32, entities: soldiers.iter().take(8).copied().collect() }
                }
            }
        };
        vec![PlayerCommand::new(p, c)]
    }
}
