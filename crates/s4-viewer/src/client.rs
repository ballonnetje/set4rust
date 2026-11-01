//! Glue between the window/headless front ends and the renderer: GUI attachment and mouse
//! routing, picking under the cursor, the minimap layer, PNG I/O. Used by the s4-viewer binary and
//! by s4-game.

use std::collections::HashMap;
use std::path::Path;

use crate::gpu::{Gpu, View};
use crate::terrain::TerrainSource;
use crate::{gui, guistate, minimap, picking, scene};

/// Compose the GUI panel for the local player `local` (its race's GUI set) and hand it to the
/// GPU. The screen height in panel pixels decides which extension bars are open (gui.rs).
pub fn attach_gui(gpu: &mut Gpu, scene: &scene::Scene, local: u8) -> anyhow::Result<()> {
    let pop = scene.population.as_deref();
    let ui = new_gui(pop, local, gui_height(gpu))?;
    install_gui(gpu, ui, pop, local)
}

/// The screen's height in panel pixels (which extension bars open).
pub fn gui_height(gpu: &Gpu) -> u32 {
    (gpu.size().1 as f32 / gpu.gui_scale.max(0.1)) as u32
}

/// The GUI of the local player's race, loaded (the GUI set, the text engine, the gfx: no GPU,
/// so a game's start loads it beside the world).
pub fn new_gui(pop: Option<&crate::population::Population>, local: u8, screen_h: u32) -> anyhow::Result<gui::Gui> {
    let l = (local as usize).min(8);
    let race = pop.and_then(|p| s4_formats::gamedata::Race::from_u16(p.races[l] as u16)).unwrap_or(s4_formats::gamedata::Race::ROMAN);
    gui::Gui::new(race, pop, local, screen_h)
}

/// [`attach_gui`] with the GUI loaded ([`new_gui`]): its panel on the GPU, the camera window.
pub fn install_gui(gpu: &mut Gpu, mut ui: gui::Gui, pop: Option<&crate::population::Population>, local: u8) -> anyhow::Result<()> {
    let l = (local as usize).min(8);
    let screen_h = gui_height(gpu);
    let panel = ui.render()?;
    gpu.gui = Some(gui::GuiOverlay::new(&gpu.device, &gpu.queue, gpu.format(), panel));
    gpu.ui = Some(ui);
    // Camera window (slot 2, screens >= 1024 tall): `GuiDlgMainBarExt2Proc(0)` points it at the local
    // player's start; with no entity followed the position is
    // `((X − Y/2)·scale − 87 − scale/24·40, Y·scale/2 − scale/24·10 − 95)` at scale 24 (zoom 1),
    // in a 174×190 window at (16, 818) (`EnableCameraWindow(1, 16, 818, 174, 190)`).
    gpu.camera_window = (screen_h >= 1024).then(|| {
        let (x, y) = pop.map_or((0, 0), |p| p.starts[l]);
        let (x, y) = (x as f32, y as f32);
        let view = View { x: (x - y / 2.0) * 24.0 - 87.0 - 40.0, y: y * 12.0 - 10.0 - 95.0, zoom: 1.0 };
        (view, (16, 818, 174, 190))
    });
    Ok(())
}

/// A mouse event for the GUI at screen point `p`. Returns the GUI's events and whether the point
/// is on the GUI (then the map does not get it). Recomposes the panel when its state changed.
pub fn gui_mouse(gpu: &mut Gpu, p: (f64, f64), what: GuiMouse) -> (Vec<guistate::UiEvent>, bool) {
    // The hit test reads the composed panel: bring it up to date first.
    gpu.flush_gui();
    let Some(ui) = gpu.ui.as_mut() else { return (Vec::new(), false) };
    let s = gpu.gui_scale.max(0.1) as f64;
    let p = (p.0 - gpu.gui_origin.0 as f64, p.1 - gpu.gui_origin.1 as f64);
    let (x, y) = ((p.0 / s).floor() as i32, (p.1 / s).floor() as i32);
    let over = ui.over_gui(x, y);
    let before = (ui.state.open.clone(), ui.state.effects.clone(), ui.state.sliders.clone(), ui.state.texts.clone(), ui.state.fonts.clone());
    let ev = match what {
        GuiMouse::Move => ui.state.mouse_move(x, y),
        GuiMouse::LeftDown => ui.state.left_down(x, y, over),
        GuiMouse::LeftUp => ui.state.left_up(x, y),
        GuiMouse::RightDown => ui.state.right_down(x, y, over),
        GuiMouse::RightUp => Vec::new(),
    };
    let costs = ui.hover_costs() | ui.graph_events(&ev);
    if costs || (&ui.state.open, &ui.state.effects, &ui.state.sliders, &ui.state.texts, &ui.state.fonts) != (&before.0, &before.1, &before.2, &before.3, &before.4) {
        recompose(gpu);
    }
    for e in &ev {
        tracing::info!("ui: {e:?}");
    }
    (ev, over)
}

