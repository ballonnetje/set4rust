//! Peers on localhost run a map in lockstep over UDP, each issuing commands, and stay
//! hash-identical: two on roman02 for 5,000 ticks, three (full mesh) on a three-player
//! multiplayer map; a divergence injected on one side is detected within one hash interval.
//! Needs the game install (skipped without it). Run with `--release`.

use std::sync::mpsc;
use std::time::Duration;

use s4_net::{GameSettings, NetEvent, Session, step};
use s4_sim::{Command, EntityData, World};

fn roman02() -> Option<World> {
    world(&s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map"))
}

/// The smallest multiplayer map for at least three players.
fn three_player_map() -> Option<std::path::PathBuf> {
    let dir = s4_formats::game_dir().join("Map").join("Multiplayer");
    let mut best: Option<(u32, std::path::PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let Ok(m) = s4_formats::map::MapFile::open(&p) else { continue };
        let Ok(g) = m.general() else { continue };
        if g.player_count >= 3 && best.as_ref().is_none_or(|b| g.size < b.0) {
            best = Some((g.size, p));
        }
    }
    best.map(|b| b.1)
}

fn world(path: &std::path::Path) -> Option<World> {
    let map = s4_formats::map::MapFile::open(path).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut world, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    // The start territory, as s4-viewer's `world_from_map` sets it up.
    let towers: Vec<(i32, i32)> = world
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&world, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut world, x, y);
    }
    Some(world)
}

/// A few commands for `player`: woodcutter sites near its start, then switching its first
/// building off. Returned with the tick after which they are issued.
fn script_for(world: &World, player: u8) -> Vec<(u32, Command)> {
    let Some(Some(p)) = world.players.get(player as usize) else { return Vec::new() };
    let (sx, sy) = (p.start_x as i32, p.start_y as i32);
    let sites = s4_sim::build::check::possible_sites(world, player, 1, (sx - 20, sy - 20), (sx + 20, sy + 20));
    let mut out: Vec<(u32, Command)> = sites.iter().step_by(7).take(3).enumerate().map(|(i, s)| (40 + 97 * i as u32 + player as u32 * 13, Command::Build { building: 1, x: s.0, y: s.1 })).collect();
    if let Some((id, _)) = world.entities.iter().find(|(_, e)| e.owner == player && matches!(e.data, EntityData::Building(_))) {
        out.push((700 + player as u32, Command::SwitchBuilding { building: id as u32 }));
    }
    out
}

struct Run {
    hashes: Vec<u64>,
    events: Vec<NetEvent>,
    issued: usize,
}

/// One peer: host (`host` = None) of `players`, or client of `host`.
fn peer(map: std::path::PathBuf, players: usize, host: Option<std::net::SocketAddr>, bound: Option<mpsc::Sender<std::net::SocketAddr>>, ticks: u32, inject_at: Option<u32>) -> anyhow::Result<Run> {
    let mut world = world(&map).ok_or_else(|| anyhow::anyhow!("no game install"))?;
    let hash = s4_net::map_hash(&map)?;
    let settings = GameSettings { map: map.to_string_lossy().into_owned(), delay_ms: 142, hash_interval: 64, map_hash: hash, ..Default::default() };
    let mut s = match host {
        None => Session::host("127.0.0.1:0", "host", players, settings, Duration::from_secs(30), |a| {
            if let Some(tx) = &bound {
                let _ = tx.send(a);
            }
        })?,
        Some(h) => Session::join("127.0.0.1:0", h, "client", hash, Duration::from_secs(30))?,
    };
    let mut plan = script_for(&world, s.local);
    plan.sort_by_key(|p| p.0);
    let issued = plan.len();
    let mut plan = plan.into_iter().peekable();
    let mut hashes = Vec::with_capacity(ticks as usize);
    let mut events = Vec::new();
    while world.tick < ticks {
        while let Some((t, c)) = plan.next_if(|p| p.0 <= world.tick) {
            let _ = t;
            s.lock.issue(c);
        }
        events.extend(step(&mut s, &mut world, Duration::from_secs(20))?);
        if Some(world.tick) == inject_at {
            // A divergence on this side only.
            world.rng.seed ^= 0x5a5a;
        }
        hashes.push(world.state_hash());
    }
    events.extend(s.linger(Duration::from_millis(300)));
    s.leave();
    Ok(Run { hashes, events, issued })
}

