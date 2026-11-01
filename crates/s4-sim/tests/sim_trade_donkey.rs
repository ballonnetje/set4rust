//! Donkey trade transports (`CDonkeyRole` event 10 / `JOB_DONKEY_TRANSPORT_GOOD` 163 and the
//! market side 0x5558b0 / 0x5561f0 / 0x555b60 / 0x5543b0 / 0x555e40 / 0x555ed0).
//!
//! * `donkey_records_hold`: the donkey records (chunk 161, `CDonkeyRole` store version 5
//!   0x5761b0) and the market records (chunk 162) of the `H8GQE2M0_*` saves satisfy the order
//!   invariants below; the same check runs on the simulation every tick.
//! * `donkey_carries_goods_between_markets`: roman02, two marketplaces of player 1 and a donkey;
//!   market A sends 10 boards to market B. The donkey walks to A, loads, walks to B, unloads
//!   into B's piles (which offer them, trade role 2) and is free again.
//!
//! The invariants (state = role +0x40):
//! * 0 free: the wait list (60) or the walk home; 5 a vehicle order (not checked further);
//! * 2 to the source: list 163 at GO_TO_SOURCE_PILE, recorded by the source, slots empty;
//! * 4 loading: list 163 at the wait task 22, +0x2e set, the source's loading donkey (+0x280 /
//!   +0x284), displacement 10;
//! * 1 to the target: list 163 at 22 (sent off, not updated yet) or walking at
//!   GO_TO_DESTINATION_PILE (+0x2f set, recorded by the target, vulnerable 0x30000 exactly while
//!   loaded);
//! * 3 unloading: list 163 at PUT_GOOD 21, the target's unloading donkey (+0x288 / +0x285);
//! * slots: amounts 0..=8, a good exactly when the amount is not 0.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use s4_sim::buildings::trading;
use s4_sim::entity::{EntityData, ObjType};
use s4_sim::jobs::save_settler;
use s4_sim::{Command, PlayerCommand, World, pile};

const DONKEY: u16 = 60;
const BOARD: u16 = 7;

/// What the check needs of a donkey.
#[derive(Clone, Debug)]
struct Donkey {
    id: u16,
    flags: u32,
    list: i16,
    cursor: Option<u16>,
    task: u8,
    displacement: u8,
    to_source: bool,
    to_target: bool,
    target: u16,
    source: u16,
    state: u32,
    slots: [(i64, i64); 2],
    /// Event 10 is queued (the order is not taken yet).
    pending: bool,
}

/// What the check needs of a market.
#[derive(Clone, Debug, Default)]
struct Market {
    transports: Vec<(u16, u32)>,
    loading: (u16, bool),
    unloading: (u16, bool),
}

fn check(d: &Donkey, markets: &HashMap<u16, Market>) -> Vec<String> {
    let mut f = Vec::new();
    let mut fail = |s: String| f.push(format!("donkey {}: {s} ({d:?})", d.id));
    for &(g, n) in &d.slots {
        if !(0..=8).contains(&n) || (n == 0) != (g == 0) {
            fail(format!("slot {g} x {n}"));
        }
    }
    let loaded = d.slots.iter().any(|s| s.1 != 0);
    let recorded = |m: u16| markets.get(&m).is_some_and(|m| m.transports.contains(&(d.id, 2)));
    let in163 = d.list == 163;
    match d.state {
        0 => {
            if d.list != DONKEY as i16 && d.task != 6 {
                fail("free but not waiting / walking".into());
            }
        }
        2 if d.pending && d.list == DONKEY as i16 => {
            if !recorded(d.source) {
                fail("not recorded by the source".into());
            }
        }
        2 => {
            if !in163 || d.cursor != Some(0) || d.source == 0 || d.target == 0 || loaded {
                fail("to the source".into());
            }
            if !recorded(d.source) {
                fail("not recorded by the source".into());
            }
        }
        4 => {
            if !in163 || d.cursor != Some(1) || d.task != 22 || !d.to_source || d.displacement != 10 {
                fail("loading".into());
            }
            if markets.get(&d.source).is_none_or(|m| m.loading != (d.id, true)) {
                fail("not the source's loading donkey".into());
            }
        }
        1 => {
            let sent = d.cursor == Some(1) && d.task == 22;
            let walking = d.cursor == Some(2) && d.task == 6 && d.to_target;
            if !in163 || !(sent || walking) || d.target == 0 {
                fail("to the target".into());
            }
            if walking && !recorded(d.target) {
                fail("not recorded by the target".into());
            }
            if walking && (d.flags & 0x30000 == 0x30000) != loaded {
                fail("vulnerable flags".into());
            }
        }
        3 => {
            if !in163 || d.cursor != Some(3) || d.task != 21 {
                fail("unloading".into());
            }
            if markets.get(&d.target).is_none_or(|m| m.unloading != (d.id, true)) {
                fail("not the target's unloading donkey".into());
            }
        }
        5 => {}
        s => fail(format!("state {s}")),
    }
    f
}

