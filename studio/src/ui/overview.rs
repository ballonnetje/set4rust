//! The start page: every group's progress, pack layout, team colour rules, keys.

use egui::{RichText, Ui};
use serde_json::Value;

use super::{Studio, editor::KEYS_HELP, widgets};
use crate::model::Kind;
use crate::status::thousands;
use crate::theme::Palette;

impl Studio {
    pub(super) fn overview(&mut self, ui: &mut Ui, p: &Palette) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(28, 22)).show(ui, |ui| {
                ui.set_max_width(1040.0);
                ui.label(RichText::new("Overview").heading());
                widgets::caption(ui, &format!("Spec from {}. Pack scale {}×: replacement pixels = original pixels × {}.", self.spec.game_dir_label, self.pack.scale(), self.pack.scale()), p);
                ui.add_space(8.0);
                egui::Frame::new().fill(p.panel).stroke(egui::Stroke::new(1.0, p.line)).corner_radius(8).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("The reference images and sounds come from the original game. They are for your eyes only: never copy them into the pack, publish or ship them.").color(p.warn));
                });
                ui.add_space(14.0);
                widgets::card_frame(p).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::Grid::new("groups").num_columns(6).spacing([22.0, 7.0]).show(ui, |ui| {
                        for h in ["Group", "Slots done", "Replaced", "", "Problems", "Frame counts"] {
                            widgets::caption(ui, h, p);
                        }
                        ui.end_row();
                        let spec = self.spec.clone();
                        for cat in &spec.categories {
                            ui.label(RichText::new(&cat.label).font(crate::theme::semibold(13.0)));
                            ui.end_row();
                            for id in &cat.groups {
                                let Some(gi) = spec.group_index(id) else { continue };
                                let g = &spec.groups[gi];
                                let s = self.status.groups[gi];
                                if ui.link(&g.label).clicked() {
                                    let (cat_id, gid) = (cat.id.clone(), id.clone());
                                    self.defer(move |st| {
                                        st.open.insert(cat_id);
                                        st.open.insert(gid);
                                        st.rows_dirty = true;
                                    });
                                }
                                ui.label(format!("{} / {}", s.done, s.slots));
                                ui.label(format!("{} / {} {}", thousands(s.have), thousands(s.need), g.unit()));
                                widgets::bar(ui, s.have, s.need, 90.0, p);
                                ui.label(if s.problem > 0 { s.problem.to_string() } else { String::new() });
                                match &g.keep_frames {
                                    Some(why) => {
                                        widgets::caption(ui, "keep (mechanics)", p).on_hover_text(why);
                                    }
                                    None => {
                                        widgets::caption(ui, if g.kind == Kind::Gfx { "free" } else { "" }, p);
                                    }
                                }
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.add_space(18.0);
                widgets::heading(ui, "Pack layout");
                egui::Grid::new("layout").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    for (k, v) in &self.spec.pack_layout {
                        widgets::caption(ui, k, p);
                        ui.add(egui::Label::new(RichText::new(v.as_str().map_or_else(|| Value::to_string(v), str::to_string)).monospace().size(11.5)).wrap());
                        ui.end_row();
                    }
                });
                ui.add_space(14.0);
                widgets::heading(ui, "Team colour");
                ui.label("Paint team-coloured areas in shades of grey (white = full player colour) and give each such frame an NNN.team.png mask (white where team colour applies). The preview, and the game later, compute colour = mix(rgb, player × luma(rgb), mask).");
                ui.add_space(14.0);
                widgets::heading(ui, "Not covered by this spec yet");
                for s in &self.spec.not_covered {
                    ui.label(format!("•  {s}"));
                }
                ui.add_space(14.0);
                widgets::heading(ui, "Keys");
                widgets::caption(ui, "Space: play / pause · ← →: step frame (nothing selected) · J / K: next / previous slot · R: reload pack · /: search", p);
                widgets::caption(ui, &format!("Editor: {KEYS_HELP}"), p);
            });
        });
    }
}
