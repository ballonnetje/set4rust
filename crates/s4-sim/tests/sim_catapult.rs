//! Catapults (`CCatapult`, `vehicles::catapult`), driven by `tick()` on roman02.
//!
//! * `catapult_shoots_an_enemy_tower`: a loaded Roman catapult of player 1 near a military
//!   building of player 2 (with a warrior type, so a target of row 8): the behaviour's search
//!   (0x5999c0) finds it, the catapult shoots (0x595010: future event 6, `ReadyToFireDelay`
//!   between shots, one ammunition each), the missile lands (0x594c80) and the damage event 2
//!   lowers the building's health. Deterministic.
//! * `catapult_takes_ammunition_from_a_pile`: an empty catapult next to a pile of its
//!   ammunition good loads from it (0x560c90) within 21 ticks.
//! * `catapult_load_sends_a_donkey`: command 5010 (`CatapultLoad`, 0x560d80) with a donkey near
//!   an offered pile of stones: the donkey takes list 164, fetches the goods and loads them into
//!   the catapult (0x560f50), which counts them as ammunition.
//! * `catapult_load_without_donkey_calls_a_carrier`: no donkey: an offered carrier of the pile's
//!   eco-sector brings one good (list 153).
//! * `destroy_vehicle_command`: 5049 only for the owner.
//!
//! Skipped when the game data is not present.

use s4_sim::entity::{EntityData, ObjType, flag};
use s4_sim::vehicles::{self, catapult, kind};
use s4_sim::{Command, PlayerCommand, World};

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

/// A node within `lo..=hi` of (x, y) where a catapult may stand.
fn place_near(w: &World, x: i32, y: i32, lo: i32, hi: i32) -> Option<(i32, i32)> {
    s4_sim::spiral::walk(w.grid, x, y, hi as u8).find(|&(px, py)| {
        let d = s4_sim::spiral::grid_distance(px - x, py - y);
        d >= lo && vehicles::can_place(w, px, py, kind::WAR_MACHINE)
    })
}

