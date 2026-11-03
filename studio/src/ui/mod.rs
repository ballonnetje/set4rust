//! The main window: state, actions, and the frame loop. The panels live in the submodules.

mod details;
mod editor;
mod export_dlg;
mod overview;
mod sidebar;
mod slot;
pub mod stage;
pub mod widgets;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use egui::{Align2, Color32, Context, Event, Key, Modifiers, RichText, Vec2, vec2};
use serde_json::{Map, Value};

use crate::audio::{self, Audio};
use crate::editor::{Editor, Scope, Tool};
use crate::images::Images;
use crate::layers::{LayerState, restore_layers};
use crate::model::{Building, GAME_FPS, Group, Kind, Seq, Slot, SlotRef, Spec};
use crate::pack::{Conf, MUSIC_EXT, Pack, animated, frame_path, mask_path, seq_dir, seq_mult, sound_path, write_atomic};
use crate::pngio;
use crate::settings::Settings;
use crate::status::{Filter, Statuses, matches, natural_cmp, plural};
use crate::theme::{self, Palette, icon};

pub use export_dlg::ExportDlg;

/// Playback position in sub-frames: SUB per original frame, so frame multipliers 1..4 (and 6) all
/// land on whole frames.
pub const SUB: f64 = 12.0;

pub const ZOOMS: [u32; 5] = [1, 2, 3, 4, 6];

/// File kinds the frame picker and the drop accept.
pub const IMAGE_EXT: [&str; 9] = ["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif", "tiff", "tga"];
/// File kinds the sound picker offers.
pub const AUDIO_EXT: [&str; 8] = ["wav", "mp3", "ogg", "flac", "m4a", "aac", "aif", "aiff"];

/// The playback clock: `pos` in sub-frames, advancing at `fps` original frames per second from
/// (`play_t`, `play_pos`) while playing.
#[derive(Clone, Copy, Debug)]
pub struct Playback {
    pub fps: f64,
    pub playing: bool,
    pub pos: f64,
    pub play_t: f64,
    pub play_pos: f64,
}

impl Default for Playback {
    fn default() -> Playback {
        Playback { fps: GAME_FPS, playing: true, pos: 0.0, play_t: 0.0, play_pos: 0.0 }
    }
}

impl Playback {
    pub fn cur(&self, now: f64) -> f64 {
        if self.playing { self.play_pos + (now - self.play_t) * self.fps * SUB } else { self.pos }
    }

    pub fn set_playing(&mut self, p: bool, now: f64) {
        if p == self.playing {
            return;
        }
        if p {
            self.play_t = now;
            self.play_pos = self.pos;
        } else {
            self.pos = self.cur(now).floor();
        }
        self.playing = p;
    }

    pub fn set_fps(&mut self, f: f64, now: f64) {
        self.play_pos = self.cur(now);
        self.play_t = now;
        self.fps = f;
    }

    /// One frame of the finest sequence on the page (snapped to that frame grid).
    pub fn step(&mut self, dir: f64, kmax: u32, now: f64) {
        self.set_playing(false, now);
        let q = SUB / kmax.max(1) as f64;
        self.pos = ((self.pos / q).floor() + dir) * q;
    }

    pub fn seek_tick(&mut self, tick: i64, now: f64) {
        self.set_playing(false, now);
        self.pos = tick as f64 * SUB;
    }

    pub fn label(&self) -> String {
        format!("{:.1} fps{}", self.fps, if (self.fps - GAME_FPS).abs() < 0.05 { " (game)" } else { "" })
    }
}

/// `(j, i)`: your frame and the original frame at position `pos`, for `n` originals at `k`×.
pub fn frames_at(pos: f64, n: usize, k: u32) -> (usize, usize) {
    let nk = (n * k as usize).max(1) as i64;
    let j = ((pos * k as f64 / SUB).floor() as i64).rem_euclid(nk) as usize;
    (j, j / k as usize)
}

/// The list the sidebar shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Category(usize),
    Group(usize),
    Slot(SlotRef),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Compare,
    Edit,
}

/// The large view's camera.
#[derive(Clone, Copy, Debug, Default)]
pub struct EdView {
    /// Zoom picked in the menu; 0: fit.
    pub pick: u32,
    /// Zoom set with the wheel; wins over `pick`.
    pub z: Option<f32>,
    pub pan: Vec2,
    /// World point in the middle of the view; set when the view opens.
    pub center: Option<[f32; 2]>,
    /// The fit zoom, and the canvas size it was found for: it follows the window, not the edits,
    /// so the view holds still under the pointer.
    pub fit: Option<(f32, Vec2)>,
}

/// A question with a yes.
pub struct Confirm {
    pub text: String,
    pub yes: String,
    pub danger: bool,
    pub on_yes: Cmd,
    pub on_no: Option<Cmd>,
}

pub type Cmd = Box<dyn FnOnce(&mut Studio)>;
pub(crate) type Done = Box<dyn FnOnce(&mut Studio) + Send>;
type Paths = Box<dyn FnOnce(&mut Studio, Vec<PathBuf>)>;

struct PendingDialog {
    rx: Receiver<Vec<PathBuf>>,
    then: Paths,
}

struct BgJob {
    label: String,
    rx: Receiver<Done>,
}

/// Dropped files waiting for the sequence they belong to.
pub struct DropChoice {
    pub files: Vec<PathBuf>,
    pub seq: usize,
}

struct Toast {
    text: String,
    until: f64,
}

