//! The slot page: header, playback bar, per-sequence comparison cards, audio cards.

use std::path::Path;
use std::time::Duration;

use egui::{Color32, FontId, RichText, Sense, Stroke, Ui, pos2, vec2};

use super::stage::{Overlay, Stage, data_layer, paint_checker};
use super::{AUDIO_EXT, IMAGE_EXT, SUB, Studio, Tab, ZOOMS, frames_at, widgets};
use crate::audio::clock;
use crate::geometry::{Bounds, card_zoom, seq_box};
use crate::images::Yours;
use crate::layers::{LayerId, layer};
use crate::model::{Building, F_TEAM, GAME_FPS, Group, Kind, Seq, Slot, SlotRef, half_k};
use crate::pack::{MULTS, animated, expect_size, frame_anchor, sound_path};
use crate::status::{Level, Problem, SeqStatus, plural, seq_status};
use crate::theme::{self, Palette, from_rgb, icon};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Original,
    Yours,
}

/// The original frame's strip on `st`.
fn draw_orig(this: &mut Studio, st: &Stage, g: &Group, seq: &Seq, i: usize, alpha: f32) {
    let spec = this.spec.clone();
    if let Some(tex) = this.images.strip(&spec, g, seq, this.team) {
        st.strip_frame(tex, &seq.frames[i], None, alpha);
    }
}

