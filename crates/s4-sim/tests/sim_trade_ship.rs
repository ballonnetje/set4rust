//! Transport ships between ports (`vehicles::transport`, V5c; the ports' side in
//! `buildings::trading`: 0x554da0 / 0x555800 / 0x5551b0 / 0x5543b0 / 0x5554d0 / 0x5555e0).
//!
//! `ship_carries_goods_between_ports`: roman02, two ports of player 1 (the variant types 52..57,
//! working area on open water, 20+ apart; their worker is taken as present) and a finished
//! transport ship near A. A sends 16 boards to B: the ship is ordered (0x554da0, state 2,
//! recorded at A), sails to A's water and loads (0x555800 → state 4; 0x5551b0 takes the boards
//! as A's piles collect them), leaves when nothing more can come (0x5543b0 → 0x5554d0: state 1,
//! recorded at B, home = A's ensign), unloads at B (state 3, 0x5555e0) and is free (state 0);
//! its way home leads onto land and so ends at once — it stays at B's water. Checked every
//! tick: the state's records / slots, the cargo slots. A counts one ship out, B one less (the
//! original's: 0x554da0 counts at the source, 0x5555e0 at the target). Deterministic.
//!
//! `ship_returns_when_the_target_goes`: B crushed while the ship carries the boards: A becomes
//! the target (0x591740), the ship brings them back and unloads at A.
//!
//! `ship_gives_up_when_the_source_goes` (0x591830) and `loading_ship_follows_a_new_target`
//! (5005 while loading → 0x553f30: the boards go to the new port).
//!
//! `ship_exchanges_goods_between_players`: the foreign trade by ship. Players 1 and 2 of
//! CoastofDeath (the first of four sea maps whose two players have port sites on one sea) each
//! get a port, player 1 a transport ship; A exports boards and imports stone, B the reverse, as
//! in `sim_trade_foreign`'s donkey test: the ship takes 16 boards to B (leg 1) and brings 16
//! stone back (leg 2); deterministic.
//!
//! Skipped when the game data is not present.

use s4_sim::buildings::trading;
use s4_sim::entity::EntityData;
use s4_sim::vehicles::{self, kind, transport};
use s4_sim::{Command, PlayerCommand, World, pile};

const BOARD: u16 = 7;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

/// Port sites of player 1 whose working area (the ships' place) is open water: `(type, x, y,
/// working area)`.
fn port_sites(w: &World) -> Vec<(u8, i32, i32, (i32, i32))> {
    let size = w.grid.size() as i32;
    let mut v = Vec::new();
    for t in [52u8, 53, 54, 55, 56, 57] {
        let info = w.rules.building(w.race_of(1), t);
        for s in s4_sim::build::possible_sites(w, 1, t, (0, 0), (size, size)) {
            let wa = (s.0 as i32 + info.working.0 as i32, s.1 as i32 + info.working.1 as i32);
            if vehicles::can_place(w, wa.0, wa.1, kind::TRANSPORT_SHIP) {
                v.push((t, s.0 as i32, s.1 as i32, wa));
            }
        }
    }
    v
}

fn board_piles(w: &World, m: u16) -> i32 {
    let Some(s) = trading::state(w, m) else { return 0 };
    s.piles.iter().filter_map(|&p| pile::get(w, p)).filter(|d| d.good == BOARD as u8 && d.trade_type == 2).map(|d| d.amount as i32).sum()
}

fn check(w: &World, sh: u16, a: u16, b: u16) -> Vec<String> {
    let mut f = Vec::new();
    let Some(d) = transport::data(w, sh) else { return vec!["no ship data".into()] };
    let recorded = |m: u16| trading::state(w, m).is_some_and(|s| s.transports.iter().any(|t| t.entity == sh as u32 && t.kind == trading::transport::SHIP));
    let (sa, sb) = (trading::state(w, a), trading::state(w, b));
    match d.state {
        0 => {}
        2 if d.source == a && d.target == b && d.to_source && recorded(a) => {}
        4 if sa.is_some_and(|s| s.ship_load.entity == sh as u32 && s.ship_load.on) => {}
        1 if d.target == b && d.to_target && recorded(b) => {}
        3 if sb.is_some_and(|s| s.ship_unload.entity == sh as u32 && s.ship_unload.on) => {}
        st => f.push(format!("state {st}: {d:?}")),
    }
    for s in &d.slots {
        if !(0..=8).contains(&s[1]) || (s[1] != 0) != (s[0] != 0) {
            f.push(format!("slot {s:?}"));
        }
    }
    f
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    states: Vec<i32>,
    at: Vec<u32>,
    delivered: i32,
    max_load: i32,
    send: i8,
    ships_out: (i32, i32),
    end: (i32, i32),
}

