//! The application: what the window shows from start to main UI. First run (no game found), the
//! asset spec being written, then the studio.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use anyhow::{Context as _, Result};
use egui::{Align, Context, Layout, RichText, Ui, vec2};
use s4_formats::install;

use crate::model::Spec;
use crate::pack::Pack;
use crate::settings::Settings;
use crate::spec;
use crate::theme::{self, Palette, icon};
use crate::ui::{Studio, widgets};

/// Which build and game folder the spec in the data folder was written from.
pub const STAMP: &str = "source.txt";

pub fn stamp(game: &Path) -> String {
    format!("{}\n{}\n", env!("CARGO_PKG_VERSION"), game.display())
}

pub fn spec_is_stale(spec: &Path, game: &Path) -> bool {
    !spec.join("index.json").is_file() || std::fs::read_to_string(spec.join(STAMP)).ok().as_deref() != Some(stamp(game).as_str())
}

struct Prepared {
    spec: Spec,
    pack: Pack,
}

/// Writes the spec when needed, then reads it and the pack.
fn prepare(game: &Path, data: &Path, groups_done: &AtomicUsize) -> Result<Prepared> {
    let dir = data.join("assetspec");
    if spec_is_stale(&dir, game) {
        spec::run_with_progress(game, &dir, &|n| groups_done.store(n, Ordering::Relaxed))?;
        std::fs::write(dir.join(STAMP), stamp(game)).context("the spec's stamp")?;
    }
    groups_done.store(spec::group_count(), Ordering::Relaxed);
    let spec = Spec::load(&dir, game)?;
    let pack = Pack::open(&data.join("pack"))?;
    Ok(Prepared { spec, pack })
}

enum Screen {
    /// No game found: asks for the folder.
    FirstRun { problem: Option<String>, picking: Option<Receiver<Option<PathBuf>>> },
    Preparing { game: PathBuf, done: Arc<AtomicUsize>, rx: Receiver<Result<Prepared>>, started: f64 },
    Studio(Box<Studio>),
    Failed(String),
}

pub struct App {
    data: PathBuf,
    settings: Settings,
    screen: Screen,
}

impl App {
    /// `game`: the folder found, `None` for the first-run screen.
    pub fn new(ctx: &Context, data: PathBuf, game: Option<PathBuf>) -> App {
        let settings = Settings::load(&data);
        theme::install_fonts(ctx);
        theme::apply(ctx, settings.dark);
        let mut app = App { data, settings, screen: Screen::FirstRun { problem: None, picking: None } };
        if let Some(game) = game {
            app.start(ctx, game);
        }
        app
    }

    /// The studio directly on a spec and pack already at hand, for tests.
    #[cfg(test)]
    pub fn with_studio(ctx: &Context, data: PathBuf, spec: Spec, pack: Pack, dark: bool) -> App {
        let settings = Settings { dark, ..Settings::load(&data) };
        theme::install_fonts(ctx);
        theme::apply(ctx, dark);
        let studio = Studio::new(ctx, data.clone(), Arc::new(spec), pack, settings.clone());
        App { data, settings, screen: Screen::Studio(Box::new(studio)) }
    }

    #[cfg(test)]
    pub fn studio(&mut self) -> &mut Studio {
        match &mut self.screen {
            Screen::Studio(s) => s,
            _ => panic!("not on the studio screen"),
        }
    }

    /// As if the folder dialog had returned `dir`.
    #[cfg(test)]
    pub fn folder_chosen(&mut self, ctx: &Context, dir: PathBuf) {
        self.chosen(ctx, dir);
    }

    /// What the first-run screen says about the folder chosen.
    #[cfg(test)]
    pub fn problem(&self) -> Option<&str> {
        match &self.screen {
            Screen::FirstRun { problem, .. } => problem.as_deref(),
            _ => None,
        }
    }

    /// As if the folder dialog were up; the sender answers for it.
    #[cfg(test)]
    pub fn fake_picking(&mut self) -> std::sync::mpsc::Sender<Option<PathBuf>> {
        let (tx, rx) = channel();
        if let Screen::FirstRun { picking, .. } = &mut self.screen {
            *picking = Some(rx);
        }
        tx
    }

