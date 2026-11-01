//! A player leaving a network game (0x5a2da0): peers on localhost run a three-player map in
//! lockstep over UDP; one leaves (its Bye, after its turns are acknowledged) or goes silent (the
//! deciding peer kicks it after the kick delay, here shortened). The others run on, every tick
//! hash-identical, run the departed player's `PLAYER_LEFT` at the same tick, and (once s4-sim
//! has the command) its people are the AI's. Needs the game
//! install (skipped without it). Run with `--release`.

use std::sync::mpsc;
use std::time::Duration;

use s4_net::lockstep::PLAYER_LEFT;
use s4_net::{GameSettings, Session};
use s4_sim::{EntityData, World};

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

fn world(path: &std::path::Path, players: u8) -> World {
    let map = s4_formats::map::MapFile::open(path).unwrap();
    let rules = s4_sim::Rules::load_default().unwrap();
    let (mut world, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame { players: Some(players), ..Default::default() }).unwrap();
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
    world
}

/// What a peer does after tick `at`.
#[derive(Clone, Copy, PartialEq)]
enum End {
    /// Run to the end.
    Stay,
    /// Leave (Bye) after this tick.
    Leave(u32),
    /// Go silent after this tick (no Bye, no more packets).
    Silent(u32),
}

struct Run {
    local: u8,
    hashes: Vec<(u32, u64)>,
    /// The ticks a PLAYER_LEFT ran at: (tick, player, control).
    left: Vec<(u32, u8, u32)>,
    ai: Vec<bool>,
}

fn peer(map: std::path::PathBuf, players: usize, host: Option<std::net::SocketAddr>, bound: Option<mpsc::Sender<std::net::SocketAddr>>, ticks: u32, end: impl Fn(u8) -> End) -> Run {
    let mut world = world(&map, players as u8);
    let hash = s4_net::map_hash(&map).unwrap();
    let settings = GameSettings { map: map.to_string_lossy().into_owned(), delay_ms: 142, hash_interval: 16, map_hash: hash, ai_level: 1, ..Default::default() };
    let mut s = match host {
        None => Session::host("127.0.0.1:0", "host", players, settings, Duration::from_secs(30), |a| {
            if let Some(tx) = &bound {
                let _ = tx.send(a);
            }
        })
        .unwrap(),
        Some(h) => Session::join("127.0.0.1:0", h, "client", hash, Duration::from_secs(30)).unwrap(),
    };
    s.lock.not_ready.kick_ms = Some(1500);
    let end = end(s.local);
    let mut run = Run { local: s.local, hashes: Vec::new(), left: Vec::new(), ai: Vec::new() };
    while world.tick < ticks {
        match end {
            End::Leave(t) if world.tick == t => {
                s.leave();
                return run;
            }
            End::Silent(t) if world.tick == t => return run,
            _ => {}
        }
        s.produce(world.tick);
        let next = world.tick + 1;
        let (cmds, _) = s.wait_ready(next, Duration::from_secs(20));
        let cmds = cmds.unwrap_or_else(|| panic!("player {}: tick {next}: no turn from {:?}", s.local, s.lock.missing(next)));
        for c in &cmds {
            let m = c.to_message();
            if m.kind == PLAYER_LEFT {
                run.left.push((next, m.wparam as u8, m.lparam));
            }
        }
        s4_sim::tick(&mut world, &cmds);
        if s.lock.hash_due(world.tick) {
            s.send_hash(world.tick, world.state_hash(), s4_net::world_parts(&world));
            s.pump();
        }
        run.hashes.push((world.tick, world.state_hash()));
    }
    run.ai = (0..=players as u8).map(|p| world.ai.is_ai(p)).collect();
    s.linger(Duration::from_millis(300));
    s.leave();
    run
}

fn group(map: std::path::PathBuf, n: usize, ticks: u32, end: fn(u8) -> End) -> Vec<Run> {
    let (tx, rx) = mpsc::channel();
    let m = map.clone();
    let host = std::thread::spawn(move || peer(m, n, None, Some(tx), ticks, end));
    let addr = rx.recv_timeout(Duration::from_secs(60)).expect("host bound");
    let clients: Vec<_> = (1..n)
        .map(|_| {
            let m = map.clone();
            std::thread::spawn(move || peer(m, n, Some(addr), None, ticks, end))
        })
        .collect();
    let mut runs = vec![host.join().unwrap()];
    runs.extend(clients.into_iter().map(|c| c.join().unwrap()));
    runs.sort_by_key(|r| r.local);
    runs
}

/// The peers that stayed ran the same states and the departed player's PLAYER_LEFT at the same
/// tick (control 3 at AI level 1).
fn check(runs: &[Run], gone: u8, ticks: u32) -> u32 {
    let stayed: Vec<&Run> = runs.iter().filter(|r| r.local != gone).collect();
    for r in &stayed {
        assert_eq!(r.hashes.len() as u32, ticks, "player {} ran to the end", r.local);
        assert_eq!(r.hashes, stayed[0].hashes, "player {} differs from player {}", r.local, stayed[0].local);
        assert_eq!(r.left.len(), 1, "player {}: one PLAYER_LEFT: {:?}", r.local, r.left);
        assert_eq!(r.left, stayed[0].left);
    }
    let (t, p, control) = stayed[0].left[0];
    assert_eq!((p, control), (gone, 3));
    // Its people are the AI's (s4-sim `Command::PlayerLeft`), once the sim has the command.
    let canonical = s4_sim::PlayerCommand::from_message(&s4_sim::NetMessage { kind: PLAYER_LEFT, player: gone, wparam: gone as u32, lparam: 3, data: Vec::new() });
    if !matches!(canonical.command, s4_sim::Command::Other { .. }) {
        for r in &stayed {
            assert!(r.ai[gone as usize], "player {}: player {gone} has an AI", r.local);
        }
    }
    // The departed player's hashes, while it ran, match.
    let g = runs.iter().find(|r| r.local == gone).unwrap();
    assert!(!g.hashes.is_empty());
    assert_eq!(&stayed[0].hashes[..g.hashes.len()], &g.hashes[..]);
    t
}

#[test]
fn two_peers_one_leaves() {
    let Some(map) = three_player_map() else { return };
    let runs = group(map, 2, 900, |p| if p == 2 { End::Leave(300) } else { End::Stay });
    let t = check(&runs, 2, 900);
    println!("2 peers: player 2 left after tick 300, PLAYER_LEFT ran at {t}; AI {:?}", runs[0].ai);
}

#[test]
fn three_peers_one_leaves_one_goes_silent() {
    let Some(map) = three_player_map() else { return };
    let runs = group(map.clone(), 3, 900, |p| if p == 3 { End::Leave(250) } else { End::Stay });
    let t = check(&runs, 3, 900);
    println!("3 peers: player 3 left after tick 250, PLAYER_LEFT ran at {t}; AI {:?}", runs[0].ai);
    let runs = group(map, 3, 900, |p| if p == 2 { End::Silent(250) } else { End::Stay });
    let t = check(&runs, 2, 900);
    println!("3 peers: player 2 went silent after tick 250, kicked; PLAYER_LEFT ran at {t}; AI {:?}", runs[0].ai);
}
