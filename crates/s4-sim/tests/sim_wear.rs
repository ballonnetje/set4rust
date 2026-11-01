//! Path wear (0x60ca10 / 0x60cad0) and the world step (0x4f2d00) on roman02; skipped without the
//! game data.

use s4_sim::World;

fn roman02() -> Option<World> {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map = s4_formats::map::MapFile::open(&p).ok()?;
    let rules = s4_sim::Rules::load_default().ok()?;
    s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).ok().map(|(w, _)| w)
}

/// A grass node whose six neighbours are grass.
fn open_grass(w: &World) -> Option<usize> {
    let size = w.grid.size() as i32;
    (0..w.layers.ground.len()).find(|&n| {
        let (x, y) = (n as i32 % size, n as i32 / size);
        w.layers.ground[n] == 0x10
            && w.layers.walk_count(n) == 0
            && [(-1, -1), (0, -1), (0, 1), (1, 1), (-1, 0), (1, 0)]
                .iter()
                .all(|&(dx, dy)| w.grid.checked_index(x + dx, y + dy).is_some_and(|m| w.layers.ground[m] == 0x10))
    })
}

#[test]
fn walking_wears_a_path_and_a_road_and_they_grow_back() {
    let Some(mut w) = roman02() else { return };
    let n = open_grass(&w).expect("an open grass node");
    for k in 1..=9 {
        s4_sim::wear::increase_walk_count(&mut w, n);
        assert_eq!((w.layers.walk_count(n), w.layers.ground[n]), (k, 0x10));
    }
    s4_sim::wear::increase_walk_count(&mut w, n);
    assert_eq!((w.layers.walk_count(n), w.layers.ground[n], w.layers.walk_move[n] & 7), (10, 0x1c, 1), "10 steps: path, move cost 1");
    for _ in 11..=15 {
        s4_sim::wear::increase_walk_count(&mut w, n);
    }
    assert_eq!((w.layers.walk_count(n), w.layers.ground[n], w.layers.walk_move[n] & 7), (15, 0x1d, 0), "15 steps: road, move cost 0");
    // Decay: 10 → path again, 6 → grass with move cost 2.
    for _ in 0..5 {
        s4_sim::wear::decrease_walk_count(&mut w, n);
    }
    assert_eq!((w.layers.walk_count(n), w.layers.ground[n], w.layers.walk_move[n] & 7), (10, 0x1c, 1));
    for _ in 0..4 {
        s4_sim::wear::decrease_walk_count(&mut w, n);
    }
    assert_eq!((w.layers.walk_count(n), w.layers.ground[n], w.layers.walk_move[n] & 7), (6, 0x10, 2));
    // The 5-bit count wraps from 31 to 0 (the release's byte arithmetic).
    for _ in 6..31 {
        s4_sim::wear::increase_walk_count(&mut w, n);
    }
    assert_eq!(w.layers.walk_count(n), 31);
    s4_sim::wear::increase_walk_count(&mut w, n);
    assert_eq!(w.layers.walk_count(n), 0);
}

#[test]
fn world_step_cursor_rng_and_fish() {
    let Some(mut w) = roman02() else { return };
    let size = w.grid.size() as i32;
    let steps = size * size / 2000;
    // Fish move by one from a fish node (also a shore node) into water; the raw bytes are
    // conserved even where 15 + 1 turns into 0x10 (no clamp in the release, 0x4f2e5a).
    let water_sum = |w: &World| -> u32 {
        (0..w.layers.ground.len())
            .filter(|&n| w.layers.ground[n] < 9 || w.layers.resource[n] & 0xf0 == 0)
            .map(|n| w.layers.resource[n] as u32)
            .sum()
    };
    let fish_before = water_sum(&w);
    let calls = w.rng.calls;
    s4_sim::wear::world_step(&mut w);
    // The cursor: `steps` advances of 13 nodes.
    let total = 13 * steps;
    assert_eq!((w.world_step.x, w.world_step.y), (total % size, total / size % size));
    // One draw per node, one more on a node with fish.
    let drawn = w.rng.calls - calls;
    assert!(drawn >= steps as u32 && drawn <= 2 * steps as u32, "{drawn} draws for {steps} steps");
    for _ in 0..2000 {
        s4_sim::wear::world_step(&mut w);
    }
    assert_eq!(fish_before, water_sum(&w));
}

/// Oracle: the fish of roman02 after the release ran it (`rom2.sav` / `AutoSave.sav`, vanilla
/// data, ~79 000 ticks) against ours at the same tick. The world step spreads the map's 8-fish
/// seas into a bell over 1..=15 and, without a clamp, turns 15 + 1 into 0x10 (type 1, amount 0);
/// both saves hold ~3 600 such water nodes. (The Settlers United saves hold none: the mod changed
/// it.) Per value the counts must agree within 20 % (or 60 nodes).
#[test]
fn fish_spread_matches_the_vanilla_saves() {
    fn find(d: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        for e in std::fs::read_dir(d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(f) = find(&p, name) {
                    return Some(f);
                }
            } else if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
                return Some(p);
            }
        }
        None
    }
    let Some(home) = std::env::var_os("USERPROFILE") else { return };
    let Some(save) = find(&std::path::PathBuf::from(home).join(r"Documents\TheSettlers4\Save"), "rom2.sav") else { return };
    let Some(mut w) = roman02() else { return };
    let m = s4_formats::map::MapFile::open(&save).unwrap();
    let chunk = |id: u16| m.container.chunks.iter().find(|h| h.kind == id).and_then(|h| m.container.load(h).ok()).unwrap();
    let (render, resource) = (chunk(200), chunk(207));
    let tick = s4_sim::save::SaveGame::open(&save).unwrap().tick;
    let mut theirs = [0i32; 17];
    for i in 0..resource.len() {
        if render[i * 4 + 1] < 9 && resource[i] <= 0x10 {
            theirs[resource[i] as usize] += 1;
        }
    }
    for _ in 0..tick {
        s4_sim::tick(&mut w, &[]);
    }
    let mut ours = [0i32; 17];
    for i in 0..w.layers.resource.len() {
        if w.layers.ground[i] < 9 && w.layers.resource[i] <= 0x10 {
            ours[w.layers.resource[i] as usize] += 1;
        }
    }
    eprintln!("tick {tick}\n theirs {theirs:?}\n ours   {ours:?}");
    for v in 1..=16 {
        let (a, b) = (theirs[v], ours[v]);
        assert!((a - b).abs() <= (a / 5).max(60), "fish value {v:#x}: release {a}, ours {b}");
    }
}
