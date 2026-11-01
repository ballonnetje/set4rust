//! Painting a world view: images and data layers. `Stage` maps world pixels to the screen.

use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use super::details::short_good;
use crate::editor::Sel;
use crate::geometry::{RoutineState, cell_px, facing_cell, is_cell};
use crate::images::Tex;
use crate::layers::{LayerId, layer};
use crate::model::{Building, Frame, WorkList, half_k};
use crate::theme::from_rgb;

/// A world view: world pixel (x, y) is at (ox + x·z, oy + y·z) on the screen.
pub struct Stage {
    pub painter: Painter,
    pub rect: Rect,
    pub z: f32,
    pub ox: f32,
    pub oy: f32,
}

/// What a data layer needs besides the building: selection, the routine shown and its state.
#[derive(Clone, Copy, Default)]
pub struct Overlay<'a> {
    pub sel: Option<Sel>,
    pub route: Option<&'a WorkList>,
    pub state: Option<&'a RoutineState<'a>>,
}

impl Stage {
    pub fn p(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.ox + x * self.z, self.oy + y * self.z)
    }

    pub fn c(&self, c: [i32; 2]) -> Pos2 {
        let [x, y] = cell_px(c);
        self.p(x as f32, y as f32)
    }

    fn r(&self, base: f32) -> f32 {
        base * (self.z / 2.0).max(1.0)
    }

    pub fn world(&self, pos: Pos2) -> [f64; 2] {
        [((pos.x - self.ox) / self.z) as f64, ((pos.y - self.oy) / self.z) as f64]
    }

    /// `tex`'s region `src` (x, y, w, h in pixels) with its top-left at `at`, `size` on screen.
    pub fn image(&self, tex: Tex, src: [f32; 4], at: Pos2, size: Vec2, alpha: f32) {
        let uv = Rect::from_min_size(pos2(src[0] / tex.w, src[1] / tex.h), vec2(src[2] / tex.w, src[3] / tex.h));
        let tint = Color32::from_white_alpha((alpha.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        self.painter.image(tex.id, Rect::from_min_size(at, size), uv, tint);
    }

    /// An original frame from its strip; `at`: the frame's own (left, top), or the place given.
    pub fn strip_frame(&self, tex: Tex, fr: &Frame, at: Option<[f32; 2]>, alpha: f32) {
        let hk = half_k(fr) as f32;
        let [lx, ly] = at.unwrap_or([fr[3] as f32 * hk, fr[4] as f32 * hk]);
        let src = [fr[6] as f32, fr[7] as f32, fr[1] as f32, fr[2] as f32];
        self.image(tex, src, self.p(-lx, -ly), vec2(fr[1] as f32 * hk * self.z, fr[2] as f32 * hk * self.z), alpha);
    }

    /// One of your frames on the anchor; the top-left and size on screen.
    pub fn pack_frame(&self, tex: Tex, anchor: [f64; 2], scale: f64, alpha: f32) -> Rect {
        let sc = scale as f32;
        let at = self.p(-anchor[0] as f32 / sc, -anchor[1] as f32 / sc);
        let size = vec2(tex.w / sc * self.z, tex.h / sc * self.z);
        self.image(tex, [0.0, 0.0, tex.w, tex.h], at, size, alpha);
        Rect::from_min_size(at, size)
    }
}

fn ring(st: &Stage, at: Pos2, r: f32) {
    st.painter.circle_stroke(at, r + 2.5, Stroke::new(2.0, Color32::WHITE));
}

fn dot(st: &Stage, c: [i32; 2], r: f32, fill: Option<Color32>, stroke: Color32, sel: bool) {
    if !is_cell(c) {
        return;
    }
    let at = st.c(c);
    let rr = st.r(r);
    match fill {
        Some(f) => {
            st.painter.circle_filled(at, rr, f);
        }
        None => {
            st.painter.circle_stroke(at, rr, Stroke::new(1.5, stroke));
        }
    }
    if sel {
        ring(st, at, rr);
    }
}

fn grid(st: &Stage) {
    // Only the nodes inside the view, as one mesh.
    let (r, z) = (st.rect, st.z);
    let (sx, sy) = (24.0 * z, 12.0 * z);
    if sy < 4.0 {
        return;
    }
    let color = Color32::from_white_alpha(71);
    let mut mesh = Mesh::default();
    let first = ((r.top() - st.oy) / sy).ceil() as i32;
    let last = ((r.bottom() - st.oy) / sy).floor() as i32;
    for dy in first..=last {
        let row_x = st.ox - 12.0 * dy as f32 * z;
        let y = st.oy + dy as f32 * sy;
        let from = ((r.left() - row_x) / sx).ceil() as i32;
        let to = ((r.right() - row_x) / sx).floor() as i32;
        for dx in from..=to {
            mesh.add_colored_rect(Rect::from_min_size(pos2(row_x + dx as f32 * sx - 1.0, y - 1.0), vec2(2.0, 2.0)), color);
        }
    }
    st.painter.add(mesh);
}

/// One data layer. A building layer without a building is drawn by nobody: the callers check.
pub fn data_layer(st: &Stage, id: LayerId, b: Option<&Building>, o: &Overlay<'_>) {
    let col = from_rgb(layer(id).color);
    let is_sel = |i: i32| o.sel.is_some_and(|s| s.layer == id && s.index == i);
    let z = st.z;
    match id {
        LayerId::Grid => grid(st),
        LayerId::Anchor => {
            let at = st.p(0.0, 0.0);
            let s = Stroke::new(1.0, col);
            st.painter.line_segment([at + vec2(-6.0, 0.5), at + vec2(7.0, 0.5)], s);
            st.painter.line_segment([at + vec2(0.5, -6.0), at + vec2(0.5, 7.0)], s);
        }
        _ => {}
    }
    let Some(b) = b else { return };
    match id {
        LayerId::Footprint => {
            let c = col.gamma_multiply(0.6);
            for &cell in &b.footprint {
                dot(st, cell, 3.5, Some(c), c, false);
            }
        }
        LayerId::Block => {
            for &cell in &b.block {
                dot(st, cell, 4.0, None, col, false);
            }
        }
        LayerId::Dig => {
            for &cell in b.dig.iter().filter(|c| is_cell(**c)) {
                let r = st.r(2.5);
                st.painter.rect_stroke(Rect::from_center_size(st.c(cell), vec2(2.0 * r, 2.0 * r)), 0.0, Stroke::new(1.0, col), egui::StrokeKind::Middle);
            }
        }
        LayerId::Door => dot(st, b.door, 4.0, Some(col), col, is_sel(-1)),
        LayerId::Flag => dot(st, b.flag, 4.0, Some(col), col, is_sel(-1)),
        LayerId::Work => {
            if let Some(c) = b.work {
                dot(st, c, 4.0, Some(col), col, is_sel(-1));
            }
        }
        LayerId::Builders => {
            for (i, s) in b.builders.iter().enumerate() {
                let c = [s[0], s[1]];
                dot(st, c, 2.5, Some(col), col, is_sel(i as i32));
                // Facing: the six hex directions as screen vectors.
                let [ex, ey] = cell_px(facing_cell(s[2]));
                let at = st.c(c);
                st.painter.line_segment([at, at + vec2(ex as f32 * z * 0.4, ey as f32 * z * 0.4)], Stroke::new(1.5, col));
            }
        }
        LayerId::Piles => {
            for (i, x) in b.piles.iter().enumerate() {
                if !is_cell(x.cell) {
                    continue;
                }
                let at = st.c(x.cell);
                let r = st.r(4.5);
                let pts = vec![at + vec2(0.0, -r), at + vec2(r, 0.0), at + vec2(0.0, r), at + vec2(-r, 0.0)];
                st.painter.add(Shape::convex_polygon(pts, col, Stroke::new(1.0, Color32::from_black_alpha(153))));
                if z >= 2.0 {
                    st.painter.text(at + vec2(r + 3.0, 0.0), Align2::LEFT_CENTER, short_good(x.good.as_deref()), FontId::proportional(11.0), Color32::from_rgb(0xe6, 0xff, 0xfb));
                }
                if is_sel(i as i32) {
                    ring(st, at, r);
                }
            }
        }
        LayerId::SoldiersPx => {
            for (i, s) in b.soldiers_px.iter().enumerate() {
                let at = st.p(s[0] as f32, s[1] as f32);
                let r = st.r(4.0);
                let pts = vec![at + vec2(0.0, -r), at + vec2(r, r * 0.8), at + vec2(-r, r * 0.8)];
                if s[3] != 0 {
                    st.painter.add(Shape::convex_polygon(pts, col, Stroke::NONE));
                } else {
                    st.painter.add(PathShape::closed_line(pts, PathStroke::new(1.5, col)));
                }
                if is_sel(i as i32) {
                    ring(st, at, r);
                }
            }
        }
        LayerId::MiniFlagPx => {
            if let Some(m) = b.mini_flag_px {
                let at = st.p(m[0] as f32, m[1] as f32);
                let r = st.r(4.0);
                st.painter.line_segment([at, at + vec2(0.0, -2.5 * r)], Stroke::new(1.5, col));
                let pts = vec![at + vec2(0.0, -2.5 * r), at + vec2(1.6 * r, -2.0 * r), at + vec2(0.0, -1.5 * r)];
                st.painter.add(Shape::convex_polygon(pts, col, Stroke::NONE));
                if is_sel(-1) {
                    ring(st, at, r);
                }
            }
        }
        LayerId::Path => {
            if let Some(wl) = o.route {
                path(st, wl, o.state, col);
            }
        }
        _ => {}
    }
}

/// The route: from the door through every walk target; numbered stops where work happens.
fn path(st: &Stage, wl: &WorkList, state: Option<&RoutineState<'_>>, col: Color32) {
    let Some(s0) = wl.steps.first() else { return };
    let mut pts = vec![st.p(s0.from[0] as f32, s0.from[1] as f32)];
    pts.extend(wl.steps.iter().map(|s| st.p(s.to[0] as f32, s.to[1] as f32)));
    st.painter.extend(Shape::dashed_line(&pts, Stroke::new(1.5, col), 5.0, 4.0));
    let mut stops: Vec<([i32; 2], Vec<usize>)> = Vec::new();
    for (i, s) in wl.steps.iter().enumerate() {
        match stops.iter_mut().find(|(at, _)| *at == s.to) {
            Some((_, steps)) => steps.push(i),
            None => stops.push((s.to, vec![i])),
        }
    }
    for (at, steps) in stops {
        let pos = st.p(at[0] as f32, at[1] as f32);
        let cur = state.is_some_and(|x| steps.contains(&x.i));
        let work = steps.iter().any(|&i| !wl.steps[i].task.contains("GO_VIRTUAL"));
        let fill = if cur {
            Color32::WHITE
        } else if work {
            col
        } else {
            Color32::from_rgba_unmultiplied(255, 209, 102, 115)
        };
        st.painter.circle_filled(pos, st.r(if cur { 5.0 } else { 3.5 }), fill);
        if st.z >= 2.0 {
            let label: Vec<String> = steps.iter().map(|i| (i + 1).to_string()).collect();
            let o = st.r(4.0);
            st.painter.text(pos + vec2(o + 2.0, -o), Align2::LEFT_BOTTOM, label.join(","), FontId::proportional(10.0), Color32::from_rgb(0xff, 0xf3, 0xcd));
        }
    }
}

/// The dark checkerboard behind images, in both themes: overlay colours stay readable.
pub fn checker_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let (a, b) = (Color32::from_rgb(0x2a, 0x2e, 0x35), Color32::from_rgb(0x23, 0x26, 0x2c));
    let img = egui::ColorImage::new([2, 2], vec![a, b, b, a]);
    ctx.load_texture("checker", img, egui::TextureOptions::NEAREST_REPEAT)
}

/// Fills `rect` with the checkerboard, 16 points per tile pair.
pub fn paint_checker(painter: &Painter, tex: &egui::TextureHandle, rect: Rect, corner: f32) {
    let uv = Rect::from_min_size(Pos2::ZERO, rect.size() / 16.0);
    painter.add(egui::epaint::RectShape::filled(rect, corner, Color32::WHITE).with_texture(tex.id(), uv));
}