pub struct Studio {
    pub data: PathBuf,
    pub spec: Arc<Spec>,
    pub pack: Arc<Pack>,
    pub status: Statuses,
    pub settings: Settings,
    pub sel: Option<SlotRef>,
    pub tab: Tab,
    pub open: HashSet<String>,
    pub filter: Filter,
    pub search_text: String,
    pub search: String,
    search_due: Option<f64>,
    pub focus_search: bool,
    pub rows: Vec<Row>,
    pub rows_dirty: bool,
    pub scroll_to_sel: u8,
    pub play: Playback,
    pub zoom: u32,
    pub team: usize,
    pub onion: bool,
    pub layers: Vec<LayerState>,
    pub ed: Option<Editor>,
    pub view: EdView,
    /// The large view's last transform, `[ox, oy, z]`.
    #[cfg(test)]
    pub xf: [f32; 3],
    pub images: Images,
    pub audio: Audio,
    pub active_seq: usize,
    pub drag_layer: Option<usize>,
    pub edit_session: u64,
    pub route_seen: Option<u64>,
    /// Finest frame multiplier on the page, for the step keys.
    pub kmax: u32,
    pub volume: f32,
    pub export: Option<ExportDlg>,
    pub drop_choice: Option<DropChoice>,
    confirms: Vec<Confirm>,
    cmds: Vec<Cmd>,
    dialog: Option<PendingDialog>,
    /// Stands in for the native dialog in tests.
    #[cfg(test)]
    pub dialog_tx: Option<Sender<Vec<PathBuf>>>,
    job: Option<BgJob>,
    toast: Option<Toast>,
    now: f64,
    close_ok: bool,
    close_asked: bool,
    settings_dirty: bool,
    pub checker: Option<egui::TextureHandle>,
    pub ctx: Context,
}

impl Studio {
    pub fn new(ctx: &Context, data: PathBuf, spec: Arc<Spec>, pack: Pack, settings: Settings) -> Studio {
        let status = Statuses::compute(&spec, &pack);
        let layers = restore_layers(&settings.layers);
        let mut st = Studio {
            data,
            status,
            pack: Arc::new(pack),
            sel: None,
            tab: Tab::Compare,
            open: HashSet::new(),
            filter: Filter::All,
            search_text: String::new(),
            search: String::new(),
            search_due: None,
            focus_search: false,
            rows: Vec::new(),
            rows_dirty: true,
            scroll_to_sel: 0,
            play: Playback::default(),
            zoom: if ZOOMS.contains(&settings.zoom) { settings.zoom } else { 2 },
            team: settings.team.min(7),
            onion: false,
            layers,
            ed: None,
            view: EdView::default(),
            #[cfg(test)]
            xf: [0.0, 0.0, 1.0],
            images: Images::new(ctx),
            audio: Audio::default(),
            active_seq: 0,
            drag_layer: None,
            edit_session: 0,
            route_seen: None,
            kmax: 1,
            volume: settings.volume.clamp(0.0, 1.0),
            export: None,
            drop_choice: None,
            confirms: Vec::new(),
            cmds: Vec::new(),
            dialog: None,
            #[cfg(test)]
            dialog_tx: None,
            job: None,
            toast: None,
            now: 0.0,
            close_ok: false,
            close_asked: false,
            settings_dirty: false,
            checker: Some(stage::checker_texture(ctx)),
            ctx: ctx.clone(),
            settings,
            spec,
        };
        if let Some(r) = st.settings.last_slot.as_deref().and_then(|id| st.spec.find(id)) {
            st.route(Some(r));
        }
        st
    }

    /// Whether a native dialog is up.
    #[cfg(test)]
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    #[cfg(test)]
    pub fn job_running(&self) -> bool {
        self.job.is_some()
    }

    #[cfg(test)]
    pub fn toast_text(&self) -> Option<&str> {
        self.toast.as_ref().map(|t| t.text.as_str())
    }

    #[cfg(test)]
    pub fn images_pending(&self) -> usize {
        self.images.pending()
    }

    #[cfg(test)]
    pub fn now(&self) -> f64 {
        self.now
    }