    #[cfg(test)]
    pub fn picking(&self) -> bool {
        matches!(self.screen, Screen::FirstRun { picking: Some(_), .. })
    }

    /// As if the spec were being prepared by a thread that died.
    #[cfg(test)]
    pub fn fake_dead_prepare(&mut self) {
        let (_, rx) = channel();
        self.screen = Screen::Preparing { game: PathBuf::new(), done: Arc::new(AtomicUsize::new(0)), rx, started: 0.0 };
    }

    /// What the failure screen says.
    #[cfg(test)]
    pub fn failed(&self) -> Option<&str> {
        match &self.screen {
            Screen::Failed(m) => Some(m),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn first_run(ctx: &Context, data: PathBuf, problem: Option<String>) -> App {
        let settings = Settings::default();
        theme::install_fonts(ctx);
        theme::apply(ctx, settings.dark);
        App { data, settings, screen: Screen::FirstRun { problem, picking: None } }
    }

    fn start(&mut self, ctx: &Context, game: PathBuf) {
        let done = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = channel();
        let (g, d, n, c) = (game.clone(), self.data.clone(), done.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(prepare(&g, &d, &n));
            c.request_repaint();
        });
        self.screen = Screen::Preparing { game, done, rx, started: ctx.input(|i| i.time) };
    }

    fn pick_folder(&mut self, ctx: &Context) {
        let (tx, rx) = channel();
        let dialog = rfd::AsyncFileDialog::new().set_title(TITLE_PICK).pick_folder();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let r = pollster::block_on(dialog);
            let _ = tx.send(r.map(|f| f.path().to_path_buf()));
            c.request_repaint();
        });
        if let Screen::FirstRun { picking, .. } = &mut self.screen {
            *picking = Some(rx);
        }
    }

    /// Validates a chosen folder: remembered and started, or the problem shown.
    fn chosen(&mut self, ctx: &Context, dir: PathBuf) {
        let missing = install::missing(&dir);
        if missing.is_empty() {
            if let Err(e) = std::fs::write(self.data.join("game_dir.txt"), dir.to_string_lossy().as_bytes()) {
                self.screen = Screen::Failed(format!("Could not remember the game folder: {e}"));
                return;
            }
            install::set_configured(Some(dir.clone()));
            self.start(ctx, dir);
        } else if let Screen::FirstRun { problem, .. } = &mut self.screen {
            *problem = Some(format!("{}\nis not a The Settlers IV History Edition folder: it lacks {}.\nChoose the folder that holds S4_Main.exe.", dir.display(), missing.join(", ")));
        }
    }

    fn poll(&mut self, ctx: &Context) {
        // A dialog whose thread died counts as cancelled: the button comes back.
        let answer = match &self.screen {
            Screen::FirstRun { picking: Some(rx), .. } => match rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Disconnected) => Some(None),
                Err(TryRecvError::Empty) => None,
            },
            _ => None,
        };
        if let Some(r) = answer {
            if let Screen::FirstRun { picking, .. } = &mut self.screen {
                *picking = None;
            }
            if let Some(dir) = r {
                self.chosen(ctx, dir);
            }
        }
        if matches!(self.screen, Screen::FirstRun { picking: Some(_), .. }) {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if let Screen::Preparing { rx, .. } = &self.screen {
            match rx.try_recv() {
                Ok(Ok(p)) => self.screen = Screen::Studio(Box::new(Studio::new(ctx, self.data.clone(), Arc::new(p.spec), p.pack, self.settings.clone()))),
                Ok(Err(e)) => self.screen = Screen::Failed(format!("{e:#}")),
                Err(TryRecvError::Disconnected) => self.screen = Screen::Failed("Preparing the asset spec stopped unexpectedly.".to_string()),
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    fn center(ui: &mut Ui, p: &Palette, content: impl FnOnce(&mut Ui)) {
        egui::CentralPanel::no_frame().frame(egui::Frame::new().fill(p.bg)).show(ui, |ui| {
            let h = ui.available_height();
            ui.vertical_centered(|ui| {
                ui.add_space((h / 2.0 - 190.0).max(20.0));
                ui.set_max_width(540.0);
                widgets::card_frame(p).inner_margin(egui::Margin::same(26)).shadow(ui.visuals().popup_shadow).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.with_layout(Layout::top_down(Align::Min), content);
                });
            });
        });
    }
}