/// The mouse wheel at screen point `p` (`delta` as WM_MOUSEWHEEL's): code 10 for the dialog
/// under it, if any (guistate `wheel`).
pub fn gui_wheel(gpu: &mut Gpu, p: (f64, f64), delta: i32) -> Vec<guistate::UiEvent> {
    let Some(ui) = gpu.ui.as_mut() else { return Vec::new() };
    let s = gpu.gui_scale.max(0.1) as f64;
    let p = (p.0 - gpu.gui_origin.0 as f64, p.1 - gpu.gui_origin.1 as f64);
    let ev: Vec<guistate::UiEvent> = ui.state.wheel((p.0 / s).floor() as i32, (p.1 / s).floor() as i32, delta).into_iter().collect();
    if ui.graph_events(&ev) {
        recompose(gpu);
    }
    ev
}

/// The GUI panel changed: it is composed and uploaded once, by the next frame or GUI mouse
/// event ([`Gpu::flush_gui`]), however many changes come before (a click recomposed it four or
/// five times, ~9 ms each).
pub fn recompose(gpu: &mut Gpu) {
    gpu.gui_dirty = true;
}

/// Refresh the GUI pages from `pop` (the shown snapshot), with the eco-sector at the centre of
/// the screen, and recompose the panel when they changed.
pub fn update_gui_pages(gpu: &mut Gpu, scene: &scene::Scene, view: View, pop: &crate::population::Population) {
    if gpu.ui.is_none() {
        return;
    }
    let centre = camera_node(gpu, view, scene.source.w);
    let local = gpu.ui.as_ref().map_or(0, |u| u.local);
    let at = crate::gui_pages::Centre {
        eco: centre.and_then(|(x, y)| pop.economy.eco_of.get(y * pop.size + x).copied()).unwrap_or(0),
        own: centre.map_or(0, |c| pop.economy.own_at_centre(pop.size, c, local)),
        current: centre.map_or(0, |c| pop.current_eco_at_centre(c, local)),
    };
    let changed = gpu.ui.as_mut().is_some_and(|ui| ui.update_pages(pop, at));
    if changed {
        recompose(gpu);
    }
}

/// The local player's first eco-sector on the spiral around the camera's node
/// (0x585870 / 0x4ebfe0, `Economy::own_at_centre`); 0 if none.
pub fn own_eco_at_centre(gpu: &mut Gpu, scene: &scene::Scene, view: View, pop: &crate::population::Population) -> u16 {
    let local = gpu.ui.as_ref().map_or(0, |u| u.local);
    camera_node(gpu, view, scene.source.w).map_or(0, |c| pop.economy.own_at_centre(pop.size, c, local))
}

/// The screen point `CStateGame`'s camera (+0x44 / +0x48) stands for, in a `w` × `h` output
/// (0x519a70: the view's top left is the camera minus this):
/// `((int)((w − 800) / 1760 · 40 + w + 100) / 2, (int)((h − 600) / 840 · 60 + h + 300) / 2)`.
pub fn camera_anchor((w, h): (u32, u32)) -> (i32, i32) {
    let a = (((w as f32 - 800.0) / 1760.0) * 40.0 + (w + 100) as f32) as i32 / 2;
    let b = (((h as f32 - 600.0) / 840.0) * 60.0 + (h + 300) as f32) as i32 / 2;
    (a, b)
}

/// `CStateGame` +0x4c for `view`: pixels per node column in 16.16 (24 at zoom 1, `TRI_W`).
pub fn camera_zoom(gpu: &Gpu, view: View) -> i32 {
    (eff_zoom(gpu, view) * 24.0 * 65536.0).round() as i32
}

/// `CStateGame`'s camera (+0x44, +0x48) for `view`: the flat map pixel (at the view's zoom) at
/// [`camera_anchor`].
pub fn camera_of(gpu: &Gpu, view: View) -> (i32, i32) {
    let z = eff_zoom(gpu, view);
    let (a, b) = camera_anchor(gpu.size());
    ((view.x * z).floor() as i32 + a, (view.y * z).floor() as i32 + b)
}

/// The camera's node (0x585870, 0x4dfde0: `y = +0x48 / (zoom >> 17)`, `x = +0x44 / (zoom >> 16)
/// + y / 2`, C division), if it is on a `size` map.
pub fn camera_node(gpu: &Gpu, view: View, size: usize) -> Option<(usize, usize)> {
    let (cx, cy) = camera_of(gpu, view);
    let zoom = camera_zoom(gpu, view);
    let (zx, zy) = (zoom >> 16, zoom >> 17);
    if zx == 0 || zy == 0 {
        return None;
    }
    let y = cy / zy;
    let x = cx / zx + y / 2;
    (x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size).then_some((x as usize, y as usize))
}

