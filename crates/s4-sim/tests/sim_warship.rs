//! War ships (`vehicles::warship`, V5d: `CWarShip` with the catapults' vehicle behaviour).
//!
//! * `warship_shoots_an_enemy_ship`: on roman02's sea a loaded Roman war ship of player 1 and a
//!   ferry of a hostile player 10 nodes away: the behaviour's search (0x5999c0, category 1 of
//!   the war map, row 10 of the war ship) picks the ferry, the ship shoots (0x592780 → 0x595010:
//!   one ammunition, `ReadyToFireDelay` between shots, gfx job 1) and the damage (event 2) lands
//!   after the missile's flight; deterministic.
//! * `warship_takes_ammunition_at_a_shipyard`: an empty war ship next to player 1's shipyard
//!   with its ammunition good in an input pile is loaded every 21 ticks (0x5928d0: 4 per good up
//!   to `MaxAmmo`).
//!
//! * `viking_warship_shoots_with_mana`: the same with player 1 made a Viking: no ammunition, every
//!   shot takes `VikingWarmachineShotCost` mana (vtbl+0xcc 0x592e60 / +0xd4 0x592ec0 → 0x4f59f0),
//!   and without mana it stops shooting (one shot's mana: one shot, one hit, in 1500 ticks).
//!
//! Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::vehicles::{self, kind};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

fn water_near(w: &World, x: i32, y: i32, r: u8) -> Option<(i32, i32)> {
    s4_sim::spiral::walk(w.grid, x, y, r).find(|&(px, py)| vehicles::can_place(w, px, py, kind::WARSHIP))
}

#[derive(Debug, PartialEq, Eq)]
struct Shots {
    hash: u64,
    shots: Vec<u32>,
    hits: Vec<(u32, u8)>,
}

fn shoot_run(ticks: u32) -> Option<Shots> {
    let mut w = roman02()?;
    let enemy = (2..=8u8).find(|&p| w.player(p).is_some() && s4_sim::warmap::hostile(&w, 1, p))?;
    let (ax, ay) = water_near(&w, 200, 90, 12)?;
    let (bx, by) = water_near(&w, ax + 10, ay, 6)?;
    let s = vehicles::add_vehicle(&mut w, ax, ay, 1, kind::WARSHIP, 0, false)?;
    let t = vehicles::add_vehicle(&mut w, bx, by, enemy, kind::FERRY, 3, false)?;
    vehicles::get_mut(&mut w, s)?.cargo = 20;
    let (mut shots, mut hits) = (Vec::new(), Vec::new());
    let (mut ammo, mut health) = (20u8, vehicles::get(&w, t)?.health);
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let Some(v) = vehicles::get(&w, s) else { break };
        if v.cargo < ammo {
            shots.push(w.tick);
            ammo = v.cargo;
            assert_eq!(v.job_part, 1, "the ship's shooting job");
        }
        let h = vehicles::get(&w, t).map_or(0, |v| v.health);
        if h < health {
            hits.push((w.tick, health - h));
            health = h;
        }
        if h == 0 {
            break;
        }
    }
    Some(Shots { hash: w.state_hash(), shots, hits })
}

#[test]
fn warship_shoots_an_enemy_ship() {
    let Some(r) = shoot_run(1500) else {
        eprintln!("skipping: no game data, no hostile player or no water");
        return;
    };
    eprintln!("{r:?}");
    assert!(r.shots.len() >= 2, "shots {:?}", r.shots);
    let delay = roman02().unwrap().rules.vehicles.get(0, kind::WARSHIP as u8).fire_delay as u32;
    assert!(r.shots.windows(2).all(|p| p[1] - p[0] == delay), "fire delay {delay}: {:?}", r.shots);
    assert!(!r.hits.is_empty() && r.hits[0].0 > r.shots[0], "damage after the shot: {r:?}");
    assert_eq!(shoot_run(1500).unwrap(), r, "two runs differ");
}

/// A shipyard site: `(type, x, y, working area)`.
type Site = (u8, i32, i32, (i32, i32));

/// roman02 with player 1's land claimed and a shipyard site (SHIPYARDA..F, 58..63) whose working
/// area is open water.
fn shipyard_world() -> Option<(World, Site)> {
    let mut w = roman02()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            s4_sim::entity::EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    let size = w.grid.size() as i32;
    // A shipyard variant (SHIPYARDA..F, 58..63) whose working area is open water.
    let mut site = None;
    for t in 58u8..=63 {
        let info = w.rules.building(w.race_of(1), t);
        for s in s4_sim::build::possible_sites(&w, 1, t, (0, 0), (size, size)) {
            let wa = (s.0 as i32 + info.working.0 as i32, s.1 as i32 + info.working.1 as i32);
            if vehicles::can_place(&w, wa.0, wa.1, kind::WARSHIP) {
                site = Some((t, s.0 as i32, s.1 as i32, wa));
                break;
            }
        }
        if site.is_some() {
            break;
        }
    }
    Some((w, site?))
}

