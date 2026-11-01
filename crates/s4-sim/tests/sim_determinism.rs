//! Same map + seed + commands → same state hash; a different seed or command → a different one.
//! Skipped when the install is absent.

use s4_formats::map::MapFile;
use s4_sim::{Command, EntityData, NewGame, ObjType, PlayerCommand, Rules, World, new_game, tick};

const TICKS: u32 = 300;

fn load(seed: u32) -> Option<World> {
    let p = s4_formats::game_dir().join(r"Map\Campaign\AO_maya1.map");
    if !p.exists() {
        eprintln!("skipping: {} not found", p.display());
        return None;
    }
    let map = MapFile::open(p).unwrap();
    let rules = Rules::load_default().unwrap();
    Some(new_game(&map, rules, &NewGame { seed, ..NewGame::default() }).unwrap().0)
}

/// First building and its owner, and its current working area.
fn some_building(w: &World) -> (u16, u8, u32) {
    w.entities
        .iter()
        .find_map(|(i, e)| match (&e.obj_type, &e.data) {
            (ObjType::Building, EntityData::Building(d)) if !e.is_dead() => Some((i, e.owner, d.working_area.0)),
            _ => None,
        })
        .expect("map has a building")
}

fn run(mut w: World, script: &[(u32, PlayerCommand)]) -> (u64, Vec<u64>) {
    let mut trail = Vec::new();
    for t in 1..=TICKS {
        let cmds: Vec<PlayerCommand> = script.iter().filter(|(at, _)| *at == t).map(|(_, c)| c.clone()).collect();
        tick(&mut w, &cmds);
        if t % 50 == 0 {
            trail.push(w.state_hash());
        }
    }
    assert_eq!(w.tick, TICKS);
    (w.state_hash(), trail)
}

#[test]
fn same_inputs_same_hash_and_inputs_matter() {
    let Some(base) = load(s4_sim::rng::GAME_SEED) else { return };
    let (b, player, _) = some_building(&base);
    let set_area = |x, y| PlayerCommand::new(player, Command::SetWorkingArea { building: b as u32, x, y });
    let script = vec![(10, set_area(100, 120)), (200, PlayerCommand::new(player, Command::Build { building: 1, x: 5, y: 5 }))];

    let (h1, t1) = run(base.clone(), &script);
    let (h2, t2) = run(base.clone(), &script);
    assert_eq!((h1, &t1), (h2, &t2), "identical runs diverged");

    // The command reaches the state (working area is part of the hash).
    let other = vec![(10, set_area(101, 120)), script[1].clone()];
    let (h3, _) = run(base.clone(), &other);
    assert_ne!(h1, h3, "different command, same hash");

    // A command from a non-owner is rejected by GetAliveBuildingPtrOfPlayer: same as no command.
    let foreign = vec![(10, PlayerCommand::new(player % 8 + 1, Command::SetWorkingArea { building: b as u32, x: 1, y: 1 }))];
    let (h4, _) = run(base.clone(), &foreign);
    let (h5, _) = run(base.clone(), &[]);
    assert_eq!(h4, h5);

    // Commands round-trip through the original message shape without changing the outcome.
    let via_messages: Vec<_> =
        script.iter().map(|(t, c)| (*t, PlayerCommand::from_message(&c.to_message()))).collect();
    assert_eq!(run(base, &via_messages).0, h1);

    // A different seed changes the state already at load (settler facing, tree frames).
    let Some(seeded) = load(12345) else { return };
    assert_ne!(seeded.state_hash(), load(s4_sim::rng::GAME_SEED).unwrap().state_hash());
    assert_ne!(run(seeded, &script).0, h1, "different seed, same hash");
}

#[test]
fn schedule_runs_and_crc_moves() {
    let Some(mut w) = load(s4_sim::rng::GAME_SEED) else { return };
    // Hives and shadow herbs register at creation (delta 31 / cycle length); after 32 ticks the
    // whole ring has been visited and the CRC has been folded 32 times.
    let crc0 = w.schedule.crc();
    for _ in 0..32 {
        tick(&mut w, &[]);
    }
    assert_eq!(w.schedule.cursor(), 0);
    assert_ne!(w.schedule.crc(), crc0);
}