impl Studio {
    pub(super) fn slot_page(&mut self, ui: &mut Ui, r: SlotRef, p: &Palette) {
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 16, top: 12, bottom: 10 }).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&slot.name).heading());
                ui.add_space(6.0);
                self.badges(ui, g, slot, r, p);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.ed.is_some() {
                        let items = [(Tab::Edit, format!("{}  Edit", icon::PENCIL_SIMPLE)), (Tab::Compare, format!("{}  Compare", icon::COLUMNS))];
                        let items: Vec<(Tab, &str)> = items.iter().map(|(t, s)| (*t, s.as_str())).collect();
                        widgets::segmented(ui, &mut self.tab, &items, p);
                    }
                });
            });
        });
        if g.kind == Kind::Gfx {
            self.playback_bar(ui, p);
        }
        ui.add_space(1.0);
        match (self.tab, g.kind) {
            (Tab::Edit, Kind::Gfx) if self.ed.is_some() => self.editor_page(ui, r, p),
            (_, Kind::Gfx) => self.cards(ui, r, g, slot, p),
            _ => self.audio_cards(ui, r, g, slot, p),
        }
    }

    fn badges(&self, ui: &mut Ui, g: &Group, slot: &Slot, r: SlotRef, p: &Palette) {
        let st = self.status.slot(r);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            widgets::chip(ui, &g.label, None, p);
            widgets::chip(ui, &format!("{} / {} {}", st.have, st.need, g.unit()), None, p);
            if let Some(why) = &g.keep_frames {
                widgets::chip(ui, "keep frame counts", Some(p.warn), p).on_hover_text(why);
            }
            if slot.seqs.iter().any(|q| q.mask.is_some()) {
                widgets::chip(ui, "team colour", Some(p.accent), p);
            }
            if slot.exact() {
                widgets::chip(ui, "exact size", Some(p.err), p);
            }
        });
    }

    fn playback_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let now = self.now;
        egui::Frame::new().fill(p.panel).stroke(Stroke::new(1.0, p.line)).inner_margin(egui::Margin::symmetric(16, 7)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let label = if self.play.playing { format!("{}  Pause", icon::PAUSE) } else { format!("{}  Play", icon::PLAY) };
                if ui.add(egui::Button::new(label).min_size(vec2(82.0, 0.0))).clicked() {
                    self.play.set_playing(!self.play.playing, now);
                }
                if widgets::icon_button(ui, icon::SKIP_BACK, "One frame back (←)", p).clicked() {
                    self.play.step(-1.0, self.kmax, now);
                }
                if widgets::icon_button(ui, icon::SKIP_FORWARD, "One frame on (→)", p).clicked() {
                    self.play.step(1.0, self.kmax, now);
                }
                ui.add_space(6.0);
                widgets::caption(ui, "speed", p).on_hover_text("Original frames per second. Yours play k times as many (k = frames per original frame).");
                let mut fps = self.play.fps;
                if widgets::slider(ui, &mut fps, 1.0..=30.0).changed() {
                    self.play.set_fps(fps, now);
                }
                ui.add_sized([96.0, 20.0], egui::Label::new(RichText::new(self.play.label()).color(p.dim).size(12.0)));
                if ui.button(format!("{}  Game speed", icon::GAUGE)).on_hover_text(format!("One original frame per 71 ms game tick ({GAME_FPS:.1} fps)")).clicked() {
                    self.play.set_fps(GAME_FPS, now);
                }
                ui.add_space(6.0);
                widgets::caption(ui, "zoom", p);
                egui::ComboBox::from_id_salt("zoom").width(56.0).selected_text(format!("{}×", self.zoom)).show_ui(ui, |ui| {
                    for z in ZOOMS {
                        if ui.selectable_label(self.zoom == z, format!("{z}×")).clicked() {
                            self.zoom = z;
                            self.save_settings_later();
                        }
                    }
                });
                widgets::caption(ui, "team", p);
                let colors = self.spec.player_colors.clone();
                egui::ComboBox::from_id_salt("team").width(104.0).selected_text(format!("player {}", self.team + 1)).show_ui(ui, |ui| {
                    for (i, c) in colors.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let (sw, _) = ui.allocate_exact_size(vec2(11.0, 11.0), Sense::hover());
                            ui.painter().rect_filled(sw, 3, from_rgb(*c));
                            if ui.selectable_label(self.team == i, format!("player {}", i + 1)).clicked() {
                                self.team = i;
                                self.save_settings_later();
                            }
                        });
                    }
                });
                let (sw, _) = ui.allocate_exact_size(vec2(11.0, 11.0), Sense::hover());
                ui.painter().rect_filled(sw, 3, from_rgb(colors[self.team]));
                ui.checkbox(&mut self.onion, "onion").on_hover_text("Cards: show the original under your image");
            });
        });
    }

    fn view_rect(&self, ui: &mut Ui, w: f32, h: f32, p: &Palette) -> (egui::Rect, egui::Response) {
        let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
        if ui.is_rect_visible(rect)
            && let Some(c) = &self.checker
        {
            paint_checker(ui.painter(), c, rect, 4.0);
            ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, p.line), egui::StrokeKind::Outside);
        }
        (rect, resp)
    }

    /// A view: the layers bottom-up, skipping the other side's image (the onion option draws the
    /// original faintly under yours).
    #[allow(clippy::too_many_arguments)]
    fn compose(&mut self, st: &Stage, side: Side, g: &Group, slot: &Slot, seq: &Seq, b: Option<&Building>, i: usize, j: usize, hot: bool) -> bool {
        let no_anchor = !slot.has_map_anchor();
        let (pack, spec) = (self.pack.clone(), self.spec.clone());
        let route = if seq.key == "1" { b.and_then(|b| b.work_lists.first()) } else { None };
        let mut any = false;
        for l in self.layers.clone().iter().rev() {
            if !l.vis || l.id == LayerId::Worker {
                continue;
            }
            match l.id {
                LayerId::Original => match side {
                    Side::Original => draw_orig(self, st, g, seq, i, 1.0),
                    Side::Yours if self.onion => draw_orig(self, st, g, seq, i, 0.35),
                    Side::Yours => {}
                },
                LayerId::Yours => {
                    if side == Side::Yours {
                        match self.images.frame(&spec, &pack, g, slot, seq, j, self.team, hot) {
                            Yours::Ready(tex) => {
                                let a = frame_anchor(&pack, slot, seq, self.conf_of(g, slot, seq), j);
                                st.pack_frame(tex, a, pack.scale(), 1.0);
                                any = true;
                            }
                            Yours::Loading => any = true,
                            Yours::None => {}
                        }
                    }
                }
                id => {
                    if !no_anchor && !(layer(id).bld && b.is_none()) {
                        data_layer(st, id, b, &Overlay { sel: None, route, state: None });
                    }
                }
            }
        }
        any
    }

    fn cards(&mut self, ui: &mut Ui, r: SlotRef, g: &Group, slot: &Slot, p: &Palette) {
        let b = self.building(g, slot);
        self.kmax = slot.seqs.iter().map(|q| self.mult_of(g, slot, q)).max().unwrap_or(1);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 22, right: 22, top: 14, bottom: 40 }).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                for si in 0..slot.seqs.len() {
                    self.gfx_card(ui, r, g, slot, si, b.as_ref(), p);
                }
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn gfx_card(&mut self, ui: &mut Ui, r: SlotRef, g: &Group, slot: &Slot, si: usize, b: Option<&Building>, p: &Palette) {
        let seq = &slot.seqs[si];
        let n = seq.frames.len();
        let k = self.mult_of(g, slot, seq);
        let nk = n * k as usize;
        let box_: Bounds = seq_box(seq, b);
        // Both views side by side: the zoom gives way to the room there is.
        let room = ((ui.available_width() - 40.0) / 2.0) as f64;
        let z = card_zoom((self.zoom as f64).min(room / box_.w), &box_) as f32;
        let (w, h) = ((box_.w as f32 * z).ceil(), (box_.h as f32 * z).ceil());
        let conf = self.conf_of(g, slot, seq).clone();
        let st = seq_status(&self.pack, g, slot, seq, &conf);
        let pos = self.play.cur(self.now);
        let (j, i) = frames_at(pos, n, k);
        let frame = widgets::card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            self.card_header(ui, slot, seq, k, nk, &st, i, j, p);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                ui.vertical(|ui| {
                    widgets::small(ui, "Original (reference only)", p);
                    let (rect, _) = self.view_rect(ui, w, h, p);
                    if ui.is_rect_visible(rect) {
                        let stage = Stage { painter: ui.painter_at(rect), rect, z, ox: rect.left() - box_.x0 as f32 * z, oy: rect.top() - box_.y0 as f32 * z };
                        self.compose(&stage, Side::Original, g, slot, seq, b, i, j, true);
                    }
                });
                ui.vertical(|ui| {
                    widgets::small(ui, format!("Yours (pack, {}× pixels shown at game size)", self.pack.scale()), p);
                    let (rect, _) = self.view_rect(ui, w, h, p);
                    if ui.is_rect_visible(rect) {
                        let stage = Stage { painter: ui.painter_at(rect), rect, z, ox: rect.left() - box_.x0 as f32 * z, oy: rect.top() - box_.y0 as f32 * z };
                        self.compose(&stage, Side::Yours, g, slot, seq, b, i, j, true);
                        if st.have == 0 {
                            let text = if rect.width() < 300.0 { "Drop images here" } else { "Drop images here\n(frames in name order; *.team.png = masks)" };
                            let hint = ui.painter().layout(text.to_string(), FontId::proportional(12.0), Color32::from_gray(214), rect.width() - 24.0);
                            let at = rect.center() - hint.size() / 2.0;
                            ui.painter().rect_filled(egui::Rect::from_min_size(at, hint.size()).expand2(vec2(10.0, 6.0)), 6, Color32::from_black_alpha(170));
                            ui.painter().galley(at, hint, Color32::WHITE);
                        }
                        // Preload the rest behind what is on show.
                        let (spec, pack) = (self.spec.clone(), self.pack.clone());
                        for x in 0..nk {
                            self.images.frame(&spec, &pack, g, slot, seq, x, self.team, false);
                        }
                    }
                });
            });
            self.problems(ui, &st.probs, p);
            ui.add_space(2.0);
            self.card_actions(ui, r, slot, si, &conf, p);
        });
        if frame.response.hovered() || ui.rect_contains_pointer(frame.response.rect) {
            self.active_seq = si;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card_header(&self, ui: &mut Ui, slot: &Slot, seq: &Seq, k: u32, nk: usize, st: &SeqStatus, i: usize, j: usize, p: &Palette) {
        let n = seq.frames.len();
        // Events keep their original frame; yours is frame f·k.
        let ev = |f: i32| if k > 1 { format!("frame {f} (yours {})", f * k as i32) } else { format!("frame {f}") };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&seq.label).font(theme::semibold(13.5)));
            if let Some(pa) = &seq.patch {
                let text = |key: &str| pa.get(key).map_or(String::new(), |v| v.as_str().map_or_else(|| v.to_string(), str::to_string));
                let sound = seq.patch_sound().map_or(String::new(), |s| format!(" · {} on {}", s.name, ev(s.frame)));
                widgets::chip(ui, &format!("patch {} · slot {} · {} ticks{sound}", text("type"), text("slot"), text("ticks")), None, p);
            }
            if let (Some(s), true) = (&slot.sound, k > 1) {
                widgets::chip(ui, &format!("{} on {}", s.name, ev(s.frame)), None, p);
            }
            if let Some(name) = seq.part_name.as_deref().filter(|s| !s.is_empty()) {
                widgets::small(ui, name, p);
            }
            let count = if k > 1 { format!("{} · yours {nk} ({k}×)", plural(n, "original frame")) } else { plural(n, "frame") };
            widgets::small(ui, format!("{count} · {} replaced", st.have), p);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = format!("frame {} / {n}{}{}", i + 1, if k > 1 { format!(" · yours {} / {nk}", j + 1) } else { String::new() }, if st.have == 0 { " · not replaced" } else { "" });
                widgets::small(ui, label, p);
            });
        });
    }

    fn problems(&self, ui: &mut Ui, probs: &[Problem], p: &Palette) {
        for x in probs {
            ui.horizontal_top(|ui| {
                let (color, ic) = match x.level {
                    Level::Error => (p.err, icon::WARNING_CIRCLE),
                    Level::Warn => (p.warn, icon::WARNING),
                    Level::Info => (p.info, icon::INFO),
                };
                ui.label(RichText::new(ic).color(color).size(14.0));
                ui.add(egui::Label::new(RichText::new(&x.msg).size(12.5)).wrap());
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card_actions(&mut self, ui: &mut Ui, r: SlotRef, slot: &Slot, si: usize, conf: &crate::pack::Conf, p: &Palette) {
        let seq = &slot.seqs[si];
        let n = seq.frames.len();
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("{}  Add frames…", icon::FILE_PLUS)).clicked() {
                self.defer(move |st| st.pick_files("Add frames", &IMAGE_EXT, true, move |st, files| st.save_frames(r, si, files)));
            }
            if animated(slot, seq) {
                let own = conf.multiplier().unwrap_or(0);
                let pack_k = self.pack.mult();
                let text = if own == 0 { format!("pack default ({pack_k}×)") } else { format!("{own}× → {} frames", n * own as usize) };
                widgets::caption(ui, "Frames per original frame", p).on_hover_text("Frames you draw per original frame. The game keeps the original count for timing and shows your extra frames in between ticks.");
                egui::ComboBox::from_id_salt(("mult", si)).width(150.0).selected_text(text).show_ui(ui, |ui| {
                    if ui.selectable_label(own == 0, format!("pack default ({pack_k}×)")).clicked() {
                        self.defer(move |st| st.save_conf(r, si, |c| { c.remove("frame_multiplier"); }));
                    }
                    let extra = (!MULTS.contains(&own) && own != 0).then_some(own);
                    for v in MULTS.into_iter().chain(extra) {
                        if ui.selectable_label(own == v, format!("{v}× → {} frames", n * v as usize)).clicked() {
                            self.defer(move |st| st.save_conf(r, si, move |c| { c.insert("frame_multiplier".into(), v.into()); }));
                        }
                    }
                });
            }
            if conf.has_anchor() && ui.button("Use original anchors").on_hover_text("Anchor set in the editor; this drops it").clicked() {
                self.defer(move |st| {
                    st.save_conf(r, si, |c| {
                        c.remove("anchor");
                        c.remove("anchors");
                    });
                });
            }
            if ui.add(egui::Button::new(RichText::new(format!("{}  Remove my frames", icon::TRASH)).color(p.err))).clicked() {
                self.defer(move |st| st.clear_seq(r, si));
            }
        });
        let [ew, eh] = expect_size(&seq.frames[0], self.pack.scale());
        let team = if seq.frames[0][5] & F_TEAM != 0 { " · team colour" } else { "" };
        let half = if half_k(&seq.frames[0]) < 1.0 { " · half size" } else { "" };
        widgets::small(ui, format!("expected size frame 0: {ew}×{eh}, original anchor {},{}{team}{half}", seq.frames[0][3], seq.frames[0][4]), p);
    }

    fn audio_cards(&mut self, ui: &mut Ui, r: SlotRef, g: &Group, slot: &Slot, p: &Palette) {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 22, right: 22, top: 14, bottom: 40 }).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon::SPEAKER_HIGH).color(p.dim).size(15.0));
                    let mut v = self.volume;
                    if widgets::slider(ui, &mut v, 0.0..=1.0).on_hover_text("Volume").changed() {
                        self.volume = v;
                        self.audio.set_volume(v);
                        self.save_settings_later();
                    }
                });
                for si in 0..slot.seqs.len() {
                    self.audio_card(ui, r, g, slot, si, p);
                }
            });
        });
    }

    fn audio_card(&mut self, ui: &mut Ui, r: SlotRef, g: &Group, slot: &Slot, si: usize, p: &Palette) {
        let seq = &slot.seqs[si];
        let st = seq_status(&self.pack, g, slot, seq, self.pack.conf(g, slot, seq));
        let mine = if g.kind == Kind::Music {
            self.pack.music_file(slot)
        } else {
            Some(sound_path(slot, seq)).filter(|f| self.pack.has(f))
        };
        let frame = widgets::card_frame(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&seq.label).font(theme::semibold(13.5)));
                if let Some(s) = seq.secs {
                    widgets::small(ui, format!("{s:.2} s · {} Hz · {}", seq.rate.unwrap_or(0), if seq.channels == Some(2) { "stereo" } else { "mono" }), p);
                }
                if seq.missing {
                    widgets::small(ui, "empty in the original (silent) — nothing to replace", p);
                }
            });
            if seq.missing {
                return;
            }
            let original = seq.reference.as_deref().map(|x| self.spec.ref_path(x));
            ui.columns(2, |c| {
                widgets::small(&mut c[0], "Original (reference only)", p);
                if let Some(path) = &original {
                    self.player(&mut c[0], path, seq.secs, p);
                }
                let title = mine.as_ref().map_or("Yours: drop an audio file here".to_string(), |m| format!("Yours: {m}"));
                widgets::small(&mut c[1], title, p);
                match &mine {
                    Some(m) => self.player(&mut c[1], &self.pack.abs(m), None, p),
                    None => {
                        widgets::small(&mut c[1], "—", p);
                    }
                }
            });
            self.problems(ui, &st.probs, p);
            ui.horizontal_wrapped(|ui| {
                if ui.button(format!("{}  Choose file…", icon::FILE_PLUS)).clicked() {
                    let (title, ext): (&str, &[&str]) = if g.kind == Kind::Music { ("Choose music", &crate::pack::MUSIC_EXT) } else { ("Choose a sound", &AUDIO_EXT) };
                    self.defer(move |st| {
                        st.pick_files(title, ext, false, move |st, files| {
                            if let Some(f) = files.into_iter().next() {
                                st.save_audio(r, si, f);
                            }
                        });
                    });
                }
                if let Some(m) = mine.clone()
                    && ui.add(egui::Button::new(RichText::new(format!("{}  Remove mine", icon::TRASH)).color(p.err))).clicked()
                {
                    self.defer(move |st| st.delete_file(r, m));
                }
                widgets::small(ui, if g.kind == Kind::Music { "MP3, OGG or WAV, stored as is." } else { "Any audio file; non-WAV files are converted to 16-bit PCM WAV." }, p);
            });
        });
        if ui.rect_contains_pointer(frame.response.rect) {
            self.active_seq = si;
        }
    }

    /// A small player: play / pause, position bar, time.
    fn player(&mut self, ui: &mut Ui, path: &Path, secs: Option<f64>, p: &Palette) {
        let playing = self.audio.is_playing(path);
        ui.horizontal(|ui| {
            if widgets::icon_button(ui, if playing { icon::PAUSE } else { icon::PLAY }, if playing { "Pause" } else { "Play" }, p).clicked() {
                let path = path.to_path_buf();
                self.defer(move |st| st.play_file(&path));
            }
            let posn = self.audio.position(path);
            let cur = posn.map_or(Duration::ZERO, |x| x.0);
            let total = posn.and_then(|x| x.1).or(secs.map(Duration::from_secs_f64));
            let width = (ui.available_width() - 96.0).max(60.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 18.0), Sense::click_and_drag());
            let track = egui::Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
            ui.painter().rect_filled(track, 2, p.line);
            if let Some(t) = total.filter(|t| !t.is_zero()) {
                let k = (cur.as_secs_f32() / t.as_secs_f32()).clamp(0.0, 1.0);
                let mut done = track;
                done.set_width(track.width() * k);
                ui.painter().rect_filled(done, 2, p.accent);
                ui.painter().circle_filled(pos2(track.left() + track.width() * k, rect.center().y), 5.0, p.accent);
                if (resp.clicked() || resp.dragged())
                    && posn.is_some()
                    && let Some(at) = resp.interact_pointer_pos()
                {
                    let k = ((at.x - track.left()) / track.width()).clamp(0.0, 1.0);
                    if let Err(e) = self.audio.seek(t.mul_f32(k)) {
                        self.toast(format!("Cannot seek: {e}"));
                    }
                }
            }
            let time = match total {
                Some(t) => format!("{} / {}", clock(cur), clock(t)),
                None => clock(cur),
            };
            widgets::small(ui, time, p);
        });
    }
}

/// The sub-frame clock the editor shares.
pub fn tick_of(pos: f64) -> i64 {
    (pos / SUB).floor() as i64
}
