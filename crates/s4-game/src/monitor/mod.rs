//! The monitor: an overlay of ours (not the original's GUI) that watches the whole program —
//! the process, the frames, the simulation and its systems — with graphs over the last minutes.
//!
//! Three states, [`Mode`]: hidden (the start of every game: nothing drawn, nothing sampled — the
//! system sampler thread stops, `s4_viewer::perf` is off, no egui pass runs), open (the window)
//! and minimized (only a small button at the top right, which opens it again; the data goes on).
//! F3 (no key of the release: `HandleF6_F7_Esc` 0x489440 takes F6 / F7 / Esc, `EventManagement`
//! 0x5172f0 F8 / F9 / F11 and the cursor, number and edit keys, `IngameKeybindings` 0x4890b0
//! typed characters) opens it when hidden and hides it otherwise; the window's "minimize"
//! button minimizes it.
//!
//! It is an immediate-mode UI (egui) drawn on the game's wgpu device through
//! [`s4_viewer::gpu::Overlay`], after the game's frame and GUI; the events it takes (over its
//! window or button) do not reach the game. Data:
//! * per frame (the last [`FRAMES`]): the interval between frames and the redraw's own time;
//! * per second (the last [`SECONDS`], 10 minutes): FPS, frame times, the [`system`] sample
//!   (memory, CPU of the process and its threads, GPU memory) and the perf spans' totals.

mod ai_tab;
mod eco_ui;
mod econ_tab;
mod graph;
mod paths_tab;
mod queues_tab;
mod sim_tab;
mod system;
mod units_tab;

pub use paths_tab::NodeWorld;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use egui_wgpu::wgpu;

use egui::{Color32, RichText};
use s4_viewer::gpu::Gpu;
use s4_viewer::perf;
use winit::window::Window;

use graph::{COLOURS, Line, Ring, fmt, graph};

/// Frames kept (about 10 s at 60 FPS).
pub const FRAMES: usize = 600;
/// Seconds kept.
pub const SECONDS: usize = 600;
/// Perf spans graphed (the busiest of the last second).
const SPAN_LINES: usize = 5;

/// The toggle key.
pub const KEY: winit::keyboard::KeyCode = winit::keyboard::KeyCode::F3;

/// What is on the screen.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hidden,
    Open,
    Minimized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    System,
    Simulation,
    Economy,
    Paths,
    Units,
    Ai,
    Queues,
}

/// A building type's name (`BUILDING_…` without the prefix).
fn building_name(k: u16) -> &'static str {
    s4_formats::gamedata::Building::ALL.iter().find(|b| **b as i32 == k as i32).map_or("?", |b| b.name().trim_start_matches("BUILDING_"))
}

/// A good's name.
fn good_name(k: u8) -> &'static str {
    s4_formats::gamedata::Good::ALL.iter().find(|g| **g as i32 == k as i32).map_or("?", |g| g.name().trim_start_matches("GOOD_"))
}

/// A settler type's name.
fn settler_name(k: u16) -> &'static str {
    s4_formats::gamedata::Settler::ALL.iter().find(|s| **s as i32 == k as i32).map_or("?", |s| s.name().trim_start_matches("SETTLER_"))
}

/// What the game gives the monitor each frame.
#[derive(Default)]
pub struct Feed<'a> {
    /// The shown snapshot (None on the start screens).
    pub snapshot: Option<&'a crate::sim::Snapshot>,
    /// The local player (0 on the start screens).
    pub local: u8,
    /// The selection (entity ids) and where a node is in the world.
    pub selection: &'a [u16],
    /// The game's sim (what the monitor reads of it).
    pub sim: Option<&'a std::sync::Arc<crate::sim::stats::Shared>>,
    pub world: Option<NodeWorld<'a>>,
}

/// One second's numbers.
#[derive(Clone, Debug, Default)]
struct Second {
    fps: f32,
    frame_avg: f32,
    frame_max: f32,
    redraw_avg: f32,
    system: Option<system::Sample>,
    /// Each span's total ms in the second.
    spans: BTreeMap<&'static str, f32>,
}

struct Data {
    /// Interval to the previous frame and the redraw's own time, ms.
    frames: Ring<f32>,
    redraws: Ring<f32>,
    seconds: Ring<Second>,
    last_frame: Option<Instant>,
    second_start: Instant,
    /// This second so far: frames, interval sum and max, redraw sum.
    acc: (u32, f32, f32, f32),
    /// The last full second's perf spans.
    spans: BTreeMap<&'static str, perf::Sum>,
    sim: sim_tab::SimData,
}

impl Data {
    fn new() -> Data {
        Data {
            frames: Ring::new(FRAMES),
            redraws: Ring::new(FRAMES),
            seconds: Ring::new(SECONDS),
            last_frame: None,
            second_start: Instant::now(),
            acc: (0, 0.0, 0.0, 0.0),
            spans: BTreeMap::new(),
            sim: sim_tab::SimData::new(),
        }
    }
}

/// The paint output of the last [`Monitor::frame`], for [`s4_viewer::gpu::Overlay::draw`].
struct Painted {
    jobs: Vec<egui::ClippedPrimitive>,
    textures: egui::TexturesDelta,
    screen: egui_wgpu::ScreenDescriptor,
}

pub struct Monitor {
    ctx: egui::Context,
    /// The window's input (None headless: [`Monitor::frame`] takes the input given).
    input: Option<egui_winit::State>,
    renderer: egui_wgpu::Renderer,
    painted: Option<Painted>,
    open: bool,
    tab: Tab,
    sampler: Option<system::Sampler>,
    data: Data,
    /// Whose economy the Economy tab shows (0: the local player's, once known), whose AI the AI
    /// tab shows.
    economy_player: u8,
    ai_player: u8,
    /// The path drawing and this frame's paths (world px).
    paths: paths_tab::PathOptions,
    path_lines: Vec<(Vec<[f32; 2]>, Color32)>,
    /// The sim the monitor reads (the shown game's).
    link: Option<std::sync::Arc<crate::sim::stats::Shared>>,
    /// Frames drawn without a layout of their own (the overlay would be missing): none expected.
    unpainted: u32,
    /// Shown as the small button only.
    minimized: bool,
    /// The players in the game (their teams non-zero; 1..=8 without a game): the player buttons.
    players: Vec<u8>,
    /// Where the last frame put the "minimize" button and the small button (tests click them).
    buttons: [Option<egui::Rect>; 2],
}

