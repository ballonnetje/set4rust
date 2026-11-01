//! Map effects the sim announces (`SimEvent::Effect`): the crush effect of command 5002
//! (`CBuilding::CrushBuilding`, release 0x5369c4..0x536a23). Skipped when the install is absent.

use s4_sim::event::SimEvent;
use s4_sim::{Grid, Player, Rules, World, tick};

fn world() -> Option<World> {
    if !s4_formats::game_dir().join("GameData").exists() {
        eprintln!("skipping: install not found");
        return None;
    }
    let mut w = World::new(Grid::new(64).unwrap(), Rules::load_default().unwrap());
    w.players[1] = Some(Player { id: 1, race: 0, start_x: 0, start_y: 0 });
    w.layers.ground.fill(16);
    let owner = vec![1u8; w.grid.len()];
    w.init_tiling(&owner);
    Some(w)
}

fn area(w: &World, kind: u8) -> i32 {
    let [min_x, max_x, min_y, max_y] = w.rules.building(0, kind).bounding_rect;
    (max_y as i32 - min_y as i32) * (max_x as i32 - min_x as i32)
}

/// Crushing a building adds effect 0x3e with `SOUND_AMB_CRUSHSMALLHOUSE` (3) at its position when
/// its `bitBoundingRect` area is under 90, else 0x3c with `SOUND_AMB_CRUSHBIGHOUSE` (2).
#[test]
fn crushing_announces_the_crush_effect() {
    let Some(w) = world() else { return };
    // Roman storage area (34) and castle (48): one of each size class.
    let kinds = [34u8, 48];
    assert!(area(&w, 34) < 90 && area(&w, 48) >= 90, "areas {} {}", area(&w, 34), area(&w, 48));
    for kind in kinds {
        let Some(mut w) = world() else { return };
        let (x, y) = (32, 32);
        let b = s4_sim::create::add_building(&mut w, x, y, 1, kind).unwrap();
        w.events.clear();
        tick(&mut w, &[s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::CrushBuilding { building: b as u32 } }]);
        let fx: Vec<&SimEvent> = w.events.iter().filter(|e| matches!(e, SimEvent::Effect { .. })).collect();
        let want = if area(&w, kind) < 90 { SimEvent::Effect { effect: 0x3e, sound: 3, x, y } } else { SimEvent::Effect { effect: 0x3c, sound: 2, x, y } };
        assert_eq!(fx, vec![&want], "kind {kind}");
        assert!(!w.events.iter().any(|e| matches!(e, SimEvent::Unported { what, .. } if what.contains("CEffects"))));
    }
}
