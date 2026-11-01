//! The scripts' vehicle calls (`vehicles::scripted`, `Vehicles.*`, `Settlers.AddSettlersToFerry`)
//! on roman02's sea and land: a ferry made by `AddVehicle`, soldiers and a filled cart put aboard
//! directly, the cargo counted by area, the health read by node, the ferry killed by area.
//! Skipped without the game data.

use s4_sim::World;
use s4_sim::vehicles::{self, ferry, kind, scripted};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

#[test]
fn script_ferry_cargo_health_kill() {
    let Some(mut w) = roman02() else { return };
    let Some((x, y)) = s4_sim::spiral::walk(w.grid, 200, 90, 12).find(|&(px, py)| vehicles::can_place(&w, px, py, kind::FERRY)) else { return };
    // Off the map, a player without a team, a type out of range: nothing.
    assert_eq!(scripted::add_vehicle(&mut w, -1, y, 1, kind::FERRY as i32, 0, 0, 0), 0);
    assert_eq!(scripted::add_vehicle(&mut w, x, y, 9, kind::FERRY as i32, 0, 0, 0), 0);
    assert_eq!(scripted::add_vehicle(&mut w, x, y, 1, 6, 0, 0, 0), 0);
    let f = scripted::add_vehicle(&mut w, x, y, 1, kind::FERRY as i32, 2, 0, 1);
    assert!(f != 0 && ferry::is_ferry(&w, f));
    // Ferries take soldiers only (warrior types); 4 level-3 swordsmen and 2 level-1 bowmen.
    scripted::add_settlers_to_ferry(&mut w, f as i32, 1, 3);
    scripted::add_settlers_to_ferry(&mut w, f as i32, 31, 4);
    scripted::add_settlers_to_ferry(&mut w, f as i32, 32, 2);
    scripted::add_wheeler_to_ferry(&mut w, f as i32, 0x406);
    let d = ferry::data(&w, f).unwrap();
    assert_eq!((d.passengers.len(), d.aboard), (7, 11), "{:?}", d.passengers);
    assert_eq!(scripted::ferry_cargo_in_area(&w, 1, x + 3, y, 3), [0, 2, 0, 4, 0, 1]);
    assert_eq!(scripted::ferry_cargo_in_area(&w, 1, x + 3, y, 2), [0; 6], "out of range");
    // Room for 4 more settlers, none for another cart.
    scripted::add_wheeler_to_ferry(&mut w, f as i32, 4);
    scripted::add_settlers_to_ferry(&mut w, f as i32, 29, 10);
    assert_eq!(ferry::data(&w, f).unwrap().aboard, 15);
    let hp = vehicles::get(&w, f).unwrap().health as i32;
    assert_eq!(scripted::health_at(&w, x, y), hp);
    assert_eq!(scripted::health_at(&w, x + 1, y), -1);
    // Kill by area: the wrong type or player spares it, then it dies with its passengers.
    scripted::kill_vehicles(&mut w, 1, kind::WARSHIP as i32, x, y, 2, 1);
    scripted::kill_vehicles(&mut w, 2, kind::FERRY as i32, x, y, 2, 1);
    assert!(ferry::is_ferry(&w, f));
    let passengers: Vec<u16> = ferry::data(&w, f).unwrap().passengers.iter().map(|p| p.id).collect();
    scripted::kill_vehicles(&mut w, 1, kind::FERRY as i32, x, y, 2, 0);
    for _ in 0..5 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(w.entities.by_index(f as u32).is_none_or(|e| e.is_dead()), "the ferry died");
    assert!(passengers.iter().all(|&p| w.entities.by_index(p as u32).is_none_or(|e| e.is_dead())), "its passengers too");
}

#[test]
fn script_catapult_with_ammunition() {
    let Some(mut w) = roman02() else { return };
    let (cx, cy) = {
        let p = w.player(1).unwrap();
        (p.start_x as i32, p.start_y as i32)
    };
    let Some((x, y)) = s4_sim::spiral::walk(w.grid, cx, cy, 20).find(|&(px, py)| vehicles::can_place(&w, px, py, kind::WAR_MACHINE)) else { return };
    let c = scripted::add_vehicle(&mut w, x, y, 1, kind::WAR_MACHINE as i32, 0, 500, 0);
    let max = w.rules.vehicles.get(0, kind::WAR_MACHINE as u8).max_ammo;
    assert_eq!(vehicles::get(&w, c).unwrap().cargo as i32, max, "clamped to MaxAmmo");
    scripted::add_ammo(&mut w, c, -5);
    assert_eq!(vehicles::get(&w, c).unwrap().cargo as i32, max);
}