    /// Runs after this frame's UI, with the whole state to itself.
    pub fn defer(&mut self, f: impl FnOnce(&mut Studio) + 'static) {
        self.cmds.push(Box::new(f));
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), until: self.now + 3.5 });
    }

    pub fn ask(&mut self, text: impl Into<String>, yes: &str, danger: bool, on_yes: impl FnOnce(&mut Studio) + 'static) {
        self.confirms.push(Confirm { text: text.into(), yes: yes.to_string(), danger, on_yes: Box::new(on_yes), on_no: None });
    }

    pub fn selected(&self) -> Option<(&Group, &Slot)> {
        self.sel.map(|r| self.spec.slot(r))
    }

    pub fn modal_open(&self) -> bool {
        !self.confirms.is_empty() || self.export.is_some() || self.drop_choice.is_some()
    }

    /// The editor holds unsaved changes.
    pub fn dirty(&self) -> bool {
        match (&self.ed, self.selected()) {
            (Some(ed), Some((g, slot))) => ed.is_dirty(g, slot, &self.pack),
            _ => false,
        }
    }

    /// Before anything that would drop the editor's working copy: runs `f`, after asking when
    /// there is something unsaved.
    pub fn guarded(&mut self, f: impl FnOnce(&mut Studio) + 'static) {
        if self.dirty() {
            self.ask("The editor has unsaved changes. Discard them?", "Discard", true, move |st| {
                st.editor_discard();
                f(st);
            });
        } else {
            f(self);
        }
    }

    /// Goes to a slot or the overview, after the guard.
    pub fn go(&mut self, target: Option<SlotRef>) {
        if target != self.sel {
            self.guarded(move |st| st.route(target));
        }
    }

    fn route(&mut self, target: Option<SlotRef>) {
        self.audio.stop();
        self.images.new_page();
        self.sel = target;
        self.ed = None;
        self.view = EdView::default();
        self.active_seq = 0;
        self.route_seen = None;
        self.settings.last_slot = None;
        if let Some(r) = target {
            let spec = self.spec.clone();
            let (g2, slot2) = spec.slot(r);
            self.open.insert(g2.category.clone());
            self.open.insert(g2.id.clone());
            self.rows_dirty = true;
            self.scroll_to_sel = 3;
            self.settings.last_slot = Some(format!("{}/{}", g2.id, slot2.id));
            if g2.kind == Kind::Gfx && slot2.has_map_anchor() {
                self.ed = Some(Editor::new(g2, slot2, &self.pack));
            } else {
                self.tab = Tab::Compare;
            }
        }
        self.settings_dirty = true;
    }

    /// Something pack-wide changed: progress and the sidebar follow.
    pub fn pack_changed(&mut self) {
        self.status = Statuses::compute(&self.spec, &self.pack);
        self.rows_dirty = true;
    }

    /// The files of one slot changed.
    fn slot_changed(&mut self, r: SlotRef) {
        let spec = self.spec.clone();
        self.status.update_slot(&spec, &self.pack, r);
        self.rows_dirty = true;
    }

    /// Re-reads the pack folder on a worker thread, after the guard.
    pub fn refresh(&mut self) {
        self.guarded(Studio::reload_pack);
    }

    fn reload_pack(&mut self) {
        let (spec, root) = (self.spec.clone(), self.pack.root.clone());
        self.spawn("Reading the pack", move || {
            let result = Pack::open(&root).map(|p| {
                let status = Statuses::compute(&spec, &p);
                (p, status)
            });
            Box::new(move |st: &mut Studio| match result {
                Ok((p, status)) => {
                    st.pack = Arc::new(p);
                    st.status = status;
                    st.rows_dirty = true;
                    if let (Some((g, slot)), true) = (st.spec.clone().selected_of(st.sel), st.ed.is_some()) {
                        st.ed = Some(Editor::new(g, slot, &st.pack));
                    }
                    st.toast("Pack re-read.");
                }
                Err(e) => st.toast(format!("Could not read the pack: {e:#}")),
            })
        });
    }

    /// A job on a worker thread; its result runs on the UI thread.
    pub(crate) fn spawn(&mut self, label: &str, f: impl FnOnce() -> Done + Send + 'static) {
        let (tx, rx) = channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(f());
            ctx.request_repaint();
        });
        self.job = Some(BgJob { label: label.to_string(), rx });
    }

    /// Starts a native dialog unless one is up already; `show` opens it and sends what it returns.
    /// `then` runs once the dialog is gone, with no paths when nothing was chosen. False: no dialog
    /// was opened, the user is told, and `then` never runs.
    fn open_dialog(&mut self, then: Paths, show: impl FnOnce(Sender<Vec<PathBuf>>, Context)) -> bool {
        if self.dialog.is_some() {
            self.toast("Close the open file dialog first.");
            return false;
        }
        let (tx, rx) = channel();
        // Tests answer through `dialog_tx`: no window opens.
        #[cfg(test)]
        {
            let _ = show;
            self.dialog_tx = Some(tx);
        }
        #[cfg(not(test))]
        show(tx, self.ctx.clone());
        self.dialog = Some(PendingDialog { rx, then });
        true
    }

    pub fn pick_files(&mut self, title: &str, ext: &[&str], multiple: bool, then: impl FnOnce(&mut Studio, Vec<PathBuf>) + 'static) {
        let d = rfd::AsyncFileDialog::new().set_title(title).add_filter("Files", ext);
        // A cancelled picker has nothing to do.
        let then: Paths = Box::new(move |st, files| {
            if !files.is_empty() {
                then(st, files);
            }
        });
        self.open_dialog(then, move |tx, ctx| {
            if multiple {
                forward(d.pick_files(), tx, ctx, |r: Option<Vec<rfd::FileHandle>>| r.unwrap_or_default().iter().map(|f| f.path().to_path_buf()).collect());
            } else {
                forward(d.pick_file(), tx, ctx, |r: Option<rfd::FileHandle>| r.iter().map(|f| f.path().to_path_buf()).collect());
            }
        });
    }

    /// Asks where to save. True: the dialog is up and `then` will run, with no path on a cancel.
    pub fn pick_save(&mut self, name: &str, then: impl FnOnce(&mut Studio, Vec<PathBuf>) + 'static) -> bool {
        let d = rfd::AsyncFileDialog::new().set_title("Save the export").set_file_name(name).add_filter("Zip", &["zip"]);
        self.open_dialog(Box::new(then), move |tx, ctx| {
            forward(d.save_file(), tx, ctx, |r: Option<rfd::FileHandle>| r.iter().map(|f| f.path().to_path_buf()).collect());
        })
    }

    // ---- writing to the pack --------------------------------------------------------------------

    fn pack_mut(&mut self) -> &mut Pack {
        Arc::make_mut(&mut self.pack)
    }

    /// Merges `patch` into a sequence's `seq.json` (an empty file is deleted), after the guard.
    pub fn save_conf(&mut self, r: SlotRef, seq: usize, patch: impl FnOnce(&mut Map<String, Value>) + 'static) {
        self.guarded(move |st| {
            st.write_conf(r, seq, patch);
        });
    }

    fn write_conf(&mut self, r: SlotRef, seq: usize, patch: impl FnOnce(&mut Map<String, Value>)) -> bool {
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let q = &slot.seqs[seq];
        let mut c: Conf = self.pack.conf(g, slot, q).clone();
        c.bad = false;
        patch(&mut c.map);
        let res = self.pack_mut().put_conf(g, slot, q, &c);
        match res {
            Ok(()) => {
                // The editor reads the new settings from here on: nothing of its own is unsaved.
                self.editor_discard();
                self.slot_changed(r);
                true
            }
            Err(e) => {
                self.toast(format!("Save failed: {e:#}"));
                false
            }
        }
    }

    /// "Remove my frames".
    pub fn clear_seq(&mut self, r: SlotRef, seq: usize) {
        self.guarded(move |st| {
            let spec = st.spec.clone();
            let (g, slot) = spec.slot(r);
            let dir = seq_dir(g, slot, &slot.seqs[seq]);
            let names = st.pack.names_in(&dir).to_vec();
            if names.is_empty() {
                return;
            }
            st.ask(format!("Delete {} in {dir}?", plural(names.len(), "file")), "Delete", true, move |st| {
                for name in names {
                    if let Err(e) = st.pack_mut().del(&format!("{dir}/{name}")) {
                        st.toast(format!("Delete failed: {e:#}"));
                    }
                }
                st.slot_changed(r);
            });
        });
    }

    /// Delete one pack file of a slot, after asking.
    pub fn delete_file(&mut self, r: SlotRef, path: String) {
        self.ask(format!("Delete {path}?"), "Delete", true, move |st| {
            if let Err(e) = st.pack_mut().del(&path) {
                st.toast(format!("Delete failed: {e:#}"));
            }
            st.slot_changed(r);
        });
    }

    /// Frames dropped on (or chosen for) a sequence.
    pub fn save_frames(&mut self, r: SlotRef, seq: usize, files: Vec<PathBuf>) {
        self.guarded(move |st| st.save_frames_checked(r, seq, files));
    }

    fn save_frames_checked(&mut self, r: SlotRef, seq: usize, files: Vec<PathBuf>) {
        let mut files: Vec<PathBuf> = files.into_iter().filter(|f| has_ext(f, &IMAGE_EXT)).collect();
        if files.is_empty() {
            self.toast("No image files.");
            return;
        }
        files.sort_by(|a, b| natural_cmp(&file_name(a), &file_name(b)));
        let (masks, frames): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().partition(|f| is_mask(&file_name(f)));
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let q = &slot.seqs[seq];
        let n = q.frames.len();
        let k = seq_mult(&self.pack, slot, q, self.pack.conf(g, slot, q)) as usize;
        let single = !self.play.playing && frames.len() + masks.len() <= 2 && frames.len() <= 1 && masks.len() <= 1;
        let start = if single { frames_at(self.play.pos, n, k as u32).0 } else { 0 };
        let count = frames.len();
        let write = move |st: &mut Studio| st.write_frames(r, seq, frames, masks, single, start);
        if !single && count > 0 && count != n * k {
            // A whole multiple of the original count: offer to switch the sequence's multiplier.
            let m = count / n;
            if animated(slot, q) && count.is_multiple_of(n) && (1..=8).contains(&m) {
                let text = format!("You dropped {count} frames: {m}× the original's {n}.\n\nSet this sequence to {m} frames per original frame and save?");
                self.ask(text, "Set and save", false, move |st| {
                    let default = st.pack.mult() as usize;
                    st.write_conf(r, seq, move |c| {
                        if m == default {
                            c.remove("frame_multiplier");
                        } else {
                            c.insert("frame_multiplier".into(), m.into());
                        }
                    });
                    write(st);
                });
            } else {
                let why = g.keep_frames.as_ref().map_or(String::new(), |why| format!("\n\nFrame counts in this group are mechanics ({why}): use {} frames ({k}× the original) or another whole multiple of {n}.", n * k));
                let text = format!("You dropped {}; this sequence expects {}.{why}\n\nSave anyway?", plural(count, "frame"), n * k);
                self.ask(text, "Save anyway", false, write);
            }
        } else {
            write(self);
        }
    }

    fn write_frames(&mut self, r: SlotRef, seq: usize, frames: Vec<PathBuf>, masks: Vec<PathBuf>, single: bool, start: usize) {
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let q = &slot.seqs[seq];
        // A full set replaces what is there.
        let mut gone = Vec::new();
        if !single && !frames.is_empty() {
            let dir = seq_dir(g, slot, q);
            gone = self.pack.names_in(&dir).iter().filter(|n| is_frame_file(n)).map(|n| format!("{dir}/{n}")).collect();
        }
        let plan: Vec<(PathBuf, String)> = frames.iter().enumerate().map(|(i, f)| (f.clone(), frame_path(g, slot, q, start + i))).chain(masks.iter().enumerate().map(|(i, f)| (f.clone(), mask_path(g, slot, q, start + i)))).collect();
        let root = self.pack.clone();
        let (nf, nm) = (frames.len(), masks.len());
        let label = if single { format!("Saving frame {start}") } else { format!("Saving {}", plural(nf, "frame")) };
        self.spawn(&label, move || {
            let mut error = None;
            for p in &gone {
                if let Err(e) = std::fs::remove_file(root.abs(p))
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    error = Some(format!("{p}: {e}"));
                }
            }
            let mut written = Vec::new();
            for (from, to) in plan {
                let r = std::fs::read(&from).map_err(anyhow::Error::from).and_then(|b| pngio::to_png(b, &file_name(&from))).and_then(|b| write_atomic(&root.abs(&to), &b));
                match r {
                    Ok(()) => written.push(to),
                    Err(e) => {
                        error = Some(format!("{e:#}"));
                        break;
                    }
                }
            }
            Box::new(move |st: &mut Studio| {
                let pack = st.pack_mut();
                for p in gone.iter().chain(&written) {
                    pack.relist(p);
                }
                st.slot_changed(r);
                let saved = if single {
                    format!("Saved frame {start}.")
                } else {
                    let masks = if nm > 0 { format!(" and {}", plural(nm, "mask")) } else { String::new() };
                    format!("Saved {}{masks}.", plural(nf, "frame"))
                };
                st.toast(error.map_or(saved, |e| format!("Save failed: {e}")));
            })
        });
    }

    /// An audio file for a sound variant or a music track.
    pub fn save_audio(&mut self, r: SlotRef, seq: usize, file: PathBuf) {
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let name = file_name(&file);
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let root = self.pack.clone();
        let to;
        let mut gone = Vec::new();
        if g.kind == Kind::Music {
            if !MUSIC_EXT.contains(&ext.as_str()) {
                self.toast("Music must be MP3, OGG or WAV.");
                return;
            }
            gone = MUSIC_EXT.iter().filter(|e| **e != ext).map(|e| format!("music/{}.{e}", slot.id)).filter(|p| self.pack.has(p)).collect();
            to = format!("music/{}.{ext}", slot.id);
        } else {
            to = sound_path(slot, &slot.seqs[seq]);
        }
        let as_is = g.kind == Kind::Music || ext == "wav";
        self.spawn(&format!("Saving {name}"), move || {
            let mut removed = Ok(());
            for p in &gone {
                removed = removed.and(std::fs::remove_file(root.abs(p)).map_err(anyhow::Error::from));
            }
            let written = removed.and_then(|()| std::fs::read(&file).map_err(anyhow::Error::from)).and_then(|b| if as_is { Ok(b) } else { audio::to_wav(b, &name) }).and_then(|b| write_atomic(&root.abs(&to), &b));
            Box::new(move |st: &mut Studio| {
                let pack = st.pack_mut();
                for p in gone.iter().chain([&to]) {
                    pack.relist(p);
                }
                st.slot_changed(r);
                st.toast(match written {
                    Ok(()) => format!("Saved {name}."),
                    Err(e) => format!("Save failed: {e:#}"),
                });
            })
        });
    }

    /// Starts or pauses a file.
    pub fn play_file(&mut self, path: &Path) {
        if let Err(e) = self.audio.toggle(path, self.volume) {
            self.toast(format!("Cannot play: {e:#}"));
        }
    }

    pub fn set_pack_meta(&mut self, key: &'static str, value: Value) {
        self.guarded(move |st| {
            let r = st.pack_mut().set_meta(key, value);
            if let Err(e) = r {
                st.toast(format!("Save failed: {e:#}"));
            }
            st.pack_changed();
        });
    }

    pub fn save_settings_later(&mut self) {
        self.settings_dirty = true;
    }

    pub fn store_settings(&mut self) {
        self.settings.layers = self.layers.clone();
        self.settings.zoom = self.zoom;
        self.settings.team = self.team;
        self.settings.volume = self.volume;
        self.settings.save(&self.data);
        self.settings_dirty = false;
    }

    // ---- the editor's actions ---------------------------------------------------------------------

    pub fn editor_save(&mut self) {
        let spec = self.spec.clone();
        let (Some((g, slot)), Some(_)) = (self.sel.map(|r| spec.slot(r)), &self.ed) else { return };
        let mut ed = self.ed.take().expect("checked");
        let r = ed.save(g, slot, self.pack_mut());
        self.ed = Some(ed);
        match r {
            Ok(true) => {
                if let Some(r) = self.sel {
                    self.slot_changed(r);
                }
                self.toast("Saved to the pack.");
            }
            Ok(false) => self.toast("Nothing to save."),
            Err(e) => self.toast(format!("Save failed: {e:#}")),
        }
    }

    pub fn editor_discard(&mut self) {
        let spec = self.spec.clone();
        if let (Some((g, slot)), Some(ed)) = (self.sel.map(|r| spec.slot(r)), self.ed.as_mut()) {
            ed.discard(g, slot, &self.pack);
        }
    }

    fn undo(&mut self, back: bool) {
        if let Some(ed) = self.ed.as_mut() {
            let m = ed.undo_step(back).unwrap_or_else(|e| e);
            self.toast(m);
        }
    }

    /// The building data the views show.
    pub fn building(&self, g: &Group, slot: &Slot) -> Option<Building> {
        match &self.ed {
            Some(ed) => ed.building().cloned(),
            None => self.pack.saved_building(g, slot).map(|(b, _)| b),
        }
    }

    /// A sequence's settings: the editor's unsaved ones win.
    pub fn conf_of<'a>(&'a self, g: &'a Group, slot: &'a Slot, seq: &'a Seq) -> &'a Conf {
        match &self.ed {
            Some(ed) => ed.conf(Scope { g, slot, pack: &self.pack, layers: &self.layers }, seq),
            None => self.pack.conf(g, slot, seq),
        }
    }

    pub fn mult_of(&self, g: &Group, slot: &Slot, seq: &Seq) -> u32 {
        seq_mult(&self.pack, slot, seq, self.conf_of(g, slot, seq))
    }

    fn step_frame(&mut self, dir: f64) {
        self.play.step(dir, self.kmax, self.now);
    }

    fn next_slot(&mut self, dir: i64) {
        let spec = self.spec.clone();
        let hit = |r: SlotRef| {
            let (_, slot) = spec.slot(r);
            matches(self.status.slot(r), slot, self.filter, &self.search)
        };
        let in_group: Vec<SlotRef> = self.sel.map_or(Vec::new(), |r| (0..spec.groups[r.g].slots.len()).map(|s| SlotRef { g: r.g, s }).filter(|x| hit(*x)).collect());
        let list = if in_group.is_empty() {
            let mut all = Vec::new();
            for cat in &spec.categories {
                for id in &cat.groups {
                    let g = spec.group_index(id).unwrap_or(0);
                    all.extend((0..spec.groups[g].slots.len()).map(|s| SlotRef { g, s }).filter(|x| hit(*x)));
                }
            }
            all
        } else {
            in_group
        };
        if list.is_empty() {
            return;
        }
        let cur = self.sel.and_then(|r| list.iter().position(|x| *x == r)).map_or(-1, |p| p as i64);
        let next = list[(cur + dir).rem_euclid(list.len() as i64) as usize];
        self.go(Some(next));
    }

    fn handle_keys(&mut self, ctx: &Context) {
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            if self.ed.is_some() {
                self.editor_save();
            }
            return;
        }
        if self.modal_open() || ctx.egui_wants_keyboard_input() {
            return;
        }
        // Off the Edit tab the selection is out of sight: the keys belong to playback there.
        let has_ed = self.ed.is_some() && self.tab == Tab::Edit;
        if has_ed {
            let z = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Z));
            let shift_z = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z));
            let y = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Y));
            if z {
                self.undo(true);
            }
            if shift_z || y {
                self.undo(false);
            }
        }
        if ctx.input(|i| i.modifiers.command) {
            return;
        }
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let held_shift = ctx.input(|i| i.modifiers.shift);
        let mut handled = false;
        if has_ed {
            let building = self.ed.as_ref().is_some_and(|e| e.building().is_some());
            let tool = [(Key::V, Tool::Select, true), (Key::N, Tool::Anchor, true), (Key::B, Tool::Paint, building), (Key::E, Tool::Erase, building), (Key::A, Tool::Add, building)];
            for (key, tool, ok) in tool {
                if ok && pressed(key) {
                    if let Some(ed) = self.ed.as_mut() {
                        ed.tool = tool;
                        ed.hover = None;
                    }
                    handled = true;
                }
            }
            if pressed(Key::Escape) {
                if let Some(ed) = self.ed.as_mut() {
                    ed.sel = None;
                }
                handled = true;
            }
            let selected = self.ed.as_ref().is_some_and(|e| e.sel.is_some());
            if selected && (pressed(Key::Delete) || pressed(Key::Backspace)) {
                if let Some(Err(m)) = self.ed.as_mut().map(Editor::delete_sel) {
                    self.toast(m);
                }
                handled = true;
            }
            if selected {
                let step = if held_shift { 10 } else { 1 };
                for (key, dx, dy) in [(Key::ArrowLeft, -step, 0), (Key::ArrowRight, step, 0), (Key::ArrowUp, 0, -step), (Key::ArrowDown, 0, step)] {
                    if pressed(key) {
                        self.nudge(dx, dy);
                        handled = true;
                    }
                }
            }
        }
        if handled {
            return;
        }
        if pressed(Key::Space) {
            let now = self.now;
            self.play.set_playing(!self.play.playing, now);
        } else if pressed(Key::ArrowRight) {
            self.step_frame(1.0);
        } else if pressed(Key::ArrowLeft) {
            self.step_frame(-1.0);
        } else if pressed(Key::J) {
            self.next_slot(1);
        } else if pressed(Key::K) {
            self.next_slot(-1);
        } else if pressed(Key::R) {
            self.refresh();
        }
        if ctx.input(|i| i.key_pressed(Key::Slash) || i.events.iter().any(|e| matches!(e, Event::Text(t) if t == "/"))) {
            self.focus_search = true;
        }
    }

    fn nudge(&mut self, dx: i32, dy: i32) {
        let spec = self.spec.clone();
        let (pack, layers) = (self.pack.clone(), self.layers.clone());
        if let (Some(r), Some(ed)) = (self.sel, self.ed.as_mut()) {
            let (g, slot) = spec.slot(r);
            ed.nudge(Scope { g, slot, pack: &pack, layers: &layers }, dx, dy);
        }
    }

    fn handle_drops(&mut self, ctx: &Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if dropped.is_empty() || self.modal_open() {
            return;
        }
        let Some(r) = self.sel else {
            self.toast("Open a slot first: dropped files go to the slot on show.");
            return;
        };
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let targets: Vec<usize> = (0..slot.seqs.len()).filter(|&i| !slot.seqs[i].missing).collect();
        match (targets.as_slice(), g.kind) {
            ([], _) => self.toast("Nothing to replace here."),
            ([one], _) => self.drop_on(r, *one, dropped),
            (_, _) => {
                let seq = if targets.contains(&self.active_seq) { self.active_seq } else { targets[0] };
                self.drop_choice = Some(DropChoice { files: dropped, seq });
            }
        }
    }

    pub fn drop_on(&mut self, r: SlotRef, seq: usize, files: Vec<PathBuf>) {
        if self.spec.slot(r).0.kind == Kind::Gfx {
            self.save_frames(r, seq, files);
        } else if let Some(f) = files.into_iter().next() {
            self.save_audio(r, seq, f);
        }
    }

    fn poll(&mut self, ctx: &Context) {
        self.now = ctx.input(|i| i.time);
        match self.job.as_ref().map(|j| j.rx.try_recv()) {
            Some(Ok(done)) => {
                self.job = None;
                done(self);
            }
            Some(Err(TryRecvError::Disconnected)) => {
                let label = self.job.take().map_or(String::new(), |j| j.label);
                self.toast(format!("{label} stopped unexpectedly: re-read the pack with R."));
            }
            _ => {}
        }
        // A dialog whose thread died counts as cancelled, so nothing keeps waiting for it.
        let answer = match self.dialog.as_ref().map(|d| d.rx.try_recv()) {
            Some(Ok(paths)) => Some(paths),
            Some(Err(TryRecvError::Disconnected)) => Some(Vec::new()),
            _ => None,
        };
        if let Some(paths) = answer {
            let d = self.dialog.take().expect("checked");
            (d.then)(self, paths);
        }
        if self.dialog.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if let Some(due) = self.search_due
            && self.now >= due
        {
            self.search_due = None;
            self.search = self.search_text.trim().to_lowercase();
            self.rows_dirty = true;
        } else if self.search_due.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
        if self.toast.as_ref().is_some_and(|t| self.now > t.until + 0.3) {
            self.toast = None;
        }
    }

    pub fn search_changed(&mut self) {
        self.search_due = Some(self.now + 0.15);
    }

    fn close_guard(&mut self, ctx: &Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.close_ok {
            return;
        }
        if self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if !self.close_asked {
                self.close_asked = true;
                let c = ctx.clone();
                self.confirms.push(Confirm {
                    text: "The editor has unsaved changes. Close and discard them?".to_string(),
                    yes: "Close without saving".to_string(),
                    danger: true,
                    on_yes: Box::new(move |st| {
                        st.close_ok = true;
                        c.send_viewport_cmd(egui::ViewportCommand::Close);
                    }),
                    on_no: Some(Box::new(|st| st.close_asked = false)),
                });
            }
        }
    }

    /// Called when the window goes: stops what still runs.
    pub fn shutdown(&mut self) {
        if let Some(d) = self.export.as_mut() {
            d.cancel_and_wait();
        }
        self.store_settings();
    }

    /// One frame of the main window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.close_guard(&ctx);
        self.handle_keys(&ctx);
        self.handle_drops(&ctx);
        let p = theme::palette(&ctx);

        self.top_bar(ui, &p);
        self.sidebar(ui, &p);
        if self.sel.is_some() {
            self.right_panel(ui, &p);
        }
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(p.bg)).show(ui, |ui| match self.sel {
            Some(r) => self.slot_page(ui, r, &p),
            None => self.overview(ui, &p),
        });

        self.export_dialog(&ctx, &p);
        self.drop_choice_dialog(&ctx, &p);
        self.confirm_dialog(&ctx, &p);
        self.hover_overlay(&ctx, &p);
        self.draw_toast(&ctx, &p);
        self.draw_job(&ctx, &p);

        for c in std::mem::take(&mut self.cmds) {
            c(self);
        }
        self.images.end_frame();
        if self.play.playing && self.sel.is_some() && self.spec.slot(self.sel.expect("checked")).0.kind == Kind::Gfx {
            let dt = 1.0 / (self.play.fps * self.kmax as f64);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(dt));
        }
        if self.audio.any_playing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            let size = [r.width(), r.height()];
            if self.settings.window != Some(size) {
                self.settings.window = Some(size);
                self.settings_dirty = true;
            }
        }
        if self.settings_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.store_settings();
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        egui::Panel::top("top")
            .exact_size(52.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(p.panel).inner_margin(egui::Margin::symmetric(14, 0)).stroke(egui::Stroke::new(1.0, p.line)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new(icon::CASTLE_TURRET).size(20.0).color(p.accent));
                    ui.label(RichText::new("Asset Studio").font(theme::semibold(15.0)));
                    if !self.pack.name().is_empty() {
                        ui.label(RichText::new(format!("{}  {}", icon::PACKAGE, self.pack.name())).color(p.dim).size(12.5));
                    }
                    ui.add_space(10.0);
                    let scale = self.pack.scale();
                    widgets::caption(ui, "Pack scale", p);
                    egui::ComboBox::from_id_salt("scale").width(58.0).selected_text(format!("{scale}×")).show_ui(ui, |ui| {
                        for v in crate::pack::SCALES {
                            if ui.selectable_label(scale == v, format!("{v}×")).clicked() {
                                self.defer(move |st| st.set_pack_meta("scale", crate::pack::num(v)));
                            }
                        }
                    });
                    let mult = self.pack.mult();
                    widgets::caption(ui, "Frames", p).on_hover_text("Default frames per original frame for animations (a sequence's seq.json can override it)");
                    egui::ComboBox::from_id_salt("mult").width(58.0).selected_text(format!("{mult}×")).show_ui(ui, |ui| {
                        for v in crate::pack::MULTS {
                            if ui.selectable_label(mult == v, format!("{v}×")).clicked() {
                                self.defer(move |st| st.set_pack_meta("frame_multiplier", v.into()));
                            }
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::icon_button(ui, if p.dark { icon::SUN } else { icon::MOON }, "Light or dark theme", p).clicked() {
                            let dark = !self.settings.dark;
                            self.settings.dark = dark;
                            theme::apply(ui.ctx(), dark);
                            self.settings_dirty = true;
                        }
                        if ui.add(egui::Button::new(format!("{}  Overview", icon::HOUSE))).clicked() {
                            self.defer(|st| st.go(None));
                        }
                        if ui.add(egui::Button::new(format!("{}  Reload pack", icon::ARROWS_CLOCKWISE))).on_hover_text("Re-read the pack folder (R)").clicked() {
                            self.defer(Studio::refresh);
                        }
                        if widgets::primary(ui, &format!("{}  Export", icon::EXPORT), true, p).on_hover_text("Export slots: your files, sprite sheets, data, CSV").clicked() {
                            self.defer(Studio::open_export);
                        }
                        ui.add_space(8.0);
                        let label = self.status.total_label();
                        ui.allocate_ui_with_layout(vec2(200.0, 40.0), egui::Layout::top_down(egui::Align::Max), |ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            ui.add_space(7.0);
                            ui.label(RichText::new(label).color(p.dim).size(12.0));
                            widgets::bar(ui, self.status.have, self.status.need, 190.0, p);
                        });
                    });
                });
            });
    }

    /// While files are dragged over the window.
    fn hover_overlay(&mut self, ctx: &Context, p: &Palette) {
        let n = ctx.input(|i| i.raw.hovered_files.len());
        if n == 0 {
            return;
        }
        let screen = ctx.content_rect();
        let paint = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop-hint")));
        paint.rect_filled(screen, 0.0, Color32::from_black_alpha(150));
        paint.rect_stroke(screen.shrink(14.0), 14.0, egui::Stroke::new(2.0, p.accent), egui::StrokeKind::Inside);
        let what = match self.selected() {
            Some((g, slot)) => format!("Drop {} on {}", plural(n, "file"), if g.kind == Kind::Gfx { slot.name.clone() } else { format!("{} (one file per variant)", slot.name) }),
            None => "Open a slot first".to_string(),
        };
        paint.text(screen.center(), Align2::CENTER_CENTER, what, theme::semibold(18.0), Color32::WHITE);
    }

    fn confirm_dialog(&mut self, ctx: &Context, p: &Palette) {
        if self.confirms.is_empty() {
            return;
        }
        let (mut yes, mut no) = (false, false);
        let c = &self.confirms[0];
        let (text, yes_label, danger) = (c.text.clone(), c.yes.clone(), c.danger);
        let resp = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.label(RichText::new(text).size(13.5));
            ui.add_space(10.0);
            widgets::button_row(ui, |ui| {
                let b = if danger { egui::Button::new(RichText::new(&yes_label).color(Color32::WHITE)).fill(p.err) } else { egui::Button::new(RichText::new(&yes_label).color(p.on_accent)).fill(p.accent) };
                yes = ui.add(b).clicked();
                no = ui.button("Cancel").clicked();
            });
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                yes = true;
            }
        });
        if resp.should_close() {
            no = true;
        }
        if yes || no {
            let c = self.confirms.remove(0);
            if yes {
                self.cmds.push(c.on_yes);
            } else if let Some(f) = c.on_no {
                self.cmds.push(f);
            }
        }
    }

    fn drop_choice_dialog(&mut self, ctx: &Context, p: &Palette) {
        let Some(d) = self.drop_choice.as_mut() else { return };
        let Some(r) = self.sel else {
            self.drop_choice = None;
            return;
        };
        let spec = self.spec.clone();
        let (g, slot) = spec.slot(r);
        let (mut go, mut cancel) = (false, false);
        let n = d.files.len();
        let resp = egui::Modal::new(egui::Id::unique("drop-choice")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(format!("{} for {}", plural(n, "file"), slot.name)).font(theme::semibold(15.0)));
            ui.label(RichText::new(if g.kind == Kind::Gfx { "Which sequence do they belong to?" } else { "Which variant does it replace?" }).color(p.dim));
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (i, q) in slot.seqs.iter().enumerate().filter(|(_, q)| !q.missing) {
                    let label = if g.kind == Kind::Gfx { format!("{} ({})", q.label, plural(q.frames.len(), "frame")) } else { q.label.clone() };
                    if ui.selectable_label(d.seq == i, label).clicked() {
                        d.seq = i;
                    }
                }
            });
            ui.add_space(8.0);
            widgets::button_row(ui, |ui| {
                go = widgets::primary(ui, "Add", true, p).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if resp.should_close() {
            cancel = true;
        }
        if go {
            let d = self.drop_choice.take().expect("checked");
            self.defer(move |st| st.drop_on(r, d.seq, d.files));
        } else if cancel {
            self.drop_choice = None;
        }
    }

    fn draw_toast(&mut self, ctx: &Context, p: &Palette) {
        let Some(t) = &self.toast else { return };
        let left = t.until - self.now;
        let alpha = (left as f32 / 0.3 + 1.0).clamp(0.0, 1.0) * ((3.5 - left as f32) / 0.15).clamp(0.0, 1.0);
        let text = t.text.clone();
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("toast")).order(egui::Order::Tooltip).pivot(Align2::RIGHT_BOTTOM).fixed_pos(screen.right_bottom() - vec2(18.0, 18.0 - 6.0 * (1.0 - alpha))).interactable(false).show(ctx, |ui| {
            ui.set_opacity(alpha);
            egui::Frame::new().fill(p.raised).stroke(egui::Stroke::new(1.0, p.line)).corner_radius(8).inner_margin(egui::Margin::symmetric(14, 9)).shadow(ctx.global_style().visuals.popup_shadow).show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.label(RichText::new(text).color(p.text));
            });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }

    fn draw_job(&mut self, ctx: &Context, p: &Palette) {
        let Some(j) = &self.job else { return };
        let label = j.label.clone();
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::unique("job")).order(egui::Order::Tooltip).pivot(Align2::LEFT_BOTTOM).fixed_pos(screen.left_bottom() + vec2(18.0, -18.0)).interactable(false).show(ctx, |ui| {
            egui::Frame::new().fill(p.raised).stroke(egui::Stroke::new(1.0, p.line)).corner_radius(8).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(label);
                });
            });
        });
    }
}

