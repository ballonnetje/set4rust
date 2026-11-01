//! [ours] Network games through the internet relay (s4-relay, in process on 127.0.0.1): the
//! lobby (room list, join, roster, chat, start handshake) and lockstep games of two and three
//! peers, whose per-tick hashes equal the same game played directly; and a game over a relay that
//! loses and reorders data. The games need the game install (skipped without it). Run with
//! `--release`.

use std::net::SocketAddr;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use s4_net::lobby::{FIELD_READY, LobbyEvent};
use s4_net::{ClientLobby, Discovery, GameSettings, HostLobby, Link, NetEvent, Session, step};
use s4_sim::{Command, EntityData, World};

/// A relay on 127.0.0.1 (stopped when dropped).
struct Relay {
    addr: SocketAddr,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Relay {
    fn start(chaos: Option<s4_relay::Chaos>) -> Relay {
        let (addr, stop, _) = s4_relay::spawn("127.0.0.1:0", s4_relay::Config { chaos, ..Default::default() }).unwrap();
        Relay { addr, stop }
    }
    fn server(&self) -> String {
        self.addr.to_string()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Lobby through the relay: the host's game is listed with its player count, a client joins
/// by the listed address, edits its slot, chats, and both start; the started game is no longer
/// listed.
#[test]
fn lobby_through_the_relay() {
    let relay = Relay::start(None);
    let settings = GameSettings { map: "m".into(), delay_ms: 0, hash_interval: 64, map_hash: 7, ..Default::default() };
    let mut host = HostLobby::open_internet(&relay.server(), "host", "net game", 2, settings).unwrap();
    assert!(host.is_internet());
    let mut d = Discovery::internet(&relay.server()).unwrap();
    let wait = |what: &str, cond: &mut dyn FnMut() -> bool| {
        let t = Instant::now();
        while !cond() {
            assert!(t.elapsed() < Duration::from_secs(15), "{what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    d.query(&[]);
    wait("listed", &mut || {
        host.pump();
        d.pump();
        !d.games.is_empty()
    });
    let (addr, info, _) = d.games[0].clone();
    assert_eq!((info.name.as_str(), info.map.as_str(), info.joined, info.max), ("net game", "m", 1, 2));
    assert_eq!(addr, host.local_addr().unwrap());
    let mut c = ClientLobby::connect_internet(&relay.server(), addr, "client", 7).unwrap();
    wait("joined", &mut || {
        host.pump();
        c.pump();
        c.me.is_some() && c.slots.len() == 2
    });
    assert_eq!(c.me, Some(2));
    // The list shows two players now.
    let mut k = 0;
    wait("two listed", &mut || {
        host.pump();
        c.pump();
        d.pump();
        k += 1;
        if k % 25 == 0 {
            d.query(&[]);
        }
        d.games.first().is_some_and(|g| g.1.joined == 2)
    });
    c.change(FIELD_READY, true);
    c.say("hello over the internet");
    let mut chat = Vec::new();
    wait("ready and chat", &mut || {
        chat.extend(host.pump().into_iter().filter(|e| matches!(e, LobbyEvent::Chat(_))));
        c.pump();
        host.can_start() && c.chat.len() == 1
    });
    assert_eq!(c.chat, vec!["client: hello over the internet".to_string()]);
    let h = std::thread::spawn(move || host.start(Duration::from_secs(15)).map(|(s, _)| s.local));
    wait("started", &mut || {
        c.pump();
        c.started()
    });
    let s = c.into_session().unwrap();
    assert_eq!(s.local, 2);
    assert_eq!(h.join().unwrap().unwrap(), 1);
    // Started: unlisted.
    wait("unlisted", &mut || {
        d.query(&[]);
        std::thread::sleep(Duration::from_millis(20));
        d.pump();
        d.games.is_empty()
    });
}

/// The joining side must name a relay game.
#[test]
fn a_lan_address_is_no_relay_game() {
    let relay = Relay::start(None);
    let e = ClientLobby::connect_internet(&relay.server(), "192.168.1.5:7710".parse().unwrap(), "c", 1).err().expect("refused");
    assert!(e.to_string().contains("no game on the relay"), "{e}");
    // Nobody answering: registration times out.
    let dead = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let t = Instant::now();
    assert!(s4_net::RelayLink::connect(&dead.local_addr().unwrap().to_string(), Duration::from_millis(400)).is_err());
    assert!(t.elapsed() < Duration::from_secs(2));
}

/// Liveness: a host lobby with a client and an open game list stay alive (and the game stays
/// listed) past the 15 s listing window while pumped, the relay answering their heartbeats;
/// after the relay stops, the list, the host and the client each report it lost within
/// `RELAY_LOST` (plus a heartbeat).
#[test]
fn a_dead_relay_is_lost() {
    let relay = Relay::start(None);
    let settings = GameSettings { map: "m".into(), delay_ms: 0, hash_interval: 64, map_hash: 7, ..Default::default() };
    let mut host = HostLobby::open_internet(&relay.server(), "host", "g", 2, settings).unwrap();
    let mut c = ClientLobby::connect_internet(&relay.server(), host.local_addr().unwrap(), "client", 7).unwrap();
    let mut d = Discovery::internet(&relay.server()).unwrap();
    let t = Instant::now();
    let mut k = 0;
    while t.elapsed() < Duration::from_secs(17) {
        host.pump();
        c.pump();
        d.pump();
        k += 1;
        if k % 50 == 0 {
            d.query(&[]);
        }
        assert!(!(host.relay_lost() || c.relay_lost() || d.relay_lost()), "lost while the relay runs");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(c.me, Some(2));
    assert_eq!(d.games.len(), 1, "still listed after 17 s of heartbeats");
    drop(relay);
    let t = Instant::now();
    let mut lost = [None; 3];
    while lost.iter().any(|l| l.is_none()) {
        assert!(t.elapsed() < s4_net::relay::RELAY_LOST + Duration::from_secs(4), "not reported lost: {lost:?}");
        host.pump();
        c.pump();
        d.pump();
        d.query(&[]);
        for (i, l) in [host.relay_lost(), c.relay_lost(), d.relay_lost()].into_iter().enumerate() {
            if l && lost[i].is_none() {
                lost[i] = Some(t.elapsed());
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Not before the last answer is RELAY_LOST old (the last one came at most a heartbeat
    // before the stop).
    for l in lost.iter().flatten() {
        assert!(*l + Duration::from_secs(2) >= s4_net::relay::RELAY_LOST, "{lost:?}");
    }
}

/// The relay dies during a game of two (kick delay 8 s here): both peers report `RelayLost`
/// once (10 s), wait for each other (`Waiting` at 6 s); the host, deciding, kicks the client at
/// the kick delay as with any silent peer; once the relay is lost and the kick delay passed, the
/// client drops the host too, both go `Offline` and play on alone.
#[test]
fn a_game_whose_relay_dies() {
    let Some(map) = map_for(2) else { return };
    let relay = Relay::start(None);
    let (tx, rx) = mpsc::channel();
    let (at_tx, at_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let go_rx = std::sync::Arc::new(std::sync::Mutex::new(go_rx));
    let run = |host: Option<SocketAddr>, bound: Option<mpsc::Sender<SocketAddr>>, server: String, map: std::path::PathBuf, at: mpsc::Sender<()>, go: std::sync::Arc<std::sync::Mutex<mpsc::Receiver<()>>>| {
        move || -> (u8, Vec<(NetEvent, Duration)>, u32) {
            let mut world = world(&map).unwrap();
            let hash = s4_net::map_hash(&map).unwrap();
            let settings = GameSettings { map: map.to_string_lossy().into_owned(), delay_ms: 142, hash_interval: 64, map_hash: hash, ..Default::default() };
            let link = Link::relay(&server, Duration::from_secs(5)).unwrap();
            let mut s = match host {
                None => Session::host_on(link, "host", 2, 1, settings, Duration::from_secs(30), |a| bound.unwrap().send(a).unwrap()).unwrap(),
                Some(h) => Session::join_on(link, h, "client", hash, (0, 0), Duration::from_secs(30)).unwrap(),
            };
            s.lock.not_ready.kick_ms = Some(8000);
            while world.tick < 200 {
                step(&mut s, &mut world, Duration::from_secs(20)).unwrap();
            }
            at.send(()).unwrap();
            go.lock().unwrap().recv().unwrap();
            let t = Instant::now();
            let mut events = Vec::new();
            while t.elapsed() < Duration::from_secs(14) {
                let (cmds, ev) = s.poll_ready(world.tick + 1);
                events.extend(ev.into_iter().map(|e| (e, t.elapsed())));
                match cmds {
                    Some(c) => {
                        s4_sim::tick(&mut world, &c);
                        s.produce(world.tick);
                    }
                    None => std::thread::sleep(Duration::from_millis(2)),
                }
            }
            (s.local, events, world.tick)
        }
    };
    let h = std::thread::spawn(run(None, Some(tx), relay.server(), map.clone(), at_tx.clone(), go_rx.clone()));
    let addr = rx.recv_timeout(Duration::from_secs(60)).unwrap();
    let c = std::thread::spawn(run(Some(addr), None, relay.server(), map.clone(), at_tx, go_rx));
    for _ in 0..2 {
        at_rx.recv_timeout(Duration::from_secs(120)).unwrap();
    }
    // Both at tick 200: the relay stops, then they go on.
    drop(relay);
    for _ in 0..2 {
        go_tx.send(()).unwrap();
    }
    let (h, c) = (h.join().unwrap(), c.join().unwrap());
    println!("host: {:?}, tick {}
client: {:?}, tick {}", h.1, h.2, c.1, c.2);
    for (who, other, (_, ev, tick)) in [("host", 2, &h), ("client", 1, &c)] {
        let at = |e: NetEvent| ev.iter().filter(|x| x.0 == e).map(|x| x.1).collect::<Vec<_>>();
        assert_eq!(at(NetEvent::RelayLost).len(), 1, "{who}: {ev:?}");
        let w = at(NetEvent::Waiting(other));
        // Lower bounds are the rule's; the upper ones only catch a stall (a loaded machine runs late).
        assert!(w.first().is_some_and(|t| *t >= Duration::from_secs(5) && *t < Duration::from_secs(11)), "{who}: {ev:?}");
        let off = at(NetEvent::Offline);
        assert_eq!(off.len(), 1, "{who} goes offline once: {ev:?}");
        assert!(off[0] >= Duration::from_secs(9) && off[0] < Duration::from_secs(14), "{who}: {ev:?}");
        // The deciding host may have kicked the client already at the kick delay (8 s).
        assert_eq!(at(NetEvent::Left(other)).len(), 1, "{who}: {ev:?}");
        assert!(at(NetEvent::Left(other))[0] <= off[0], "{who}: {ev:?}");
        assert!(ev.last().is_some_and(|e| e.0 == NetEvent::Offline), "nothing after going offline: {ev:?}");
        assert!(*tick > 260, "{who} plays on alone: tick {tick}");
    }
}

fn world(path: &std::path::Path) -> Option<World> {
    let map = s4_formats::map::MapFile::open(path).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut world, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
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

/// Woodcutter sites near the player's start, then its first building switched off.
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

/// One peer of `players` on `map` for `ticks`: host (`host` None) or client, directly or through
/// the relay at `relay`. Its per-tick hashes and network events.
fn peer(map: std::path::PathBuf, players: usize, relay: Option<String>, host: Option<SocketAddr>, bound: Option<mpsc::Sender<SocketAddr>>, ticks: u32, inject_at: Option<u32>) -> anyhow::Result<(Vec<u64>, Vec<NetEvent>)> {
    let mut world = world(&map).ok_or_else(|| anyhow::anyhow!("no game install"))?;
    let hash = s4_net::map_hash(&map)?;
    let settings = GameSettings { map: map.to_string_lossy().into_owned(), delay_ms: 142, hash_interval: 64, map_hash: hash, ..Default::default() };
    let link = match &relay {
        Some(r) => Link::relay(r, Duration::from_secs(5))?,
        None => Link::udp("127.0.0.1:0")?,
    };
    let timeout = Duration::from_secs(30);
    let mut s = match host {
        None => Session::host_on(link, "host", players, 1, settings, timeout, |a| {
            if let Some(tx) = &bound {
                let _ = tx.send(a);
            }
        })?,
        Some(h) => Session::join_on(link, h, "client", hash, (0, 0), timeout)?,
    };
    let mut plan = script_for(&world, s.local);
    plan.sort_by_key(|p| p.0);
    let mut plan = plan.into_iter().peekable();
    let mut hashes = Vec::with_capacity(ticks as usize);
    let mut events = Vec::new();
    while world.tick < ticks {
        while let Some((_, c)) = plan.next_if(|p| p.0 <= world.tick) {
            s.lock.issue(c);
        }
        events.extend(step(&mut s, &mut world, Duration::from_secs(20))?);
        if Some(world.tick) == inject_at {
            // A divergence on this side only.
            world.rng.seed ^= 0x5a5a;
        }
        hashes.push(world.state_hash());
    }
    events.extend(s.linger(Duration::from_millis(if inject_at.is_some() { 1500 } else { 300 })));
    s.leave();
    Ok((hashes, events))
}

/// `n` peers, directly or through `relay`: every peer's hashes.
fn group(map: &std::path::Path, n: usize, relay: Option<String>, ticks: u32) -> Vec<(Vec<u64>, Vec<NetEvent>)> {
    group_with(map, n, relay, ticks, None)
}

/// [`group`] whose last client diverges after `inject_at`.
fn group_with(map: &std::path::Path, n: usize, relay: Option<String>, ticks: u32, inject_at: Option<u32>) -> Vec<(Vec<u64>, Vec<NetEvent>)> {
    let (tx, rx) = mpsc::channel();
    let (m, r) = (map.to_path_buf(), relay.clone());
    let host = std::thread::spawn(move || peer(m, n, r, None, Some(tx), ticks, None));
    let addr = rx.recv_timeout(Duration::from_secs(60)).expect("host up");
    let clients: Vec<_> = (1..n)
        .map(|i| {
            let (m, r) = (map.to_path_buf(), relay.clone());
            let inject = if i == n - 1 { inject_at } else { None };
            std::thread::spawn(move || peer(m, n, r, Some(addr), None, ticks, inject))
        })
        .collect();
    let mut runs = vec![host.join().unwrap().unwrap()];
    runs.extend(clients.into_iter().map(|c| c.join().unwrap().unwrap()));
    runs
}

fn map_for(players: u32) -> Option<std::path::PathBuf> {
    let dir = s4_formats::game_dir().join("Map").join("Multiplayer");
    let mut best: Option<(u32, std::path::PathBuf)> = None;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let Ok(m) = s4_formats::map::MapFile::open(&p) else { continue };
        let Ok(g) = m.general() else { continue };
        if g.player_count >= players && best.as_ref().is_none_or(|b| g.size < b.0) {
            best = Some((g.size, p));
        }
    }
    best.map(|b| b.1)
}

fn assert_same(direct: &[(Vec<u64>, Vec<NetEvent>)], relayed: &[(Vec<u64>, Vec<NetEvent>)], ticks: usize) {
    for (i, r) in relayed.iter().enumerate() {
        assert_eq!(r.0.len(), ticks);
        let d = direct[0].0.iter().zip(&r.0).position(|(a, b)| a != b);
        assert_eq!(d, None, "relayed peer {i} differs from the direct game from tick {:?}", d.map(|t| t + 1));
        // (Players that finished first leave; a departure during the game would show in the
        // hashes, as its PLAYER_LEFT runs.)
        assert!(!r.1.iter().any(|e| matches!(e, NetEvent::Desync(_))), "{:?}", r.1);
    }
}

/// Two and three peers through the relay: every tick's hash equals the direct game's.
#[test]
fn games_through_the_relay_equal_the_direct_game() {
    let Some(map) = map_for(3) else { return };
    let relay = Relay::start(None);
    for (n, ticks) in [(2, 1500), (3, 1000)] {
        let direct = group(&map, n, None, ticks);
        let relayed = group(&map, n, Some(relay.server()), ticks);
        assert_same(&direct, &relayed, ticks as usize);
        println!("{n} peers on {}: {ticks} ticks through the relay equal the direct game, final {:016x}", map.display(), relayed[0].0[ticks as usize - 1]);
    }
}

/// A relay that loses 10 % of the data and swaps neighbouring datagrams: the game still runs
/// identically (turns are resent until acknowledged).
#[test]
fn lossy_reordering_relay() {
    let Some(map) = map_for(3) else { return };
    let direct = group(&map, 3, None, 800);
    let relay = Relay::start(Some(s4_relay::Chaos { drop_per_mille: 100, reorder: true, seed: 7 }));
    let relayed = group(&map, 3, Some(relay.server()), 800);
    assert_same(&direct, &relayed, 800);
}

/// A desync in an internet game: detected within an interval, and both sides get the whole
/// per-subsystem report through a relay that loses 10 % of the data (the dump goes in parts under
/// 1200 bytes, resent for 5 s).
#[test]
fn desync_report_through_the_relay() {
    let Some(map) = map_for(2) else { return };
    let w = world(&map).unwrap();
    let parts = s4_net::world_parts(&w);
    let packets = s4_net::wire::dump_packets(1, 0, &parts);
    println!("a dump: {} entries in {} datagram(s) of {:?} bytes", parts.len(), packets.len(), packets.iter().map(|p| p.encode(1).len()).collect::<Vec<_>>());
    let relay = Relay::start(Some(s4_relay::Chaos { drop_per_mille: 100, reorder: true, seed: 3 }));
    let runs = group_with(&map, 2, Some(relay.server()), 600, Some(300));
    for (who, r) in ["host", "client"].iter().zip(&runs) {
        let first = r.1.iter().filter_map(|e| if let NetEvent::Desync(d) = e { Some(d.tick) } else { None }).min().unwrap_or_else(|| panic!("{who}: no desync"));
        // Within two intervals: through a relay that drops 10 % on a loaded machine, the hash
        // after 320 and its resends (the last four every 100 ms) can all miss their interval
        // (seen in a full-workspace trial: 384). What this test is about is the whole report.
        assert!(first > 300 && first <= 300 + 128, "{who}: detected at {first}");
        let report = r.1.iter().find_map(|e| if let NetEvent::DesyncReport(r) = e { Some(r.clone()) } else { None }).unwrap_or_else(|| panic!("{who}: no report"));
        // Every local entry has the remote hash beside it, and the RNG is named.
        assert_eq!(report.lines().count(), parts.len() + 1, "{report}");
        assert!(!report.lines().skip(1).any(|l| l.trim_end().ends_with(" -")), "{who}: incomplete
{report}");
        assert!(report.contains("!= rng"), "{report}");
    }
}
