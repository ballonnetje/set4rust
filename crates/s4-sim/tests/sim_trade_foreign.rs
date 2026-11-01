//! The foreign trade (`CInternationalTrader`, `buildings::trader`, and the trading role's foreign
//! parts).
//!
//! * `chunk_167_records_hold`: chunk 167 of every save parses to the byte; its records are the
//!   finished ports / marketplaces of their owner (chunk 162), and each record's good states are
//!   the role's +0x1d6 bits 2 / 3 (`TradeGood` 0x5530f0 writes both).
//! * `commands_cycle_the_states`: roman02, a marketplace of player 1: `Init` records it in the
//!   player's entry; 5008 cycles a good 0 → 4 → 8 → 0 in the role and the record; 5007 toggles a
//!   trade-with flag, 5050 sets / clears it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use s4_sim::buildings::{trader, trading};
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

fn saves() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE") else { return vec![] };
    let mut stack = vec![Path::new(&home).join("Documents").join("TheSettlers4").join("Save")];
    let mut v = Vec::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "sav") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

fn tokens(d: &[u8]) -> Vec<i64> {
    d.split(|c| c.is_ascii_whitespace() || *c == 0).filter(|t| !t.is_empty()).map(|t| std::str::from_utf8(t).ok().and_then(|s| s.parse().ok()).unwrap_or(i64::MIN)).collect()
}

/// The finished trading buildings of chunk 162: id → (owner, type, +0x1d6 flags).
fn trading_buildings(b: &[i64]) -> HashMap<u16, (u8, u16, Vec<i64>)> {
    let mut out = HashMap::new();
    let mut k = 0;
    while k + 10 <= b.len() {
        let v = &b[k..k + 10];
        let head = v[1] == 2 && v[6] == 8 && (1..0xFFFF).contains(&v[2]) && (v[3] == 32 || v[3] == 33) && v[8] & 0x10000 != 0 && (1..=8).contains(&(v[7] >> 4));
        if !head {
            k += 1;
            continue;
        }
        let seg = &b[k + 10..(k + 900).min(b.len())];
        if let Some(j) = (0..seg.len().saturating_sub(4)).find(|&j| seg[j] == 4 && seg[j + 2] == 1 && seg[j + 3] == 43) {
            // 4, target, 43-table, 43-table, 8 bools, 1 43 [flags].
            let at = j + 2 + 45 + 45 + 8;
            if seg.get(at..at + 2) == Some(&[1, 43]) {
                out.insert(v[2] as u16, ((v[7] >> 4) as u8, v[3] as u16, seg[at + 2..at + 45].to_vec()));
            }
        }
        k += 10;
    }
    out
}

