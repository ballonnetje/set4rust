//! D20 (the small temple, fill 0x54fe40) on the sim: a finished small temple of player 1 on
//! roman02 (as `s4-sim/tests/sim_magic.rs`) with three bottles of wine on its offering pile
//! shows the pile's good / amount at +0x14 / +0x15 and the player's mana at +0x18 while the
//! servant turns the wine into mana. Skipped without the game data.

use s4_sim::World;
use s4_sim::entity::EntityData;
use s4_viewer::building_dialog::{Facts, Infos, fill};

const SMALLTEMPLE: u8 = 43;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> =
        w.entities.iter().filter_map(|(id, e)| matches!(e.data, EntityData::Building(_)).then_some(id).filter(|&id| s4_sim::buildings::have_inhabitant(&w, id)).map(|_| (e.x(), e.y()))).collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

#[test]
fn small_temple_dialog_shows_offerings_and_mana() {
    let Some(mut w) = roman02() else { return };
    let size = w.grid.size() as i32;
    let (cx, cy) = w.player(1).map(|p| (p.start_x as i32, p.start_y as i32)).unwrap();
    let free = |w: &World, x: i32, y: i32| w.rules.building(0, SMALLTEMPLE).building_cells.iter().all(|&(dx, dy)| w.grid.checked_index(x + dx as i32, y + dy as i32).is_some_and(|n| w.layers.map_object[n] == 0));
    let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, SMALLTEMPLE, (0, 0), (size, size))
        .iter()
        .filter(|s| free(&w, s.0 as i32, s.1 as i32))
        .min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs())
        .expect("a site");
    let t = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, SMALLTEMPLE).unwrap();
    let pile = s4_sim::buildings::temple::small_state(&w, t).unwrap().pile;
    if let Some(EntityData::Pile(p)) = w.entities.by_index_mut(pile as u32).map(|e| &mut e.data) {
        p.amount = 3;
    }
    let shown = |w: &World| {
        let f = Facts::from_world(w, t).unwrap();
        let mut infos = Infos::default();
        assert_eq!(fill(&mut infos, &f), Some(8));
        let o = &infos.obj[8];
        (o[0x14], o[0x15], i32::from_le_bytes(o[0x18..0x1c].try_into().unwrap()))
    };
    let wine = s4_sim::pile::get(&w, pile).unwrap().good;
    assert_eq!(shown(&w), (wine, 3, 0));
    let mut seen_mana = 0;
    for _ in 0..3000 {
        s4_sim::tick(&mut w, &[]);
        let (good, amount, mana) = shown(&w);
        let p = s4_sim::pile::get(&w, pile).unwrap();
        assert_eq!((good, amount), (p.good, p.amount));
        assert_eq!(mana, s4_sim::magic::mana(&w, 1));
        seen_mana = seen_mana.max(mana);
    }
    assert!(seen_mana > 0, "the offerings turned into mana");
}
