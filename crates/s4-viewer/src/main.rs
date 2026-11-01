//! s4-viewer: winit window + wgpu renderer. What is drawn lives in `scene.rs`.
//!
//! ```text
//! s4-viewer [--map path] [--at x,y] [--zoom z] [--linear] [--mipmaps] [--unlit] [--no-sprites] [--raw-map]
//! s4-viewer --capture out.png [--width 1920] [--height 1080] [same options]
//! s4-viewer --origin wx,wy ...        top-left of the view in world px instead of --at
//! s4-viewer --diff a.png b.png out.png   per-pixel comparison (stats + amplified diff image)
//! s4-viewer --gradient-stats map ...    stored map gradient vs RefreshShading recomputation
//! --tick n (capture mode): game tick for the water animation and the fog (default 0)
//! s4-viewer --refstats ref.png [--ref-scale 1.25] [--mask-left px] [--refstats-save prefix] --map m --origin x,y --width w --height h
//!     matched-surface comparison with a reference screenshot (see refstats.rs)
//! Optional modern effects, all off by default (effects.rs): --ao, --sharpen, --smooth-shadows,
//!     --hidpi factor (window: the monitor's scale factor), --clouds; keys F1..F5 toggle them.
//! --gui [scale]: draw the in-game GUI panel in captures (the window shows it by default, G toggles;
//!     it is scaled by the window's scale factor)
//! --click x,y (capture, repeatable): left click (down + up) at screen point x,y on the GUI; the UI
//!     events are printed
//! --hover x,y (capture): pick the node under screen point x,y, print it and mark it
//! Mouse: hover shows the node under the cursor (title bar + marker); left click on the minimap
//!     centres the view there.
//! --fog: fog-of-war test pattern (territory around --at or the map centre, three scouts)
//! --flatten x,y (repeatable, capture mode): level ground around a node via the dirty-band path
//! ```
//! Keys: arrows/WASD scroll, wheel zooms, right-drag pans, F toggles point/linear filtering, M toggles
//! mipmaps (used below zoom 1),
//! L toggles lighting, X flattens ground around the node at the screen centre (dirty rebuild).


use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use s4_viewer::client::{GuiMouse, attach_gui, eff_zoom, gui_mouse, hover_node, minimap_node, read_png, update_minimap, write_png};
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::terrain::{ROW_H, TRI_W, TerrainSource};
use s4_viewer::terrain_gpu::Filter;
use s4_viewer::{effects, refstats, scene, terrain, waves};

#[derive(Default)]
struct Args {
    map: Option<String>,
    capture: Option<String>,
    width: u32,
    height: u32,
    at: Option<(usize, usize)>,
    origin: Option<(f32, f32)>,
    zoom: f32,
    linear: bool,
    mipmaps: bool,
    unlit: bool,
    no_sprites: bool,
    tick: u64,
    fog: bool,
    raw_map: bool,
    hover: Option<(f32, f32)>,
    clicks: Vec<(f64, f64)>,
    gui: Option<f32>,
    effects: effects::Effects,
    hidpi_factor: f32,
    refstats: Option<String>,
    ref_scale: f32,
    mask_left: u32,
    refstats_save: Option<String>,
    diff: Option<[String; 3]>,
    gradient_stats: Vec<String>,
    flatten: Vec<(usize, usize)>,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut a = Args { width: 1920, height: 1080, zoom: 1.0, ref_scale: 1.0, ..Default::default() };
    let mut it = std::env::args().skip(1);
    let pair = |s: String| -> anyhow::Result<(String, String)> {
        let (x, y) = s.split_once(',').ok_or_else(|| anyhow::anyhow!("expected x,y: {s}"))?;
        Ok((x.to_string(), y.to_string()))
    };
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| anyhow::anyhow!("{arg} needs a value"));
        match arg.as_str() {
            "--map" => a.map = Some(val()?),
            "--capture" => a.capture = Some(val()?),
            "--width" => a.width = val()?.parse()?,
            "--height" => a.height = val()?.parse()?,
            "--zoom" => a.zoom = val()?.parse()?,
            "--at" => {
                let (x, y) = pair(val()?)?;
                a.at = Some((x.parse()?, y.parse()?));
            }
            "--origin" => {
                let (x, y) = pair(val()?)?;
                a.origin = Some((x.parse()?, y.parse()?));
            }
            "--linear" => a.linear = true,
            "--mipmaps" => a.mipmaps = true,
            "--unlit" => a.unlit = true,
            "--no-sprites" => a.no_sprites = true,
            "--tick" => a.tick = val()?.parse()?,
            "--fog" => a.fog = true,
            "--raw-map" => a.raw_map = true,
            "--click" => {
                let (x, y) = pair(val()?)?;
                a.clicks.push((x.parse()?, y.parse()?));
            }
            "--hover" => {
                let (x, y) = pair(val()?)?;
                a.hover = Some((x.parse()?, y.parse()?));
            }
            "--gui" => a.gui = Some(val()?.parse()?),
            "--ao" => a.effects.ao = true,
            "--sharpen" => a.effects.sharpen = true,
            "--smooth-shadows" => a.effects.smooth_shadows = true,
            "--clouds" => a.effects.clouds = true,
            "--hidpi" => {
                a.effects.hidpi = true;
                a.hidpi_factor = val()?.parse()?;
            }
            "--refstats" => a.refstats = Some(val()?),
            "--ref-scale" => a.ref_scale = val()?.parse()?,
            "--mask-left" => a.mask_left = val()?.parse()?,
            "--refstats-save" => a.refstats_save = Some(val()?),
            "--diff" => a.diff = Some([val()?, val()?, val()?]),
            "--gradient-stats" => a.gradient_stats.push(val()?),
            "--flatten" => {
                let (x, y) = pair(val()?)?;
                a.flatten.push((x.parse()?, y.parse()?));
            }
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    Ok(a)
}

