//! Gardeners (`CGardenerRole`, release vtable 0x105b51c, role 13) on dark03, driven by `tick()`
//! and a SendTo command only: three gardeners of player 1, sent onto the Dark Tribe's land, look
//! for dark nodes around them (0x579f80) and turn them light (`CDarkTribe` vtbl+0x10). Skipped
//! when the game data is not present.

use s4_sim::World;
use s4_sim::entity::EntityData;

const GARDENER: u16 = 50;

fn map() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("dark03.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

fn dark_nodes(w: &World) -> usize {
    w.layers.flags.iter().filter(|&&f| f & 4 != 0).count()
}

fn run(ticks: u32) -> Option<(u64, usize)> {
    let mut w = map()?;
    let size = w.grid.size() as i32;
    // The first walkable light grass node (row by row) with dark land within 3.
    let (x, y) = (2..size - 2)
        .flat_map(|y| (2..size - 2).map(move |x| (x, y)))
        .find(|&(x, y)| {
            let n = w.grid.index(x as u32, y as u32);
            w.layers.flags[n] & 5 == 0
                && w.layers.map_object[n] == 0
                && w.layers.ground[n] & 0xf0 == 0x10
                && (1..=s4_sim::spiral::last(3)).any(|k| w.grid.checked_index(x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k)).is_some_and(|m| w.layers.flags[m] & 4 != 0))
        })
        .expect("light land next to dark land");
    let mut gardeners = Vec::new();
    for k in 0..30 {
        let (px, py) = (x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k));
        let n = w.grid.index(px as u32, py as u32);
        if gardeners.len() < 3 && w.layers.flags[n] & 1 == 0 && w.layers.map_object[n] == 0 {
            gardeners.extend(s4_sim::create::add_settler(&mut w, px, py, 1, GARDENER));
        }
    }
    let target = (1..=s4_sim::spiral::last(3))
        .map(|k| (x + s4_sim::spiral::dx(k), y + s4_sim::spiral::dy(k)))
        .find(|&(a, b)| w.layers.flags[w.grid.index(a as u32, b as u32)] & 5 == 4)
        .expect("a dark node");
    let dark0 = dark_nodes(&w);
    let cmd = s4_sim::PlayerCommand { player: 1, command: s4_sim::Command::SendTo { x: target.0 as u16, y: target.1 as u16, mode: 0, entities: gardeners.clone() } };
    s4_sim::tick(&mut w, &[cmd]);
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        assert!(!w.events.iter().any(|e| matches!(e, s4_sim::event::SimEvent::Unported { what, .. } if what.contains("Gardener") || what.contains("CDarkTribe"))));
    }
    for &id in &gardeners {
        let EntityData::Settler(s) = &w.entities.by_index(id as u32).unwrap().data else { panic!() };
        assert_eq!(s.job.role, 13);
        // Lightening dark land (list 175), or beating a mushroom (174).
        assert!(matches!(s.job.list_id, 174 | 175), "gardener {id} list {}", s.job.list_id);
    }
    Some((w.state_hash(), dark0 - dark_nodes(&w)))
}

#[test]
fn sent_gardeners_turn_dark_land_light() {
    let Some(a) = run(1500) else { return };
    eprintln!("hash {:x}, nodes lightened {}", a.0, a.1);
    assert!(a.1 >= 60, "too little light land");
    assert_eq!(Some(a), run(1500), "not deterministic");
}