/// `n` peers on `map`; the last client injects the divergence, if any.
fn group(map: std::path::PathBuf, n: usize, ticks: u32, inject_at: Option<u32>) -> Vec<Run> {
    let (tx, rx) = mpsc::channel();
    let m = map.clone();
    let host = std::thread::spawn(move || peer(m, n, None, Some(tx), ticks, None));
    let addr = rx.recv_timeout(Duration::from_secs(60)).expect("host bound");
    let clients: Vec<_> = (1..n)
        .map(|i| {
            let m = map.clone();
            let inject = if i == n - 1 { inject_at } else { None };
            std::thread::spawn(move || peer(m, n, Some(addr), None, ticks, inject))
        })
        .collect();
    let mut runs = vec![host.join().unwrap().unwrap()];
    runs.extend(clients.into_iter().map(|c| c.join().unwrap().unwrap()));
    runs
}

fn pair(ticks: u32, inject_at: Option<u32>) -> Option<(Run, Run)> {
    roman02()?;
    let mut r = group(s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map"), 2, ticks, inject_at);
    let c = r.pop()?;
    Some((r.pop()?, c))
}

#[test]
fn a_different_map_is_rejected() {
    let Some(map) = three_player_map() else { return };
    let settings = GameSettings { map: "x".into(), delay_ms: 0, hash_interval: 64, map_hash: s4_net::map_hash(&map).unwrap(), ..Default::default() };
    let (tx, rx) = mpsc::channel();
    let host = std::thread::spawn(move || Session::host("127.0.0.1:0", "host", 2, settings, Duration::from_secs(10), |a| tx.send(a).unwrap()).err());
    let addr = rx.recv().unwrap();
    let e = Session::join("127.0.0.1:0", addr, "client", 1234, Duration::from_secs(10)).err().expect("rejected");
    assert!(e.to_string().contains("differs from the host's"), "{e}");
    assert!(host.join().unwrap().is_some(), "the host gets nobody");
}

#[test]
fn three_peers_mesh_stays_identical() {
    let Some(map) = three_player_map() else { return };
    let runs = group(map.clone(), 3, 2000, None);
    assert!(runs.iter().all(|r| r.issued > 0), "every side issues commands");
    for (i, r) in runs.iter().enumerate().skip(1) {
        let d = runs[0].hashes.iter().zip(&r.hashes).position(|(a, b)| a != b);
        assert_eq!(d, None, "peer {i} differs from tick {:?}", d.map(|t| t + 1));
    }
    assert!(!runs.iter().flat_map(|r| &r.events).any(|e| matches!(e, NetEvent::Desync(_))));
    println!("3 peers on {}: 2000 ticks identical, final {:016x}", map.display(), runs[0].hashes[1999]);
}

#[test]
fn two_peers_stay_identical_for_5000_ticks() {
    let Some((h, c)) = pair(5000, None) else { return };
    assert!(h.issued > 0 && c.issued > 0, "both sides issue commands");
    assert_eq!(h.hashes.len(), 5000);
    let first_diff = h.hashes.iter().zip(&c.hashes).position(|(a, b)| a != b);
    assert_eq!(first_diff, None, "per-tick hashes differ from tick {:?}", first_diff.map(|i| i + 1));
    assert!(!h.events.iter().chain(&c.events).any(|e| matches!(e, NetEvent::Desync(_))), "{:?} {:?}", h.events, c.events);
    // The commands changed the game: the same world without them ends elsewhere.
    let mut alone = roman02().unwrap();
    for _ in 0..5000 {
        s4_sim::tick(&mut alone, &[]);
    }
    assert_ne!(alone.state_hash(), *h.hashes.last().unwrap(), "the commands had an effect");
    println!("5000 ticks identical, final {:016x}; host issued {}, client {}", h.hashes[4999], h.issued, c.issued);
}

#[test]
fn injected_divergence_is_detected_within_two_intervals() {
    let Some((h, c)) = pair(600, Some(300)) else { return };
    let found: Vec<u32> = h.events.iter().chain(&c.events).filter_map(|e| if let NetEvent::Desync(d) = e { Some(d.tick) } else { None }).collect();
    let first = *found.iter().min().expect("desync detected");
    // Normally the next exchange (≤ 364); allowed the one after (a loaded machine can delay the
    // hash after 320 and its resends past it). The report below is the point.
    assert!(first > 300 && first <= 300 + 128, "detected at {first}");
    let report = h.events.iter().chain(&c.events).find_map(|e| if let NetEvent::DesyncReport(r) = e { Some(r.clone()) } else { None }).expect("dump exchanged");
    // Only the RNG was touched at 300; 20 ticks later more has diverged, the header (tick) has not.
    assert!(report.contains("!= rng") && !report.contains("!= header"), "{report}");
    println!("diverged after 300, detected at {first}\n{report}");
}