/// Initial camera: `--origin` (top-left, world px) or centred on `--at` node / the map centre.
/// Positions are rounded to whole pixels so zoom-1 captures are pixel-exact.
fn initial_view(args: &Args, scene: &scene::Scene, (w, h): (u32, u32)) -> View {
    let zoom = args.zoom;
    if let Some((x, y)) = args.origin {
        return View { x, y, zoom };
    }
    let c = match args.at {
        Some((x, y)) => scene.node_world(x, y),
        None => scene.centre(),
    };
    // With HiDPI scaling the renderer multiplies the zoom by the factor; centre for that.
    let eff = if args.effects.hidpi { zoom * args.hidpi_factor } else { zoom };
    View { x: (c[0] - w as f32 / 2.0 / eff).round(), y: (c[1] - h as f32 / 2.0 / eff).round(), zoom }
}

/// Centre of the fog test pattern: `--at` or the map centre.
fn fog_centre(args: &Args, scene: &scene::Scene) -> (usize, usize) {
    args.at.unwrap_or((scene.mesh.width / 2, scene.mesh.height / 2))
}

struct App {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    scene: scene::Scene,
    args: Args,
    view: Option<View>,
    keys: [bool; 4], // left, right, up, down
    drag: Option<(f64, f64)>,
    cursor: (f64, f64),
    /// Node under the cursor (`SearchMapPoint`).
    hover: Option<(usize, usize)>,
    last: Instant,
    /// Stand-in for the simulation's tick counter (one tick per 71 ms, game speed 1).
    start: Instant,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes().with_title("s4rs").with_inner_size(LogicalSize::new(1280, 800));
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        let mut gpu = Gpu::new(window.clone()).expect("init GPU");
        gpu.show_hover = true;
        gpu.effects = self.args.effects;
        gpu.scale_factor = window.scale_factor() as f32;
        gpu.gui_scale = gpu.scale_factor;
        if let Err(e) = attach_gui(&mut gpu, &self.scene, 1) {
            tracing::warn!("GUI panel: {e}");
        }
        self.scene.upload(&mut gpu);
        if self.args.fog {
            self.scene.enable_fog(&mut gpu, fog_centre(&self.args, &self.scene));
        }
        if let Some(t) = &mut gpu.terrain {
            t.filter = if self.args.linear { Filter::Linear } else { Filter::Point };
            t.mipmaps = self.args.mipmaps;
            t.lighting = !self.args.unlit;
        }
        self.view = Some(initial_view(&self.args, &self.scene, gpu.size()));
        self.gpu = Some(gpu);
        self.window = Some(window);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(view) = &mut self.view else { return };
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.scale_factor = scale_factor as f32;
                    gpu.gui_scale = scale_factor as f32;
                    if let Err(e) = attach_gui(gpu, &self.scene, 1) {
                        tracing::warn!("GUI panel: {e}");
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    match code {
                        KeyCode::ArrowLeft | KeyCode::KeyA => self.keys[0] = down,
                        KeyCode::ArrowRight | KeyCode::KeyD => self.keys[1] = down,
                        KeyCode::ArrowUp | KeyCode::KeyW => self.keys[2] = down,
                        KeyCode::ArrowDown | KeyCode::KeyS => self.keys[3] = down,
                        KeyCode::Escape => el.exit(),
                        _ if !down || event.repeat => {}
                        KeyCode::KeyF => {
                            if let Some(t) = self.gpu.as_mut().and_then(|g| g.terrain.as_mut()) {
                                t.filter = if t.filter == Filter::Point { Filter::Linear } else { Filter::Point };
                            }
                        }
                        KeyCode::F1 | KeyCode::F2 | KeyCode::F3 | KeyCode::F4 | KeyCode::F5 => {
                            if let Some(g) = &mut self.gpu {
                                let e = &mut g.effects;
                                match code {
                                    KeyCode::F1 => e.ao = !e.ao,
                                    KeyCode::F2 => e.sharpen = !e.sharpen,
                                    KeyCode::F3 => e.smooth_shadows = !e.smooth_shadows,
                                    KeyCode::F4 => e.hidpi = !e.hidpi,
                                    _ => e.clouds = !e.clouds,
                                }
                                tracing::info!("effects: {:?}", g.effects);
                                // AO turned on: the rows changed while it was off get theirs.
                                g.sync_terrain(&mut self.scene.mesh);
                            }
                        }
                        KeyCode::KeyG => {
                            if let Some(g) = &mut self.gpu {
                                g.show_gui = !g.show_gui;
                            }
                        }
                        KeyCode::KeyM => {
                            if let Some(t) = self.gpu.as_mut().and_then(|g| g.terrain.as_mut()) {
                                t.mipmaps = !t.mipmaps;
                            }
                        }
                        KeyCode::KeyL => {
                            if let Some(t) = self.gpu.as_mut().and_then(|g| g.terrain.as_mut()) {
                                t.lighting = !t.lighting;
                            }
                        }
                        KeyCode::KeyX => {
                            if let Some(gpu) = &mut self.gpu {
                                let (w, h) = gpu.size();
                                let (cx, cy) = (view.x + w as f32 / 2.0 / view.zoom, view.y + h as f32 / 2.0 / view.zoom);
                                let ny = (cy / ROW_H as f32).round().max(0.0) as usize;
                                let nx = ((cx + ny as f32 * TRI_W as f32 / 2.0) / TRI_W as f32).round().max(0.0) as usize;
                                let (mw, mh) = (self.scene.mesh.width, self.scene.mesh.height);
                                self.scene.flatten(gpu, nx.min(mw - 1), ny.min(mh - 1));
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let down = state == ElementState::Pressed;
                let what = if down { GuiMouse::LeftDown } else { GuiMouse::LeftUp };
                let over = self.gpu.as_mut().is_some_and(|g| gui_mouse(g, self.cursor, what).1);
                if down
                    && let Some(gpu) = &self.gpu
                    && let Some(n) = minimap_node(gpu, &self.scene, self.cursor).filter(|_| !over)
                {
                    // [inf] Centre the view on the clicked node (the original's handler is in the
                    // input processor, not traced).
                    let (w, h) = gpu.size();
                    let z = eff_zoom(gpu, *view);
                    let c = self.scene.node_world(n.0, n.1);
                    view.x = c[0] - w as f32 / 2.0 / z;
                    view.y = c[1] - h as f32 / 2.0 / z;
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } => {
                let over = state == ElementState::Pressed && self.gpu.as_mut().is_some_and(|g| gui_mouse(g, self.cursor, GuiMouse::RightDown).1);
                self.drag = (state == ElementState::Pressed && !over).then_some(self.cursor);
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some((dx, dy)) = self.drag {
                    view.x -= (position.x - dx) as f32 / view.zoom;
                    view.y -= (position.y - dy) as f32 / view.zoom;
                    self.drag = Some((position.x, position.y));
                }
                self.cursor = (position.x, position.y);
                if let Some(g) = &mut self.gpu {
                    gui_mouse(g, self.cursor, GuiMouse::Move);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                // Zoom about the cursor so the point under it stays put.
                let (cx, cy) = (self.cursor.0 as f32, self.cursor.1 as f32);
                let before = (view.x + cx / view.zoom, view.y + cy / view.zoom);
                view.zoom = (view.zoom * 1.15f32.powf(steps)).clamp(0.25, 6.0);
                view.x = before.0 - cx / view.zoom;
                view.y = before.1 - cy / view.zoom;
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

impl App {
    fn redraw(&mut self) {
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        let (Some(gpu), Some(view)) = (&mut self.gpu, &mut self.view) else { return };
        let speed = 900.0 * dt / view.zoom;
        view.x += speed * (self.keys[1] as i32 - self.keys[0] as i32) as f32;
        view.y += speed * (self.keys[3] as i32 - self.keys[2] as i32) as f32;
        let tick = self.start.elapsed().as_millis() as u64 / waves::TICK_MS;
        self.scene.set_tick(gpu, tick);
        gpu.cloud_time = self.start.elapsed().as_secs_f64();
        if gpu.show_gui {
            update_minimap(gpu, &mut self.scene, *view, 1);
        }
        let hover = hover_node(gpu, &self.scene, *view, (self.cursor.0 as f32, self.cursor.1 as f32));
        if hover != self.hover {
            self.hover = hover;
            if let (Some(w), Some((x, y))) = (&self.window, hover) {
                let n = self.scene.source.node(x, y);
                w.set_title(&format!("s4rs: node {x},{y} height {} ground {}", n.height, n.ground));
            }
        }
        gpu.draw(*view);
    }
}

/// Compare two same-sized images: print statistics, write a diff image (|a−b| × 8, and pure
/// magenta where exactly one side is background).
fn diff(a: &str, b: &str, out: &str) -> anyhow::Result<()> {
    let (wa, ha, pa) = read_png(a)?;
    let (wb, hb, pb) = read_png(b)?;
    anyhow::ensure!((wa, ha) == (wb, hb), "sizes differ: {wa}x{ha} vs {wb}x{hb}");
    let mut img = vec![0u8; pa.len()];
    let (mut same, mut small, mut big, mut sum) = (0u64, 0u64, 0u64, 0u64);
    let mut hist = [0u64; 9];
    for (i, (p, q)) in pa.as_chunks::<4>().0.iter().zip(pb.as_chunks::<4>().0).enumerate() {
        let d = (0..3).map(|c| (p[c] as i32 - q[c] as i32).unsigned_abs()).max().unwrap();
        sum += d as u64;
        match d {
            0 => same += 1,
            1..=8 => small += 1,
            _ => big += 1,
        }
        hist[(d as usize).min(8)] += 1;
        for c in 0..3 {
            img[i * 4 + c] = ((p[c] as i32 - q[c] as i32).unsigned_abs() * 8).min(255) as u8;
        }
        img[i * 4 + 3] = 255;
    }
    let n = (wa * ha) as f64;
    println!(
        "{a} vs {b}: identical {:.2}%, max-channel diff 1..8 {:.2}%, >8 {:.2}%, mean {:.3}",
        same as f64 / n * 100.0,
        small as f64 / n * 100.0,
        big as f64 / n * 100.0,
        sum as f64 / n
    );
    println!("histogram of max-channel diff (0..=7, >=8): {hist:?}");
    write_png(Path::new(out), wa, ha, &img)
}

/// How often the gradient stored in a map (chunk 13 byte 2) equals what `RefreshShading` would
/// compute from the heights.
fn gradient_stats(m: &str) -> anyhow::Result<()> {
    let g = terrain::Grid::from_map(&scene::resolve_map(m))?;
    let mut r = terrain::Grid { w: g.w, h: g.h, nodes: g.nodes.clone() };
    let (mut same, mut off1, mut other) = (0usize, 0usize, 0usize);
    let mut bits = [0usize; 4];
    for y in 0..g.h {
        for x in 0..g.w {
            r.refresh_shading(x, y);
            let (a, b) = (g.nodes[y * g.w + x].gradient, r.nodes[y * g.w + x].gradient);
            for (i, bit) in [0x10u8, 0x20, 0x40, 0x80].iter().enumerate() {
                bits[i] += (a & bit != 0) as usize;
            }
            match ((a & 15) as i32 - (b & 15) as i32).abs() {
                0 => same += 1,
                1 => off1 += 1,
                _ => other += 1,
            }
        }
    }
    let n = (g.w * g.h) as f64;
    println!(
        "{m}: {}x{}  stored==recomputed {:.2}%  off by 1 {:.2}%  other {:.2}%  high bits set (0x10,0x20,0x40,0x80): {bits:?}",
        g.w,
        g.h,
        same as f64 / n * 100.0,
        off1 as f64 / n * 100.0,
        other as f64 / n * 100.0
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = parse_args()?;
    if let Some([a, b, out]) = &args.diff {
        return diff(a, b, out);
    }
    if !args.gradient_stats.is_empty() {
        for m in &args.gradient_stats {
            gradient_stats(m)?;
        }
        return Ok(());
    }
    let scene = scene::Scene::load(args.map.as_deref(), !args.no_sprites, args.raw_map)?;

    if args.capture.is_some() || args.refstats.is_some() {
        let mut gpu = Gpu::headless(args.width, args.height)?;
        gpu.show_hover = true;
        gpu.effects = args.effects;
        gpu.scale_factor = args.hidpi_factor;
        if let Some(s) = args.gui {
            gpu.gui_scale = s;
            attach_gui(&mut gpu, &scene, 1)?;
        }
        let mut scene = scene;
        scene.upload(&mut gpu);
        if args.fog {
            scene.enable_fog(&mut gpu, fog_centre(&args, &scene));
        }
        if let Some(t) = &mut gpu.terrain {
            t.filter = if args.linear { Filter::Linear } else { Filter::Point };
            t.mipmaps = args.mipmaps;
            t.lighting = !args.unlit;
        }
        // Exercise the dirty-band path after the initial upload.
        for &(x, y) in &args.flatten {
            scene.flatten(&mut gpu, x, y);
        }
        scene.set_tick(&mut gpu, args.tick);
        gpu.cloud_time = s4_viewer::clouds::seconds(args.tick as f64);
        let view = initial_view(&args, &scene, gpu.size());
        if let Some(reference) = &args.refstats {
            return refstats::run(
                &mut gpu,
                &refstats::RefStatsArgs {
                    reference,
                    ref_scale: args.ref_scale,
                    mask_left: args.mask_left,
                    view,
                    size: (args.width, args.height),
                    save_prefix: args.refstats_save.as_deref(),
                },
            );
        }
        for &p in &args.clicks {
            gui_mouse(&mut gpu, p, GuiMouse::Move);
            let (down, over) = gui_mouse(&mut gpu, p, GuiMouse::LeftDown);
            let (up, _) = gui_mouse(&mut gpu, p, GuiMouse::LeftUp);
            println!("click {},{} (on GUI: {over}): {:?}", p.0, p.1, down.iter().chain(&up).collect::<Vec<_>>());
        }
        if args.gui.is_some() {
            update_minimap(&mut gpu, &mut scene, view, 64);
        }
        if let Some(p) = args.hover {
            match hover_node(&mut gpu, &scene, view, p) {
                Some((x, y)) => {
                    let n = scene.source.node(x, y);
                    println!("hover {},{} -> node {x},{y} (height {}, ground {})", p.0, p.1, n.height, n.ground);
                }
                None => println!("hover {},{} -> outside the map", p.0, p.1),
            }
        }
        let out = args.capture.as_deref().unwrap_or_default();
        let rgba = gpu.capture(view)?;
        write_png(Path::new(out), args.width, args.height, &rgba)?;
        println!("captured {out} ({}x{}, view origin {},{} zoom {})", args.width, args.height, view.x, view.y, view.zoom);
        return Ok(());
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        window: None,
        gpu: None,
        scene,
        args,
        view: None,
        keys: [false; 4],
        drag: None,
        cursor: (0.0, 0.0),
        hover: None,
        last: Instant::now(),
        start: Instant::now(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