// ---- the saves ----

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

/// The trading records of chunk 162 (see `sim_trade_save.rs`): building id → market.
fn saved_markets(b: &[i64]) -> HashMap<u16, Market> {
    let mut out = HashMap::new();
    let mut k = 0;
    while k + 10 <= b.len() {
        let v = &b[k..k + 10];
        let head = v[1] == 2 && v[6] == 8 && (1..0xFFFF).contains(&v[2]) && (v[3] == 32 || v[3] == 33) && v[8] & 0x10000 != 0 && (1..=8).contains(&(v[7] >> 4));
        if !head {
            k += 1;
            continue;
        }
        let id = v[2] as u16;
        let seg = &b[k + 10..(k + 900).min(b.len())];
        k += 10;
        let Some(j) = (0..seg.len().saturating_sub(4)).find(|&j| seg[j] == 4 && seg[j + 2] == 1 && seg[j + 3] == 43) else { continue };
        let r = &seg[j..];
        // 4, target, 43-table, 43-table, 8 bools, 43-table, n piles, 13 slot words, m transports.
        let mut at = 2 + 45 + 45 + 8 + 45;
        let n = r[at] as usize;
        at += 1 + n;
        let s = &r[at..at + 13];
        at += 13;
        let m = r[at] as usize;
        let transports = (0..m).map(|i| (r[at + 1 + 3 * i] as u16, r[at + 2 + 3 * i] as u32)).collect();
        // +0x270, +0x274, +0x278, +0x279, +0x27c, +0x280, +0x284, +0x285, +0x288, ...
        out.insert(id, Market { transports, loading: (s[5] as u16, s[6] != 0), unloading: (s[8] as u16, s[7] != 0) });
    }
    out
}

/// A donkey record: the role tail is `5, +0x2c, +0x2d, +0x2e, +0x2f, +0x30, +0x34, +0x38, +0x3c,
/// +0x40, +0x44, +0x48, +0x4a, +0x4c, 2 × (+0x54, +0x58, +0x50), +0x68, +0x6c, +0x74, +0x78`.
fn saved_donkey(s: &save_settler::SavedSettler) -> Option<Donkey> {
    let t = &s.role_tail;
    if t.len() < 24 || t[0] != 5 || t[5] != s.id as i64 {
        return None;
    }
    Some(Donkey {
        id: s.id,
        flags: s.flags,
        list: s.list_id,
        cursor: s.cursor,
        task: s.task(),
        displacement: s.displacement_costs,
        to_source: t[3] != 0,
        to_target: t[4] != 0,
        target: t[6] as u16,
        source: t[7] as u16,
        state: t[9] as u32,
        slots: [(t[16], t[14]), (t[19], t[17])],
        pending: s.event_list.iter().any(|e| e[0] == 10),
    })
}

