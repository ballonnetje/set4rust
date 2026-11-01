//! Manakopters drawn (flyer_gfx.rs) on roman02: one for player 1 a few nodes from its castle,
//! sent (`flying::fly_to`) 12 nodes east and 6 south; the scene, fixed on the middle of the
//! route, is captured every `STEP` ticks until it has landed (and once more). Prints its
//! offset, state and moving effect per capture.
//!
//! cargo run --release -p s4-viewer --example manakopters -- OUT_PREFIX

use s4_sim::entity::ObjType;
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::scene::Scene;

const STEP: u32 = 8;

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "out/viewer/manako/m".into());
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
    let start = (castle.0 + 8, castle.1 + 4);
    let goal = (start.0 + 12, start.1 + 6);
    let id = s4_sim::flying::add_manakopter(&mut w, start.0, start.1, 1).ok_or_else(|| anyhow::anyhow!("no slot"))?;
    s4_sim::flying::fly_to(&mut w, id, goal.0, goal.1);
    println!("manakopter {id} at {start:?} to {goal:?}");

    let mut gpu = Gpu::headless(1024, 768)?;
    let mut scene = Scene::load(Some(&map_s), true, false)?;
    scene.upload(&mut gpu);
    let size = w.grid.size() as usize;
    let mid = ((start.0 + goal.0) as usize / 2, (start.1 + goal.1) as usize / 2 - 4);
    let view = s4_viewer::client::view_on_node(&gpu, View { x: 0.0, y: 0.0, zoom: 1.0 }, mid, size);
    let mut landed = 0;
    for k in 0..400u32 {
        if k % STEP == 0 {
            let pop = s4_viewer::population::from_world(&w);
            let f = pop.flyers.iter().find(|f| f.id == id).copied().ok_or_else(|| anyhow::anyhow!("gone"))?;
            scene.set_population(&mut gpu, pop.clone())?;
            if let (Some(s), Some(r)) = (scene.sprites.as_mut(), gpu.sprites.as_mut()) {
                s.set_ambient(&s4_viewer::flyer_gfx::sprites(&pop, w.tick))?;
                r.sync(&gpu.device, &gpu.queue, s);
                r.set_scene(s);
            }
            scene.set_tick(&mut gpu, w.tick as u64);
            let rgba = gpu.capture(view)?;
            let path = format!("{out}_t{:04}.png", w.tick);
            s4_viewer::client::write_png(std::path::Path::new(&path), 1024, 768, &rgba)?;
            let st = s4_sim::flying::get(&w, id).map_or(0, |g| g.state);
            println!("{path}: tick {} node {:?} offset {:?} state {st:#x} flight {:?}", w.tick, f.flight.from, f.offset, f.flight);
            let at = w.entities.by_index(id as u32).map(|e| (e.x(), e.y()));
            if at == Some(goal) && f.offset.1 >= -50 {
                landed += 1;
                if landed > 1 {
                    break;
                }
            }
        }
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
    }
    Ok(())
}
