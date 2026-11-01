//! A player who leaves a network game (command 4009, `Command::PlayerLeft`, injected by the
//! network layer at the same tick on every peer): it becomes a computer player at the control
//! the host gives (0x5a2da0: GlobalGameState +0x6c) and gets an AI (event 0x39 → 0x517795
//! `AddPlayer`) unless it is out of the game already (record byte 0x149b608). Skipped without the
//! game data.

use s4_sim::{Command, PlayerCommand, Rules, World};

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let mut setup = s4_sim::NewGame::default();
    setup.control[0] = 1;
    setup.control[1] = 1;
    s4_sim::new_game(&map, Rules::load_default().ok()?, &setup).ok().map(|(w, _)| w)
}

#[test]
fn a_player_who_leaves_gets_an_ai() {
    let (Some(mut w), Some(mut out)) = (roman02(), roman02()) else { return };
    assert!(s4_sim::ai::player::get(&w, 2).is_none());
    let left = PlayerCommand { player: 2, command: Command::PlayerLeft { player: 2, control: 3 } };
    s4_sim::tick(&mut w, std::slice::from_ref(&left));
    assert_eq!(w.setup.control[2], 3);
    assert!(s4_sim::ai::player::get(&w, 2).is_some(), "player 2 has an AI");
    // The AI acts from then on.
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(!w.ai.players[2].as_ref().unwrap().sched.queue.is_empty());
    // A player out of the game already gets none.
    s4_sim::ai::player_out(&mut out, 2);
    s4_sim::tick(&mut out, &[left]);
    assert_eq!(out.setup.control[2], 3);
    assert!(s4_sim::ai::player::get(&out, 2).is_none(), "no AI for a player out of the game");
}