impl Monitor {
    fn new(gpu: &Gpu, window: Option<&Window>) -> Monitor {
        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        let input = window.map(|w| egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, w, Some(w.scale_factor() as f32), None, None));
        let renderer = egui_wgpu::Renderer::new(&gpu.device, gpu.format(), None, 1, false);
        name_this_thread("main");
        Monitor { ctx, input, renderer, painted: None, open: false, tab: Tab::System, sampler: None, data: Data::new(), economy_player: 0, ai_player: 2, paths: paths_tab::PathOptions { selected: true, all_of: 0 }, path_lines: Vec::new(), link: None, unpainted: 0, minimized: false, buttons: [None; 2], players: (1..=8).collect() }
    }

    /// The monitor of `gpu`, made on first use.
    pub fn of<'a>(gpu: &'a mut Gpu, window: Option<&Window>) -> &'a mut Monitor {
        if gpu.overlay.as_mut().is_none_or(|o| o.as_any_mut().downcast_mut::<Monitor>().is_none()) {
            let m = Monitor::new(gpu, window);
            gpu.overlay = Some(Box::new(m));
        }
        gpu.overlay.as_mut().and_then(|o| o.as_any_mut().downcast_mut::<Monitor>()).expect("the monitor")
    }

    #[cfg(test)]
    pub fn mode(&self) -> Mode {
        match (self.open, self.minimized) {
            (false, _) => Mode::Hidden,
            (true, false) => Mode::Open,
            (true, true) => Mode::Minimized,
        }
    }

    /// F3: hidden → open; open or minimized → hidden.
    pub fn toggle(&mut self) {
        let open = !self.open;
        self.set_open(open);
        self.minimized = false;
    }

    /// Hide the monitor of `gpu`, if there is one (a game starts with it hidden).
    pub fn hide(gpu: &mut Gpu) {
        if let Some(m) = gpu.overlay.as_mut().and_then(|o| o.as_any_mut().downcast_mut::<Monitor>()) {
            m.set_open(false);
            m.minimized = false;
        }
    }

    /// Shown (open or minimized) or hidden: the sampler thread, the perf spans and the sim's
    /// debug channel run only while it is shown.
    pub fn set_open(&mut self, open: bool) {
        if open == self.open {
            return;
        }
        self.open = open;
        if open {
            self.sampler = Some(system::Sampler::start());
            perf::take();
            perf::enable(true);
            if let Some(l) = &self.link {
                l.set_monitored(true);
            }
            self.data = Data::new();
        } else {
            self.sampler = None;
            perf::enable(false);
            perf::take();
            if let Some(l) = &self.link {
                l.set_monitored(false);
            }
        }
    }

    /// A window event: F3 toggles; what egui wants (over its window or the button) is taken.
    /// Returns whether the game should not see it.
    pub fn on_window_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        use winit::event::{ElementState, WindowEvent};
        use winit::keyboard::PhysicalKey;
        if let WindowEvent::KeyboardInput { event: k, .. } = event
            && k.physical_key == PhysicalKey::Code(KEY)
        {
            if k.state == ElementState::Pressed && !k.repeat {
                self.toggle();
            }
            return true;
        }
        // Hidden: nothing of the monitor's is on the screen, every event is the game's.
        if !self.open {
            return false;
        }
        let Some(st) = &mut self.input else { return false };
        let r = st.on_window_event(window, event);
        // Keys go to the game unless a text field of the monitor has the focus.
        match event {
            WindowEvent::KeyboardInput { .. } => r.consumed && self.ctx.wants_keyboard_input(),
            WindowEvent::CursorMoved { .. } => false,
            _ => r.consumed,
        }
    }

    /// What the frame's redraw (the game's, or the start screens') took.
    pub fn redraw_took(&mut self, d: Duration) {
        if self.open {
            let ms = d.as_secs_f32() * 1000.0;
            self.data.redraws.push(ms);
            self.data.acc.3 += ms;
        }
    }

    /// Before the frame is drawn: count it and lay the monitor out. `raw`: the input when there
    /// is no window (tests); `size`: the frame in px; `ppp`: px per point.
    pub fn frame(&mut self, window: Option<&Window>, raw: Option<egui::RawInput>, size: (u32, u32), ppp: f32, feed: &Feed) {
        let now = Instant::now();
        if self.economy_player == 0 {
            self.economy_player = feed.local;
        }
        if let Some(s) = feed.snapshot {
            let players: Vec<u8> = (1..=8u8).filter(|&p| s.population.teams[p as usize] != 0).collect();
            if !players.is_empty() && players != self.players {
                self.players = players;
            }
        }
        // Another game (or none): the monitor reads that one's sim.
        if self.link.as_ref().map(std::sync::Arc::as_ptr) != feed.sim.map(std::sync::Arc::as_ptr) {
            if let Some(l) = self.link.take().filter(|_| self.open) {
                l.set_monitored(false);
            }
            self.link = feed.sim.cloned();
            if let Some(l) = self.link.as_ref().filter(|_| self.open) {
                l.set_monitored(true);
            }
            self.data.sim = sim_tab::SimData::new();
        }
        if self.open {
            // What the sim thread should read for the tabs shown (and the paths drawn).
            let settlers: Vec<u16> = if self.tab == Tab::Units { feed.selection.iter().copied().take(units_tab::INSPECTED).collect() } else { Vec::new() };
            let want = crate::sim::stats::Want {
                economy: if self.tab == Tab::Economy { self.economy_player } else { 0 },
                ai: if self.tab == Tab::Ai { self.ai_player } else { 0 },
                settlers,
                paths: if self.paths.selected { feed.selection.iter().copied().take(100).collect() } else { Vec::new() },
                paths_of: self.paths.all_of,
                queues: if self.tab == Tab::Queues { self.economy_player } else { 0 },
            };
            if let Some(l) = &self.link {
                l.want(want);
            }
            self.data.sim.take(self.link.as_deref(), feed.snapshot);
            self.count_frame(now, feed);
            if self.minimized {
                self.path_lines.clear();
            } else {
                self.place_paths(feed.world);
            }
        } else {
            // Hidden: no egui pass, nothing drawn.
            self.path_lines.clear();
            self.painted = None;
            return;
        }
        let mut raw = match (raw, &mut self.input, window) {
            (Some(r), _, _) => r,
            (None, Some(st), Some(w)) => st.take_egui_input(w),
            _ => egui::RawInput::default(),
        };
        raw.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / ppp));
        if let Some(vp) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
            vp.native_pixels_per_point = Some(ppp);
        }
        let ctx = self.ctx.clone();
        let out = ctx.run(raw, |ctx| self.ui(ctx));
        let jobs = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        // A frame that was not drawn (no target) keeps its texture changes for the next.
        let mut textures = self.painted.take().map(|p| p.textures).unwrap_or_default();
        textures.append(out.textures_delta);
        self.painted = Some(Painted { jobs, textures, screen: egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point: out.pixels_per_point } });
    }

    fn count_frame(&mut self, now: Instant, feed: &Feed) {
        let d = &mut self.data;
        if let Some(prev) = d.last_frame {
            let ms = now.duration_since(prev).as_secs_f32() * 1000.0;
            d.frames.push(ms);
            d.acc.0 += 1;
            d.acc.1 += ms;
            d.acc.2 = d.acc.2.max(ms);
        }
        d.last_frame = Some(now);
        let secs = now.duration_since(d.second_start).as_secs_f32();
        if secs >= 1.0 {
            let n = d.acc.0.max(1) as f32;
            let spans = perf::take();
            let sample = self.sampler.as_ref().and_then(|s| s.latest.lock().unwrap_or_else(|e| e.into_inner()).clone());
            d.seconds.push(Second {
                fps: d.acc.0 as f32 / secs,
                frame_avg: d.acc.1 / n,
                frame_max: d.acc.2,
                redraw_avg: d.acc.3 / n,
                system: sample,
                spans: spans.iter().map(|(k, s)| (*k, s.total.as_secs_f32() * 1000.0 / secs)).collect(),
            });
            d.spans = spans;
            d.acc = (0, 0.0, 0.0, 0.0);
            d.second_start = now;
            d.sim.second(secs, feed.snapshot);
        }
    }

    fn ui(&mut self, ctx: &egui::Context) {
        if self.minimized {
            egui::Area::new(egui::Id::new("monitor button")).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-6.0, 6.0)).show(ctx, |ui| {
                // Big enough to find at 1920 x 1080.
                let b = ui.add(egui::Button::new(RichText::new("📊 Monitor").size(16.0).strong()).min_size(egui::vec2(110.0, 30.0)).wrap_mode(egui::TextWrapMode::Extend));
                self.buttons[1] = Some(b.rect);
                if b.on_hover_text("Open the monitor (F3 hides it)").clicked() {
                    self.minimized = false;
                }
            });
            return;
        }
        let mut open = true;
        let mut minimize = false;
        egui::Window::new("s4rs monitor")
            .open(&mut open)
            .collapsible(false)
            .default_pos(ctx.screen_rect().right_top() + egui::vec2(-700.0, 32.0))
            .default_size(egui::vec2(680.0, 720.0))
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let b = ui.button("minimize");
                    self.buttons[0] = Some(b.rect);
                    if b.on_hover_text("Down to a small button (F3 hides the monitor)").clicked() {
                        minimize = true;
                    }
                    ui.separator();
                    ui.selectable_value(&mut self.tab, Tab::System, "System");
                    ui.selectable_value(&mut self.tab, Tab::Simulation, "Simulation");
                    ui.selectable_value(&mut self.tab, Tab::Economy, "Economy");
                    ui.selectable_value(&mut self.tab, Tab::Paths, "Pathfinding");
                    ui.selectable_value(&mut self.tab, Tab::Units, "Units");
                    ui.selectable_value(&mut self.tab, Tab::Ai, "AI");
                    ui.selectable_value(&mut self.tab, Tab::Queues, "Queues");
                });
                ui.separator();
                // Both ways: a row wider than the window (a long thread name in a legend) scrolls
                // instead of widening the window by a fraction of a pixel (a steady-test flake).
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                    Tab::System => self.system_tab(ui),
                    Tab::Simulation => self.simulation_tab(ui),
                    Tab::Economy => self.economy_tab(ui),
                    Tab::Paths => self.paths_tab(ui),
                    Tab::Units => self.units_tab(ui),
                    Tab::Ai => self.ai_tab(ui),
                    Tab::Queues => self.queues_tab(ui),
                });
            });
        if minimize {
            self.minimized = true;
        }
        if !open {
            self.set_open(false);
        }
    }

    fn system_tab(&self, ui: &mut egui::Ui) {
        let d = &self.data;
        let last = d.seconds.last().cloned().unwrap_or_default();
        let sys = last.system.clone().unwrap_or_default();
        let mb = |b: u64| b as f32 / 1048576.0;
        let pct = |part: u64, whole: u64| if whole == 0 { "?".to_string() } else { format!("{:.1} %", part as f64 / whole as f64 * 100.0) };
        // The frames of the last few seconds: their 1 % slowest and the worst.
        let mut frames: Vec<f32> = d.frames.iter().copied().collect();
        frames.sort_by(f32::total_cmp);
        let p99 = frames.get((frames.len() * 99 / 100).min(frames.len().saturating_sub(1))).copied().unwrap_or(0.0);
        let worst = frames.last().copied().unwrap_or(0.0);
        let cores = sys.cores.max(1);
        // Off Windows there are no per-process GPU numbers (system.rs): no GPU line without at
        // least the card's name, no GPU graphs.
        let gpu_known = cfg!(windows) || !sys.gpu_name.is_empty();
        graph::summary(
            ui,
            "system summary",
            &[
                (
                    "CPU",
                    vec![
                        (format!("game: {:.1} % of the machine", sys.cpu / cores as f32), "The game's CPU use as a share of the whole machine (all cores together = 100 %)."),
                        (format!("{cores} cores"), "Logical processors of this machine."),
                        (format!("machine: {:.0} % in use", sys.machine_cpu), "The whole machine's CPU use, every program together."),
                    ],
                ),
                (
                    "Memory",
                    vec![
                        (format!("in use: {}", graph::bytes(sys.working_set)), "The game's RAM in use now (working set)."),
                        (format!("{} of {}", pct(sys.working_set, sys.ram_total), graph::bytes(sys.ram_total)), "That RAM as a share of the machine's installed RAM."),
                        (format!("committed: {}", graph::bytes(sys.private)), "Memory the game has reserved for itself (private bytes; part may be on disk)."),
                        (format!("machine free: {}", graph::bytes(sys.ram_free)), "RAM still free on the machine."),
                    ],
                ),
                (
                    "GPU",
                    vec![
                        (if sys.gpu_name.is_empty() { "?".to_string() } else { sys.gpu_name.clone() }, "The graphics card (the one with the most video memory)."),
                        (
                            format!("video memory: {} of {} ({})", sys.gpu.map_or("?".into(), graph::bytes), graph::bytes(sys.vram_total), sys.gpu.map_or("?".into(), |g| pct(g, sys.vram_total))),
                            "The card's own memory the game uses, of what the card has.",
                        ),
                        (format!("3D: {}", sys.gpu_3d.map_or("?".into(), |u| format!("{u:.0} %"))), "How busy the card's 3D engine is with the game's drawing."),
                    ],
                ),
                (
                    "Frames",
                    vec![
                        (format!("{:.0} FPS", last.fps), "Frames drawn in the last second."),
                        (format!("{} ms a frame", fmt(last.frame_avg)), "The average time from one frame to the next."),
                        (format!("1 % low: {:.0} FPS ({} ms)", if p99 > 0.0 { 1000.0 / p99 } else { 0.0 }, fmt(p99)), "The slowest 1 % of the last frames: how smooth it feels."),
                        (format!("worst: {} ms", fmt(worst)), "The slowest frame of the last few seconds."),
                        (format!("drawing: {} ms", fmt(last.redraw_avg)), "The game's own work per frame (the rest of a frame is waiting for the screen)."),
                    ],
                ),
            ]
            .into_iter()
            .filter(|row| row.0 != "GPU" || gpu_known)
            .collect::<Vec<_>>(),
        );
        let per_second = |f: &dyn Fn(&Second) -> f32| d.seconds.iter().map(f).collect::<Vec<f32>>();
        graph(
            ui,
            "Frames",
            "ms",
            &[Line { name: "interval", colour: COLOURS[0], values: d.frames.iter().copied().collect() }, Line { name: "redraw", colour: COLOURS[1], values: d.redraws.iter().copied().collect() }],
            FRAMES,
            20.0,
        );
        graph(ui, "FPS", "", &[Line { name: "fps", colour: COLOURS[2], values: per_second(&|s| s.fps) }], SECONDS, 60.0);
        graph(
            ui,
            "Frame time per second",
            "ms",
            &[Line { name: "avg", colour: COLOURS[0], values: per_second(&|s| s.frame_avg) }, Line { name: "max", colour: COLOURS[3], values: per_second(&|s| s.frame_max) }],
            SECONDS,
            20.0,
        );
        let sys_of = |f: &dyn Fn(&system::Sample) -> f32| d.seconds.iter().map(|s| s.system.as_ref().map_or(0.0, f)).collect::<Vec<f32>>();
        let memory = [
            Line { name: "RAM in use", colour: COLOURS[0], values: sys_of(&|s| mb(s.working_set)) },
            Line { name: "committed", colour: COLOURS[1], values: sys_of(&|s| mb(s.private)) },
            Line { name: "video memory", colour: COLOURS[2], values: sys_of(&|s| s.gpu.map_or(0.0, mb)) },
        ];
        graph(ui, "Memory (game)", "MB", &memory[..if cfg!(windows) { 3 } else { 2 }], SECONDS, 100.0);
        graph(
            ui,
            "CPU",
            "% of the machine",
            &[
                Line { name: "game", colour: COLOURS[0], values: sys_of(&|s| s.cpu / s.cores.max(1) as f32) },
                Line { name: "machine", colour: COLOURS[3], values: sys_of(&|s| s.machine_cpu) },
            ],
            SECONDS,
            10.0,
        );
        // The busiest threads now, over the whole time (a thread can use one core: 100 %).
        let names: Vec<String> = sys.threads.iter().take(5).map(|t| t.0.clone()).collect();
        let mut lines = Vec::new();
        for (k, n) in names.iter().enumerate() {
            let values = d.seconds.iter().map(|s| s.system.as_ref().and_then(|x| x.threads.iter().find(|t| &t.0 == n)).map_or(0.0, |t| t.1)).collect();
            lines.push(Line { name: n, colour: COLOURS[k % COLOURS.len()], values });
        }
        graph(ui, "Threads", "% of one core", &lines, SECONDS, 100.0);
        if cfg!(windows) {
            graph(ui, "GPU 3D engine", "%", &[Line { name: "3D", colour: COLOURS[2], values: sys_of(&|s| s.gpu_3d.unwrap_or(0.0)) }], SECONDS, 10.0);
        }
        egui::CollapsingHeader::new(format!("Threads ({})", sys.threads.len())).id_salt("threads@mod.rs").show(ui, |ui| {
            egui::Grid::new("threads").striped(true).min_col_width(48.0).show(ui, |ui| {
                ui.label(RichText::new("thread").strong());
                ui.label(RichText::new("% of one core").strong()).on_hover_text("A thread runs on one core at a time: 100 % is one core fully busy.");
                ui.end_row();
                for (n, p) in &sys.threads {
                    ui.label(n);
                    ui.label(format!("{} %", fmt(*p)));
                    ui.end_row();
                }
            });
        });
        // The perf spans: the busiest of the last second, graphed and listed.
        let mut busiest: Vec<(&&'static str, &perf::Sum)> = d.spans.iter().collect();
        busiest.sort_by_key(|b| std::cmp::Reverse(b.1.total));
        let lines: Vec<Line> = busiest
            .iter()
            .filter(|(_, s)| !s.total.is_zero())
            .take(SPAN_LINES)
            .enumerate()
            .map(|(k, (n, _))| Line { name: n, colour: COLOURS[k % COLOURS.len()], values: d.seconds.iter().map(|s| s.spans.get(**n).copied().unwrap_or(0.0)).collect() })
            .collect();
        graph(ui, "Perf spans", "ms per s", &lines, SECONDS, 10.0);
        egui::CollapsingHeader::new(format!("Perf spans, last second ({})", busiest.len())).id_salt("perf_spans@mod.rs").default_open(true).show(ui, |ui| {
            egui::Grid::new("spans").striped(true).min_col_width(48.0).show(ui, |ui| {
                for h in ["span", "calls", "total ms", "avg ms", "max ms"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (n, s) in &busiest {
                    let total = s.total.as_secs_f32() * 1000.0;
                    ui.label(**n);
                    ui.label(s.count.to_string());
                    if s.total.is_zero() {
                        ui.label("");
                        ui.label("");
                        ui.label("");
                    } else {
                        ui.label(fmt(total));
                        ui.label(fmt(total / s.count.max(1) as f32));
                        let max = s.max.as_secs_f32() * 1000.0;
                        ui.label(RichText::new(fmt(max)).color(if max > 30.0 { Color32::LIGHT_RED } else { Color32::GRAY }));
                    }
                    ui.end_row();
                }
            });
        });
    }
}

impl s4_viewer::gpu::Overlay for Monitor {
    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, size: (u32, u32), view: s4_viewer::gpu::View) {
        let Some(mut p) = self.painted.take() else {
            if self.open {
                self.unpainted += 1;
            }
            return;
        };
        // The paths with this frame's camera (no lag behind the scroll), under the windows.
        let shapes = self.path_shapes(view, p.screen.pixels_per_point, size);
        if !shapes.is_empty() {
            let mut jobs = self.ctx.tessellate(shapes, p.screen.pixels_per_point);
            jobs.append(&mut p.jobs);
            p.jobs = jobs;
        }
        for (id, delta) in &p.textures.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        let extra = self.renderer.update_buffers(device, queue, encoder, &p.jobs, &p.screen);
        if !extra.is_empty() {
            queue.submit(extra);
        }
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("monitor"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            self.renderer.render(&mut pass.forget_lifetime(), &p.jobs, &p.screen);
        }
        for id in &p.textures.free {
            self.renderer.free_texture(id);
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Name the calling thread for the monitor's thread list (Rust names the threads it spawns,
/// not the main one).
fn name_this_thread(name: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadDescription};
        let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
        // SAFETY: the current thread's pseudo handle and a NUL-terminated wide string.
        unsafe { SetThreadDescription(GetCurrentThread(), w.as_ptr()) };
    }
    // Linux: the thread's `comm` (what /proc and the monitor's thread list show).
    #[cfg(target_os = "linux")]
    let _ = std::fs::write("/proc/thread-self/comm", name);
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = name;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The monitor over a running game (headless, 1920×1080): closed it draws only its button;
    /// opened, after three seconds of frames its System tab has per-second samples (memory, CPU,
    /// spans) and the capture `out/gui/monitor_system.png` shows it; closed again, the sampler
    /// and the perf spans stop.
    #[test]
    fn system_tab_over_a_game() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1920u32, 1080u32);
        let mut gpu = Gpu::headless(w, h).unwrap();
        // Two players with medium start resources (the map places nothing), the other a computer.
        let players = crate::sim::PlayerSetup { races: [Some(0), Some(1), None, None, None, None, None, None], teams: [Some(0), Some(1), None, None, None, None, None, None], control: [1, 3, 0, 0, 0, 0, 0, 0], game_type: 0, local: 1, players: Some(2), race_goods: [0; 7], decided: false, start_resources: 2 };
        let args = crate::Args { delay: 0, hash_interval: 64, no_fog: true, players: Some(players), ..Default::default() };
        let mut g = crate::Game::start("Multiplayer/Choices", &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        let view = g.cam.view();
        let raw = || egui::RawInput::default();
        Monitor::of(&mut gpu, None).frame(None, Some(raw()), (w, h), 1.0, &Feed::default());
        g.redraw(&mut gpu, (960.0, 540.0), None);
        assert_eq!(Monitor::of(&mut gpu, None).mode(), Mode::Hidden);
        // Hidden: no egui pass at all.
        let t = Instant::now();
        for _ in 0..100 {
            Monitor::of(&mut gpu, None).frame(None, Some(raw()), (w, h), 1.0, &Feed::default());
        }
        eprintln!("MONITOR hidden: {:.4} ms a frame", t.elapsed().as_secs_f64() * 10.0);
        assert!(Monitor::of(&mut gpu, None).painted.is_none(), "nothing to draw while hidden");
        Monitor::of(&mut gpu, None).set_open(true);
        let t = Instant::now();
        let mut frames = 0;
        // Until two seconds with a system sample have been counted (the sampler's first comes
        // after a second; a busy machine may take longer: a generous guard, not wall time).
        let sampled = |m: &Monitor| m.data.seconds.iter().filter(|s| s.system.is_some()).count();
        while frames < 30 || sampled(Monitor::of(&mut gpu, None)) < 2 {
            assert!(t.elapsed().as_secs() < 120, "no system samples in 2 minutes ({} seconds counted)", Monitor::of(&mut gpu, None).data.seconds.iter().count());
            frames += 1;
            let started = Instant::now();
            Monitor::of(&mut gpu, None).frame(None, Some(raw()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local: 1, sim: Some(&g.sim.monitor), ..Default::default() });
            g.sim.step(2);
            g.redraw(&mut gpu, (960.0, 540.0), None);
            gpu.wait().unwrap();
            Monitor::of(&mut gpu, None).redraw_took(started.elapsed());
        }
        let m = Monitor::of(&mut gpu, None);
        assert_eq!(m.players, [1, 2], "the player buttons: the game's two players");
        let secs: Vec<&Second> = m.data.seconds.iter().collect();
        assert!(secs.len() >= 2, "{} seconds", secs.len());
        let last = secs.last().unwrap();
        assert!(last.fps > 0.0 && last.frame_avg > 0.0);
        let sys = last.system.as_ref().expect("a system sample");
        assert!(sys.working_set > 0 && sys.private > 0 && sys.cores > 0);
        assert!(sys.ram_total > sys.working_set && sys.ram_free > 0, "the machine's RAM");
        assert!((0.0..=100.0).contains(&sys.machine_cpu));
        eprintln!("MONITOR gpu {:?} {} MB VRAM, game {:?} MB, 3D {:?} %", sys.gpu_name, sys.vram_total >> 20, sys.gpu.map(|g| g >> 20), sys.gpu_3d);
        assert!(!sys.threads.is_empty(), "thread CPU");
        assert!(!m.data.spans.is_empty(), "perf spans while open");
        m.frame(None, Some(raw()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local: 1, sim: Some(&g.sim.monitor), ..Default::default() });
        let rgba = gpu.capture(view).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/monitor_system.png");
        let _ = std::fs::create_dir_all(out.parent().unwrap());
        s4_viewer::client::write_png(&out, w, h, &rgba).unwrap();
        // A save's thumbnail shows the game without the monitor.
        let bare = crate::capture_game(&mut gpu, view).unwrap();
        let px = |img: &[u8], x: u32, y: u32| img[((y * w + x) * 4) as usize..][..4].to_vec();
        let (mx, my) = (w - 200, 300);
        assert_ne!(px(&rgba, mx, my), px(&bare, mx, my), "the monitor's window is in the capture, not in the thumbnail's");
        // The Simulation tab: the sim thread's ticks and the entities.
        let m = Monitor::of(&mut gpu, None);
        assert!(m.data.sim.ticks.iter().count() > 10, "ticks from the sim thread");
        assert!(m.data.sim.counts.as_ref().is_some_and(|c| c.settlers.iter().sum::<u32>() > 0), "entity counts");
        m.tab = Tab::Simulation;
        m.frame(None, Some(raw()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local: 1, sim: Some(&g.sim.monitor), ..Default::default() });
        let rgba = gpu.capture(view).unwrap();
        s4_viewer::client::write_png(&out.with_file_name("monitor_simulation.png"), w, h, &rgba).unwrap();
        // The Economy tab, the computer's (player 2): the sim thread reads it once a second while
        // the tab shows.
        let m = Monitor::of(&mut gpu, None);
        m.tab = Tab::Economy;
        m.economy_player = 2;
        // Until a view from tick 3000 on has come (a second apart on the sim thread).
        let t = Instant::now();
        while Monitor::of(&mut gpu, None).data.sim.views.economy.as_ref().is_none_or(|e| e.tick < 3000) {
            assert!(t.elapsed().as_secs() < 120, "no economy view");
            Monitor::of(&mut gpu, None).frame(None, Some(raw()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local: 1, sim: Some(&g.sim.monitor), ..Default::default() });
            g.sim.step(50);
            g.redraw(&mut gpu, (960.0, 540.0), None);
        }
        let m = Monitor::of(&mut gpu, None);
        let e = m.data.sim.views.economy.as_ref().expect("an economy view");
        assert_eq!(e.player, 2);
        assert!(e.goods.iter().sum::<i32>() > 0, "start goods in store: {e:?}");
        assert!(!e.sites.is_empty() || !e.producers.is_empty(), "the computer builds: {e:?}");
        m.frame(None, Some(raw()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local: 1, sim: Some(&g.sim.monitor), ..Default::default() });
        let rgba = gpu.capture(view).unwrap();
        s4_viewer::client::write_png(&out.with_file_name("monitor_economy.png"), w, h, &rgba).unwrap();
        // The tick's parts came with the ticks (s4_sim::debug on while open).
        let m = Monitor::of(&mut gpu, None);
        assert!(m.data.sim.ticks.iter().any(|t| !t.parts.is_empty()), "tick parts");
        // Pathfinding: every walker's path of the computer drawn over its land; Units: its first
        // settlers selected; AI: its AI.
        let start = g.cur.population.starts[2];
        let at = crate::view_on(&g.scene, Some((start.0 as usize, start.1 as usize)), gpu.size(), 2);
        g.cam.jump(at, &crate::screen(&gpu, g.scene.source.w));
        let sel: Vec<u16> = g.cur.population.settlers.iter().filter(|u| u.owner == 2).map(|u| u.id).take(5).collect();
        let m = Monitor::of(&mut gpu, None);
        m.paths.all_of = 2;
        let shot = |gpu: &mut Gpu, g: &mut crate::Game, tab: Tab, name: &str, until: &dyn Fn(&Monitor) -> bool| {
            Monitor::of(gpu, None).tab = tab;
            let t = Instant::now();
            loop {
                g.sim.step(5);
                g.redraw(gpu, (960.0, 540.0), None);
                let scene = &g.scene;
                let world = |x: i32, y: i32| scene.node_world(x.max(0) as usize, y.max(0) as usize);
                let m = Monitor::of(gpu, None);
                m.frame(None, Some(raw()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local: 1, sim: Some(&g.sim.monitor), selection: &sel, world: Some(&world) });
                if until(m) {
                    break;
                }
                assert!(t.elapsed().as_secs() < 60, "{name}: nothing came");
            }
            let rgba = gpu.capture(g.cam.view()).unwrap();
            s4_viewer::client::write_png(&out.with_file_name(name), w, h, &rgba).unwrap();
        };
        shot(&mut gpu, &mut g, Tab::Paths, "monitor_paths.png", &|m| !m.path_lines.is_empty() && m.data.sim.seconds.iter().any(|s| s.paths.iter().sum::<u32>() > 0));
        shot(&mut gpu, &mut g, Tab::Units, "monitor_units.png", &|m| !m.data.sim.views.settlers.is_empty() && !m.data.sim.log.iter().next().is_none());
        Monitor::of(&mut gpu, None).ai_player = 2;
        shot(&mut gpu, &mut g, Tab::Ai, "monitor_ai.png", &|m| m.data.sim.views.ai.as_ref().is_some_and(|a| a.player == 2 && !a.agents.is_empty()) && m.data.sim.views.eco.as_ref().is_some_and(|e| e.player == 2 && e.sectors.iter().any(|s| !s.entries.is_empty())));
        // Queues: the computer's sites and production orders.
        Monitor::of(&mut gpu, None).economy_player = 2;
        shot(&mut gpu, &mut g, Tab::Queues, "monitor_queues.png", &|m| {
            m.data.sim.views.queues.as_ref().is_some_and(|q| q.player == 2 && !q.jobs.is_empty() && !q.attacks.is_empty() && q.transport.iter().any(|t| !t.offers.is_empty())) && m.data.sim.views.eco.as_ref().is_some_and(|e| e.player == 2)
        });
        let m = Monitor::of(&mut gpu, None);
        m.set_open(false);
        assert!(m.sampler.is_none());
        // (Whether perf is off is not checked here: its switch is process-wide, and other tests
        // running beside this one turn it on.)
    }
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    /// The Queues tab's production orders on a campaign map with smiths and barracks placed
    /// (MCD2_roman1: a barracks, a weapon and a tool smith for player 1): the eco-sector's tool
    /// and weapon lists: each listed with its orders. Capture
    /// `out/gui/monitor_orders.png`.
    #[test]
    fn production_orders() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1920u32, 1080u32);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = crate::Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = crate::Game::start("Campaign/MCD2_roman1", &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        let has = g.cur.population.buildings.iter().filter(|b| b.owner == g.sim.local && matches!(b.kind, 21..=24 | 31)).count();
        let m = Monitor::of(&mut gpu, None);
        m.set_open(true);
        m.tab = Tab::Queues;
        m.economy_player = g.sim.local;
        let t = Instant::now();
        loop {
            g.sim.step(5);
            g.redraw(&mut gpu, (960.0, 540.0), None);
            let local = g.sim.local;
            let m = Monitor::of(&mut gpu, None);
            m.frame(None, Some(egui::RawInput::default()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local, sim: Some(&g.sim.monitor), ..Default::default() });
            if m.data.sim.views.queues.as_ref().is_some_and(|q| q.player == local) {
                break;
            }
            assert!(t.elapsed().as_secs() < 60, "no queues view");
        }
        let q = Monitor::of(&mut gpu, None).data.sim.views.queues.clone().unwrap();
        eprintln!("ORDERS {has} order buildings on the map; eco-sectors {:?}", q.eco_orders.iter().map(|e| (e.eco, e.tools.len(), e.weapons.len(), e.weapon_auto)).collect::<Vec<_>>());
        assert!(has >= 3);
        assert!(q.eco_orders.iter().any(|e| !e.tools.is_empty() && !e.weapons.is_empty()), "the smiths' eco-sector with its tool and weapon lists");
        assert!(!q.jobs.is_empty(), "job states");
        // The smiths' dialog facts: weapons have percentages (0x54d660 reads +0x37c), tools none
        // (0x54d480 reads only the counts).
        for (kind, pct) in [(21u8, false), (22, true)] {
            let id = g.cur.population.buildings.iter().find(|b| b.owner == g.sim.local && b.kind == kind).unwrap().id;
            let (tx, rx) = std::sync::mpsc::channel();
            g.sim.with_world(move |w| {
                let _ = tx.send(s4_viewer::building_dialog::Facts::from_world(w, id).unwrap().orders);
            });
            g.sim.step(1);
            let orders = rx.recv().unwrap();
            assert!(!orders.is_empty());
            assert_eq!(orders.iter().any(|o| o.2 != 0), pct, "smith {kind}: {orders:?}");
        }
        let local = g.sim.local;
        Monitor::of(&mut gpu, None).frame(None, Some(egui::RawInput::default()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local, sim: Some(&g.sim.monitor), ..Default::default() });
        let rgba = gpu.capture(g.cam.view()).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/monitor_orders.png");
        s4_viewer::client::write_png(&out, w, h, &rgba).unwrap();
    }
}

