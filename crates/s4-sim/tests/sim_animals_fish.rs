//! Jumping fish (`CAnimalMgr` 0x5301c0, client side): around a node with fish on MCD2_roman1,
//! [`s4_sim::animals::fish::jump`] finds fish nodes — more fish than FISH_MIN, not dark — in the
//! cells around the centre's, and the fish job exists in `Gfx\8.gfx`. Skipped without the game.

use s4_sim::animals::fish::{FISH_JOB, jump};

#[test]
fn fish_jump_near_fish() {
    let p = s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_roman1.map");
    if !p.exists() {
        return;
    }
    let map = s4_formats::map::MapFile::open(&p).unwrap();
    let rules = s4_sim::Rules::load_default().unwrap();
    let (w, _) = s4_sim::new_game(&map, rules, &s4_sim::NewGame::default()).unwrap();
    let min = w.rules.animals.fish_min;
    let n = (0..w.layers.resource.len()).find(|&n| w.layers.resource[n] & 0xf0 == 0 && (w.layers.resource[n] & 0xf) as i32 > min).expect("fish on the map");
    let size = w.grid.size() as usize;
    let (x, y) = ((n % size) as i32, (n / size) as i32);
    let mut rng = s4_sim::rng::Random16::new(s4_sim::rng::ANIMAL_SEED, 0);
    let mut hits = 0;
    for _ in 0..200 {
        if let Some(j) = jump(&w, &mut rng, x, y) {
            let m = w.grid.index(j.x as u32, j.y as u32);
            assert!((w.layers.resource[m] & 0xf) as i32 > min && w.layers.resource[m] & 0xf0 == 0);
            assert_eq!(w.layers.flags[m] & 4, 0);
            assert!(((j.x >> 4) - (x >> 4)).abs() <= 2 && ((j.y >> 4) - (y >> 4)).abs() <= 2);
            assert!(j.dir < 6);
            hits += 1;
        }
    }
    eprintln!("{hits} jumps in 200 tries around ({x},{y})");
    assert!(hits > 0);
    let g = s4_formats::gfx::GfxFile::open_default(8).unwrap();
    let frames = g.job_frames(FISH_JOB as usize, 2).map_or(0, |r| r.len());
    eprintln!("fish job {FISH_JOB}: {frames} frames");
    assert!(frames > 1);
}
