//! A Dark Tribe player building in play (dark03, 20 000 ticks). The Dark Tribe builds only
//! mushroom farms (49) and manakopter halls (80) at run time — its AI sends 5025 for type 0x50
//! (0x44b260) and the scripts add farms (dark07); the temple (50) and the fortress (51) are only
//! preplaced. Both go through `CheckForBuildDarkTribe` (0x53ebc0) and the site states 6 → 8
//! (0x541cc0). Here the AI's orders are stood in for by commands every 500 ticks: every order
//! that the check accepts must end as a finished building, and the economy's lists stay sound.

use s4_sim::World;
use s4_sim::build::check::CheckResult;

#[test]
fn a_dark_tribe_player_builds_farms_and_halls_for_20000_ticks() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("dark03.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let dt = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3)).expect("a Dark Tribe player");
    let size = w.grid.size() as i32;
    let spot = |w: &World, kind: u8| {
        (20..size - 20).step_by(3).flat_map(|y| (20..size - 20).step_by(3).map(move |x| (x, y))).find(|&(x, y)| {
            w.layers.flags[w.grid.index(x as u32, y as u32)] & 0x5F == 4
                && matches!(s4_sim::build::check::check_for_build(w, x, y, dt, kind), CheckResult::Possible { .. })
        })
    };
    let mut ordered = Vec::new();
    for t in 1..=20_000u32 {
        let mut cmds = Vec::new();
        if t % 500 == 0 && ordered.len() < 20 {
            // A hall when there is room for one (its footprint rarely fits on dark03), else a farm.
            let want_hall = ordered.len() % 4 == 3;
            let order = want_hall.then(|| spot(&w, 80).map(|s| (80u8, s))).flatten().or_else(|| spot(&w, 49).map(|s| (49u8, s)));
            if let Some((kind, (x, y))) = order {
                cmds.push(s4_sim::PlayerCommand::new(dt, s4_sim::Command::Build { building: kind as u32, x: x as u16, y: y as u16 }));
                ordered.push((kind, x, y));
            }
        }
        s4_sim::tick(&mut w, &cmds);
        if t % 1000 == 0 {
            s4_sim::eco::check_offer_lists(&w).unwrap();
        }
    }
    assert!(ordered.len() >= 10, "only {} orders placed", ordered.len());
    let finished = |kind: u8| {
        w.entities
            .iter()
            .filter(|(_, e)| {
                e.owner == dt && e.kind == kind as u16 && e.obj_type == s4_sim::entity::ObjType::Building && e.flags & s4_sim::entity::flag::READY != 0
            })
            .count()
    };
    let sites_left = w.sites.roles.keys().filter(|&&id| w.entities.by_index(id as u32).is_some_and(|e| e.owner == dt)).count();
    eprintln!("{} orders; finished farms {}, halls {}; Dark Tribe sites left {sites_left}", ordered.len(), finished(49), finished(80));
    assert_eq!(sites_left, 0, "Dark Tribe sites never finished");
}

/// A manakopter hall (type 80) ordered for the Dark Tribe on MD_roman2 (one of the few maps with
/// room for one) is levelled, placed and finished without builders (site states 6 → 8).
#[test]
fn a_dark_tribe_manakopter_hall_is_finished() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MD_roman2.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let dt = (1..=8u8).find(|&p| w.player(p).is_some_and(|q| q.race == 3)).expect("a Dark Tribe player");
    let size = w.grid.size() as i32;
    let (x, y) = (20..size - 20)
        .flat_map(|y| (20..size - 20).map(move |x| (x, y)))
        .find(|&(x, y)| matches!(s4_sim::build::check::check_for_build(&w, x, y, dt, 80), CheckResult::Possible { .. }))
        .expect("room for a hall");
    let before = w.sites.roles.len();
    s4_sim::tick(&mut w, &[s4_sim::PlayerCommand::new(dt, s4_sim::Command::Build { building: 80, x: x as u16, y: y as u16 })]);
    assert_eq!(w.sites.roles.len(), before + 1);
    let hall = w.sites.roles.keys().copied().max().unwrap();
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        if !w.sites.roles.contains_key(&hall) {
            break;
        }
    }
    assert!(!w.sites.roles.contains_key(&hall), "the hall site never finished");
    let e = w.entities.by_index(hall as u32).unwrap();
    assert_ne!(e.flags & s4_sim::entity::flag::READY, 0);
    assert_eq!(e.kind, 80);
}