fn forward<T: Send + 'static>(f: impl std::future::Future<Output = T> + Send + 'static, tx: Sender<Vec<PathBuf>>, ctx: Context, map: impl FnOnce(T) -> Vec<PathBuf> + Send + 'static) {
    std::thread::spawn(move || {
        let r = pollster::block_on(f);
        let _ = tx.send(map(r));
        ctx.request_repaint();
    });
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn has_ext(p: &Path, ext: &[&str]) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| ext.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Files with `team` or `mask` in the name become masks.
pub fn is_mask(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("team") || n.contains("mask")
}

/// `NNN.png` or `NNN.team.png`.
fn is_frame_file(name: &str) -> bool {
    let stem = name.strip_suffix(".team.png").or_else(|| name.strip_suffix(".png"));
    stem.is_some_and(|s| s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit()))
}

impl Spec {
    fn selected_of(&self, sel: Option<SlotRef>) -> Option<(&Group, &Slot)> {
        sel.map(|r| self.slot(r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_clock() {
        let mut p = Playback::default();
        assert_eq!(p.cur(1.0), GAME_FPS * SUB);
        p.set_playing(false, 1.0);
        assert_eq!(p.pos, (GAME_FPS * SUB).floor());
        p.pos = 13.0;
        p.step(1.0, 2, 1.0);
        assert_eq!(p.pos, 18.0, "half a frame on the 2× grid");
        p.step(-1.0, 2, 1.0);
        p.step(-1.0, 2, 1.0);
        assert_eq!(p.pos, 6.0);
        p.set_playing(true, 2.0);
        p.set_fps(1.0, 3.0);
        assert!((p.cur(4.0) - (6.0 + GAME_FPS * SUB + SUB)).abs() < 1e-9);
        p.seek_tick(5, 5.0);
        assert_eq!((p.playing, p.pos), (false, 60.0));
        assert_eq!(Playback::default().label(), "14.1 fps (game)");
    }

    #[test]
    fn frame_of_a_position() {
        assert_eq!(frames_at(0.0, 4, 1), (0, 0));
        assert_eq!(frames_at(12.0, 4, 1), (1, 1));
        assert_eq!(frames_at(18.0, 4, 2), (3, 1), "1.5 originals = your frame 3 at 2×");
        assert_eq!(frames_at(48.0, 4, 1), (0, 0), "wraps");
        assert_eq!(frames_at(-12.0, 4, 1), (3, 3), "wraps backwards");
    }

    #[test]
    fn mask_names() {
        for m in ["000.team.png", "a_mask_1.png", "TEAM.png"] {
            assert!(is_mask(m), "{m}");
        }
        assert!(!is_mask("frame_001.png"));
        assert!(is_frame_file("000.png") && is_frame_file("012.team.png"));
        assert!(!is_frame_file("seq.json") && !is_frame_file("1000.png") && !is_frame_file("a.png"));
    }

}
