//! Trade carts (`CCart` with `CTradingBuildingRole` 0x5562b0 / 0x556c70 / 0x5565e0 / 0x5543b0 /
//! 0x5567a0 / 0x556980, the cart's 0x55c690 / 0x55ccf0 / 0x55cc80 / 0x55c920 / 0x55b8a0).
//!
//! * `cart_carries_goods_between_markets`: roman02, two marketplaces of player 1 and a finished
//!   cart near A; A sends 20 boards (from 17 goods a cart is ordered first, 0x554170) to B. The
//!   cart drives to A's ensign, loads, drives to B, unloads into B's piles and goes back to where
//!   it stood when it left A.
//! * `cart_transports_in_the_saves`: every cart transport a trading record of the saves lists
//!   (chunk 162, kind 3) is a cart (chunk 166) that names the building as its source (+0x11c,
//!   state 2) or target (+0x118, states 1 / 3); a loading / unloading cart slot names a cart in
//!   state 4 / 3.
//!
//! The invariants checked every tick (state = cart +0x120):
//! * 0 no trade; 2 to the source: recorded there (kind 3), +0x110; 4 loading: the source's
//!   loading cart (+0x28c / +0x290); 1 to the target: recorded there, +0x112; 3 unloading: the
//!   target's unloading cart (+0x294 / +0x291);
//! * slots: amounts 0..=8, a good exactly when the amount is not 0; VULNERABLE (0x30000) exactly
//!   with cargo.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use s4_sim::buildings::trading;
use s4_sim::entity::{EntityData, flag};
use s4_sim::vehicles::{self, cart, kind};
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