#[test]
fn donkey_records_hold() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let (mut donkeys, mut ordered) = (0, HashMap::<u32, usize>::new());
    let mut failures = Vec::new();
    for f in &files {
        let m = s4_formats::map::MapFile::open(f).unwrap();
        let markets = saved_markets(&tokens(&m.container.load_id(162, 0).unwrap().unwrap().1));
        let (settlers, _) = save_settler::load(f).unwrap();
        for s in settlers.iter().filter(|s| s.kind == DONKEY) {
            let Some(d) = saved_donkey(s) else {
                failures.push(format!("{}: donkey {} record {:?}", f.display(), s.id, s.role_tail));
                continue;
            };
            donkeys += 1;
            *ordered.entry(d.state).or_default() += 1;
            failures.extend(check(&d, &markets).into_iter().map(|x| format!("{}: {x}", f.display())));
        }
    }
    eprintln!("{} saves: {donkeys} donkey records, by state {ordered:?}", files.len());
    for x in failures.iter().take(20) {
        eprintln!("  {x}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}

// ---- the simulation ----

fn sim_donkey(w: &World, id: u16) -> Option<Donkey> {
    let e = w.entities.by_index(id as u32)?;
    let EntityData::Settler(s) = &e.data else { return None };
    let j = &s.job;
    let d = j.donkey.as_deref().copied().unwrap_or_default();
    Some(Donkey {
        id,
        flags: e.flags,
        list: j.list_id,
        cursor: (j.list_id != -1).then_some(j.cursor),
        task: j.task,
        displacement: j.displacement_costs,
        to_source: d.to_source,
        to_target: d.to_target,
        target: d.target,
        source: d.source,
        state: d.state,
        slots: [(d.slots[0].0 as i64, d.slots[0].1 as i64), (d.slots[1].0 as i64, d.slots[1].1 as i64)],
        pending: j.events.iter().any(|e| e.event == 10),
    })
}

fn sim_markets(w: &World, ids: &[u16]) -> HashMap<u16, Market> {
    ids.iter()
        .filter_map(|&m| {
            let s = trading::state(w, m)?;
            Some((
                m,
                Market {
                    transports: s.transports.iter().map(|t| (t.entity as u16, t.kind)).collect(),
                    loading: (s.donkey_load.entity as u16, s.donkey_load.on),
                    unloading: (s.donkey_unload.entity as u16, s.donkey_unload.on),
                },
            ))
        })
        .collect()
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

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    /// The donkey's order states in the order they came.
    states: Vec<u32>,
    /// Ticks the states began.
    at: Vec<u32>,
    /// Boards put into B's piles (sum of increases).
    delivered: i32,
    /// Most goods the donkey carried at once.
    max_load: i64,
    /// +0x1ab boards at the end.
    send: i8,
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
    // B: a place 15..30 nodes from A.
    let sites = s4_sim::build::possible_sites(&w, 1, market_type, (0, 0), (size, size));
    let &(bx, by, _) = sites.iter().filter(|s| (15..=30).contains(&s4_sim::spiral::grid_distance(s.0 as i32 - ax as i32, s.1 as i32 - ay as i32))).min_by_key(|s| (s.0 as i32 - ax as i32).abs() + (s.1 as i32 - ay as i32).abs())?;
    let b = s4_sim::create::add_building(&mut w, bx as i32, by as i32, 1, market_type).ok()?;
    // A donkey a few nodes from A's ensign.
    let ens = match &w.entities.by_index(a as u32)?.data {
        EntityData::Building(d) => d.ensign,
        _ => return None,
    };
    let (dx, dy) = s4_sim::spiral::walk(w.grid, ens.x() + 6, ens.y() + 6, 10).find(|&(x, y)| {
        let n = w.grid.index(x as u32, y as u32);
        w.layers.map_object[n] == 0 && w.owner_of(n) == 1 && w.tiling.sector_of(n) == w.tiling.sector_of(w.grid.index(ens.x() as u32, ens.y() as u32))
    })?;
    let donkey = s4_sim::create::add_settler(&mut w, dx, dy, 1, DONKEY)?;
    assert!(w.entities.by_index(donkey as u32).is_some_and(|e| e.obj_type == ObjType::Settler));
    let cmds = [
        PlayerCommand { player: 1, command: Command::TransportGood { building: a, good: BOARD, amount: 10 } },
        PlayerCommand { player: 1, command: Command::SetTradeTarget { building: a as u32, target: b as u32 } },
    ];
    let (mut states, mut at, mut delivered, mut max_load) = (vec![0u32], vec![0u32], 0, 0);
    let mut before = board_piles(&w, b);
    for t in 0..ticks {
        let c = if t == 0 { cmds.to_vec() } else { vec![] };
        s4_sim::tick(&mut w, &c);
        for e in &w.events {
            if let s4_sim::event::SimEvent::Unported { what, .. } = e {
                assert!(!what.contains("onkey"), "unported: {what}");
            }
        }
        let d = sim_donkey(&w, donkey)?;
        let markets = sim_markets(&w, &[a, b]);
        let f = check(&d, &markets);
        assert!(f.is_empty(), "tick {}: {f:?}", w.tick);
        if *states.last()? != d.state {
            states.push(d.state);
            at.push(w.tick);
        }
        max_load = max_load.max(d.slots[0].1 + d.slots[1].1);
        let now = board_piles(&w, b);
        delivered += (now - before).max(0);
        before = now;
    }
    Some(Outcome { hash: w.state_hash(), states, at, delivered, max_load, send: trading::state(&w, a)?.send[BOARD as usize] })
}

#[test]
fn donkey_carries_goods_between_markets() {
    let Some(r) = run(9_000) else {
        eprintln!("skipping: no game data or no places for the markets");
        return;
    };
    eprintln!("{r:?}");
    assert_eq!(&r.states[..6], &[0, 2, 4, 1, 3, 0], "order states");
    assert_eq!(r.max_load, 10, "the donkey carried all ten boards");
    assert_eq!(r.delivered, 10, "boards put into B's piles");
    assert_eq!(r.send, 0, "nothing left to send");
    assert_eq!(run(9_000).unwrap(), r, "two runs differ");
}