/// 0x519a70: the camera kept on a `size` map (`+0x40` = 0, `+0x3c` = the map size): y in
/// `0..=size · z / 2`, then x in `−y..=size · z − y`, `z` = zoom >> 16. Returns the view moved
/// accordingly (unchanged when nothing was clamped).
pub fn clamp_view(gpu: &Gpu, view: View, size: usize) -> View {
    let z = camera_zoom(gpu, view) >> 16;
    let (x, y) = camera_of(gpu, view);
    let size = size as i32;
    let ny = if y < 0 { 0 } else { y.min(size * z / 2) };
    let nx = if x < -ny { -ny } else { x.min(size * z - ny) };
    if (nx, ny) == (x, y) {
        return view;
    }
    let ez = eff_zoom(gpu, view);
    let (a, b) = camera_anchor(gpu.size());
    View { x: if nx != x { (nx - a) as f32 / ez } else { view.x }, y: if ny != y { (ny - b) as f32 / ez } else { view.y }, zoom: view.zoom }
}

/// 0x514ce0: the minimap point `(u, v)` (0..=160 each) under screen point `p`: `v = y − 8`,
/// `u = x − 15 − (160 − v) / 2` in panel px (C division).
pub fn minimap_point(gpu: &Gpu, p: (f64, f64)) -> Option<(i32, i32)> {
    let s = gpu.gui_scale.max(0.1) as f64;
    let (x, y) = ((p.0 / s).floor() as i32, (p.1 / s).floor() as i32);
    let v = y - 8;
    if v > 160 {
        return None;
    }
    let u = x - 15 - (160 - v) / 2;
    ((0..=160).contains(&u) && v >= 0).then_some((u, v))
}

/// EventManagement case 7 on the minimap: the camera to minimap point `(u, v)`: `+0x48 = v · z ·
/// size / 320`, `+0x44 = u · z · size / 160 − +0x48` (unsigned division, `z` = zoom >> 16), then
/// 0x519a70.
pub fn view_from_minimap(gpu: &Gpu, view: View, (u, v): (i32, i32), size: usize) -> View {
    let z = camera_zoom(gpu, view) >> 16;
    let cy = ((v * z * size as i32) as u32 / 0x140) as i32;
    let cx = ((u * z * size as i32) as u32 / 0xa0) as i32 - cy;
    let ez = eff_zoom(gpu, view);
    let (a, b) = camera_anchor(gpu.size());
    clamp_view(gpu, View { x: (cx - a) as f32 / ez, y: (cy - b) as f32 / ez, zoom: view.zoom }, size)
}

/// EventManagement case 9 on the minimap: the node of GUI event 0x1e0 for minimap point
/// `(u, v)`: `(u · size · z / 160) / z` and the same for `v` (unsigned, then signed division);
/// (0, 0) when either is below 1.
pub fn minimap_order_node(gpu: &Gpu, view: View, (u, v): (i32, i32), size: usize) -> (usize, usize) {
    let z = (camera_zoom(gpu, view) >> 16).max(1);
    let x = ((u * size as i32 * z) as u32 / 0xa0) as i32 / z;
    let y = ((v * size as i32 * z) as u32 / 0xa0) as i32 / z;
    if x < 1 || y < 1 { (0, 0) } else { (x as usize, y as usize) }
}

/// `CStateGame` +0x4c limits (0x5198e0 / 0x519870) and the step of one unit.
pub const ZOOM_MIN: i32 = 0xe0000;
pub const ZOOM_MAX: i32 = 0x480000;
pub const ZOOM_STEP: i32 = 20000;

/// 0x519870 (`units` > 0: in) / 0x5198e0 (`units` < 0: out): +0x4c ± 20000 per unit, kept in
/// 0xe0000..=0x480000; the camera scaled by new / old zoom (doubles, truncated), then clamped
/// and the zoom slider moved (0x5199a0 → 0x519a70, GUI event 0x25d).
pub fn zoom_view(gpu: &mut Gpu, view: View, units: i32, size: usize) -> View {
    let old = camera_zoom(gpu, view);
    let new = if units > 0 { (old + units * ZOOM_STEP).min(ZOOM_MAX) } else { (old + units * ZOOM_STEP).max(ZOOM_MIN) };
    if old == 0 {
        return view;
    }
    let view = if new == old { view } else { rezoom(gpu, view, old, new, size) };
    let v = slider_of_zoom(camera_zoom(gpu, view));
    zoom_slider(gpu, v);
    gpu.zoom_events.push((0x25d, v as i32));
    view
}

/// 0x5199a0's slider value: `(+0x4c − 0xe0000) · 100 / 3801088` (doubles, truncated).
pub fn slider_of_zoom(z: i32) -> u32 {
    ((z - ZOOM_MIN) as u32 as f64 * 100.0 / 3801088.0) as u32
}

/// `EventManagement` case 0x12's zoom for slider value `v`.
pub fn zoom_of_slider(v: i32) -> i32 {
    (v as u32 as f64 * 3801088.0 / 100.0) as i32 + ZOOM_MIN
}