fn check(w: &World, c: u16, a: u16, b: u16) -> Vec<String> {
    let mut f = Vec::new();
    let Some(cd) = cart::data(w, c) else { return vec!["no cart data".into()] };
    let flags = w.entities.by_index(c as u32).map_or(0, |e| e.flags);
    let recorded = |m: u16| trading::state(w, m).is_some_and(|s| s.transports.iter().any(|t| t.entity == c as u32 && t.kind == trading::transport::CART));
    let (sa, sb) = (trading::state(w, a), trading::state(w, b));
    match cd.state {
        0 => {}
        2 => {
            if !(cd.source == a as i32 && recorded(a) && cd.to_source) {
                f.push(format!("state 2: source {} recorded {} +0x110 {}", cd.source, recorded(a), cd.to_source));
            }
        }
        4 => {
            if !sa.is_some_and(|s| s.cart_load.entity == c as u32 && s.cart_load.on) {
                f.push("state 4: not A's loading cart".into());
            }
        }
        1 => {
            if !(cd.target == b as i32 && recorded(b)) {
                f.push(format!("state 1: target {} recorded {}", cd.target, recorded(b)));
            }
        }
        3 => {
            if !sb.is_some_and(|s| s.cart_unload.entity == c as u32 && s.cart_unload.on) {
                f.push("state 3: not B's unloading cart".into());
            }
        }
        s => f.push(format!("state {s}")),
    }
    for s in &cd.slots {
        if !(0..=8).contains(&s[1]) || (s[1] != 0) != (s[0] != 0) {
            f.push(format!("slot {s:?}"));
        }
    }
    if (flags & flag::VULNERABLE_MASK != 0) != cd.has_cargo() {
        f.push(format!("vulnerable {:x} with cargo {}", flags & flag::VULNERABLE_MASK, cd.has_cargo()));
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
    /// Where the cart ends, and A's ensign: it goes back to where it stood when it left A
    /// (+0x114 is set by every move order, 0x55c170), i.e. near A's ensign.
    home: Option<(i32, i32)>,
    a_ensign: (i32, i32),
}

fn board_piles(w: &World, m: u16) -> i32 {
    let Some(s) = trading::state(w, m) else { return 0 };
    s.piles.iter().filter_map(|&p| pile::get(w, p)).filter(|d| d.good == BOARD as u8 && d.trade_type == 2).map(|d| d.amount as i32).sum()
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = roman02()?;
    let (cx, cy) = {
        let pl = w.player(1)?;
        (pl.start_x as i32, pl.start_y as i32)
    };
    let size = w.grid.size() as i32;
    let market_type = trading::MARKETPLACE as u8;
    let sites = s4_sim::build::possible_sites(&w, 1, market_type, (0, 0), (size, size));
    let &(ax, ay, _) = sites.iter().min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs())?;
    let a = s4_sim::create::add_building(&mut w, ax as i32, ay as i32, 1, market_type).ok()?;
    let sites = s4_sim::build::possible_sites(&w, 1, market_type, (0, 0), (size, size));
    let &(bx, by, _) = sites.iter().filter(|s| (15..=30).contains(&s4_sim::spiral::grid_distance(s.0 as i32 - ax as i32, s.1 as i32 - ay as i32))).min_by_key(|s| (s.0 as i32 - ax as i32).abs() + (s.1 as i32 - ay as i32).abs())?;
    let b = s4_sim::create::add_building(&mut w, bx as i32, by as i32, 1, market_type).ok()?;
    // A finished cart a few nodes from A's ensign, where a cart may stand.
    let ens = match &w.entities.by_index(a as u32)?.data {
        EntityData::Building(d) => d.ensign,
        _ => return None,
    };
    let (vx, vy) = s4_sim::spiral::walk(w.grid, ens.x() + 7, ens.y() + 5, 12).find(|&(x, y)| {
        let n = w.grid.index(x as u32, y as u32);
        w.owner_of(n) == 1 && vehicles::can_place(&w, x, y, kind::CART) && w.tiling.sector_of(n) == w.tiling.sector_of(w.grid.index(ens.x() as u32, ens.y() as u32))
    })?;
    let c = vehicles::add_vehicle(&mut w, vx, vy, 1, kind::CART, -1, false)?;
    assert!(cart::is_free(&w, c), "a free cart");
    let cmds = [
        PlayerCommand { player: 1, command: Command::TransportGood { building: a, good: BOARD, amount: 20 } },
        PlayerCommand { player: 1, command: Command::SetTradeTarget { building: a as u32, target: b as u32 } },
    ];
    let (mut states, mut at, mut delivered, mut max_load) = (vec![0], vec![0u32], 0, 0);
    let mut before = board_piles(&w, b);
    for t in 0..ticks {
        let cm = if t == 0 { cmds.to_vec() } else { vec![] };
        s4_sim::tick(&mut w, &cm);
        for e in std::mem::take(&mut w.events) {
            if let s4_sim::event::SimEvent::Unported { what, .. } = e {
                assert!(!what.contains("cart") && !what.contains("CCart"), "unported: {what}");
            }
        }
        let f = check(&w, c, a, b);
        assert!(f.is_empty(), "tick {}: {f:?}", w.tick);
        let cd = cart::data(&w, c)?;
        if *states.last()? != cd.state {
            states.push(cd.state);
            at.push(w.tick);
        }
        max_load = max_load.max(cd.slots.iter().map(|s| s[1]).sum());
        let now = board_piles(&w, b);
        delivered += (now - before).max(0);
        before = now;
    }
    let e = w.entities.by_index(c as u32)?;
    let home = Some((e.x(), e.y()));
    eprintln!("cart placed at {:?}", (vx, vy));
    Some(Outcome { hash: w.state_hash(), states, at, delivered, max_load, send: trading::state(&w, a)?.send[BOARD as usize], home, a_ensign: (ens.x(), ens.y()) })
}

#[test]
fn cart_carries_goods_between_markets() {
    let Some(r) = run(12_000) else {
        eprintln!("skipping: no game data or no places for the markets / the cart");
        return;
    };
    eprintln!("{r:?}");
    assert!(r.states.len() >= 6, "states {:?}", r.states);
    assert_eq!(&r.states[..6], &[0, 2, 4, 1, 3, 0], "trade states");
    assert_eq!(r.delivered, 20, "boards put into B's piles");
    assert_eq!(r.send, 0, "nothing left to send");
    assert!(r.max_load > 8, "more than one slot used: {}", r.max_load);
    let (h, e) = (r.home.unwrap(), r.a_ensign);
    assert!(s4_sim::spiral::grid_distance(h.0 - e.0, h.1 - e.1) <= 3, "back near A's ensign: {h:?} {e:?}");
    assert_eq!(run(12_000).unwrap(), r, "two runs differ");
}

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

