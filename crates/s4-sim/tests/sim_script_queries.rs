//! The script queries (`query.rs`) on the campaign map roman01, whose script leaves victory to
//! the default rule (`ScriptDefaultVictoryConditionCheck` → `Game.DefaultPlayersLostCheck`).

use s4_sim::entity::ObjType;
use s4_sim::{Rules, World};

fn roman01() -> Option<World> {
    let p = s4_formats::game_dir().join(r"Map\Campaign\roman01.map");
    let map = s4_formats::map::MapFile::open(p).ok()?;
    let (w, _) = s4_sim::new_game(&map, Rules::load_default().ok()?, &s4_sim::NewGame::default()).ok()?;
    Some(w)
}

#[test]
fn roman01_queries_and_default_victory() {
    let Some(mut w) = roman01() else {
        eprintln!("skipping: roman01 not found");
        return;
    };
    use s4_sim::query::{ALL, ANY_SETTLER, READY, UNDER_CONSTRUCTION};
    assert_eq!(w.number_of_players(), 2);
    // Player 1: two small towers (46) at (288, 135) and (301, 114); player 2: three.
    assert_eq!((w.buildings_amount(1, 46, READY), w.buildings_amount(2, 46, ALL), w.buildings_amount(2, 46, UNDER_CONSTRUCTION)), (2, 3, 0));
    assert_eq!(w.buildings_amount(9, 46, READY), 0);
    assert!(w.exists_building_in_area(2, 46, 113, 160, 2, READY));
    assert!(!w.exists_building_in_area(2, 46, 113, 160, 1, READY));
    assert!(!w.exists_building_in_area(2, 46, 113, 158, 5, UNDER_CONSTRUCTION));
    assert!(w.is_area_owned(1, 288, 135, 5) && !w.is_area_owned(2, 288, 135, 5));
    assert!(w.is_area_green(288, 135, 5));
    assert!(w.are_player_areas_connected(1, 288, 135, 1, 301, 114));
    assert!(!w.are_player_areas_connected(1, 288, 135, 2, 113, 158), "different alliances");
    assert!(w.settlers_amount_in_area(1, ANY_SETTLER, 288, 135, 30) > 0);
    assert_eq!(w.settlers_amount_in_area(2, ANY_SETTLER, 288, 135, 5), 0);
    assert!(!w.is_almost_all_land_explored(), "no fog");

    // The default rule: nobody has lost at the start; without its towers player 2 has.
    assert!(!w.default_player_lost(1) && !w.default_player_lost(2));
    let towers: Vec<u16> =
        w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == 2 && e.kind == 46).map(|(i, _)| i).collect();
    for (k, &t) in towers.iter().enumerate() {
        assert!(!w.default_player_lost(2), "tower {k}");
        w.kill(t, 1);
    }
    assert!(w.default_player_lost(2) && !w.default_player_lost(1));
    // Killed but not yet destroyed, the towers still count for Buildings.Amount.
    assert_eq!(w.buildings_amount(2, 46, READY), 3);
}
