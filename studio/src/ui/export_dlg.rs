//! The export dialog: scope, contents, sequence choice, progress and cancel.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread::JoinHandle;

use egui::{Context, RichText, Ui};

use super::{Studio, widgets};
use crate::export::{self, Job, MAX_REFERENCE_FRAMES, Options, Progress, REFERENCE_WARNING, Scope, Summary};
use crate::model::SlotRef;
use crate::status::{plural, thousands};
use crate::theme::{self, Palette, icon};

struct Running {
    progress: Arc<Progress>,
    rx: Receiver<anyhow::Result<Summary>>,
    thread: Option<JoinHandle<()>>,
}

pub struct ExportDlg {
    scope: Scope,
    group: usize,
    /// The slot open when the dialog opened.
    slot: Option<SlotRef>,
    with_files: usize,
    images: bool,
    sheets: bool,
    data: bool,
    csv: bool,
    sounds: bool,
    empty: bool,
    reference: bool,
    keys: String,
    /// Sequence keys ticked, for one slot.
    ticked: BTreeSet<String>,
    status: String,
    running: Option<Running>,
    /// The save dialog is up.
    choosing: bool,
    pub unsaved: bool,
}

impl ExportDlg {
    fn options(&self, team: usize) -> Options {
        let seq_keys = if self.scope == Scope::Slot && self.slot.is_some() {
            Some(self.ticked.iter().cloned().collect::<HashSet<String>>())
        } else {
            let keys: HashSet<String> = self.keys.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(str::to_string).collect();
            (!keys.is_empty()).then_some(keys)
        };
        Options { images: self.images, sheets: self.sheets, data: self.data, csv: self.csv, sounds: self.sounds, empty: self.empty, reference: self.reference, team, seq_keys }
    }

