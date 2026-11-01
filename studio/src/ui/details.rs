//! Right-hand panel: the slot's data, and in the editor the layers, inspector and gameplay data.

use egui::{Color32, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::{Studio, Tab, widgets};
use crate::editor::changed_keys;
use crate::geometry::is_cell;
use crate::layers::{default_layers, layer, layer_of_key, move_layer};
use crate::model::{Group, Slot, TICK_MS};
use crate::theme::{self, Palette, from_rgb, icon};

pub fn short_good(s: Option<&str>) -> String {
    s.unwrap_or("?").strip_prefix("GOOD_").unwrap_or(s.unwrap_or("?")).to_lowercase()
}

fn row(ui: &mut Ui, k: &str, v: &str, p: &Palette) {
    ui.add_space(5.0);
    widgets::small(ui, k, p);
    ui.add(egui::Label::new(RichText::new(v).size(12.5)).wrap());
}

impl Studio {
    pub(super) fn right_panel(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(r) = self.sel else { return };
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let panel = egui::Panel::right("details").resizable(true).default_size(self.settings.inspector).size_range(260.0..=520.0).frame(egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.line))).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if self.tab == Tab::Edit && self.ed.is_some() {
                        self.layers_panel(ui, g, slot, p);
                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(8.0);
                        self.inspector(ui, r, p);
                        if slot.building.is_some() {
                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(8.0);
                            self.gameplay_panel(ui, g, slot, p);
                        }
                    } else {
                        widgets::heading(ui, "Details");
                        self.info_rows(ui, g, slot, p);
                        if self.ed.is_some() {
                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(8.0);
                            self.layers_panel(ui, g, slot, p);
                        }
                    }
                });
            });
        });
        let w = panel.response.rect.width();
        if (w - self.settings.inspector).abs() > 1.0 {
            self.settings.inspector = w;
            self.save_settings_later();
        }
    }

    fn info_rows(&mut self, ui: &mut Ui, g: &Group, slot: &Slot, p: &Palette) {
        row(ui, "Slot", &format!("{}/{}", g.id, slot.id), p);
        if let Some(o) = &slot.owner {
            row(ui, "Owner", o, p);
        }
        if let Some(job) = slot.job {
            row(ui, "Gfx job", &format!("{job} (file {})", g.file.map_or("?".to_string(), |f| f.to_string())), p);
        }
        if let Some(s) = &slot.sound {
            // Events stay on the original frame; with k frames per original frame that is your frame f·k.
            let mut ks: Vec<u32> = slot.seqs.iter().map(|q| self.mult_of(g, slot, q)).collect();
            ks.sort_unstable();
            ks.dedup();
            let yours = match ks.as_slice() {
                [k] if *k > 1 => format!(", your frame {} at {k}×", s.frame * *k as i32),
                [_] => String::new(),
                _ => " (your frame per direction below)".to_string(),
            };
            row(ui, "Sound", &format!("{} on frame {}{yours} ({} %)", s.name, s.frame, s.chance), p);
        }
        if !slot.job_lists.is_empty() {
            row(ui, "Job lists", &slot.job_lists.join(", "), p);
        }
        if !slot.length_from_frames.is_empty() {
            row(ui, "Length = frames in", &slot.length_from_frames.join(", "), p);
        }
        if let Some(v) = slot.volume {
            row(ui, "Volume", &format!("{v} (Config/SoundFX.cfg; 100 = default)"), p);
        }
        if !slot.uses.is_empty() {
            let mut lines: Vec<String> = slot.uses.iter().take(40).cloned().collect();
            if slot.uses.len() > 40 {
                lines.push(format!("… {} more", slot.uses.len() - 40));
            }
            row(ui, "Used by", &lines.join("\n"), p);
        }
        if let Some(n) = &slot.note {
            row(ui, "Note", n, p);
        }
        if let Some(b) = self.building(g, slot) {
            let kinds: Vec<&str> = [b.kind.as_deref(), b.inhabitant.as_deref()].into_iter().flatten().collect();
            row(ui, "Building", &kinds.join(" · "), p);
            let work = b.work.filter(|c| is_cell(*c)).map_or("none".to_string(), |c| format!("{},{}", c[0], c[1]));
            row(ui, "Footprint", &format!("{} nodes, door {},{}, flag {},{}, work spot {work}", b.footprint.len(), b.door[0], b.door[1], b.flag[0], b.flag[1]), p);
            if !b.piles.is_empty() {
                let piles: Vec<String> = b.piles.iter().map(|x| format!("{} at {},{}", short_good(x.good.as_deref()), x.cell[0], x.cell[1])).collect();
                row(ui, "Piles", &piles.join(" · "), p);
            }
            if !b.work_lists.is_empty() {
                let lists: Vec<String> = b.work_lists.iter().map(|w| format!("{} ({} steps, {} ticks = {:.1} s)", w.name, w.steps.len(), w.ticks, w.ticks as f64 * TICK_MS / 1000.0)).collect();
                row(ui, "Worker routine", &lists.join(" · "), p);
            }
            let changed = changed_keys(Some(&b), slot);
            if !changed.is_empty() {
                let names: Vec<&str> = changed.iter().filter_map(|k| layer_of_key(k)).map(|d| d.label).collect();
                ui.add_space(5.0);
                widgets::small(ui, "Gameplay data", p);
                ui.label(RichText::new(format!("changed in the pack: {}", names.join(", "))).color(p.warn).size(12.5));
            }
        }
    }

    fn layers_panel(&mut self, ui: &mut Ui, g: &Group, slot: &Slot, p: &Palette) {
        let has_b = self.building(g, slot).is_some();
        ui.horizontal(|ui| {
            widgets::heading(ui, "Layers");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Reset").on_hover_text("Default order and visibility").clicked() {
                    self.layers = default_layers();
                    self.save_settings_later();
                }
            });
        });
        widgets::small(ui, "Top of the list is drawn on top. The sequence cards use the same layers.", p);
        ui.add_space(4.0);
        let mut rects: Vec<(usize, egui::Rect)> = Vec::new();
        let n = self.layers.len();
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let released = ui.input(|i| i.pointer.primary_released());
        ui.spacing_mut().item_spacing.y = 0.0;
        for idx in 0..n {
            let l = self.layers[idx];
            let def = layer(l.id);
            if def.bld && !has_b {
                continue;
            }
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
            rects.push((idx, rect));
            let hovered = ui.rect_contains_pointer(rect);
            if self.drag_layer == Some(idx) {
                ui.painter().rect_filled(rect, 5, p.accent_soft);
            } else if hovered {
                ui.painter().rect_filled(rect, 5, p.panel2);
            }
            let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(2.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
            row.spacing_mut().item_spacing.x = 5.0;
            let grip = row.add(egui::Label::new(RichText::new(icon::DOTS_SIX_VERTICAL).color(p.faint).size(13.0)).sense(Sense::drag()));
            if grip.drag_started() {
                self.drag_layer = Some(idx);
            }
            grip.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to reorder");
            let eye = row.add(egui::Label::new(RichText::new(if l.vis { icon::EYE } else { icon::EYE_SLASH }).size(15.0).color(if l.vis { p.text } else { p.faint })).sense(Sense::click())).on_hover_text("Show / hide");
            if eye.clicked() {
                self.layers[idx].vis = !l.vis;
                self.save_settings_later();
            }
            let (sw, _) = row.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            row.painter().rect_filled(sw, 3, from_rgb(def.color));
            row.painter().rect_stroke(sw, 3, Stroke::new(1.0, p.line), egui::StrokeKind::Outside);
            row.label(RichText::new(def.label).size(12.5).color(if l.vis { p.text } else { p.dim }));
            if hovered {
                row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |row| {
                    if row.add(egui::Button::new(RichText::new(icon::ARROW_DOWN).size(11.0)).frame_when_inactive(false).min_size(vec2(20.0, 20.0))).on_hover_text("Move down").clicked() && idx + 1 < n {
                        move_layer(&mut self.layers, idx, idx + 1);
                        self.save_settings_later();
                    }
                    if row.add(egui::Button::new(RichText::new(icon::ARROW_UP).size(11.0)).frame_when_inactive(false).min_size(vec2(20.0, 20.0))).on_hover_text("Move up").clicked() && idx > 0 {
                        move_layer(&mut self.layers, idx, idx - 1);
                        self.save_settings_later();
                    }
                });
            }
        }
        if let (Some(from), Some(pos)) = (self.drag_layer, pointer) {
            let target = rects.iter().find(|(_, r)| pos.y < r.bottom()).or(rects.last()).copied();
            if let Some((to, r)) = target {
                let y = if to > from { r.bottom() } else { r.top() };
                ui.painter().hline(r.x_range(), y, Stroke::new(2.0, p.accent));
                if released {
                    move_layer(&mut self.layers, from, to);
                    self.save_settings_later();
                }
            }
        }
        if released {
            self.drag_layer = None;
        }
        ui.spacing_mut().item_spacing.y = 6.0;
    }

    /// A panel title with a swatch.
    pub(super) fn sel_title(ui: &mut Ui, text: &str, color: Color32) {
        ui.horizontal(|ui| {
            let (sw, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(sw, 3, color);
            ui.label(RichText::new(text).font(theme::semibold(13.0)));
        });
    }
}

/// A row of coloured dots with names.
pub fn legend(ui: &mut Ui, items: &[(Color32, &str)], p: &Palette) {
    ui.horizontal_wrapped(|ui| {
        for (c, label) in items {
            let (r, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
            ui.painter().circle_filled(pos2(r.center().x, r.center().y), 4.0, *c);
            widgets::small(ui, *label, p);
        }
    });
}