/// The trading records of chunk 162: building id → (13 slot words +0x270.., transports).
fn trading_records(t: &[i64]) -> HashMap<i64, (Vec<i64>, Vec<[i64; 3]>)> {
    let mut out = HashMap::new();
    let mut k = 0;
    while k + 10 <= t.len() {
        let v = &t[k..k + 10];
        let head = v[1] == 2 && v[6] == 8 && (32..=33).contains(&v[3]) && (1..0xFFFF).contains(&v[2]) && v[8] & 0x10000 != 0;
        if !head {
            k += 1;
            continue;
        }
        let seg = &t[k + 10..(k + 900).min(t.len())];
        if let Some(j) = (0..seg.len().saturating_sub(4)).find(|&j| seg[j] == 4 && seg[j + 2] == 1 && seg[j + 3] == 43) {
            // 4, target, 3 tables of 45 around 8 bools, piles, 13 slot words, transports.
            let r = &seg[j..];
            let mut at = 2 + 45 + 45 + 8 + 45;
            let n = r.get(at).copied().unwrap_or(0).max(0) as usize;
            at += 1 + n;
            if let (Some(slots), Some(&m)) = (r.get(at..at + 13), r.get(at + 13)) {
                let tr = (0..m.max(0) as usize).filter_map(|i| r.get(at + 14 + 3 * i..at + 17 + 3 * i)).map(|x| [x[0], x[1], x[2]]).collect();
                out.insert(v[2], (slots.to_vec(), tr));
            }
        }
        k += 10;
    }
    out
}

#[test]
fn cart_transports_in_the_saves() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let (mut transports, mut slots) = (0, 0);
    let mut failures = Vec::new();
    for f in &files {
        let Some((vs, _, _)) = vehicles::save::load(f).unwrap() else { continue };
        let carts: HashMap<i64, vehicles::save::SavedCart> = vs.iter().filter(|v| v.kind == 5).filter_map(|v| Some((v.id as i64, vehicles::save::parse_cart(&v.tail)?))).collect();
        let m = s4_formats::map::MapFile::open(f).unwrap();
        let Some((_, d)) = m.container.load_id(162, 0).unwrap() else { continue };
        for (b, (sl, tr)) in trading_records(&tokens(&d)) {
            let what = format!("{} building {b}", f.display());
            for t in tr.iter().filter(|t| t[1] == 3) {
                transports += 1;
                let Some(c) = carts.get(&t[0]) else {
                    failures.push(format!("{what}: cart transport {} is no cart", t[0]));
                    continue;
                };
                let [_, target, source, state] = c.i114;
                let ok = (state == 2 && source == b) || (matches!(state, 1 | 3) && target == b);
                if !ok {
                    failures.push(format!("{what}: cart {} target {target} source {source} state {state}", t[0]));
                }
            }
            // Slot words +0x270, +0x274, +0x278, +0x279, +0x27c, +0x280, +0x284, +0x285, +0x288,
            // +0x28c, +0x290, +0x291, +0x294: the loading cart 9 / 10, the unloading one 12 / 11.
            for (e, on, want) in [(sl[9], sl[10], 4), (sl[12], sl[11], 3)] {
                if on != 0 {
                    slots += 1;
                    if carts.get(&e).is_none_or(|c| c.i114[3] != want) {
                        failures.push(format!("{what}: cart slot {e} state {:?}, want {want}", carts.get(&e).map(|c| c.i114)));
                    }
                }
            }
        }
    }
    eprintln!("{transports} cart transports, {slots} busy cart slots");
    for x in failures.iter().take(20) {
        eprintln!("  {x}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}