/// `CGuiEventHandler` event 0x25d (0x480a5d): `SetSliderPosition(dialog 0, 0x265, v)` (the zoom
/// slider's knob 613 above the minimap) while dialog 0 is open.
pub fn zoom_slider(gpu: &mut Gpu, v: u32) {
    if let Some(ui) = gpu.ui.as_mut().filter(|ui| ui.state.open.contains(&0)) {
        ui.state.set_slider(0, 0x265, v);
    }
}

/// The game start's event 0x25d (`CStateGame` constructor 0x513190, after `LoadGUI` and the
/// camera; also after a loaded game): the slider at `(float)(+0x4c − 0xe0000) · 100f / 3801088f`,
/// truncated.
pub fn zoom_slider_start(gpu: &mut Gpu, view: View) {
    let z = camera_zoom(gpu, view);
    let v = ((z - ZOOM_MIN) as u32 as f64 as f32 * 100.0f32 / 3801088.0f32) as u32;
    zoom_slider(gpu, v);
    gpu.zoom_events.push((0x25d, v as i32));
}

/// `EventManagement` case 0x12 (event 0x12, sent by dialog 0's procedure 0x49b6b0 for knob 0x265
/// on codes 2 and 3 with the slider value `v`): +0x4c = `(int)((double)(unsigned)v · 3801088 /
/// 100) + 0xe0000` (no bound), the camera scaled by new / old, then clamped (0x519a70: no event
/// 0x25d back).
pub fn zoom_from_slider(gpu: &Gpu, view: View, v: i32, size: usize) -> View {
    let old = camera_zoom(gpu, view);
    let new = zoom_of_slider(v);
    if old == 0 || new == old {
        return view;
    }
    rezoom(gpu, view, old, new, size)
}

/// Dialog 0's procedure 0x49b6b0 for the zoom knob 0x265: code 2 (the value while it moves) and
/// code 3 (the release; dialog 0 sits at x 0, so its value is the knob's) send event 0x12 →
/// [`zoom_from_slider`].
pub fn zoom_slider_events(gpu: &mut Gpu, mut view: View, events: &[guistate::UiEvent], size: usize) -> View {
    for e in events {
        let v = match *e {
            guistate::UiEvent::Slider { dialog: 0, link: 0x265, value } => value,
            guistate::UiEvent::Command { dialog: 0, link: 0x265, .. } => match gpu.ui.as_ref().and_then(|ui| ui.state.knob_value(0, 0x265)) {
                Some(v) => v as i32,
                None => continue,
            },
            _ => continue,
        };
        view = zoom_from_slider(gpu, view, v, size);
        gpu.zoom_events.push((0x12, v));
    }
    view
}

/// Dialog 0's procedure 0x49b6b0 for the minimap toggles (code 3): 614 (0x266) buildings,
/// 615 (0x267) settlers, 616 (0x268) warriors, 617 (0x269) sectors set (pressed) or clear
/// `MinimapViewFlags` bit 0x1 / 0x4 / 0x2 / 0x8, then event 0x6a applies the flags to the object
/// groups (`EventManagement` 0x5177d6 → 0x59a9a0, [`minimap::ObjectLayer`]). [inf] Not
/// modelled: the proc's guard 0x15aabb5 (no change while set) and its click sound 0xb.
pub fn minimap_toggles(scene: &mut scene::Scene, events: &[guistate::UiEvent]) {
    for e in events {
        let guistate::UiEvent::Command { dialog: 0, link, effects } = *e else { continue };
        let bit = match link {
            0x266 => 0x1,
            0x267 => 0x4,
            0x268 => 0x2,
            0x269 => 0x8,
            _ => continue,
        };
        let f = &mut scene.minimap_layer.flags;
        *f = if effects & 1 != 0 { *f | bit } else { *f & !bit };
    }
}

/// +0x4c from `old` to `new`: the camera (+0x44 / +0x48) times new / old (doubles, truncated),
/// then clamped (0x519a70).
fn rezoom(gpu: &Gpu, view: View, old: i32, new: i32, size: usize) -> View {
    let (cx, cy) = camera_of(gpu, view);
    let f = new as f64 / old as f64;
    let (cx, cy) = ((cx as f64 * f) as i32, (cy as f64 * f) as i32);
    // The view's own zoom (without the HiDPI factor) for the new pixels per node.
    let hidpi = eff_zoom(gpu, view) / view.zoom;
    let zoom = new as f32 / (24.0 * 65536.0) / hidpi;
    let ez = zoom * hidpi;
    let (a, b) = camera_anchor(gpu.size());
    clamp_view(gpu, View { x: (cx - a) as f32 / ez, y: (cy - b) as f32 / ez, zoom }, size)
}

