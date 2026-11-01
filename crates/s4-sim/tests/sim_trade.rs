//! The marketplace collecting goods for trade on roman02, driven by `tick()` only. A marketplace is
//! placed (like a map building) at the nearest possible place to player 1's start; the player asks it to
//! send 10 boards (command 5006). `CTradingBuildingRole` (0x553660) turns free trade piles into
//! collecting piles of boards (role 1) where the eco-sector offers boards, their expectations
//! make the piles request boards (`CTradePileRole::LogicUpdate`), and carriers bring them; each
//! arrival counts +0x180 down (`GoodArrive` 0x5522f0). Skipped without the game data.

use s4_sim::build::possible_sites;
use s4_sim::buildings::trading;
use s4_sim::entity::EntityData;
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

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    /// Boards on the market's collecting piles at the end.
    stock: i32,
    /// +0x180 / +0x1ab for boards at the end.
    collect: i8,
    send: i8,
    /// Most piles collecting boards at once.
    max_piles: i32,
    /// Tick the first board arrived.
    first: Option<u32>,
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = roman02()?;
    let (cx, cy) = {
        let pl = w.player(1)?;
        (pl.start_x as i32, pl.start_y as i32)
    };
    let size = w.grid.size() as i32;
    let market_type = trading::MARKETPLACE as u8;
    let sites = possible_sites(&w, 1, market_type, (0, 0), (size, size));
    let &(mx, my, _) = sites.iter().min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs())?;
    let market = s4_sim::create::add_building(&mut w, mx as i32, my as i32, 1, market_type).ok()?;
    assert!(trading::state(&w, market).is_some(), "a trading role");
    let order = PlayerCommand { player: 1, command: Command::TransportGood { building: market, good: BOARD, amount: 10 } };
    let (mut max_piles, mut first) = (0, None);
    for t in 0..ticks {
        let cmds = if t == 0 { vec![order.clone()] } else { vec![] };
        s4_sim::tick(&mut w, &cmds);
        let s = trading::state(&w, market)?;
        let n = s.piles.iter().filter(|&&p| pile::get(&w, p).is_some_and(|d| matches!(d.trade_type, 1 | 3) && d.good == BOARD as u8)).count() as i32;
        max_piles = max_piles.max(n);
        if first.is_none() && trading::stock(&w, market, BOARD as u8) > 0 {
            first = Some(w.tick);
        }
    }
    let s = trading::state(&w, market)?;
    Some(Outcome {
        hash: w.state_hash(),
        stock: trading::stock(&w, market, BOARD as u8),
        collect: s.collect[BOARD as usize],
        send: s.send[BOARD as usize],
        max_piles,
        first,
    })
}

#[test]
fn market_collects_the_goods_to_send() {
    let Some(r) = run(6_000) else {
        eprintln!("skipping: no game data or no place for a marketplace");
        return;
    };
    eprintln!("{r:?}");
    assert!(r.first.is_some(), "no board reached the market");
    // 10 asked: the piles expect 10, carriers bring them, +0x180 counts down per arrival.
    assert_eq!(r.stock + r.collect as i32, 10, "stock + still to collect");
    assert_eq!(r.send, 10, "nothing sent without a target");
    assert!((1..=3).contains(&r.max_piles), "piles collecting boards: {}", r.max_piles);
    assert_eq!(run(6_000).unwrap(), r, "two runs differ");
}