#[cfg(test)]
mod steady_tests {
    use super::*;

    /// Flicker: every tab drawn 60 frames in a row at a real frame rate (16 ms) while the game
    /// runs at its speed (a tick each 71 ms) and the sim thread sends its reads (paths 10 times a
    /// second, the rest once a second): the monitor's window keeps its place and size, every frame
    /// is drawn with the overlay, and a view once shown never goes back to nothing.
    #[test]
    fn tabs_are_steady() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1920u32, 1080u32);
        let mut gpu = Gpu::headless(w, h).unwrap();
        gpu.set_offscreen(true);
        let players = crate::sim::PlayerSetup { races: [Some(0), Some(1), None, None, None, None, None, None], teams: [Some(0), Some(1), None, None, None, None, None, None], control: [1, 3, 0, 0, 0, 0, 0, 0], game_type: 0, local: 1, players: Some(2), race_goods: [0; 7], decided: false, start_resources: 2 };
        let args = crate::Args { delay: 0, hash_interval: 64, no_fog: true, players: Some(players), ..Default::default() };
        let mut g = crate::Game::start("Multiplayer/Choices", &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        g.sim.step(1500);
        let sel: Vec<u16> = g.cur.population.settlers.iter().filter(|u| u.owner == 2).map(|u| u.id).take(5).collect();
        let m = Monitor::of(&mut gpu, None);
        m.set_open(true);
        m.economy_player = 2;
        m.ai_player = 2;
        m.paths.all_of = 2;
        let mut last_tick = Instant::now();
        for tab in [Tab::System, Tab::Simulation, Tab::Economy, Tab::Paths, Tab::Units, Tab::Ai, Tab::Queues] {
            Monitor::of(&mut gpu, None).tab = tab;
            let mut rects = Vec::new();
            let mut shown = false;
            for k in 0..80 {
                let frame = Instant::now();
                if last_tick.elapsed().as_millis() >= 71 {
                    g.sim.step(1);
                    last_tick = Instant::now();
                }
                // As the app does: the monitor's layout, then the game's redraw (one draw).
                let scene = &g.scene;
                let world = |x: i32, y: i32| scene.node_world(x.max(0) as usize, y.max(0) as usize);
                let m = Monitor::of(&mut gpu, None);
                m.frame(None, Some(egui::RawInput::default()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local: 1, selection: &sel, sim: Some(&g.sim.monitor), world: Some(&world) });
                let v = &m.data.sim.views;
                let has = match tab {
                    Tab::System => !m.data.seconds.iter().next().is_none(),
                    Tab::Simulation => m.data.sim.counts.is_some(),
                    Tab::Economy => v.economy.as_ref().is_some_and(|e| e.player == 2),
                    Tab::Paths => !m.path_lines.is_empty(),
                    Tab::Units => !v.settlers.is_empty(),
                    Tab::Ai => v.ai.is_some() && v.eco.is_some(),
                    Tab::Queues => v.queues.as_ref().is_some_and(|q| q.player == 2),
                };
                assert!(!(shown && !has), "{tab:?}: the view went away at frame {k}");
                shown |= has;
                let rect = m.ctx.memory(|mem| mem.area_rect(egui::Id::new("s4rs monitor")));
                g.redraw(&mut gpu, (960.0, 540.0), None);
                gpu.wait().unwrap();
                // The last 60 frames: the window does not move or change size.
                if k >= 20 {
                    rects.push(rect);
                }
                let spent = frame.elapsed();
                if spent < std::time::Duration::from_millis(16) {
                    std::thread::sleep(std::time::Duration::from_millis(16) - spent);
                }
            }
            assert!(shown, "{tab:?}: nothing came");
            // Flicker, not sub-pixel layout jitter: within 1 px of the first rect.
            let near = |a: &Option<egui::Rect>, b: &Option<egui::Rect>| match (a, b) {
                (Some(a), Some(b)) => (a.min - b.min).length() <= 1.0 && (a.max - b.max).length() <= 1.0,
                _ => a == b,
            };
            let first = rects[0];
            assert!(rects.iter().all(|r| near(r, &first)), "{tab:?}: the window moved: {first:?} .. {:?}", rects.iter().find(|r| !near(r, &first)));
        }
        assert_eq!(Monitor::of(&mut gpu, None).unpainted, 0, "frames drawn without the overlay");
    }
}