/// 0x519c50 (event 0x13): the camera on node `(x, y)`: `+0x48 = z · y / 2`, `+0x44 = z · x −
/// +0x48` (`z` = zoom >> 16, the flat position), then clamped.
pub fn view_on_node(gpu: &Gpu, view: View, (x, y): (usize, usize), size: usize) -> View {
    let z = camera_zoom(gpu, view) >> 16;
    let cy = z * y as i32 / 2;
    let cx = z * x as i32 - cy;
    let ez = eff_zoom(gpu, view);
    let (a, b) = camera_anchor(gpu.size());
    clamp_view(gpu, View { x: (cx - a) as f32 / ez, y: (cy - b) as f32 / ez, zoom: view.zoom }, size)
}

/// Screen point at the centre of control `link` of open dialog `dialog` (for scripted clicks).
pub fn control_point(gpu: &Gpu, dialog: usize, link: u16) -> Option<(f64, f64)> {
    let ui = gpu.ui.as_ref()?;
    let d = ui.state.set.dialog(dialog)?;
    let c = d.controls.iter().find(|c| c.link == link)?;
    let s = gpu.gui_scale.max(0.1) as f64;
    let (ox, oy) = (gpu.gui_origin.0 as f64, gpu.gui_origin.1 as f64);
    Some((((d.x + c.x) as f64 + c.width as f64 / 2.0) * s + ox, ((d.y + c.y) as f64 + c.height as f64 / 2.0) * s + oy))
}

/// Release control `link` of dialog `dialog` (clear its pressed bit) and recompose the panel.
pub fn release_control(gpu: &mut Gpu, dialog: usize, link: u16) {
    let Some(ui) = gpu.ui.as_mut() else { return };
    let Some(i) = ui.state.set.dialog(dialog).and_then(|d| d.controls.iter().position(|c| c.link == link)) else { return };
    let Some(e) = ui.state.effects.get_mut(&(dialog, i)) else { return };
    if *e & 1 == 0 {
        return;
    }
    *e &= !1;
    match ui.render() {
        Ok(panel) => {
            if let Some(g) = &mut gpu.gui {
                g.set_panel(&gpu.device, &gpu.queue, panel);
            }
        }
        Err(e) => tracing::warn!("GUI: {e}"),
    }
}

#[derive(Clone, Copy)]
pub enum GuiMouse {
    Move,
    LeftDown,
    LeftUp,
    RightDown,
    /// The release of the right button (event 10): only screens that end on it read it.
    RightUp,
}

/// Zoom actually used for rendering (HiDPI scaling multiplies it).
pub fn eff_zoom(gpu: &Gpu, view: View) -> f32 {
    if gpu.effects.hidpi { view.zoom * gpu.scale_factor } else { view.zoom }
}

/// The node under screen point `p` (outside the GUI panel) and its hover marker.
pub fn hover_node(gpu: &mut Gpu, scene: &scene::Scene, view: View, p: (f32, f32)) -> Option<(usize, usize)> {
    let s = gpu.gui_scale.max(0.1);
    let on_gui = gpu.ui.as_ref().is_some_and(|u| u.over_gui((p.0 / s).floor() as i32, (p.1 / s).floor() as i32));
    if on_gui {
        if let Some(g) = &mut gpu.gui {
            g.set_marker(&gpu.device, &gpu.queue, None, 1.0);
        }
        return None;
    }
    let z = eff_zoom(gpu, view);
    let (wx, wy) = (view.x + p.0 / z, view.y + p.1 / z);
    let src = &scene.source;
    let (w, h) = (src.w as i32, src.h as i32);
    let height = |x: i32, y: i32| src.node(x.clamp(0, w - 1) as usize, y.clamp(0, h - 1) as usize).height;
    let n = picking::search_map_point(src.w, &height, wx.floor() as i32, wy.floor() as i32);
    let marker = n.map(|(x, y)| {
        let a = scene.node_world(x as usize, y as usize);
        ((a[0] - view.x) * z, (a[1] - view.y) * z)
    });
    let scale = gpu.gui_scale;
    let marker = marker.filter(|_| gpu.show_hover);
    if let Some(g) = &mut gpu.gui {
        g.set_marker(&gpu.device, &gpu.queue, marker, scale / z);
    }
    n.map(|(x, y)| (x as usize, y as usize))
}

/// The map node under a click on the minimap (minimap.rs projection inverted), if any.
pub fn minimap_node(gpu: &Gpu, scene: &scene::Scene, cursor: (f64, f64)) -> Option<(usize, usize)> {
    gpu.gui.as_ref()?;
    let s = gpu.gui_scale;
    let (px, py) = (cursor.0 as f32 / s - minimap::POS.0 as f32, cursor.1 as f32 / s - minimap::POS.1 as f32);
    let (i, j) = (py.floor() as i32, (px.floor() as i32) - 80 + (py.floor() as i32) / 2);
    if !(0..160).contains(&i) || !(0..160).contains(&j) {
        return None;
    }
    let k = scene.source.w as f32 / 160.0;
    Some((((j as f32 * k + 0.5) as usize).min(scene.source.w - 1), ((i as f32 * k + 0.5) as usize).min(scene.source.h - 1)))
}

