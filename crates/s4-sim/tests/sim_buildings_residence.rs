//! `CResidenceBuildingRole` (release Init 0x54db60, LogicUpdate 0x54dca0) on roman02, driven by
//! `tick()` only: a small residence set up like a map building brings a carrier into the world
//! next to its door every work cycle until its beds are taken (+0x17c), and adds its beds to the
//! player's. Skipped when the game data is not present.

use s4_sim::World;
use s4_sim::entity::{EntityData, ObjType};

const RESIDENCE_SMALL: u8 = 40;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok()?;
    let towers: Vec<(i32, i32)> = w
        .entities
        .iter()
        .filter_map(|(id, e)| match &e.data {
            EntityData::Building(_) if s4_sim::buildings::have_inhabitant(&w, id) => Some((e.x(), e.y())),
            _ => None,
        })
        .collect();
    for (x, y) in towers {
        s4_sim::territory::set_owner(&mut w, x, y);
    }
    Some(w)
}

fn settlers(w: &World) -> i32 {
    w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1).count() as i32
}

/// (residents per 100 ticks, settlers gained, beds of the type, total beds gained, hash).
fn run() -> Option<(Vec<u8>, i32, i32, i32, u64)> {
    let mut w = roman02()?;
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y()))?;
    let size = w.grid.size() as i32;
    let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, RESIDENCE_SMALL, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())?;
    let beds_before = s4_sim::eco::current_total_beds(&w, 1);
    let before = settlers(&w);
    assert!(w.economy.strikers[1].is_empty());
    let home = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, RESIDENCE_SMALL).expect("residence");
    let beds = w.rules.building(0, RESIDENCE_SMALL).beds;
    let gained_beds = s4_sim::eco::current_total_beds(&w, 1) - beds_before;
    let mut residents = Vec::new();
    for t in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        if (t + 1) % 100 == 0 {
            residents.push(s4_sim::buildings::residence::residents(&w, home)?);
        }
    }
    Some((residents, settlers(&w) - before, beds, gained_beds, w.state_hash()))
}

#[test]
fn residence_fills_its_beds() {
    let Some(a) = run() else { return };
    eprintln!("{a:?}");
    let (residents, gained, beds, gained_beds, _) = &a;
    assert_eq!(*gained_beds, *beds, "the residence adds its beds");
    assert_eq!(*residents.last().unwrap() as i32, *beds, "all beds taken");
    assert!(residents.windows(2).all(|w| w[0] <= w[1]));
    assert!(residents[0] < residents[residents.len() - 1], "one settler per work cycle");
    assert_eq!(*gained, *beds, "one new settler per bed");
    assert_eq!(Some(a), run(), "not deterministic");
}