/// What other modules' tests (main.rs `qa_pass3`) drive: the tab shown, the buttons, the paths.
#[cfg(test)]
impl Monitor {
    /// Show tab `k` of System, Simulation, Economy, Paths, Units, AI, Queues.
    pub fn qa_tab(&mut self, k: usize) {
        self.tab = [Tab::System, Tab::Simulation, Tab::Economy, Tab::Paths, Tab::Units, Tab::Ai, Tab::Queues][k % 7];
    }

    /// The "minimize" button and the small button where the last frame put them.
    pub fn qa_buttons(&self) -> [Option<egui::Rect>; 2] {
        self.buttons
    }

    /// Draw every walker's path of `player` (0: off) besides the selection's; the path lines
    /// this frame.
    pub fn qa_paths(&mut self, all_of: u8) -> usize {
        self.paths.all_of = all_of;
        self.path_lines.len()
    }
}

#[cfg(test)]
mod mode_tests {
    use super::*;

    /// The monitor's states: hidden (the start: nothing drawn) → F3 → open → its "minimize"
    /// button → minimized (the small button only) → a click on it → open → F3 → hidden; and
    /// minimized → F3 → hidden. A game's start hides it.
    #[test]
    fn hidden_open_minimized() {
        let (w, h) = (1280u32, 1024u32);
        let gpu = Gpu::headless(w, h);
        let Ok(mut gpu) = gpu else { return };
        let none = Feed::default();
        // A click at `r`'s centre: pressed this frame, released the next.
        let click = |gpu: &mut Gpu, r: egui::Rect| {
            let pos = r.center();
            for pressed in [true, false] {
                let mut raw = egui::RawInput::default();
                raw.events.push(egui::Event::PointerMoved(pos));
                raw.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
                Monitor::of(gpu, None).frame(None, Some(raw), (w, h), 1.0, &none);
            }
        };
        let frame = |gpu: &mut Gpu| Monitor::of(gpu, None).frame(None, Some(egui::RawInput::default()), (w, h), 1.0, &none);
        frame(&mut gpu);
        let m = Monitor::of(&mut gpu, None);
        assert_eq!(m.mode(), Mode::Hidden);
        assert!(m.painted.is_none(), "hidden: nothing drawn");
        m.toggle(); // F3
        assert_eq!(m.mode(), Mode::Open);
        frame(&mut gpu);
        frame(&mut gpu);
        let r = Monitor::of(&mut gpu, None).buttons[0].expect("the minimize button");
        click(&mut gpu, r);
        frame(&mut gpu);
        frame(&mut gpu);
        assert_eq!(Monitor::of(&mut gpu, None).mode(), Mode::Minimized);
        let r = Monitor::of(&mut gpu, None).buttons[1].expect("the small button");
        assert!(r.width() >= 100.0 && r.height() >= 28.0, "the minimized button is findable: {r:?}");
        frame(&mut gpu);
        if let Ok(rgba) = gpu.capture(s4_viewer::gpu::View { x: 0.0, y: 0.0, zoom: 1.0 }) {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/monitor_minimized.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = s4_viewer::client::write_png(&out, w, h, &rgba);
        }
        click(&mut gpu, r);
        assert_eq!(Monitor::of(&mut gpu, None).mode(), Mode::Open);
        Monitor::of(&mut gpu, None).toggle(); // F3
        assert_eq!(Monitor::of(&mut gpu, None).mode(), Mode::Hidden);
        // Minimized → F3 → hidden.
        let m = Monitor::of(&mut gpu, None);
        m.toggle();
        m.minimized = true;
        assert_eq!(m.mode(), Mode::Minimized);
        m.toggle();
        assert_eq!(m.mode(), Mode::Hidden);
        // A game's start hides an open one.
        Monitor::of(&mut gpu, None).toggle();
        Monitor::hide(&mut gpu);
        assert_eq!(Monitor::of(&mut gpu, None).mode(), Mode::Hidden);
    }
}