/// The view rectangle's inputs for [`minimap::mark_area`]: the view's top-left in nodes
/// (`SetCameraPosition` 0x658ad0, called by 0x519a70 with the camera minus [`camera_anchor`]:
/// `viewportX = (x << 15) / (zoom / 2)`, `viewportY` the same with the row zoom `zoom / 2`, C
/// division) and its size on the minimap (0x665ba0: the output size in whole nodes, times
/// 160 / size, `cvtss2si`: rounded to nearest).
pub fn minimap_view(gpu: &Gpu, view: View, size: usize) -> ((i32, i32), (i32, i32)) {
    let z = camera_zoom(gpu, view) as i64;
    let zb = z / 2;
    let ez = eff_zoom(gpu, view);
    let (x, y) = ((view.x * ez).floor() as i64, (view.y * ez).floor() as i64);
    let vx = ((x << 15) / (z / 2).max(1)) as i32;
    let vy = ((y << 15) / (zb / 2).max(1)) as i32;
    let (w, h) = gpu.size();
    let shear = 160.0 / size as f32;
    let cols = (z >> 16).max(1) as u32;
    let rows = (zb >> 16).max(1) as u32;
    let rect = (((w / cols) as f32 * shear).round_ties_even() as i32, ((h / rows) as f32 * shear).round_ties_even() as i32);
    ((vx, vy), rect)
}

/// Refresh the minimap layer of the GUI for the current main view after `frames` rendered
/// frames ([`minimap::Minimap`]: one eighth of the rows and columns per frame).
pub fn update_minimap(gpu: &mut Gpu, scene: &mut scene::Scene, view: View, frames: u32) {
    let (v, rect) = minimap_view(gpu, view, scene.source.w);
    let local = gpu.ui.as_ref().map_or(1, |u| u.local);
    let Some(img) = scene.minimap(v, rect, frames, local) else { return };
    if let Some(g) = &mut gpu.gui {
        g.set_minimap(&gpu.device, &gpu.queue, (minimap::WIDTH, minimap::HEIGHT), minimap::POS, &img);
    }
}

pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut e = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()?.write_image_data(rgba)?;
    Ok(())
}

pub fn read_png(path: &str) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let mut r = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?)).read_info()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf)?;
    anyhow::ensure!(info.bit_depth == png::BitDepth::Eight, "{path}: need 8-bit");
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        c => anyhow::bail!("{path}: unsupported colour type {c:?}"),
    };
    Ok((info.width, info.height, rgba))
}


// ------------------------------------------------------------------------------------------------
// Selection (selection.rs)

/// Show a selection change in the GUI the way `how` says ([`crate::selection::Shown`]) and report
/// it as [`guistate::UiEvent::Selection`].
fn selection_changed(gpu: &mut Gpu, pop: &crate::population::Population, sel: &mut crate::selection::Selection, how: crate::selection::Shown) -> Vec<guistate::UiEvent> {
    sel.stamp(pop);
    let mut ev = gpu.ui.as_mut().map(|ui| ui.show_selection(pop, &sel.ids, how)).unwrap_or_default();
    recompose(gpu);
    ev.push(guistate::UiEvent::Selection(sel.ids.clone()));
    ev
}

/// The boxes `GetTheBestEntity` (0x4ed490) tests of the `local` player's settlers and vehicles,
/// by entity id: (width, height, left, top).
pub fn local_boxes(scene: &mut scene::Scene, pop: &crate::population::Population, local: u8) -> HashMap<u16, crate::selection::BBox> {
    let mut boxes = HashMap::new();
    if let Some(s) = &mut scene.sprites {
        // `GetTheBestEntity` (0x4ed490) skips entities whose gfx job `+0x26` is 0, and anchors
        // the box at the entity's node (`+0x18`), not at its drawn (walking) position.
        for u in pop.settlers.iter().filter(|u| u.owner == local) {
            let job = (u.id != 0 && (u.job != 0 || u.task != 0)).then_some(u.job);
            if job == Some(0) {
                continue;
            }
            if let Some(b) = s.settler_bbox(u.kind, u.race, job) {
                boxes.insert(u.id, b);
            }
        }
        // Ships and land vehicles: 0x4626d0 with their job +0x26, direction 0, frame 0.
        for u in pop.vehicles.iter().filter(|u| u.owner == local && u.id != 0) {
            if let Some(b) = s.vehicle_bbox(u.race, u.job) {
                boxes.insert(u.id, b);
            }
        }
    }
    boxes
}

