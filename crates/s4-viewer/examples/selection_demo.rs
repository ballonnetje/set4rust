//! Unit selection on roman02 (selection.rs): a box over player 1's free soldiers, then a click
//! on one, each captured with the GUI (soldier context dialog 45) and the selection icons.
//! `cargo run -p s4-viewer --example selection_demo -- OUT_PREFIX`
//!
//! The sim does not set `EntityFlag_Selectable` yet (the soldier role's `Init` is not ported), so
//! this demo sets it on player 1's soldiers outside buildings the way `CSoldierRole::Init` does
//! (`SetFlagBits(VulnerableMask | Selectable)`). Nothing else is staged.

use s4_viewer::client;
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::scene::Scene;
use s4_viewer::selection::Selection;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let out = a.first().cloned().unwrap_or_else(|| "out/viewer/selection".into());
    let (w, h) = (1280u32, 1024u32);
    let mut gpu = Gpu::headless(w, h)?;
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false)?;
    scene.upload(&mut gpu);
    client::attach_gui(&mut gpu, &scene, 1)?;
    let mut pop = scene.population.as_deref().cloned().unwrap();
    let mut soldiers = Vec::new();
    for u in pop.settlers.iter_mut().filter(|u| u.owner == 1 && u.flags & 0xF != 0) {
        u.flags |= 0x2000 | 0x30000;
        soldiers.push((u.x, u.y));
    }
    println!("{} free soldiers of player 1", soldiers.len());
    anyhow::ensure!(!soldiers.is_empty(), "no soldiers outside buildings");
    scene.set_population(&mut gpu, pop.clone())?;
    // View centred on the soldiers.
    let (sx, sy) = soldiers.iter().fold((0.0, 0.0), |acc, &(x, y)| {
        let c = scene.node_world(x as usize, y as usize);
        (acc.0 + c[0], acc.1 + c[1])
    });
    let n = soldiers.len() as f32;
    let view = View { x: (sx / n - w as f32 / 2.0 - 100.0).round(), y: (sy / n - h as f32 / 2.0).round(), zoom: 1.0 };
    let to_screen = |scene: &Scene, (x, y): (u16, u16)| {
        let c = scene.node_world(x as usize, y as usize);
        ((c[0] - view.x) as f64, (c[1] - view.y) as f64)
    };
    let mut sel = Selection::default();

    // 1. Box over every soldier (with some margin).
    let pts: Vec<(f64, f64)> = soldiers.iter().map(|&s| to_screen(&scene, s)).collect();
    let x0 = pts.iter().map(|p| p.0).fold(f64::MAX, f64::min) - 30.0;
    let y0 = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min) - 40.0;
    let x1 = pts.iter().map(|p| p.0).fold(f64::MIN, f64::max) + 30.0;
    let y1 = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max) + 20.0;
    let ev = client::select_box(&mut gpu, &scene, view, &pop, &mut sel, 1, (x0, y0), (x1, y1), false);
    println!("box ({x0:.0},{y0:.0})-({x1:.0},{y1:.0}): {ev:?}");
    capture(&mut gpu, &mut scene, view, &pop, &mut sel, &format!("{out}_box.png"))?;

    // 2. Click on the first soldier.
    let p = pts[0];
    let ev = client::select_at(&mut gpu, &mut scene, view, &pop, &mut sel, 1, p, false);
    println!("click ({:.0},{:.0}): {ev:?}", p.0, p.1);
    capture(&mut gpu, &mut scene, view, &pop, &mut sel, &format!("{out}_click.png"))?;

    // 3. Click on empty ground: cleared, back to the build menu.
    let ev = client::select_at(&mut gpu, &mut scene, view, &pop, &mut sel, 1, (700.0, 80.0), false);
    println!("click on nothing: {ev:?}");
    capture(&mut gpu, &mut scene, view, &pop, &mut sel, &format!("{out}_cleared.png"))?;
    Ok(())
}

fn capture(
    gpu: &mut Gpu,
    scene: &mut Scene,
    view: View,
    pop: &s4_viewer::population::Population,
    sel: &mut Selection,
    path: &str,
) -> anyhow::Result<()> {
    let (icons, _) = client::selection_frame(gpu, pop, sel, 1);
    scene.set_markers(gpu, &[], None, &icons)?;
    client::update_minimap(gpu, scene, view, 64);
    let (w, h) = gpu.size();
    let img = gpu.capture(view)?;
    client::write_png(std::path::Path::new(path), w, h, &img)?;
    println!("  {} selected, {} icons → {path}", sel.ids.len(), icons.len());
    Ok(())
}