fn health(w: &World, b: u16) -> Option<u8> {
    match &w.entities.by_index(b as u32)?.data {
        EntityData::Building(d) => Some(d.health),
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Shots {
    hash: u64,
    target: u16,
    shots: Vec<u32>,
    hits: Vec<(u32, u8)>,
    ammo: u8,
}

fn shoot_run(ticks: u32) -> Option<Shots> {
    let mut w = roman02()?;
    // An enemy building a catapult may target (warrior type set, row 8's mask).
    let target = w.entities.iter().find(|(id, e)| e.obj_type == ObjType::Building && e.owner != 1 && e.flags & 0xF != 0 && s4_sim::jobs::warrior::valid_target(&w, *id, 0x202e0ff)).map(|(id, _)| id)?;
    let tp = w.entities.by_index(target as u32)?.pos;
    let (cx, cy) = place_near(&w, tp.x(), tp.y(), 12, 18)?;
    let c = vehicles::add_vehicle(&mut w, cx, cy, 1, kind::WAR_MACHINE, -1, false)?;
    vehicles::get_mut(&mut w, c)?.cargo = 32;
    let (mut shots, mut hits) = (Vec::new(), Vec::new());
    let mut last_h = health(&w, target)?;
    let mut last_ammo = 32;
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let Some(ammo) = vehicles::get(&w, c).map(|v| v.cargo) else {
            eprintln!("the catapult is gone at {}", w.tick);
            break;
        };
        if ammo < last_ammo {
            shots.push(w.tick);
            last_ammo = ammo;
        }
        let h = health(&w, target).unwrap_or(0);
        if h < last_h {
            hits.push((w.tick, last_h - h));
            last_h = h;
        }
    }
    Some(Shots { hash: w.state_hash(), target, shots, hits, ammo: last_ammo })
}

#[test]
fn catapult_shoots_an_enemy_tower() {
    let Some(r) = shoot_run(1500) else {
        eprintln!("skipping: no game data, no enemy target or no place");
        return;
    };
    eprintln!("{r:?}");
    assert!(r.shots.len() >= 3, "shots {:?}", r.shots);
    // `ReadyToFireDelay` (Roman: 56) between shots at least.
    assert!(r.shots.windows(2).all(|p| p[1] - p[0] >= 56), "fire delay {:?}", r.shots);
    assert!(!r.hits.is_empty(), "no damage arrived");
    assert!(r.hits[0].0 > r.shots[0], "the damage comes after the shot");
    assert_eq!(shoot_run(1500).unwrap(), r, "two runs differ");
}

#[test]
fn catapult_takes_ammunition_from_a_pile() {
    let Some(mut w) = roman02() else { return };
    let pl = w.player(1).cloned().unwrap();
    let Some((cx, cy)) = place_near(&w, pl.start_x as i32 + 8, pl.start_y as i32 + 8, 0, 20) else { return };
    let c = vehicles::add_vehicle(&mut w, cx, cy, 1, kind::WAR_MACHINE, -1, false).unwrap();
    let good = w.rules.vehicles.get(0, kind::WAR_MACHINE as u8).x50 as u8;
    let (px, py) = s4_sim::spiral::walk(w.grid, cx, cy, 4)
        .skip(7)
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0 && w.layers.flags[n] & 0x7f == 0
        })
        .unwrap();
    let pile = s4_sim::create::add_pile(&mut w, px, py, good, 5, s4_sim::pile::role::ANYWHERE).unwrap();
    assert_eq!(vehicles::get(&w, c).unwrap().cargo, 0);
    let mut loaded = None;
    for _ in 0..40 {
        s4_sim::tick(&mut w, &[]);
        if loaded.is_none() && vehicles::get(&w, c).unwrap().cargo > 0 {
            loaded = Some(w.tick);
        }
    }
    let v = vehicles::get(&w, c).unwrap();
    eprintln!("loaded at {loaded:?}: ammo {}, pile {:?}", v.cargo, s4_sim::pile::get(&w, pile).map(|p| p.amount));
    assert_eq!(v.cargo, 20, "5 goods · 4");
    assert!(loaded.is_some_and(|t| t <= 22));
}

#[test]
fn catapult_load_sends_a_donkey() {
    let Some(mut w) = roman02() else { return };
    let pl = w.player(1).cloned().unwrap();
    let Some((cx, cy)) = place_near(&w, pl.start_x as i32 + 6, pl.start_y as i32 + 6, 0, 20) else { return };
    let c = vehicles::add_vehicle(&mut w, cx, cy, 1, kind::WAR_MACHINE, -1, false).unwrap();
    let good = w.rules.vehicles.get(0, kind::WAR_MACHINE as u8).x50 as u8;
    // An offered pile of the ammunition good away from the catapult, a donkey near it.
    let (px, py) = s4_sim::spiral::walk(w.grid, cx + 8, cy - 4, 8)
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.owner_of(n) == 1 && w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0 && w.layers.flags[n] & 0x7f == 0 && s4_sim::spiral::grid_distance(x - cx, y - cy) > 6
        })
        .unwrap();
    let pile = s4_sim::create::add_pile(&mut w, px, py, good, 8, s4_sim::pile::role::ANYWHERE).unwrap();
    let (dx, dy) = s4_sim::spiral::walk(w.grid, px + 3, py + 2, 6)
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.owner_of(n) == 1 && w.layers.map_object[n] == 0 && w.layers.flags[n] & 1 == 0
        })
        .unwrap();
    let donkey = s4_sim::create::add_settler(&mut w, dx, dy, 1, 60).unwrap();
    for _ in 0..3 {
        s4_sim::tick(&mut w, &[]);
    }
    if let Some(cd) = catapult::data(&w, c) {
        assert_eq!(cd.carrier, 0);
    }
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::CatapultLoad { vehicle: c as u32 } }]);
    assert_eq!(catapult::data(&w, c).unwrap().carrier, donkey as i32, "the donkey is its transporter");
    let mut got = None;
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        if got.is_none() && vehicles::get(&w, c).unwrap().cargo > 0 {
            got = Some(w.tick);
            break;
        }
    }
    let v = vehicles::get(&w, c).unwrap();
    eprintln!("ammo {} at {got:?}, pile {:?}, carrier {:?}", v.cargo, s4_sim::pile::get(&w, pile).map(|p| (p.amount, p.leaving)), catapult::data(&w, c).map(|d| d.carrier));
    assert_eq!(v.cargo, 32, "8 goods · 4 brought by the donkey (MaxAmmo 32)");
    assert_eq!(catapult::data(&w, c).unwrap().carrier, 0, "the transporter is forgotten");
    let free = w.entities.by_index(donkey as u32).is_some_and(|e| e.flags & flag::READY != 0);
    assert!(free);
}

