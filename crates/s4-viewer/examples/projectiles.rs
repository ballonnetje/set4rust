//! Projectiles in flight (`SimEvent::Projectile` → `SpriteScene::add_projectile`) on roman02: a
//! loaded Roman catapult of player 1 12..18 nodes from an enemy military building, and player 1
//! bowmen 9..12 nodes from it; the run stops in the middle of the first catapult stone's flight
//! and captures the scene with every projectile in the air then, and again in the middle of the first
//! arrow or axe (a bowman's, a tower soldier's or an axe thrower's) that flies `margin` ticks
//! or more, and at the first tick with two arrows a tick into flights of three ticks or more.
//!
//! cargo run --release -p s4-viewer --example projectiles -- OUT_PREFIX

use s4_sim::entity::{EntityData, ObjType};
use s4_sim::event::SimEvent;
use s4_sim::vehicles::{self, kind};
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::scene::Scene;
use s4_viewer::sprites::ProjectileKind;

/// (tick, kind, from, to, delay, offset, frames)
type Shot = (u32, u8, (i32, i32), (i32, i32), i32, (i32, i32), u32);

fn free_node(w: &s4_sim::World, x: i32, y: i32) -> bool {
    w.grid.checked_index(x, y).is_some_and(|n| w.layers.ground[n] >= 16 && w.layers.flags[n] & 0x7F == 0 && w.layers.map_object[n] == 0 && w.tiling.sector_of(n) != 0)
}

fn in_air(s: &Shot, now: u32) -> bool {
    let start = s.0 as i64 + s.4 as i64;
    (now as i64) >= start && (now as i64) < start + s.6 as i64
}

