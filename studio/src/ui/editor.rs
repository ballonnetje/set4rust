//! The editor page: tools, one large view, the routine timeline; and its inspector panels.

use egui::{Align2, Color32, CursorIcon, FontId, PointerButton, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::details::{legend, short_good};
use super::slot::tick_of;
use super::stage::{Overlay, Stage, data_layer, paint_checker};
use super::{EdView, Studio, frames_at, widgets};
use crate::editor::{Editor, Scope, Tool};
use crate::geometry::{RoutineState, TASK_LEGEND, cell_px, routine_at, task_color};
use crate::images::Yours;
use crate::layers::{LayerId, LayerKind, layer, layer_of_key};
use crate::model::{BLD_KEYS, Building, Group, Seq, Slot, SlotRef, WorkList, half_k};
use crate::theme::{self, Palette, from_rgb, icon};

pub const KEYS_HELP: &str = "V select · B paint · E erase · A add · N set anchor · Del delete · arrows nudge (Shift ×10) · Ctrl+Z / Ctrl+Y undo / redo · Ctrl+S save · Esc deselect";

const AREAS: [LayerId; 3] = [LayerId::Footprint, LayerId::Block, LayerId::Dig];
const ADDS: [LayerId; 7] = [LayerId::Piles, LayerId::Builders, LayerId::SoldiersPx, LayerId::Door, LayerId::Flag, LayerId::Work, LayerId::MiniFlagPx];
const FIT_ZOOMS: [u32; 6] = [0, 1, 2, 3, 4, 6];

impl Studio {
    pub(super) fn editor_page(&mut self, ui: &mut Ui, r: SlotRef, p: &Palette) {
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let seqs_k = self.ed.as_ref().map_or(1, |ed| ed.mult(Scope { g, slot, pack: &self.pack, layers: &self.layers }, ed.current_seq(slot)));
        self.kmax = seqs_k;
        self.editor_tools(ui, g, slot, p);
        let work_lists = self.ed.as_ref().and_then(|e| e.building().map(|b| b.work_lists.clone()));
        if let Some(lists) = work_lists.filter(|l| !l.is_empty()) {
            egui::Panel::bottom("timeline").resizable(true).default_size(196.0).size_range(120.0..=520.0).frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.line)).inner_margin(egui::Margin::same(12))).show(ui, |ui| {
                self.timeline(ui, &lists, p);
            });
        }
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(p.bg)).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                let help = 20.0;
                let size = vec2(ui.available_width(), (ui.available_height() - help).max(160.0));
                self.canvas(ui, g, slot, size, p);
                widgets::small(ui, KEYS_HELP, p);
            });
        });
    }

    fn editor_tools(&mut self, ui: &mut Ui, g: &Group, slot: &Slot, p: &Palette) {
        let Some(ed) = self.ed.as_mut() else { return };
        let has_b = ed.building().is_some();
        let has_parts = has_b && slot.seqs.iter().any(|q| q.patch.is_some());
        let mut new_seq = None;
        egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.line)).inner_margin(egui::Margin::symmetric(16, 7)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let tool = |ui: &mut Ui, ed: &mut Editor, t: Tool, ic: &str, label: &str, key: &str| {
                    if widgets::tool(ui, &format!("{ic}  {label}"), ed.tool == t, &format!("{label} ({key})"), p).clicked() {
                        ed.tool = t;
                        ed.hover = None;
                    }
                };
                tool(ui, ed, Tool::Select, icon::CURSOR, "Select", "V");
                if has_b {
                    tool(ui, ed, Tool::Paint, icon::PAINT_BRUSH, "Paint", "B");
                    tool(ui, ed, Tool::Erase, icon::ERASER, "Erase", "E");
                    egui::ComboBox::from_id_salt("paint").width(112.0).selected_text(layer(ed.paint).label).show_ui(ui, |ui| {
                        for id in AREAS {
                            if ui.selectable_label(ed.paint == id, layer(id).label).clicked() {
                                ed.paint = id;
                                if ed.tool != Tool::Erase {
                                    ed.tool = Tool::Paint;
                                }
                            }
                        }
                    }).response.on_hover_text("Area layer to paint or erase");
                    tool(ui, ed, Tool::Add, icon::PLUS_CIRCLE, "Add", "A");
                    egui::ComboBox::from_id_salt("add").width(116.0).selected_text(layer(ed.add).label).show_ui(ui, |ui| {
                        for id in ADDS {
                            if ui.selectable_label(ed.add == id, layer(id).label).clicked() {
                                ed.add = id;
                                ed.tool = Tool::Add;
                            }
                        }
                    }).response.on_hover_text("What a click adds (single items move there)");
                }
                tool(ui, ed, Tool::Anchor, icon::CROSSHAIR, "Set anchor", "N");
                ui.separator();
                egui::ComboBox::from_id_salt("edseq").width(120.0).selected_text(slot.seqs[ed.seq].label.as_str()).show_ui(ui, |ui| {
                    for (i, q) in slot.seqs.iter().enumerate() {
                        if ui.selectable_label(ed.seq == i, &q.label).clicked() {
                            new_seq = Some(i);
                        }
                    }
                });
                let pick = self.view.pick;
                let shown = if self.view.z.is_some() { "custom".to_string() } else if pick == 0 { "fit".to_string() } else { format!("{pick}×") };
                egui::ComboBox::from_id_salt("edzoom").width(62.0).selected_text(shown).show_ui(ui, |ui| {
                    for z in FIT_ZOOMS {
                        if ui.selectable_label(pick == z && self.view.z.is_none(), if z == 0 { "fit".to_string() } else { format!("{z}×") }).clicked() {
                            self.view = EdView { pick: z, ..EdView::default() };
                        }
                    }
                });
                if has_parts {
                    ui.checkbox(&mut ed.parts, "animate parts").on_hover_text("Play the building's animation parts (patches) on top of the finished image, as the game does");
                }
            });
        });
        if let Some(i) = new_seq {
            ed.seq = i;
            ed.sel = None;
            self.view = EdView { pick: self.view.pick, ..EdView::default() };
        }
        let dirty = ed.is_dirty(g, slot, &self.pack);
        let (undo, redo) = (ed.can_undo(), ed.can_redo());
        egui::Frame::new().fill(p.panel2).inner_margin(egui::Margin::symmetric(16, 5)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (text, color) = if dirty { (format!("{}  Unsaved changes: nothing is written to the pack until you press Save", icon::WARNING), p.warn) } else { (format!("{}  All changes saved", icon::CHECK_CIRCLE), p.dim) };
                ui.label(RichText::new(text).color(color).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::primary(ui, &format!("{}  Save", icon::FLOPPY_DISK), dirty, p).on_hover_text("Write the changes to the pack (Ctrl+S)").clicked() {
                        self.defer(Studio::editor_save);
                    }
                    if ui.add_enabled(dirty, egui::Button::new("Discard")).on_hover_text("Drop all unsaved changes").clicked() {
                        self.defer(|st| st.ask("Discard all unsaved changes?", "Discard", true, Studio::editor_discard));
                    }
                    ui.separator();
                    if ui.add_enabled(redo, egui::Button::new(RichText::new(icon::REDO).size(15.0))).on_hover_text("Redo (Ctrl+Y)").clicked() {
                        self.defer(|st| st.undo(false));
                    }
                    if ui.add_enabled(undo, egui::Button::new(RichText::new(icon::UNDO).size(15.0))).on_hover_text("Undo (Ctrl+Z)").clicked() {
                        self.defer(|st| st.undo(true));
                    }
                });
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_original(&mut self, st: &Stage, g: &Group, slot: &Slot, seq: &Seq, i: usize, tick: i64, parts: bool) {
        let spec = self.spec.clone();
        if let Some(tex) = self.images.strip(&spec, g, seq, self.team) {
            st.strip_frame(tex, &seq.frames[i], None, 1.0);
        }
        if parts {
            let main = &seq.frames[i];
            let at = [(main[3] as f64 * half_k(main)) as f32, (main[4] as f64 * half_k(main)) as f32];
            for q in slot.seqs.iter().filter(|q| q.patch.is_some() && !q.frames.is_empty()) {
                if let Some(tex) = self.images.strip(&spec, g, q, self.team) {
                    st.strip_frame(tex, &q.frames[tick.rem_euclid(q.frames.len() as i64) as usize], Some(at), 1.0);
                }
            }
        }
    }

    fn draw_worker(&mut self, st: &Stage, g: &Group, state: Option<&RoutineState<'_>>) {
        let Some(s) = state.filter(|s| s.step.visible) else { return };
        let spec = self.spec.clone();
        let Some(sg) = spec.settler_group(g) else { return };
        let Some(sl) = sg.slots.iter().find(|x| x.id == s.step.job) else { return };
        let dir = s.dir.to_string();
        let Some(q) = sl.seqs.iter().find(|x| x.key == dir).or(sl.seqs.first()) else { return };
        if let Some(tex) = self.images.strip(&spec, sg, q, self.team) {
            let fr = &q.frames[(s.frame.max(0) as usize).min(q.frames.len() - 1)];
            let hk = half_k(fr) as f32;
            let src = [fr[6] as f32, fr[7] as f32, fr[1] as f32, fr[2] as f32];
            let at = st.p(s.pos[0] as f32 - fr[3] as f32 * hk, s.pos[1] as f32 - fr[4] as f32 * hk);
            st.image(tex, src, at, vec2(fr[1] as f32 * hk * st.z, fr[2] as f32 * hk * st.z), 1.0);
        }
    }

    /// Your frame `j` and, on the finished image, your patch frames at its top-left. The rectangle
    /// the frame covers, when it is there.
    #[allow(clippy::too_many_arguments)]
    fn draw_yours(&mut self, st: &Stage, g: &Group, slot: &Slot, seq: &Seq, j: usize, tick: i64, parts: bool, anchor: [f64; 2]) -> Option<egui::Rect> {
        let (spec, pack) = (self.spec.clone(), self.pack.clone());
        let Yours::Ready(tex) = self.images.frame(&spec, &pack, g, slot, seq, j, self.team, true) else { return None };
        let sc = pack.scale();
        let rect = st.pack_frame(tex, anchor, sc, 1.0);
        if parts {
            for q in slot.seqs.iter().filter(|q| q.patch.is_some() && !q.frames.is_empty()) {
                let k = self.mult_of(g, slot, q) as i64;
                let n = q.frames.len() as i64 * k;
                let f = (tick * k).rem_euclid(n) as usize;
                if let Yours::Ready(ps) = self.images.frame(&spec, &pack, g, slot, q, f, self.team, true) {
                    st.image(ps, [0.0, 0.0, ps.w, ps.h], rect.min, vec2(ps.w / sc as f32 * st.z, ps.h / sc as f32 * st.z), 1.0);
                }
            }
        }
        Some(rect)
    }

    fn canvas(&mut self, ui: &mut Ui, g: &Group, slot: &Slot, size: egui::Vec2, p: &Palette) {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        if let Some(c) = &self.checker {
            paint_checker(ui.painter(), c, rect, 6.0);
        }
        ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, p.line), egui::StrokeKind::Outside);
        let pack = self.pack.clone();
        let layers = self.layers.clone();
        let s = Scope { g, slot, pack: &pack, layers: &layers };
        let Some(ed) = self.ed.as_mut() else { return };
        let bounds = ed.bounds(s);
        let seq = ed.current_seq(slot);
        let fit = match self.view.fit {
            Some((z, size)) if size == rect.size() => z,
            _ => {
                let z = ((2.0 * (rect.width() as f64 / bounds.w).min(rect.height() as f64 / bounds.h)).floor() / 2.0).clamp(0.5, 6.0) as f32;
                self.view.fit = Some((z, rect.size()));
                z
            }
        };
        let mut z = self.view.z.unwrap_or(if self.view.pick > 0 { self.view.pick as f32 } else { fit });
        let center = *self.view.center.get_or_insert([(bounds.x0 + bounds.w / 2.0) as f32, (bounds.y0 + bounds.h / 2.0) as f32]);

        // Wheel zooms around the pointer; the middle or right button pans.
        if resp.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (1.0 + scroll * 0.002);
            if (factor - 1.0).abs() > 1e-4
                && let Some(at) = resp.hover_pos()
            {
                let nz = (z * factor).clamp(0.5, 16.0);
                let c = rect.center() + self.view.pan;
                self.view.pan += (at - c) * (1.0 - nz / z);
                z = nz;
                self.view.z = Some(nz);
            }
        }
        if resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) {
            self.view.pan += resp.drag_delta();
        }
        let o = rect.center() + self.view.pan;
        let stage = Stage { painter: ui.painter_at(rect), rect, z, ox: o.x - center[0] * z, oy: o.y - center[1] * z };

        #[cfg(test)]
        {
            self.xf = [stage.ox, stage.oy, stage.z];
        }

        // Input.
        let (pressed, released, pointer) = ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.latest_pos()));
        let mut message = None;
        let world = pointer.map(|q| stage.world(q));
        if let Some([wx, wy]) = world {
            if pressed && resp.hovered() {
                if let Err(m) = ed.press(s, wx, wy, z as f64) {
                    message = Some(m);
                }
            } else if ed.dragging() {
                ed.drag_to(s, wx, wy);
            }
            if matches!(ed.tool, Tool::Paint | Tool::Erase) {
                ed.hover = resp.hovered().then(|| crate::geometry::snap_cell(wx, wy));
            }
        }
        if released {
            ed.release(s);
        }
        if resp.hovered() {
            let h = if ed.tool == Tool::Select { world.and_then(|[x, y]| ed.hit(s, x, y, z as f64)) } else { None };
            ui.ctx().set_cursor_icon(if ed.tool != Tool::Select {
                CursorIcon::Crosshair
            } else if h.is_some() {
                CursorIcon::Move
            } else {
                CursorIcon::Default
            });
            if let Some(h) = h {
                resp.clone().on_hover_text(layer(h.layer).label);
            }
        }
        if let Some(m) = message {
            self.toast(m);
        }

        // Drawing.
        let Some(ed) = self.ed.as_mut() else { return };
        let n = seq.frames.len();
        let k = ed.mult(s, seq);
        let nk = n * k as usize;
        let pos = self.play.cur(self.now);
        let (j, i) = frames_at(pos, n, k);
        let tick = tick_of(pos);
        ed.frame = j;
        let (b, parts_on, route, sel, this_anchor, tool, hover) = {
            let b: Option<Building> = ed.building().cloned();
            let finished = seq.key == "1";
            let parts = ed.parts && b.is_some() && finished;
            (b, parts, ed.route, ed.sel, ed.anchors_now(s).get(j).copied().unwrap_or([0.0, 0.0]), ed.tool, ed.hover)
        };
        let wl: Option<&WorkList> = b.as_ref().and_then(|x| x.work_lists.get(route));
        let state = wl.and_then(|w| routine_at(w, tick));
        let shown_route = if seq.key == "1" { wl } else { None };
        for l in layers.iter().rev() {
            let def = layer(l.id);
            if !l.vis || (def.bld && b.is_none()) {
                continue;
            }
            match l.id {
                LayerId::Original => self.draw_original(&stage, g, slot, seq, i, tick, parts_on),
                LayerId::Yours => {
                    let rect2 = self.draw_yours(&stage, g, slot, seq, j, tick, parts_on, this_anchor);
                    if let (Some(rc), Some(x)) = (rect2, sel)
                        && x.layer == LayerId::Yours
                    {
                        let line = [rc.left_top(), rc.right_top(), rc.right_bottom(), rc.left_bottom(), rc.left_top()];
                        stage.painter.extend(egui::Shape::dashed_line(&line, Stroke::new(1.0, Color32::WHITE), 4.0, 3.0));
                    }
                }
                LayerId::Worker => {
                    if seq.key == "1" {
                        self.draw_worker(&stage, g, state.as_ref());
                    }
                }
                id => data_layer(&stage, id, b.as_ref(), &Overlay { sel, route: shown_route, state: state.as_ref() }),
            }
        }
        if let (Some(c), true) = (hover, matches!(tool, Tool::Paint | Tool::Erase)) {
            let [hx, hy] = cell_px(c);
            let at = stage.p(hx as f32, hy as f32);
            let r = 6.0 * (z / 2.0).max(1.0);
            let col = if tool == Tool::Paint { Color32::from_rgb(0x7c, 0xfc, 0x9a) } else { Color32::from_rgb(0xff, 0x5c, 0x5c) };
            stage.painter.circle_stroke(at, r, Stroke::new(2.0, col));
            if tool == Tool::Erase {
                let d = r * 0.6;
                stage.painter.line_segment([at + vec2(-d, -d), at + vec2(d, d)], Stroke::new(2.0, col));
                stage.painter.line_segment([at + vec2(d, -d), at + vec2(-d, d)], Stroke::new(2.0, col));
            }
        }
        let status = format!(
            "frame {} / {n}{}{}",
            i + 1,
            if k > 1 { format!(" · yours {} / {nk}", j + 1) } else { String::new() },
            match (&state, wl) {
                (Some(st), Some(w)) => format!(" · routine tick {} / {}, step {}", st.t, w.ticks, st.i + 1),
                _ => String::new(),
            }
        );
        let badge = stage.rect.left_top() + vec2(10.0, 10.0);
        let label = ui.painter().layout_no_wrap(status, FontId::proportional(11.5), Color32::from_gray(225));
        ui.painter().rect_filled(egui::Rect::from_min_size(badge, label.size()).expand2(vec2(7.0, 3.0)), 5, Color32::from_black_alpha(150));
        ui.painter().galley(badge, label, Color32::WHITE);
    }

    fn seek(&mut self, tick: i64) {
        let now = self.now;
        self.play.seek_tick(tick, now);
    }

    fn timeline(&mut self, ui: &mut Ui, lists: &[WorkList], p: &Palette) {
        let route = self.ed.as_ref().map_or(0, |e| e.route).min(lists.len() - 1);
        let wl = &lists[route];
        ui.horizontal(|ui| {
            ui.label(RichText::new("Worker routine").font(theme::semibold(13.0)));
            egui::ComboBox::from_id_salt("route").width(220.0).selected_text(format!("{} ({} ticks)", wl.name, wl.ticks)).show_ui(ui, |ui| {
                for (i, w) in lists.iter().enumerate() {
                    if ui.selectable_label(route == i, format!("{} ({} ticks)", w.name, w.ticks)).clicked()
                        && let Some(ed) = self.ed.as_mut()
                    {
                        ed.route = i;
                    }
                }
            });
            ui.label(RichText::new(icon::INFO).color(p.faint).size(14.0)).on_hover_text("From jobInfo.xml; timing as CHouseWorkerRole runs it, one tick = 71 ms. Shown on the finished image. Click the bar or a row to jump.");
        });
        let pos = self.play.cur(self.now);
        let state = routine_at(wl, tick_of(pos));
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 4, p.panel2);
        let t_x = |t: f32| rect.left() + rect.width() * t / wl.ticks.max(1) as f32;
        let mut tip = None;
        let hover_x = resp.hover_pos().map(|q| q.x);
        for s in wl.steps.iter().filter(|s| s.ticks.is_some_and(|t| t > 0)) {
            let t = s.ticks.unwrap_or(0) as f32;
            let seg = egui::Rect::from_min_max(pos2(t_x(s.start as f32), rect.top() + 3.0), pos2((t_x(s.start as f32 + t) - 1.0).max(t_x(s.start as f32) + 1.0), rect.bottom() - 3.0));
            ui.painter().rect_filled(seg, 2, from_rgb(task_color(&s.task)).gamma_multiply(0.85));
            if hover_x.is_some_and(|x| x >= seg.left() && x <= seg.right()) {
                let marks: Vec<String> = s.trigger.iter().cloned().chain(s.sound.iter().map(|x| format!("{} on frame {}", x.name, x.frame))).collect();
                let marks = if marks.is_empty() { String::new() } else { format!("\n{}", marks.join(" · ")) };
                tip = Some(format!("{} · {} · {} ticks{marks}", s.task_name(), s.job, t));
            }
            if s.sound.is_some() || s.trigger.is_some() {
                ui.painter().rect_filled(egui::Rect::from_min_size(pos2(seg.left(), rect.top()), vec2(2.0, 7.0)), 0, Color32::WHITE);
            }
        }
        if let Some(st) = &state {
            let x = t_x(st.t as f32);
            ui.painter().vline(x, rect.y_range(), Stroke::new(2.0, Color32::WHITE));
        }
        if resp.is_pointer_button_down_on()
            && let Some(q) = resp.interact_pointer_pos()
        {
            let t = ((q.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * wl.ticks as f32;
            self.seek(t.floor() as i64);
        }
        if let Some(t) = tip {
            resp.on_hover_text(t);
        }
        let mut items: Vec<(Color32, &str)> = TASK_LEGEND.iter().map(|(t, l)| (from_rgb(task_color(t)), *l)).collect();
        items.push((Color32::WHITE, "trigger / sound"));
        legend(ui, &items, p);
        let cols = [(0.0, "#"), (28.0, "tick"), (66.0, "ticks"), (104.0, "task"), (240.0, "cell"), (296.0, "animation"), (520.0, "dir"), (552.0, "good"), (640.0, "trigger / sound")];
        let header = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover()).0;
        for (x, h) in cols {
            ui.painter().text(pos2(header.left() + 6.0 + x, header.center().y), Align2::LEFT_CENTER, h, FontId::proportional(11.0), p.dim);
        }
        let cur = state.as_ref().map(|s| s.i);
        egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("steps").show_rows(ui, 22.0, wl.steps.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                let s = &wl.steps[i];
                let (row, r) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                if cur == Some(i) {
                    ui.painter().rect_filled(row, 4, p.accent_soft);
                } else if r.hovered() {
                    ui.painter().rect_filled(row, 4, p.panel2);
                }
                let trigger: Vec<String> = s.trigger.iter().cloned().chain(s.sound.iter().map(|x| format!("{} @{}", x.name, x.frame))).collect();
                let cells = [
                    (i + 1).to_string(),
                    s.start.to_string(),
                    s.ticks.map_or("—".to_string(), |t| t.to_string()),
                    s.task_name().to_string(),
                    format!("{}, {}", s.cell[0], s.cell[1]),
                    format!("{} ({})", s.job, s.frames),
                    s.dir.to_string(),
                    s.good.as_deref().map_or(String::new(), |g| short_good(Some(g))),
                    trigger.join(" · "),
                ];
                for (n, (c, (x, _))) in cells.iter().zip(cols).enumerate() {
                    let at = pos2(row.left() + 6.0 + x, row.center().y);
                    if n == 3 {
                        ui.painter().circle_filled(at + vec2(4.0, 0.0), 4.0, from_rgb(task_color(&s.task)));
                        ui.painter().text(at + vec2(13.0, 0.0), Align2::LEFT_CENTER, c, FontId::proportional(11.5), p.text);
                    } else {
                        ui.painter().text(at, Align2::LEFT_CENTER, c, FontId::proportional(11.5), p.text);
                    }
                }
                if r.clicked() {
                    let start = s.start as i64;
                    self.defer(move |st| st.seek(start));
                }
            }
        });
    }

    fn num(&mut self, ui: &mut Ui, value: i64, range: Option<(i64, i64)>) -> Option<(i64, u64)> {
        let mut v = value;
        let mut d = egui::DragValue::new(&mut v).speed(0.25);
        if let Some((a, b)) = range {
            d = d.range(a..=b);
        }
        let r = ui.add(d);
        if r.drag_stopped() || r.lost_focus() {
            self.edit_session += 1;
        }
        (r.changed() && v != value).then_some((v, r.id.value() ^ self.edit_session))
    }

    pub(super) fn inspector(&mut self, ui: &mut Ui, r: SlotRef, p: &Palette) {
        widgets::heading(ui, "Inspector");
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let Some(sel) = self.ed.as_ref().and_then(|e| e.sel) else {
            let has_b = self.ed.as_ref().is_some_and(|e| e.building().is_some());
            widgets::small(ui, if has_b {
                "Click a dot to select it, drag to move. Paint adds and Erase removes cells of the chosen area layer; Add places the chosen item. Drag your image (or click a pixel with Set anchor) to place it on the anchor node. Changes stay unsaved until you press Save."
            } else {
                "Drag your image, or click the pixel that stands on the map node with Set anchor. Changes stay unsaved until you press Save."
            }, p);
            return;
        };
        let def = layer(sel.layer);
        let title = if sel.index >= 0 { format!("{} {}", def.label, sel.index + 1) } else { def.label.to_string() };
        Studio::sel_title(ui, &title, from_rgb(def.color));
        if sel.layer == LayerId::Yours {
            self.inspect_image(ui, g, slot, p);
            return;
        }
        let Some(b) = self.ed.as_ref().and_then(|e| e.building().cloned()) else { return };
        let i = sel.index.max(0) as usize;
        match sel.layer {
            id if def.kind == LayerKind::Cell1 => {
                let c = match id {
                    LayerId::Door => Some(b.door),
                    LayerId::Flag => Some(b.flag),
                    _ => b.work,
                };
                if let Some(c) = c {
                    self.cell_row(ui, "cell", c, p, def.label, move |o, v| match id {
                        LayerId::Door => o.door = v,
                        LayerId::Flag => o.flag = v,
                        _ => o.work = Some(v),
                    });
                }
            }
            LayerId::Builders => {
                let q = b.builders[i];
                self.cell_row(ui, "cell", [q[0], q[1]], p, "builder", move |o, v| {
                    o.builders[i][0] = v[0];
                    o.builders[i][1] = v[1];
                });
                ui.horizontal(|ui| {
                    widgets::caption(ui, "facing", p);
                    if let Some((v, m)) = self.num(ui, q[2] as i64, Some((0, 5))) {
                        self.edit("builder", m, move |o| o.builders[i][2] = (v as i32).rem_euclid(6));
                    }
                });
            }
            LayerId::Piles => {
                let x = b.piles[i].clone();
                self.cell_row(ui, "cell", x.cell, p, "pile", move |o, v| o.piles[i].cell = v);
                ui.horizontal(|ui| {
                    widgets::caption(ui, "good", p);
                    let id = ui.scope_id().with(("good", i));
                    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| x.good.clone().unwrap_or_default());
                    let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(150.0));
                    if edit.changed() {
                        ui.data_mut(|d| d.insert_temp(id, text.clone()));
                    }
                    if edit.lost_focus() {
                        ui.data_mut(|d| d.remove::<String>(id));
                        let good = Some(text.trim().to_string()).filter(|t| !t.is_empty());
                        self.edit("pile good", 0, move |o| o.piles[i].good = good);
                    }
                    let goods = self.spec.goods.clone();
                    egui::ComboBox::from_id_salt(("goods", i)).width(24.0).selected_text("").show_ui(ui, |ui| {
                        for name in &goods {
                            if ui.selectable_label(x.good.as_deref() == Some(name), name).clicked() {
                                let good = Some(name.clone());
                                self.edit("pile good", 0, move |o| o.piles[i].good = good);
                            }
                        }
                    });
                });
                self.cell_row(ui, "pixel offset", x.px, p, "pile", move |o, v| o.piles[i].px = v);
                widgets::small(ui, "xPixelOffset / yPixelOffset from buildingInfo.xml; they do not match the cell and the renderer does not use them yet, so their meaning is unverified.", p);
            }
            LayerId::SoldiersPx => {
                let q = b.soldiers_px[i];
                self.cell_row(ui, "pixel", [q[0], q[1]], p, "soldier slot", move |o, v| {
                    o.soldiers_px[i][0] = v[0];
                    o.soldiers_px[i][1] = v[1];
                });
                ui.horizontal(|ui| {
                    widgets::caption(ui, "facing", p);
                    if let Some((v, m)) = self.num(ui, q[2] as i64, Some((0, 5))) {
                        self.edit("soldier slot", m, move |o| o.soldiers_px[i][2] = (v as i32).rem_euclid(6));
                    }
                });
                let mut top = q[3] != 0;
                if ui.checkbox(&mut top, "on top (bowman); off = swordsman").changed() {
                    self.edit("soldier slot", 0, move |o| o.soldiers_px[i][3] = top as i32);
                }
            }
            LayerId::MiniFlagPx => {
                if let Some(q) = b.mini_flag_px {
                    self.cell_row(ui, "pixel", q, p, "mini flag", |o, v| o.mini_flag_px = Some(v));
                }
            }
            _ => {}
        }
        let required = matches!(sel.layer, LayerId::Door | LayerId::Flag);
        if ui.add_enabled(!required, egui::Button::new(RichText::new(format!("{}  Delete", icon::TRASH)).color(if required { p.faint } else { p.err }))).clicked() {
            self.defer(|st| {
                if let Some(Err(m)) = st.ed.as_mut().map(Editor::delete_sel) {
                    st.toast(m);
                }
            });
        }
    }

    fn edit(&mut self, label: &str, merge: u64, f: impl FnOnce(&mut Building)) {
        if let Some(ed) = self.ed.as_mut() {
            ed.edit(label, (merge != 0).then_some(merge), f);
        }
    }

    fn cell_row(&mut self, ui: &mut Ui, caption: &str, v: [i32; 2], p: &Palette, label: &str, set: impl Fn(&mut Building, [i32; 2])) {
        ui.horizontal(|ui| {
            widgets::caption(ui, caption, p);
            if let Some((x, m)) = self.num(ui, v[0] as i64, None) {
                self.edit(label, m, |o| set(o, [x as i32, v[1]]));
            }
            if let Some((y, m)) = self.num(ui, v[1] as i64, None) {
                self.edit(label, m, |o| set(o, [v[0], y as i32]));
            }
        });
    }

    fn inspect_image(&mut self, ui: &mut Ui, g: &Group, slot: &Slot, p: &Palette) {
        let pack = self.pack.clone();
        let layers = self.layers.clone();
        let s = Scope { g, slot, pack: &pack, layers: &layers };
        let Some(ed) = self.ed.as_ref() else { return };
        let j = ed.frame;
        let before = ed.anchors_now(s);
        let a = before.get(j).copied().unwrap_or([0.0, 0.0]);
        let this_frame = ed.this_frame;
        ui.horizontal(|ui| {
            widgets::caption(ui, &format!("anchor of frame {}", j + 1), p);
            let x = self.num(ui, a[0].round() as i64, None);
            let y = self.num(ui, a[1].round() as i64, None);
            let change = x.map(|(v, m)| ([v as f64 - a[0], 0.0], m)).or(y.map(|(v, m)| ([0.0, v as f64 - a[1]], m)));
            if let (Some((d, m)), Some(ed)) = (change, self.ed.as_mut()) {
                let after: Vec<[f64; 2]> = before.iter().enumerate().map(|(i, q)| if this_frame && i != j { *q } else { [q[0] + d[0], q[1] + d[1]] }).collect();
                ed.commit_anchors_merged(s, &before, Some(after), "anchor", Some(m));
            }
        });
        if let Some(ed) = self.ed.as_mut() {
            ui.checkbox(&mut ed.this_frame, "change this frame only");
            if ui.button("Use original anchors").clicked() {
                ed.commit_anchors(s, &before, None, "original anchors");
            }
        }
        widgets::small(ui, "Pixels in your frame (pack scale). Pause playback to work on one frame.", p);
    }

    pub(super) fn gameplay_panel(&mut self, ui: &mut Ui, g: &Group, slot: &Slot, p: &Palette) {
        let Some(ed) = self.ed.as_ref() else { return };
        let keys = ed.changed_keys(slot);
        let bad = self.pack.saved_building(g, slot).is_some_and(|(_, bad)| bad);
        let cur = ed.building().cloned();
        ui.horizontal(|ui| {
            widgets::heading(ui, "Gameplay data");
            if !keys.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Reset all").color(p.err)).small()).on_hover_text("Back to the original game data (written when you press Save)").clicked()
                        && let Some(ed) = self.ed.as_mut()
                    {
                        ed.reset_all(slot);
                    }
                });
            }
        });
        if bad {
            ui.label(RichText::new("building.json is not valid.").color(p.err));
        }
        if keys.is_empty() {
            widgets::small(ui, "Same as the original game data.", p);
            return;
        }
        ui.label(RichText::new("Changed from the original (written to the pack's building.json on Save):").size(12.0));
        for k in BLD_KEYS.iter().filter(|k| keys.contains(k)) {
            let label = layer_of_key(k).map_or(*k, |d| d.label);
            let detail = match (cur.as_ref(), slot.building.as_ref()) {
                (Some(c), Some(o)) if matches!(*k, "footprint" | "block" | "dig") => {
                    let (a, b) = match *k {
                        "footprint" => (&o.footprint, &c.footprint),
                        "block" => (&o.block, &c.block),
                        _ => (&o.dig, &c.dig),
                    };
                    format!(" (+{} / −{} cells)", b.iter().filter(|x| !a.contains(x)).count(), a.iter().filter(|x| !b.contains(x)).count())
                }
                _ => String::new(),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{label}{detail}")).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Reset").clicked()
                        && let Some(ed) = self.ed.as_mut()
                    {
                        ed.reset_key(slot, k, label);
                    }
                });
            });
        }
        widgets::small(ui, "This changes how the building plays (placement, paths, where goods lie). The game does not read pack building data yet.", p);
    }
}
