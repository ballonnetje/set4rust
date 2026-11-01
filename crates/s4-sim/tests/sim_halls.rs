//! The manakopter hall positions (`build::halls`, `CBuildingMgr` +0x46fdc / +0x472d0): the list
//! `AddBuilding` fills (0x53f810), the second hall it destroys, the hall tick `DestroyBuilding`
//! sets (0x53cec7) and the AI's position pick (0x53f750). Skipped without the game data.

use s4_sim::World;
use s4_sim::build::halls;
use s4_sim::entity::ObjType;

fn map(name: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(name);
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn halls_of(w: &World, p: u8) -> Vec<u16> {
    w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.kind == 80 && e.owner == p && !e.is_dead()).map(|(i, _)| i).collect()
}

/// AO_trojan04 places three halls for its Dark Tribe player: all three positions are listed, the
/// second and third halls are destroyed at once, and the hall tick stays 0.
#[test]
fn map_halls_fill_the_list_and_only_the_first_stands() {
    let Some(mut w) = map("AO_trojan04.map") else { return };
    let dt = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3)).unwrap();
    let placed: Vec<(u16, u16)> = {
        let m = s4_formats::map::MapFile::open(s4_formats::game_dir().join("Map/Campaign/AO_trojan04.map")).unwrap();
        m.buildings().unwrap().iter().filter(|b| b.owner == dt && b.kind == 80).map(|b| (b.x, b.y)).collect()
    };
    assert_eq!(placed.len(), 3);
    let listed: Vec<(u16, u16)> = w.sites.halls.positions[dt as usize].iter().map(|p| (p.x() as u16, p.y() as u16)).collect();
    assert_eq!(listed, placed, "every placed hall's position is listed, in load order");
    let alive = halls_of(&w, dt);
    assert_eq!(alive.len(), 1, "only the first hall stands");
    assert_eq!(w.sites.halls.tick[dt as usize], 0, "the second-hall destruction keeps the tick");

    // Destroying the hall sets the tick; then the AI's pick finds a listed position.
    for _ in 0..100 {
        s4_sim::tick(&mut w, &[]);
    }
    let h = alive[0];
    let (x, y) = { let e = w.entities.by_index(h as u32).unwrap(); (e.x(), e.y()) };
    s4_sim::buildings::crush::destroy_building_at(&mut w, x, y, -1);
    assert_eq!(w.sites.halls.tick[dt as usize], w.tick, "DestroyBuilding sets the hall tick");
    for _ in 0..50 {
        s4_sim::tick(&mut w, &[]);
    }
    if let Some(p) = halls::build_position(&w, dt) {
        assert!(w.sites.halls.positions[dt as usize].contains(&p));
    }
    let first = w.sites.halls.positions[dt as usize][0];
    assert!(halls::near_position(&w, dt, first));
    assert!(halls::near_position(&w, dt, s4_sim::Y16X16::pack(first.x() + 24, first.y())));
}

/// Ordering a second hall (5025) while one stands: the order's position joins the list and the
/// new site is destroyed at once.
#[test]
fn a_second_ordered_hall_is_destroyed_at_once() {
    let Some(mut w) = map("MD_roman2.map") else { return };
    let dt = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3)).unwrap();
    let size = w.grid.size() as i32;
    let spots: Vec<(i32, i32)> = (20..size - 20)
        .flat_map(|y| (20..size - 20).map(move |x| (x, y)))
        .filter(|&(x, y)| matches!(s4_sim::build::check::check_for_build(&w, x, y, dt, 80), s4_sim::build::CheckResult::Possible { .. }))
        .collect();
    let a = spots[0];
    let b = *spots.iter().find(|&&(x, y)| (x - a.0).abs() + (y - a.1).abs() > 30).expect("two distant spots");
    let order = |x: i32, y: i32| s4_sim::PlayerCommand::new(dt, s4_sim::Command::Build { building: 80, x: x as u16, y: y as u16 });
    s4_sim::tick(&mut w, &[order(a.0, a.1)]);
    assert_eq!(halls_of(&w, dt).len(), 1);
    let tick_before = w.sites.halls.tick[dt as usize];
    s4_sim::tick(&mut w, &[order(b.0, b.1)]);
    assert_eq!(w.sites.halls.positions[dt as usize].len(), 2, "both positions listed");
    assert_eq!(halls_of(&w, dt).len(), 1, "the second hall is gone");
    assert_eq!(w.sites.halls.tick[dt as usize], tick_before, "the tick is kept");
}
