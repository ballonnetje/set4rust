//! `RefreshShading` (0x66d7b0) on height changes: a digger levelling a site leaves every node's
//! shading nibble equal to a fresh recompute from the heights (the map's own shading is that
//! formula at every node). roman02, skipped without data.

use s4_sim::World;

/// The release's formula for node `n` (0x66d7b0).
fn shade(w: &World, n: usize) -> u8 {
    let size = w.grid.size() as usize;
    let h = w.layers.height[n] as i32;
    let up = if n >= size { w.layers.height[n - size] as i32 } else { h };
    let down = if n + size < w.layers.height.len() { w.layers.height[n + size] as i32 } else { h };
    (((up - down) >> 1) + 8) as u8 & 0x0F
}

fn mismatches(w: &World) -> usize {
    (0..w.layers.height.len()).filter(|&n| w.layers.gradient[n] & 0x0F != shade(w, n)).count()
}

#[test]
fn levelling_keeps_the_shading_current() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let (Ok(map), Ok(rules)) = (s4_formats::map::MapFile::open(&p), s4_sim::Rules::load_default()) else { return };
    let (mut w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let at_load = mismatches(&w);
    // The map's shading is the formula (every node of roman02).
    assert_eq!(at_load, 0, "map shading differs from 0x66d7b0's formula");
    let before = w.layers.height.clone();
    // Sites near player 1's buildings; diggers level them.
    let (bx, by) = w.entities.iter().find(|(_, e)| e.owner == 1 && e.obj_type == s4_sim::entity::ObjType::Building).map(|(_, e)| (e.x(), e.y())).unwrap();
    let mut cmds = Vec::new();
    for (k, (dx, dy)) in [(8, 4), (-8, 6), (5, -9), (-6, -7), (12, 0)].into_iter().enumerate() {
        for kind in [2u32, 3, 4, 1] {
            cmds.push((1 + k as u32, s4_sim::PlayerCommand::new(1, s4_sim::Command::Build { building: kind, x: (bx + dx) as u16, y: (by + dy) as u16 })));
        }
    }
    for t in 1..=3000u32 {
        let c: Vec<_> = cmds.iter().filter(|(at, _)| *at == t).map(|(_, c)| c.clone()).collect();
        s4_sim::tick(&mut w, &c);
    }
    let changed: Vec<usize> = (0..before.len()).filter(|&n| before[n] != w.layers.height[n]).collect();
    eprintln!("{} nodes levelled", changed.len());
    assert!(!changed.is_empty(), "no digger levelled anything");
    assert_eq!(mismatches(&w), 0, "stale shading after levelling");
}

/// Every shipped map's stored shading (chunk 13 byte 2, low nibble) is 0x66d7b0's formula.
#[test]
fn every_maps_shading_is_the_formula() {
    let root = s4_formats::game_dir().join("Map");
    let (mut maps, mut bad) = (0, Vec::new());
    for dir in ["Campaign", "Multiplayer", "Singleplayer", "Tutorial"] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map")) {
                continue;
            }
            let Ok(land) = s4_formats::map::MapFile::open(&p).and_then(|m| m.landscape()) else { continue };
            let size = (land.nodes.len() as f64).sqrt() as usize;
            let h = |n: usize| land.nodes[n].height as i32;
            let off = (0..land.nodes.len())
                .filter(|&n| {
                    let up = if n >= size { h(n - size) } else { h(n) };
                    let down = if n + size < land.nodes.len() { h(n + size) } else { h(n) };
                    land.nodes[n].gradient & 0x0F != (((up - down) >> 1) + 8) as u8 & 0x0F
                })
                .count();
            maps += 1;
            if off != 0 {
                bad.push((p.file_name().unwrap().to_string_lossy().to_string(), off));
            }
        }
    }
    eprintln!("{maps} maps; differing: {bad:?}");
    assert!(maps == 0 || bad.is_empty(), "maps whose shading is not the formula: {bad:?}");
}
