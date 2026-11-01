//! The search routines no shipped settler reaches through the table (release 0x54ea10 venison,
//! 0x54eaa0.. resources, 0x54eb90 / 0x54ebc0 dark / green land), on dark03 after the animals
//! are out. Skipped without the game data.

use s4_sim::World;
use s4_sim::buildings::search;
use s4_sim::entity::ObjType;

fn map(name: &str) -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join(name);
    if !p.exists() {
        return None;
    }
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

#[test]
fn the_rest_of_the_search_table() {
    let Some(mut w) = map("dark03.map") else { return };
    for _ in 0..200 {
        s4_sim::tick(&mut w, &[]);
    }
    let size = w.grid.size() as i32;
    let (mut dark, mut green, mut ore) = (0, 0, [0; 5]);
    for y in 0..size {
        for x in 0..size {
            let n = w.grid.index(x as u32, y as u32);
            let is_dark = w.layers.flags[n] & 0x04 != 0;
            assert_eq!(search::run(&mut w, 23, x, y) == -1, is_dark);
            assert_eq!(search::run(&mut w, 24, x, y) == -1, !is_dark);
            dark += i32::from(is_dark);
            green += i32::from(!is_dark);
            for t in 18..=22u16 {
                let want = [0x10, 0x20, 0x10, 0x50, 0x40][(t - 18) as usize];
                let hit = search::run(&mut w, t, x, y);
                assert_eq!(hit == -1, w.layers.resource_type(n) == want, "type {t} at {x},{y}");
                ore[(t - 18) as usize] += i32::from(hit == -1);
            }
        }
    }
    assert!(dark > 0 && green > 0, "dark {dark} green {green}");
    assert!(ore.iter().any(|&c| c > 0), "no resources: {ore:?}");
    // Venison: every alive huntable animal is found on its node, nothing else is.
    let animals: Vec<_> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Animal).map(|(i, e)| (i, e.pos.x(), e.pos.y(), w.animals.types[e.kind as usize].huntable)).collect();
    assert!(animals.iter().any(|a| a.3), "no huntable animal");
    let mut found = 0;
    for (i, x, y, huntable) in animals {
        let n = w.grid.index(x as u32, y as u32);
        if w.layers.map_object[n] == i {
            assert_eq!(search::run(&mut w, 16, x, y), if huntable { i as i32 } else { 0 });
            found += i32::from(huntable);
        }
    }
    assert!(found > 0, "no huntable animal on its node");
    eprintln!("dark {dark} green {green} resources {ore:?} huntable animals found {found}");
}