/// A click on the map at screen point `p` (`DotSelection`); `append` = Ctrl held.
#[allow(clippy::too_many_arguments)]
pub fn select_at(
    gpu: &mut Gpu,
    scene: &mut scene::Scene,
    view: View,
    pop: &crate::population::Population,
    sel: &mut crate::selection::Selection,
    local: u8,
    p: (f64, f64),
    append: bool,
) -> Vec<guistate::UiEvent> {
    let Some((x, y)) = hover_node(gpu, scene, view, (p.0 as f32, p.1 as f32)) else { return Vec::new() };
    let boxes = local_boxes(scene, pop, local);
    let screen = |x: u16, y: u16| {
        let a = scene.node_world(x as usize, y as usize);
        (a[0], a[1])
    };
    let bbox = |c: &crate::selection::Candidate| if matches!(c.obj, 1 | 2 | 4) { boxes.get(&c.id).copied() } else { None };
    match sel.click(pop, local, pop.size, (x as u16, y as u16), append, &screen, &bbox) {
        crate::selection::Change::None => Vec::new(),
        // `DotSelection` 0x4eb3b0: an entity → `RequestDialog` 0x4ee730; Ctrl on the last
        // selected one → `RemoveFromSelection` 0x4eda10 → 0x4edb10 + `sub_EDC90`.
        crate::selection::Change::Selected if !sel.is_empty() => selection_changed(gpu, pop, sel, crate::selection::Shown::Request),
        _ => selection_changed(gpu, pop, sel, crate::selection::Shown::Reset),
    }
}

/// Shift or Alt + click at screen point `p` (event 0x1aa, 0x4eb5f0): the selection replaced by
/// the local player's units of the clicked one's type and rank around it
/// ([`crate::selection::Selection::same_type_around`]; `wide` = Alt), then `RequestDialog`.
#[allow(clippy::too_many_arguments)]
pub fn select_same_type(
    gpu: &mut Gpu,
    scene: &mut scene::Scene,
    view: View,
    pop: &crate::population::Population,
    sel: &mut crate::selection::Selection,
    local: u8,
    p: (f64, f64),
    wide: bool,
) -> Vec<guistate::UiEvent> {
    let Some((x, y)) = hover_node(gpu, scene, view, (p.0 as f32, p.1 as f32)) else { return Vec::new() };
    let boxes = local_boxes(scene, pop, local);
    let screen = |x: u16, y: u16| {
        let a = scene.node_world(x as usize, y as usize);
        (a[0], a[1])
    };
    let bbox = |c: &crate::selection::Candidate| if matches!(c.obj, 1 | 2 | 4) { boxes.get(&c.id).copied() } else { None };
    sel.same_type_around(pop, local, pop.size, (x as u16, y as u16), wide, &screen, &bbox);
    let shown = if sel.is_empty() { crate::selection::Shown::Reset } else { crate::selection::Shown::Request };
    selection_changed(gpu, pop, sel, shown)
}

/// A box dragged on the map from screen point `a` to `b` (`PrepareBoxSelection` …
/// `BoxSelection`); `strict` = add only the current selection's type.
#[allow(clippy::too_many_arguments)]
pub fn select_box(
    gpu: &mut Gpu,
    scene: &scene::Scene,
    view: View,
    pop: &crate::population::Population,
    sel: &mut crate::selection::Selection,
    local: u8,
    a: (f64, f64),
    b: (f64, f64),
    strict: bool,
) -> Vec<guistate::UiEvent> {
    let z = eff_zoom(gpu, view);
    let w = |p: (f64, f64)| (view.x + p.0 as f32 / z, view.y + p.1 as f32 / z);
    let (wa, wb) = (w(a), w(b));
    let screen = |x: u16, y: u16| {
        let n = scene.node_world(x as usize, y as usize);
        (n[0], n[1])
    };
    // CSettler::GetGfx returns before IAnimatedEntity::BoxSelection for invisible settlers.
    let visible = |c: &crate::selection::Candidate| c.obj == 8 || c.flags & 0x100 != 0;
    // 0x4ed2c0: `sub_EDC90` when the box selected nothing, then `RequestDialog`.
    match sel.boxed(pop, local, (wa.0, wa.1, wb.0, wb.1), strict, &screen, &visible) {
        crate::selection::Change::None => Vec::new(),
        crate::selection::Change::Selected => selection_changed(gpu, pop, sel, crate::selection::Shown::Request),
        crate::selection::Change::Cleared => selection_changed(gpu, pop, sel, crate::selection::Shown::Reset),
    }
}