#[test]
fn chunk_167_records_hold() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let (mut tables, mut records, mut states) = (0, 0, 0);
    let mut failures = Vec::new();
    for f in &files {
        let m = s4_formats::map::MapFile::open(f).unwrap();
        let Some((_, c)) = m.container.load_id(167, 0).unwrap() else {
            failures.push(format!("{}: no chunk 167", f.display()));
            continue;
        };
        let Some(t) = trader::parse_chunk(&c) else {
            failures.push(format!("{}: chunk 167 does not parse ({} bytes)", f.display(), c.len()));
            continue;
        };
        tables += 1;
        let blds = trading_buildings(&tokens(&m.container.load_id(162, 0).unwrap().unwrap().1));
        for (p, e) in t.iter().enumerate() {
            for r in e.ports.iter().chain(&e.markets) {
                records += 1;
                let what = format!("{} player {p} record {}", f.display(), r.building);
                match blds.get(&r.building) {
                    Some((owner, kind, flags)) if *owner as usize == p && *kind == r.kind => {
                        for (g, &(good, st)) in r.goods.iter().enumerate() {
                            if st != 0 {
                                states += 1;
                                if good as usize != g {
                                    failures.push(format!("{what}: good {g} field {good}"));
                                }
                            }
                            if (flags[g] & 0xc) as u8 != st {
                                failures.push(format!("{what}: good {g} state {st}, role flags {}", flags[g]));
                            }
                        }
                    }
                    b => failures.push(format!("{what}: building {b:?}, record type {}", r.kind)),
                }
            }
            // Every finished trading building of the player has a record.
            for (id, (owner, kind, _)) in &blds {
                if *owner as usize == p && !e.ports.iter().chain(&e.markets).any(|r| r.building == *id && r.kind == *kind) {
                    failures.push(format!("{} player {p}: building {id} has no record", f.display()));
                }
            }
        }
    }
    eprintln!("{} saves: {tables} trader tables, {records} records, {states} goods with a state", files.len());
    for x in failures.iter().take(20) {
        eprintln!("  {x}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}

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

#[test]
fn commands_cycle_the_states() {
    let Some(mut w) = roman02() else { return };
    let (cx, cy) = {
        let pl = w.player(1).unwrap();
        (pl.start_x as i32, pl.start_y as i32)
    };
    let size = w.grid.size() as i32;
    let sites = s4_sim::build::possible_sites(&w, 1, trading::MARKETPLACE as u8, (0, 0), (size, size));
    let Some(&(mx, my, _)) = sites.iter().min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs()) else { return };
    let m = s4_sim::create::add_building(&mut w, mx as i32, my as i32, 1, trading::MARKETPLACE as u8).unwrap();
    let t = &w.buildings.traders;
    assert!(t.made, "the table is made");
    assert_eq!(t.players.len(), w.players.iter().rposition(|p| p.is_some()).unwrap() + 1, "one entry per player 0..=PlayerCount");
    assert_eq!(t.get(1).unwrap().markets.iter().map(|r| r.building).collect::<Vec<_>>(), vec![m], "Init records the market");
    let cmd = |command| vec![PlayerCommand { player: 1, command }];
    let board = 7u32;
    let state = |w: &World| (trading::state(w, m).unwrap().flags[board as usize], w.buildings.traders.get(1).unwrap().record(m, 33).unwrap().goods[board as usize]);
    let mut seen = Vec::new();
    for _ in 0..3 {
        s4_sim::tick(&mut w, &cmd(Command::TradeGood { building: m as u32, value: board }));
        seen.push(state(&w));
    }
    assert_eq!(seen, vec![(4, (7, 4)), (8, (7, 8)), (0, (7, 0))], "0 → import → export → 0");
    s4_sim::tick(&mut w, &cmd(Command::TradeWith { building: m as u32, value: 2 }));
    assert!(w.buildings.traders.get(1).unwrap().trade_with[2], "5007 toggles on");
    s4_sim::tick(&mut w, &cmd(Command::TradeWith { building: m as u32, value: 2 }));
    assert!(!w.buildings.traders.get(1).unwrap().trade_with[2], "5007 toggles off");
    s4_sim::tick(&mut w, &cmd(Command::SetTradeStatus { building: m, player: 3, value: 1 }));
    assert!(w.buildings.traders.get(1).unwrap().trade_with[3], "5050 sets");
    s4_sim::tick(&mut w, &cmd(Command::SetTradeStatus { building: m, player: 3, value: 0 }));
    assert!(!w.buildings.traders.get(1).unwrap().trade_with[3], "5050 clears");
}

/// roman02, player 1's marketplace exports boards and trades with player 2 (no partner): the
/// matching tops +0x201 / +0x22c up to 16 with the export bit (0x4ef5d0), free piles where the
/// eco-sector offers boards collect them as role 3 (0x557a40), carriers bring them and each
/// arrival counts +0x201 down.
#[test]
fn exports_are_collected() {
    let Some(mut w) = roman02() else { return };
    let (cx, cy) = {
        let pl = w.player(1).unwrap();
        (pl.start_x as i32, pl.start_y as i32)
    };
    let Some(m) = market_near(&mut w, 1, cx, cy) else { return };
    let c = |command| PlayerCommand { player: 1, command };
    let start = vec![
        c(Command::TradeGood { building: m as u32, value: BOARD as u32 }),
        c(Command::TradeGood { building: m as u32, value: BOARD as u32 }),
        c(Command::TradeWith { building: m as u32, value: 2 }),
    ];
    let mut first = None;
    let mut max_piles = 0;
    for t in 0..6_000 {
        s4_sim::tick(&mut w, if t == 0 { &start } else { &[] });
        let s = trading::state(&w, m).unwrap();
        let ps: Vec<_> = s.piles.iter().filter_map(|&p| s4_sim::pile::get(&w, p)).filter(|d| d.trade_type == 3 && d.good == BOARD).map(|d| d.amount as i32).collect();
        max_piles = max_piles.max(ps.len());
        if first.is_none() && ps.iter().sum::<i32>() > 0 {
            first = Some(w.tick);
        }
    }
    let s = trading::state(&w, m).unwrap();
    let stock: i32 = s.piles.iter().filter_map(|&p| s4_sim::pile::get(&w, p)).filter(|d| d.trade_type == 3 && d.good == BOARD).map(|d| d.amount as i32).sum();
    eprintln!("first board {first:?}, stock {stock}, +0x201 {}, +0x22c {}, flags {}, piles {max_piles}", s.foreign_collect[BOARD as usize], s.foreign_send[BOARD as usize], s.flags[BOARD as usize]);
    assert!(first.is_some(), "no board collected");
    assert_eq!(s.flags[BOARD as usize], 8, "export bit");
    assert_eq!(s.foreign_send[BOARD as usize], 16, "+0x22c topped up to 16");
    assert_eq!(stock + s.foreign_collect[BOARD as usize] as i32, 16, "stock + still to collect");
    assert!((1..=2).contains(&max_piles), "piles collecting: {max_piles}");
}

const BOARD: u8 = 7;
const STONE: u8 = 32;

fn sim_world(name: &str) -> Option<World> {
    let p = ["Multiplayer", "Singleplayer"].iter().map(|d| s4_formats::game_dir().join("Map").join(d).join(name)).find(|p| p.exists())?;
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

/// A marketplace of `player` at the possible place nearest to (x, y).
fn market_near(w: &mut World, player: u8, x: i32, y: i32) -> Option<u16> {
    let size = w.grid.size() as i32;
    let sites = s4_sim::build::possible_sites(w, player, trading::MARKETPLACE as u8, (0, 0), (size, size));
    let &(mx, my, _) = sites.iter().min_by_key(|s| (s.0 as i32 - x).abs() + (s.1 as i32 - y).abs())?;
    s4_sim::create::add_building(w, mx as i32, my as i32, player, trading::MARKETPLACE as u8).ok()
}

fn ensign(w: &World, b: u16) -> (i32, i32) {
    match &w.entities.by_index(b as u32).unwrap().data {
        EntityData::Building(d) => (d.ensign.x(), d.ensign.y()),
        _ => (0, 0),
    }
}

/// 16 goods into two free piles as foreign collecting piles (role 3), one good at a time (each
/// counts +0x201 down as a carrier's delivery would).
fn fill(w: &mut World, m: u16, good: u8) {
    let free: Vec<u16> = trading::state(w, m).unwrap().piles.iter().copied().filter(|&p| s4_sim::pile::get(w, p).is_some_and(|d| d.trade_type == 0)).take(2).collect();
    for p in free {
        s4_sim::pile::set_good_type(w, p, good);
        s4_sim::pile::set_trade_type(w, p, 3);
        for _ in 0..8 {
            s4_sim::pile::increase_unforeseen(w, p, 1);
        }
    }
}

fn received(w: &World, m: u16, good: u8) -> i32 {
    trading::state(w, m).unwrap().piles.iter().filter_map(|&p| s4_sim::pile::get(w, p)).filter(|d| d.good == good && d.trade_type == 2).map(|d| d.amount as i32).sum()
}

/// (tick, donkey state, leg, slots).
type Step = (u32, u32, u32, [(u8, i32); 2]);

#[derive(Debug, PartialEq, Eq)]
struct Exchange {
    hash: u64,
    partners: (u16, u16),
    /// (tick, state, leg, slots) whenever the donkey's state or leg changed.
    log: Vec<Step>,
    boards_at_b: i32,
    stone_at_a: i32,
}

fn exchange(ticks: u32) -> Option<Exchange> {
    let mut w = sim_world("zz_test_bigfight.map")?;
    let (p1, p2) = (w.player(1)?.clone(), w.player(2)?.clone());
    let a = market_near(&mut w, 1, p1.start_x as i32, p1.start_y as i32)?;
    let b = market_near(&mut w, 2, p2.start_x as i32, p2.start_y as i32)?;
    assert!(trader::same_sector(&w, a, b), "the markets share a sector");
    let (ex, ey) = ensign(&w, a);
    let sector = w.tiling.sector_of(w.grid.index(ex as u32, ey as u32));
    let (dx, dy) = s4_sim::spiral::walk(w.grid, ex + 4, ey + 4, 10).find(|&(x, y)| {
        let n = w.grid.index(x as u32, y as u32);
        w.layers.map_object[n] == 0 && w.owner_of(n) == 1 && w.tiling.sector_of(n) == sector
    })?;
    let donkey = s4_sim::create::add_settler(&mut w, dx, dy, 1, 60)?;
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
    let (mut boards_at_b, mut stone_at_a) = (0, 0);
    let (mut rb, mut ra) = (0, 0);
    for t in 0..ticks {
        s4_sim::tick(&mut w, if t == 0 { &start } else { &[] });
        // Once both have set up their exports (+0x22c = 16), the goods are "collected".
        if t == 30 {
            fill(&mut w, a, BOARD);
            fill(&mut w, b, STONE);
        }
        for e in &w.events {
            if let s4_sim::event::SimEvent::Unported { what, .. } = e {
                assert!(!what.contains("onkey") && !what.contains("oreign"), "unported: {what}");
            }
        }
        let d = s4_sim::jobs::donkey::data(&w, donkey).copied().unwrap_or_default();
        if (d.state, d.leg) != last {
            last = (d.state, d.leg);
            log.push((w.tick, d.state, d.leg, d.slots));
        }
        let (nb, na) = (received(&w, b, BOARD), received(&w, a, STONE));
        boards_at_b += (nb - rb).max(0);
        stone_at_a += (na - ra).max(0);
        (rb, ra) = (nb, na);
    }
    let partners = (trading::state(&w, a)?.partner, trading::state(&w, b)?.partner);
    Some(Exchange { hash: w.state_hash(), partners, log, boards_at_b, stone_at_a })
}

#[test]
fn donkey_exchanges_goods_between_players() {
    let Some(r) = exchange(4_000) else {
        eprintln!("skipping: no game data / map / places");
        return;
    };
    for l in &r.log {
        eprintln!("{l:?}");
    }
    eprintln!("partners {:?}, boards at B {}, stone at A {}", r.partners, r.boards_at_b, r.stone_at_a);
    let states: Vec<(u32, u32)> = r.log.iter().map(|l| (l.1, l.2)).collect();
    // Leg 1: to A, load boards, to B, unload; leg 2: load stone at B, to A, unload, free.
    assert_eq!(&states[..], &[(2, 1), (4, 1), (1, 1), (3, 1), (2, 2), (4, 2), (1, 2), (3, 2), (0, 2)], "states and legs");
    assert_eq!(r.boards_at_b, 16, "boards delivered to player 2");
    assert_eq!(r.stone_at_a, 16, "stone delivered to player 1");
    assert_eq!(exchange(4_000).unwrap(), r, "two runs differ");
}