    /// Stops a running export and waits for its thread.
    pub fn cancel_and_wait(&mut self) {
        if let Some(r) = self.running.as_mut() {
            r.progress.cancel.store(true, Ordering::Relaxed);
            if let Some(t) = r.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// An export runs or the save dialog is up: the dialog's controls are off.
    pub fn busy(&self) -> bool {
        self.running.is_some() || self.choosing
    }

    /// The line under the options.
    #[cfg(test)]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// As if the export thread had died.
    #[cfg(test)]
    pub fn fake_dead_export(&mut self) {
        let (_, rx) = channel();
        self.running = Some(Running { progress: Arc::new(Progress::default()), rx, thread: None });
    }
}

impl Studio {
    pub fn open_export(&mut self) {
        if self.export.is_some() {
            return;
        }
        let slot = self.sel;
        let group = slot.map_or(0, |r| r.g);
        let ticked = slot.map_or(BTreeSet::new(), |r| self.spec.slot(r).1.seqs.iter().map(|q| q.key.clone()).collect());
        self.export = Some(ExportDlg {
            scope: if slot.is_some() { Scope::Slot } else { Scope::Group },
            group,
            slot,
            with_files: self.pack.slots_with_files().len(),
            images: true,
            sheets: true,
            data: true,
            csv: true,
            sounds: true,
            empty: false,
            reference: false,
            keys: String::new(),
            ticked,
            status: String::new(),
            running: None,
            choosing: false,
            unsaved: self.dirty(),
        });
    }

    fn start_export(&mut self) {
        let Some(d) = self.export.as_mut() else { return };
        let opt = d.options(self.team);
        let (scope, group, slot) = (d.scope, d.group, d.slot);
        let list = export::slot_list(&self.spec, &self.pack, scope, group, slot, &opt);
        if opt.reference {
            let n = export::reference_frames(&self.spec, &list, &opt);
            if n > MAX_REFERENCE_FRAMES {
                self.toast(format!("That is {} original frames, more than one zip holds: export a group at a time.", thousands(n)));
                return;
            }
        }
        if list.is_empty() {
            self.toast("Nothing to export: no slot in this scope has files of yours (tick “Also slots without any file of yours” for data only).");
            return;
        }
        let what = match (scope, slot) {
            (Scope::Slot, Some(r)) => self.spec.slot(r).1.id.clone(),
            (Scope::Group, _) => self.spec.groups[group].id.clone(),
            _ => scope.name().to_string(),
        };
        let name = export::file_name(&what);
        let opened = self.pick_save(&name, move |st, paths| match paths.into_iter().next() {
            Some(out) => st.run_export(scope, list, opt, out),
            None => {
                if let Some(d) = st.export.as_mut() {
                    d.choosing = false;
                }
            }
        });
        if opened && let Some(d) = self.export.as_mut() {
            d.choosing = true;
        }
    }

    fn run_export(&mut self, scope: Scope, list: Vec<SlotRef>, opt: Options, out: PathBuf) {
        let Some(d) = self.export.as_mut() else { return };
        d.choosing = false;
        d.status.clear();
        let progress = Arc::new(Progress::default());
        progress.total.store(list.len(), Ordering::Relaxed);
        let (tx, rx) = channel();
        let job = Job { spec: self.spec.clone(), pack: self.pack.clone(), scope, list, opt, out };
        let (p, ctx) = (progress.clone(), self.ctx.clone());
        let thread = std::thread::spawn(move || {
            let _ = tx.send(export::run(&job, &p));
            ctx.request_repaint();
        });
        d.running = Some(Running { progress, rx, thread: Some(thread) });
    }

    fn poll_export(&mut self) {
        let mut done = None;
        if let Some(d) = self.export.as_mut() {
            let finished = match d.running.as_ref().map(|r| r.rx.try_recv()) {
                Some(Ok(res)) => Some(res),
                // The thread died: nothing will come, and Close must still work.
                Some(Err(TryRecvError::Disconnected)) => Some(Err(anyhow::anyhow!("the export stopped unexpectedly"))),
                _ => None,
            };
            if let Some(res) = finished {
                if let Some(t) = d.running.take().and_then(|mut r| r.thread.take()) {
                    let _ = t.join();
                }
                d.status = match &res {
                    Ok(s) => s.label(),
                    Err(e) if export::is_cancelled(e) => "Cancelled.".to_string(),
                    Err(e) => format!("Export failed: {e:#}"),
                };
                done = res.ok();
            }
        }
        if let Some(s) = done {
            self.toast(format!("Export written to {}.", s.path.display()));
        }
    }

    pub(super) fn export_dialog(&mut self, ctx: &Context, p: &Palette) {
        self.poll_export();
        if self.export.is_none() {
            return;
        }
        let mut close = false;
        let mut go = false;
        let resp = egui::Modal::new(egui::Id::unique("export")).show(ctx, |ui| {
            ui.set_width(600.0);
            ui.set_max_height(ctx.content_rect().height() - 90.0);
            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                (close, go) = self.export_body(ui, p);
            });
        });
        if resp.should_close() {
            close = true;
        }
        if go {
            self.defer(Studio::start_export);
        }
        if close {
            let d = self.export.as_mut().expect("checked");
            if let Some(r) = &d.running {
                r.progress.cancel.store(true, Ordering::Relaxed);
            } else if !d.choosing {
                self.export = None;
            }
        }
        if self.export.as_ref().is_some_and(|d| d.running.is_some()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn export_body(&mut self, ui: &mut Ui, p: &Palette) -> (bool, bool) {
        let spec = self.spec.clone();
        let team = self.team;
        let d = self.export.as_mut().expect("the dialog is open");
        let running = d.busy();
        let mut close = false;
        let mut go = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Export").font(theme::semibold(16.0)));
            widgets::button_row(ui, |ui| close = widgets::icon_button(ui, icon::X, "Close", p).clicked());
        });
        widgets::small(ui, "Builds a zip from saved data. Original game sounds are never exported; original frames only when you tick them, into separate reference/ folders.", p);
        if d.unsaved {
            ui.label(RichText::new("The editor has unsaved changes: the export uses saved data. Save first to include them.").color(p.warn));
        }
        ui.add_enabled_ui(!running, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("What").font(theme::semibold(13.0)));
            let slot_name = d.slot.map(|r| spec.slot(r).1.name.clone());
            ui.add_enabled_ui(d.slot.is_some(), |ui| {
                let label = slot_name.as_ref().map_or("This slot (open a slot first)".to_string(), |n| format!("This slot: {n}"));
                ui.radio_value(&mut d.scope, Scope::Slot, label);
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.scope, Scope::Group, "Group");
                egui::ComboBox::from_id_salt("exgroup").width(260.0).selected_text(spec.groups[d.group].label.as_str()).show_ui(ui, |ui| {
                    for (i, g) in spec.groups.iter().enumerate() {
                        if ui.selectable_label(d.group == i, &g.label).clicked() {
                            d.group = i;
                        }
                    }
                });
            });
            ui.radio_value(&mut d.scope, Scope::Pack, format!("Everything in your pack ({} with files)", plural(d.with_files, "slot")));
            ui.radio_value(&mut d.scope, Scope::All, "All slots: the complete data set (game systems), images where you have them");
            ui.add_space(6.0);
            ui.label(RichText::new("Include").font(theme::semibold(13.0)));
            ui.checkbox(&mut d.images, "Your frames and team masks (PNG)");
            ui.checkbox(&mut d.sheets, "Sprite sheets per sequence + atlas JSON (TexturePacker hash layout, anchor as pivot)");
            ui.checkbox(&mut d.data, "Data per slot (data.json): timing, anchors, events, building cells and pixels, worker routines, parts, job lists");
            ui.checkbox(&mut d.csv, "CSV tables: coords.csv, routines.csv, frames.csv");
            ui.checkbox(&mut d.sounds, "Your sounds and music");
            ui.checkbox(&mut d.reference, format!("Original frames as PNG (reference only), team colour of player {}", team + 1));
            if d.reference {
                egui::Frame::new().fill(p.panel2).corner_radius(6).inner_margin(egui::Margin::symmetric(10, 7)).show(ui, |ui| {
                    ui.label(RichText::new(REFERENCE_WARNING).color(p.warn).size(12.0));
                });
            }
            ui.checkbox(&mut d.empty, "Also slots without any file of yours (data only)").on_hover_text("For a group or everything; the data of slots you have not replaced yet");
            ui.add_space(6.0);
            ui.label(RichText::new("Sequences (parts, directions)").font(theme::semibold(13.0)));
            match d.slot.filter(|_| d.scope == Scope::Slot) {
                Some(r) => {
                    let slot = spec.slot(r).1;
                    ui.horizontal(|ui| {
                        if ui.small_button("All").clicked() {
                            d.ticked = slot.seqs.iter().map(|q| q.key.clone()).collect();
                        }
                        if ui.small_button("None").clicked() {
                            d.ticked.clear();
                        }
                    });
                    egui::Frame::new().stroke(egui::Stroke::new(1.0, p.line)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(150.0).id_salt("seqpick").show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for q in &slot.seqs {
                                let mut on = d.ticked.contains(&q.key);
                                if ui.checkbox(&mut on, format!("{} ({})", q.label, plural(q.frames.len().max(1), "frame"))).changed() {
                                    if on {
                                        d.ticked.insert(q.key.clone());
                                    } else {
                                        d.ticked.remove(&q.key);
                                    }
                                }
                            }
                        });
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        widgets::caption(ui, "Sequence keys", p);
                        ui.add(egui::TextEdit::singleline(&mut d.keys).hint_text("e.g. 1  or  0,1  (empty = all)").desired_width(f32::INFINITY));
                    });
                    widgets::small(ui, "Buildings: 0 construction, 1 finished, 2 and up animation parts. Settlers, animals, vehicles: 0–5 directions. Objects: growth stages.", p);
                }
            }
        });
        if let Some(r) = &d.running {
            let (done, total) = (r.progress.done.load(Ordering::Relaxed), r.progress.total.load(Ordering::Relaxed).max(1));
            ui.add_space(6.0);
            ui.add(egui::ProgressBar::new(done as f32 / total as f32).desired_height(6.0));
            widgets::small(ui, format!("{done} / {total} slots · {} files · {:.1} MB", r.progress.files.load(Ordering::Relaxed), r.progress.bytes.load(Ordering::Relaxed) as f64 / 1048576.0), p);
        } else if d.choosing {
            ui.add_space(6.0);
            ui.label(RichText::new("Waiting for the save dialog: choose a place for the zip, or cancel it.").color(p.dim));
        } else if !d.status.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(&d.status).color(p.dim));
        }
        ui.add_space(8.0);
        widgets::button_row(ui, |ui| {
            go = widgets::primary(ui, "Export", !running, p).clicked();
            close |= ui.button(if d.running.is_some() { "Cancel" } else { "Close" }).clicked();
        });
        (close, go)
    }
}