#[test]
fn destroy_vehicle_command() {
    let Some(mut w) = roman02() else { return };
    let pl = w.player(1).cloned().unwrap();
    let Some((cx, cy)) = place_near(&w, pl.start_x as i32 + 8, pl.start_y as i32 + 8, 0, 20) else { return };
    let c = vehicles::add_vehicle(&mut w, cx, cy, 1, kind::WAR_MACHINE, -1, false).unwrap();
    s4_sim::tick(&mut w, &[]);
    // Another player's command does nothing (5049 checks the owner).
    s4_sim::tick(&mut w, &[PlayerCommand { player: 2, command: Command::DestroyVehicle { vehicle: c as u32 } }]);
    assert!(vehicles::get(&w, c).is_some());
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::DestroyVehicle { vehicle: c as u32 } }]);
    assert!(w.entities.by_index(c as u32).is_none_or(|e| e.is_dead()), "destroyed");
}

#[test]
fn catapult_load_without_donkey_calls_a_carrier() {
    let Some(mut w) = roman02() else { return };
    let pl = w.player(1).cloned().unwrap();
    let Some((cx, cy)) = place_near(&w, pl.start_x as i32 + 6, pl.start_y as i32 + 6, 0, 20) else { return };
    let c = vehicles::add_vehicle(&mut w, cx, cy, 1, kind::WAR_MACHINE, -1, false).unwrap();
    let good = w.rules.vehicles.get(0, kind::WAR_MACHINE as u8).x50 as u8;
    let (px, py) = s4_sim::spiral::walk(w.grid, cx + 8, cy - 4, 8)
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.owner_of(n) == 1 && w.layers.map_object[n] == 0 && w.layers.deco_object[n] == 0 && w.layers.flags[n] & 0x7f == 0 && s4_sim::spiral::grid_distance(x - cx, y - cy) > 6
        })
        .unwrap();
    s4_sim::create::add_pile(&mut w, px, py, good, 8, s4_sim::pile::role::ANYWHERE).unwrap();
    for _ in 0..3 {
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::CatapultLoad { vehicle: c as u32 } }]);
    let t = catapult::data(&w, c).unwrap().carrier;
    let is_carrier = w.entities.by_index(t as u32).is_some_and(|e| e.obj_type == ObjType::Settler && e.kind == 1);
    assert!(is_carrier, "roman02 starts without donkeys: a carrier ({t})");
    let mut got = None;
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        if vehicles::get(&w, c).unwrap().cargo > 0 {
            got = Some(w.tick);
            break;
        }
    }
    let v = vehicles::get(&w, c).unwrap();
    eprintln!("ammo {} at {got:?}, carrier {:?}", v.cargo, catapult::data(&w, c).map(|d| d.carrier));
    assert_eq!(v.cargo, 4, "one good · 4 brought by the carrier");
    assert_eq!(catapult::data(&w, c).unwrap().carrier, 0, "the transporter is forgotten");
}
