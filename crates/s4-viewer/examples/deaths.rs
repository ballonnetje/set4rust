//! The death effect (`SimEvent::TribeEffect` → `SpriteScene::add_tribe_effect`) on roman02: two
//! hostile swordsmen (players 1 and 2) a few nodes apart near player 1's castle fight until one
//! dies; the scene is captured centred on the dead one at the effect's ages `0, n/4, n/2, 3n/4,
//! n − 1` (`n` its lifetime) and once after it ended. Prints each tribe effect's frame counts.
//!
//! cargo run --release -p s4-viewer --example deaths -- OUT_PREFIX [RACE_OF_PLAYER_2]

use s4_formats::gamedata::Race;
use s4_sim::entity::ObjType;
use s4_sim::event::SimEvent;
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::scene::Scene;
use s4_viewer::sprites::TRIBE_EFFECT_JOBS;

const SWORDSMAN_1: u16 = 29;

fn free_node(w: &s4_sim::World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
}

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "out/viewer/death/death".into());
    let race2: Option<u8> = std::env::args().nth(2).map(|s| s.parse()).transpose()?;
    let map_path = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map_s = map_path.to_string_lossy().into_owned();
    let map = s4_formats::map::MapFile::open(&map_path)?;
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default()?, &s4_sim::NewGame::default())?;
    if let (Some(r), Some(Some(p))) = (race2, w.players.get_mut(2)) {
        p.race = r;
    }
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .ok_or_else(|| anyhow::anyhow!("castle"))?;
    let ring = |c: (i32, i32), r: u8| (s4_sim::spiral::first(r)..=s4_sim::spiral::last(r)).map(move |k| (c.0 + s4_sim::spiral::dx(k), c.1 + s4_sim::spiral::dy(k)));
    let (a, b) = (12..40)
        .flat_map(|r| ring(castle, r))
        .filter(|&p| free_node(&w, p.0, p.1))
        .find_map(|p| {
            let sector = w.tiling.sector_of(w.grid.index(p.0 as u32, p.1 as u32));
            let q = ring(p, 5).find(|&q| free_node(&w, q.0, q.1) && w.tiling.sector_of(w.grid.index(q.0 as u32, q.1 as u32)) == sector)?;
            Some((p, q))
        })
        .ok_or_else(|| anyhow::anyhow!("two free nodes"))?;
    s4_sim::create::add_settler(&mut w, a.0, a.1, 1, SWORDSMAN_1).ok_or_else(|| anyhow::anyhow!("swordsman 1"))?;
    s4_sim::create::add_settler(&mut w, b.0, b.1, 2, SWORDSMAN_1).ok_or_else(|| anyhow::anyhow!("swordsman 2"))?;
    println!("swordsmen at {a:?} (player 1) and {b:?} (player 2)");

    let mut gpu = Gpu::headless(1024, 768)?;
    let mut scene = Scene::load(Some(&map_s), true, false)?;
    scene.upload(&mut gpu);
    let size = w.grid.size() as usize;
    if let Some(s) = scene.sprites.as_mut() {
        for race in [Race::ROMAN, Race::VIKING, Race::MAYA, Race::DARK, Race::TROJAN] {
            let file = s4_formats::gamedata::resolve::settler_file(race);
            let counts: Vec<[usize; 6]> = TRIBE_EFFECT_JOBS[1..].iter().map(|&j| std::array::from_fn(|d| s.job_frame_count(file, j as usize, d))).collect();
            println!("{race:?} file {file}: jobs 0x14d..0x14f frames per direction {counts:?}");
        }
    }
    let mut death = None;
    for _ in 0..3000 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        if let Some(e) = w.events.iter().find(|e| matches!(e, SimEvent::TribeEffect { .. })).cloned() {
            death = Some((w.tick, e));
            break;
        }
    }
    let (tick, e) = death.ok_or_else(|| anyhow::anyhow!("nobody died"))?;
    let SimEvent::TribeEffect { effect, x, y, player, .. } = e else { unreachable!() };
    let pop = s4_viewer::population::from_world(&w);
    let race = pop.races[player as usize];
    let life = scene.sprites.as_mut().map_or(0, |s| {
        let file = s4_formats::gamedata::resolve::settler_file(Race::from_u16(race as u16).unwrap_or(Race::ROMAN));
        s.job_frame_count(file, TRIBE_EFFECT_JOBS[effect as usize] as usize, 2)
    }) as u32;
    println!("tick {tick}: tribe effect {effect} of player {player} (race {race}) at ({x}, {y}), lifetime {life}");
    scene.set_population(&mut gpu, pop)?;
    scene.add_tribe_effect(&mut gpu, effect, x as u16, y as u16, player, race, tick as u64)?;
    let view = s4_viewer::client::view_on_node(&gpu, View { x: 0.0, y: 0.0, zoom: 1.0 }, (x as usize, y as usize), size);
    let ages = [0, life / 4, life / 2, 3 * life / 4, life.saturating_sub(1), life];
    for age in ages {
        scene.set_tick(&mut gpu, (tick + age) as u64);
        let rgba = gpu.capture(view)?;
        let path = format!("{out}_age{age:02}.png");
        s4_viewer::client::write_png(std::path::Path::new(&path), 1024, 768, &rgba)?;
        println!("{path}");
    }
    Ok(())
}
