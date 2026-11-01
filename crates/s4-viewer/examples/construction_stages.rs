//! Construction stages of a building as `CBuildingSiteRole::FillGfxInfo` + `BlitBuilding` show
//! them: one roman02 building turned into a site at 0 … 100 % work, captured side by side.
//! `cargo run -p s4-viewer --example construction_stages -- OUT.png [building index]`

use s4_viewer::gpu::{Gpu, View};
use s4_viewer::population::SiteGfx;
use s4_viewer::scene::Scene;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let out = a.first().cloned().unwrap_or_else(|| "out/viewer/construction_stages.png".into());
    let which: usize = a.get(1).map_or(Ok(0), |s| s.parse())?;
    let (cw, ch) = (220u32, 220u32);
    let steps = [0, 10, 25, 40, 50, 60, 75, 90, 100];
    let mut gpu = Gpu::headless(cw, ch)?;
    let mut scene = Scene::load(Some("Campaign/roman02"), true, false)?;
    scene.upload(&mut gpu);
    let pop = scene.population.as_deref().cloned().unwrap();
    let b = pop.buildings.iter().filter(|b| b.inhabitants.is_empty()).nth(which).unwrap().clone();
    let c = scene.node_world(b.x as usize, b.y as usize);
    let view = View { x: (c[0] - cw as f32 / 2.0).round(), y: (c[1] - ch as f32 * 0.7).round(), zoom: 1.0 };
    let work = 800;
    let mut sheet = vec![0u8; (cw * ch * 4) as usize * steps.len()];
    let sw = cw * steps.len() as u32;
    for (n, &pct) in steps.iter().enumerate() {
        let mut p = pop.clone();
        for q in p.buildings.iter_mut().filter(|q| (q.x, q.y) == (b.x, b.y)) {
            q.site = (pct < 100).then_some(SiteGfx { state: 5, work_done: work * pct / 100, work, step: 131_070 / work, builders: 0 });
        }
        scene.set_population(&mut gpu, p)?;
        let img = gpu.capture(view)?;
        for y in 0..ch {
            let src = &img[(y * cw * 4) as usize..((y + 1) * cw * 4) as usize];
            let d = ((y * sw + n as u32 * cw) * 4) as usize;
            sheet[d..d + (cw * 4) as usize].copy_from_slice(src);
        }
        println!("{pct:3} %: captured");
    }
    s4_viewer::client::write_png(std::path::Path::new(&out), sw, ch, &sheet)?;
    println!("building type {} at {},{} → {out}", b.kind, b.x, b.y);
    Ok(())
}
