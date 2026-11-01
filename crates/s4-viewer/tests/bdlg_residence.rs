//! D15 (house dialog, code 0) of a residence: `CResidenceBuildingRole` vt+0x58 (0x54df70) writes
//! `max(0, buildingInfo +0x1f0 − role +0x17c)` (the free beds) into info +0x13, and the update
//! 0x4709c0 shows it at link 472 for the residences (40..=42). The sim's residence brings one
//! settler per work cycle (`s4_sim::buildings::residence`), so the shown number counts down from
//! the beds to 0. Runs on roman02 when the game data is present.

use s4_sim::World;
use s4_sim::entity::EntityData;
use s4_viewer::building_dialog::{Facts, Infos, fill};

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

#[test]
fn residence_free_beds_count_down() {
    let Some(mut w) = roman02() else { return };
    let castle = w.entities.iter().find(|(id, e)| e.owner == 1 && matches!(&e.data, EntityData::Building(_)) && s4_sim::buildings::have_inhabitant(&w, *id)).map(|(_, e)| (e.x(), e.y())).unwrap();
    let size = w.grid.size() as i32;
    let &(x, y, _) = s4_sim::build::possible_sites(&w, 1, RESIDENCE_SMALL, (0, 0), (size, size))
        .iter()
        .min_by_key(|s| (s.0 as i32 - castle.0).abs() + (s.1 as i32 - castle.1).abs())
        .unwrap();
    let home = s4_sim::create::add_building(&mut w, x as i32, y as i32, 1, RESIDENCE_SMALL).expect("residence");
    let beds = w.rules.building(0, RESIDENCE_SMALL).beds;
    let shown = |w: &World| {
        let f = Facts::from_world(w, home).unwrap();
        let mut infos = Infos::default();
        assert_eq!(fill(&mut infos, &f), Some(0), "D15");
        infos.obj[0][0x13] as i32
    };
    let mut seen = vec![shown(&w)];
    for _ in 0..2000 {
        s4_sim::tick(&mut w, &[]);
        let n = shown(&w);
        assert_eq!(n, beds - s4_sim::buildings::residence::residents(&w, home).unwrap() as i32);
        if seen.last() != Some(&n) {
            seen.push(n);
        }
    }
    eprintln!("residence at {x},{y}: beds {beds}, shown {seen:?}");
    assert_eq!(seen, (0..=beds).rev().collect::<Vec<_>>(), "one bed fewer per settler, from {beds} to 0");
}