/// roman02: two ports A / B of player 1 (worker taken as present), a transport ship near A, and
/// the commands that send 16 boards from A to B.
fn setup() -> Option<(World, u16, u16, u16, Vec<PlayerCommand>)> {
    let mut w = roman02()?;
    let (cx, cy) = {
        let pl = w.player(1)?;
        (pl.start_x as i32, pl.start_y as i32)
    };
    let sites = port_sites(&w);
    let &(ta, ax, ay, wa) = sites.iter().min_by_key(|s| s4_sim::spiral::grid_distance(s.1 - cx, s.2 - cy))?;
    let a = s4_sim::create::add_building(&mut w, ax, ay, 1, ta).ok()?;
    let sites = port_sites(&w);
    let &(tb, bx, by, _) = sites.iter().filter(|s| s4_sim::spiral::grid_distance(s.3.0 - wa.0, s.3.1 - wa.1) >= 20).min_by_key(|s| s4_sim::spiral::grid_distance(s.1 - ax, s.2 - ay))?;
    let b = s4_sim::create::add_building(&mut w, bx, by, 1, tb).ok()?;
    // The port worker is not the subject: taken as present.
    for m in [a, b] {
        s4_sim::buildings::role_mut(&mut w, m)?.base.worker_in_house = true;
    }
    let (sx, sy) = s4_sim::spiral::walk(w.grid, wa.0 + 3, wa.1 + 3, 8).find(|&(x, y)| vehicles::can_place(&w, x, y, kind::TRANSPORT_SHIP))?;
    let sh = vehicles::add_vehicle(&mut w, sx, sy, 1, kind::TRANSPORT_SHIP, 0, false)?;
    let cmds = vec![
        PlayerCommand { player: 1, command: Command::TransportGood { building: a, good: BOARD, amount: 16 } },
        PlayerCommand { player: 1, command: Command::SetTradeTarget { building: a as u32, target: b as u32 } },
    ];
    Some((w, a, b, sh, cmds))
}

fn run(ticks: u32) -> Option<Outcome> {
    let (mut w, a, b, sh, cmds) = setup()?;
    let (mut states, mut at, mut delivered, mut max_load) = (vec![0], vec![0u32], 0, 0);
    let mut before = board_piles(&w, b);
    for t in 0..ticks {
        s4_sim::tick(&mut w, if t == 0 { &cmds } else { &[] });
        for e in std::mem::take(&mut w.events) {
            if let s4_sim::event::SimEvent::Unported { what, .. } = e {
                assert!(!what.contains("ship"), "unported: {what}");
            }
        }
        let f = check(&w, sh, a, b);
        assert!(f.is_empty(), "tick {}: {f:?}", w.tick);
        let d = transport::data(&w, sh)?;
        if *states.last()? != d.state {
            states.push(d.state);
            at.push(w.tick);
        }
        max_load = max_load.max(d.total());
        let now = board_piles(&w, b);
        delivered += (now - before).max(0);
        before = now;
    }
    let e = w.entities.by_index(sh as u32)?;
    let (oa, ob) = (trading::state(&w, a)?.ships_out, trading::state(&w, b)?.ships_out);
    Some(Outcome { hash: w.state_hash(), states, at, delivered, max_load, send: trading::state(&w, a)?.send[BOARD as usize], ships_out: (oa, ob), end: (e.x(), e.y()) })
}

