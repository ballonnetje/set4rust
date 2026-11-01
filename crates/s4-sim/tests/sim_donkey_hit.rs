//! A loaded donkey that is hit (`CDonkeyRole` vtbl+0x44 0x576360): it takes no damage and
//! `JOB_DONKEY_ATTACK_REACTION` (list 162, task 32: `TakeJob` 0x576640, `LogicUpdateJob`
//! 0x576460, `UpdateJob` 0x576610) drops its load where it stands (0x576b10), after which it
//! is no longer vulnerable. roman02, the trade route of `sim_trade_donkey`: two marketplaces of
//! player 1, ten boards from A to B; the donkey is hit on its way to B.

use s4_sim::buildings::trading;
use s4_sim::entity::EntityData;
use s4_sim::{Command, PlayerCommand, World};

const DONKEY: u16 = 60;
const BOARD: u8 = 7;

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

/// The donkey's list, task, load and flags.
fn donkey(w: &World, id: u16) -> (i16, u8, i32, u32, u8) {
    let e = w.entities.by_index(id as u32).unwrap();
    let EntityData::Settler(s) = &e.data else { panic!("not a settler") };
    let load = s.job.donkey.as_deref().map_or(0, |d| d.slots.iter().map(|s| s.1).sum());
    (s.job.list_id, s.job.task, load, e.flags, s.health)
}

/// Boards on piles of nobody's trade role within 3 of the node.
fn boards_near(w: &World, x: i32, y: i32) -> i32 {
    s4_sim::spiral::walk(w.grid, x, y, 3)
        .filter_map(|(px, py)| {
            let id = w.layers.deco_object[w.grid.index(px as u32, py as u32)];
            match w.entities.by_index(id as u32).map(|e| &e.data) {
                Some(EntityData::Pile(p)) if id != 0 && p.good == BOARD => Some(p.amount as i32),
                _ => None,
            }
        })
        .sum()
}

#[test]
fn donkey_hit_while_loaded_drops_its_load() {
    let Some(mut w) = roman02() else { return };
    let (cx, cy) = {
        let pl = w.player(1).unwrap();
        (pl.start_x as i32, pl.start_y as i32)
    };
    let size = w.grid.size() as i32;
    let market = trading::MARKETPLACE as u8;
    let sites = s4_sim::build::possible_sites(&w, 1, market, (0, 0), (size, size));
    let Some(&(ax, ay, _)) = sites.iter().min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs()) else { return };
    let a = s4_sim::create::add_building(&mut w, ax as i32, ay as i32, 1, market).unwrap();
    let sites = s4_sim::build::possible_sites(&w, 1, market, (0, 0), (size, size));
    let Some(&(bx, by, _)) = sites
        .iter()
        .filter(|s| (15..=30).contains(&s4_sim::spiral::grid_distance(s.0 as i32 - ax as i32, s.1 as i32 - ay as i32)))
        .min_by_key(|s| (s.0 as i32 - ax as i32).abs() + (s.1 as i32 - ay as i32).abs())
    else {
        return;
    };
    let b = s4_sim::create::add_building(&mut w, bx as i32, by as i32, 1, market).unwrap();
    let ens = match &w.entities.by_index(a as u32).unwrap().data {
        EntityData::Building(d) => d.ensign,
        _ => unreachable!(),
    };
    let (dx, dy) = s4_sim::spiral::walk(w.grid, ens.x() + 6, ens.y() + 6, 10)
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.layers.map_object[n] == 0 && w.owner_of(n) == 1 && w.tiling.sector_of(n) == w.tiling.sector_of(w.grid.index(ens.x() as u32, ens.y() as u32))
        })
        .unwrap();
    let id = s4_sim::create::add_settler(&mut w, dx, dy, 1, DONKEY).unwrap();
    let cmds = [
        PlayerCommand { player: 1, command: Command::TransportGood { building: a, good: BOARD as u16, amount: 10 } },
        PlayerCommand { player: 1, command: Command::SetTradeTarget { building: a as u32, target: b as u32 } },
    ];
    // On its way to B, loaded and vulnerable, a few steps out.
    let mut walked = 0;
    for t in 0..9000 {
        s4_sim::tick(&mut w, if t == 0 { &cmds[..] } else { &[] });
        let (list, task, load, flags, _) = donkey(&w, id);
        if list == 163 && task == 6 && load == 10 && flags & 0x30000 == 0x30000 {
            walked += 1;
            if walked == 60 {
                break;
            }
        }
    }
    assert_eq!(walked, 60, "the donkey never walked loaded to B");
    let (_, _, _, _, health) = donkey(&w, id);
    s4_sim::combat::take_damage(&mut w, id, 30, 2);
    let (list, task, load, _, after) = donkey(&w, id);
    assert_eq!(after, health, "a donkey takes no damage");
    assert_eq!((list, task, load), (162, 32, 10), "the attack reaction, still loaded");
    // A second hit while reacting changes nothing.
    s4_sim::combat::take_damage(&mut w, id, 30, 2);
    assert_eq!(donkey(&w, id).0, 162);
    let mut dropped = None;
    for _ in 0..400 {
        s4_sim::tick(&mut w, &[]);
        let (list, task, load, flags, _) = donkey(&w, id);
        if load == 0 && dropped.is_none() {
            assert_eq!((list, task), (162, 32), "the load goes down during task 32");
            assert_eq!(flags & 0x30000, 0, "no longer vulnerable");
            let e = w.entities.by_index(id as u32).unwrap();
            dropped = Some(boards_near(&w, e.x(), e.y()));
        }
        if list != 162 {
            break;
        }
    }
    assert_eq!(dropped, Some(10), "the ten boards lie where the donkey stood");
    let (list, task, load, _, _) = donkey(&w, id);
    eprintln!("after the reaction: list {list} task {task} load {load}");
    assert_ne!((list, task), (162, 32), "the reaction ends");
    assert_eq!(load, 0);
}