#[test]
fn warship_takes_ammunition_at_a_shipyard() {
    let Some((mut w, (t, x, y, wa))) = shipyard_world() else {
        eprintln!("skipping: no shipyard site");
        return;
    };
    let yard = s4_sim::create::add_building(&mut w, x, y, 1, t).unwrap();
    let good = w.rules.vehicles.get(0, kind::WARSHIP as u8).x50 as u8;
    let pile = s4_sim::buildings::workshop::pile_with_good(&w, yard, good);
    assert_ne!(pile, 0, "the shipyard has a pile of the ammunition good {good}");
    s4_sim::pile::increase_unforeseen(&mut w, pile, 3);
    let s = vehicles::add_vehicle(&mut w, wa.0, wa.1, 1, kind::WARSHIP, 0, false).unwrap();
    let max = w.rules.vehicles.get(0, kind::WARSHIP as u8).max_ammo;
    let mut loaded = None;
    for _ in 0..60 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        if loaded.is_none() && vehicles::get(&w, s).unwrap().cargo > 0 {
            loaded = Some((w.tick, vehicles::get(&w, s).unwrap().cargo));
        }
    }
    eprintln!("shipyard {yard} type {t} at ({x},{y}), ship at {wa:?}: loaded {loaded:?}, max {max}");
    let (tick, n) = loaded.expect("the ship took ammunition");
    assert!(tick <= 22, "at its first 21-tick step: {tick}");
    assert_eq!(n as i32, (3 * 4).min(max));
    assert_eq!(s4_sim::pile::get(&w, pile).unwrap().amount as i32, 3 - (3 * 4).min(max) / 4);
}

#[test]
fn viking_warship_shoots_with_mana() {
    let Some(mut w) = roman02() else { return };
    let Some(enemy) = (2..=8u8).find(|&p| w.player(p).is_some() && s4_sim::warmap::hostile(&w, 1, p)) else { return };
    let Some((ax, ay)) = water_near(&w, 200, 90, 12) else { return };
    let Some((bx, by)) = water_near(&w, ax + 10, ay, 6) else { return };
    if let Some(Some(p)) = w.players.get_mut(1) {
        p.race = 1;
    }
    let cost = w.rules.magic.viking_warmachine_shot_cost.max(1);
    // One shot's mana: the ferry survives one hit (two sink it), so a second shot would come.
    w.magic.players[1].mana = cost;
    let s = vehicles::add_vehicle(&mut w, ax, ay, 1, kind::WARSHIP, 0, false).unwrap();
    let t = vehicles::add_vehicle(&mut w, bx, by, enemy, kind::FERRY, 3, false).unwrap();
    assert_eq!(w.entities.by_index(s as u32).unwrap().race, 1);
    let (mut shots, mut hits) = (Vec::new(), 0);
    let mut mana = w.magic.players[1].mana;
    let mut health = vehicles::get(&w, t).unwrap().health;
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        w.events.clear();
        let m = w.magic.players[1].mana;
        if m < mana {
            assert_eq!(mana - m, cost, "one shot's cost");
            shots.push(w.tick);
            mana = m;
        }
        let h = vehicles::get(&w, t).map_or(0, |v| v.health);
        if h < health {
            hits += 1;
            health = h;
        }
        if h == 0 {
            break;
        }
    }
    eprintln!("viking shots {shots:?}, hits {hits}, mana left {mana}");
    assert_eq!(shots.len(), 1, "one shot's mana, then none");
    assert_eq!((mana, hits), (0, 1));
    assert!(health > 0, "the ferry survives the one hit");
}

/// A map's shipyard (`LoadBuildingData` 0x53d940 → `CBuilding::PostLoad` 0x536170 → the
/// workshop's 0x5588e0): water bit 0x1000 at its workingpos.
#[test]
fn shipyard_post_load_marks_its_water() {
    let Some((mut w, (t, x, y, wa))) = shipyard_world() else { return };
    let yard = s4_sim::create::add_building(&mut w, x, y, 1, t).unwrap();
    let n = w.grid.checked_index(wa.0, wa.1).unwrap();
    w.tiling.water_flags[n] &= !0x1000;
    s4_sim::buildings::workshop::post_load(&mut w, yard);
    assert!(w.tiling.water_flags[n] > 0xbfff && w.tiling.water_flags[n] & 0x1000 != 0, "{:#x}", w.tiling.water_flags[n]);
}
