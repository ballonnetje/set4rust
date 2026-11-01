//! A ship sailing (vehicle_gfx.rs): on roman02 a warship, a ferry and a transport ship for player
//! 1 on open water east of its castle, all sent (5014 `SendTo`) 12 nodes further east; prints
//! their node, direction, job and frame every `STEP` ticks and captures the scene, fixed on the
//! route.
//!
//! cargo run --release -p s4-viewer --example ships -- OUT_PREFIX

use s4_sim::entity::ObjType;
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::scene::Scene;

const STEP: u32 = 10;

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "out/viewer/ships/s".into());
    let map_path = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map_s = map_path.to_string_lossy().into_owned();
    let map = s4_formats::map::MapFile::open(&map_path)?;
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default()?, &s4_sim::NewGame::default())?;
    let castle = w
        .entities
        .iter()
        .find(|(id, e)| e.owner == 1 && e.obj_type == ObjType::Building && s4_sim::buildings::have_inhabitant(&w, *id))
        .map(|(_, e)| (e.x(), e.y()))
        .ok_or_else(|| anyhow::anyhow!("castle"))?;
    // Open water east of the castle with room for the route.
    let water = (castle.0 + 14..castle.0 + 60)
        .flat_map(|x| (castle.1 - 6..castle.1 + 12).map(move |y| (x, y)))
        .find(|&(x, y)| (0..=24).step_by(6).all(|dx| (0..=12).step_by(6).all(|dy| s4_sim::vehicles::ship::can_place(&w, x + dx, y + dy))))
        .ok_or_else(|| anyhow::anyhow!("no open water"))?;
    let mut ids = Vec::new();
    for (k, kind) in [1u8, 2, 3].into_iter().enumerate() {
        let at = (water.0, water.1 + 6 * k as i32);
        let id = s4_sim::create::add_vehicle(&mut w, at.0, at.1, 1, kind).ok_or_else(|| anyhow::anyhow!("ship {kind}"))?;
        ids.push((id, at));
    }
    for &(id, at) in &ids {
        let cmd = s4_sim::Command::SendTo { x: (at.0 + 12) as u16, y: at.1 as u16, mode: 0, entities: vec![id] };
        s4_sim::tick(&mut w, &[s4_sim::PlayerCommand { player: 1, command: cmd }]);
    }
    println!("ships {ids:?}");
    let mut gpu = Gpu::headless(1024, 768)?;
    let mut scene = Scene::load(Some(&map_s), true, false)?;
    scene.upload(&mut gpu);
    let size = w.grid.size() as usize;
    let view = s4_viewer::client::view_on_node(&gpu, View { x: 0.0, y: 0.0, zoom: 1.0 }, ((water.0 + 6) as usize, (water.1 + 6) as usize), size);
    let mut anim = s4_viewer::animate::Animator::default();
    for k in 0..=300u32 {
        if k % STEP == 0 {
            let pop = s4_viewer::population::from_world(&w);
            let line: Vec<String> = ids
                .iter()
                .filter_map(|(id, _)| pop.vehicles.iter().find(|v| v.id == *id))
                .map(|v| format!("{}:({},{}) d{} j{} f{} dist{}", v.id, v.x, v.y, v.dir, v.job, v.frame, v.distance))
                .collect();
            println!("tick {}: {}", w.tick, line.join("  "));
            scene.set_population(&mut gpu, pop.clone())?;
            if let (Some(s), Some(r)) = (scene.sprites.as_mut(), gpu.sprites.as_mut()) {
                let poses = anim.vehicle_poses(&pop, w.tick as f64, s.vehicle_frames());
                r.sync(&gpu.device, &gpu.queue, s);
                s.set_unit_poses(poses);
                r.set_scene(s);
            }
            scene.set_tick(&mut gpu, w.tick as u64);
            let rgba = gpu.capture(view)?;
            s4_viewer::client::write_png(std::path::Path::new(&format!("{out}_t{:04}.png", w.tick)), 1024, 768, &rgba)?;
        }
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
    }
    Ok(())
}