/// The settler search's mode 1 (0x4ec230): `sub_EE690`, then the box over the whole output
/// (0, 0)..(output width, height) keeping only the drawn own entities of type `ty`
/// ([`crate::selection::Selection::boxed_type`]), ended as a dragged box is (0x4ed2c0).
pub fn select_type_on_screen(
    gpu: &mut Gpu,
    scene: &scene::Scene,
    view: View,
    pop: &crate::population::Population,
    sel: &mut crate::selection::Selection,
    local: u8,
    ty: u32,
) -> Vec<guistate::UiEvent> {
    let mut ev = deselect(gpu, pop, sel);
    let z = eff_zoom(gpu, view);
    let (w, h) = gpu.size();
    let rect = (view.x, view.y, view.x + w as f32 / z, view.y + h as f32 / z);
    let screen = |x: u16, y: u16| {
        let n = scene.node_world(x as usize, y as usize);
        (n[0], n[1])
    };
    let visible = |c: &crate::selection::Candidate| c.obj == 8 || c.flags & 0x100 != 0;
    ev.extend(match sel.boxed_type(pop, local, rect, ty, &screen, &visible) {
        crate::selection::Change::None => Vec::new(),
        crate::selection::Change::Selected => selection_changed(gpu, pop, sel, crate::selection::Shown::Request),
        crate::selection::Change::Cleared => selection_changed(gpu, pop, sel, crate::selection::Shown::Reset),
    });
    ev
}

/// Keep the selection in step with a new snapshot (entities gone or no longer selectable are
/// dropped, the dialog refilled) and return the markers to draw over the selected settlers.
pub fn selection_frame(
    gpu: &mut Gpu,
    pop: &crate::population::Population,
    sel: &mut crate::selection::Selection,
    local: u8,
) -> (Vec<crate::selection::Icon>, Vec<guistate::UiEvent>) {
    let ev = if sel.refresh(pop, local) && sel.is_empty() {
        // Entities that left the selection go through `RemoveFromSelection` 0x4eda10: emptied,
        // 0x4edb10 (GUI events 0x260, 0x259) + `sub_EDC90` (0x25b): the dialogs close.
        selection_changed(gpu, pop, sel, crate::selection::Shown::Reset)
    } else {
        // Otherwise 0x4eda10 does nothing more: the open dialog stays (D40 of two ferries, one
        // sunk, stays D40 and is not swapped for the one ferry's D42) and refreshes.
        if !sel.is_empty()
            && let Some(ui) = gpu.ui.as_mut()
            && ui.refill_selection(pop, &sel.ids)
        {
            recompose(gpu);
        }
        Vec::new()
    };
    (sel.markers(pop), ev)
}

/// `sub_EDC90` (`CInputProcessor::Clear`): drop the selection and go back to the main menu
/// (GUI event 0x25b).
pub fn clear_selection(gpu: &mut Gpu, pop: &crate::population::Population, sel: &mut crate::selection::Selection) -> Vec<guistate::UiEvent> {
    *sel = crate::selection::Selection::default();
    selection_changed(gpu, pop, sel, crate::selection::Shown::Reset)
}

/// `sub_EE690` alone: drop the selection and leave the dialogs to the caller (the main and
/// submenu tabs' 0x192 handler 0x4edac0, the search page 0x4ec230).
pub fn deselect(gpu: &mut Gpu, pop: &crate::population::Population, sel: &mut crate::selection::Selection) -> Vec<guistate::UiEvent> {
    if sel.is_empty() {
        return Vec::new();
    }
    *sel = crate::selection::Selection::default();
    selection_changed(gpu, pop, sel, crate::selection::Shown::Keep)
}

/// Narrow the selection to `ids` in place, without `RequestDialog` (the wounded filter
/// 0x4ecda0 takes entities out of the list and clears their flags 0x600): the dialogs as they
/// are, refreshed.
pub fn narrow_selection(gpu: &mut Gpu, pop: &crate::population::Population, sel: &mut crate::selection::Selection, ids: Vec<u16>) -> Vec<guistate::UiEvent> {
    if ids == sel.ids {
        return Vec::new();
    }
    sel.ids = ids;
    let how = if sel.is_empty() { crate::selection::Shown::Reset } else { crate::selection::Shown::Keep };
    selection_changed(gpu, pop, sel, how)
}

/// Select exactly `ids` (the next building of a type, 0x4e9e70: `SetId`, then `RequestDialog`)
/// and show it.
pub fn select_ids(gpu: &mut Gpu, pop: &crate::population::Population, sel: &mut crate::selection::Selection, ids: Vec<u16>) -> Vec<guistate::UiEvent> {
    sel.ids = ids;
    sel.other.clear();
    selection_changed(gpu, pop, sel, crate::selection::Shown::Request)
}

/// Show (or refresh) the selected building's dialog from its facts (building_dialog.rs) and
/// recompose the panel when it changed.
pub fn building_frame(gpu: &mut Gpu, facts: &crate::building_dialog::Facts) {
    if gpu.ui.as_mut().is_some_and(|ui| ui.show_building(facts)) {
        recompose(gpu);
    }
}

/// Show (`Some((x0, y0, x1, y1))`, screen px) or hide the selection box.
pub fn selection_box(gpu: &mut Gpu, rect: Option<(i32, i32, i32, i32)>) {
    let size = gpu.size();
    if let Some(g) = &mut gpu.gui {
        g.set_selection_box(&gpu.device, &gpu.queue, size, rect);
    }
}
