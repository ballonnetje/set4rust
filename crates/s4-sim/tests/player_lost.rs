//! A player declared lost (the client's event 0x38, 0x517631..0x5176b9 on every peer,
//! `s4_sim::ai::player_lost`): it is out of the game, a computer player loses its AI, and every
//! working building but the towers and the castle is switched off, so its economy stops. On
//! dark02, whose Maya computer (player 2) is what the player beats there. Skipped without the
//! game data.

use s4_sim::entity::ObjType;
use s4_sim::{Rules, World};

const WORKING: u32 = 0x1000;

fn dark02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("dark02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let mut setup = s4_sim::NewGame::default();
    setup.control[1] = 2;
    s4_sim::new_game(&map, Rules::load_default().ok()?, &setup).ok().map(|(w, _)| w)
}

/// Player `p`'s buildings (sites too) that are not dead: (id, type, flags).
fn buildings(w: &World, p: u8) -> Vec<(u16, u16, u32)> {
    w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == p && !e.is_dead()).map(|(i, e)| (i, e.kind, e.flags)).collect()
}

#[test]
fn a_lost_player_stops() {
    let Some(mut w) = dark02() else { return };
    assert_eq!(w.player(2).map(|p| p.race), Some(2), "dark02's player 2 is the Maya computer");
    for _ in 0..3000 {
        s4_sim::tick(&mut w, &[]);
    }
    assert!(s4_sim::ai::player::get(&w, 2).is_some(), "the computer plays");
    let before = buildings(&w, 2);
    assert!(before.iter().any(|&(_, k, f)| !matches!(k, 0x2e..=0x30) && f & WORKING != 0), "it has working buildings");

    s4_sim::ai::player_lost(&mut w, 2);
    assert_ne!(w.setup.out & (1 << 2), 0, "out of the game");
    assert!(s4_sim::ai::player::get(&w, 2).is_none(), "no AI any more");
    for (id, k, _) in buildings(&w, 2) {
        let f = w.entities.by_index(id as u32).unwrap().flags;
        let was = before.iter().find(|b| b.0 == id).unwrap().2;
        if matches!(k, 0x2e..=0x30) {
            assert_eq!(f & WORKING, was & WORKING, "towers and castles are left as they were ({id}, type {k})");
        } else {
            assert_eq!(f & WORKING, 0, "building {id} (type {k}) is switched off");
        }
    }

    // From then on nothing is ordered, built or switched on again.
    for _ in 0..3000 {
        s4_sim::tick(&mut w, &[]);
    }
    let after = buildings(&w, 2);
    assert!(after.iter().all(|a| before.iter().any(|b| b.0 == a.0)), "no new building or site");
    assert!(after.iter().all(|&(_, k, f)| matches!(k, 0x2e..=0x30) || f & WORKING == 0), "every building stays off");
    // The other players play on.
    assert!(s4_sim::ai::player::get(&w, 3).is_some() || w.setup.control[3] == 0);
}
