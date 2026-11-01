//! 0x536a80's branch for the last mission of the Dark Tribe's and the Trojans' campaign: a
//! destroyed building of type 0x33 stays on the map and marks its owner lost (`AO_trojan12`'s
//! `VictoryConditionCheck` asks `Game.HasPlayerLost(6)`); anywhere else it is destroyed.

use s4_sim::buildings::crush;
use s4_sim::entity::ObjType;

fn world() -> Option<s4_sim::World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("dark12.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return None };
    Some(s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap().0)
}

fn temple(w: &s4_sim::World) -> (u16, u8) {
    let (id, e) = w.entities.iter().find(|(_, e)| e.obj_type == ObjType::Building && e.kind == 0x33).expect("a building of type 0x33 on dark12");
    assert_eq!(e.race, 3, "the Dark Tribe's");
    (id, e.owner)
}

#[test]
fn the_last_missions_temple_stays_and_its_owner_loses() {
    let Some(mut w) = world() else { return };
    let (id, owner) = temple(&w);
    let hash = w.layers.map_object.clone();
    w.final_temple = true;
    crush::destroy(&mut w, id, 1);
    assert_eq!(w.script_events.lost, 1 << owner);
    assert!(w.entities.by_index(id as u32).is_some_and(|e| !e.is_dead()), "still there");
    assert_eq!(w.layers.map_object, hash, "still on the map");
    assert_eq!(w.delete_cause, 0);
}

#[test]
fn elsewhere_it_is_destroyed() {
    let Some(mut w) = world() else { return };
    let (id, _) = temple(&w);
    crush::destroy(&mut w, id, 1);
    assert_eq!(w.script_events.lost, 0);
    assert!(w.entities.by_index(id as u32).is_none_or(|e| e.is_dead()), "gone");
}
