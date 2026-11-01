//! `NewGame::players`: a map started with fewer players than slots (the map settings' count).
//! Skipped without the game data.

use s4_sim::entity::ObjType;

fn start(name: &str, players: Option<u8>, control: [u8; 8]) -> Option<s4_sim::World> {
    let p = s4_formats::game_dir().join("Map").join("Multiplayer").join(name);
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame { players, control, ..Default::default() }).ok().map(|(w, _)| w)
}

#[test]
fn slots_above_the_count_have_no_player() {
    let Some(mut w) = start("Deadsimple.map", Some(2), [3; 8]) else { return };
    assert!(w.player(1).is_some() && w.player(2).is_some());
    assert!((3..=8).all(|p| w.player(p).is_none()), "players 3..8 do not play");
    assert_eq!(w.player_count(), 2);
    // Only playing slots get an AI (0x4ff5f0 loops to PlayerCount).
    assert!(w.ai.is_ai(2) && !(3..=8).any(|p| w.ai.is_ai(p)));
    for _ in 0..500 {
        s4_sim::tick(&mut w, &[]);
    }
}

/// [inf] The preplaced entities of a slot that does not play stay (no loader reads PlayerCount).
#[test]
fn preplaced_entities_of_missing_slots_stay_and_the_game_runs() {
    let Some(all) = start("Roemerberg.map", None, [0; 8]) else { return };
    let Some(mut w) = start("Roemerberg.map", Some(3), [0; 8]) else { return };
    let count = |w: &s4_sim::World, owner: u8| w.entities.iter().filter(|(_, e)| e.owner == owner && matches!(e.obj_type, ObjType::Settler | ObjType::Building)).count();
    assert!(w.player(4).is_none());
    assert_eq!(count(&w, 5), count(&all, 5), "slot 5's entities are loaded");
    for _ in 0..3000 {
        s4_sim::tick(&mut w, &[]);
    }
    s4_sim::eco::check_offer_lists(&w).unwrap();
}