const TITLE_PICK: &str = "Choose The Settlers IV History Edition folder (the one with S4_Main.exe)";

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        let p = theme::palette(&ctx);
        let mut pick = false;
        match &mut self.screen {
            Screen::Studio(s) => s.ui(ui),
            Screen::FirstRun { problem, picking } => {
                let (problem, busy) = (problem.clone(), picking.is_some());
                App::center(ui, &p, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon::CASTLE_TURRET).size(30.0).color(p.accent));
                        ui.label(RichText::new("S4RS Asset Studio").font(theme::semibold(21.0)));
                    });
                    ui.add_space(8.0);
                    ui.label("The Asset Studio reads the game data of your own copy of The Settlers IV History Edition. No installed copy was found on this computer.");
                    ui.add_space(4.0);
                    widgets::caption(ui, "Choose the game's folder, the one with S4_Main.exe. The choice is remembered.", &p);
                    if let Some(text) = &problem {
                        ui.add_space(8.0);
                        egui::Frame::new().fill(p.panel2).corner_radius(6).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(text).color(p.err).size(12.5));
                        });
                    }
                    ui.add_space(14.0);
                    pick = widgets::primary(ui, &format!("{}  Choose the game folder…", icon::FOLDER_OPEN), !busy, &p).clicked();
                });
            }
            Screen::Preparing { game, done, started, .. } => {
                let n = done.load(Ordering::Relaxed);
                let total = spec::group_count();
                let game = game.clone();
                let started = *started;
                App::center(ui, &p, |ui| {
                    ui.label(RichText::new("Preparing the asset spec").font(theme::semibold(18.0)));
                    ui.add_space(4.0);
                    widgets::caption(ui, &format!("From {}", game.display()), &p);
                    ui.add_space(10.0);
                    ui.add(egui::ProgressBar::new(n as f32 / total as f32).desired_height(8.0).animate(true));
                    ui.add_space(4.0);
                    let t = ui.input(|i| i.time) - started;
                    widgets::small(ui, if n >= total { "Reading the spec and your pack…".to_string() } else { format!("{n} / {total} groups · {t:.0} s · the first start takes some seconds and about 300 MB") }, &p);
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Screen::Failed(msg) => {
                let msg = msg.clone();
                App::center(ui, &p, |ui| {
                    ui.label(RichText::new("The Asset Studio cannot start").font(theme::semibold(18.0)).color(p.err));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(msg).wrap());
                    ui.add_space(12.0);
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            }
        }
        if pick {
            self.pick_folder(&ctx);
        }
    }

    fn on_exit(&mut self) {
        if let Screen::Studio(s) = &mut self.screen {
            s.shutdown();
        }
    }

    fn clear_color(&self, v: &egui::Visuals) -> [f32; 4] {
        v.panel_fill.to_normalized_gamma_f32()
    }
}

/// Default window size.
pub fn window_size(data: &Path) -> egui::Vec2 {
    Settings::load(data).window.map_or(vec2(1440.0, 900.0), |w| vec2(w[0].max(960.0), w[1].max(600.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spec_is_rewritten_for_another_game_folder() {
        let dir = crate::pack::tests::temp("stamp");
        let (a, b) = (Path::new("a"), Path::new("b"));
        assert!(spec_is_stale(&dir, a), "no spec yet");
        std::fs::write(dir.join("index.json"), "{}").unwrap();
        assert!(spec_is_stale(&dir, a), "a spec without a stamp");
        std::fs::write(dir.join(STAMP), stamp(a)).unwrap();
        assert!(!spec_is_stale(&dir, a));
        assert!(spec_is_stale(&dir, b));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