fn capture(map: &str, w: &s4_sim::World, shots: &[Shot], centre: (i32, i32), path: &str) -> anyhow::Result<()> {
    let mut gpu = Gpu::headless(1024, 768)?;
    let mut scene = Scene::load(Some(map), true, false)?;
    scene.upload(&mut gpu);
    scene.set_population(&mut gpu, s4_viewer::population::from_world(w))?;
    let now = w.tick;
    let mut n = 0;
    for s in shots.iter().filter(|s| in_air(s, now)) {
        let k = ProjectileKind::of(s.1).ok_or_else(|| anyhow::anyhow!("kind {}", s.1))?;
        let node = |p: (i32, i32)| (p.0 as u16, p.1 as u16);
        scene.add_projectile(&mut gpu, &k, node(s.2), node(s.3), s.4, s.5, s.0 as u64)?;
        n += 1;
    }
    scene.set_tick(&mut gpu, now as u64);
    let size = w.grid.size() as usize;
    let view = s4_viewer::client::view_on_node(&gpu, View { x: 0.0, y: 0.0, zoom: 1.0 }, (centre.0 as usize, centre.1 as usize), size);
    let rgba = gpu.capture(view)?;
    s4_viewer::client::write_png(std::path::Path::new(path), 1024, 768, &rgba)?;
    println!("{path}: tick {now}, {n} in the air: {:?}", shots.iter().filter(|s| in_air(s, now)).map(|s| (s.1, s.2, s.3, s.0 as i64 + s.4 as i64, s.6)).collect::<Vec<_>>());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "out/viewer/projectiles/shots".into());
    let margin: u32 = std::env::args().nth(2).map_or(Ok(5), |s| s.parse())?;
    let map_path = s4_formats::game_dir().join("Map").join("Campaign").join("roman02.map");
    let map_s = map_path.to_string_lossy().into_owned();
    let map = s4_formats::map::MapFile::open(&map_path)?;
    let (mut w, _) = s4_sim::new_game(&map, s4_sim::Rules::load_default()?, &s4_sim::NewGame::default())?;
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
    let target = w
        .entities
        .iter()
        .find(|(id, e)| e.obj_type == ObjType::Building && e.owner != 1 && e.flags & 0xF != 0 && s4_sim::jobs::warrior::valid_target(&w, *id, 0x202e0ff))
        .map(|(id, _)| id)
        .ok_or_else(|| anyhow::anyhow!("no target"))?;
    let tp = w.entities.by_index(target as u32).unwrap().pos;
    let (tx, ty) = (tp.x(), tp.y());
    // The farthest place in screen pixels: a stone needs more than 2 · 90 px to arc.
    let px_dist = |x: i32, y: i32| {
        let (dx, dy) = (24 * (x - tx) - 12 * (y - ty), 12 * (y - ty));
        dx * dx + dy * dy
    };
    let (cx, cy) = s4_sim::spiral::walk(w.grid, tx, ty, 18)
        .filter(|&(px, py)| s4_sim::spiral::grid_distance(px - tx, py - ty) >= 12 && vehicles::can_place(&w, px, py, kind::WAR_MACHINE))
        .max_by_key(|&(x, y)| px_dist(x, y))
        .ok_or_else(|| anyhow::anyhow!("no catapult place"))?;
    let c = vehicles::add_vehicle(&mut w, cx, cy, 1, kind::WAR_MACHINE, -1, false).ok_or_else(|| anyhow::anyhow!("no catapult"))?;
    vehicles::get_mut(&mut w, c).unwrap().cargo = 32;
    let spots: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, tx, ty, 12).filter(|&(px, py)| s4_sim::spiral::grid_distance(px - tx, py - ty) >= 9 && free_node(&w, px, py)).step_by(4).take(10).collect();
    for &(x, y) in &spots {
        s4_sim::create::add_settler(&mut w, x, y, 1, 32);
    }
    println!("target {target} at ({tx}, {ty}), catapult at ({cx}, {cy}), bowmen at {spots:?}");
    let mut shots: Vec<Shot> = Vec::new();
    let (mut stone_done, mut arrow_done, mut arrows_done) = (false, false, false);
    for _ in 0..3000 {
        w.events.clear();
        s4_sim::tick(&mut w, &[]);
        for e in &w.events {
            if let SimEvent::Projectile { kind, from, to, delay, offset, frames } = *e {
                shots.push((w.tick, kind, from, to, delay, offset, frames));
            }
        }
        let now = w.tick;
        if !stone_done
            && let Some(s) = shots.iter().find(|s| (2..=4).contains(&s.1) && in_air(s, now) && s.6 >= margin && now as i64 >= s.0 as i64 + s.4 as i64 + s.6 as i64 / 2)
        {
            let mid = ((s.2.0 + s.3.0) / 2, (s.2.1 + s.3.1) / 2);
            capture(&map_s, &w, &shots, mid, &format!("{out}_stone.png"))?;
            stone_done = true;
        }
        if !arrow_done
            && let Some(s) = shots.iter().find(|s| matches!(s.1, 0 | 1 | 7) && in_air(s, now) && s.6 >= margin && now as i64 >= s.0 as i64 + s.4 as i64 + s.6 as i64 / 2)
        {
            let mid = ((s.2.0 + s.3.0) / 2, (s.2.1 + s.3.1) / 2);
            capture(&map_s, &w, &shots, mid, &format!("{out}_arrow.png"))?;
            arrow_done = true;
        }
        // Two or more arrows a tick into flights of 3 ticks or more.
        let flying: Vec<&Shot> = shots.iter().filter(|s| matches!(s.1, 0 | 7) && s.6 >= 3 && in_air(s, now) && now as i64 > s.0 as i64 + s.4 as i64).collect();
        if !arrows_done && flying.len() >= 2 {
            let s = flying[0];
            capture(&map_s, &w, &shots, s.3, &format!("{out}_arrows.png"))?;
            arrows_done = true;
        }
        if stone_done && arrow_done && arrows_done {
            break;
        }
    }
    let kinds: std::collections::BTreeMap<u8, usize> = shots.iter().fold(Default::default(), |mut m, s| {
        *m.entry(s.1).or_default() += 1;
        m
    });
    println!("tick {}: projectiles by kind {kinds:?}", w.tick);
    Ok(())
}
