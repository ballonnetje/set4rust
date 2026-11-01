//! Dark gardeners (`CDarkGardenerRole`, release vtable 0x105ad04, role 14) on dark03, driven by
//! `tick()` and a SendTo command only: player 2's gardeners, sent next to light land, plant
//! shadow herbs around the place (`JOB_DARKGARDENER_WORK`) and the herbs darken the land.
//! Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType};

const DARK_GARDENER: u16 = 52;

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

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    hash: u64,
    planted: usize,
    darkened: usize,
    far: usize,
}

fn run(ticks: u32) -> Option<Outcome> {
    let mut w = map()?;
    let gardeners: Vec<u16> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.kind == DARK_GARDENER).map(|(i, _)| i).take(3).collect();
    assert_eq!(gardeners.len(), 3, "dark03 has dark gardeners");
    let g = w.entities.by_index(gardeners[0] as u32).unwrap();
    let (gx, gy, owner) = (g.x(), g.y(), g.owner);
    // The nearest walkable light grass node.
    let (tx, ty) = (0..s4_sim::spiral::last(30))
        .map(|k| (gx + s4_sim::spiral::dx(k), gy + s4_sim::spiral::dy(k)))
        .find(|&(x, y)| w.grid.checked_index(x, y).is_some_and(|n| w.layers.flags[n] & 5 == 0 && w.layers.ground[n] & 0xf0 == 0x10))
        .expect("light land nearby");
    let dark0 = dark_nodes(&w);
    let cmd = s4_sim::PlayerCommand { player: owner, command: s4_sim::Command::SendTo { x: tx as u16, y: ty as u16, mode: 0, entities: gardeners.clone() } };
    s4_sim::tick(&mut w, &[cmd]);
    let mut herbs = std::collections::BTreeSet::new();
    for _ in 0..ticks {
        s4_sim::tick(&mut w, &[]);
        for (id, e) in w.entities.iter() {
            if e.obj_type == ObjType::Deco && e.kind == 216 {
                herbs.insert((id, e.x(), e.y()));
            }
        }
        assert!(!w.events.iter().any(|e| matches!(e, s4_sim::event::SimEvent::Unported { what, .. } if what.contains("DarkGardener"))));
    }
    for &id in &gardeners {
        let e = w.entities.by_index(id as u32).unwrap();
        let EntityData::Settler(s) = &e.data else { panic!() };
        assert_eq!(s.job.list_id, 160, "gardener {id} works");
        // Its work centre is its place of the formation around the click (`group::formation`).
        let (cx, cy) = s.job.gardener.centre;
        assert!(s4_sim::spiral::grid_distance(cx - tx, cy - ty) <= 4, "gardener {id} centre ({cx},{cy})");
    }
    // Herbs go in around the work centre (the search walks the spiral around the gardener and
    // prefers nodes near the centre).
    let far = herbs.iter().filter(|&&(_, x, y)| s4_sim::spiral::grid_distance(x - tx, y - ty) > 20).count();
    Some(Outcome { hash: w.state_hash(), planted: herbs.len(), darkened: dark_nodes(&w) - dark0, far })
}

#[test]
fn sent_dark_gardeners_plant_shadow_herbs() {
    let Some(a) = run(1500) else { return };
    eprintln!("{a:?}");
    assert!(a.planted >= 6, "too few herbs");
    assert!(a.darkened >= 100, "too little dark land");
    assert_eq!(a.far, 0, "herbs far from the work centre");
    assert_eq!(Some(a), run(1500), "not deterministic");
}
