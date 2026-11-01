//! `CSettler::Delete` (0x580b20) → `CSettlerMgr` 0x582940 for a non-warrior: an offered carrier
//! that dies leaves the eco-sector's offer list and count. (Before, a killed carrier stayed
//! linked in the list; its reused id then linked two lists, and `InitTransport` looped.)
//! roman02, skipped without the data.

use s4_sim::entity::{ObjType, flag};

#[test]
fn a_killed_carrier_leaves_the_offer_and_the_count() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    for _ in 0..100 {
        s4_sim::tick(&mut w, &[]);
    }
    let c = w
        .entities
        .iter()
        .find(|(_, e)| e.obj_type == ObjType::Settler && e.kind == 1 && e.flags & flag::OFFERED != 0)
        .map(|(i, _)| i)
        .expect("an offered carrier");
    let e = w.entities.by_index(c as u32).unwrap();
    let eco = s4_sim::eco::eco_of(&w, w.grid.index(e.x() as u32, e.y() as u32));
    let before = w.economy.get(eco).unwrap().settlers[1];
    w.kill(c, 0);
    assert_eq!(w.economy.get(eco).unwrap().settlers[1], before - 1, "out of the count");
    let mut id = w.economy.get(eco).unwrap().settler_offer[1];
    while id != 0 {
        assert_ne!(id, c, "still offered");
        id = w.entities.by_index(id as u32).unwrap().next;
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
    // Its id is reused only after the destroy delay; the lists stay sound meanwhile.
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
        s4_sim::eco::check_offer_lists(&w).unwrap();
    }
}

/// `GetPileOutOfOffer` with an eco-sector whose list does not hold the pile (a pile whose node
/// changed owner before its building died): the head of the list that holds it is fixed, the
/// other list is left alone ([deviation] from the release, which writes the given head).
#[test]
fn unlinking_from_the_wrong_eco_sector_keeps_both_lists() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let ecos: Vec<u16> = (1..64).filter(|&e| w.economy.get(e).is_some()).take(2).collect();
    assert_eq!(ecos.len(), 2, "two eco-sectors");
    let (a, b) = (ecos[0], ecos[1]);
    let good = 1u8;
    let pile = w
        .entities
        .iter()
        .find(|(_, e)| e.obj_type == ObjType::Pile && e.flags & flag::OFFERED == 0)
        .map(|(i, _)| i)
        .expect("a pile not offered");
    if let Some(s4_sim::entity::EntityData::Pile(d)) = w.entities.by_index_mut(pile as u32).map(|e| &mut e.data) {
        d.good = good;
    }
    let b_head = w.economy.get(b).unwrap().good_offer[good as usize];
    s4_sim::eco::set_good_offer(&mut w, a, good, pile);
    assert_eq!(w.economy.get(a).unwrap().good_offer[good as usize], pile);
    s4_sim::eco::get_pile_out_of_offer(&mut w, b, pile);
    assert_ne!(w.economy.get(a).unwrap().good_offer[good as usize], pile, "a's head fixed");
    assert_eq!(w.economy.get(b).unwrap().good_offer[good as usize], b_head, "b's head untouched");
    assert_eq!(w.entities.by_index(pile as u32).unwrap().flags & flag::OFFERED, 0);
}

/// `CBuilding::Delete` (0x5361d0) → 0x56e130: every observer's `SetObserverTarget(0, 0)` at once —
/// a worker's home (role +0x20) becomes 0 and its ATTACHED flag goes, a pile is set free.
#[test]
fn a_deleted_building_releases_its_workers_and_piles() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    for _ in 0..300 {
        s4_sim::tick(&mut w, &[]);
    }
    let home_of = |w: &s4_sim::World, s: u16| match w.entities.by_index(s as u32).map(|e| &e.data) {
        Some(s4_sim::entity::EntityData::Settler(d)) => d.job.home,
        _ => 0,
    };
    let (worker, b) = w
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Settler && e.flags & flag::ATTACHED != 0)
        .map(|(i, _)| (i, home_of(&w, i)))
        .find(|&(_, h)| w.entities.by_index(h as u32).is_some_and(|e| e.obj_type == ObjType::Building))
        .expect("a worker with a home");
    let piles: Vec<u16> = w
        .entities
        .iter()
        .filter(|(_, e)| matches!(&e.data, s4_sim::entity::EntityData::Pile(d) if d.building == b))
        .map(|(i, _)| i)
        .collect();
    w.kill(b, 0);
    assert_eq!(home_of(&w, worker), 0, "worker {worker} still has home {b}");
    assert_eq!(w.entities.by_index(worker as u32).unwrap().flags & flag::ATTACHED, 0);
    for p in piles {
        let d = s4_sim::pile::get(&w, p).unwrap();
        assert_eq!((d.building, d.role), (0, s4_sim::pile::role::ANYWHERE), "pile {p}");
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
}

/// A crushed construction site releases its diggers / builders at once as well (0x56e130; the
/// site's observers are its workers and its material piles).
#[test]
fn a_crushed_site_releases_its_workers() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let home_of = |w: &s4_sim::World, s: u16| match w.entities.by_index(s as u32).map(|e| &e.data) {
        Some(s4_sim::entity::EntityData::Settler(d)) => d.job.home,
        _ => 0,
    };
    let (bx, by) = w.entities.iter().find(|(_, e)| e.owner == 1 && e.obj_type == ObjType::Building).map(|(_, e)| (e.x(), e.y())).unwrap();
    let mut cmds = Vec::new();
    for (dx, dy) in [(8, 4), (-8, 6), (5, -9), (-6, -7), (12, 0)] {
        cmds.push(s4_sim::PlayerCommand::new(1, s4_sim::Command::Build { building: 2, x: (bx + dx) as u16, y: (by + dy) as u16 }));
    }
    s4_sim::tick(&mut w, &cmds);
    let mut found = None;
    for _ in 0..1500 {
        s4_sim::tick(&mut w, &[]);
        found = w.sites.roles.keys().copied().find_map(|site| {
            w.entities.iter().find(|(i, e)| e.obj_type == ObjType::Settler && home_of(&w, *i) == site).map(|(i, _)| (site, i))
        });
        if found.is_some() {
            break;
        }
    }
    let Some((site, worker)) = found else { panic!("no worker at a site") };
    s4_sim::tick(&mut w, &[s4_sim::PlayerCommand::new(1, s4_sim::Command::CrushBuilding { building: site as u32 })]);
    assert_eq!(home_of(&w, worker), 0, "worker {worker} still has site {site}");
    for _ in 0..300 {
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
}
