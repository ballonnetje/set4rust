//! The sprite scene follows population changes (entities created and removed after load);
//! skipped when the game data is not present.

use s4_viewer::population::{self, Unit};
use s4_viewer::scene::resolve_map;
use s4_viewer::sprites::SpriteScene;

fn instances(s: &mut SpriteScene) -> Vec<String> {
    s.instances().iter().map(|i| format!("{i:?}")).collect()
}

#[test]
fn entities_appear_and_disappear() {
    if !s4_formats::game_dir().join("Gfx").join("20.gfx").exists() {
        return;
    }
    let map = s4_formats::map::MapFile::open(resolve_map("Campaign/roman02")).unwrap();
    let pop = population::from_world(&population::world_from_map(&map).unwrap());
    let mut scene = SpriteScene::from_population(&pop).unwrap();
    let before = instances(&mut scene);
    let images = scene.stats.unique_sprites;
    let _ = scene.take_dirty();

    // A building is destroyed; a Viking carrier (a race roman02 does not have, so images that are
    // not in the atlas yet) appears.
    // (Same tick, so the deco animation frames stay comparable.)
    let mut next = pop.clone();
    let gone = next.buildings.remove(0);
    let s = pop.settlers[0];
    next.settlers.push(Unit { id: 9000, flags: 0x100, x: s.x + 2, y: s.y, kind: 1, owner: s.owner, race: 1, ..Default::default() });
    let t = std::time::Instant::now();
    scene.set_population(&next).unwrap();
    let took = t.elapsed();
    assert_eq!(scene.stats.buildings, pop.buildings.len() - 1);
    assert_eq!(scene.stats.settlers, pop.settlers.len() + 1);
    assert!(scene.stats.unique_sprites > images, "the Viking carrier's image was added");
    let dirty = scene.take_dirty();
    assert!(!dirty.atlas.is_empty(), "new atlas rows to upload");
    let after = instances(&mut scene);
    assert_ne!(before, after);
    eprintln!(
        "removed building type {} at {},{}; added a Viking carrier: {} -> {} instances, {} -> {} images, set_population {:.2} ms",
        gone.kind,
        gone.x,
        gone.y,
        before.len(),
        after.len(),
        images,
        scene.stats.unique_sprites,
        took.as_secs_f64() * 1e3
    );

    // Back to the first population: the same instances as at load; nothing new to decode.
    let grown = scene.stats.unique_sprites;
    scene.set_population(&pop).unwrap();
    assert_eq!(instances(&mut scene), before);
    assert_eq!(scene.stats.unique_sprites, grown, "the atlas never shrinks");
    assert!(scene.take_dirty().atlas.is_empty());
}

/// The same through the GPU: the removed building and the new carrier show in a capture, the new
/// image and palette reach the textures, and going back gives the load-time frame exactly.
#[test]
fn gpu_follows_population() {
    use s4_viewer::gpu::{Gpu, View};
    use s4_viewer::scene::Scene;
    if !s4_formats::game_dir().join("Gfx").join("20.gfx").exists() {
        return;
    }
    let Ok(mut gpu) = Gpu::headless(480, 360) else { return };
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false).unwrap();
    scene.upload(&mut gpu);
    let pop = scene.population.as_deref().cloned().unwrap();
    let mut next = pop.clone();
    let gone = next.buildings.remove(0);
    let s = pop.settlers[0];
    next.settlers.push(Unit { id: 9000, x: gone.x + 4, y: gone.y + 1, kind: 1, owner: s.owner, race: 1, ..Default::default() });
    let c = scene.node_world(gone.x as usize, gone.y as usize);
    let view = View { x: (c[0] - 240.0).round(), y: (c[1] - 220.0).round(), zoom: 1.0 };

    let a = gpu.capture(view).unwrap();
    scene.set_population(&mut gpu, next).unwrap();
    let b = gpu.capture(view).unwrap();
    scene.set_population(&mut gpu, pop).unwrap();
    let c2 = gpu.capture(view).unwrap();
    let changed = a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p != q).count();
    eprintln!("building removed + Viking carrier added: {changed} of {} px changed", a.len() / 4);
    if let Some(dir) = std::env::var_os("S4_TEST_OUT") {
        let dir = std::path::PathBuf::from(dir);
        for (n, img) in [("a", &a), ("b", &b), ("c", &c2)] {
            s4_viewer::client::write_png(&dir.join(format!("dynamic_scene_{n}.png")), 480, 360, img).unwrap();
        }
    }
    assert!(changed > 1000, "{changed}");
    assert!(a == c2, "back to the load-time frame");
}

/// `CEffects` (release 0x4e2190 / 0x4e33b0): a crush effect (0x3e) is file 6's job 0x3e (32
/// frames), one frame per game tick from its start tick, drawn after its row's objects, gone once
/// its age reaches the frame count.
#[test]
fn crush_effect_runs_its_frames_and_ends() {
    if !s4_formats::game_dir().join("Gfx").join("6.gfx").exists() {
        return;
    }
    let map = s4_formats::map::MapFile::open(resolve_map("Campaign/roman02")).unwrap();
    let pop = population::from_world(&population::world_from_map(&map).unwrap());
    let mut scene = SpriteScene::from_population(&pop).unwrap();
    let t0 = pop.tick as u64;
    let b = &pop.buildings[0];
    let n = scene.instances().len();
    scene.add_effect(0x3e, b.x, b.y, t0 + 1).unwrap();
    assert_eq!(scene.instances().len(), n, "not started yet");
    let mut seen = Vec::new();
    for t in t0 + 1..t0 + 40 {
        scene.set_tick(t);
        let now = scene.instances().len();
        if now > n {
            assert_eq!(now, n + 1);
            seen.push(t);
        }
    }
    assert_eq!(seen, (t0 + 1..t0 + 33).collect::<Vec<_>>(), "32 ticks, from the start tick");
}

/// A ranch draws its pen as FX slot 0 from the client's patch state (s4-game `Patches::pen`:
/// the type's first .dil + animals + 1) on top of the house.
#[test]
fn ranch_pen_is_drawn_with_its_animals() {
    if !s4_formats::game_dir().join("Gfx").join("10.gfx").exists() {
        return;
    }
    let map = s4_formats::map::MapFile::open(resolve_map("Campaign/roman02")).unwrap();
    let mut pop = population::from_world(&population::world_from_map(&map).unwrap());
    let i = pop.buildings.iter().position(|b| b.site.is_none() && b.race == 0 && b.id != 0).unwrap();
    pop.buildings[i].kind = 12;
    pop.buildings[i].inhabitants.clear();
    pop.buildings[i].flag = None;
    let id = pop.buildings[i].id;
    let first = s4_formats::gfx::GfxFile::open_default(10).unwrap().job_dirs(12).unwrap().start as u16;
    for animals in [1u16, 6] {
        pop.buildings[i].pen = Some(animals as u8);
        let mut s = SpriteScene::from_population(&pop).unwrap();
        let base = s.instances().len();
        let mut slots = [(0u16, 0u32); 10];
        slots[0] = (first + animals + 1, 0);
        s.set_patches(&[(id, (0u8, slots))].into_iter().collect()).unwrap();
        assert_eq!(s.instances().len(), base + 1, "{animals} animals: the pen image");
    }
}
