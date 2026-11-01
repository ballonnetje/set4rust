//! The computer players' state survives the savegame body (bincode of `s4_sim::ai::Ai`, as
//! `save.rs` writes it): Cordoba with every player a computer player, 4000 ticks, the AI state
//! written and read back into a copy of the world; both then run 4000 more ticks with equal state
//! hashes (squads, attacks, goal caches, cursors, task-force timers, eco-sector AIs, AI infos).
//! Skipped without the game data.

#[test]
fn ai_state_round_trips_through_the_save_body() {
    let p = s4_formats::game_dir().join("Map").join("Singleplayer").join("Cordoba.map");
    let Ok(map) = s4_formats::map::MapFile::open(&p) else { return };
    let setup = s4_sim::NewGame { control: [3; 8], ..Default::default() };
    let (mut a, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default().unwrap(), &setup).unwrap();
    for p in 1..=8usize {
        let level = a.ai.vars.records[p].level as usize;
        a.ai.vars.records[p].columns[level][1] = 5;
    }
    for _ in 0..4000 {
        s4_sim::tick(&mut a, &[]);
    }
    let bytes = bincode::serialize(&a.ai).unwrap();
    let mut b = a.clone();
    b.ai = bincode::deserialize(&bytes).unwrap();
    assert_eq!(a.state_hash(), b.state_hash());
    for t in 1..=4000 {
        s4_sim::tick(&mut a, &[]);
        s4_sim::tick(&mut b, &[]);
        if t % 250 == 0 {
            assert_eq!(a.state_hash(), b.state_hash(), "tick {t} after the round trip");
        }
    }
}