#[test]
fn ship_carries_goods_between_ports() {
    let Some(r) = run(3000) else {
        eprintln!("skipping: no game data or no port sites");
        return;
    };
    eprintln!("{r:?}");
    assert_eq!(&r.states[..], &[0, 2, 4, 1, 3, 0], "trade states");
    assert_eq!(r.delivered, 16, "boards put into B's piles");
    assert_eq!(r.max_load, 16);
    assert_eq!(r.send, 0, "nothing left to send");
    assert_eq!(r.ships_out, (1, -1));
    assert_eq!(run(3000).unwrap(), r, "two runs differ");
}

const STONE: u8 = 32;

/// A map from Singleplayer / Multiplayer with the start towers' land claimed.
fn sim_world(name: &str) -> Option<World> {
    let p = ["Singleplayer", "Multiplayer"].iter().map(|d| s4_formats::game_dir().join("Map").join(d).join(name)).find(|p| p.exists())?;
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

/// Port sites of `player` (variant types 52..57) whose working area is open water.
fn port_sites_of(w: &World, player: u8) -> Vec<(u8, i32, i32, (i32, i32))> {
    let size = w.grid.size() as i32;
    let mut v = Vec::new();
    for t in [52u8, 53, 54, 55, 56, 57] {
        let info = w.rules.building(w.race_of(player), t);
        for s in s4_sim::build::possible_sites(w, player, t, (0, 0), (size, size)) {
            let wa = (s.0 as i32 + info.working.0 as i32, s.1 as i32 + info.working.1 as i32);
            if vehicles::can_place(w, wa.0, wa.1, kind::TRANSPORT_SHIP) {
                v.push((t, s.0 as i32, s.1 as i32, wa));
            }
        }
    }
    v
}

/// 16 goods into two free piles of `m` as foreign collecting piles (role 3).
fn fill(w: &mut World, m: u16, good: u8) {
    let free: Vec<u16> = trading::state(w, m).unwrap().piles.iter().copied().filter(|&p| pile::get(w, p).is_some_and(|d| d.trade_type == 0)).take(2).collect();
    for p in free {
        pile::set_good_type(w, p, good);
        pile::set_trade_type(w, p, 3);
        for _ in 0..8 {
            pile::increase_unforeseen(w, p, 1);
        }
    }
}

fn received(w: &World, m: u16, good: u8) -> i32 {
    trading::state(w, m).unwrap().piles.iter().filter_map(|&p| pile::get(w, p)).filter(|d| d.good == good && d.trade_type == 2).map(|d| d.amount as i32).sum()
}

#[derive(Debug, PartialEq, Eq)]
struct Foreign {
    map: &'static str,
    hash: u64,
    partners: (u16, u16),
    /// (tick, state, leg, foreign) whenever the ship's state or leg changed.
    log: Vec<(u32, i32, i32, bool)>,
    boards_at_b: i32,
    stone_at_a: i32,
}

/// Players 1 and 2 each get a port on a shared sea (the first map that has such sites), player
/// 1 a transport ship; both trade by the foreign trader as the donkey test does (A exports
/// boards, imports stone; B the other way round).
fn foreign(ticks: u32) -> Option<Foreign> {
    for map in ["Harbour.map", "Harbour Island.map", "MiddleIslands.map", "CoastofDeath.map"] {
        let Some(mut w) = sim_world(map) else { continue };
        if w.player(2).is_none() {
            continue;
        }
        let (s1, s2) = (port_sites_of(&w, 1), port_sites_of(&w, 2));
        let sector = |w: &World, p: (i32, i32)| w.tiling.sector_of(w.grid.index(p.0 as u32, p.1 as u32));
        let Some(&(ta, ax, ay, wa)) = s1.iter().find(|a| s2.iter().any(|b| sector(&w, a.3) == sector(&w, b.3))) else {
            eprintln!("{map}: no shared sea ({} / {} sites)", s1.len(), s2.len());
            continue;
        };
        let &(tb, bx, by, _) = s2.iter().find(|b| sector(&w, b.3) == sector(&w, wa))?;
        let a = s4_sim::create::add_building(&mut w, ax, ay, 1, ta).ok()?;
        let b = s4_sim::create::add_building(&mut w, bx, by, 2, tb).ok()?;
        assert!(s4_sim::buildings::trader::same_sector(&w, a, b), "{map}: the ports share a sea");
        for m in [a, b] {
            s4_sim::buildings::role_mut(&mut w, m)?.base.worker_in_house = true;
        }
        let (sx, sy) = s4_sim::spiral::walk(w.grid, wa.0 + 3, wa.1 + 3, 8).find(|&(x, y)| vehicles::can_place(&w, x, y, kind::TRANSPORT_SHIP))?;
        let sh = vehicles::add_vehicle(&mut w, sx, sy, 1, kind::TRANSPORT_SHIP, 0, false)?;
        let c = |player: u8, command| PlayerCommand { player, command };
        let start = vec![
            c(1, Command::TradeGood { building: a as u32, value: BOARD as u32 }),
            c(1, Command::TradeGood { building: a as u32, value: BOARD as u32 }),
            c(1, Command::TradeGood { building: a as u32, value: STONE as u32 }),
            c(1, Command::TradeWith { building: a as u32, value: 2 }),
            c(2, Command::TradeGood { building: b as u32, value: STONE as u32 }),
            c(2, Command::TradeGood { building: b as u32, value: STONE as u32 }),
            c(2, Command::TradeGood { building: b as u32, value: BOARD as u32 }),
            c(2, Command::TradeWith { building: b as u32, value: 1 }),
        ];
        let mut log = Vec::new();
        let mut last = (0, 0);
        let (mut boards_at_b, mut stone_at_a, mut rb, mut ra) = (0, 0, 0, 0);
        for t in 0..ticks {
            s4_sim::tick(&mut w, if t == 0 { &start } else { &[] });
            if t == 30 {
                fill(&mut w, a, BOARD as u8);
                fill(&mut w, b, STONE);
            }
            for e in std::mem::take(&mut w.events) {
                if let s4_sim::event::SimEvent::Unported { what, .. } = e {
                    assert!(!what.contains("ship") && !what.contains("oreign"), "unported: {what}");
                }
            }
            let d = transport::data(&w, sh)?;
            if (d.state, d.leg) != last {
                last = (d.state, d.leg);
                log.push((w.tick, d.state, d.leg, d.foreign));
            }
            let (nb, na) = (received(&w, b, BOARD as u8), received(&w, a, STONE));
            boards_at_b += (nb - rb).max(0);
            stone_at_a += (na - ra).max(0);
            (rb, ra) = (nb, na);
        }
        let partners = (trading::state(&w, a)?.partner, trading::state(&w, b)?.partner);
        return Some(Foreign { map, hash: w.state_hash(), partners, log, boards_at_b, stone_at_a });
    }
    None
}

#[test]
fn ship_exchanges_goods_between_players() {
    let Some(r) = foreign(6000) else {
        eprintln!("skipping: no game data or no shared sea");
        return;
    };
    eprintln!("{}: partners {:?}, boards at B {}, stone at A {}", r.map, r.partners, r.boards_at_b, r.stone_at_a);
    for l in &r.log {
        eprintln!("  {l:?}");
    }
    // Leg 1: to A, loading, to B, unloading; at B 0x557420 makes it B's loading ship for the
    // way back (state 2, leg 2, loading without state 4), to A, unloading, free. Foreign
    // throughout (0x591f80).
    let states: Vec<(i32, i32, bool)> = r.log.iter().map(|l| (l.1, l.2, l.3)).collect();
    assert_eq!(&states[..], &[(2, 1, true), (4, 1, true), (1, 1, true), (3, 1, true), (2, 2, true), (1, 2, true), (3, 2, true), (0, 2, true)]);
    assert_eq!(r.boards_at_b, 16, "boards delivered to player 2");
    assert_eq!(r.stone_at_a, 16, "stone delivered to player 1");
    assert_eq!(foreign(6000).unwrap(), r, "two runs differ");
}

/// The target goes while the ship carries the boards (0x591740 through B's crush 0x554ca0 /
/// event 7 or 9): A, the living source, becomes the target, the ship sails back with its cargo
/// and unloads there; then it is free.
#[test]
fn ship_returns_when_the_target_goes() {
    let Some((mut w, a, b, sh, cmds)) = setup() else { return };
    let before = board_piles(&w, a);
    let (mut crushed, mut states) = (None, Vec::new());
    for t in 0..3000 {
        s4_sim::tick(&mut w, if t == 0 { &cmds } else { &[] });
        w.events.clear();
        let d = transport::data(&w, sh).unwrap().clone();
        if states.last() != Some(&d.state) {
            states.push(d.state);
        }
        if crushed.is_none() && d.state == 1 && d.total() == 16 {
            s4_sim::buildings::crush::crush_building(&mut w, b);
            crushed = Some(w.tick);
            let d = transport::data(&w, sh).unwrap();
            assert_eq!((d.target, d.to_target, d.total()), (a, true, 16), "back to A with the cargo");
        }
    }
    let d = transport::data(&w, sh).unwrap();
    eprintln!("crushed B at {crushed:?}, states {states:?}, A's boards {} → {}", before, board_piles(&w, a));
    assert!(crushed.is_some());
    assert_eq!((d.state, d.total()), (0, 0), "unloaded and free");
    assert_eq!(*states.last().unwrap(), 0);
    assert!(states.windows(2).any(|p| p == [1, 3]), "unloading at A: {states:?}");
}

/// The source goes while the ship sails there to load (0x591830): forgotten, its record goes,
/// state 0, home; nothing is carried.
#[test]
fn ship_gives_up_when_the_source_goes() {
    let Some((mut w, a, _b, sh, cmds)) = setup() else { return };
    let mut crushed = None;
    for t in 0..600 {
        s4_sim::tick(&mut w, if t == 0 { &cmds } else { &[] });
        w.events.clear();
        let d = transport::data(&w, sh).unwrap();
        if crushed.is_none() && d.state == 2 {
            s4_sim::buildings::crush::crush_building(&mut w, a);
            crushed = Some(w.tick);
            let d = transport::data(&w, sh).unwrap();
            assert_eq!((d.state, d.source, d.total()), (0, 0, 0), "free at once");
        }
    }
    assert!(crushed.is_some(), "the ship was ordered");
    let d = transport::data(&w, sh).unwrap();
    assert_eq!((d.state, d.total()), (0, 0));
}

/// A new trade target while the ship loads (5005 → 0x553f30): the loading ship is redirected and
/// delivers the boards to the new port C.
#[test]
fn loading_ship_follows_a_new_target() {
    let Some((mut w, a, b, sh, cmds)) = setup() else { return };
    let wa = |w: &World, m: u16| match &w.entities.by_index(m as u32).unwrap().data {
        EntityData::Building(d) => (d.working_area.x(), d.working_area.y()),
        _ => (0, 0),
    };
    let (pa, pb) = (wa(&w, a), wa(&w, b));
    let far = |p: (i32, i32), q: (i32, i32)| s4_sim::spiral::grid_distance(p.0 - q.0, p.1 - q.1) >= 20;
    let Some(&(tc, cx, cy, _)) = port_sites(&w).iter().find(|s| far(s.3, pa) && far(s.3, pb)) else {
        eprintln!("skipping: no third port site");
        return;
    };
    let c = s4_sim::create::add_building(&mut w, cx, cy, 1, tc).unwrap();
    s4_sim::buildings::role_mut(&mut w, c).unwrap().base.worker_in_house = true;
    let (mut redirected, mut at_c) = (None, 0);
    let mut before = board_piles(&w, c);
    for t in 0..4000 {
        let mut now = if t == 0 { cmds.clone() } else { Vec::new() };
        if redirected.is_none() && transport::data(&w, sh).unwrap().state == 4 {
            now.push(PlayerCommand { player: 1, command: Command::SetTradeTarget { building: a as u32, target: c as u32 } });
            redirected = Some(w.tick);
        }
        s4_sim::tick(&mut w, &now);
        w.events.clear();
        let n = board_piles(&w, c);
        at_c += (n - before).max(0);
        before = n;
    }
    let d = transport::data(&w, sh).unwrap();
    eprintln!("redirected at {redirected:?}: boards at C {at_c}, at B {}", board_piles(&w, b));
    assert!(redirected.is_some());
    assert_eq!(at_c, 16, "C got the boards");
    assert_eq!(board_piles(&w, b), 0, "B none");
    assert_eq!((d.state, d.total()), (0, 0));
}
