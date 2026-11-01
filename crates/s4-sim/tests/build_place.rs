//! Placement on a real map: possible sites for a woodcutter on roman02, then command 5025 and the
//! site's first updates. Skipped when the game data is not present.

use s4_sim::build::{CheckResult, check_for_build, possible_sites};
use s4_sim::entity::EntityData;
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

#[test]
fn woodcutter_sites_and_build() {
    let Some(mut w) = roman02() else { return };
    let size = w.grid.size() as i32;
    let t = std::time::Instant::now();
    let sites = possible_sites(&w, 1, 1, (0, 0), (size, size));
    let took = t.elapsed();
    let mut classes = [0usize; 9];
    for &(_, _, h) in &sites {
        classes[(h - 9) as usize] += 1;
    }
    eprintln!("{} possible woodcutter sites for player 1 in {:.1} ms; by class 1..8: {:?}", sites.len(), took.as_secs_f64() * 1e3, &classes[1..]);
    assert!(!sites.is_empty());
    // Every marker re-checks the same; a node next to an existing building does not.
    for &(x, y, h) in sites.iter().take(50) {
        assert_eq!(check_for_build(&w, x as i32, y as i32, 1, 1).code(), (h - 9) as i32);
    }
    // Build on the flattest one.
    let &(x, y, _) = sites.iter().min_by_key(|s| s.2).unwrap();
    let before = w.entities.iter().count();
    let cmd = PlayerCommand { player: 1, command: Command::Build { building: 1, x, y } };
    s4_sim::tick(&mut w, &[cmd]);
    let after = w.entities.iter().count();
    let site = w.sites.roles.iter().next().map(|(id, s)| (*id, s.clone()));
    eprintln!("built at {x},{y}: entities {before} -> {after}, site {site:?}");
    let (id, s) = site.expect("a site");
    assert!(matches!(s.state, 2 | 4));
    assert!(matches!(check_for_build(&w, x as i32, y as i32, 1, 1), CheckResult::Impossible(_)));
    // Run 100 ticks: a flat site goes on to state 5 and stamps its footprint.
    for _ in 0..100 {
        s4_sim::tick(&mut w, &[]);
    }
    let s = &w.sites.roles[&id];
    eprintln!("after 100 ticks: state {}", s.state);
}

/// A site driven to completion without workers: digging marked done, then `add_work` as builders
/// would do it. The site must stamp its footprint (state 4 → 5), then on enough work delete its
/// piles and stakes and become a READY building.
#[test]
fn site_completes_with_synthetic_work() {
    let Some(mut w) = roman02() else { return };
    let size = w.grid.size() as i32;
    let sites = possible_sites(&w, 1, 1, (0, 0), (size, size));
    let &(x, y, _) = sites.iter().min_by_key(|s| s.2).unwrap();
    s4_sim::tick(&mut w, &[PlayerCommand { player: 1, command: Command::Build { building: 1, x, y } }]);
    let (&id, s) = w.sites.roles.iter().next().expect("a site");
    let piles = s.piles;
    // The diggers are done.
    w.sites.roles.get_mut(&id).unwrap().dig_effort = 0;
    let mut state5_at = None;
    for t in 0..200 {
        s4_sim::tick(&mut w, &[]);
        if state5_at.is_none() && w.sites.roles.get(&id).is_some_and(|s| s.state == 5) {
            state5_at = Some(t);
            let work = w.sites.roles[&id].work;
            s4_sim::build::add_work(&mut w, id, work);
        }
        if !w.sites.roles.contains_key(&id) {
            break;
        }
    }
    eprintln!("state 5 after {state5_at:?} ticks; site left: {}", w.sites.roles.contains_key(&id));
    assert!(state5_at.is_some(), "the site reached state 5");
    assert!(!w.sites.roles.contains_key(&id), "BuildingDone ran");
    let e = w.entities.by_index(id as u32).unwrap();
    assert!(e.flags & s4_sim::entity::flag::READY != 0 && e.flags & s4_sim::entity::flag::BIRTH == 0);
    for p in piles.into_iter().filter(|&p| p != 0) {
        assert!(w.entities.by_index(p as u32).is_none_or(|e| e.is_dead()), "pile {p} deleted");
    }
    let n = w.grid.index(x as u32, y as u32);
    assert_eq!(w.layers.map_object[n], id);
    assert_eq!(w.layers.flags[n] & 0x08, 0x08);
}
