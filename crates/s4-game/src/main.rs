//! s4-game: the client. The simulation runs on its own thread (sim.rs); the window renders the
//! latest published snapshot with the s4-viewer library, interpolating unit positions between
//! ticks; GUI input becomes stamped commands; audio gets per-tick events.
//!
//! ```text
//! s4-game --map Campaign/roman02 [--speed 1] [--no-script]          window
//!         [--windowed | --fullscreen]                               this run's window mode
//! s4-game --headless --map M --ticks N --capture-at t1,t2 --out prefix [--commands log]
//!         [--width 1280] [--height 1024] [--at x,y] [--gui 1] [--sound]
//! s4-game --determinism --map M --ticks N [--commands log]          two runs, compare hashes
//! s4-game --list-buildings P --map M                                 building ids for command logs
//! ```
//! `--sound` (headless): run the game's sound (sound.rs) on an offline mixer at the capture view,
//! 71 ms of audio per tick; prints what was played per sound id and writes `<out>_sound.wav`.
//! Headless runs print the sim's text messages and keep the local player's message list
//! (`--messages-for P`: player P's), whose sounds `--sound` plays.
//! `--input FILE` (headless): scripted mouse input, one action per line, `TICK ACTION ARGS`:
//! `click X Y` / `rclick X Y` (screen px), `click-control DIALOG LINK` (centre of a GUI control),
//! `click-node X Y` / `hover-node X Y` (a map node), `hover X Y`. Actions run when the sim has
//! reached TICK, before a capture at the same tick.
//!
//! Window keys: arrows and W A S D scroll while held (camera.rs `Camera`), PageUp / numpad −
//! zoom in, PageDown / numpad + zoom out, Home to the start, P pause, = / - game speed, G GUI on/off, F5 cloud shadows on/off, Esc the extras menu's quit tab
//! and F6 its message recipients tab (as the original; the start screens still quit on Esc). Mouse: build-page
//! buttons enter the build mode (markers on the possible sites), left click on a marker places the
//! building, right click leaves the mode. The cursor on the window's outermost pixel scrolls, a
//! right-button drag grabs and pans the map, the wheel zooms around the cursor (camera.rs). `--input` also takes `rpress X Y`, `move X Y`
//! and `rrelease`.
//!
//! The window covers its monitor without a frame (borderless) unless the settings'
//! `fullscreen` is off; Alt+Enter switches between the two, as the original (see `App::toggle_fullscreen`).
// The packaged build (`--features package`, tools/package) is a Windows-subsystem program: no
// console window behind the game; a terminal it is started from is attached (`console`).
#![cfg_attr(all(windows, feature = "package"), windows_subsystem = "windows")]

mod ambient;
mod bdlg;
mod briefing;
mod build;
mod camera;
mod clipboard;
mod commands;
mod console;
mod econ_view;
mod queues_view;
mod cursor;
mod frontend;
mod fog;
mod gamedir;
mod logfile;
mod input;
mod interp;
mod menu;
mod messages;
mod missions;
mod monitor;
mod msgwin;
mod options;
mod pages;
#[cfg(test)]
mod playtest;
mod patches;
mod progress;
mod save;
mod script;
mod select;
mod settings;
mod sim;
mod sound;
mod tutorial;
mod video;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use s4_viewer::animate::Pose;
use s4_viewer::client::{self, GuiMouse};
use s4_viewer::gpu::{Gpu, View};
use s4_viewer::perf;
use s4_viewer::scene::{Scene, resolve_map};

use commands::{Stamped, execution_tick};
use sim::{Clock, SimHandle, SimSetup, Snapshot};

#[derive(Default)]
struct Args {
    map: String,
    speed: u32,
    headless: bool,
    determinism: bool,
    /// `--convert-videos`: convert the install's videos into the cache now and exit (video.rs).
    convert_videos: bool,
    ticks: u32,
    capture_at: Vec<u32>,
    out: String,
    commands: Option<String>,
    width: u32,
    height: u32,
    at: Option<(usize, usize)>,
    gui: Option<f32>,
    no_script: bool,
    no_fog: bool,
    list_buildings: Option<u8>,
    input: Option<String>,
    /// `--map` / `--speed` were given (else the start screens and the saved settings decide).
    map_given: bool,
    speed_given: bool,
    /// Headless start screens: run `--input` against the menu, capturing after every action.
    menu: bool,
    /// Network game: host for this many players (`--host N`), or join a host (`--join ip:port`).
    host: Option<usize>,
    join: Option<String>,
    /// Local UDP address (host default 0.0.0.0:7710, client 0.0.0.0:0).
    bind: Option<String>,
    /// [ours] The internet relay (`--relay host:port`): `--host` / `--join` go through it (the
    /// join address is then the room number the host printed), and it is the start screens'
    /// relay for this run instead of `Settings::relay_server`.
    relay: Option<String>,
    /// Network delay in ms (`dt` = delay / 71 ticks).
    delay: u32,
    /// Ticks between state-hash exchanges.
    hash_interval: u32,
    /// Continue a savegame (`--load file`).
    load: Option<String>,
    /// Headless: run the sound on an offline mixer (`--sound`).
    sound: bool,
    /// Headless: keep the message list (messages.rs) for this player instead of the local one
    /// (`--messages-for P`).
    messages_for: Option<u8>,
    /// The players the start screens' game settings set up (single player).
    players: Option<sim::PlayerSetup>,
    /// `--fullscreen` / `--windowed`: this run's window mode, else `Settings::fullscreen` (the
    /// original's command line `Fullscreen`, 0x5009c9).
    fullscreen: Option<bool>,
    /// `--bench FRAMES`: frame time (see [`bench`]).
    bench: u32,
    /// `--perf`: the window logs its frame rate and where the frames' time goes every 2 s.
    perf: bool,
    /// `--audit TICKS`: the missing-graphics audit (see [`audit`]); `--audit-part K/N`: every
    /// N-th map from the K-th (parallel runs).
    audit: u32,
    audit_part: Option<(usize, usize)>,
    /// `--soak TICKS`: a long session's memory and frame time (see [`soak`]).
    soak: u32,
    /// Headless: the cloud shadows on (the window takes them from the settings, F5 toggles).
    clouds: bool,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut a = Args {
        map: "Campaign/roman02".into(),
        speed: 1,
        ticks: 840,
        width: 1280,
        height: 1024,
        out: "out/game/run".into(),
        delay: network_time_delta(),
        hash_interval: 64,
        ..Default::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| anyhow::anyhow!("{arg} needs a value"));
        match arg.as_str() {
            "--map" => {
                a.map = val()?;
                a.map_given = true;
            }
            "--speed" => {
                a.speed = val()?.parse()?;
                a.speed_given = true;
            }
            "--headless" => a.headless = true,
            "--fullscreen" => a.fullscreen = Some(true),
            "--windowed" => a.fullscreen = Some(false),
            "--sound" => a.sound = true,
            "--messages-for" => a.messages_for = Some(val()?.parse()?),
            "--menu" => a.menu = true,
            "--determinism" => a.determinism = true,
            "--convert-videos" => a.convert_videos = true,
            "--ticks" => a.ticks = val()?.parse()?,
            "--capture-at" => a.capture_at = val()?.split(',').map(|t| t.parse()).collect::<Result<_, _>>()?,
            "--out" => a.out = val()?,
            "--commands" => a.commands = Some(val()?),
            "--width" => a.width = val()?.parse()?,
            "--height" => a.height = val()?.parse()?,
            "--gui" => a.gui = Some(val()?.parse()?),
            "--no-script" => a.no_script = true,
            "--no-fog" => a.no_fog = true,
            "--clouds" => a.clouds = true,
            "--input" => a.input = Some(val()?),
            "--list-buildings" => a.list_buildings = Some(val()?.parse()?),
            "--host" => a.host = Some(val()?.parse()?),
            "--join" => a.join = Some(val()?),
            "--bind" => a.bind = Some(val()?),
            "--relay" => {
                a.relay = Some(val()?);
                settings::override_relay(a.relay.as_deref().unwrap_or_default());
            }
            "--delay" => a.delay = val()?.parse()?,
            "--hash-interval" => a.hash_interval = val()?.parse()?,
            "--load" => a.load = Some(val()?),
            "--bench" => a.bench = val()?.parse()?,
            "--perf" => a.perf = true,
            "--audit" => a.audit = val()?.parse()?,
            "--soak" => a.soak = val()?.parse()?,
            "--audit-part" => {
                let v = val()?;
                let (k, n) = v.split_once('/').ok_or_else(|| anyhow::anyhow!("--audit-part K/N"))?;
                a.audit_part = Some((k.parse()?, n.parse()?));
            }
            "--at" => {
                let v = val()?;
                let (x, y) = v.split_once(',').ok_or_else(|| anyhow::anyhow!("--at x,y"))?;
                a.at = Some((x.parse()?, y.parse()?));
            }
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    Ok(a)
}

/// The players' names the GUI shows (the chat page, the statistics, the end statistics): the
/// record +0x14 (0x149b5ec) that `AddPlayer` 0x509f80 takes from `GlobalGameState` + (p + 0x28)
/// · 0x18 (0x507d40), which the map load 0x5069b0 fills from the map's players chunk (char[33],
/// e.g. Aeneas: "Latinus", a computer's empty one its `Config\AINames.cfg` entry, 0x507659);
/// ours for the local player, the lobby's in a network game. [inf] A random map's computers
/// (0x506cf1: always their AINames entry) come out the same, the generated players chunk having
/// no names.
fn chat_names(gpu: &mut Gpu, sim: &SimHandle, map: &std::path::Path) {
    let Some(ui) = gpu.ui.as_mut() else { return };
    let names = &mut ui.state.page_info.chat.names;
    if let Ok(m) = s4_formats::map::MapFile::open(map) {
        for p in m.players().unwrap_or_default() {
            if let Some(n) = names.get_mut(p.id as usize) {
                *n = if p.name.is_empty() { frontend::setup::ai_name(p.id as usize - 1) } else { p.name };
            }
        }
    }
    for (id, n) in &sim.names {
        if let Some(slot) = names.get_mut(*id as usize) {
            *slot = n.clone();
        }
    }
    if let Some(n) = names.get_mut(sim.local as usize) {
        *n = settings::Settings::load().player_name;
    }
}

/// `CGameType::Init`: the network delay (`m_iNetworkTimeDelta`, ms) from `Config\Network.cfg`
/// `[NETWORK] NetworkTimeDelta` (500 in the install; `dt` = 500 / 71 = 7 ticks).
fn network_time_delta() -> u32 {
    let p = s4_formats::path::resolve(s4_formats::game_dir().join("Config").join("Network.cfg"));
    let text = std::fs::read_to_string(p).unwrap_or_default();
    s4_audio::cfg::Cfg::parse(&text).int("NETWORK", "NetworkTimeDelta", 500).max(0) as u32
}

fn setup(a: &Args) -> anyhow::Result<SimSetup> {
    let p = resolve_map(&a.map);
    let campaign = p.to_string_lossy().to_ascii_lowercase().contains("campaign");
    // Game type 4 (0x527c10): a tutorial map; its script is the `Script\` file like a campaign's.
    let is_tutorial = tutorial::is_tutorial_map(&p);
    let shared = tutorial::shared();
    shared.lock().unwrap_or_else(|e| e.into_inner()).active = is_tutorial;
    // `--host` / `--join` with `--load`: a loaded network game (every peer its own save of it).
    let saved = match (&a.load, a.host.is_some() || a.join.is_some()) {
        (Some(f), true) => Some(save::read(std::path::Path::new(f), s4_sim::Rules::load_default()?)?),
        _ => None,
    };
    let p = saved.as_ref().map_or(p, |l| resolve_map(&l.header.map));
    let net = match (a.host, &a.join) {
        (Some(players), _) => Some(sim::NetSetup::Host {
            bind: a.relay.as_ref().map(|r| format!("{}{r}", sim::RELAY_BIND)).or_else(|| a.bind.clone()).unwrap_or_else(|| "0.0.0.0:7710".into()),
            players,
            settings: s4_net::GameSettings {
                map: saved.as_ref().map_or(a.map.clone(), |l| l.header.map.clone()),
                delay_ms: a.delay,
                hash_interval: a.hash_interval,
                map_hash: s4_net::map_hash(&p)?,
                load_tick: saved.as_ref().map_or(0, |l| l.header.tick),
                load_hash: saved.as_ref().map_or(0, |l| l.header.world_hash),
                ..Default::default()
            },
        }),
        (None, Some(h)) => Some(sim::NetSetup::Join {
            bind: a.relay.as_ref().map(|r| format!("{}{r}", sim::RELAY_BIND)).or_else(|| a.bind.clone()).unwrap_or_else(|| "0.0.0.0:0".into()),
            // Through the relay: the host's room number.
            host: match (&a.relay, h.parse::<u16>()) {
                (Some(_), Ok(room)) => s4_net::relay::relay_addr(room),
                _ => h.parse().map_err(|e| anyhow::anyhow!("--join {h}: {e}"))?,
            },
            map_hash: s4_net::map_hash(&p)?,
            save: saved.as_ref().map_or((0, 0), |l| (l.header.world_hash, l.client.local)),
        }),
        _ => None,
    };
    Ok(SimSetup { map_path: p, prefer_file_script: campaign || is_tutorial, run_script: !a.no_script, fog: !a.no_fog, outcome: Default::default(), tutorial: shared, net, players: a.players, load: a.load.clone().map(std::path::PathBuf::from) })
}

fn command_log(a: &Args) -> anyhow::Result<Vec<Stamped>> {
    match &a.commands {
        Some(f) => commands::parse_log(&std::fs::read_to_string(f)?),
        None => Ok(Vec::new()),
    }
}

/// The camera key of a key (physical position: W A S D on any layout).
fn scroll_key(code: KeyCode) -> Option<camera::ScrollKey> {
    use camera::ScrollKey as K;
    Some(match code {
        KeyCode::ArrowLeft => K::Left,
        KeyCode::ArrowRight => K::Right,
        KeyCode::ArrowUp => K::Up,
        KeyCode::ArrowDown => K::Down,
        KeyCode::KeyA => K::A,
        KeyCode::KeyD => K::D,
        KeyCode::KeyW => K::W,
        KeyCode::KeyS => K::S,
        _ => return None,
    })
}

/// An edit field of the GUI (the chat input, a save name) has the keyboard.
fn text_focus(gpu: &Gpu) -> bool {
    gpu.ui.as_ref().is_some_and(|ui| ui.state.focus.is_some_and(|(d, _)| ui.state.open.contains(&d)))
}

/// View centred on a node (or the local player's start).
/// `GameManager +0x144` (`SetSettings` 0x514d30): the extended tooltips setting, forced on in a
/// tutorial (`GlobalGameStateObject +0x3c` = 4).
fn extended_tooltips(st: &settings::Settings, sim: &SimHandle) -> bool {
    st.extended_tooltip || sim.tutorial.lock().unwrap_or_else(|e| e.into_inner()).active
}

fn view_on(scene: &Scene, at: Option<(usize, usize)>, (w, h): (u32, u32), local: u8) -> View {
    let l = local as usize;
    let (x, y) = at.or_else(|| scene.population.as_ref().map(|p| (p.starts[l].0 as usize, p.starts[l].1 as usize))).unwrap_or((0, 0));
    let c = scene.node_world(x, y);
    View { x: (c[0] - w as f32 / 2.0).round(), y: (c[1] - h as f32 / 2.0).round(), zoom: 1.0 }
}

/// The window and map the camera works in (camera.rs).
fn screen(gpu: &Gpu, map: usize) -> camera::Screen {
    camera::Screen { size: gpu.size(), hidpi: client::eff_zoom(gpu, View { x: 0.0, y: 0.0, zoom: 1.0 }), map }
}

/// Zoom the camera by `units` around `at` (the screen centre for `None`) and move the zoom
/// slider to the wanted zoom (0x5199a0 → GUI event 0x25d).
fn zoom_camera(cam: &mut camera::Camera, gpu: &mut Gpu, units: i32, at: Option<(f64, f64)>, map: usize) {
    let s = screen(gpu, map);
    let z = match at {
        Some(p) => cam.zoom_by(units, p, &s),
        None => cam.zoom_by_centre(units, &s),
    };
    let v = client::slider_of_zoom(z);
    client::zoom_slider(gpu, v);
    gpu.zoom_events.push((0x25d, v as i32));
}

/// The zoom slider (dialog 0, knob 0x265: code 2 while it moves, code 3 on release → event
/// 0x12): the camera eases to the slider's zoom around the screen centre.
fn slider_zoom(cam: &mut camera::Camera, gpu: &mut Gpu, events: &[s4_viewer::guistate::UiEvent], map: usize) {
    use s4_viewer::guistate::UiEvent;
    for e in events {
        let v = match *e {
            UiEvent::Slider { dialog: 0, link: 0x265, value } => value,
            UiEvent::Command { dialog: 0, link: 0x265, .. } => match gpu.ui.as_ref().and_then(|ui| ui.state.knob_value(0, 0x265)) {
                Some(v) => v as i32,
                None => continue,
            },
            _ => continue,
        };
        let s = screen(gpu, map);
        let c = (s.size.0 as f64 / 2.0, s.size.1 as f64 / 2.0);
        cam.zoom_to_units(client::zoom_of_slider(v), c, &s);
        gpu.zoom_events.push((0x12, v));
    }
}

/// `CAnimalMgr::Update`'s ambient part for logic tick `tick` (ambient.rs), after that tick's
/// sounds: started once the snapshot brings the land; the tick's spell butterflies first (made
/// in the logic update, before `Animal::Update`); its `AddEffect`s wait in `added`.
#[allow(clippy::too_many_arguments)]
fn ambient_tick<B: kira::backend::Backend>(amb: &mut Option<ambient::Ambient>, scene: &mut Scene, snap: &Arc<Snapshot>, lis: &sound::Listener, tick: u32, events: &[s4_sim::event::SimEvent], snd: Option<&mut sound::GameSound<B>>) {
    if amb.is_none()
        && let Some(land) = &snap.ambient
    {
        *amb = Some(ambient::Ambient::new(land.clone(), ambient_frames(scene)));
    }
    let Some(a) = amb else { return };
    let nodes = ambient::Nodes::of(&snap.population);
    butterflies(a, &nodes, events, tick);
    let centre = sound::centre_node(&snap.population, lis);
    let mut sink = sound::AmbientSink::new(snd, snap, lis);
    a.tick(&nodes, tick, centre, &mut sink);
}

/// The tick's `SimEvent::Butterfly`s (the Maya's spell 5) to the ambient manager.
fn butterflies(a: &mut ambient::Ambient, nodes: &ambient::Nodes, events: &[s4_sim::event::SimEvent], tick: u32) {
    for e in events {
        if let s4_sim::event::SimEvent::Butterfly { x, y } = *e {
            a.butterfly(nodes, x, y, tick);
        }
    }
}

/// A `SimEvent::Projectile` (`CEffects` vtbl+0x18 0x4e3b20 → 0x4e2310) of tick `tick` onto the
/// scene: `SpriteScene::add_projectile` with the kind's record.
fn add_projectile(gpu: &mut Gpu, scene: &mut Scene, tick: u32, e: &s4_sim::event::SimEvent) {
    let s4_sim::event::SimEvent::Projectile { kind, from, to, delay, offset, .. } = *e else { return };
    let Some(k) = s4_viewer::sprites::ProjectileKind::of(kind) else { return };
    let node = |(x, y): (i32, i32)| (x.clamp(0, u16::MAX as i32) as u16, y.clamp(0, u16::MAX as i32) as u16);
    if let Err(err) = scene.add_projectile(gpu, &k, node(from), node(to), delay, offset, tick as u64) {
        tracing::warn!("projectile {kind}: {err}");
    }
}

/// A `SimEvent::TribeEffect` (`CEffects` vtbl+0x14 0x4e3c80 → 0x4e2260, the dying warrior's
/// 0x580bff) of tick `tick` onto the scene, in the settler file of the player's tribe.
fn add_tribe_effect(gpu: &mut Gpu, scene: &mut Scene, pop: &s4_viewer::population::Population, tick: u32, e: &s4_sim::event::SimEvent) {
    let s4_sim::event::SimEvent::TribeEffect { effect, x, y, player, .. } = *e else { return };
    let race = pop.races.get(player as usize).copied().unwrap_or(0);
    let node = |v: i32| v.clamp(0, u16::MAX as i32) as u16;
    if let Err(err) = scene.add_tribe_effect(gpu, effect, node(x), node(y), player, race, tick as u64) {
        tracing::warn!("tribe effect {effect}: {err}");
    }
}

/// The mushroom farms' pulls (`CEffects` vtbl+0x1c 0x4e3b90): effect 0x14 at the mushroom
/// for its frames, then the flight 0x15 (0x4e25c0) from the mushroom − (0, 18) px to the farm −
/// (48, 90) px in `max(1, (int)(√D · 0.2))` ticks (s4-sim `mushfarm::pull_duration`), both at
/// the nodes' `ConvertMapPositionToWorldScreenOffsets`.
fn farm_pulls(gpu: &mut Gpu, scene: &mut Scene, patches: &mut patches::Patches, pop: &s4_viewer::population::Population) {
    let screen = |(x, y): (u16, u16)| {
        let h = pop.height.get(y as usize * pop.size + x as usize).copied().unwrap_or(0);
        (24 * x as i32 - 12 * y as i32, 12 * y as i32 - s4_sim::buildings::mushfarm::height_px(h))
    };
    for (from, to, tick) in patches.pulls(pop) {
        let first = scene.sprites.as_mut().map_or(1, |s| s.job_frame_count(6, 0x14, 0).clamp(1, 255)) as u32;
        let (m, f) = (screen(from), screen(to));
        let (a, b) = ((m.0, m.1 - 18), (f.0 - 48, f.1 - 90));
        let d = ((b.0 - a.0) as i64).pow(2) + ((b.1 - a.1) as i64).pow(2);
        let steps = ((d / 25).isqrt() as u32).max(1);
        let res = scene
            .add_effect(gpu, 0x14, from.0, from.1, tick as u64)
            .and_then(|_| scene.add_flight(gpu, 0x15, from.0, from.1, (a.0 as f32, a.1 as f32), (b.0 as f32, b.1 as f32), steps, (tick + first) as u64));
        if let Err(e) = res {
            tracing::warn!("mushroom pull: {e}");
        }
    }
}

/// The frame counts the ambient records take (`GetJobFrameCount` 0x461fe0), from the scene's
/// gfx files.
fn ambient_frames(scene: &mut Scene) -> ambient::Frames {
    let mut out = ambient::Frames::default();
    let Some(s) = scene.sprites.as_mut() else { return out };
    for (i, n) in out.types.iter_mut().enumerate() {
        *n = s.job_frame_count(6, 0xb + i, 0).clamp(1, 255) as u8;
    }
    out.fish = s.job_frame_count(8, ambient::FISH_JOB as usize, 2).clamp(1, 255) as u8;
    for (i, n) in out.water.iter_mut().enumerate() {
        *n = s.job_frame_count(8, ambient::ANIMAL_JOB[10 + i] as usize, 2).clamp(1, 255) as u8;
    }
    out
}

/// Show `cur` (interpolated from `prev`): entities, animation tick, unit offsets, GPU upload.
/// `base` is the snapshot the sprite scene was last built from; when `cur` places anything
/// different (entities created, removed or moved, territory) the scene follows it and `base`
/// becomes `cur`. Returns whether that happened.
#[allow(clippy::too_many_arguments)]
fn present(
    gpu: &mut Gpu,
    scene: &mut Scene,
    base: &mut Arc<Snapshot>,
    prev: &Snapshot,
    cur: &Arc<Snapshot>,
    alpha: f32,
    markers: &[(u16, u16, u8)],
    ghost: Option<(u16, u16, u8, u8, u8)>,
    selection: &[(u16, u16, u16, u8)],
    patches: &s4_viewer::sprites::ShownPatches,
    ambient: &[s4_viewer::sprites::AmbientSprite],
    offsets: &mut interp::Offsets,
) -> anyhow::Result<bool> {
    // The sprite renderer takes the scene once, after all of the frame's changes.
    if let Some(r) = &mut gpu.sprites {
        r.hold = true;
    }
    let res = (|| {
        let rebuilt = {
            let _t = perf::span("present: compare");
            !interp::same_content(&base.population, &cur.population)
        };
        if rebuilt {
            let _t = perf::span("present: set_population");
            scene.set_population(gpu, cur.population.clone())?;
            *base = cur.clone();
        }
        let _t = perf::span("present: markers, fog");
        if let Some(f) = &cur.fog {
            let _t = perf::span("present: fog");
            scene.set_fog_layer(gpu, &f.gfx, f.fade, f.border);
        }
        place_camera_window(gpu, &cur.population, cur.fog.as_ref());
        {
            let _t = perf::span("present: set_markers");
            scene.set_markers(gpu, markers, ghost, selection)?;
        }
        let t = perf::span("present: set_tick");
        scene.set_tick(gpu, cur.tick as u64);
        drop(t);
        drop(_t);
        let _t = perf::span("present: poses");
        // Units without a draw-time state: interpolated between the snapshots; settlers and
        // vehicles: as the original draws them.
        let t = perf::span("poses: offsets");
        let mut poses: HashMap<u16, Pose> =
            offsets.get(base, prev, cur, alpha).map(|(id, (dx, dy))| (id, Pose { dx, dy, frame: None, turn: None })).collect();
        drop(t);
        let t = perf::span("poses: animator");
        poses.extend(scene.animator.poses(&cur.population, cur.tick as f64 + alpha as f64));
        drop(t);
        if let Some(s) = &scene.sprites {
            poses.extend(scene.animator.vehicle_poses(&cur.population, cur.tick as f64 + alpha as f64, s.vehicle_frames()));
        }
        drop(_t);
        let _t = perf::span("present: sprites");
        if let (Some(s), Some(r)) = (&mut scene.sprites, &mut gpu.sprites) {
            // New patch and ambient frames reach the GPU atlas before they are drawn.
            s.set_patches(patches)?;
            // The manakopters are `CEffects` records too (flyer_gfx.rs).
            let flyers = s4_viewer::flyer_gfx::sprites(&cur.population, cur.tick);
            if flyers.is_empty() {
                s.set_ambient(ambient)?;
            } else {
                s.set_ambient(&[ambient, &flyers].concat())?;
            }
            let t = perf::span("sprites: set_unit_poses");
            s.set_unit_poses(poses);
            drop(t);
            r.sync(&gpu.device, &gpu.queue, s);
        }
        Ok(rebuilt)
    })();
    if let (Some(s), Some(r)) = (&mut scene.sprites, &mut gpu.sprites) {
        r.hold = false;
        r.set_scene(s);
    }
    res
}

/// The camera window's view (dialog 2, s4_viewer::mini_view): 0x4fdc60 runs every logic tick on
/// the state after the last one, which is what `pop` shows.
fn place_camera_window(gpu: &mut Gpu, pop: &s4_viewer::population::Population, fog: Option<&fog::FogFrame>) {
    use s4_viewer::mini_view::Followed;
    if gpu.camera_window.is_none() {
        return;
    }
    let Some(ui) = gpu.ui.as_mut() else { return };
    let size = pop.size;
    let on_board = |flags: u32| flags & s4_sim::entity::flag::ON_BOARD != 0;
    let entity = |id: u16| {
        let unit = pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == id);
        unit.map(|u| Followed { uid: u.uid, on_board: on_board(u.flags), node: (u.x as i32, u.y as i32), walk: Some((u.dir, u.distance)) }).or_else(|| {
            // [inf] Animals' +0x40 / +0x41 are not in the population: drawn on their node.
            let a = pop.animals.iter().find(|a| a.id == id)?;
            Some(Followed { uid: a.uid, on_board: on_board(a.flags), node: (a.x as i32, a.y as i32), walk: None })
        })
    };
    let visible = |x: i32, y: i32| fog.is_none_or(|f| f.is_position_visible(size, x, y));
    let height = |x: i32, y: i32, s: i32| {
        let h = (x >= 0 && y >= 0 && (x as usize) < size).then(|| pop.height.get(y as usize * size + x as usize)).flatten();
        h.map_or(0, |&h| s4_formats::gfx::landscape::height_offset_fixed(h, s << 16) >> 16)
    };
    let p = ui.mini_view.place(&entity, &visible, &height);
    let v = ui.mini_view.view(p);
    if let Some((view, _)) = &mut gpu.camera_window {
        *view = v;
    }
}

/// Everything a mouse click touches.
struct Input<'a> {
    gpu: &'a mut Gpu,
    scene: &'a mut Scene,
    view: View,
    sim: &'a SimHandle,
    build: &'a mut build::BuildMode,
    pointer: &'a mut select::Pointer,
    bdlg: &'a mut bdlg::BuildingDialog,
    pages: &'a mut pages::Pages,
}

impl Input<'_> {
    /// The GUI's events (of a click, or of the tutorial's `PressButton`): the build mode, the
    /// tutorial, the net commands, the building dialog and the pages.
    fn ui_events(&mut self, events: &[s4_viewer::guistate::UiEvent]) {
        self.build.ui(events, self.sim);
        tutorial::ui_events(&self.sim.tutorial, events);
        msgwin::tutorial_events(self.gpu, &self.sim.tutorial);
        // UI event 0x192 (the main and submenu tabs): state 0 from every state; 0x4edb10 →
        // `sub_EDC10` ends the build mode (+0x50 = 0, markers off). The selection, the sidebar
        // and the dialog modes go in bdlg.rs.
        if events.iter().any(|e| matches!(*e, s4_viewer::guistate::UiEvent::Command { dialog, link, .. } if input::sends_192(dialog, link))) {
            self.build.cancel(self.gpu, self.sim);
            // 0x4edac0 also empties `GoodDistributionInfoHolder` (the goods race's refresh).
            if let Some(ui) = self.gpu.ui.as_mut() {
                ui.race_holder = None;
            }
        }
        for e in events {
            if let Some(c) = commands::ui_command(e) {
                self.sim.command(Stamped { tick: execution_tick(self.sim.snapshot().tick), command: s4_sim::PlayerCommand { player: self.build.player, command: c } });
            }
        }
        self.bdlg.ui(events, self.gpu, self.sim, self.pointer, self.build.building.is_some());
        self.eyecatchers(events);
        self.mini_view(events);
        let state = self.input_state();
        self.pages.ui(events, self.gpu, self.scene, self.view, self.sim, self.pointer, state);
    }

    /// The tutorial's requests to the GUI this frame (msgwin.rs): `PressButton`'s events as if
    /// clicked, `SelectNextBuilding` as UI event 0x198.
    /// The town page's eye catchers toggle (0x473430 code 3 on 0x224, s4_viewer::eyecatcher_sidebar):
    /// pressed → UI 0x1ca (handler 0x4ebc60, states 0 / 4 → 0: a build mode ends) opens the
    /// sidebar; released → 0x1cc (no transition outside states 1 / 8), GUI event 0x260 (the
    /// sidebar closed) and 0x191 (the build mode left).
    fn eyecatchers(&mut self, events: &[s4_viewer::guistate::UiEvent]) {
        use s4_viewer::eyecatcher_sidebar::TOGGLE;
        for e in events {
            let s4_viewer::guistate::UiEvent::Command { dialog, link, effects } = *e else { continue };
            if (dialog, link) != TOGGLE {
                continue;
            }
            let state = self.input_state();
            if effects & 1 != 0 {
                if input::transition(state, 0x1ca).is_none() {
                    continue;
                }
                self.build.cancel(self.gpu, self.sim);
                let snap = self.sim.snapshot();
                if self.gpu.ui.as_mut().is_some_and(|ui| ui.open_eyecatchers(&snap.population)) {
                    client::recompose(self.gpu);
                }
            } else {
                if let Some(ui) = self.gpu.ui.as_mut() {
                    ui.close_sidebar();
                }
                if input::transition(state, 0x191).is_some() {
                    self.build.cancel(self.gpu, self.sim);
                }
                client::recompose(self.gpu);
            }
        }
    }

    /// Dialog 2, the camera window (s4_viewer::mini_view, procedure 0x47c870): 0x282 → UI 0x1e7
    /// (handler 0x4eeb90: state 10, the pick mode), the scale buttons, and 0x282's right click
    /// (code 7) → event 0x13 to the followed entity or the shown node.
    fn mini_view(&mut self, events: &[s4_viewer::guistate::UiEvent]) {
        use s4_viewer::guistate::UiEvent;
        use s4_viewer::mini_view::{DIALOG, PICK_EVENT};
        for e in events {
            match *e {
                UiEvent::Command { dialog: DIALOG, link, .. } => {
                    let state = self.input_state();
                    let Some(ui) = self.gpu.ui.as_mut() else { continue };
                    if ui.mini_view.command(link) != Some(PICK_EVENT) || input::transition(state, PICK_EVENT).is_none() {
                        continue;
                    }
                    // 0x4eeb90 only sets mode +0x50 = 10: the modes of states 3, 4, 7 and 8 end
                    // with it [inf].
                    self.bdlg.cancel();
                    self.bdlg.spell_mode = false;
                    if self.build.building.is_some() {
                        self.build.cancel(self.gpu, self.sim);
                    }
                    self.bdlg.mini_pick = true;
                }
                UiEvent::Context { dialog: DIALOG, link } => {
                    let snap = self.sim.snapshot();
                    let pop = &snap.population;
                    let entity = |id: u16| {
                        let node = |x: u16, y: u16| (x as i32, y as i32);
                        pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == id).map(|u| (u.uid, node(u.x, u.y))).or_else(|| pop.animals.iter().find(|a| a.id == id).map(|a| (a.uid, node(a.x, a.y))))
                    };
                    let Some(ui) = self.gpu.ui.as_mut() else { continue };
                    if let Some((x, y)) = ui.mini_view.context(link, &entity) {
                        self.pages.centre = Some((x as u16, y as u16));
                    }
                }
                _ => {}
            }
        }
    }

    /// State 10's map click at `node` (UI 0x1dd → 0x4eeba0, then state 1): of the map objects on
    /// the 19 spiral nodes around it, the animals and the local player's settlers and vehicles go
    /// to `GetTheBestEntity` at the node's screen point; the camera window follows the one it
    /// picks, else shows the node. [gap] Animals have no box here (only settlers and vehicles do,
    /// `client::local_boxes`), so they are never picked.
    fn mini_pick(&mut self, node: (usize, usize)) {
        use s4_viewer::selection::Candidate;
        self.bdlg.mini_pick = false;
        self.bdlg.held_one = true;
        let snap = self.sim.snapshot();
        let pop = &snap.population;
        let local = self.pointer.player;
        let mut by_node = std::collections::HashMap::new();
        for u in pop.settlers.iter().chain(&pop.vehicles) {
            by_node.insert((u.x, u.y), Candidate { id: u.id, x: u.x, y: u.y, obj: u.obj, kind: u.kind, flags: u.flags, owner: u.owner });
        }
        for a in &pop.animals {
            by_node.entry((a.x, a.y)).or_insert(Candidate { id: a.id, x: a.x, y: a.y, obj: 0x80, kind: a.kind, flags: a.flags, owner: 0 });
        }
        let (x, y) = (node.0 as i32, node.1 as i32);
        let mut occupied = Vec::new();
        let mut objects = Vec::new();
        for i in 0..=s4_sim::spiral::last(2) {
            let (px, py) = (x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i));
            if px < 0 || py < 0 || px as usize >= pop.size || py as usize >= pop.size {
                continue;
            }
            if let Some(c) = by_node.get(&(px as u16, py as u16)) {
                occupied.push((px, py));
                objects.push(*c);
            }
        }
        let candidates = s4_viewer::mini_view::candidates(&objects, local);
        let boxes = client::local_boxes(self.scene, pop, local);
        let scene = &*self.scene;
        let screen = |x: u16, y: u16| {
            let a = scene.node_world(x as usize, y as usize);
            (a[0], a[1])
        };
        let best = if candidates.is_empty() { None } else { s4_viewer::mini_view::best(&candidates, screen(node.0 as u16, node.1 as u16), &screen, &|c| boxes.get(&c.id).copied()) };
        let uid = |id: u16| pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == id).map(|u| u.uid).or_else(|| pop.animals.iter().find(|a| a.id == id).map(|a| a.uid)).unwrap_or(0);
        if let Some(ui) = self.gpu.ui.as_mut() {
            ui.mini_view.pick((x, y), &occupied, best.map(|id| (id, uid(id))));
        }
    }

    /// Also the dialogs' deferred sidebar press (0x12938c7, `Gui::deferred_events`).
    fn tutorial_frame(&mut self, tf: msgwin::TutorialFrame) {
        self.ui_events(&tf.events);
        let deferred = self.gpu.ui.as_mut().map(|ui| std::mem::take(&mut ui.deferred_events)).unwrap_or_default();
        if !deferred.is_empty() {
            self.ui_events(&deferred);
        }
        for kind in tf.next_building {
            self.bdlg.select_next_building(self.gpu, self.sim, self.pointer, kind);
        }
    }

    /// Left button down / up at screen point `p`: the GUI first; on the map, a build-mode click,
    /// else a selection click or box. Returns the GUI's events.
    fn left(&mut self, p: (f64, f64), down: bool) -> Vec<s4_viewer::guistate::UiEvent> {
        let what = if down { GuiMouse::LeftDown } else { GuiMouse::LeftUp };
        if down {
            msgwin::tooltip_press(self.gpu);
        }
        let state = self.input_state();
        if let Some(ui) = self.gpu.ui.as_mut() {
            ui.state.input_state = state;
        }
        let (events, over) = client::gui_mouse(self.gpu, p, what);
        self.ui_events(&events);
        if down && !over && self.bdlg.mini_pick {
            if let Some(node) = client::hover_node(self.gpu, self.scene, self.view, (p.0 as f32, p.1 as f32)) {
                self.mini_pick(node);
            }
        } else if down && !over && self.bdlg.in_mode() {
            if let Some(node) = client::hover_node(self.gpu, self.scene, self.view, (p.0 as f32, p.1 as f32))
                && !self.bdlg.target_click(self.sim, self.pointer, node)
                && !self.bdlg.map_click(self.gpu, self.sim, self.pointer.player, node)
            {
                self.bdlg.trade_click(self.gpu, self.sim, self.pointer.player, node);
            }
        } else if down && !over && self.build.building.is_some() {
            if let Some(node) = client::hover_node(self.gpu, self.scene, self.view, (p.0 as f32, p.1 as f32)) {
                let snap = self.sim.snapshot();
                self.build.click(node, &snap, self.gpu, self.sim);
            }
        } else if down && !over {
            // The map click ends in 0x1dd / 0x1de (state 1) or 0x191 (state 0).
            self.bdlg.held_one = false;
            self.bdlg.spell_mode = false;
            self.pointer.press(self.gpu, p);
        } else if !down {
            let snap = self.sim.snapshot();
            let scale = self.gpu.gui_scale as f64;
            self.pointer.release(self.gpu, self.scene, self.view, &snap.population, p, scale);
        }
        events
    }

    /// `CInputProcessor`'s state for the client's modes (input.rs).
    fn input_state(&self) -> u8 {
        self.bdlg.state_of(self.build.building.is_some(), self.pointer)
    }

    /// Right button down: the GUI's. Returns whether the cursor is over the GUI (else the map
    /// gets the button: camera.rs decides between a drag and a click).
    fn right_down(&mut self, p: (f64, f64)) -> bool {
        tutorial::right_down(&self.sim.tutorial);
        let (events, over) = client::gui_mouse(self.gpu, p, GuiMouse::RightDown);
        // Code 7 (`UiEvent::Context`) reaches the procedures' handlers: the build pages' go-to,
        // D55's right click.
        let state = self.input_state();
        if let Some(ui) = self.gpu.ui.as_mut() {
            ui.state.input_state = state;
        }
        self.ui_events(&events);
        over
    }

    /// A right click on the map (GUI event 0x1e0 at the press point, sent when the button is
    /// released without a drag): leave the dialog's mode or the build mode, else send the
    /// selection.
    fn right_click(&mut self, p: (f64, f64)) {
        let node = client::hover_node(self.gpu, self.scene, self.view, (p.0 as f32, p.1 as f32));
        self.right_click_node(node);
    }

    /// GUI event 0x1e0 with `node` (the map's or the minimap's, EventManagement cases 10 / 9).
    fn right_click_node(&mut self, node: Option<(usize, usize)>) {
        if let Some(n) = node
            && self.bdlg.cast(self.sim, self.pointer, n)
        {
            return;
        }
        if self.bdlg.cancel() {
            return;
        }
        if self.build.building.is_some() {
            self.build.cancel(self.gpu, self.sim);
        } else if let Some(node) = node {
            let snap = self.sim.snapshot();
            let had = !self.pointer.selection.ids.is_empty();
            let ev = self.pointer.send(self.gpu, &snap.population, node);
            // 0x1e0 keeps state 1 also when `SendTo` empties the selection.
            if had && self.pointer.selection.ids.is_empty() {
                self.bdlg.held_one = true;
            }
            for e in ev {
                if let Some(c) = commands::ui_command(&e) {
                    tracing::info!("command: {c:?}");
                    self.sim.command(Stamped { tick: execution_tick(snap.tick), command: s4_sim::PlayerCommand { player: self.pointer.player, command: c } });
                }
            }
        }
    }
}

/// A scripted input action (`--input`).
enum Action {
    Click(f64, f64),
    RClick(f64, f64),
    /// Right button down / up at a screen point, and a cursor move (camera.rs: drag or click).
    RPress(f64, f64),
    RRelease,
    Move(f64, f64),
    Hover(f64, f64),
    /// Left button down / up at a screen point (a GUI drag: `press`, `hover`…, `release`).
    Press(f64, f64),
    Release(f64, f64),
    /// Mouse wheel notches (+ up), and an arrow key held (direction x, y) for some ms
    /// (`arrow DX DY [MS]`, default 250; camera.rs `Camera::hold`).
    Wheel(i32),
    Arrow(i32, i32, u32),
    ClickControl(usize, u16),
    HoverControl(usize, u16),
    ClickNode(usize, usize),
    HoverNode(usize, usize),
    /// Text typed into the focused edit field (`\r` = return).
    Type(String),
    /// Left click with Ctrl held (append to the selection).
    CtrlClickNode(usize, usize),
    RClickNode(usize, usize),
    /// Left drag from one node's screen point to another's (box selection).
    DragNodes(usize, usize, usize, usize),
    /// Left button down at a node's screen point, the cursor moved there with the button down,
    /// left button up there (a drag in steps, to capture the box while it is shown).
    PressNode(usize, usize),
    MoveNode(usize, usize),
    ReleaseNode(usize, usize),
    /// The mouse wheel at a screen point (`delta` as WM_MOUSEWHEEL's, 120 a notch up): the GUI's
    /// if a dialog is there, else the zoom.
    WheelAt(f64, f64, i32),
    /// The Enter key (the chat input, `IngameKeybindings` 0x4890b0).
    Enter,
    /// The space bar pressed and released (the tutorial's `SPACE`).
    Space,
    /// F7: the clock (`HandleF6_F7_Esc` 0x489440, s4_viewer::clock).
    F7,
    /// The camera centred on a node (as event 0x13).
    ViewNode(usize, usize),
}

fn input_script(a: &Args) -> anyhow::Result<Vec<(u32, Action)>> {
    let Some(f) = &a.input else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for (ln, line) in std::fs::read_to_string(f)?.lines().enumerate() {
        let w: Vec<&str> = line.split('#').next().unwrap_or("").split_whitespace().collect();
        if w.is_empty() {
            continue;
        }
        let bad = || anyhow::anyhow!("{f}:{}: bad action", ln + 1);
        let n = |i: usize| -> anyhow::Result<f64> { w.get(i).ok_or_else(bad)?.parse::<f64>().map_err(|_| bad()) };
        let act = match *w.get(1).ok_or_else(bad)? {
            "click" => Action::Click(n(2)?, n(3)?),
            "rclick" => Action::RClick(n(2)?, n(3)?),
            "rpress" => Action::RPress(n(2)?, n(3)?),
            "rrelease" => Action::RRelease,
            "move" => Action::Move(n(2)?, n(3)?),
            "hover" => Action::Hover(n(2)?, n(3)?),
            "press" => Action::Press(n(2)?, n(3)?),
            "wheel" => Action::Wheel(n(2)? as i32),
            "arrow" => Action::Arrow(n(2)? as i32, n(3)? as i32, if w.len() > 4 { n(4)? as u32 } else { 250 }),
            "release" => Action::Release(n(2)?, n(3)?),
            "click-control" => Action::ClickControl(n(2)? as usize, n(3)? as u16),
            "hover-control" => Action::HoverControl(n(2)? as usize, n(3)? as u16),
            "click-node" => Action::ClickNode(n(2)? as usize, n(3)? as usize),
            "hover-node" => Action::HoverNode(n(2)? as usize, n(3)? as usize),
            "ctrl-click-node" => Action::CtrlClickNode(n(2)? as usize, n(3)? as usize),
            "type" => Action::Type(w.get(2).map(|s| s.replace("\\r", "\r").replace('_', " ")).unwrap_or_default()),
            "rclick-node" => Action::RClickNode(n(2)? as usize, n(3)? as usize),
            "drag-nodes" => Action::DragNodes(n(2)? as usize, n(3)? as usize, n(4)? as usize, n(5)? as usize),
            "press-node" => Action::PressNode(n(2)? as usize, n(3)? as usize),
            "move-node" => Action::MoveNode(n(2)? as usize, n(3)? as usize),
            "release-node" => Action::ReleaseNode(n(2)? as usize, n(3)? as usize),
            "wheel-at" => Action::WheelAt(n(2)?, n(3)?, n(4)? as i32),
            "enter" => Action::Enter,
            "space" => Action::Space,
            "f7" => Action::F7,
            "view-node" => Action::ViewNode(n(2)? as usize, n(3)? as usize),
            _ => return Err(bad()),
        };
        out.push((n(0)? as u32, act));
    }
    out.sort_by_key(|a| a.0);
    Ok(out)
}

/// Screen point of a map node (its anchor, with height) in `view`.
fn node_screen(scene: &Scene, view: View, (x, y): (usize, usize)) -> (f64, f64) {
    let c = scene.node_world(x, y);
    (((c[0] - view.x) * view.zoom) as f64, ((c[1] - view.y) * view.zoom) as f64)
}

fn headless(a: &Args) -> anyhow::Result<()> {
    let net = a.host.is_some() || a.join.is_some();
    let sim = SimHandle::spawn(setup(a)?, Clock::Manual)?;
    let mut offsets = interp::Offsets::default();
    // Single player: the log is stamped. Network: each command is issued when the game reaches
    // its tick (the lockstep stamps it with the local player's next turn).
    let mut log = command_log(a)?.into_iter().peekable();
    if !net {
        for s in log.by_ref() {
            sim.command(s);
        }
    }
    let advance = |target: u32, log: &mut std::iter::Peekable<std::vec::IntoIter<Stamped>>, on_step: &mut dyn FnMut()| {
        let mut have = sim.snapshot().tick;
        while have < target {
            while let Some(s) = log.next_if(|s| s.tick <= have + 2) {
                sim.command(s);
            }
            let n = if log.peek().is_some() || a.sound { 1 } else { target - have };
            let reached = sim.step(n);
            if reached == have {
                break; // the network game stopped
            }
            have = reached;
            on_step();
        }
    };
    let mut scene = Scene::load(Some(&a.map), true, false)?;
    let mut gpu = Gpu::headless(a.width, a.height)?;
    gpu.effects.clouds = a.clouds;
    scene.upload(&mut gpu);
    if let Some(sc) = a.gui {
        gpu.gui_scale = sc;
        client::attach_gui(&mut gpu, &scene, sim.local)?;
        chat_names(&mut gpu, &sim, &resolve_map(&a.map));
    }
    let mut view = view_on(&scene, a.at, (a.width, a.height), sim.local);
    client::zoom_slider_start(&mut gpu, view);
    let mut st = settings::Settings::load();
    options::init(&mut gpu, &st);
    options::apply_graphics(&mut gpu, &st);
    // `GameManager +0x144`: forced on in a tutorial (`SetSettings` 0x514d30).
    let tutorial_game = sim.tutorial.lock().unwrap_or_else(|e| e.into_inner()).active;
    let mut camera = camera::Camera::new(view, st.border_scroll, st.scroll_step);
    let mut camera_tick = sim.snapshot().tick;
    // The first snapshot (tick 0) is the world as loaded.
    let mut base = sim.snapshot();
    let mut prev = base.clone();
    let mut captures: Vec<u32> = a.capture_at.clone();
    captures.sort();
    let mut actions = input_script(a)?.into_iter().peekable();
    let mut build = build::BuildMode::new(sim.local);
    let mut pointer = select::Pointer::new(sim.local);
    let mut bdlg = bdlg::BuildingDialog::default();
    let mut pages = pages::Pages::default();
    let mut save_menu = menu::SaveMenu::default();
    let mut cursor: Option<(f64, f64)> = None;
    let started = Instant::now();
    let mut counts = EventCounts { patches: patches::Patches::load(1), ambient_frames: ambient_frames(&mut scene), ..Default::default() };
    let st_msg = settings::Settings::load();
    counts.msg = Some((a.messages_for.unwrap_or(sim.local), messages::Messages::new(messages::Classes::load(), st_msg.msg_level_mask, st_msg.popup_all_msgs), 0));
    // `--sound`: the camera as the sound sees it.
    let listener = |gpu: &Gpu, view: View| sound::Listener { camera: view, draw_zoom: client::eff_zoom(gpu, view), size: (a.width, a.height) };
    let mut console = msgwin::Console::default();
    let mut snd = if a.sound { Some(HeadlessSound::new(sim.snapshot().population.races.get(sim.local as usize).copied().unwrap_or(0))?) } else { None };
    for &t in &captures {
        loop {
            let at = actions.peek().map(|x| x.0).filter(|&x| x <= t);
            let target = at.unwrap_or(t);
            let lis = listener(&gpu, view);
            advance(target, &mut log, &mut || count_events(&sim, &mut counts, snd.as_mut(), &lis));
            // The camera's time: 71 ms per logic step here (border scroll, a zoom under way).
            let now_tick = sim.snapshot().tick;
            let map = scene.source.w;
            for _ in camera_tick..now_tick {
                camera.update(0.071, &screen(&gpu, map));
            }
            camera.settle(&screen(&gpu, map));
            camera_tick = now_tick;
            view = camera.view();
            let tf = msgwin::tutorial(&mut gpu, &sim.tutorial, st.extended_tooltip || tutorial_game);
            Input { gpu: &mut gpu, scene: &mut scene, view, sim: &sim, build: &mut build, pointer: &mut pointer, bdlg: &mut bdlg, pages: &mut pages }.tutorial_frame(tf);
            tutorial::publish_selection(&sim.tutorial, &pointer.selection.ids);
            let (v, exit) = tutorial::client_frame(&sim.tutorial, &mut gpu, &mut scene, view, now_tick as u64);
            if v != view {
                camera.jump(v, &screen(&gpu, scene.source.w));
                view = camera.view();
            }
            if exit {
                println!("tick {}: the tutorial ended (Tutorial.Exit)", sim.snapshot().tick);
            }
            // The clock's timer event 0x15 (s4_viewer::clock; its 14-tick step decides here).
            if let Some(ui) = gpu.ui.as_mut()
                && ui.clock.timer(&mut ui.state, now_tick as u32)
            {
                client::recompose(&mut gpu);
            }
            let Some((_, act)) = at.and_then(|_| actions.next()) else { break };
            let mut menu_events: Vec<s4_viewer::guistate::UiEvent> = Vec::new();
            let mut inp = Input { gpu: &mut gpu, scene: &mut scene, view, sim: &sim, build: &mut build, pointer: &mut pointer, bdlg: &mut bdlg, pages: &mut pages };
            match act {
                Action::Click(x, y) => {
                    if let Some(m) = client::minimap_point(inp.gpu, (x, y)) {
                        camera.jump(client::view_from_minimap(inp.gpu, view, m, inp.scene.source.w), &screen(inp.gpu, inp.scene.source.w));
                    }
                    menu_events.extend(inp.left((x, y), true));
                    menu_events.extend(inp.left((x, y), false));
                }
                Action::WheelAt(x, y, d) => {
                    // The GUI first (0x672f90 event 0xe: code 10 to the dialog there), else zoom.
                    cursor = Some((x, y));
                    let ev = client::gui_wheel(inp.gpu, (x, y), d);
                    if ev.is_empty() {
                        zoom_camera(&mut camera, inp.gpu, camera::wheel_units(d), Some((x, y)), inp.scene.source.w);
                        camera.settle(&screen(inp.gpu, inp.scene.source.w));
                    }
                    inp.ui_events(&ev);
                    menu_events.extend(ev);
                }
                Action::Type(ref t) => {
                    let chat = inp.gpu.ui.as_ref().is_some_and(|u| u.msg_dialog.is_some() && u.state.focus.is_some_and(|f| f.1 == s4_viewer::msg_window::CHAT_INPUT));
                    let ev = inp.gpu.ui.as_mut().map(|u| u.state.type_text(t, if chat { 299 } else { 16 })).unwrap_or_default();
                    msgwin::chat_events(&ev, inp.gpu, &sim, &mut console, counts.msg.as_mut().map(|m| &mut m.1));
                    menu_events.extend(ev);
                }
                Action::Enter => {
                    if let Some((_, msgs, _)) = &mut counts.msg {
                        msgwin::chat_key(inp.gpu, msgs, st.extended_tooltip || tutorial_game);
                    }
                }
                Action::Space => {
                    tutorial::space(&sim.tutorial, true);
                    tutorial::space(&sim.tutorial, false);
                }
                Action::F7 => {
                    let screen_w = (inp.gpu.size().0 as f32 / inp.gpu.gui_scale.max(0.1)) as u32;
                    if let Some(ui) = inp.gpu.ui.as_mut() {
                        ui.clock.key(&mut ui.state, screen_w);
                    }
                    client::recompose(inp.gpu);
                }
                Action::ViewNode(x, y) => camera.jump(client::view_on_node(inp.gpu, view, (x, y), inp.scene.source.w), &screen(inp.gpu, inp.scene.source.w)),
                Action::RClick(x, y) => {
                    if let Some(m) = client::minimap_point(inp.gpu, (x, y)) {
                        let node = client::minimap_order_node(inp.gpu, view, m, inp.scene.source.w);
                        inp.right_click_node(Some(node));
                    } else if !inp.right_down((x, y)) {
                        inp.right_click((x, y));
                    }
                }
                Action::RPress(x, y) => {
                    if !inp.right_down((x, y)) {
                        camera.right_down((x, y), &screen(inp.gpu, inp.scene.source.w));
                    }
                    cursor = Some((x, y));
                }
                Action::RRelease => {
                    if let Some(p) = camera.right_up() {
                        inp.right_click(p);
                    }
                }
                Action::Move(x, y) => {
                    let held = camera.right_held();
                    camera.moved((x, y), &screen(inp.gpu, inp.scene.source.w), held);
                    cursor = Some((x, y));
                }
                Action::Hover(x, y) => cursor = Some((x, y)),
                Action::Press(x, y) => {
                    cursor = Some((x, y));
                    menu_events.extend(inp.left((x, y), true));
                }
                Action::Wheel(n) => {
                    zoom_camera(&mut camera, inp.gpu, camera::wheel_units(n * 120), None, inp.scene.source.w);
                    camera.settle(&screen(inp.gpu, inp.scene.source.w));
                }
                Action::Arrow(dx, dy, ms) => camera.hold((dx, dy), ms as f32 / 1000.0, &screen(inp.gpu, inp.scene.source.w)),
                Action::Release(x, y) => {
                    cursor = Some((x, y));
                    menu_events.extend(inp.left((x, y), false));
                }
                Action::ClickControl(d, l) => {
                    let p = client::control_point(inp.gpu, d, l).ok_or_else(|| anyhow::anyhow!("no control {l} in dialog {d}"))?;
                    menu_events.extend(inp.left(p, true));
                    menu_events.extend(inp.left(p, false));
                }
                Action::HoverControl(d, l) => {
                    cursor = Some(client::control_point(inp.gpu, d, l).ok_or_else(|| anyhow::anyhow!("no control {l} in dialog {d}"))?);
                }
                Action::ClickNode(x, y) => {
                    let p = node_screen(inp.scene, view, (x, y));
                    inp.left(p, true);
                    inp.left(p, false);
                }
                Action::HoverNode(x, y) => cursor = Some(node_screen(inp.scene, view, (x, y))),
                Action::CtrlClickNode(x, y) => {
                    let p = node_screen(inp.scene, view, (x, y));
                    inp.pointer.ctrl = true;
                    inp.left(p, true);
                    inp.left(p, false);
                    inp.pointer.ctrl = false;
                }
                Action::RClickNode(x, y) => {
                    let p = node_screen(inp.scene, view, (x, y));
                    if !inp.right_down(p) {
                        inp.right_click(p);
                    }
                }
                Action::DragNodes(x0, y0, x1, y1) => {
                    let (a, b) = (node_screen(inp.scene, view, (x0, y0)), node_screen(inp.scene, view, (x1, y1)));
                    inp.left(a, true);
                    let scale = inp.gpu.gui_scale as f64;
                    inp.pointer.moved(inp.gpu, b, scale);
                    inp.left(b, false);
                }
                Action::PressNode(x, y) => {
                    let p = node_screen(inp.scene, view, (x, y));
                    inp.left(p, true);
                }
                Action::MoveNode(x, y) => {
                    let p = node_screen(inp.scene, view, (x, y));
                    cursor = Some(p);
                    let scale = inp.gpu.gui_scale as f64;
                    inp.pointer.moved(inp.gpu, p, scale);
                }
                Action::ReleaseNode(x, y) => {
                    let p = node_screen(inp.scene, view, (x, y));
                    inp.left(p, false);
                }
            }
            let clicks = std::mem::take(&mut pointer.sounds);
            if !clicks.is_empty() {
                println!("selection sounds {clicks:?}");
            }
            if let Some(s) = snd.as_mut() {
                s.snd.ui(&clicks);
            }
            slider_zoom(&mut camera, &mut gpu, &menu_events, scene.source.w);
            camera.settle(&screen(&gpu, scene.source.w));
            client::minimap_toggles(&mut scene, &menu_events);
            let ch = options::ui(&menu_events, &mut gpu, &mut st, counts.msg.as_mut().map(|m| &mut m.1), false);
            if ch.game {
                camera.border = st.border_scroll;
                camera.step = st.scroll_step;
            }
            // The message window (msgwin.rs), then the load / save submenu (menu.rs).
            if let Some((_, msgs, _)) = &mut counts.msg {
                let (centre, _) = msgwin::ui(&menu_events, &mut gpu, msgs, st.extended_tooltip || tutorial_game);
                if let Some((x, y)) = centre {
                    camera.jump(client::view_on_node(&gpu, camera.view(), (x as usize, y as usize), scene.source.w), &screen(&gpu, scene.source.w));
                }
            }
            view = camera.view();
            if let Some(menu::MenuAction::Save(name)) = save_menu.events(&mut gpu, &std::mem::take(&mut menu_events))
                && let Some(path) = menu::file_for(&name)
            {
                let client = save::ClientState { view: (view.x, view.y, view.zoom), local: sim.local, campaign: None, speed: 1, logos: Vec::new(), net: None };
                let (w, h) = gpu.size();
                let thumb = capture_game(&mut gpu, view).map(|rgba| save::thumbnail(&rgba, w, h)).unwrap_or_default();
                match sim.save(&path, &name, client, thumb) {
                    Ok(h) => {
                        println!("saved \"{name}\" at tick {} to {}", h.tick, path.display());
                        save_menu.init(&mut gpu);
                    }
                    Err(e) => println!("save failed: {e}"),
                }
            }
            bdlg.frame(&mut gpu, &sim, &pointer);
            // The settler search centres the view on what it found (event 0x13).
            if let Some((x, y)) = pages.centre.take() {
                camera.jump(client::view_on_node(&gpu, view, (x as usize, y as usize), scene.source.w), &screen(&gpu, scene.source.w));
            }
            client::recompose(&mut gpu);
            if let Some(p) = cursor {
                let (ev, _) = client::gui_mouse(&mut gpu, p, GuiMouse::Move);
                slider_zoom(&mut camera, &mut gpu, &ev, scene.source.w);
                camera.settle(&screen(&gpu, scene.source.w));
                options::ui(&ev, &mut gpu, &mut st, counts.msg.as_mut().map(|m| &mut m.1), false);
                if let Some((_, msgs, _)) = &mut counts.msg {
                    msgwin::ui(&ev, &mut gpu, msgs, st.extended_tooltip || tutorial_game);
                }
            }
            view = camera.view();
            let calls = sound::drain_gui_calls(&mut gpu);
            if let Some(s) = snd.as_mut() {
                s.snd.gui(sound::gui::Procs::Game, &calls);
            }
        }
        let cur = sim.snapshot();
        let hover = cursor.and_then(|p| client::hover_node(&mut gpu, &scene, view, (p.0 as f32, p.1 as f32)));
        let mut markers = build.display(&cur, hover);
        markers.extend(bdlg.markers(&cur));
        let ghost = build.ghost(&cur, hover);
        let (sel_icons, _) = client::selection_frame(&mut gpu, &cur.population, &mut pointer.selection, pointer.player);
        bdlg.frame(&mut gpu, &sim, &pointer);
        for (tick, effect, x, y) in counts.pending.drain(..) {
            scene.add_effect(&mut gpu, effect, x as u16, y as u16, tick as u64)?;
        }
        for (tick, e) in counts.flights.drain(..) {
            add_projectile(&mut gpu, &mut scene, tick, &e);
            add_tribe_effect(&mut gpu, &mut scene, &cur.population, tick, &e);
        }
        farm_pulls(&mut gpu, &mut scene, &mut counts.patches, &cur.population);
        for (effect, x, y, tick) in counts.ambient.as_mut().map(|a| std::mem::take(&mut a.added)).unwrap_or_default() {
            scene.add_effect(&mut gpu, effect as u8, x as u16, y as u16, tick as u64)?;
        }
        let ambient = counts.ambient.as_ref().map(|a| a.sprites(cur.tick)).unwrap_or_default();
        let rebuilt = present(&mut gpu, &mut scene, &mut base, &prev, &cur, interp::alpha(&cur, Instant::now()), &markers, ghost, &sel_icons, &counts.patches.shown(), &ambient, &mut offsets)?;
        if let Some((_, msgs, _)) = &mut counts.msg {
            let own = st.player_name.clone();
            msgwin::chat_arrivals(&sim, msgs, &cur.population.teams, &|p| msgwin::player_name(&sim, &own, p));
            msgwin::script_messages(&gpu, &sim.tutorial, msgs, sim.local, &|p| msgwin::player_name(&sim, &own, p));
            msgwin::sync(&mut gpu, msgs, st.extended_tooltip || tutorial_game);
        }
        msgwin::tooltip(&mut gpu, cursor);
        if a.gui.is_some() {
            client::update_gui_pages(&mut gpu, &scene, view, &cur.population);
            // [inf] Many frames pass between two headless draws: a full minimap cycle.
            client::update_minimap(&mut gpu, &mut scene, view, 64);
        }
        count_events(&sim, &mut counts, snd.as_mut(), &listener(&gpu, view));
        match sim.outcome.load(std::sync::atomic::Ordering::Relaxed) {
            0 => {}
            o => println!("tick {}: the script decided the game: {}", cur.tick, if o == script::WON { "won" } else { "lost" }),
        }
        gpu.cloud_time = s4_viewer::clouds::seconds(cur.tick as f64);
        let rgba = gpu.capture(view)?;
        let path = format!("{}_t{:05}.png", a.out, cur.tick);
        client::write_png(std::path::Path::new(&path), a.width, a.height, &rgba)?;
        let (p, st) = (&cur.population, scene.sprites.as_ref().map(|s| s.stats.clone()).unwrap_or_default());
        println!(
            "tick {:5}: hash {:016x}; {} buildings ({} sites), {} settlers outside, {} vehicles, {} piles, {} decos, {} animals{}; {} markers; scene {}: {} unique images, {} atlas layers; captured {path}",
            cur.tick,
            cur.hash,
            p.buildings.len(),
            p.buildings.iter().filter(|b| b.site.is_some()).count(),
            p.settlers.len(),
            p.vehicles.len(),
            p.piles.len(),
            p.decos.len(),
            p.animals.len(),
            p.animals.first().map_or(String::new(), |a| format!(" (first at {},{})", a.x, a.y)),
            markers.len(),
            if rebuilt { "rebuilt" } else { "unchanged" },
            st.unique_sprites,
            st.atlas_layers
        );
        prev = cur;
    }
    if a.sound {
        let lis = listener(&gpu, view);
        advance(a.ticks, &mut log, &mut || count_events(&sim, &mut counts, snd.as_mut(), &lis));
    } else {
        let have = sim.snapshot().tick;
        if a.ticks > have {
            sim.step(a.ticks - have);
        }
    }
    let end = sim.snapshot();
    count_events(&sim, &mut counts, snd.as_mut(), &listener(&gpu, view));
    println!(
        "sim events: {} job sounds, {} effects, {} sounds, {} war actions, {} messages, {} attacks, {} job triggers, {} unported calls",
        counts.job, counts.effects, counts.sounds, counts.war, counts.messages, counts.attacks, counts.triggers, counts.unported
    );
    if let Some(a) = &counts.ambient {
        let mut kinds = std::collections::BTreeMap::new();
        for e in a.effects.values() {
            *kinds.entry(e.kind).or_insert(0) += 1;
        }
        println!(
            "ambient creatures by type {kinds:?}, {} jumping fish now, {} started (land: {} butterfly nodes, {} bird cells, {} seagull cells, {} fish cells, amounts {:?}); {:?}",
            a.fish.len(),
            a.log.fish,
            a.land.butterfly_nodes.len(),
            a.land.bird_cells.len(),
            a.land.seagull_cells.len(),
            a.land.fish_cells.iter().filter(|&&f| f).count(),
            a.land.amounts,
            a.log
        );
    }
    if let Some(s) = snd {
        s.finish(&a.out)?;
    }
    println!(
        "ran {} ticks ({:.1} game seconds) in {:.2} s wall time; final hash {:016x}",
        end.tick,
        end.tick as f64 * 0.071,
        started.elapsed().as_secs_f64(),
        end.hash
    );
    Ok(())
}

#[derive(Default)]
struct EventCounts {
    job: u64,
    effects: u64,
    sounds: u64,
    war: u64,
    unported: u64,
    /// The drained `SimEvent::Effect`s not yet on the scene: (tick, effect, x, y).
    pending: Vec<(u32, u8, i32, i32)>,
    /// The drained `SimEvent::Projectile`s and `TribeEffect`s not yet on the scene: (tick, event).
    flights: Vec<(u32, s4_sim::event::SimEvent)>,
    messages: u64,
    attacks: u64,
    triggers: u64,
    /// The local player's message list (messages.rs) and the 100 ms timer events it had, run
    /// on game time (71 ms per tick, as a fast frame loop would see them).
    msg: Option<(u8, messages::Messages, u64)>,
    /// The buildings' patches (patches.rs), stepped as every tick were drawn.
    patches: patches::Patches,
    /// The ambient creatures and jumping fish (ambient.rs), with the record frame counts.
    ambient: Option<ambient::Ambient>,
    ambient_frames: ambient::Frames,
}

/// The drained ticks' events: counted, and with `--sound` played (each tick against the latest
/// snapshot; headless steps several ticks at once).
fn count_events(sim: &SimHandle, c: &mut EventCounts, mut snd: Option<&mut HeadlessSound>, lis: &sound::Listener) {
    use s4_sim::event::SimEvent;
    let snap = sim.snapshot();
    for (tick, events) in sim.drain_events() {
        for e in &events {
            match *e {
                SimEvent::JobSound { .. } => c.job += 1,
                SimEvent::Effect { effect, x, y, .. } => {
                    c.effects += 1;
                    c.pending.push((tick, effect, x, y));
                }
                SimEvent::EffectLater { effect, x, y, delay, .. } => {
                    c.effects += 1;
                    c.pending.push((tick + delay, effect, x, y));
                }
                SimEvent::Sound { .. } => c.sounds += 1,
                SimEvent::WarAction => c.war += 1,
                SimEvent::Unported { .. } => c.unported += 1,
                SimEvent::Message { player, text, x, y, filtered } => {
                    c.messages += 1;
                    println!("message: tick {tick}, player {player}, text {text:#x} at ({x}, {y}), filtered {filtered}");
                }
                SimEvent::Attacked { .. } => c.attacks += 1,
                SimEvent::JobTrigger { building, trigger } => {
                    c.triggers += 1;
                    if let Some(d) = c.patches.job_trigger(&snap.population, building, trigger)
                        && let Some(s) = snd.as_deref_mut()
                    {
                        s.snd.patch_sounds(&[d]);
                    }
                }
                SimEvent::SpellCast { player, spell, x, y } => println!("spell: tick {tick}, player {player}, spell {spell} at ({x}, {y})"),
                SimEvent::Projectile { .. } => c.flights.push((tick, e.clone())),
                SimEvent::TribeEffect { .. } => {
                    c.effects += 1;
                    c.flights.push((tick, e.clone()));
                }
                // Played by `GameSound::tick` (sound/rotor.rs).
                SimEvent::Rotor { .. } | SimEvent::RotorStop { .. } => {}
                // Made by the ambient manager below (`butterflies`).
                SimEvent::Butterfly { .. } => {}
            }
        }
        let mut msg_sounds = Vec::new();
        if let Some((local, msg, fired)) = &mut c.msg {
            while *fired < tick as u64 * 71 / 100 {
                *fired += 1;
                msg_sounds.extend(msg.timer());
            }
            let before = msg.list.len();
            msg_sounds.extend(msg.sim_events(*local, tick, &events));
            if !msg_sounds.is_empty() || msg.list.len() != before {
                println!("message list at tick {tick}: sounds {msg_sounds:?}, {} unread", msg.lines().count());
            }
        }
        if let Some(s) = snd.as_deref_mut() {
            s.snd.ui(&msg_sounds);
            s.snd.tick(&events, &snap, lis);
        }
        if c.ambient.is_none()
            && let Some(land) = &snap.ambient
        {
            c.ambient = Some(ambient::Ambient::new(land.clone(), c.ambient_frames));
        }
        if let Some(a) = &mut c.ambient {
            let nodes = ambient::Nodes::of(&snap.population);
            butterflies(a, &nodes, &events, tick);
            let centre = sound::centre_node(&snap.population, lis);
            let mut sink = sound::AmbientSink::new(snd.as_deref_mut().map(|s| &mut s.snd), &snap, lis);
            a.tick(&nodes, tick, centre, &mut sink);
        }
        if let Some(s) = snd.as_deref_mut() {
            s.render_tick();
        }
    }
    // `FillGfxInfo` runs for the map objects the object pass visits (sound.rs `DrawRange`).
    let range = sound::DrawRange::new(&snap, lis);
    let due = c.patches.draw(&snap.population, snap.population.tick, |x, y| range.object_drawn(x, y));
    if let Some(s) = snd {
        s.snd.patch_sounds(&due);
        s.snd.draw(&snap, lis);
    }
}

/// `--sound`: the game's sound on an offline mixer, rendered 71 ms per tick.
struct HeadlessSound {
    snd: sound::GameSound<s4_audio::offline::OfflineBackend>,
    renderer: s4_audio::offline::OfflineRenderer,
    pcm: Vec<f32>,
}

impl HeadlessSound {
    const RATE: u32 = 44100;

    fn new(race: u8) -> anyhow::Result<HeadlessSound> {
        let (settings, renderer) = s4_audio::offline::offline_settings(Self::RATE);
        let manager = kira::AudioManager::<s4_audio::offline::OfflineBackend>::new(settings).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let mut snd = sound::GameSound::new(s4_audio::Audio::new(manager, s4_formats::game_dir())?, sound::JobSoundRoll::new(1));
        snd.start(race);
        Ok(HeadlessSound { snd, renderer, pcm: Vec::new() })
    }

    /// 71 ms of audio for the tick just played (after `GameSound::tick` and the ambient
    /// creatures), in blocks so fades and successors are stepped as in a frame loop.
    fn render_tick(&mut self) {
        let target = ((self.snd.log.ticks * 71 * Self::RATE as u64) / 1000) as usize;
        while self.pcm.len() / 2 < target {
            let n = (target - self.pcm.len() / 2).min(1024);
            self.snd.frame(std::time::Duration::from_secs_f64(n as f64 / Self::RATE as f64));
            self.pcm.extend(self.renderer.render(n));
        }
    }

    /// Print the log and the mix's levels, write `<out>_sound.wav`.
    fn finish(self, out: &str) -> anyhow::Result<()> {
        println!("sound:
{}", self.snd.log.report());
        let window = (Self::RATE / 10) as usize;
        let (mut loud, mut windows, mut sum) = (0, 0, 0.0f64);
        for w in self.pcm.chunks(window * 2) {
            let e: f64 = w.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>();
            sum += e;
            windows += 1;
            loud += usize::from((e / w.len() as f64).sqrt() > 1e-3);
        }
        let rms = (sum / self.pcm.len().max(1) as f64).sqrt();
        let path = format!("{out}_sound.wav");
        if let Some(d) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&path, s4_audio::offline::wav_bytes(Self::RATE, &self.pcm))?;
        println!(
            "sound mix: {:.1} s, RMS {:.1} dBFS, {loud} of {windows} 100 ms windows above -60 dBFS; music streams now {:?}; wrote {path}",
            self.pcm.len() as f64 / 2.0 / Self::RATE as f64,
            20.0 * rms.max(1e-12).log10(),
            self.snd.audio.music_streams()
        );
        Ok(())
    }
}

fn determinism(a: &Args) -> anyhow::Result<()> {
    let log = command_log(a)?;
    let s = setup(a)?;
    let one = sim::run_headless(&s, &log, a.ticks)?;
    let two = sim::run_headless(&s, &log, a.ticks)?;
    let first_diff = one.iter().zip(&two).position(|(x, y)| x != y);
    let changes = one.windows(2).filter(|w| w[0] != w[1]).count();
    println!(
        "{} ticks, {} commands: run A final {:016x}, run B final {:016x}; per-tick hashes {} (hash changed on {changes} ticks)",
        a.ticks,
        log.len(),
        one.last().copied().unwrap_or(0),
        two.last().copied().unwrap_or(0),
        match first_diff {
            None => "identical".to_string(),
            Some(i) => format!("DIFFER from tick {}", i + 1),
        }
    );
    anyhow::ensure!(first_diff.is_none(), "runs diverged");
    if !log.is_empty() {
        // The same map without the commands: where the command log first changes the state.
        let none = sim::run_headless(&s, &[], a.ticks)?;
        match none.iter().zip(&one).position(|(x, y)| x != y) {
            Some(i) => println!("without the commands the state first differs after tick {}", i + 1),
            None => println!("the commands did not change the state"),
        }
    }
    Ok(())
}

/// `--bench FRAMES [--map M] [--ticks T] [--load SAVE] [--at X,Y] [--no-fog] [--width W --height H] [--out DIR]`: the
/// window's frame (`Game::redraw` with the GUI) rendered offscreen, timed. Every map slot is a
/// normal computer (control 3, the local player's too); the game runs `T` ticks first and is
/// saved to `<out>/<map>_t<T>.sav`, which later runs load (`--load` names another save). Then
/// per zoom (fully out, fully in) `FRAMES` frames to warm up and `FRAMES` timed ones, a logic
/// tick every 4th (speed 1 at about 57 fps), the time split into the [`perf`] spans, and a
/// capture of the last frame (`<out>/<map>_<zoom>.png`) to compare across builds. The game is paused between the steps,
/// so the frames do not depend on the wall clock.
fn bench(a: &Args) -> anyhow::Result<()> {
    let mut gpu = Gpu::headless(a.width, a.height)?;
    let stem = std::path::Path::new(&a.map).file_stem().map(|s| s.to_string_lossy().replace(' ', "_")).unwrap_or_default();
    let out = std::path::PathBuf::from(&a.out);
    std::fs::create_dir_all(&out)?;
    let save = a.load.clone().map(std::path::PathBuf::from).unwrap_or_else(|| out.join(format!("{stem}_t{}.sav", a.ticks)));
    if !save.exists() {
        let players = sim::PlayerSetup { races: [None; 8], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control: [3; 8], game_type: 0, local: 0, players: None, race_goods: [0; 7], decided: false, start_resources: 0 };
        let ga = Args { players: Some(players), delay: 0, hash_interval: 64, ..Default::default() };
        let g = Game::start(&a.map, &ga, 0, &mut gpu, None)?;
        let t = Instant::now();
        let mut tick = 0;
        while tick < a.ticks {
            tick = g.sim.step(1000.min(a.ticks - tick));
            g.sim.drain_events();
            eprintln!("tick {tick} ({:.1} s)", t.elapsed().as_secs_f32());
        }
        println!("{} ticks in {:.1} s ({:.2} ms per tick)", a.ticks, t.elapsed().as_secs_f32(), t.elapsed().as_secs_f64() * 1000.0 / a.ticks.max(1) as f64);
        g.sim.save(&save, &stem, g.client_state(), Vec::new())?;
        println!("saved {}", save.display());
    }
    let ga = Args { load: Some(save.to_string_lossy().into_owned()), no_fog: a.no_fog, delay: 0, hash_interval: 64, ..Default::default() };
    let mut g = Game::start(&a.map, &ga, 0, &mut gpu, None)?;
    // A loaded game runs at the save's speed: paused again and stepped on to a round tick, with
    // a fixed seed for the patches' `rand()`, so every run draws the same frames.
    g.sim.set_speed(0);
    let t0 = g.sim.step(0);
    g.sim.step((t0 / 100 + 1) * 100 - t0);
    g.patches = patches::Patches::load(1);
    g.alpha = Some(1.0);
    if a.at.is_some() {
        g.cam.jump(view_on(&g.scene, a.at, gpu.size(), g.sim.local), &screen(&gpu, g.scene.source.w));
    }
    gpu.set_offscreen(true);
    let pop = &g.sim.snapshot().population;
    println!(
        "{} at tick {}, {}x{}: {} settlers, {} buildings, {} vehicles, {} animals, {} decos, {} piles",
        a.map,
        pop.tick,
        a.width,
        a.height,
        pop.settlers.len(),
        pop.buildings.len(),
        pop.vehicles.len(),
        pop.animals.len(),
        pop.decos.len(),
        pop.piles.len()
    );
    // Per player: buildings (construction sites) and settlers, the races as the game set them.
    let mut per: std::collections::BTreeMap<u8, (usize, usize, usize)> = Default::default();
    for b in &pop.buildings {
        let e = per.entry(b.owner).or_default();
        if b.site.is_some() { e.1 += 1 } else { e.0 += 1 }
    }
    for u in &pop.settlers {
        per.entry(u.owner).or_default().2 += 1;
    }
    for (p, (b, sites, settlers)) in &per {
        println!("  player {p} (race {}): {b} buildings, {sites} sites, {settlers} settlers", pop.races.get(*p as usize).copied().unwrap_or(0));
    }
    let size = g.scene.source.w;
    let cursor = (a.width as f64 / 2.0, a.height as f64 / 2.0);
    // A logic tick every 4th frame (speed 1 at about 57 fps), drawn at the frame's fraction.
    let frames = |g: &mut Game, gpu: &mut Gpu, n: u32| -> anyhow::Result<std::time::Duration> {
        let mut worst = std::time::Duration::ZERO;
        for f in 0..n {
            let t = Instant::now();
            if f % 4 == 0 {
                let s = perf::span("sim: step");
                g.sim.step(1);
                drop(s);
            }
            g.alpha = Some((f % 4 + 1) as f32 / 4.0);
            g.msg_clock = Some(g.msg_clock.unwrap_or_default() + std::time::Duration::from_micros(17_500));
            g.redraw(gpu, cursor, None);
            let s = perf::span("gpu: wait");
            gpu.wait()?;
            drop(s);
            worst = worst.max(t.elapsed());
        }
        Ok(worst)
    };
    for (zoom, units) in [("out", -1000), ("in", 1000)] {
        g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), units, size), &screen(&gpu, g.scene.source.w));
        // Warm up: the images seen for the first time after the load are decoded and packed.
        frames(&mut g, &mut gpu, a.bench)?;
        perf::take();
        perf::enable(true);
        let all = Instant::now();
        let worst = frames(&mut g, &mut gpu, a.bench)?;
        let all = all.elapsed();
        perf::enable(false);
        let n = a.bench as f64;
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0 / n;
        let sums = perf::take();
        let other = |k: &str| sums.get(k).map_or(0.0, |s| ms(s.total));
        println!(
            "zoom {zoom}: {:.2} ms per frame (worst {:.2}), render side {:.2} (without the sim step and the GPU wait)",
            ms(all),
            worst.as_secs_f64() * 1000.0,
            ms(all) - other("sim: step") - other("gpu: wait")
        );
        for (name, s) in sums {
            println!("  {name:<28} {:>7.3} ms per frame, {:>3} calls, longest {:>7.3} ms", ms(s.total), s.count, s.max.as_secs_f64() * 1000.0);
        }
        let rgba = gpu.capture(g.cam.view())?;
        let png = out.join(format!("{stem}_{zoom}.png"));
        client::write_png(&png, a.width, a.height, &rgba)?;
        println!("  captured {}", png.display());
    }
    Ok(())
}

/// Working set and private bytes of this process (`K32GetProcessMemoryInfo`), in bytes.
#[cfg(windows)]
fn process_memory() -> (u64, u64) {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
    // SAFETY: a pseudo handle and a struct of the size given (PROCESS_MEMORY_COUNTERS_EX).
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok == 0 { (0, 0) } else { (c.working_set as u64, c.private as u64) }
}

/// Linux: resident (`VmRSS`) and anonymous (`RssAnon`) memory from `/proc/self/status`.
#[cfg(not(windows))]
fn process_memory() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = |name: &str| s.lines().find_map(|l| l.strip_prefix(name)?.strip_prefix(':')?.split_whitespace().next()?.parse::<u64>().ok()).unwrap_or(0) * 1024;
    (kb("VmRSS"), kb("RssAnon"))
}

/// This process's dedicated GPU memory in bytes, from the `GPU Process Memory` counters (Windows,
/// through PowerShell; about a second), or None.
fn gpu_process_memory() -> Option<u64> {
    if !cfg!(windows) {
        return None;
    }
    let pid = std::process::id();
    let cmd = format!("(Get-Counter '\\GPU Process Memory(pid_{pid}_*)\\Dedicated Usage' -ErrorAction Stop).CounterSamples | Measure-Object -Property CookedValue -Sum | ForEach-Object {{ [int64]$_.Sum }}");
    let out = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &cmd]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// `--soak TICKS [--map M] [--width W --height H] [--out DIR]`: a long session. Four normal
/// computers (the local player's slot too; Roman, Viking, Maya, Trojan) on a free map (default
/// `Multiplayer/Choices`) with medium start resources, `TICKS` ticks through `Game::redraw` with a frame every 4th tick; the
/// camera visits the four starts in turn (every 500 ticks) and changes the zoom every 2500.
/// Every 5000 ticks: process memory (working set, private bytes), the process's dedicated GPU
/// memory, live wgpu objects, the sprite scene's caches, frame / `set_population` / sim-step
/// times since the last line, and the population; written to `<out>/soak.txt` as it goes.
fn soak(a: &Args) -> anyhow::Result<()> {
    use std::fmt::Write as _;
    let mut gpu = Gpu::headless(a.width, a.height)?;
    gpu.set_offscreen(true);
    let out = std::path::PathBuf::from(&a.out);
    std::fs::create_dir_all(&out)?;
    let map = if a.map == "Campaign/roman02" { "Multiplayer/Choices".to_string() } else { a.map.clone() };
    let players = sim::PlayerSetup { races: [Some(0), Some(1), Some(2), Some(4), None, None, None, None], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control: [3, 3, 3, 3, 0, 0, 0, 0], game_type: 0, local: 0, players: Some(4), race_goods: [0; 7], decided: false, start_resources: 2 };
    // No fog: the camera's tour shows every player's land, as hours of play would.
    let ga = Args { players: Some(players), no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
    let mut g = Game::start(&map, &ga, 0, &mut gpu, None)?;
    g.progress_file = None;
    g.autosave_interval = None;
    let starts: Vec<(usize, usize)> = g.cur.population.starts.iter().skip(1).take(4).map(|&(x, y)| (x as usize, y as usize)).collect();
    let size = g.scene.source.w;
    let cursor = (a.width as f64 / 2.0, a.height as f64 / 2.0);
    let mut log = String::new();
    let line = |log: &mut String, l: String| {
        println!("{l}");
        log.push_str(&l);
        log.push('\n');
        let _ = std::fs::write(out.join("soak.txt"), &*log);
    };
    line(&mut log, format!("soak: {map}, {} ticks, {}x{}, starts {starts:?}", a.soak, a.width, a.height));
    perf::enable(true);
    perf::take();
    let t0 = Instant::now();
    let mut window = (std::time::Duration::ZERO, std::time::Duration::ZERO, 0u32);
    let mut tick = g.cur.tick;
    let mut next_report = 0;
    let mut zoom_in = false;
    let mut sums: std::collections::BTreeMap<&'static str, perf::Sum> = Default::default();
    let mut spikes: Vec<String> = Vec::new();
    loop {
        if tick >= next_report {
            let (ws, private) = process_memory();
            let gpu_mem = gpu_process_memory();
            let sums = std::mem::take(&mut sums);
            let span = |k: &str| sums.get(k).map(|s| (s.total.as_secs_f64() * 1000.0 / s.count.max(1) as f64, s.max.as_secs_f64() * 1000.0)).unwrap_or_default();
            let (pop_avg, pop_max) = span("present: set_population");
            let (sim_avg, sim_max) = span("sim: step");
            let frames = window.2.max(1) as f64;
            let pop = &g.cur.population;
            let mb = |b: u64| b as f64 / 1048576.0;
            let mut l = format!(
                "t {tick:>6} ({:>5.0} s): ws {:>7.1} MB, private {:>7.1} MB, gpu {}, frame {:.2} ms (max {:.1}), set_population {pop_avg:.2} ms (max {pop_max:.1}), sim step (4 ticks) {sim_avg:.2} ms (max {sim_max:.1}); {} settlers, {} buildings, {} vehicles, {} animals, {} piles",
                t0.elapsed().as_secs_f64(),
                mb(ws),
                mb(private),
                gpu_mem.map_or("?".into(), |b| format!("{:.1} MB", mb(b))),
                window.0.as_secs_f64() * 1000.0 / frames,
                window.1.as_secs_f64() * 1000.0,
                pop.settlers.len(),
                pop.buildings.len(),
                pop.vehicles.len(),
                pop.animals.len(),
                pop.piles.len()
            );
            if let Some([b, t, v, bg]) = Gpu::live_resources() {
                let _ = write!(l, "; wgpu buffers {b}, textures {t}, views {v}, bind groups {bg}");
            }
            if let Some(s) = g.scene.sprites.as_ref() {
                let _ = write!(l, "; scene {:?}", s.cache_sizes());
            }
            if !spikes.is_empty() {
                let _ = write!(l, "\n    frames over 40 ms: {}", spikes.join("; "));
                spikes.clear();
            }
            line(&mut log, l);
            window = (std::time::Duration::ZERO, std::time::Duration::ZERO, 0);
            next_report += 5000;
        }
        if tick >= a.soak {
            break;
        }
        // The camera: the next start every 500 ticks, the zoom in or out every 2500.
        let jump = tick % 500 == 0;
        if jump && !starts.is_empty() {
            let st = starts[(tick / 500) as usize % starts.len()];
            g.cam.jump(view_on(&g.scene, Some(st), gpu.size(), g.sim.local), &screen(&gpu, g.scene.source.w));
            if tick % 2500 == 0 {
                zoom_in = !zoom_in;
            }
            if zoom_in {
                g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), 3, size), &screen(&gpu, g.scene.source.w));
            }
        }
        let t = Instant::now();
        let s = perf::span("sim: step");
        tick = g.sim.step(4);
        drop(s);
        g.sim.drain_events();
        g.redraw(&mut gpu, cursor, None);
        gpu.wait()?;
        let d = t.elapsed();
        window = (window.0 + d, window.1.max(d), window.2 + 1);
        // This frame's spans into the window's; a long frame says where its time went.
        let frame = perf::take();
        if d.as_millis() > 40 && spikes.len() < 12 {
            let mut top: Vec<_> = frame.iter().map(|(k, s)| (s.total, *k)).collect();
            top.sort_by_key(|t| std::cmp::Reverse(t.0));
            let top: Vec<String> = top.iter().take(3).map(|(d, k)| format!("{k} {:.0}", d.as_secs_f64() * 1000.0)).collect();
            spikes.push(format!("t {tick} {:.0} ms{} ({})", d.as_secs_f64() * 1000.0, if jump { " camera jump" } else { "" }, top.join(", ")));
        }
        for (k, f) in frame {
            let e = sums.entry(k).or_default();
            e.total += f.total;
            e.count += f.count;
            e.max = e.max.max(f.max);
        }
    }
    let rgba = gpu.capture(g.cam.view())?;
    client::write_png(&out.join("soak_end.png"), a.width, a.height, &rgba)?;
    Ok(())
}

/// `--audit TICKS [--map M] [--out DIR]`: the missing-graphics audit. Every shipped map
/// (`Map\Campaign`, `Multiplayer`, `Singleplayer`, `Tutorial`; `--map` one of them) with every
/// slot a normal computer and no fog runs `TICKS` ticks through `Game::redraw` (a frame every
/// 4th tick, at the tick's quarters in turn); every draw that finds no image or falls back
/// (s4_viewer::audit: unresolved entities, jobs past a file's count, frames past a job's end,
/// missing directions, effects without images) is counted per map and written with the
/// totals to `<out>/audit.txt` (`--audit-part K/N`: every N-th map from the K-th).
fn audit(a: &Args) -> anyhow::Result<()> {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    let mut gpu = Gpu::headless(a.width, a.height)?;
    let root = s4_formats::path::resolve(s4_formats::game_dir().join("Map"));
    let mut maps = Vec::new();
    for sub in ["Campaign", "Multiplayer", "Singleplayer", "Tutorial"] {
        for e in std::fs::read_dir(root.join(sub))?.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("map")) {
                maps.push(format!("{sub}/{}", p.file_stem().unwrap_or_default().to_string_lossy()));
            }
        }
    }
    maps.sort();
    if a.map_given {
        maps.retain(|m| m.eq_ignore_ascii_case(&a.map));
    }
    if let Some((k, n)) = a.audit_part {
        maps = maps.into_iter().skip(k).step_by(n.max(1)).collect();
    }
    let out = std::path::PathBuf::from(&a.out);
    std::fs::create_dir_all(&out)?;
    let mut report = String::new();
    let mut total: BTreeMap<String, (u64, Vec<String>)> = BTreeMap::new();
    s4_viewer::audit::enable(true);
    for (n, map) in maps.iter().enumerate() {
        let players = sim::PlayerSetup { races: [None; 8], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control: [3; 8], game_type: 0, local: 0, players: None, race_goods: [0; 7], decided: false, start_resources: 2 };
        let ga = Args { players: Some(players), no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
        let t = Instant::now();
        let mut g = match Game::start(map, &ga, 0, &mut gpu, None) {
            Ok(g) => g,
            Err(e) => {
                let _ = writeln!(report, "{map}: does not start: {e}");
                continue;
            }
        };
        g.patches = patches::Patches::load(1);
        let mut f = 0;
        while g.cur.tick < a.audit {
            g.sim.step(4);
            g.alpha = Some((f % 4 + 1) as f32 / 4.0);
            f += 1;
            g.redraw(&mut gpu, (0.0, 0.0), None);
            // As a window's frame would: the replaced atlases are freed.
            gpu.wait()?;
        }
        let misses = s4_viewer::audit::take();
        let _ = writeln!(report, "{map}: {} kinds of miss ({:.0} s)", misses.len(), t.elapsed().as_secs_f32());
        // What was on the map at the end: the settler types (count), vehicles, flyers.
        let pop = &g.cur.population;
        let mut kinds = BTreeMap::<u16, u32>::new();
        for u in &pop.settlers {
            *kinds.entry(u.kind).or_default() += 1;
        }
        let _ = writeln!(report, "    seen: settler types {kinds:?}, {} vehicles, {} flyers, {} buildings", pop.vehicles.len(), pop.flyers.len(), pop.buildings.len());
        for (what, count) in &misses {
            let _ = writeln!(report, "    {count:>8}  {what}");
            let e = total.entry(what.clone()).or_default();
            e.0 += count;
            e.1.push(map.clone());
        }
        println!("[{}/{}] {map}: {} kinds of miss", n + 1, maps.len(), misses.len());
        // Nothing is submitted headless: let the device free the last game's textures.
        drop(g);
        gpu.wait()?;
        std::fs::write(out.join("audit.txt"), &report)?;
    }
    let mut sorted: Vec<_> = total.into_iter().collect();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.1.0));
    let mut sum = String::from("Totals (count, maps, what, the first maps):\n");
    for (what, (count, maps)) in &sorted {
        let _ = writeln!(sum, "{count:>10} {:>4}  {what}   [{}]", maps.len(), maps.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
    }
    std::fs::write(out.join("audit.txt"), format!("{sum}\n{report}"))?;
    println!("{sum}");
    Ok(())
}

/// `--headless --menu`: the start screens offscreen, each `--input` action followed by a capture
/// (`<out>_NN.png`); a started map is loaded and captured as the last frame. With `--sound`, the
/// GUI sounds each action played (`Frontend::take_sounds`).
fn menu_headless(a: &Args) -> anyhow::Result<()> {
    let mut gpu = Gpu::headless(a.width, a.height)?;
    frontend::Frontend::fit(&mut gpu);
    let mut f = frontend::Frontend::new(&mut gpu, settings::Settings::load().language)?;
    let capture = |gpu: &mut Gpu, n: usize| -> anyhow::Result<()> {
        let rgba = gpu.capture(View { x: 0.0, y: 0.0, zoom: 1.0 })?;
        let path = format!("{}_{n:02}.png", a.out);
        client::write_png(std::path::Path::new(&path), a.width, a.height, &rgba)?;
        println!("captured {path}");
        Ok(())
    };
    capture(&mut gpu, 0)?;
    for (n, (_, act)) in input_script(a)?.into_iter().enumerate() {
        let p = match act {
            Action::Click(x, y) => (x, y),
            Action::ClickControl(d, l) => client::control_point(&gpu, d, l).ok_or_else(|| anyhow::anyhow!("no control {l} in dialog {d}"))?,
            Action::Hover(x, y) => {
                f.mouse(&mut gpu, (x, y), GuiMouse::Move);
                capture(&mut gpu, n + 1)?;
                continue;
            }
            _ => anyhow::bail!("menu input: only click, click-control and hover"),
        };
        f.mouse(&mut gpu, p, GuiMouse::Move);
        f.mouse(&mut gpu, p, GuiMouse::LeftDown);
        let act = f.mouse(&mut gpu, p, GuiMouse::LeftUp);
        if a.sound {
            println!("input {n}: GUI sounds {:?}", f.take_sounds(&mut gpu));
        }
        if let Some(frontend::Action::Start(map, _, players)) = act {
            let map = map.to_string_lossy().into_owned();
            println!("start {map}");
            let ga = Args { players, no_script: a.no_script, no_fog: a.no_fog, delay: a.delay, hash_interval: a.hash_interval, ..Default::default() };
            let mut g = Game::start(&map, &ga, 1, &mut gpu, None)?;
            if let Some(s) = g.scene.sprites.as_mut() {
                s.set_logos(&f.logos);
            }
            g.cur = g.sim.snapshot();
            present(&mut gpu, &mut g.scene, &mut g.base, &g.prev.clone(), &g.cur.clone(), interp::alpha(&g.cur, Instant::now()), &[], None, &[], &g.patches.shown(), &[], &mut g.offsets)?;
            client::update_minimap(&mut gpu, &mut g.scene, g.cam.view(), 64);
            let rgba = gpu.capture(g.cam.view())?;
            let path = format!("{}_{:02}.png", a.out, n + 1);
            client::write_png(std::path::Path::new(&path), a.width, a.height, &rgba)?;
            println!("captured {path}");
            return Ok(());
        }
        capture(&mut gpu, n + 1)?;
    }
    Ok(())
}

/// A running game: the sim thread, the scene drawn from its snapshots, input state.
struct Game {
    sim: SimHandle,
    scene: Scene,
    /// The view and everything that moves it (camera.rs).
    cam: camera::Camera,
    base: Arc<Snapshot>,
    prev: Arc<Snapshot>,
    cur: Arc<Snapshot>,
    speed: u32,
    last: Instant,
    /// The game's sound (sound.rs); `None` without an audio device.
    sound: Option<sound::GameSound>,
    build: build::BuildMode,
    pointer: select::Pointer,
    /// The selected building's dialog (bdlg.rs).
    bdlg: bdlg::BuildingDialog,
    pages: pages::Pages,
    /// The campaign mission being played: (campaign type, level).
    campaign: Option<(u8, u32)>,
    /// The map file (briefing.rs reads its goal).
    map_path: std::path::PathBuf,
    /// The logos the players' flags show (App `logos` at the start, a loaded save's own).
    logos: [Option<Vec<u8>>; 8],
    /// The players' names as the GUI got them at the start (`chat_names`).
    ui_names: Option<[String; 9]>,
    /// 0x11b589c as the GUI keeps it (`Gui::hidden_last`), for the end record.
    hidden_last: i32,
    /// The script's decision, once shown (script.rs `WON` / `LOST`).
    decided: u8,
    /// The load / save submenu.
    menu: menu::SaveMenu,
    /// "Quit" confirmed on page 34: the shell ends the game.
    quit: bool,
    /// A save chosen on page 33 to continue instead.
    load_request: Option<std::path::PathBuf>,
    /// Tick of the last save (`CStateGame +0x104`).
    last_save: u32,
    /// Autosave interval in minutes, `None` = off (settings.toml).
    autosave_interval: Option<u32>,
    /// A network game (saves go through the lockstep).
    network: bool,
    /// Where won missions are recorded (progress.rs; tests use their own file).
    progress_file: Option<std::path::PathBuf>,
    /// The left button is held (the border flags keep their value).
    left_held: bool,
    /// The cursor shape (`CursorList` index, cursor.rs) and what it depends on: the Alt key, the
    /// shape before a pause (0x514be0 keeps it), the input state and selection last seen, the
    /// tick it was last chosen at.
    cursor_shape: u8,
    /// The shape is locked during a right drag (`IGfxEngine +0x21`).
    cursor_locked: bool,
    alt: bool,
    cursor_before_pause: Option<u8>,
    cursor_state: u8,
    cursor_sel: Option<u16>,
    cursor_tick: u32,
    /// The digit key pressed last and the tick (`GameManager` +0xf4 / +0xf8): the same digit
    /// again within 6 ticks jumps to the group (0x1d9).
    group_key: Option<(u8, u32)>,
    /// The last cursor position seen.
    cursor_last: (f64, f64),
    /// The text messages (messages.rs) and their 100 ms wall-clock timer.
    messages: messages::Messages,
    /// The cheat console's switch (`CStateGame +0x102`, msgwin.rs).
    console: msgwin::Console,
    msg_timer: messages::Timer,
    msg_epoch: Instant,
    /// `ExtendedTooltip` (settings.rs): the message window's width.
    extended_tooltip: bool,
    /// The settings the settings pages change (options.rs).
    settings: settings::Settings,
    /// The buildings' patches (patches.rs): drawn, and their sounds.
    patches: patches::Patches,
    /// The ambient creatures and jumping fish (ambient.rs), once the snapshot brings their land.
    ambient: Option<ambient::Ambient>,
    /// Benchmarks: how far into the tick frames are drawn, and the message timer's clock,
    /// instead of the wall clock.
    alpha: Option<f32>,
    msg_clock: Option<std::time::Duration>,
    /// The units' interpolation anchors (interp.rs).
    offsets: interp::Offsets,
}

impl Game {
    /// A digit key (`EventManagement` 0x5172f0 key case 0xb: '1'..'9' groups 1..9, '0' group 10;
    /// key flags: Shift 4, Ctrl 8, Alt 0x10), as UI events to `CInputProcessor`, each only where
    /// its state machine has the transition (input.rs):
    /// * Ctrl: 0x1d6 (0x4ec630): command 5041 `DefineGroup` with the first 100 selected ids;
    /// * no key held, the same digit as last within 6 ticks: 0x1d9 (0x4ec760): the camera to the
    ///   mean position of the selected own non-buildings (event 0x13);
    /// * else 0x1d8 (0x4ec880, Shift adds): the group's members that are selectable (flag
    ///   0x2000), up to 100, replace the selection (Shift: are added while the group's first
    ///   member's selection category is the first selected one's, 0x4e6a40 vs 0x4eeb50), then
    ///   `RequestDialog`.
    fn group_key(&mut self, gpu: &mut Gpu, g: u8) {
        let state = self.bdlg.state_of(self.build.building.is_some(), &self.pointer);
        let tick = self.cur.tick;
        let plain = !self.pointer.shift && !self.alt;
        let again = plain && self.group_key.is_some_and(|(k, t)| k == g && tick.saturating_sub(6) < t);
        let ev = if self.pointer.ctrl {
            0x1d6
        } else if again {
            0x1d9
        } else {
            self.group_key = Some((g, tick));
            0x1d8
        };
        if input::transition(state, ev).is_none() {
            return;
        }
        // bdlg.rs (also the groups sidebar D55's buttons); 0x1d9's event 0x13 at once.
        self.bdlg.group_event(ev, g, gpu, &self.sim, &mut self.pointer);
        if let Some((x, y)) = self.bdlg.centre.take() {
            self.centre_on(gpu, (x as usize, y as usize));
        }
    }

    /// The window and map the camera works in.
    fn screen(&self, gpu: &Gpu) -> camera::Screen {
        screen(gpu, self.scene.source.w)
    }

    /// Put the view at `v` at once (a jump).
    fn jump(&mut self, gpu: &Gpu, v: View) {
        let s = self.screen(gpu);
        self.cam.jump(v, &s);
    }

    /// Event 0x13: the view centred on a node (0x519c50).
    fn centre_on(&mut self, gpu: &Gpu, node: (usize, usize)) {
        let v = client::view_on_node(gpu, self.cam.view(), node, self.scene.source.w);
        self.jump(gpu, v);
    }

    /// The cursor shape (cursor.rs): a mode entered or left sets its shape (`TryBuild` 0x4ea250:
    /// 3, the trade target 0x4eaf60: 6, the leave handler `sub_EDC10`: the arrow), a pause shows
    /// 0x1f (0x514be0), and at every logic tick or selection change 0x4f2900 decides.
    fn update_cursor(&mut self) {
        // A right drag (0x518186): `SetCursorShape(1, 1)`, then the shape is locked (0x65e590(1):
        // `SetCursorShape` does nothing while +0x21 is set) until the button comes up (0x5182f6).
        if self.cam.right_dragging() {
            if !self.cursor_locked {
                self.cursor_shape = cursor::shape::DRAG;
                self.cursor_locked = true;
            }
            return;
        }
        self.cursor_locked = false;
        let paused = self.cur.tick_ms == 0;
        match (paused, self.cursor_before_pause) {
            (true, None) => {
                self.cursor_before_pause = Some(self.cursor_shape);
                self.cursor_shape = cursor::shape::PAUSE;
            }
            (false, Some(before)) => {
                self.cursor_shape = before;
                self.cursor_before_pause = None;
            }
            _ => {}
        }
        if paused {
            return;
        }
        let state = self.bdlg.state_of(self.build.building.is_some(), &self.pointer);
        if state != self.cursor_state {
            match state {
                4 => self.cursor_shape = cursor::shape::BUILD,
                7 => self.cursor_shape = cursor::shape::TRADE_TARGET,
                0 | 1 if matches!(self.cursor_state, 3 | 4 | 7) => self.cursor_shape = cursor::shape::ARROW,
                _ => {}
            }
            self.cursor_state = state;
        }
        let first = self.pointer.selection.ids.first().copied();
        if self.cur.tick == self.cursor_tick && first == self.cursor_sel {
            return;
        }
        self.cursor_tick = self.cur.tick;
        self.cursor_sel = first;
        let pop = &self.cur.population;
        let selected = first.and_then(|id| {
            pop.settlers.iter().chain(&pop.vehicles).find(|u| u.id == id).map(|u| cursor::Selected { obj: u.obj, kind: u.kind, flags: u.flags }).or_else(|| {
                pop.buildings.iter().find(|b| b.id == id).map(|b| cursor::Selected { obj: 8, kind: b.kind as u16, flags: 0 })
            })
        });
        let mods = cursor::Mods { alt: self.alt, shift: self.pointer.shift, ctrl: self.pointer.ctrl };
        let held = self.cam.right_held() || self.cam.both_held();
        if let Some(shape) = cursor::choose(self.cursor_shape, state, held, selected, mods) {
            self.cursor_shape = shape;
        }
    }

    /// The script decided the game (`CStateGame::OnEvent` 0x34 → `CGameData::TeamWon`, event
    /// 0x261 → `GuiDlgWinLossProc`, dialog 72: link 1795 = string 1964 "won" / 1934 "lost").
    /// A won campaign mission raises the campaign's status (`CStateGame::EndGame`:
    /// `SetCampaignStatus(type, level + 1)`), which unlocks the next mission.
    fn check_outcome(&mut self, gpu: &mut Gpu) {
        let o = self.sim.outcome.load(std::sync::atomic::Ordering::Relaxed);
        if o == 0 || self.decided != 0 {
            return;
        }
        self.decided = o;
        // A tutorial (type 4): event 0x37 only records the winner (0x5172f0), no win screen;
        // `Tutorial.Exit` ends the game.
        if self.sim.tutorial.lock().unwrap_or_else(|e| e.into_inner()).active {
            return;
        }
        let won = o == script::WON;
        // Event 0x37 (EventManagement 0x5173e9): the winning team's player sees the whole map
        // (`CFogging::SetRWM(1)`, 0x51744e), a loser only in a network game (`CStateGame +0xa4`,
        // 0x5174ab); a game loaded after the decision does the same (0x513851 / 0x51386f).
        if won || self.network {
            self.sim.reveal_map();
        }
        let (texts, end_event, on_load) = {
            let t = self.sim.tutorial.lock().unwrap_or_else(|e| e.into_inner());
            (t.end_texts, t.end_event, t.decided_on_load)
        };
        let snd = if on_load { self.messages.decided_on_load(won, self.sim.local) } else { self.messages.game_decided(won, end_event, self.sim.local, texts) };
        if let Some(s) = &mut self.sound {
            s.ui(&snd);
        }
        let screen_w = (gpu.size().0 as f32 / gpu.gui_scale.max(0.1)) as u32;
        if let Some(ui) = gpu.ui.as_mut() {
            ui.show_win_loss(won, screen_w);
        }
        client::recompose(gpu);
        if won
            && let Some((campaign, level)) = self.campaign
            && let Some(file) = &self.progress_file
        {
            let mut p = progress::Progress::load_from(file);
            if p.won(campaign, level) {
                p.save_to(file);
                tracing::info!("campaign {campaign}: mission {} won, {} won so far", level + 1, p.wins(campaign));
            }
        }
    }

    /// Start `map` (a path or a name under `Map\`) on the window's GPU.
    /// `net`: a game the lobby screens set up (session and players).
    fn start(map: &str, args: &Args, speed: u32, gpu: &mut Gpu, net: Option<(Box<s4_net::Session>, sim::PlayerSetup)>) -> anyhow::Result<Game> {
        let mut a = Args { map: map.to_string(), ..Default::default() };
        a.no_script = args.no_script;
        a.no_fog = args.no_fog;
        a.host = args.host;
        a.join = args.join.clone();
        a.bind = args.bind.clone();
        a.delay = args.delay;
        a.hash_interval = args.hash_interval;
        a.load = args.load.clone();
        a.players = args.players;
        let mut s = setup(&a)?;
        let map_path = s.map_path.clone();
        let net_game = net.is_some();
        if let Some((session, players)) = net {
            s.net = Some(sim::NetSetup::Ready(session, players));
        }
        // The scene loads while the sim thread builds the world (both read the map; neither
        // needs the other): UnitedNations 2.6 s + 0.7 s one after the other.
        let t = perf::span("start: scene load + sim world");
        // Ours: the scene from the map's raw data (the first snapshot replaces it at the end of
        // the start, `set_population`), not from a second world build. The building patches'
        // tables and the sound device meanwhile too.
        let roll = sound::JobSoundRoll::from_clock().state();
        // The GUI's height in panel px with the game's GUI scale (set below to the scale factor).
        let gui_h = (gpu.size().1 as f32 / gpu.scale_factor.max(0.1)) as u32;
        // The local player's race (the sim's first snapshot) for the sound thread.
        let (race_tx, race_rx) = std::sync::mpsc::channel::<u8>();
        let (sim, scene, patches, device, ui) = std::thread::scope(|sc| {
            let scene = sc.spawn(|| Scene::load(Some(map), true, true));
            let patches = sc.spawn(move || patches::Patches::load(roll));
            // The device, its volumes and the race's music (`CStateGame` 0x5155b0: volumes, then
            // `PlayBackgroundMusic(race, SETTLE)`), off the main thread: opening the device and
            // the first stream took ~330 ms there.
            let device = sc.spawn(move || {
                let _t = perf::span("start: sound device");
                let st = settings::Settings::load();
                let mut s = sound::open_device().map(|a| {
                    let mut s = sound::GameSound::new(a, sound::JobSoundRoll::from_clock());
                    s.audio.set_volumes(st.sound_level(), st.music_level(), false);
                    s
                })?;
                // No race without a sim: no music.
                let race = race_rx.recv().ok()?;
                s.start(race);
                Some(s)
            });
            let t = perf::span("start: sim world (main thread)");
            let sim = SimHandle::spawn(s, Clock::RealTime { speed });
            drop(t);
            if let Ok(h) = &sim {
                let _ = race_tx.send(h.snapshot().population.races.get(h.local as usize).copied().unwrap_or(0));
            }
            drop(race_tx);
            // The local player's GUI (its race's set, the text engine, the gfx), from the sim's
            // first snapshot, while the scene still loads.
            let ui = sim.as_ref().ok().map(|h| {
                let (first, local) = (h.snapshot(), h.local);
                sc.spawn(move || {
                    let _t = perf::span("start: GUI load");
                    client::new_gui(Some(&first.population), local, gui_h).map(|ui| (ui, first))
                })
            });
            let scene = scene.join().unwrap_or_else(|_| Err(anyhow::anyhow!("scene load panicked")));
            let ui = ui.and_then(|h| h.join().ok());
            (sim, scene, patches.join(), device.join().ok().flatten(), ui)
        });
        let patches = patches.unwrap_or_else(|_| patches::Patches::load(roll));
        drop(t);
        let (sim, mut scene) = (sim?, scene?);
        let local = sim.local;
        for s in command_log(args)? {
            sim.command(s);
        }
        // Every game starts with the monitor hidden (its state is the session's, not saved).
        monitor::Monitor::hide(gpu);
        let t = perf::span("start: upload");
        scene.upload(gpu_reset(gpu));
        drop(t);
        gpu.gui_scale = gpu.scale_factor;
        let t = perf::span("start: attach_gui");
        let attached = match ui {
            Some(Ok((ui, first))) => client::install_gui(gpu, ui, Some(&first.population), sim.local),
            Some(Err(e)) => Err(e),
            None => client::attach_gui(gpu, &scene, sim.local),
        };
        if let Err(e) = attached {
            tracing::warn!("GUI: {e}");
        }
        drop(t);
        let t = perf::span("start: chat names, view, options");
        chat_names(gpu, &sim, &map_path);
        let ui_names = gpu.ui.as_ref().map(|u| u.state.page_info.chat.names.clone());
        let mut view = view_on(&scene, args.at, gpu.size(), sim.local);
        let mut speed = speed;
        let mut campaign = None;
        if let Some(c) = &sim.loaded {
            view = View { x: c.view.0, y: c.view.1, zoom: c.view.2 };
            campaign = c.campaign;
            speed = c.speed.max(1);
            sim.set_speed(speed);
        }
        client::zoom_slider_start(gpu, view);
        let first = sim.snapshot();
        let st = settings::Settings::load();
        let extended_tooltip = extended_tooltips(&st, &sim);
        options::init(gpu, &st);
        options::apply_graphics(gpu, &st);
        let sound = device;
        let sim_tick = first.tick;
        drop(t);
        let t = perf::span("start: Game");
        // The network game's host only, as the original; ours: single player only for now (a
        // game from the lobby comes with `net`, the command line's with --host / --join).
        let autosave_interval = (st.autosave && !net_game && args.host.is_none() && args.join.is_none()).then_some(st.autosave_interval);
        let mut g = Game {
            sim,
            cam: camera::Camera::new(view, st.border_scroll, st.scroll_step),
            scene,
            base: first.clone(),
            prev: first.clone(),
            cur: first,
            speed: speed.max(1),
            last: Instant::now(),
            sound,
            build: build::BuildMode::new(local),
            pointer: select::Pointer::new(local),
            bdlg: bdlg::BuildingDialog::default(),
            pages: pages::Pages::default(),
            campaign,
            logos: Default::default(),
            decided: 0,
            progress_file: progress::path(),
            menu: menu::SaveMenu::default(),
            quit: false,
            load_request: None,
            last_save: sim_tick,
            autosave_interval,
            network: args.host.is_some() || args.join.is_some() || net_game,
            left_held: false,
            cursor_shape: cursor::shape::ARROW,
            cursor_locked: false,
            alt: false,
            cursor_before_pause: None,
            cursor_state: 0,
            cursor_sel: None,
            cursor_tick: 0,
            group_key: None,
            cursor_last: (0.0, 0.0),
            messages: { let _t = perf::span("start: messages"); messages::Messages::new(messages::Classes::load(), st.msg_level_mask, st.popup_all_msgs) },
            console: Default::default(),
            msg_timer: messages::Timer::default(),
            msg_epoch: Instant::now(),
            extended_tooltip,
            settings: st.clone(),
            patches,
            ambient: None,
            map_path,
            ui_names,
            hidden_last: 0,
            alpha: None,
            msg_clock: None,
            offsets: Default::default(),
        };
        drop(t);
        // The scene was read from the map file; the game's first snapshot can already differ (the
        // start resources, a loaded save) while `base` equals it, so `present` would not rebuild
        // until something moved.
        let t = perf::span("start: first set_population");
        if let Err(e) = g.scene.set_population(gpu, g.cur.population.clone()) {
            tracing::warn!("scene: {e}");
        }
        drop(t);
        // The first fog re-meshes the whole terrain: here, under the start, not in the first
        // frame the player sees.
        let t = perf::span("start: first fog");
        if let Some(f) = &g.cur.fog {
            g.scene.set_fog_layer(gpu, &f.gfx, f.fade, f.border);
        }
        drop(t);
        // The first frame's one-offs, under the start: the minimap drawn whole (its first
        // update), and every upload so far submitted and done (the first frame's GPU wait was
        // ~55 ms after loading a save).
        let t = perf::span("start: minimap, GPU uploads");
        if gpu.show_gui {
            client::update_minimap(gpu, &mut g.scene, g.cam.view(), 1);
        }
        if let Err(e) = gpu.wait() {
            tracing::warn!("GPU: {e}");
        }
        drop(t);
        g.set_briefing(gpu);
        Ok(g)
    }

    /// The mission goal (0x149be6c, briefing.rs) for the extras' 929 and page 35: the
    /// campaign's or the map's.
    fn set_briefing(&mut self, gpu: &mut Gpu) {
        let Some(ui) = gpu.ui.as_mut() else { return };
        let text = |id: usize| Some(ui.string(id)).filter(|t| !t.is_empty());
        let goal = briefing::goal(self.campaign, &self.map_path, self.settings.language, &text);
        ui.state.page_info.briefing = (ui.string(880), goal);
    }

    /// `CEndStatistic` 0x147c940 as 0x4e4660 fills it: the tick, `PlayerCount`, the map's path
    /// (`GlobalGameState +0x18`; [inf] relative to the game, "Map\…"), the names (ours; the
    /// others' as the chat knows them), races, colours ([inf] p − 1), teams and the statistic
    /// records. [unk] `GlobalGameStateObject +0xd / +0xe` (a team game, the winning team): no
    /// writer found, taken as no team game; `Game.DisableLastNPlayersInStatistic` (+0x9a6c = 0x11b589c)
    /// as the GUI got it.
    fn end_record(&self) -> frontend::endstat::EndRecord {
        let pop = &self.cur.population;
        let (stats, warrior) = self.sim.end_stats();
        let map = s4_formats::path::resolve(s4_formats::game_dir().join("Map"));
        let rel = self.map_path.strip_prefix(&map).unwrap_or(&self.map_path);
        let path = format!("Map\\{}", rel.to_string_lossy().replace('/', "\\"));
        let own = self.settings.player_name.clone();
        let ui_names = self.ui_names.clone();
        frontend::endstat::EndRecord {
            tick: self.cur.tick,
            players: (1..9).rev().find(|&p| pop.teams[p] != 0).unwrap_or(0) as i32,
            removed: self.hidden_last,
            decided: self.decided != 0 || self.sim.tutorial.lock().unwrap_or_else(|e| e.into_inner()).winner != 0,
            winner: self.sim.tutorial.lock().unwrap_or_else(|e| e.into_inner()).winner as i32,
            path,
            names: std::array::from_fn(|p| match (p, ui_names.as_ref()) {
                (0, _) => String::new(),
                (_, Some(n)) => n[p].clone(),
                _ => msgwin::player_name(&self.sim, &own, p as u8),
            }),
            race: std::array::from_fn(|p| pop.races[p] as i32),
            colour: std::array::from_fn(|p| p as i32 - 1),
            team: std::array::from_fn(|p| pop.teams[p] as i32),
            warrior,
            stats,
        }
    }

    /// The game's state for a savegame.
    fn client_state(&self) -> save::ClientState {
        save::ClientState { view: (self.cam.view().x, self.cam.view().y, self.cam.view().zoom), local: self.sim.local, campaign: self.campaign, speed: self.speed, logos: self.logos.to_vec(), net: None }
    }

    /// The GUI's wheel events (code 10 to the dialog under the cursor): the message window and
    /// settings, the save page (menu.rs), the transport page (pages.rs).
    fn wheel_events(&mut self, gpu: &mut Gpu, ev: &[s4_viewer::guistate::UiEvent]) {
        self.msg_events(gpu, ev);
        self.menu_events(gpu, ev);
        Input { gpu, scene: &mut self.scene, view: self.cam.view(), sim: &self.sim, build: &mut self.build, pointer: &mut self.pointer, bdlg: &mut self.bdlg, pages: &mut self.pages }.ui_events(ev);
    }

    /// The load / save submenu's events (menu.rs).
    fn menu_events(&mut self, gpu: &mut Gpu, ev: &[s4_viewer::guistate::UiEvent]) {
        let action = self.menu.events(gpu, ev);
        let snd = std::mem::take(&mut self.menu.sounds);
        if let Some(s) = &mut self.sound {
            s.ui(&snd);
        }
        match action {
            Some(menu::MenuAction::Save(name)) => {
                if self.save_as(gpu, &name, 0) {
                    self.menu.init(gpu);
                }
            }
            Some(menu::MenuAction::Load(p)) => self.load_request = Some(p),
            Some(menu::MenuAction::Quit) => self.quit = true,
            None => {}
        }
        client::recompose(gpu);
    }

    /// `CGameRun::SaveGame` under `name` (`%APPDATA%\s4rs\saves\<name>.s4rs`), with a thumbnail
    /// of the screen. Returns whether it was written. `kind`: 0 named, 1 QuickSave, 2 AutoSave
    /// (the save event's data, for the messages 0x5172f0 posts: `Messages::saved`).
    fn save_as(&mut self, gpu: &mut Gpu, name: &str, kind: u8) -> bool {
        let Some(path) = menu::file_for(name) else {
            tracing::warn!("save: no save folder");
            return false;
        };
        // `CStateGame::CanSave` (release 0x515880): in a network game, more than 840 ticks since
        // the last save.
        if self.network && self.cur.tick.saturating_sub(self.last_save) <= 840 {
            tracing::warn!("save: less than 840 ticks since the last save");
            return false;
        }
        let (w, h) = gpu.size();
        let thumb = capture_game(gpu, self.cam.view()).map(|rgba| save::thumbnail(&rgba, w, h)).unwrap_or_default();
        match self.sim.save(&path, name, self.client_state(), thumb) {
            Ok(hd) => {
                tracing::info!("saved \"{name}\" at tick {}", hd.tick);
                self.last_save = hd.tick;
                let snd = self.messages.saved(kind, true);
                if let Some(s) = &mut self.sound {
                    s.ui(&snd);
                }
                true
            }
            Err(e) if self.network => {
                // Scheduled as a lockstep command: every peer saves at that tick.
                tracing::info!("save \"{name}\": {e}");
                self.last_save = self.cur.tick;
                true
            }
            Err(e) => {
                tracing::warn!("save: {e}");
                let snd = self.messages.saved(kind, false);
                if let Some(s) = &mut self.sound {
                    s.ui(&snd);
                }
                false
            }
        }
    }

    /// `CheckAutosaveTimer` (release 0x5158c0): with `IsAutosaveEnabled`, when the seconds since
    /// the last save (ticks / 14) exceed `max(60 · AutoSaveInterval, 60)`, save type 2
    /// ("AutoSave"). [inf] Network games: only single player here (network saves go through the
    /// lockstep).
    fn autosave(&mut self, gpu: &mut Gpu) {
        let Some(interval) = self.autosave_interval else { return };
        let limit = (60 * interval).max(60);
        let tick = self.cur.tick;
        if tick.saturating_sub(self.last_save) / 14 > limit && self.decided == 0 {
            self.save_as(gpu, "AutoSave", 2);
        }
    }

    fn mouse(&mut self, gpu: &mut Gpu, cursor: (f64, f64), button: MouseButton, state: ElementState) {
        // Both buttons (camera.rs): the second press starts the zoom, a release ends it.
        let pressed = state == ElementState::Pressed;
        if self.cam.both_held() {
            if !pressed {
                match button {
                    MouseButton::Left => self.left_held = false,
                    MouseButton::Right => {}
                    _ => return,
                }
                self.cam.both_up();
            }
            return;
        }
        let other_held = match button {
            MouseButton::Left => self.cam.right_held(),
            MouseButton::Right => self.left_held,
            _ => false,
        };
        if pressed && other_held && client::minimap_point(gpu, cursor).is_none() {
            if button == MouseButton::Left {
                self.left_held = true;
            }
            // [inf] The pending map press (box selection) is dropped.
            self.pointer.cancel(gpu);
            self.cam.both_down(cursor);
            return;
        }
        let scr = self.screen(gpu);
        let view = self.cam.view();
        let mut inp = Input { gpu, scene: &mut self.scene, view, sim: &self.sim, build: &mut self.build, pointer: &mut self.pointer, bdlg: &mut self.bdlg, pages: &mut self.pages };
        let ev = match (button, state) {
            (MouseButton::Left, s) => {
                self.left_held = s == ElementState::Pressed;
                // The minimap (0x514ce0): the camera jumps there.
                if s == ElementState::Pressed
                    && !self.cam.right_held()
                    && let Some(m) = client::minimap_point(inp.gpu, cursor)
                {
                    self.cam.jump(client::view_from_minimap(inp.gpu, view, m, inp.scene.source.w), &scr);
                }
                inp.left(cursor, s == ElementState::Pressed)
            }
            (MouseButton::Right, ElementState::Pressed) => {
                // EventManagement case 9 on the minimap: GUI event 0x1e0 with its node at once.
                if let Some(m) = client::minimap_point(inp.gpu, cursor) {
                    let node = client::minimap_order_node(inp.gpu, view, m, inp.scene.source.w);
                    inp.right_click_node(Some(node));
                } else if !inp.right_down(cursor) {
                    self.cam.right_down(cursor, &scr);
                }
                Vec::new()
            }
            (MouseButton::Right, ElementState::Released) => {
                if let Some(p) = self.cam.right_up() {
                    inp.right_click(p);
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
        if !ev.is_empty() {
            self.menu_events(gpu, &ev);
            self.msg_events(gpu, &ev);
        }
        let clicks = std::mem::take(&mut self.pointer.sounds);
        if let Some(s) = &mut self.sound {
            s.ui(&clicks);
        }
    }

    /// The message window's GUI events (msgwin.rs): a line's message centres the view.
    fn msg_events(&mut self, gpu: &mut Gpu, ev: &[s4_viewer::guistate::UiEvent]) {
        let (centre, snd) = msgwin::ui(ev, gpu, &mut self.messages, self.extended_tooltip);
        if let Some(s) = &mut self.sound {
            s.ui(&snd);
        }
        if let Some((x, y)) = centre {
            self.centre_on(gpu, (x as usize, y as usize));
        }
        // The zoom slider (dialog 0, knob 0x265 → event 0x12).
        slider_zoom(&mut self.cam, gpu, ev, self.scene.source.w);
        client::minimap_toggles(&mut self.scene, ev);
        // The settings pages (options.rs).
        let ch = options::ui(ev, gpu, &mut self.settings, Some(&mut self.messages), !cfg!(test));
        if ch.volumes
            && let Some(s) = &mut self.sound
        {
            s.audio.set_volumes(self.settings.sound_level(), self.settings.music_level(), false);
        }
        if ch.game {
            self.extended_tooltip = extended_tooltips(&self.settings, &self.sim);
            self.cam.border = self.settings.border_scroll;
            self.cam.step = self.settings.scroll_step;
        }
    }

    /// The cursor moved to `p` (window px): the border flags, a drag, the both-button zoom
    /// (camera.rs).
    fn cursor_moved(&mut self, gpu: &mut Gpu, p: (f64, f64)) {
        self.cursor_last = p;
        let held = self.left_held || self.cam.right_held() || self.cam.both_held();
        let s = self.screen(gpu);
        self.cam.moved(p, &s, held);
        if self.cam.both_held() {
            let v = client::slider_of_zoom((self.cam.zoom_target() * s.hidpi * 24.0 * 65536.0).round() as i32);
            client::zoom_slider(gpu, v);
        }
    }

    /// A click on control `link` of the open dialog `dialog`, as the keys make it, unless the
    /// control is pressed.
    fn click_control(&mut self, gpu: &mut Gpu, dialog: usize, link: u16) {
        let Some(ui) = gpu.ui.as_ref() else { return };
        let pressed = ui.state.index_of(dialog, link).is_none_or(|i| ui.state.is_pressed(dialog, i));
        if pressed || !ui.state.open.contains(&dialog) {
            return;
        }
        let Some(p) = client::control_point(gpu, dialog, link) else { return };
        self.mouse(gpu, p, MouseButton::Left, ElementState::Pressed);
        self.mouse(gpu, p, MouseButton::Left, ElementState::Released);
        // The release calls the control's callback (0x48979a) and moves nothing: the hover (and
        // its tooltip) goes back to where the cursor is.
        let (ev, _) = client::gui_mouse(gpu, self.cursor_last, GuiMouse::Move);
        self.msg_events(gpu, &ev);
    }

    /// Typed text for a focused edit field of the GUI (the save name, the chat input). Returns
    /// whether it took it.
    fn type_text(&mut self, gpu: &mut Gpu, text: &str) -> bool {
        let Some(ui) = gpu.ui.as_mut() else { return false };
        let Some((d, l)) = ui.state.focus.filter(|(d, _)| ui.state.open.contains(d)) else { return false };
        // The save name: 16 characters (edit properties 0x0410); the chat input: [inf] the
        // 300-byte `TextInputBuffers` slot.
        let max = if l == s4_viewer::msg_window::CHAT_INPUT && ui.msg_dialog == Some(d) { 299 } else { 16 };
        let ev = ui.state.type_text(text, max);
        self.menu_events(gpu, &ev);
        let snd = msgwin::chat_events(&ev, gpu, &self.sim, &mut self.console, Some(&mut self.messages));
        if let Some(s) = &mut self.sound {
            s.ui(&snd);
        }
        true
    }

    /// `IngameKeybindings` 0x4890b0 for a typed character (GUI key event 0xd; Ctrl makes a
    /// control character of it, so no letter): nothing while a tutorial runs (`CGuiEventHandler
    /// +0x40`), else `SelectControl` (0x677430) of a main tab and one of its submenu's tabs:
    /// F the settlers' tab and UI event 0x1e5 (state 0 after the tab's 0x192: 0x4ea370 → GUI
    /// event 0x278 → `SelectControl(4, 748)`), Q / E / R / T the build tabs 536 / 538..=540 (Alt+Q
    /// (flags 0x10): the extras' quit 930), X / C / Y / Z the goods' 1067 / 1068 / 1069, M the
    /// extras' 929 with a mission goal or in game type 3 (briefing.rs).
    /// Left out on the user's request (W / A / S / D are camera keys, camera.rs): the
    /// release's W → 606 + 537, A → 607 + 747, S → 607 + 746, D → 607 + 745.
    fn char_key(&mut self, gpu: &mut Gpu, c: char) {
        if self.sim.tutorial.lock().unwrap_or_else(|e| e.into_inner()).active {
            return;
        }
        let Some(ui) = gpu.ui.as_ref() else { return };
        let briefing = !ui.state.page_info.briefing.1.is_empty() || self.cur.population.economy.game_type == 3;
        let presses: &[(usize, u16)] = match c {
            'C' | 'c' => &[(0, 608), (5, 1068)],
            'E' | 'e' => &[(0, 606), (3, 538)],
            'F' | 'f' => &[(0, 607), (4, 748)],
            'M' | 'm' if briefing => &[(0, 611), (8, 929)],
            'Q' | 'q' if self.alt => &[(0, 611), (8, 930)],
            'Q' | 'q' => &[(0, 606), (3, 536)],
            'R' | 'r' => &[(0, 606), (3, 539)],
            'T' | 't' => &[(0, 606), (3, 540)],
            'X' | 'x' => &[(0, 608), (5, 1067)],
            'Y' | 'y' | 'Z' | 'z' => &[(0, 608), (5, 1069)],
            _ => return,
        };
        let mut ev = Vec::new();
        for &(dialog, link) in presses {
            if let Some(ui) = gpu.ui.as_mut() {
                ev.extend(ui.state.select_control(dialog, link));
            }
        }
        Input { gpu, scene: &mut self.scene, view: self.cam.view(), sim: &self.sim, build: &mut self.build, pointer: &mut self.pointer, bdlg: &mut self.bdlg, pages: &mut self.pages }.ui_events(&ev);
        if !ev.is_empty() {
            self.menu_events(gpu, &ev);
            self.msg_events(gpu, &ev);
        }
        client::recompose(gpu);
    }

    fn key(&mut self, gpu: &mut Gpu, code: KeyCode, down: bool, repeat: bool) {
        let size = self.scene.source.w;
        let digit = match code {
            KeyCode::Digit0 => Some(10),
            KeyCode::Digit1 => Some(1),
            KeyCode::Digit2 => Some(2),
            KeyCode::Digit3 => Some(3),
            KeyCode::Digit4 => Some(4),
            KeyCode::Digit5 => Some(5),
            KeyCode::Digit6 => Some(6),
            KeyCode::Digit7 => Some(7),
            KeyCode::Digit8 => Some(8),
            KeyCode::Digit9 => Some(9),
            _ => None,
        };
        if let Some(g) = digit {
            if down && !repeat {
                self.group_key(gpu, g);
            }
            return;
        }
        match code {
            // `HandleF6_F7_Esc` 0x489440 (GUI key event 0xb): Esc (key 0x1b, 0x4896fc) clicks the
            // extras tab (dialog 0, 611) and its quit tab (dialog 8, 0x3a2 = 930 at 0x489815: page
            // 34); F6 (key 0x75, 0x489506) the extras tab and its message recipients tab (0x3a3 =
            // 931 at 0x48962f: UI event 0x1e → GUI event 0x26c → page 58). Each only when it is
            // not pressed (0x675ad0, the +0x1b test). The extras tab's 0x192 deselects and leaves
            // the modes. [inf] F6's `+0x40` condition (also tested for the 0x27 page) taken as clear.
            KeyCode::Escape | KeyCode::F6 if down => {
                let second = if code == KeyCode::Escape { 930 } else { 931 };
                for (dialog, link) in [(0, 611), (8, second)] {
                    self.click_control(gpu, dialog, link);
                }
            }
            // F7 (0x48948f, key 0x76): the clock toggled unless a script's countdown runs
            // (s4_viewer::clock).
            // [ours] F5 (no key of the release, see monitor/mod.rs): the cloud shadows, remembered
            // in the settings.
            KeyCode::F5 if down && !repeat => {
                gpu.effects.clouds = !gpu.effects.clouds;
                self.settings.effects.clouds = gpu.effects.clouds;
                let mut st = settings::Settings::load();
                st.effects.clouds = gpu.effects.clouds;
                st.save();
            }
            KeyCode::F7 if down => {
                let screen_w = (gpu.size().0 as f32 / gpu.gui_scale.max(0.1)) as u32;
                if let Some(ui) = gpu.ui.as_mut() {
                    ui.clock.key(&mut ui.state, screen_w);
                    let tick = self.cur.tick;
                    ui.clock.timer(&mut ui.state, tick);
                }
                client::recompose(gpu);
            }
            // Ctrl+S: QuickSave (release key switch at 0x518a2c: 'S' with MK_CONTROL → SaveGame(1)).
            KeyCode::KeyS if down && !repeat && self.pointer.ctrl => {
                self.save_as(gpu, "QuickSave", 1);
            }
            // The arrows and W / A / S / D scroll while held (camera.rs, not the
            // original's 6 × `ScrollStepValue` jumps per key event). A letter with Ctrl or Alt is
            // not a scroll key; a release always counts.
            _ if let Some(k) = scroll_key(code) => {
                let letter = !matches!(code, KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::ArrowUp | KeyCode::ArrowDown);
                if !down || !(letter && (self.pointer.ctrl || self.alt)) {
                    self.cam.key(k, down);
                }
            }
            // 0x21 PageUp / 0x6d numpad '-': 0x519870(5); 0x22 PageDown / 0x6b numpad '+':
            // 0x5198e0(5) (each key event).
            KeyCode::PageUp | KeyCode::NumpadSubtract if down => zoom_camera(&mut self.cam, gpu, 5, None, size),
            KeyCode::PageDown | KeyCode::NumpadAdd if down => zoom_camera(&mut self.cam, gpu, -5, None, size),
            // `IngameKeybindings` 0x4890b0: Enter (0xd, not with Ctrl / Shift) opens the window
            // (event 0x269) and the chat input (`UIChatInteraction` 0x499980).
            KeyCode::Enter | KeyCode::NumpadEnter if down && !repeat && !self.pointer.ctrl && !self.pointer.shift => {
                let snd = msgwin::chat_key(gpu, &mut self.messages, self.extended_tooltip);
                if let Some(s) = &mut self.sound {
                    s.ui(&snd);
                }
            }
            // The space bar (key events 0xb / 0x14): the tutorial's `SPACE`, once per press.
            KeyCode::Space if !repeat => tutorial::space(&self.sim.tutorial, down),
            // 0x24 Home: event 0x13 to the player's start (0x149b5d8 / 0x149b5dc).
            // VK_BACK (key case 0xb, 8): UI event 0x19f with Shift (key flags & 4) in w, only with
            // a selection (state 1); 0x4ecda0 keeps the local player's selected settlers whose
            // hitpoints are below half the type's maximum (`SettlerInfo +2`, table 0x1546930), with
            // Shift those at half or above; other entities stay. (Tab, key 9 -> event 0x6b, is the
            // voice chat's push-to-talk: `CVC_EventHandle` 0x697e20; there is no voice chat here.)
            KeyCode::Backspace if down => {
                let state = self.bdlg.state_of(self.build.building.is_some(), &self.pointer);
                if input::transition(state, 0x19f).is_some() {
                    let pop = self.cur.population.clone();
                    let ids = bdlg::by_health(&pop, &self.pointer.selection.ids, self.pointer.shift);
                    client::narrow_selection(gpu, &pop, &mut self.pointer.selection, ids);
                }
            }
            // VK_HOME (key case 0x24) and the character 'H' (case 0xd, 0x48 / 0x68): event 0x13 to
            // the local player's start position.
            KeyCode::Home | KeyCode::KeyH if down => {
                let l = self.sim.local as usize;
                if let Some(&(x, y)) = self.cur.population.starts.get(l) {
                    self.centre_on(gpu, (x as usize, y as usize));
                }
            }
            _ if !down || repeat => {}
            // 'P' (case 0xd, 0x50 / 0x70) and VK_PAUSE (case 0x13): 0x514be0.
            KeyCode::KeyP | KeyCode::Pause => {
                let paused = self.cur.tick_ms == 0;
                self.sim.set_speed(if paused { self.speed.max(1) } else { 0 });
            }
            KeyCode::Equal => {
                self.speed = (self.speed + 1).min(8);
                self.sim.set_speed(self.speed);
            }
            KeyCode::Minus => {
                self.speed = self.speed.saturating_sub(1).max(1);
                self.sim.set_speed(self.speed);
            }
            KeyCode::KeyG => gpu.show_gui = !gpu.show_gui,
            _ => {}
        }
    }

    fn redraw(&mut self, gpu: &mut Gpu, cursor: (f64, f64), window: Option<&Window>) {
        let pre = perf::span("redraw: before present");
        // The frame's effects change the sprite scene many times in a fight: the renderer takes
        // it once, at the end of `present`.
        if let Some(r) = &mut gpu.sprites {
            r.hold = true;
        }
        // The script's messages first: its player-lost events precede the decision (0x38 before
        // 0x37), and `client_frame` would take the requests.
        let own = self.settings.player_name.clone();
        let sim = &self.sim;
        let snd = msgwin::script_messages(gpu, &sim.tutorial, &mut self.messages, sim.local, &|p| msgwin::player_name(sim, &own, p));
        if let Some(s) = &mut self.sound {
            s.ui(&snd);
        }
        // `GlobalGameState +0x2a8` (guistate `game_kind`).
        if let Some(ui) = gpu.ui.as_mut() {
            ui.state.game_kind = if self.network {
                2
            } else if ui.tutorial {
                4
            } else if self.campaign.is_some() {
                3
            } else {
                1
            };
        }
        self.check_outcome(gpu);
        self.autosave(gpu);
        let tf = msgwin::tutorial(gpu, &self.sim.tutorial, self.extended_tooltip);
        Input { gpu, scene: &mut self.scene, view: self.cam.view(), sim: &self.sim, build: &mut self.build, pointer: &mut self.pointer, bdlg: &mut self.bdlg, pages: &mut self.pages }.tutorial_frame(tf);
        tutorial::publish_selection(&self.sim.tutorial, &self.pointer.selection.ids);
        let (v, exit) = tutorial::client_frame(&self.sim.tutorial, gpu, &mut self.scene, self.cam.view(), self.cur.tick as u64);
        if v != self.cam.view() {
            self.jump(gpu, v);
        }
        if exit {
            self.quit = true;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        // The camera by real time (camera.rs); no keys while an edit field (the chat, a save
        // name) has the keyboard.
        if text_focus(gpu) {
            self.cam.release_keys();
        }
        let s = self.screen(gpu);
        self.cam.update(dt, &s);
        // Lock-free: the snapshot stays valid while we draw from it.
        let latest = self.sim.snapshot();
        let ticks = self.sim.drain_events();
        if latest.tick != self.cur.tick {
            self.prev = std::mem::replace(&mut self.cur, latest);
        } else {
            // Same tick, republished (build markers changed).
            self.cur = latest;
        }
        self.update_cursor();
        let calls = sound::drain_gui_calls(gpu);
        let mut msg_sounds = Vec::new();
        for (tick, events) in &ticks {
            msg_sounds.extend(self.messages.sim_events(self.sim.local, *tick, events));
        }
        if self.msg_timer.poll(self.msg_clock.unwrap_or_else(|| self.msg_epoch.elapsed())) {
            // Event 0x15: the clock (s4_viewer::clock), then the message button.
            if let Some(ui) = gpu.ui.as_mut()
                && ui.clock.timer(&mut ui.state, self.cur.tick)
            {
                client::recompose(gpu);
            }
            msg_sounds.extend(self.messages.timer());
        }
        // Chat lines that arrived (EventManagement 0xfa5).
        let own = self.settings.player_name.clone();
        let sim = &self.sim;
        msg_sounds.extend(msgwin::chat_arrivals(sim, &mut self.messages, &self.cur.population.teams, &|p| msgwin::player_name(sim, &own, p)));
        msg_sounds.extend(msgwin::net_notices(gpu, sim, &mut self.messages, &|p| msgwin::player_name(sim, &own, p)));
        msgwin::sync(gpu, &mut self.messages, self.extended_tooltip);
        msgwin::tooltip(gpu, Some(cursor));
        // The buildings' patches: triggers, then one `FillGfxInfo` step of the drawn ones.
        let lis = sound::Listener { camera: self.cam.view(), draw_zoom: client::eff_zoom(gpu, self.cam.view()), size: gpu.size() };
        let mut due = Vec::new();
        for (_, events) in &ticks {
            for e in events {
                if let s4_sim::event::SimEvent::JobTrigger { building, trigger } = *e {
                    due.extend(self.patches.job_trigger(&self.cur.population, building, trigger));
                }
            }
        }
        let range = sound::DrawRange::new(&self.cur, &lis);
        due.extend(self.patches.draw(&self.cur.population, self.cur.population.tick, |x, y| range.object_drawn(x, y)));
        farm_pulls(gpu, &mut self.scene, &mut self.patches, &self.cur.population);
        let ts = perf::span("redraw: sounds, ambient");
        if let Some(snd) = &mut self.sound {
            snd.gui(sound::gui::Procs::Game, &calls);
            snd.ui(&msg_sounds);
        }
        for (tick, events) in &ticks {
            if let Some(snd) = &mut self.sound {
                snd.tick(events, &self.cur, &lis);
            }
            ambient_tick(&mut self.ambient, &mut self.scene, &self.cur, &lis, *tick, events, self.sound.as_mut());
        }
        if let Some(snd) = &mut self.sound {
            snd.patch_sounds(&due);
            snd.draw(&self.cur, &lis);
        }
        drop(ts);
        // The ambient creatures' `AddEffect` (a creature on dark land vanishes).
        for (effect, x, y, tick) in self.ambient.as_mut().map(|a| std::mem::take(&mut a.added)).unwrap_or_default() {
            if let Err(err) = self.scene.add_effect(gpu, effect as u8, x as u16, y as u16, tick as u64) {
                tracing::warn!("effect {effect:#x}: {err}");
            }
        }
        // `CEffects::AddEffect` (the sim's crush / destroy effects): drawn from their tick on.
        for (tick, events) in &ticks {
            for e in events {
                let start = match *e {
                    s4_sim::event::SimEvent::Effect { effect, x, y, .. } => Some((effect, x, y, *tick)),
                    s4_sim::event::SimEvent::EffectLater { effect, x, y, delay, .. } => Some((effect, x, y, *tick + delay)),
                    _ => None,
                };
                if let Some((effect, x, y, at)) = start
                    && let Err(err) = self.scene.add_effect(gpu, effect, x as u16, y as u16, at as u64)
                {
                    tracing::warn!("effect {effect:#x}: {err}");
                }
                add_projectile(gpu, &mut self.scene, *tick, e);
                add_tribe_effect(gpu, &mut self.scene, &self.cur.population, *tick, e);
            }
        }
        let hover = client::hover_node(gpu, &self.scene, self.cam.view(), (cursor.0 as f32, cursor.1 as f32));
        let mut markers = self.build.display(&self.cur, hover);
        markers.extend(self.bdlg.markers(&self.cur));
        let ghost = self.build.ghost(&self.cur, hover);
        let (sel_icons, _) = client::selection_frame(gpu, &self.cur.population, &mut self.pointer.selection, self.pointer.player);
        self.bdlg.frame(gpu, &self.sim, &self.pointer);
        // The "next building" button centres the view on it (event 0x13).
        if let Some((x, y)) = self.bdlg.centre.take().or(self.pages.centre.take()) {
            self.centre_on(gpu, (x as usize, y as usize));
        }
        let ambient = self.ambient.as_ref().map(|a| a.sprites(self.cur.tick)).unwrap_or_default();
        drop(pre);
        let alpha = self.alpha.unwrap_or_else(|| interp::alpha(&self.cur, now));
        if self.cur.tick_ms != 0 {
            gpu.cloud_time += dt as f64;
        }
        if let Err(e) = present(gpu, &mut self.scene, &mut self.base, &self.prev, &self.cur, alpha, &markers, ghost, &sel_icons, &self.patches.shown(), &ambient, &mut self.offsets) {
            tracing::warn!("scene: {e}");
        }
        if let Some(snd) = &mut self.sound {
            let _t = perf::span("redraw: sound frame");
            snd.frame(std::time::Duration::from_secs_f32(dt));
        }
        if gpu.show_gui {
            let _t = perf::span("redraw: gui pages, minimap");
            client::update_gui_pages(gpu, &self.scene, self.cam.view(), &self.cur.population);
            client::update_minimap(gpu, &mut self.scene, self.cam.view(), 1);
        }
        if let Some(w) = window {
            w.set_title(&format!("s4rs: tick {} ({:.1} s) speed {}", self.cur.tick, self.cur.tick as f32 * 0.071, if self.cur.tick_ms == 0 { 0 } else { self.speed }));
        }
        let _t = perf::span("gpu: draw");
        gpu.draw(self.cam.view());
    }
}

/// The GPU without a previous game's or the menu's layers.
/// The game's frame without our tooling over it (the monitor): what a save's thumbnail shows.
fn capture_game(gpu: &mut Gpu, view: View) -> anyhow::Result<Vec<u8>> {
    let overlay = gpu.overlay.take();
    let r = gpu.capture(view);
    gpu.overlay = overlay;
    r
}

fn gpu_reset(gpu: &mut Gpu) -> &mut Gpu {
    gpu.gui_origin = (0.0, 0.0);
    gpu.terrain = None;
    gpu.sprites = None;
    gpu.gui = None;
    gpu.ui = None;
    gpu.camera_window = None;
    gpu
}

/// The window: the start screens until a map is started (or `--map` given), then the game.
struct App {
    args: Args,
    settings: settings::Settings,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    cursor: (f64, f64),
    front: Option<frontend::Frontend>,
    game: Option<Game>,
    /// The start screens' music (sound.rs `MenuMusic`), while they are shown.
    menu_music: Option<sound::MenuMusic>,
    /// The credits' playlist is on (`sound::MenuMusic::credits`).
    credits_music: bool,
    /// The game's 36 cursors (`CursorList`, cursor.rs) and the one shown.
    cursors: Vec<winit::window::CustomCursor>,
    cursor_shown: Option<u8>,
    /// `Logo\Current` (frontend.rs `Frontend::logos`): kept between the start screens and the
    /// games; every game composes its sprite file 1 from it.
    logos: [Option<Vec<u8>>; 8],
    /// The modifier keys held (Alt+Enter).
    mods: winit::keyboard::ModifiersState,
    /// `--perf`: frames presented since this time ([`App::perf_log`]).
    perf: Option<(Instant, u32)>,
    /// The videos (video.rs): the cache and its conversion, the one playing (`CStateVideo`), the
    /// ones queued after it.
    videos: video::Videos,
    playing: Option<video::Playing>,
    video_queue: std::collections::VecDeque<u8>,
}

/// Save the app's copy of the settings, which F5 in a game has not reached.
fn save_app_settings(st: &mut settings::Settings, gpu: Option<&Gpu>) {
    if let Some(g) = gpu {
        st.effects.clouds = g.effects.clouds;
    }
    st.save();
}

/// The window mode (`GAMESETTINGS` `Fullscreen`): the original's fullscreen is a frameless
/// window the size of its monitor (0x505290: `MonitorFromWindow(.., MONITOR_DEFAULTTONEAREST)`,
/// the style without `WS_OVERLAPPEDWINDOW` 0xcf0000, `SetWindowPos` to the monitor's size), not
/// a display mode change; the plain window is `WindowWidth` × `WindowHeight`.
fn window_mode(fullscreen: bool) -> Option<winit::window::Fullscreen> {
    fullscreen.then_some(winit::window::Fullscreen::Borderless(None))
}

/// The plain window inside its monitor's work area (a modern extra: a size saved on a larger
/// monitor must not overflow; the original keeps `WindowWidth` / `WindowHeight` as they are):
/// when the frame does not fit, the client shrinks by what is missing and the window moves to
/// the work area's top left.
fn fit_to_work_area(w: &Window) {
    let Some((x, y, aw, ah)) = work_area(w) else { return };
    let (outer, inner) = (w.outer_size(), w.inner_size());
    if outer.width <= aw && outer.height <= ah {
        return;
    }
    let (fw, fh) = (outer.width.saturating_sub(inner.width), outer.height.saturating_sub(inner.height));
    let _ = w.request_inner_size(winit::dpi::PhysicalSize::new(inner.width.min(aw.saturating_sub(fw)).max(1), inner.height.min(ah.saturating_sub(fh)).max(1)));
    w.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
}

/// The work area (physical px: x, y, width, height) of the monitor the window is on
/// (`GetMonitorInfoW` of `MonitorFromWindow(.., MONITOR_DEFAULTTONEAREST)`).
#[cfg(windows)]
fn work_area(w: &Window) -> Option<(i32, i32, u32, u32)> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    let RawWindowHandle::Win32(h) = w.window_handle().ok()?.as_raw() else { return None };
    // SAFETY: a live window handle; MONITORINFO is plain data with its size set.
    unsafe {
        let m = MonitorFromWindow(h.hwnd.get(), MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(m, &mut info) == 0 {
            return None;
        }
        let r = info.rcWork;
        Some((r.left, r.top, (r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32))
    }
}

/// Elsewhere: the whole monitor.
#[cfg(not(windows))]
fn work_area(w: &Window) -> Option<(i32, i32, u32, u32)> {
    let m = w.current_monitor()?;
    let (p, s) = (m.position(), m.size());
    Some((p.x, p.y, s.width, s.height))
}

impl App {
    /// `--perf`: every 2 s the frames presented per second and the spans' time per frame
    /// (s4_viewer::perf), the longest first.
    fn perf_log(&mut self) {
        let Some((since, frames)) = &mut self.perf else { return };
        *frames += 1;
        let dt = since.elapsed();
        if dt.as_secs_f64() < 2.0 {
            return;
        }
        let n = *frames as f64;
        let mut sums: Vec<_> = perf::take().into_iter().collect();
        sums.sort_by_key(|s| std::cmp::Reverse(s.1.total));
        let top: Vec<String> = sums.iter().take(8).map(|(k, s)| format!("{k} {:.2} (max {:.1})", s.total.as_secs_f64() * 1000.0 / n, s.max.as_secs_f64() * 1000.0)).collect();
        tracing::info!("{:.1} fps; ms per frame: {}", n / dt.as_secs_f64(), top.join(", "));
        self.perf = Some((Instant::now(), 0));
    }

    /// Alt+Enter (`CGameStateEventHandle` 0x5026f0, event 0xc with key 0x0d and flag 0x10 at
    /// 0x502a04): `Fullscreen` flips, the settings are written (0x45b2a0) and the window takes the
    /// new mode (0x5051a0 → 0x504b30 → 0x505290); the GUI is laid out for the new size (here by
    /// the resize that follows).
    fn toggle_fullscreen(&mut self) {
        let Some(w) = &self.window else { return };
        self.settings.fullscreen = w.fullscreen().is_none();
        w.set_fullscreen(window_mode(self.settings.fullscreen));
        save_app_settings(&mut self.settings, self.gpu.as_ref());
    }

    /// `IGfxEngine::SetCursorShape`: show cursor `shape` (`CursorList` index).
    fn set_cursor(&mut self, shape: u8) {
        if self.cursor_shown == Some(shape) {
            return;
        }
        if let (Some(w), Some(c)) = (&self.window, self.cursors.get(shape as usize)) {
            w.set_cursor(c.clone());
            self.cursor_shown = Some(shape);
        }
    }

    /// Play the next queued video (those that cannot play are skipped: no file, not converted,
    /// `ShowVideos` off); with none left, the start screens go on to what follows
    /// ([`frontend::Frontend::video_done`]). The cursor is hidden meanwhile (`SetCursorShape(0)`
    /// before the open, docs/VIDEO.md).
    fn next_video(&mut self) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        if let Some(p) = self.playing.take() {
            p.close(gpu);
        }
        while let Some(n) = self.video_queue.pop_front() {
            if !self.settings.show_videos {
                continue;
            }
            if let Some(p) = video::file(n, self.settings.language).and_then(|f| self.videos.open(&f)) {
                tracing::info!("video {n}: playing");
                self.playing = Some(p);
                if let Some(w) = &self.window {
                    w.set_cursor_visible(false);
                }
                return;
            }
        }
        if let Some(w) = &self.window {
            w.set_cursor_visible(true);
        }
        if let Some(f) = &mut self.front {
            f.video_done(gpu);
        }
    }

    /// A video the start screens queued before their next screen.
    fn front_video(&mut self) {
        if self.playing.is_none()
            && let Some(n) = self.front.as_mut().and_then(|f| f.take_video())
        {
            self.video_queue.push_back(n);
            self.next_video();
        }
    }

    /// A window event while a video plays (handler 0x528760): a left button release or Esc ends
    /// it, the window going inactive pauses it; other input goes nowhere. (The release also ends
    /// it on `WM_SIZE`; ours does not: a fullscreen switch or the first resize would cut it.)
    /// True when the event is used up.
    fn video_event(&mut self, event: &WindowEvent) -> bool {
        let Some(p) = self.playing.as_mut() else { return false };
        match event {
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => self.next_video(),
            WindowEvent::KeyboardInput { event: k, .. } if k.state == ElementState::Pressed && k.physical_key == PhysicalKey::Code(KeyCode::Escape) => self.next_video(),
            WindowEvent::Focused(on) => p.pause(!on),
            WindowEvent::KeyboardInput { event: k, .. } if self.mods.alt_key() && matches!(k.physical_key, PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter)) => return false,
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::KeyboardInput { .. } => {}
            _ => return false,
        }
        true
    }

    /// The video's frame: true while one plays (nothing else is drawn).
    fn video_frame(&mut self) -> bool {
        if self.playing.is_none() {
            return false;
        }
        // The open closes every Miles stream (0x69aa60): the menu music stops, and the next screen
        // starts it afresh.
        sound::MenuMusic::follow(&mut self.menu_music, false, &self.settings);
        let volume = if self.settings.sound_enabled || self.settings.music_enabled { 1.0 } else { 0.0 };
        let (Some(gpu), Some(p)) = (self.gpu.as_mut(), self.playing.as_mut()) else { return false };
        if p.tick(gpu, &self.videos, volume) {
            gpu.draw(View { x: 0.0, y: 0.0, zoom: 1.0 });
        } else {
            self.next_video();
        }
        true
    }

    /// What the start screens asked for.
    fn front_action(&mut self, el: &ActiveEventLoop, act: Option<frontend::Action>) {
        if let Some(f) = &self.front {
            self.logos = f.logos.clone();
        }
        match act {
            Some(frontend::Action::Start(p, campaign, players)) => {
                let map = p.to_string_lossy().into_owned();
                self.args.players = players;
                self.start(el, &map, None);
                self.args.players = None;
                if let Some(g) = &mut self.game
                    && let Some(gpu) = self.gpu.as_mut()
                {
                    g.campaign = campaign;
                    g.set_briefing(gpu);
                }
            }
            Some(frontend::Action::StartNet(session, p, players)) => {
                let map = p.to_string_lossy().into_owned();
                // A loaded network game: this peer's own save of it.
                let (hash, local) = (session.settings.load_hash, session.local);
                let save = (hash != 0).then(|| save::network_saves(hash).into_iter().find(|s| s.1 == local)).flatten();
                if hash != 0 && save.is_none() {
                    tracing::error!("no save of player {local} for the loaded game {hash:016x}");
                    return;
                }
                self.args.load = save.map(|s| s.0.to_string_lossy().into_owned());
                self.start(el, &map, Some((session, players)));
                self.args.load = None;
            }
            Some(frontend::Action::Load(path)) => match save::read_header(&path) {
                Ok(h) => {
                    self.args.load = Some(path.to_string_lossy().into_owned());
                    self.start(el, &h.map, None);
                    self.args.load = None;
                }
                Err(e) => tracing::warn!("load: {e}"),
            },
            Some(frontend::Action::Quit) => el.exit(),
            None => {}
        }
    }

    /// Leave the game (page 34's yes, event 0x17 → `CStateGame::EndGame`): a campaign mission
    /// returns to its selection screen, any other game to the main menu.
    ///
    /// `EndGameGoToStatistics` 0x514450: the end statistics (frontend/endstat.rs) unless the game
    /// was a tutorial (`GlobalGameStateObject +0x3c` = 4); game type 3: its own screen 0x5128e0
    /// (frontend/race_end.rs).
    fn end_game(&mut self, el: &ActiveEventLoop) {
        let Some(gpu) = &mut self.gpu else { return };
        let Some(mut g) = self.game.take() else { return };
        g.hidden_last = gpu.ui.as_ref().map_or(0, |u| u.hidden_last);
        let campaign = g.campaign.map(|(campaign, level)| frontend::victory::Outcome { campaign, level, won: g.decided == script::WON });
        let tutorial = g.sim.tutorial.lock().unwrap_or_else(|e| e.into_inner()).active;
        let race = g.cur.population.economy.game_type == 3;
        let record = (!tutorial).then(|| g.end_record());
        // The goods race info object as D77's fill last left it (all 0 if never filled).
        let rows = gpu.ui.as_ref().and_then(|u| u.race_rows).unwrap_or_default();
        let goods: frontend::race_end::Goods = std::array::from_fn(|k| (rows[k].good as i32, rows[k].team, rows[k].rest));
        drop(g);
        frontend::Frontend::fit(gpu);
        let f = match (record, campaign) {
            (Some(rec), _) if race => frontend::Frontend::race_end(gpu, self.settings.language, rec, goods, campaign),
            (Some(rec), _) => frontend::Frontend::end_statistic(gpu, self.settings.language, rec, campaign),
            (None, Some(c)) => frontend::Frontend::after_mission(gpu, self.settings.language, c.campaign),
            (None, None) if tutorial => frontend::Frontend::after_tutorial(gpu, self.settings.language),
            (None, None) => frontend::Frontend::new(gpu, self.settings.language),
        };
        match f {
            Ok(mut f) => {
                f.logos = self.logos.clone();
                self.front = Some(f);
            }
            Err(e) => {
                tracing::error!("start screens: {e}");
                el.exit();
            }
        }
    }

    fn start(&mut self, el: &ActiveEventLoop, map: &str, net: Option<(Box<s4_net::Session>, sim::PlayerSetup)>) {
        // The game state's construction shows the hourglass (`CStateGame` 0x513190: shape 4).
        self.set_cursor(cursor::shape::WAIT);
        let Some(gpu) = &mut self.gpu else { return };
        match Game::start(map, &self.args, self.settings.speed, gpu, net) {
            Ok(mut g) => {
                // A loaded save brings its own logos (`CUserLogoChunk` 0x4c4790); they stay the
                // session's `Logo\Current`.
                if let Some(saved) = g.sim.loaded.as_ref().map(|c| &c.logos).filter(|l| l.len() == 8) {
                    self.logos = std::array::from_fn(|i| saved[i].clone());
                }
                g.logos = self.logos.clone();
                if let Some(s) = g.scene.sprites.as_mut() {
                    s.set_logos(&self.logos);
                }
                if self.settings.language != 0
                    && let Some(ui) = gpu.ui.as_mut()
                {
                    ui.set_language(self.settings.language);
                }
                self.front = None;
                self.game = Some(g);
            }
            Err(e) => {
                // The start screens stay (or come back); a failed start never ends the program.
                tracing::error!("cannot start {map}: {e}");
                if self.front.is_none() {
                    frontend::Frontend::fit(gpu);
                    match frontend::Frontend::new(gpu, self.settings.language) {
                        Ok(mut f) => {
                f.logos = self.logos.clone();
                self.front = Some(f);
            }
                        Err(e) => {
                            tracing::error!("start screens: {e}");
                            el.exit();
                        }
                    }
                }
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let s = &self.settings;
        let fullscreen = self.args.fullscreen.unwrap_or(s.fullscreen);
        let attrs = Window::default_attributes()
            .with_title("s4rs")
            .with_inner_size(LogicalSize::new(s.width, s.height))
            .with_fullscreen(window_mode(fullscreen));
        let window = Arc::new(el.create_window(attrs).expect("create window"));
        if !fullscreen {
            fit_to_work_area(&window);
        }
        let mut gpu = Gpu::new(window.clone()).expect("init GPU");
        gpu.scale_factor = window.scale_factor() as f32;
        gpu.effects = s.viewer_effects();
        // The 36 cursors of S4_Main.exe (`LoadCursorA` at the engine init, cursor.rs).
        match s4_formats::cursor::load() {
            Ok(list) => {
                self.cursors = list
                    .into_iter()
                    .filter_map(|c| {
                        let src = winit::window::CustomCursor::from_rgba(c.rgba, c.width as u16, c.height as u16, c.hotspot.0, c.hotspot.1).ok()?;
                        Some(el.create_custom_cursor(src))
                    })
                    .collect();
            }
            Err(e) => tracing::warn!("cursors: {e}"),
        }
        self.window = Some(window);
        self.gpu = Some(gpu);
        self.set_cursor(cursor::shape::ARROW);
        // `--load` is for this first start only: every later start (the menus) is a new game
        // unless a load is asked for there. (It used to stay in `args`, so a campaign mission
        // started from the menus loaded that save instead.)
        let load = self.args.load.clone();
        let startup = load.is_none() && !self.args.map_given;
        let first = match &load {
            Some(p) => match save::read_header(std::path::Path::new(p)) {
                Ok(h) => Some(if self.args.map_given { self.args.map.clone() } else { h.map }),
                Err(e) => {
                    tracing::error!("--load: {e}");
                    None
                }
            },
            None => self.args.map_given.then(|| self.args.map.clone()),
        };
        if let Some(map) = first {
            self.start(el, &map, None);
        }
        self.args.load = None;
        if self.game.is_none()
            && let Some(gpu) = &mut self.gpu
        {
            frontend::Frontend::fit(gpu);
            match frontend::Frontend::new(gpu, self.settings.language) {
                Ok(mut f) => {
                f.logos = self.logos.clone();
                self.front = Some(f);
            }
                Err(e) => {
                    tracing::error!("start screens: {e}");
                    el.exit();
                }
            }
        }
        // 0x503a58: a normal start shows the logos and video 0 before the main menu.
        if startup && self.front.is_some() {
            self.video_queue.extend(video::STARTUP);
            self.next_video();
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.video_event(&event) {
            return;
        }
        let Some(gpu) = &mut self.gpu else { return };
        // The monitor (F3, its button and window) first: what it takes the game does not get.
        if let Some(w) = self.window.clone()
            && !matches!(event, WindowEvent::RedrawRequested)
            && monitor::Monitor::of(gpu, Some(&w)).on_window_event(&w, &event)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                gpu.resize(s.width, s.height);
                if self.front.is_some() {
                    frontend::Frontend::fit(gpu);
                }
                // Only the plain window's size is kept (as `SaveWindowSettings` 0x45a100), and it
                // stays inside its monitor's work area (also when it comes back from fullscreen
                // with the size it was created with).
                if let Some(w) = &self.window
                    && w.fullscreen().is_none()
                {
                    fit_to_work_area(w);
                    let l = s.to_logical::<u32>(w.scale_factor());
                    (self.settings.width, self.settings.height) = (l.width, l.height);
                }
            }
            // The window went to a monitor of another scale: the in-game GUI follows it (the start
            // screens refit on the resize that comes with it).
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                gpu.scale_factor = scale_factor as f32;
                if self.front.is_none() {
                    gpu.gui_scale = gpu.scale_factor;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                match &mut self.front {
                    Some(f) => {
                        f.mouse(gpu, self.cursor, GuiMouse::Move);
                    }
                    None => {
                        let (ev, _) = client::gui_mouse(gpu, self.cursor, GuiMouse::Move);
                        if let Some(g) = &mut self.game {
                            g.msg_events(gpu, &ev);
                            let scale = gpu.gui_scale as f64;
                            g.pointer.cursor_shape = g.cursor_shape;
                            g.pointer.moved(gpu, self.cursor, scale);
                            g.cursor_moved(gpu, self.cursor);
                        }
                    }
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(g) = &mut self.game {
                    g.cam.left_window();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let up = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y > 0.0,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y > 0.0,
                };
                if let Some(f) = &mut self.front {
                    f.wheel(gpu, up);
                } else if let Some(g) = &mut self.game {
                    let delta120 = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => (y * 120.0) as i32,
                        winit::event::MouseScrollDelta::PixelDelta(p) => p.y as i32,
                    };
                    // The GUI first (0x672f90 event 0xe: code 10 to the dialog under the cursor),
                    // else the zoom.
                    let ev = client::gui_wheel(gpu, self.cursor, delta120);
                    if ev.is_empty() {
                        let size = g.scene.source.w;
                        zoom_camera(&mut g.cam, gpu, camera::wheel_units(delta120), Some(self.cursor), size);
                    } else {
                        g.wheel_events(gpu, &ev);
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(f) = &mut self.front {
                    let what = match (button, state) {
                        (MouseButton::Left, ElementState::Pressed) => GuiMouse::LeftDown,
                        (MouseButton::Left, ElementState::Released) => GuiMouse::LeftUp,
                        (MouseButton::Right, ElementState::Pressed) => GuiMouse::RightDown,
                        (MouseButton::Right, ElementState::Released) => GuiMouse::RightUp,
                        _ => return,
                    };
                    let act = f.mouse(gpu, self.cursor, what);
                    self.front_action(el, act);
                } else if let Some(g) = &mut self.game {
                    g.mouse(gpu, self.cursor, button, state);
                    if let Some(p) = g.load_request.take() {
                        let map = save::read_header(&p).map(|h| h.map);
                        self.end_game(el);
                        match map {
                            Ok(map) => {
                                self.front = None;
                                self.args.load = Some(p.to_string_lossy().into_owned());
                                self.start(el, &map, None);
                                self.args.load = None;
                            }
                            Err(e) => tracing::warn!("load: {e}"),
                        }
                    } else if g.quit {
                        self.end_game(el);
                    }
                }
            }
            // Keys released while the window is not focused never arrive.
            WindowEvent::Focused(false) => {
                if let Some(g) = &mut self.game {
                    g.cam.release_keys();
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                if let Some(g) = &mut self.game {
                    g.pointer.ctrl = m.state().control_key();
                    g.pointer.shift = m.state().shift_key();
                    g.alt = m.state().alt_key();
                    g.pointer.alt = g.alt;
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                if down
                    && !event.repeat
                    && self.mods.alt_key()
                    && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter))
                {
                    self.toggle_fullscreen();
                    return;
                }
                // Typing into the start screens' edit fields.
                if down && let Some(f) = &mut self.front {
                    use winit::keyboard::{Key, NamedKey};
                    let text = match &event.logical_key {
                        Key::Named(NamedKey::Backspace) => Some("\u{8}".to_string()),
                        Key::Named(NamedKey::Enter) => Some("\r".to_string()),
                        _ => event.text.as_ref().map(|t| t.to_string()),
                    };
                    if let Some(t) = text {
                        let act = f.key_text(gpu, &t);
                        self.front_action(el, act);
                        return;
                    }
                }
                // Typing into a game page's edit field (the save name).
                if down && let Some(g) = &mut self.game {
                    use winit::keyboard::{Key, NamedKey};
                    let text = match &event.logical_key {
                        Key::Named(NamedKey::Backspace) => Some("\u{8}".to_string()),
                        Key::Named(NamedKey::Enter) => Some("\r".to_string()),
                        Key::Named(_) => None,
                        _ => event.text.as_ref().map(|t| t.to_string()),
                    };
                    if let Some(t) = text
                        && g.type_text(gpu, &t)
                    {
                        return;
                    }
                    // The character for the GUI's hotkeys (`IngameKeybindings`).
                    if !g.pointer.ctrl
                        && let Some(c) = event.text.as_ref().and_then(|t| t.chars().next())
                    {
                        g.char_key(gpu, c);
                    }
                }
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(g) = &mut self.game {
                        g.key(gpu, code, down, event.repeat);
                    } else if down && self.front.as_mut().is_some_and(|f| f.key_down(gpu)) {
                    } else if code == KeyCode::Space && down && self.front.as_mut().is_some_and(|f| f.space()) {
                        el.exit();
                    } else if code == KeyCode::Escape && down {
                        // The start screens take Esc as their states do (frontend `escape`); the
                        // add-on screens, which do not yet, quit.
                        if !self.front.as_mut().is_some_and(|f| f.escape(gpu)) {
                            el.exit();
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.front_video();
                if self.video_frame() {
                    return;
                }
                let Some(gpu) = &mut self.gpu else { return };
                sound::MenuMusic::follow(&mut self.menu_music, self.front.is_some(), &self.settings);
                // The credits state (0x511cf0) has its own playlist; leaving it the menu's again.
                let credits = self.front.as_ref().is_some_and(|f| f.credits_shown());
                if credits != self.credits_music {
                    self.credits_music = credits;
                    sound::MenuMusic::credits(&mut self.menu_music, credits);
                }
                if let Some(f) = &mut self.front {
                    let ids = f.take_sounds(gpu);
                    sound::MenuMusic::play(&mut self.menu_music, &ids);
                }
                if let Some(f) = &mut self.front {
                    let act = f.tick(gpu);
                    if act.is_some() {
                        self.front_action(el, act);
                        return;
                    }
                }
                let Some(gpu) = &mut self.gpu else { return };
                let started = Instant::now();
                if let Some(w) = self.window.clone() {
                    let size = gpu.size();
                    let game = self.game.as_ref();
                    let snap = game.map(|g| g.cur.clone());
                    // A node in the world: the scene's node position (height included); the
                    // overlay projects it with the camera the frame is drawn with.
                    let world = |x: i32, y: i32| game.map_or([0.0; 2], |g| g.scene.node_world(x.max(0) as usize, y.max(0) as usize));
                    let selection = game.map(|g| g.pointer.selection.ids.clone()).unwrap_or_default();
                    let feed = monitor::Feed { snapshot: snap.as_deref(), local: game.map_or(0, |g| g.sim.local), selection: &selection, sim: game.map(|g| &g.sim.monitor), world: game.map(|_| &world as monitor::NodeWorld) };
                    monitor::Monitor::of(gpu, Some(&w)).frame(Some(&w), None, size, w.scale_factor() as f32, &feed);
                }
                let shape = match &mut self.game {
                    Some(g) => {
                        let t = perf::span("frame");
                        g.redraw(gpu, self.cursor, self.window.as_deref());
                        drop(t);
                        g.cursor_shape
                    }
                    None => {
                        gpu.draw(View { x: 0.0, y: 0.0, zoom: 1.0 });
                        cursor::shape::ARROW
                    }
                };
                if self.window.is_some() {
                    monitor::Monitor::of(gpu, None).redraw_took(started.elapsed());
                }
                self.set_cursor(shape);
                self.perf_log();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        save_app_settings(&mut self.settings, self.gpu.as_ref());
    }
}

fn main() -> anyhow::Result<()> {
    console::attach();
    let args = parse_args()?;
    // Standard error and the day's log file (%APPDATA%\s4rs\logs); a panic is logged and, with a
    // window, shown in a message box naming the file.
    logfile::init(!args.headless);
    console::log_state();
    // The game data come from the player's install (detected, or picked at the first start).
    if gamedir::resolve(!args.headless).is_none() {
        std::process::exit(2);
    }
    let gui = !args.headless;
    run(args).inspect_err(|e| {
        tracing::error!("{e:#}");
        if gui {
            let log = logfile::file().map_or("(no log file)".into(), |p| p.display().to_string());
            gamedir::message_box("S4RS", &format!("S4RS stopped with an error:

{e:#}

The log file: {log}"));
        }
    })
}

fn run(args: Args) -> anyhow::Result<()> {
    // Off the main thread at the start, not at the first use: the save lists' rules fingerprint
    // (~200 ms), the GUIs' fonts (~80 ms, every screen change parsed them again) and the map
    // lists' headers.
    std::thread::spawn(|| {
        let _ = s4_viewer::gui::TextEngine::shared();
        save::current_rules();
        frontend::warm_map_lists();
    });
    if let Some(p) = args.list_buildings {
        for (id, kind, x, y) in sim::buildings(&setup(&args)?, p)? {
            println!("building {id}: type {kind} at {x},{y}");
        }
        return Ok(());
    }
    if args.determinism {
        return determinism(&args);
    }
    if args.convert_videos {
        return video::convert_all();
    }
    if args.bench > 0 {
        return bench(&args);
    }
    if args.audit > 0 {
        return audit(&args);
    }
    if args.soak > 0 {
        return soak(&args);
    }
    if args.headless && args.menu {
        return menu_headless(&args);
    }
    if args.headless {
        return headless(&args);
    }
    let mut settings = settings::Settings::load();
    if args.speed_given {
        settings.speed = args.speed;
    }
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    // The first launch converts the install's videos in the background, the start's first.
    let language = settings.language;
    let videos = video::Videos::start(settings.show_videos, |folder, stem| video::STARTUP.iter().filter_map(|&n| video::file(n, language)).any(|f| video::names(&f, folder, stem)));
    let mut app = App {
        args,
        settings,
        window: None,
        gpu: None,
        cursor: (0.0, 0.0),
        front: None,
        game: None,
        menu_music: None,
        credits_music: false,
        cursors: Vec::new(),
        cursor_shown: None,
        logos: s4_formats::logo::installed_current(&s4_formats::game_dir()),
        mods: Default::default(),
        perf: None,
        videos,
        playing: None,
        video_queue: Default::default(),
    };
    if app.args.perf {
        perf::enable(true);
        app.perf = Some((Instant::now(), 0));
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The QA passes' building orders: for each kind the first possible site within 25 nodes of
    /// `local`'s start, more than 6 from it and at least 7 from the others.
    fn qa_sites(w: &s4_sim::World, local: u8, kinds: &[u8]) -> Vec<(u8, u16, u16)> {
        let pl = w.players[local as usize].as_ref().map_or((0, 0), |p| (p.start_x as i32, p.start_y as i32));
        let mut used: Vec<(u16, u16)> = Vec::new();
        let mut orders = Vec::new();
        for &k in kinds {
            let sites = s4_sim::build::possible_sites(w, local, k, (pl.0 - 25, pl.1 - 25), (pl.0 + 25, pl.1 + 25));
            let far = |s: &(u16, u16, u8)| (s.0 as i32 - pl.0).abs() + (s.1 as i32 - pl.1).abs() > 6;
            let apart = |s: &(u16, u16, u8)| used.iter().all(|&(ux, uy)| (ux as i32 - s.0 as i32).abs() + (uy as i32 - s.1 as i32).abs() >= 7);
            if let Some(&(x, y, _)) = sites.iter().find(|s| far(s) && apart(s)) {
                used.push((x, y));
                orders.push((k, x, y));
            }
        }
        orders
    }

    /// The game start's and the first frames' time by part (perf spans): a free UnitedNations
    /// game with computers (`S4RS_STALL_MAP`), then its save loaded; each with its first 5
    /// frames. `cargo test --release -p s4-game load_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn load_probe() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let saves = std::env::temp_dir().join(format!("s4rs-load-saves-{}", std::process::id()));
        // SAFETY (set_var): the only test that reads it runs alone (ignored, run by name).
        unsafe { std::env::set_var("S4RS_SAVE_DIR", &saves) };
        let map = std::env::var("S4RS_STALL_MAP").unwrap_or_else(|_| "Multiplayer/UnitedNations".into());
        let mut gpu = Gpu::headless(1920, 1080).unwrap();
        gpu.set_offscreen(true);
        perf::enable(true);
        let report = |what: &str, t: Instant| {
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            eprintln!("LOAD {ms:>8.1} ms  {what}");
            for (k, s) in perf::take() {
                if s.total.as_secs_f64() > 0.002 {
                    eprintln!("LOAD     {:>8.1} ms  {k} ({}x, longest {:.1})", s.total.as_secs_f64() * 1000.0, s.count, s.max.as_secs_f64() * 1000.0);
                }
            }
        };
        let frames = |g: &mut Game, gpu: &mut Gpu, what: &str| {
            for k in 0..5 {
                let t = Instant::now();
                g.redraw(gpu, (960.0, 540.0), None);
                let w = perf::span("gpu: wait");
                gpu.wait().unwrap();
                drop(w);
                report(&format!("{what} frame {k}"), t);
            }
        };
        perf::take();
        let players = sim::PlayerSetup { races: [None; 8], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control: [3; 8], game_type: 0, local: 0, players: None, race_goods: [0; 7], decided: false, start_resources: 2 };
        let args = Args { players: Some(players), delay: 0, hash_interval: 64, ..Default::default() };
        let t = Instant::now();
        let mut g = Game::start(&map, &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        report(&format!("Game::start {map}"), t);
        frames(&mut g, &mut gpu, "start");
        g.sim.step(2000);
        g.redraw(&mut gpu, (960.0, 540.0), None);
        assert!(g.save_as(&mut gpu, "probe", 0));
        let save = std::fs::read_dir(&saves).unwrap().flatten().map(|e| e.path()).next().unwrap();
        drop(g);
        perf::take();
        let args = Args { load: Some(save.to_string_lossy().into_owned()), delay: 0, hash_interval: 64, ..Default::default() };
        let t = Instant::now();
        let mut g = Game::start(&map, &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        report("Game::start loaded", t);
        frames(&mut g, &mut gpu, "loaded");
        drop(g);
        let _ = std::fs::remove_dir_all(&saves);
    }

    /// Main-thread stalls a player meets in a late game: a free game on `S4RS_STALL_MAP`
    /// (default UnitedNations) with computers in every slot, run `S4RS_STALL_TICKS` (20 000)
    /// ticks, then each action's frame timed (the action and the redraw that shows it, GPU
    /// waited): the main tabs and their submenu pages, one building of each type selected, a
    /// save, the save page with 60 saves, the end statistics, loading a save, a free start
    /// through the start screens and its first frames. Prints a table (over 30 ms marked):
    /// `cargo test --release -p s4-game stall_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn stall_probe() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let saves = std::env::temp_dir().join(format!("s4rs-stall-saves-{}", std::process::id()));
        // SAFETY (set_var): the only test that reads it runs alone (ignored, run by name).
        unsafe { std::env::set_var("S4RS_SAVE_DIR", &saves) };
        let map = std::env::var("S4RS_STALL_MAP").unwrap_or_else(|_| "Multiplayer/UnitedNations".into());
        let ticks: u32 = std::env::var("S4RS_STALL_TICKS").ok().and_then(|t| t.parse().ok()).unwrap_or(20_000);
        let (w, h) = (1920u32, 1080u32);
        let cursor = (960.0, 540.0);
        let mut gpu = Gpu::headless(w, h).unwrap();
        gpu.set_offscreen(true);
        let mut rows: Vec<(String, f64)> = Vec::new();
        let row = |rows: &mut Vec<(String, f64)>, what: String, t: Instant| {
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            eprintln!("STALL {ms:>9.1} ms{}  {what}", if ms > 30.0 { " *" } else { "  " });
            rows.push((what, ms));
        };
        let players = sim::PlayerSetup { races: [None; 8], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control: [3; 8], game_type: 0, local: 0, players: None, race_goods: [0; 7], decided: false, start_resources: 2 };
        let args = Args { players: Some(players), delay: 0, hash_interval: 64, ..Default::default() };
        let t = Instant::now();
        let mut g = Game::start(&map, &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        g.autosave_interval = None;
        row(&mut rows, format!("Game::start {map}"), t);
        for k in 0..3 {
            let t = Instant::now();
            g.redraw(&mut gpu, cursor, None);
            gpu.wait().unwrap();
            row(&mut rows, format!("first frame {k} (minimap's first draw)"), t);
        }
        let t = Instant::now();
        while g.cur.tick < ticks {
            g.sim.step(500.min(ticks - g.cur.tick));
            g.redraw(&mut gpu, cursor, None);
        }
        gpu.wait().unwrap();
        let pop = &g.cur.population;
        eprintln!("STALL ran to {} in {:.0} s: {} settlers, {} buildings, {} vehicles", g.cur.tick, t.elapsed().as_secs_f64(), pop.settlers.len(), pop.buildings.len(), pop.vehicles.len());
        // Home: the local player's start.
        let l = g.sim.local as usize;
        let st = g.cur.population.starts.get(l).copied().unwrap_or((0, 0));
        g.cam.jump(view_on(&g.scene, Some((st.0 as usize, st.1 as usize)), gpu.size(), g.sim.local), &screen(&gpu, g.scene.source.w));
        let frame = |g: &mut Game, gpu: &mut Gpu| {
            g.redraw(gpu, cursor, None);
            gpu.wait().unwrap();
        };
        let mut base = Vec::new();
        for _ in 0..10 {
            let t = Instant::now();
            frame(&mut g, &mut gpu);
            base.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        base.sort_by(f64::total_cmp);
        eprintln!("STALL baseline frame: median {:.1} ms, max {:.1} ms", base[5], base[9]);
        let open = |gpu: &Gpu| gpu.ui.as_ref().map(|u| u.state.open.clone()).unwrap_or_default();
        // The GUI panel's parts: the CPU composition and the whole recompose (+ upload).
        g.click_control(&mut gpu, 0, 607);
        frame(&mut g, &mut gpu);
        let t = Instant::now();
        let _ = gpu.ui.as_ref().unwrap().compose();
        row(&mut rows, format!("GUI compose alone (open {:?})", open(&gpu)), t);
        let t = Instant::now();
        client::recompose(&mut gpu);
        gpu.wait().unwrap();
        row(&mut rows, "GUI recompose + upload".to_string(), t);
        // A tab click's parts.
        let p = client::control_point(&gpu, 0, 608).unwrap();
        let t = Instant::now();
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Pressed);
        row(&mut rows, "tab 608: press".to_string(), t);
        let t = Instant::now();
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Released);
        row(&mut rows, "tab 608: release".to_string(), t);
        let t = Instant::now();
        let (ev, _) = client::gui_mouse(&mut gpu, g.cursor_last, GuiMouse::Move);
        g.msg_events(&mut gpu, &ev);
        row(&mut rows, "tab 608: hover back".to_string(), t);
        let t = Instant::now();
        frame(&mut g, &mut gpu);
        row(&mut rows, "tab 608: frame".to_string(), t);
        // The main tabs and every submenu page.
        let subs: [(usize, std::ops::RangeInclusive<u16>); 5] = [(3, 536..=540), (4, 745..=748), (5, 1067..=1069), (7, 1021..=1024), (8, 928..=931)];
        for tab in 606..=611u16 {
            let t = Instant::now();
            g.click_control(&mut gpu, 0, tab);
            frame(&mut g, &mut gpu);
            row(&mut rows, format!("main tab {tab} → open {:?}", open(&gpu)), t);
            let t = Instant::now();
            frame(&mut g, &mut gpu);
            row(&mut rows, "  next frame".to_string(), t);
            for (d, links) in subs.iter().cloned() {
                if !open(&gpu).contains(&d) {
                    continue;
                }
                for link in links {
                    let before = open(&gpu);
                    let t = Instant::now();
                    g.click_control(&mut gpu, d, link);
                    frame(&mut g, &mut gpu);
                    let now: Vec<usize> = open(&gpu).into_iter().filter(|x| !before.contains(x)).collect();
                    row(&mut rows, format!("  submenu {d}/{link} → new {now:?}"), t);
                    let t = Instant::now();
                    frame(&mut g, &mut gpu);
                    row(&mut rows, "    next frame".to_string(), t);
                }
            }
        }
        // One building of each type of the local player.
        let mut kinds = std::collections::BTreeMap::new();
        for b in g.cur.population.buildings.iter().filter(|b| b.owner == g.sim.local && b.site.is_none()) {
            kinds.entry(b.kind).or_insert(b.id);
        }
        for (kind, id) in kinds {
            let t = Instant::now();
            let snap = g.sim.snapshot();
            client::select_ids(&mut gpu, &snap.population, &mut g.pointer.selection, vec![id]);
            frame(&mut g, &mut gpu);
            row(&mut rows, format!("building {kind} selected → open {:?}", open(&gpu)), t);
            let t = Instant::now();
            frame(&mut g, &mut gpu);
            row(&mut rows, "  next frame".to_string(), t);
        }
        // Every own soldier: the group dialog.
        let soldiers: Vec<u16> = g.cur.population.settlers.iter().filter(|u| u.owner == g.sim.local && u.flags & 0xf != 0).map(|u| u.id).take(100).collect();
        let t = Instant::now();
        let snap = g.sim.snapshot();
        client::select_ids(&mut gpu, &snap.population, &mut g.pointer.selection, soldiers.clone());
        frame(&mut g, &mut gpu);
        row(&mut rows, format!("{} soldiers selected → open {:?}", soldiers.len(), open(&gpu)), t);
        let snap = g.sim.snapshot();
        client::clear_selection(&mut gpu, &snap.population, &mut g.pointer.selection);
        // Saves: one timed, 59 more for the page.
        let t = Instant::now();
        assert!(g.save_as(&mut gpu, "stall 0", 0));
        row(&mut rows, "save (late game)".to_string(), t);
        for i in 1..60 {
            g.save_as(&mut gpu, &format!("stall {i}"), 0);
        }
        let t = Instant::now();
        let n = menu::list_saves().len();
        row(&mut rows, format!("list_saves ({n} saves)"), t);
        let t = Instant::now();
        let _ = save::current_rules();
        row(&mut rows, "save::current_rules (first call)".to_string(), t);
        let t0 = Instant::now();
        let t = Instant::now();
        g.click_control(&mut gpu, 0, 611);
        row(&mut rows, "  click 611".to_string(), t);
        let t = Instant::now();
        g.click_control(&mut gpu, 8, 928);
        row(&mut rows, "  click 928".to_string(), t);
        let t = Instant::now();
        frame(&mut g, &mut gpu);
        row(&mut rows, "  frame".to_string(), t);
        row(&mut rows, format!("save page with 60 saves → open {:?}", open(&gpu)), t0);
        let t = Instant::now();
        frame(&mut g, &mut gpu);
        row(&mut rows, "  next frame".to_string(), t);
        // The end statistics.
        let t = Instant::now();
        let rec = g.end_record();
        row(&mut rows, "end record (sim's statistics)".to_string(), t);
        let save = std::fs::read_dir(&saves).unwrap().flatten().map(|e| e.path()).next().unwrap();
        drop(g);
        let t = Instant::now();
        let mut f = frontend::Frontend::end_statistic(&mut gpu, 0, rec, None).unwrap();
        f.tick(&mut gpu);
        frontend::render(&mut gpu);
        gpu.wait().unwrap();
        row(&mut rows, "end statistics screen".to_string(), t);
        drop(f);
        // Loading the save.
        let t = Instant::now();
        let largs = Args { load: Some(save.to_string_lossy().into_owned()), delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start(&map, &largs, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        row(&mut rows, "load save (Game::start)".to_string(), t);
        let t = Instant::now();
        g.redraw(&mut gpu, cursor, None);
        gpu.wait().unwrap();
        row(&mut rows, "  its first frame".to_string(), t);
        drop(g);
        // A free game through the start screens: main menu → single → free → the map → start.
        // The app warms the map lists at its start (off the main thread): so does this run.
        frontend::warm_map_lists();
        frontend::Frontend::fit(&mut gpu);
        let t = Instant::now();
        let mut f = frontend::Frontend::new(&mut gpu, 0).unwrap();
        row(&mut rows, "start screens (Frontend::new)".to_string(), t);
        for k in 0..2 {
            let t = Instant::now();
            let _ = gpu.ui.as_ref().unwrap().compose();
            row(&mut rows, format!("start screen compose alone ({k})"), t);
        }
        let click = |f: &mut frontend::Frontend, gpu: &mut Gpu, dialog: usize, link: u16| -> Option<frontend::Action> {
            let p = client::control_point(gpu, dialog, link)?;
            f.mouse(gpu, p, GuiMouse::Move);
            f.mouse(gpu, p, GuiMouse::LeftDown);
            f.mouse(gpu, p, GuiMouse::LeftUp)
        };
        for (d, l, what) in [(0, 2536, "single player"), (7, 2414, "free games"), (7, 2404, "map type"), (18, 2450, "free map list")] {
            let t = Instant::now();
            click(&mut f, &mut gpu, d, l);
            f.tick(&mut gpu);
        frontend::render(&mut gpu);
            gpu.wait().unwrap();
            row(&mut rows, format!("start screen: {what}"), t);
        }
        let t = Instant::now();
        click(&mut f, &mut gpu, 18, 2417);
        f.tick(&mut gpu);
        frontend::render(&mut gpu);
        gpu.wait().unwrap();
        row(&mut rows, "start screen: a map picked (preview)".to_string(), t);
        let t = Instant::now();
        click(&mut f, &mut gpu, 18, 2433);
        f.tick(&mut gpu);
        frontend::render(&mut gpu);
        gpu.wait().unwrap();
        row(&mut rows, "start screen: game settings".to_string(), t);
        if let Some(frontend::Action::Start(path, None, players)) = click(&mut f, &mut gpu, 19, 2142) {
            drop(f);
            let t = Instant::now();
            let args = Args { players, delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start(&path.to_string_lossy(), &args, 0, &mut gpu, None).unwrap();
            g.progress_file = None;
            row(&mut rows, format!("free start {} (Game::start)", path.display()), t);
            let t = Instant::now();
            g.redraw(&mut gpu, cursor, None);
            gpu.wait().unwrap();
            row(&mut rows, "  its first frame".to_string(), t);
        }
        let _ = std::fs::remove_dir_all(&saves);
        eprintln!("STALL over 30 ms:");
        for (what, ms) in rows.iter().filter(|r| r.1 > 30.0) {
            eprintln!("STALL  {ms:>9.1} ms  {what}");
        }
    }

    /// Pre-playtest QA: the start screens and the game driven as a player would (clicks on the
    /// controls, the camera keys, the zoom), 1920×1080 captures at every step into `out/qa/`
    /// for a person to look at. Saves go to a test folder, wins are not recorded. Slow (a few
    /// minutes): `cargo test --release -p s4-game qa_pass -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn qa_pass() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        use s4_sim::{Command, PlayerCommand};
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/qa");
        let _ = std::fs::create_dir_all(&out);
        let saves = std::env::temp_dir().join(format!("s4rs-qa-saves-{}", std::process::id()));
        // SAFETY (set_var): the only test that reads it runs alone (ignored, run by name).
        unsafe { std::env::set_var("S4RS_SAVE_DIR", &saves) };
        let (w, h) = (1920u32, 1080u32);
        let mut gpu = Gpu::headless(w, h).unwrap();
        gpu.set_offscreen(true);
        let shot = |gpu: &mut Gpu, view: View, name: &str| match gpu.capture(view) {
            Ok(rgba) => {
                let _ = client::write_png(&out.join(format!("{name}.png")), w, h, &rgba);
                eprintln!("QA capture {name}");
            }
            Err(e) => eprintln!("QA capture {name}: {e}"),
        };
        let front_view = View { x: 0.0, y: 0.0, zoom: 1.0 };
        let click = |f: &mut frontend::Frontend, gpu: &mut Gpu, dialog: usize, link: u16| -> Option<frontend::Action> {
            let Some(p) = client::control_point(gpu, dialog, link) else {
                eprintln!("QA: no control {link} in dialog {dialog}");
                return None;
            };
            f.mouse(gpu, p, GuiMouse::Move);
            f.mouse(gpu, p, GuiMouse::LeftDown);
            f.mouse(gpu, p, GuiMouse::LeftUp)
        };
        let open = |gpu: &Gpu| gpu.ui.as_ref().map(|u| u.state.open.clone()).unwrap_or_default();
        // A game as App::start makes it (paused: the steps drive it).
        let start = |gpu: &mut Gpu, map: &str, players: Option<sim::PlayerSetup>, campaign: Option<(u8, u32)>, load: Option<String>| -> Game {
            let args = Args { players, load, delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start(map, &args, 0, gpu, None).unwrap();
            g.progress_file = None;
            g.campaign = campaign;
            g.set_briefing(gpu);
            g
        };
        let play = |g: &mut Game, gpu: &mut Gpu, ticks: u32| {
            let to = g.cur.tick + ticks;
            while g.cur.tick < to {
                g.sim.step(20.min(to - g.cur.tick));
                g.redraw(gpu, (960.0, 540.0), None);
                gpu.wait().unwrap();
            }
        };
        let census = |g: &Game, what: &str| {
            let pop = &g.cur.population;
            let mut per = std::collections::BTreeMap::<u8, (usize, usize, usize)>::new();
            for b in &pop.buildings {
                let e = per.entry(b.owner).or_default();
                if b.site.is_some() {
                    e.1 += 1
                } else {
                    e.0 += 1
                }
            }
            for u in &pop.settlers {
                per.entry(u.owner).or_default().2 += 1;
            }
            eprintln!("QA: {what} at {} (local {}, starts {:?}): buildings / sites / settlers per player {per:?}", pop.tick, g.sim.local, &pop.starts[..5]);
        };
        let at_home = |g: &mut Game, gpu: &mut Gpu| {
            let l = g.sim.local as usize;
            let st = g.cur.population.starts.get(l).copied().unwrap_or((0, 0));
            g.cam.jump(view_on(&g.scene, Some((st.0 as usize, st.1 as usize)), gpu.size(), g.sim.local), &screen(gpu, g.scene.source.w));
            g.redraw(gpu, (960.0, 540.0), None);
        };
        // Building orders near the local player's start: the first possible sites, apart.
        let build_near_home = |g: &Game, kinds: &[u8]| {
            let (tx, rx) = std::sync::mpsc::channel();
            let kinds = kinds.to_vec();
            let local = g.sim.local;
            g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
                let _ = tx.send(qa_sites(w, local, &kinds));
            }));
            g.sim.step(1);
            let orders = rx.recv().unwrap();
            let t = g.sim.snapshot().tick + 1;
            for &(k, x, y) in &orders {
                g.sim.command(Stamped { tick: t, command: PlayerCommand::new(local, Command::Build { building: k as u32, x, y }) });
            }
            eprintln!("QA: build orders {orders:?}");
            orders.len()
        };

        // 1. The start screens → single player → the Roman campaign → mission 1.
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::new(&mut gpu, 0).unwrap();
        shot(&mut gpu, front_view, "01_main_menu");
        click(&mut f, &mut gpu, 0, 2536);
        shot(&mut gpu, front_view, "02_single_player");
        click(&mut f, &mut gpu, 7, 2414);
        click(&mut f, &mut gpu, 7, 2401);
        shot(&mut gpu, front_view, "03_campaigns");
        click(&mut f, &mut gpu, 8, 2044);
        shot(&mut gpu, front_view, "04_briefing_roman01");
        let Some(frontend::Action::Start(path, campaign, players)) = click(&mut f, &mut gpu, 10, 2056) else { panic!("QA: no roman01 start, open {:?}", open(&gpu)) };
        let mut g = start(&mut gpu, &path.to_string_lossy(), players, campaign, None);
        eprintln!("QA: roman01 races {:?}", g.cur.population.races);
        at_home(&mut g, &mut gpu);
        shot(&mut gpu, g.cam.view(), "05_roman01_start");
        census(&g, "roman01 start");
        let n = build_near_home(&g, &[1, 2, 4, 3, 41]);
        assert!(n > 0, "QA: no build site near the start");
        play(&mut g, &mut gpu, 1000);
        at_home(&mut g, &mut gpu);
        shot(&mut gpu, g.cam.view(), "06_roman01_t1000");
        play(&mut g, &mut gpu, 4000);
        at_home(&mut g, &mut gpu);
        shot(&mut gpu, g.cam.view(), "07_roman01_t5000");
        census(&g, "roman01");
        // The camera keys and the zoom on the game view.
        g.key(&mut gpu, KeyCode::ArrowRight, true, false);
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(16));
            g.redraw(&mut gpu, (960.0, 540.0), None);
        }
        g.key(&mut gpu, KeyCode::ArrowRight, false, false);
        shot(&mut gpu, g.cam.view(), "08_roman01_scrolled_right");
        let size = g.scene.source.w;
        g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), 3, size), &screen(&gpu, g.scene.source.w));
        g.redraw(&mut gpu, (960.0, 540.0), None);
        shot(&mut gpu, g.cam.view(), "09_roman01_zoomed_in");
        g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), -6, size), &screen(&gpu, g.scene.source.w));
        g.redraw(&mut gpu, (960.0, 540.0), None);
        shot(&mut gpu, g.cam.view(), "10_roman01_zoomed_out");
        // Save → load → continue.
        assert!(g.save_as(&mut gpu, "qa roman01", 0), "QA: save failed");
        let save = std::fs::read_dir(&saves).unwrap().flatten().map(|e| e.path()).next().expect("QA: a save file");
        let tick = g.cur.tick;
        drop(g);
        let mut g = start(&mut gpu, "Campaign/roman01", None, None, Some(save.to_string_lossy().into_owned()));
        g.redraw(&mut gpu, (960.0, 540.0), None);
        eprintln!("QA: saved at {tick}, loaded at {}", g.cur.tick);
        shot(&mut gpu, g.cam.view(), "11_roman01_loaded");
        play(&mut g, &mut gpu, 500);
        shot(&mut gpu, g.cam.view(), "12_roman01_loaded_t500");
        drop(g);

        // 2. A free map against three computers at normal.
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::new(&mut gpu, 0).unwrap();
        click(&mut f, &mut gpu, 0, 2536);
        click(&mut f, &mut gpu, 7, 2414);
        click(&mut f, &mut gpu, 7, 2404);
        click(&mut f, &mut gpu, 18, 2450);
        shot(&mut gpu, front_view, "13_free_map_list");
        // Choices: a 4-player map with computer slots (scrolled to, as a player would).
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(18, l)).cloned().unwrap_or_default();
        let mut picked = false;
        'maps: for _ in 0..60 {
            for row in 2417..=2423u16 {
                if text(&gpu, row) == "Choices" {
                    click(&mut f, &mut gpu, 18, row);
                    picked = true;
                    break 'maps;
                }
            }
            f.wheel(&mut gpu, false);
        }
        assert!(picked, "QA: Choices not in the list");
        for _ in 0..8 {
            if text(&gpu, 2443) == "4" {
                break;
            }
            click(&mut f, &mut gpu, 18, 2442);
        }
        shot(&mut gpu, front_view, "14_free_map_settings");
        click(&mut f, &mut gpu, 18, 2433);
        shot(&mut gpu, front_view, "15_free_game_settings");
        let Some(frontend::Action::Start(path, None, players)) = click(&mut f, &mut gpu, 19, 2142) else { panic!("QA: no free start, open {:?}", open(&gpu)) };
        eprintln!("QA: free map {} players {:?}", path.display(), players.map(|p| (p.control, p.races)));
        let mut g = start(&mut gpu, &path.to_string_lossy(), players, None, None);
        at_home(&mut g, &mut gpu);
        shot(&mut gpu, g.cam.view(), "16_free_start");
        census(&g, "free start");
        build_near_home(&g, &[1, 2, 4, 3, 41]);
        for i in 1..=3u32 {
            play(&mut g, &mut gpu, 5000);
            at_home(&mut g, &mut gpu);
            shot(&mut gpu, g.cam.view(), &format!("17_free_t{}", i * 5000));
        }
        census(&g, "free map");
        // The end statistics → back.
        let rec = g.end_record();
        drop(g);
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::end_statistic(&mut gpu, 0, rec, None).unwrap();
        shot(&mut gpu, front_view, "18_end_statistics");
        f.escape(&mut gpu);
        shot(&mut gpu, front_view, "19_after_statistics");

        // 3. A random map from the dialog.
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::new(&mut gpu, 0).unwrap();
        click(&mut f, &mut gpu, 0, 2536);
        click(&mut f, &mut gpu, 7, 2414);
        click(&mut f, &mut gpu, 7, 2404);
        click(&mut f, &mut gpu, 18, 2507);
        shot(&mut gpu, front_view, "20_random_dialog");
        click(&mut f, &mut gpu, frontend::random::LOCAL, frontend::random::START);
        shot(&mut gpu, front_view, "21_random_game_settings");
        let Some(frontend::Action::Start(path, None, players)) = click(&mut f, &mut gpu, 19, 2142) else { panic!("QA: no random start, open {:?}", open(&gpu)) };
        eprintln!("QA: random map {}", path.display());
        let mut g = start(&mut gpu, &path.to_string_lossy(), players, None, None);
        at_home(&mut g, &mut gpu);
        shot(&mut gpu, g.cam.view(), "22_random_start");
        census(&g, "random start");
        build_near_home(&g, &[1, 2, 4]);
        play(&mut g, &mut gpu, 5000);
        at_home(&mut g, &mut gpu);
        shot(&mut gpu, g.cam.view(), "23_random_t5000");
        census(&g, "random");
        drop(g);

        // 4. After a won tutorial: the tutorial menu → Esc → the main menu.
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::after_tutorial(&mut gpu, 0).unwrap();
        shot(&mut gpu, front_view, "24_after_tutorial");
        f.escape(&mut gpu);
        shot(&mut gpu, front_view, "25_tutorial_menu_back");
        let _ = std::fs::remove_dir_all(&saves);
    }

    /// The QA pass's second part, the paths `qa_pass` does not drive: the add-on campaigns'
    /// start screens (Add-On, Mission CD, Mission CD 2 and 3: menu → mission screen → briefing →
    /// the first mission, 2000 ticks); the Dark Tribe campaign → dark01 (2000 ticks) with the
    /// in-game settings pages and the save / load page; saves of an older s4rs (the user's
    /// `%APPDATA%\s4rs\saves` ones below this build's version, copied; a made-up one if there
    /// are none) refused on the in-game page, the start screens' load list and by `Game::start`;
    /// a LAN game: two start screens on localhost, host + join by address, both games 3000
    /// ticks, then the same state hash at the same tick and no desync in the log. 1920×1080
    /// captures into `out/qa2/`, the log in `out/qa2/log.txt`. Slow (a few minutes):
    /// `cargo test --release -p s4-game qa_pass2 -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn qa_pass2() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/qa2");
        let _ = std::fs::create_dir_all(&out);
        let log = std::fs::File::create(out.join("log.txt")).unwrap();
        let _ = tracing_subscriber::fmt().with_ansi(false).with_writer(std::sync::Mutex::new(log)).try_init();
        // The user's saves of older versions, copied into a test folder.
        let user_saves = progress::path().and_then(|p| p.parent().map(|d| d.join("saves")));
        let saves = std::env::temp_dir().join(format!("s4rs-qa2-saves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&saves);
        std::fs::create_dir_all(&saves).unwrap();
        // SAFETY (set_var): the only test that reads it runs alone (ignored, run by name).
        unsafe { std::env::set_var("S4RS_SAVE_DIR", &saves) };
        let mut old: Vec<(std::path::PathBuf, u32)> = Vec::new();
        for p in user_saves.iter().filter_map(|d| std::fs::read_dir(d).ok()).flatten().flatten().map(|e| e.path()) {
            if let Ok(h) = save::read_header(&p)
                && h.version < save::VERSION
            {
                let dst = saves.join(p.file_name().unwrap());
                std::fs::copy(&p, &dst).unwrap();
                eprintln!("QA: older save {} (version {}, map {})", dst.display(), h.version, h.map);
                old.push((dst, h.version));
            }
        }
        let (w, h) = (1920u32, 1080u32);
        let mut gpu = Gpu::headless(w, h).unwrap();
        gpu.set_offscreen(true);
        let shot = |gpu: &mut Gpu, view: View, name: &str| match gpu.capture(view) {
            Ok(rgba) => {
                let _ = client::write_png(&out.join(format!("{name}.png")), w, h, &rgba);
                eprintln!("QA capture {name}");
            }
            Err(e) => eprintln!("QA capture {name}: {e}"),
        };
        let front_view = View { x: 0.0, y: 0.0, zoom: 1.0 };
        let open = |gpu: &Gpu| gpu.ui.as_ref().map(|u| u.state.open.clone()).unwrap_or_default();
        let click = |f: &mut frontend::Frontend, gpu: &mut Gpu, dialog: usize, link: u16| -> Option<frontend::Action> {
            let Some(p) = client::control_point(gpu, dialog, link) else {
                eprintln!("QA: no control {link} in dialog {dialog} (open {:?})", open(gpu));
                return None;
            };
            f.mouse(gpu, p, GuiMouse::Move);
            f.mouse(gpu, p, GuiMouse::LeftDown);
            f.mouse(gpu, p, GuiMouse::LeftUp)
        };
        // A click on a control of the game's GUI, as the window gives it.
        let ui_click = |g: &mut Game, gpu: &mut Gpu, dialog: usize, link: u16| {
            let Some(p) = client::control_point(gpu, dialog, link) else {
                eprintln!("QA: no control {link} in dialog {dialog} (open {:?})", open(gpu));
                return;
            };
            g.cursor_moved(gpu, p);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Pressed);
            g.redraw(gpu, p, None);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Released);
            g.redraw(gpu, p, None);
        };
        let start = |gpu: &mut Gpu, map: &str, players: Option<sim::PlayerSetup>, campaign: Option<(u8, u32)>, load: Option<String>| -> anyhow::Result<Game> {
            let args = Args { players, load, delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start(map, &args, 0, gpu, None)?;
            g.progress_file = None;
            // A loaded save keeps its own.
            if campaign.is_some() {
                g.campaign = campaign;
            }
            g.set_briefing(gpu);
            Ok(g)
        };
        let play = |g: &mut Game, gpu: &mut Gpu, ticks: u32| {
            let to = g.cur.tick + ticks;
            while g.cur.tick < to {
                g.sim.step(20.min(to - g.cur.tick));
                g.redraw(gpu, (960.0, 540.0), None);
                gpu.wait().unwrap();
            }
        };
        let census = |g: &Game, what: &str| {
            let pop = &g.cur.population;
            let mut per = std::collections::BTreeMap::<u8, (usize, usize, usize)>::new();
            for b in &pop.buildings {
                let e = per.entry(b.owner).or_default();
                if b.site.is_some() {
                    e.1 += 1
                } else {
                    e.0 += 1
                }
            }
            for u in &pop.settlers {
                per.entry(u.owner).or_default().2 += 1;
            }
            eprintln!("QA: {what} at {} (local {}, races {:?}): buildings / sites / settlers per player {per:?}", pop.tick, g.sim.local, &pop.races[..5]);
        };
        let at_home = |g: &mut Game, gpu: &mut Gpu| {
            let l = g.sim.local as usize;
            let st = g.cur.population.starts.get(l).copied().unwrap_or((0, 0));
            g.cam.jump(view_on(&g.scene, Some((st.0 as usize, st.1 as usize)), gpu.size(), g.sim.local), &screen(gpu, g.scene.source.w));
            g.redraw(gpu, (960.0, 540.0), None);
        };
        let mission = |gpu: &mut Gpu, act: Option<frontend::Action>, name: &str| -> Game {
            let Some(frontend::Action::Start(path, campaign, players)) = act else { panic!("QA: no {name} start, open {:?}", open(gpu)) };
            eprintln!("QA: {name}: {} campaign {campaign:?} control {:?}", path.display(), players.map(|p| p.control));
            let mut g = start(gpu, &path.to_string_lossy(), players, campaign, None).unwrap();
            at_home(&mut g, gpu);
            shot(gpu, g.cam.view(), &format!("{name}_start"));
            census(&g, &format!("{name} start"));
            play(&mut g, gpu, 2000);
            at_home(&mut g, gpu);
            shot(gpu, g.cam.view(), &format!("{name}_t2000"));
            census(&g, name);
            g
        };

        // 1. The add-on campaigns: the main menu's button, then (dialog, link, capture after)
        // through the menu and the mission screen to the briefing, and its start.
        type Steps = &'static [(usize, u16, &'static str)];
        let packs: [(&str, u16, Steps, (usize, u16)); 4] = [
            ("ao", 2548, &[(0, 65, "menu"), (0, 60, "roman"), (1, 47, "briefing")], (7, 2)),
            ("md", 2531, &[(0, 1924, "menu"), (0, 1918, "roman"), (1, 1903, "briefing")], (5, 1868)),
            ("mcd2", 2532, &[(0, 1825, "menu"), (0, 1832, "briefing")], (1, 1804)),
            ("xmd3", 2534, &[(0, 2937, "menu"), (0, 2939, "briefing")], (1, 2916)),
        ];
        for (k, &(pack, button, steps, go)) in packs.iter().enumerate() {
            frontend::Frontend::fit(&mut gpu);
            let mut f = frontend::Frontend::new(&mut gpu, 0).unwrap();
            click(&mut f, &mut gpu, 0, button);
            for (i, &(d, l, what)) in steps.iter().enumerate() {
                click(&mut f, &mut gpu, d, l);
                shot(&mut gpu, front_view, &format!("{}{}_{pack}_{what}", k + 1, (b'a' + i as u8) as char));
            }
            let act = click(&mut f, &mut gpu, go.0, go.1);
            drop(mission(&mut gpu, act, &format!("{}z_{pack}", k + 1)));
        }

        // 2. The Dark Tribe campaign → dark01.
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::new(&mut gpu, 0).unwrap();
        click(&mut f, &mut gpu, 0, 2536);
        click(&mut f, &mut gpu, 7, 2414);
        click(&mut f, &mut gpu, 7, 2400);
        shot(&mut gpu, front_view, "5a_dark_campaign");
        click(&mut f, &mut gpu, 9, 2081);
        shot(&mut gpu, front_view, "5b_dark_briefing");
        let act = click(&mut f, &mut gpu, 10, 2056);
        let mut g = mission(&mut gpu, act, "5z_dark01");

        // 3. The in-game pages: Esc (the extras menu's quit page, answered no), F6 (its message
        // recipients page), the settings tab and its four pages.
        let key = |g: &mut Game, gpu: &mut Gpu, k: KeyCode| {
            g.key(gpu, k, true, false);
            g.key(gpu, k, false, false);
            g.redraw(gpu, (960.0, 540.0), None);
        };
        key(&mut g, &mut gpu, KeyCode::Escape);
        eprintln!("QA: Esc opens {:?}", open(&gpu));
        shot(&mut gpu, g.cam.view(), "6a_esc");
        ui_click(&mut g, &mut gpu, menu::QUIT_PAGE, 1072);
        ui_click(&mut g, &mut gpu, 0, 606);
        key(&mut g, &mut gpu, KeyCode::F6);
        eprintln!("QA: F6 opens {:?}", open(&gpu));
        shot(&mut gpu, g.cam.view(), "6b_f6");
        ui_click(&mut g, &mut gpu, 0, 610);
        for tab in 1021..=1024u16 {
            ui_click(&mut g, &mut gpu, 7, tab);
            eprintln!("QA: settings tab {tab}: open {:?}", open(&gpu));
            shot(&mut gpu, g.cam.view(), &format!("6c_settings_{tab}"));
        }
        // The save page with a save of this build next to the older ones.
        assert!(g.save_as(&mut gpu, "qa current", 0), "QA: save failed");
        let current = std::fs::read_dir(&saves).unwrap().flatten().map(|e| e.path()).find(|p| save::read_header(p).is_ok_and(|h| h.name == "qa current")).expect("QA: the current save");
        if old.is_empty() {
            // None of the user's: this build's save with the version below (the header's version
            // follows the magic string).
            let mut bytes = std::fs::read(&current).unwrap();
            let at = 8 + save::MAGIC.len();
            bytes[at..at + 4].copy_from_slice(&(save::VERSION - 1).to_le_bytes());
            let p = saves.join("qa older.s4rs");
            std::fs::write(&p, bytes).unwrap();
            eprintln!("QA: no older save of the user's; made {}", p.display());
            old.push((p, save::VERSION - 1));
        }
        ui_click(&mut g, &mut gpu, 0, 611);
        ui_click(&mut g, &mut gpu, 8, 928);
        eprintln!("QA: save page: open {:?}, rows {:?}", open(&gpu), g.menu.saves.iter().map(|s| (s.1.name.clone(), s.1.version)).collect::<Vec<_>>());
        shot(&mut gpu, g.cam.view(), "6f_save_page");
        let page_text = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.texts.get(&(menu::QUIT_PAGE, 1070)).cloned().unwrap_or_default();
        // An older save: a second click on its row refuses with a message.
        let row = |g: &Game, p: &std::path::Path| 915 + g.menu.saves.iter().position(|s| s.0 == p).expect("QA: the save in the page's list") as u16;
        let r = row(&g, &old[0].0);
        let name_field = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.texts.get(&(menu::SAVE_PAGE, 914)).cloned().unwrap_or_default();
        ui_click(&mut g, &mut gpu, menu::SAVE_PAGE, r);
        eprintln!("QA: row {r} clicked once: name field {:?}, open {:?}", name_field(&gpu), open(&gpu));
        ui_click(&mut g, &mut gpu, menu::SAVE_PAGE, r);
        eprintln!("QA: older save on the page: {:?}, load request {:?}", page_text(&gpu), g.load_request);
        assert!(g.load_request.is_none() && page_text(&gpu).contains("cannot be loaded"), "QA: the page loads an older save");
        shot(&mut gpu, g.cam.view(), "6g_save_page_older_refused");
        ui_click(&mut g, &mut gpu, menu::QUIT_PAGE, 1072);
        // The briefing and quit tabs (no on page 34 sends 0x25b: the build tab again).
        ui_click(&mut g, &mut gpu, 0, 611);
        ui_click(&mut g, &mut gpu, 8, 929);
        shot(&mut gpu, g.cam.view(), "6h_briefing_tab");
        ui_click(&mut g, &mut gpu, 8, 930);
        shot(&mut gpu, g.cam.view(), "6i_quit_tab");
        ui_click(&mut g, &mut gpu, menu::QUIT_PAGE, 1072);
        // This build's save: the page asks, yes → the load as the window does it.
        ui_click(&mut g, &mut gpu, 0, 611);
        ui_click(&mut g, &mut gpu, 8, 928);
        let r = row(&g, &current);
        ui_click(&mut g, &mut gpu, menu::SAVE_PAGE, r);
        ui_click(&mut g, &mut gpu, menu::SAVE_PAGE, r);
        shot(&mut gpu, g.cam.view(), "6j_save_page_load_asked");
        ui_click(&mut g, &mut gpu, menu::QUIT_PAGE, 1071);
        let req = g.load_request.take().expect("QA: no load request for this build's save");
        let tick = g.cur.tick;
        drop(g);
        let map = save::read_header(&req).unwrap().map;
        let mut g = start(&mut gpu, &map, None, None, Some(req.to_string_lossy().into_owned())).unwrap();
        g.redraw(&mut gpu, (960.0, 540.0), None);
        eprintln!("QA: dark01 saved at {tick}, loaded at {} (campaign {:?})", g.cur.tick, g.campaign);
        shot(&mut gpu, g.cam.view(), "6k_dark01_loaded");
        drop(g);

        // 4. Older saves elsewhere: `Game::start` (the window's load path, the command line)
        // refuses each, the start screens' load list shows why and does not load it.
        for (p, v) in &old {
            let map = save::read_header(p).unwrap().map;
            match start(&mut gpu, &map, None, None, Some(p.to_string_lossy().into_owned())) {
                Ok(_) => panic!("QA: version {v} save {} loaded", p.display()),
                Err(e) => eprintln!("QA: Game::start refuses version {v}: {e}"),
            }
        }
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::new(&mut gpu, 0).unwrap();
        click(&mut f, &mut gpu, 0, 2549);
        shot(&mut gpu, front_view, "7a_load_type");
        let campaign = old.iter().any(|(p, _)| save::read_header(p).is_ok_and(|h| frontend::load::kind_of(&h) == 3));
        click(&mut f, &mut gpu, frontend::load::DIALOG, if campaign { frontend::load::CAMPAIGN } else { frontend::load::LOCAL });
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(6, l)).cloned().unwrap_or_default();
        let rows: Vec<(String, String)> = (0..18).map(|i| if i == 0 { (2343, 2344) } else { (2344 + i, 2361 + i) }).map(|(a, b)| (text(&gpu, a), text(&gpu, b))).collect();
        eprintln!("QA: load list {:?}", rows.iter().filter(|r| !r.0.is_empty()).collect::<Vec<_>>());
        shot(&mut gpu, front_view, "7b_load_list");
        let i = rows.iter().position(|r| r.1.starts_with("not loadable")).expect("QA: no older save in the load list") as u16;
        click(&mut f, &mut gpu, 6, if i == 0 { 2343 } else { 2344 + i });
        let act = click(&mut f, &mut gpu, 6, 2342);
        assert!(act.is_none(), "QA: the load list loads an older save");
        shot(&mut gpu, front_view, "7c_load_list_older_selected");
        drop(f);

        // 5. A LAN game: host and client start screens on localhost (a port of our own), each
        // peer on its own thread with its own GPU as two processes would be (the host's start
        // handshake waits for the client's answers).
        let port = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let tick_until = |f: &mut frontend::Frontend, gpu: &mut Gpu, what: &str, done: &mut dyn FnMut(&mut frontend::Frontend) -> bool| -> Option<frontend::Action> {
            let t = Instant::now();
            loop {
                if let Some(a) = f.tick(gpu) {
                    return Some(a);
                }
                if done(f) {
                    return None;
                }
                assert!(t.elapsed() < std::time::Duration::from_secs(60), "QA: LAN: no {what} in 60 s");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        // A peer's game from its start action at speed 6 (14 ms a tick) to 3000 ticks, captured
        // every 1000; then paused. Returns the game.
        let run = |gpu: &mut Gpu, act: Option<frontend::Action>, who: &str| -> Game {
            let Some(frontend::Action::StartNet(s, map, players)) = act else { panic!("QA: LAN: no {who} start, open {:?}", open(gpu)) };
            eprintln!("QA: LAN {who}: player {} on {}, races {:?}", s.local, map.display(), players.races);
            let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start(&map.to_string_lossy(), &args, 6, gpu, Some((s, players))).unwrap();
            g.progress_file = None;
            // Building orders near the start, through the lockstep as a player's clicks.
            let (tx, rx) = std::sync::mpsc::channel();
            let local = g.sim.local;
            g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
                let _ = tx.send(qa_sites(w, local, &[1, 2, 4, 3, 41]));
            }));
            let orders = rx.recv().unwrap();
            eprintln!("QA: LAN {who}: build orders {orders:?}");
            for (k, x, y) in orders {
                g.sim.command(Stamped { tick: 0, command: s4_sim::PlayerCommand::new(local, s4_sim::Command::Build { building: k as u32, x, y }) });
            }
            let t = Instant::now();
            let mut next = 0;
            loop {
                g.redraw(gpu, (960.0, 540.0), None);
                gpu.wait().unwrap();
                if g.cur.tick >= next {
                    at_home(&mut g, gpu);
                    shot(gpu, g.cam.view(), &format!("9_lan_{who}_t{next}"));
                    census(&g, &format!("LAN {who}"));
                    if next >= 3000 {
                        break;
                    }
                    next += 1000;
                }
                assert!(t.elapsed() < std::time::Duration::from_secs(900), "QA: LAN {who} stuck at {}", g.cur.tick);
            }
            eprintln!("QA: LAN {who}: 3000 ticks in {:.0} s", t.elapsed().as_secs_f64());
            g.sim.set_speed(0);
            std::thread::sleep(std::time::Duration::from_millis(300));
            g
        };
        // Step a paused peer to `at`, then its tick and state hash.
        let state_at = |g: &Game, at: u32| {
            let now = g.sim.snapshot().tick;
            if now < at {
                g.sim.step(at - now);
            }
            let (tx, rx) = std::sync::mpsc::channel();
            g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
                let _ = tx.send((w.tick, w.state_hash()));
            }));
            rx.recv().unwrap()
        };
        let (to_main, from_client) = std::sync::mpsc::channel::<(u32, u64)>();
        let (to_client, from_main) = std::sync::mpsc::channel::<u32>();
        // Multiplayer → LAN → create (the map settings, two players) → the lobby.
        frontend::Frontend::fit(&mut gpu);
        let mut host = frontend::Frontend::new(&mut gpu, 0).unwrap();
        host.set_lan_port(port);
        click(&mut host, &mut gpu, 0, 2537);
        shot(&mut gpu, front_view, "8a_host_mp_type");
        click(&mut host, &mut gpu, 1, 2521);
        shot(&mut gpu, front_view, "8b_host_game_list");
        click(&mut host, &mut gpu, 2, 2311);
        let players = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.texts.get(&(3, 2443)).cloned().unwrap_or_default();
        for _ in 0..8 {
            if players(&gpu) == "2" {
                break;
            }
            click(&mut host, &mut gpu, 3, 2442);
        }
        shot(&mut gpu, front_view, "8c_host_map_settings");
        click(&mut host, &mut gpu, 3, 2433);
        assert_eq!(host.current_screen(), 4, "QA: the host's lobby, open {:?}", open(&gpu));
        let (hh, ch) = std::thread::scope(|sc| {
            // The client: Multiplayer → LAN → the address typed → the game → join → ready.
            let (shot, click, run, state_at, tick_until) = (&shot, &click, &run, &state_at, &tick_until);
            let client = sc.spawn(move || {
                let mut gc = Gpu::headless(w, h).unwrap();
                gc.set_offscreen(true);
                frontend::Frontend::fit(&mut gc);
                let mut join = frontend::Frontend::new(&mut gc, 0).unwrap();
                join.set_lan_port(port);
                click(&mut join, &mut gc, 0, 2537);
                click(&mut join, &mut gc, 1, 2521);
                click(&mut join, &mut gc, 2, 2313);
                if gc.ui.as_ref().unwrap().state.focus != Some((2, 2313)) {
                    eprintln!("QA: a click on the address field does not focus it");
                    gc.ui.as_mut().unwrap().state.focus = Some((2, 2313));
                }
                join.key_text(&mut gc, "127.0.0.1\r");
                tick_until(&mut join, &mut gc, "game found", &mut |j| j.keep_games_on(port) > 0);
                shot(&mut gc, front_view, "8d_client_game_list");
                click(&mut join, &mut gc, 2, 2277);
                tick_until(&mut join, &mut gc, "join", &mut |j| j.lobby_state().1 == Some(2));
                click(&mut join, &mut gc, 4, 2244);
                shot(&mut gc, front_view, "8f_client_lobby");
                let act = tick_until(&mut join, &mut gc, "start", &mut |_| false);
                drop(join);
                let g = run(&mut gc, act, "client");
                let _ = to_main.send((g.sim.snapshot().tick, 0));
                let at = from_main.recv().unwrap();
                let _ = to_main.send(state_at(&g, at));
            });
            tick_until(&mut host, &mut gpu, "client ready", &mut |f| f.lobby_state().0);
            shot(&mut gpu, front_view, "8e_host_lobby");
            let act = click(&mut host, &mut gpu, 4, 2142);
            drop(host);
            let g = run(&mut gpu, act, "host");
            let (ct, _) = from_client.recv().unwrap();
            let at = g.sim.snapshot().tick.max(ct);
            let _ = to_client.send(at);
            let hh = state_at(&g, at);
            let ch = from_client.recv().unwrap();
            client.join().unwrap();
            (hh, ch)
        });
        eprintln!("QA: LAN state: host {hh:?} client {ch:?}");
        let log = std::fs::read_to_string(out.join("log.txt")).unwrap_or_default();
        let desyncs: Vec<&str> = log.lines().filter(|l| l.contains("DESYNC")).collect();
        eprintln!("QA: LAN desync lines: {desyncs:?}");
        assert_eq!(hh, ch, "QA: LAN peers differ");
        assert!(desyncs.is_empty(), "QA: LAN desync");
        let _ = std::fs::remove_dir_all(&saves);
    }

    /// The missing-graphics audit for what computers rarely do (`--audit` covers the rest): on
    /// SeaQuest with Romans, Vikings, Mayas and Trojans, each player gets near its start a priest
    /// casting all eight of its race's spells (mana given) next to it, every settler type of its
    /// race walking, a squad of the next player's swordsmen (far enough not to kill the priest), a
    /// cart driving to the next player's start, a war machine with ammunition next to a tower of
    /// the next player, a warship, ferry and transport ship on the nearest water sailing, and two
    /// warships of the two players side by side with ammunition. 480 ticks through
    /// `Game::redraw`: every spell cast, the war machines and the warships shot their ammunition,
    /// and no draw missed an image or fell back (s4_viewer::audit). Captures every 120 ticks
    /// around player 1 (`out/viewer/scenarios/`).
    #[test]
    fn audit_scenarios() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        use s4_sim::{Command, PlayerCommand};
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        gpu.set_offscreen(true);
        let players = sim::PlayerSetup { races: [Some(0), Some(1), Some(2), Some(4), None, None, None, None], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control: [1, 1, 1, 1, 0, 0, 0, 0], game_type: 0, local: 0, players: Some(4), race_goods: [0; 7], decided: false, start_resources: 0 };
        let args = Args { players: Some(players), no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Multiplayer/SeaQuest", &args, 0, &mut gpu, None).unwrap();
        // Everything placed on the sim thread; the ids come back through a channel.
        let (tx, rx) = std::sync::mpsc::channel();
        // Only the settler types the race has (SettlerValues of that race with a role).
        let data = s4_formats::gamedata::GameData::load_default().unwrap();
        let has = move |race: u8, k: u16| data.settlers.iter().any(|i| i.race as u16 == race as u16 && i.settler as u16 == k && i.role.is_some());
        g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
            let size = w.grid.size() as i32;
            let n = w.player_count();
            let start = |w: &s4_sim::World, p: u8| w.players[p as usize].as_ref().map_or((size / 2, size / 2), |pl| (pl.start_x as i32, pl.start_y as i32));
            let mut out = Vec::new();
            for p in 1..=n {
                let next = p % n + 1;
                let (sx, sy) = start(w, p);
                let (nx, ny) = start(w, next);
                if let Some(pool) = w.magic.players.get_mut(p as usize) {
                    pool.mana = 1_000_000;
                }
                let priest = s4_sim::create::add_settler(w, sx + 3, sy + 3, p, 45);
                s4_sim::create::add_settlers_around(w, sx + 30, sy + 30, next, 29, 6);
                let mut walkers = Vec::new();
                let race = w.race_of(p);
                for k in (1..=66u16).filter(|&k| has(race, k)) {
                    if let Some(id) = s4_sim::create::add_settler(w, sx - 6 + (k as i32 % 8), sy - 6 + (k as i32 / 8), p, k) {
                        walkers.push(id);
                    }
                }
                let cars: Vec<u16> = [5u8].iter().filter_map(|&k| s4_sim::create::add_vehicle(w, sx + 5, sy + 2, p, k)).collect();
                // A siege: a war machine with ammunition near a tower of the next player.
                let tower = s4_sim::build::scripted::add_building_scripted(w, sx - 22, sy + 8, next, 46, 4);
                let machine = s4_sim::vehicles::scripted::add_vehicle(w, sx - 14, sy + 8, p as i32, 4, 0, 8, 0);
                // The nearest water nodes (ground below 8) for the three ships.
                let mut water = Vec::new();
                'find: for r in 1..60i32 {
                    for dy in -r..=r {
                        for dx in -r..=r {
                            if dx.abs().max(dy.abs()) != r {
                                continue;
                            }
                            if let Some(i) = w.grid.checked_index(sx + dx, sy + dy)
                                && w.layers.ground[i] < 8
                            {
                                water.push((sx + dx, sy + dy));
                                if water.len() >= 400 {
                                    break 'find;
                                }
                            }
                        }
                    }
                }
                let ships: Vec<u16> = (1u8..=3).zip(water.iter().step_by(13)).filter_map(|(k, &(x, y))| s4_sim::create::add_vehicle(w, x, y, p, k)).collect();
                // A sea battle: a warship with ammunition and the next player's next to it.
                let mut fleet = Vec::new();
                for &(x, y) in water.iter().rev() {
                    let q = if fleet.is_empty() { p } else { next };
                    let id = s4_sim::vehicles::scripted::add_vehicle(w, x, y, q as i32, 1, 0, 8, 0);
                    if id != 0 {
                        fleet.push(id);
                        if fleet.len() == 2 {
                            break;
                        }
                    }
                }
                out.push((p, priest, walkers, cars, ships, (sx, sy), (nx, ny), water, (tower, machine, fleet)));
            }
            let _ = tx.send(out);
        }));
        g.sim.step(1);
        let placed = rx.recv().unwrap();
        s4_viewer::audit::take();
        s4_viewer::audit::enable(true);
        let send = |g: &Game, p: u8, c: Command| {
            let t = g.sim.snapshot().tick + 1;
            g.sim.command(Stamped { tick: t, command: PlayerCommand::new(p, c) });
        };
        for (p, _, walkers, cars, ships, (sx, sy), (nx, ny), water, _) in &placed {
            send(&g, *p, Command::SendTo { x: (*sx + 10) as u16, y: (*sy - 8) as u16, mode: 0, entities: walkers.clone() });
            send(&g, *p, Command::SendTo { x: *nx as u16, y: *ny as u16, mode: 0, entities: cars.clone() });
            if let Some(&(x, y)) = water.last() {
                send(&g, *p, Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: ships.clone() });
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/scenarios");
        let (p1x, p1y) = placed[0].5;
        for t in 0..480u32 {
            if t % 60 == 0 && t / 60 < 8 {
                for (p, priest, _, _, _, (sx, sy), _, _, _) in &placed {
                    if let Some(id) = priest {
                        send(&g, *p, Command::CastSpell { priest: *id, spell: (t / 60) as u16, x: (*sx + 6) as u16, y: (*sy + 6) as u16 });
                    }
                }
            }
            g.sim.step(1);
            g.alpha = Some(0.5);
            g.redraw(&mut gpu, (0.0, 0.0), None);
            gpu.wait().unwrap();
            if t % 120 == 119 {
                g.cam.jump(view_on(&g.scene, Some((p1x as usize, p1y as usize)), gpu.size(), g.sim.local), &screen(&gpu, g.scene.source.w));
                g.redraw(&mut gpu, (0.0, 0.0), None);
                if let Ok(rgba) = gpu.capture(g.cam.view()) {
                    let _ = client::write_png(&out.join(format!("p1_t{t}.png")), 1280, 1024, &rgba);
                }
            }
        }
        s4_viewer::audit::enable(false);
        let misses = s4_viewer::audit::take();
        // The scenario happened: spells cast, ships and vehicles placed and moved.
        let (tx, rx) = std::sync::mpsc::channel();
        let battles: Vec<(u16, u16, Vec<u16>)> = placed.iter().map(|x| x.8.clone()).collect();
        let priests: Vec<Option<u16>> = placed.iter().map(|x| x.1).collect();
        g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
            let casts: Vec<Vec<u32>> = (1..=4u8).map(|p| (0..8u8).map(|sp| s4_sim::magic::casts(w, p, sp)).collect()).collect();
            // Ammunition left per war machine and warship (8 given; the Vikings shoot mana).
            let priests: Vec<bool> = priests.iter().map(|&id| id.is_some_and(|id| w.entities.by_index(id as u32).is_some_and(|e| e.kind == 45 && !e.is_dead()))).collect();
            let ammo: Vec<(u16, Vec<u8>)> = battles.iter().map(|(_, m, f)| (*m, std::iter::once(*m).chain(f.iter().copied()).filter_map(|id| s4_sim::vehicles::get(w, id).map(|v| v.cargo)).collect())).collect();
            let _ = tx.send((casts, ammo, priests));
        }));
        g.sim.step(1);
        let (casts, ammo, priests) = rx.recv().unwrap();
        eprintln!("priests alive: {priests:?}");
        eprintln!("ammunition left (war machine, warships) per player: {ammo:?}; towers {:?}", placed.iter().map(|x| x.8.0).collect::<Vec<_>>());
        let pop = &g.sim.snapshot().population;
        let ships = pop.vehicles.iter().filter(|v| (1..=3).contains(&v.kind)).count();
        let machines = pop.vehicles.iter().filter(|v| v.kind == 4).count();
        eprintln!("casts per player and spell: {casts:?}; ships {ships}, war machines {machines}");
        for (p, placed) in placed.iter().map(|x| (x.0, x.4.len())) {
            eprintln!("player {p}: {placed} ships placed");
        }
        assert!(casts.iter().flatten().all(|&c| c > 0), "spells cast: {casts:?}");
        assert!(priests.iter().all(|&a| a), "priests alive: {priests:?}");
        assert!(ammo.iter().all(|(_, a)| a.first() == Some(&0)), "the war machines shot: {ammo:?}");
        assert!(ammo.iter().any(|(_, a)| a.len() == 3 && a[1..] == [0, 0]), "a sea battle: {ammo:?}");
        assert!(ships >= 4 && machines >= 4, "ships {ships}, war machines {machines}");
        assert!(misses.is_empty(), "draws without an image or with a fallback:\n{}", misses.iter().map(|(k, n)| format!("{n:>6} {k}")).collect::<Vec<_>>().join("\n"));
    }

    /// roman01 (default victory rule: `ScriptDefaultVictoryConditionCheck` →
    /// `World::default_player_lost`) through the real game: the sim thread with the script, player
    /// 2's three towers killed → the script decides the game → dialog 72 says "You have won!" and
    /// the mission is recorded as won.
    /// The work-area mode end to end through the window's code path (`Game::mouse` / `redraw`):
    /// select roman02's woodcutter, its work-area button (D18 1084 → 0x193), a map click → 5001
    /// `SetWorkingArea` moves the area and the rings follow it.
    #[test]
    fn work_area_through_the_window_path() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        // Paused and stepped: no wall clock.
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let click = |g: &mut Game, gpu: &mut Gpu, p: (f64, f64)| {
            // As in the window: frames are drawn while the button is down.
            g.cursor_moved(gpu, p);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Pressed);
            for _ in 0..3 {
                g.redraw(gpu, p, None);
            }
            g.mouse(gpu, p, MouseButton::Left, ElementState::Released);
            g.redraw(gpu, p, None);
        };
        let wait = |g: &mut Game, gpu: &mut Gpu, until: &dyn Fn(&Game) -> bool| {
            for _ in 0..200 {
                g.redraw(gpu, (0.0, 0.0), None);
                if until(g) {
                    return;
                }
                g.sim.step(1);
            }
        };
        g.cam.jump(view_on(&g.scene, Some((137, 68)), (1280, 1024), g.sim.local), &screen(&gpu, g.scene.source.w));
        let p = node_screen(&g.scene, g.cam.view(), (137, 68));
        click(&mut g, &mut gpu, p);
        wait(&mut g, &mut gpu, &|g| g.sim.snapshot().building.as_ref().is_some_and(|f| f.id == 453));
        let before = g.sim.snapshot().building.clone().expect("woodcutter 453 watched");
        let b = client::control_point(&gpu, 18, 1084).expect("D18 work-area button");
        click(&mut g, &mut gpu, b);
        assert!(g.bdlg.in_mode(), "work-area mode on");
        assert!(!g.bdlg.markers(&g.sim.snapshot()).is_empty(), "rings shown");
        let target = (141, 74);
        let p = node_screen(&g.scene, g.cam.view(), target);
        click(&mut g, &mut gpu, p);
        wait(&mut g, &mut gpu, &|g| g.sim.snapshot().building.as_ref().is_some_and(|f| f.work_area != before.work_area));
        let after = g.sim.snapshot().building.clone().unwrap();
        assert_ne!(after.work_area, before.work_area, "5001 moved the working area");
        assert_eq!(g.pointer.selection.ids.first(), Some(&453), "the building stays selected");
        let rings = g.bdlg.markers(&g.sim.snapshot());
        assert!(!rings.is_empty(), "the mode stays on with the new rings");
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/bdlg/wa_window.png");
        if let Ok(rgba) = gpu.capture(g.cam.view()) {
            let _ = client::write_png(&out, 1280, 1024, &rgba);
        }
    }

    /// The right-click move end to end through the window's code path: a left drag over the
    /// local player's soldiers (box selection), then a right press and release on a map node
    /// without a drag (camera.rs → GUI event 0x1e0 → 5014 `SendTo`); the selected soldiers walk
    /// to the formation around that node, nobody else moves.
    #[test]
    fn right_click_move_through_the_window_path() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let soldier = |u: &s4_viewer::population::Unit| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xf));
        let snap = g.sim.snapshot();
        let first = *snap.population.settlers.iter().find(|u| soldier(u)).expect("roman01: player 1 has soldiers");
        g.cam.jump(view_on(&g.scene, Some((first.x as usize, first.y as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        let frames = |g: &mut Game, gpu: &mut Gpu, p: (f64, f64)| {
            for _ in 0..3 {
                g.redraw(gpu, p, None);
            }
        };
        // Box: 150 px around the first soldier.
        let c = node_screen(&g.scene, g.cam.view(), (first.x as usize, first.y as usize));
        let (a, b) = ((c.0 - 150.0, c.1 - 150.0), (c.0 + 150.0, c.1 + 150.0));
        g.cursor_moved(&mut gpu, a);
        g.mouse(&mut gpu, a, MouseButton::Left, ElementState::Pressed);
        frames(&mut g, &mut gpu, a);
        g.cursor_moved(&mut gpu, b);
        let scale = gpu.gui_scale as f64;
        g.pointer.moved(&mut gpu, b, scale);
        frames(&mut g, &mut gpu, b);
        g.mouse(&mut gpu, b, MouseButton::Left, ElementState::Released);
        frames(&mut g, &mut gpu, b);
        let ids = g.pointer.selection.ids.clone();
        assert!(!ids.is_empty() && ids.contains(&first.id), "box selected the soldiers: {ids:?}");
        let pos = |g: &Game| -> HashMap<u16, (u16, u16)> { g.sim.snapshot().population.settlers.iter().map(|u| (u.id, (u.x, u.y))).collect() };
        let before = pos(&g);
        // Right click 16 / 12 nodes down-right: press and release at the same point.
        let target = (first.x as usize + 16, first.y as usize + 12);
        let p = node_screen(&g.scene, g.cam.view(), target);
        g.cursor_moved(&mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Pressed);
        frames(&mut g, &mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Released);
        // Paused (speed 0): the sim only moves when stepped, so the run is the same every time.
        g.sim.step(400);
        g.redraw(&mut gpu, p, None);
        let after = pos(&g);
        let dist = |q: (u16, u16)| (q.0 as i32 - target.0 as i32).abs().max((q.1 as i32 - target.1 as i32).abs());
        // The formation's places are the target and `target + 2·spiral(k)`: all within 2·3 nodes
        // for up to 37 soldiers.
        for id in &ids {
            let (b, a) = (before[id], after[id]);
            assert!(a != b && dist(a) <= 6, "soldier {id} of {} walked to {target:?}: {b:?} → {a:?}", ids.len());
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/sendto/window.png");
        if let Ok(rgba) = gpu.capture(g.cam.view()) {
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, w, h, &rgba);
        }
    }

    /// Shift + click through the window path (select.rs → event 0x1aa, s4_viewer::selection
    /// `same_type_around`): on roman01, clicking a soldier selects only the local player's
    /// soldiers of its settler type (weapon and rank), and every such soldier within 10 nodes;
    /// with Alt at least as many. The release's forced order: Alt + right click sends mode bit 2.
    #[test]
    fn shift_click_selects_soldiers_of_one_type_and_rank() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        g.sim.step(20);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let pop = g.sim.snapshot().population.clone();
        // The soldier type the player has most of.
        let soldiers: Vec<_> = pop.settlers.iter().filter(|u| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xF))).collect();
        let count = |k: u16| soldiers.iter().filter(|u| u.kind == k).count();
        let first = **soldiers.iter().max_by_key(|u| count(u.kind)).expect("soldiers");
        eprintln!("soldier kinds: {:?}", soldiers.iter().map(|u| u.kind).collect::<std::collections::BTreeSet<_>>());
        g.cam.jump(view_on(&g.scene, Some((first.x as usize, first.y as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let p = node_screen(&g.scene, g.cam.view(), (first.x as usize, first.y as usize));
        let p = (p.0, p.1 - 6.0);
        let click = |g: &mut Game, gpu: &mut Gpu| {
            g.cursor_moved(gpu, p);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Pressed);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Released);
            g.pointer.selection.ids.clone()
        };
        g.pointer.shift = true;
        let near = click(&mut g, &mut gpu);
        g.pointer.shift = false;
        let picked: Vec<_> = near.iter().filter_map(|id| pop.settlers.iter().find(|u| u.id == *id)).collect();
        assert!(!picked.is_empty() && picked.len() == near.len(), "{near:?}");
        let kind = picked[0].kind;
        assert!(picked.iter().all(|u| u.kind == kind && u.owner == local && u.flags & 0xF == picked[0].flags & 0xF), "one type and rank: {:?}", picked.iter().map(|u| u.kind).collect::<Vec<_>>());
        let around: Vec<u16> = pop
            .settlers
            .iter()
            .filter(|u| u.owner == local && u.kind == kind && u.flags & 0x2000 != 0 && u.land == picked[0].land)
            .filter(|u| s4_sim::spiral::grid_distance(u.x as i32 - first.x as i32, u.y as i32 - first.y as i32) <= 10)
            .map(|u| u.id)
            .collect();
        assert!(around.iter().all(|id| near.contains(id)), "every one within 10 nodes: {around:?} in {near:?}");
        let others = pop.settlers.iter().filter(|u| u.owner == local && u.flags & 0xF != 0 && u.kind != kind).count();
        eprintln!("shift + click: {} of type {kind} selected ({} other soldiers / specialists of the player)", near.len(), others);
        g.pointer.alt = true;
        let far = click(&mut g, &mut gpu);
        assert!(far.len() >= near.len());
        // Alt + right click: `SendTo` with mode bit 2.
        let target = (first.x as usize + 3, first.y as usize);
        let ev = g.pointer.send(&mut gpu, &pop, target);
        g.pointer.alt = false;
        assert!(matches!(ev.first(), Some(s4_viewer::guistate::UiEvent::SendTo { mode: 4, .. })), "{ev:?}");
    }

    #[test]
    fn click_picks_a_soldier_at_every_zoom() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let snap = g.sim.snapshot();
        let first = *snap.population.settlers.iter().find(|u| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xf))).unwrap();
        let node = (first.x as usize, first.y as usize);
        // +0x4c from 0xe0000 to 0x480000 (14 to 72 px per node column; 24 is zoom 1).
        for px in [14.0f32, 24.0, 43.0, 72.0] {
            let zoom = px / 24.0;
            let c = g.scene.node_world(node.0, node.1);
            g.cam.jump(View { x: c[0] - w as f32 / 2.0 / zoom, y: c[1] - h as f32 / 2.0 / zoom, zoom }, &screen(&gpu, g.scene.source.w));
            g.redraw(&mut gpu, (640.0, 512.0), None);
            g.pointer.selection.ids.clear();
            // A few px above the node's point: inside the soldier's box at every zoom.
            let p = node_screen(&g.scene, g.cam.view(), node);
            let p = (p.0, p.1 - 6.0 * zoom as f64);
            g.cursor_moved(&mut gpu, p);
            g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Pressed);
            g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Released);
            let ids = &g.pointer.selection.ids;
            let picked = ids.first().and_then(|id| g.sim.snapshot().population.settlers.iter().find(|u| u.id == *id).copied());
            assert!(picked.is_some_and(|u| (u.x as usize, u.y as usize) == node), "zoom {px} px: picked {ids:?}");
        }
    }

    /// The world point under screen point `p`.
    fn world_under(g: &Game, gpu: &Gpu, p: (f64, f64)) -> (f32, f32) {
        let v = g.cam.view();
        let ez = client::eff_zoom(gpu, v);
        (v.x + p.0 as f32 / ez, v.y + p.1 as f32 / ez)
    }

    /// The camera through the window path (camera.rs), at HiDPI 1 and 1.5: a right drag keeps
    /// the grabbed world point under the cursor across uneven frames (7..200 ms) and coalesced
    /// moves, the release adds nothing; a right press and release without movement is a click
    /// (the view stays); the wheel's zoom keeps the point under the cursor while it eases.
    /// Traced to `out/viewer/camera/drag_trace.txt`.
    #[test]
    fn camera_drag_and_zoom_through_the_window() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        use std::fmt::Write as _;
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 8, &mut gpu, None).unwrap();
        let frame = |g: &mut Game, gpu: &mut Gpu, p: (f64, f64), ms: u64| {
            g.last = Instant::now() - std::time::Duration::from_millis(ms);
            g.redraw(gpu, p, None);
        };
        let mut trace = String::new();
        for hidpi in [false, true] {
            gpu.effects.hidpi = hidpi;
            gpu.scale_factor = if hidpi { 1.5 } else { 1.0 };
            let mut p = (640.0, 512.0);
            g.cursor_moved(&mut gpu, p);
            frame(&mut g, &mut gpu, p, 16);
            // A click: no movement, the view stays, nothing is held after.
            let v = g.cam.view();
            g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Pressed);
            g.cursor_moved(&mut gpu, (p.0 + 3.0, p.1 - 4.0));
            frame(&mut g, &mut gpu, p, 16);
            g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Released);
            assert!(g.cam.view() == v && !g.cam.right_held(), "a click moved the view");
            // A drag.
            g.cursor_moved(&mut gpu, p);
            g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Pressed);
            let grabbed = world_under(&g, &gpu, p);
            let plan: Vec<((f64, f64), u64)> = (0..20)
                .map(|i| ((6.0, if i % 2 == 0 { 3.0 } else { -2.0 }), 16))
                .chain([((120.0, 40.0), 200), ((-7.0, 0.0), 0), ((-150.0, 25.0), 120), ((4.0, -60.0), 7), ((30.0, 30.0), 33)])
                .chain((0..15).map(|_| ((-9.0, 5.0), 33)))
                .collect();
            let _ = writeln!(trace, "hidpi {hidpi}: grabbed {grabbed:?}");
            for (i, (d, ms)) in plan.iter().enumerate() {
                p = (p.0 + d.0, p.1 + d.1);
                g.cursor_moved(&mut gpu, p);
                if *ms > 0 {
                    frame(&mut g, &mut gpu, p, *ms);
                }
                let u = world_under(&g, &gpu, p);
                let _ = writeln!(trace, "move {i:2} {:+6.1} {:+6.1}, frame {ms:3} ms: under the cursor {u:?}", d.0, d.1);
                assert!((u.0 - grabbed.0).abs() < 1e-2 && (u.1 - grabbed.1).abs() < 1e-2, "hidpi {hidpi} move {i}: {u:?} for {grabbed:?}");
            }
            let v = g.cam.view();
            g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Released);
            frame(&mut g, &mut gpu, p, 16);
            assert!(g.cam.view() == v, "the release moved the view");
            // The wheel: one notch in around the cursor, eased over the frames.
            let at = (900.0, 300.0);
            g.cursor_moved(&mut gpu, at);
            let u0 = world_under(&g, &gpu, at);
            let z0 = g.cam.view().zoom;
            zoom_camera(&mut g.cam, &mut gpu, camera::wheel_units(120), Some(at), g.scene.source.w);
            for ms in [16u64, 7, 33, 16, 16, 100, 16, 16, 250, 250] {
                frame(&mut g, &mut gpu, at, ms);
                let u = world_under(&g, &gpu, at);
                assert!((u.0 - u0.0).abs() < 1e-2 && (u.1 - u0.1).abs() < 1e-2, "hidpi {hidpi}: zoom moved the point {u:?} {u0:?}");
            }
            assert!(g.cam.view().zoom > z0 && g.cam.view().zoom == g.cam.zoom_target(), "{} {z0}", g.cam.view().zoom);
            zoom_camera(&mut g.cam, &mut gpu, camera::wheel_units(-120), Some(at), g.scene.source.w);
            for _ in 0..4 {
                frame(&mut g, &mut gpu, at, 250);
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/camera");
        let _ = std::fs::create_dir_all(&out);
        let _ = std::fs::write(out.join("drag_trace.txt"), trace);
    }

    /// Camera keys through the window path: a held arrow or W / A / S / D moves the view each
    /// frame by `KEY_SPEED` times the frame's real time (screen px), a release stops it, a letter
    /// with Ctrl does not scroll, and nothing moves while the chat input has the keyboard.
    #[test]
    fn camera_keys_scroll_while_held() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 8, &mut gpu, None).unwrap();
        let centre = (640.0, 512.0);
        g.redraw(&mut gpu, centre, None);
        // One frame of `ms` real time: the view's move in screen px.
        let frame = |g: &mut Game, gpu: &mut Gpu, ms: u64| {
            g.last = Instant::now() - std::time::Duration::from_millis(ms);
            let (v, z) = (g.cam.view(), client::eff_zoom(gpu, g.cam.view()));
            g.redraw(gpu, centre, None);
            ((g.cam.view().x - v.x) * z, (g.cam.view().y - v.y) * z)
        };
        assert_eq!(frame(&mut g, &mut gpu, 50), (0.0, 0.0));
        for (code, dir) in [(KeyCode::ArrowRight, (1.0, 0.0)), (KeyCode::KeyA, (-1.0, 0.0)), (KeyCode::KeyS, (0.0, 1.0)), (KeyCode::ArrowUp, (0.0, -1.0))] {
            g.key(&mut gpu, code, true, false);
            // Past the ramp: the full speed, frame after frame (the redraw itself adds a little time).
            frame(&mut g, &mut gpu, 200);
            for ms in [16u64, 7, 33] {
                let (dx, dy) = frame(&mut g, &mut gpu, ms);
                let want = camera::KEY_SPEED * ms as f32 / 1000.0;
                let along = dx * dir.0 + dy * dir.1;
                assert!(along >= want - 0.5 && along < want + 20.0, "{code:?}: {dx} {dy} in {ms} ms, want {want}");
                assert!((dx * dir.1).abs() < 1e-3 && (dy * dir.0).abs() < 1e-3, "{code:?}: off axis {dx} {dy}");
            }
            g.key(&mut gpu, code, false, false);
            assert_eq!(frame(&mut g, &mut gpu, 50), (0.0, 0.0), "{code:?} released");
        }
        // A letter with Ctrl is not a camera key (Ctrl+S is QuickSave; not pressed here, it would
        // write the save folder).
        g.pointer.ctrl = true;
        g.key(&mut gpu, KeyCode::KeyW, true, false);
        g.pointer.ctrl = false;
        assert_eq!(frame(&mut g, &mut gpu, 50), (0.0, 0.0), "Ctrl+W");
        g.key(&mut gpu, KeyCode::KeyW, false, false);
        // The chat input has the keyboard: no camera keys.
        g.key(&mut gpu, KeyCode::Enter, true, false);
        assert!(text_focus(&gpu));
        g.key(&mut gpu, KeyCode::ArrowLeft, true, false);
        assert_eq!(frame(&mut g, &mut gpu, 100), (0.0, 0.0), "chat focused");
    }

    /// Every other camera source goes through the camera and lands where it did: the minimap
    /// click (0x514ce0), Home and H (the start), a group's and the go-to buttons' centring
    /// (bdlg / pages, event 0x13), the tutorial's `Map.SetScreenPos` and `Tutorial.SetZoom`,
    /// the zoom slider (eased to its zoom), the border scroll (a velocity), the saved view.
    #[test]
    fn camera_jump_sources() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 8, &mut gpu, None).unwrap();
        let size = g.scene.source.w;
        let centre = (640.0, 512.0);
        let frame = |g: &mut Game, gpu: &mut Gpu, ms: u64| {
            g.last = Instant::now() - std::time::Duration::from_millis(ms);
            g.redraw(gpu, centre, None);
        };
        frame(&mut g, &mut gpu, 16);
        let near = |a: View, b: View, what: &str| {
            assert!((a.x - b.x).abs() <= 1.0 && (a.y - b.y).abs() <= 1.0 && (a.zoom - b.zoom).abs() < 1e-4, "{what}: {a:?}, want {b:?}");
        };
        let away = |g: &mut Game, gpu: &mut Gpu| {
            let v = client::view_on_node(gpu, g.cam.view(), (40, 200), size);
            g.jump(gpu, v);
        };
        // The minimap (panel point (80, 80): x = u + 15 + (160 − v) / 2, y = v + 8).
        away(&mut g, &mut gpu);
        let before = g.cam.view();
        let p = (80.0 + 15.0 + 40.0, 88.0);
        g.cursor_moved(&mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Pressed);
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Released);
        near(g.cam.view(), client::view_from_minimap(&gpu, before, (80, 80), size), "minimap");
        g.cursor_moved(&mut gpu, centre);
        // Home and H.
        let st = g.cur.population.starts[g.sim.local as usize];
        let home = client::view_on_node(&gpu, g.cam.view(), (st.0 as usize, st.1 as usize), size);
        for k in [KeyCode::Home, KeyCode::KeyH] {
            away(&mut g, &mut gpu);
            g.key(&mut gpu, k, true, false);
            g.key(&mut gpu, k, false, false);
            near(g.cam.view(), home, &format!("{k:?}"));
        }
        // A group's / the go-to buttons' centre (bdlg, pages).
        for which in 0..2 {
            away(&mut g, &mut gpu);
            if which == 0 {
                g.bdlg.centre = Some((100, 120));
            } else {
                g.pages.centre = Some((100, 120));
            }
            frame(&mut g, &mut gpu, 16);
            near(g.cam.view(), client::view_on_node(&gpu, g.cam.view(), (100, 120), size), if which == 0 { "bdlg centre" } else { "pages centre" });
        }
        // The tutorial's camera calls.
        away(&mut g, &mut gpu);
        g.sim.tutorial.lock().unwrap().requests.push(tutorial::Request::SetScreenPos(90, 110));
        frame(&mut g, &mut gpu, 16);
        near(g.cam.view(), client::view_on_node(&gpu, g.cam.view(), (90, 110), size), "SetScreenPos");
        let before = g.cam.view();
        g.sim.tutorial.lock().unwrap().requests.push(tutorial::Request::SetZoom(70));
        frame(&mut g, &mut gpu, 16);
        near(g.cam.view(), client::zoom_from_slider(&gpu, before, 70, size), "SetZoom");
        // The zoom slider: eased to the slider's zoom.
        g.msg_events(&mut gpu, &[s4_viewer::guistate::UiEvent::Slider { dialog: 0, link: 0x265, value: 20 }]);
        for _ in 0..60 {
            frame(&mut g, &mut gpu, 16);
        }
        let want = client::zoom_of_slider(20) as f32 / (24.0 * 65536.0);
        assert!((g.cam.view().zoom - want).abs() < 1e-4, "slider: {} want {want}", g.cam.view().zoom);
        // The border scroll: the left edge for 0.5 s of frames moves left at the key speed family.
        g.cam.border = true;
        g.cam.step = 72;
        away(&mut g, &mut gpu);
        let x0 = g.cam.view().x;
        g.cursor_moved(&mut gpu, (0.0, 512.0));
        for _ in 0..30 {
            frame(&mut g, &mut gpu, 16);
        }
        assert!(g.cam.view().x < x0 - 100.0, "border scroll: {} from {x0}", g.cam.view().x);
        g.cursor_moved(&mut gpu, centre);
        // The saved view is the camera's.
        let v = g.cam.view();
        assert_eq!(g.client_state().view, (v.x, v.y, v.zoom));
    }

    /// The cursor shape through the window path (cursor.rs, 0x4f2900 at each logic tick): the
    /// arrow without a selection; selected soldiers the sword (0x11), with Ctrl 0x13, with Alt
    /// 0xa; a pause 0x1f and back (0x514be0); the build mode 3 (`TryBuild` 0x4ea250).
    #[test]
    fn cursor_shapes_follow_the_selection_keys_and_modes() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.sim.set_speed(1);
        let local = g.sim.local;
        let tick = |g: &mut Game, gpu: &mut Gpu| {
            g.sim.step(1);
            g.redraw(gpu, (640.0, 512.0), None);
        };
        tick(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::ARROW);
        let soldier = |u: &s4_viewer::population::Unit| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xf));
        let first = *g.sim.snapshot().population.settlers.iter().find(|u| soldier(u)).expect("roman01: player 1 has soldiers");
        g.cam.jump(view_on(&g.scene, Some((first.x as usize, first.y as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        let c = node_screen(&g.scene, g.cam.view(), (first.x as usize, first.y as usize));
        let (a, b) = ((c.0 - 150.0, c.1 - 150.0), (c.0 + 150.0, c.1 + 150.0));
        g.cursor_moved(&mut gpu, a);
        g.mouse(&mut gpu, a, MouseButton::Left, ElementState::Pressed);
        g.cursor_moved(&mut gpu, b);
        let scale = gpu.gui_scale as f64;
        g.pointer.moved(&mut gpu, b, scale);
        g.mouse(&mut gpu, b, MouseButton::Left, ElementState::Released);
        tick(&mut g, &mut gpu);
        assert!(!g.pointer.selection.ids.is_empty(), "soldiers selected");
        assert_eq!(g.cursor_shape, cursor::shape::SOLDIERS);
        g.pointer.ctrl = true;
        tick(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::SOLDIERS_CTRL, "Ctrl, at the next tick");
        g.pointer.ctrl = false;
        g.alt = true;
        tick(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::SEND, "Alt");
        g.alt = false;
        tick(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::SOLDIERS);
        // Pause and back.
        g.sim.set_speed(0);
        tick(&mut g, &mut gpu);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert_eq!(g.cursor_shape, cursor::shape::PAUSE);
        g.sim.set_speed(1);
        tick(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::SOLDIERS, "the shape before the pause");
        // The build mode (a woodcutter from the build page), then left with Escape.
        g.build.building = Some(1);
        tick(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::BUILD);
    }

    /// Selection groups by the digit keys (EventManagement key case 0xb): Ctrl+1 stores the
    /// selection (0x1d6 → command 5041), '1' recalls it (0x1d8), '1' again within 6 ticks moves
    /// the camera to it (0x1d9 → event 0x13); 'H' goes to the start position (case 0xd).
    #[test]
    fn selection_groups_by_the_digit_keys() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let tick = |g: &mut Game, gpu: &mut Gpu| {
            g.sim.step(1);
            g.redraw(gpu, (640.0, 512.0), None);
        };
        tick(&mut g, &mut gpu);
        let soldier = |u: &s4_viewer::population::Unit| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xf));
        let first = *g.sim.snapshot().population.settlers.iter().find(|u| soldier(u)).expect("roman01: player 1 has soldiers");
        g.cam.jump(view_on(&g.scene, Some((first.x as usize, first.y as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        let c = node_screen(&g.scene, g.cam.view(), (first.x as usize, first.y as usize));
        let (a, b) = ((c.0 - 150.0, c.1 - 150.0), (c.0 + 150.0, c.1 + 150.0));
        g.cursor_moved(&mut gpu, a);
        g.mouse(&mut gpu, a, MouseButton::Left, ElementState::Pressed);
        g.cursor_moved(&mut gpu, b);
        let scale = gpu.gui_scale as f64;
        g.pointer.moved(&mut gpu, b, scale);
        g.mouse(&mut gpu, b, MouseButton::Left, ElementState::Released);
        tick(&mut g, &mut gpu);
        let mut chosen = g.pointer.selection.ids.clone();
        assert!(!chosen.is_empty(), "soldiers selected");
        // Ctrl+1: command 5041 at its tick.
        g.pointer.ctrl = true;
        g.key(&mut gpu, KeyCode::Digit1, true, false);
        g.pointer.ctrl = false;
        for _ in 0..12 {
            tick(&mut g, &mut gpu);
        }
        let stored = g.cur.population.groups[local as usize * s4_sim::group::GROUPS + 1].clone();
        assert!(!stored.is_empty(), "group 1 stored");
        // Away: no selection, the camera elsewhere.
        let pop = g.cur.population.clone();
        client::clear_selection(&mut gpu, &pop, &mut g.pointer.selection);
        g.cam.view().x += 900.0;
        g.cam.view().y += 500.0;
        tick(&mut g, &mut gpu);
        // '1': the group's selectable members, in group order.
        let away = g.cam.view();
        g.key(&mut gpu, KeyCode::Digit1, true, false);
        let mut got = g.pointer.selection.ids.clone();
        got.sort();
        chosen.sort();
        let mut want = stored.clone();
        want.sort();
        assert_eq!(got, want, "recalled");
        assert!(want.iter().all(|id| chosen.contains(id)), "the stored ones were selected");
        assert_eq!((g.cam.view().x, g.cam.view().y), (away.x, away.y), "the first press does not move the camera");
        // '1' again within 6 ticks: the camera to the group's mean position.
        tick(&mut g, &mut gpu);
        g.key(&mut gpu, KeyCode::Digit1, true, false);
        let node = client::camera_node(&gpu, g.cam.view(), g.scene.source.w).unwrap();
        let pop = &g.cur.population;
        let pos: Vec<(u16, u16)> = stored.iter().filter_map(|id| pop.settlers.iter().find(|u| u.id == *id)).map(|u| (u.x, u.y)).collect();
        let mean = (pos.iter().map(|p| p.0 as i32).sum::<i32>() / pos.len() as i32, pos.iter().map(|p| p.1 as i32).sum::<i32>() / pos.len() as i32);
        assert!((node.0 as i32 - mean.0).abs() <= 2 && (node.1 as i32 - mean.1).abs() <= 2, "camera at {node:?}, group at {mean:?}");
        // 'H': the start position.
        g.key(&mut gpu, KeyCode::KeyH, true, false);
        let start = g.cur.population.starts[local as usize];
        let node = client::camera_node(&gpu, g.cam.view(), g.scene.source.w).unwrap();
        assert!((node.0 as i32 - start.0 as i32).abs() <= 2 && (node.1 as i32 - start.1 as i32).abs() <= 2, "{node:?} vs start {start:?}");
        // Backspace (0x19f, 0x4ecda0): keep the wounded (below half health), with Shift the
        // healthy. The roman01 soldiers are all healthy.
        for _ in 0..8 {
            tick(&mut g, &mut gpu);
        }
        g.key(&mut gpu, KeyCode::Digit1, true, false);
        let all = g.pointer.selection.ids.clone();
        assert!(!all.is_empty());
        g.pointer.shift = true;
        g.key(&mut gpu, KeyCode::Backspace, true, false);
        g.pointer.shift = false;
        assert_eq!(g.pointer.selection.ids, all, "Shift+Backspace keeps the healthy");
        g.key(&mut gpu, KeyCode::Backspace, true, false);
        assert!(g.pointer.selection.ids.is_empty(), "Backspace keeps only the wounded: none");
    }

    /// Both buttons through the window path: right then left down, a move 40 px down (the
    /// original's cursor 40 px below the centre it was put at) zooms out by 10 units (0x5198e0),
    /// releasing one button ends it.
    #[test]
    fn both_buttons_zoom_through_the_window_path() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 8, &mut gpu, None).unwrap();
        let p = (700.0, 500.0);
        g.cursor_moved(&mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Pressed);
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Pressed);
        // The wanted zoom (+0x4c); the view eases to it.
        let target = |g: &Game| (g.cam.zoom_target() * 24.0 * 65536.0).round() as i32;
        let before = target(&g);
        g.cursor_moved(&mut gpu, (700.0, 540.0));
        let after = target(&g);
        assert_eq!(after, before - 10 * client::ZOOM_STEP, "40 px down: 10 units out");
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Released);
        assert!(!g.cam.both_held());
        g.cursor_moved(&mut gpu, (640.0, 600.0));
        assert_eq!(target(&g), after, "no zoom after the release");
        for _ in 0..4 {
            g.last = Instant::now() - std::time::Duration::from_millis(250);
            g.redraw(&mut gpu, (640.0, 600.0), None);
        }
        assert!((client::camera_zoom(&gpu, g.cam.view()) - after).abs() <= 1, "eased to the wanted zoom");
    }

    /// Chat in a single player game: Enter opens the window and the chat input (three line
    /// slots); a typed line and return send it as 0xFA5 to the recipients (our bit included); it
    /// arrives at its tick as "[team] name: text" in the message list (0x52a060) and the input
    /// is hidden again.
    #[test]
    fn chat_line_goes_round() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 8, &mut gpu, None).unwrap();
        g.key(&mut gpu, KeyCode::Enter, true, false);
        let ui = gpu.ui.as_ref().unwrap();
        let d = ui.msg_dialog.expect("the window opened");
        assert_eq!(ui.state.focus, Some((d, s4_viewer::msg_window::CHAT_INPUT)));
        assert_eq!(ui.msg.visible, 3);
        // The line runs at its execution tick (msgwin.rs: `execution_tick` of the tick it was
        // typed in): wait for the real-time sim to pass it, not for wall time, which a loaded
        // machine stretches (the guard only catches a sim that stopped).
        let typed = g.sim.snapshot().tick;
        assert!(g.type_text(&mut gpu, "hello all\r"));
        let ui = gpu.ui.as_ref().unwrap();
        assert_eq!(ui.state.focus, None);
        assert_eq!(ui.msg.visible, 4);
        // Under load the sim can run past the tick before a redraw takes the line in: keep
        // redrawing until it is there; it is missing for real once the sim is 64 ticks past due.
        let due = s4_net::commands::execution_tick(typed) + 8;
        let guard = Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let tick = g.sim.snapshot().tick;
            g.redraw(&mut gpu, (640.0, 500.0), None);
            if g.messages.list.iter().any(|m| m.formatted.is_some()) {
                break;
            }
            assert!(tick <= due + 64, "the chat line did not arrive (due by tick {due}, now {tick})");
            assert!(Instant::now() < guard, "the sim stopped at tick {tick} (line due by {due})");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let m = g.messages.list.iter().find(|m| m.formatted.is_some()).unwrap();
        let want = format!("[{}] {}: hello all", g.cur.population.teams[1], g.settings.player_name);
        assert_eq!(m.formatted.as_deref(), Some(want.as_str()));
        assert_eq!((m.text, m.life, m.icon), (0x2a7, -1, 0x1f9));
    }

    /// The cheat console (`EventManagement` 0x26): "!incubation2" switches the cheats on with
    /// 0x52a100's line (text 0xefe, player 9), a Lua line runs in the mission script's state
    /// (`Settlers.AddSettlers`, 0x515420 → 0x67f2a0), "!win" sends event 0x37: the game is won.
    #[test]
    fn cheat_console() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let say = |g: &mut Game, gpu: &mut Gpu, t: &str| {
            g.key(gpu, KeyCode::Enter, true, false);
            assert!(g.type_text(gpu, t));
        };
        let own = |g: &Game| {
            let (tx, rx) = std::sync::mpsc::channel();
            g.sim.with_world(move |w| {
                let s: Vec<_> = w.entities.iter().filter(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Settler && e.owner == 1).map(|(_, e)| (e.pos.x(), e.pos.y())).collect();
                let _ = tx.send(s);
            });
            g.sim.step(1);
            rx.recv().unwrap()
        };
        let before = own(&g);
        let (x, y) = before[0];
        let lua = format!("!Settlers.AddSettlers({x}, {y}, 1, 1, 5)\r");
        // Off: dropped.
        say(&mut g, &mut gpu, &lua);
        assert_eq!(own(&g).len(), before.len());
        say(&mut g, &mut gpu, "!incubation2\r");
        assert!(g.console.cheats);
        let m = g.messages.list.last().unwrap();
        assert_eq!((m.text, m.player, m.formatted.as_deref()), (0xefe, 9, Some("Cheats activated!")));
        say(&mut g, &mut gpu, &lua);
        assert_eq!(own(&g).len(), before.len() + 5);
        assert_eq!(g.sim.outcome.load(std::sync::atomic::Ordering::Relaxed), 0);
        say(&mut g, &mut gpu, "!win\r");
        assert_eq!(g.sim.outcome.load(std::sync::atomic::Ordering::Relaxed), script::WON);
        // No chat line went out for any of them.
        assert!(g.messages.list.iter().all(|m| m.text != 0x2a7));
    }

    /// QA pass 3 (2026-10-24) over the day's additions, headless at 1920×1080, captures into
    /// `out/qa3/` for a person to look at:
    /// * the monitor over a busy game (Choices, 4 computers, 8000 ticks): hidden, every tab,
    ///   paths of a selection and of a player, minimized, F3 back to hidden;
    /// * Shift / Alt + click among swordsmen of three ranks and bowmen;
    /// * a 300-swordsman selection sent by a right click, and its formation;
    /// * the wind on a forest at two zooms (frames a few ticks apart);
    /// * tower bowmen shooting in a siege (the user's `towerbug.s4rs`, when present).
    ///
    /// Prints `QA3` lines. `cargo test --release -p s4-game qa_pass3 -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn qa_pass3() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/qa3");
        let _ = std::fs::create_dir_all(&out);
        let saves = std::env::temp_dir().join(format!("s4rs-qa3-saves-{}", std::process::id()));
        // SAFETY (set_var): the only test that reads it runs alone (ignored, run by name).
        unsafe { std::env::set_var("S4RS_SAVE_DIR", &saves) };
        let (w, h) = (1920u32, 1080u32);
        let centre = (960.0, 540.0);
        let mut gpu = Gpu::headless(w, h).unwrap();
        gpu.set_offscreen(true);
        let shot = |gpu: &mut Gpu, view: View, name: &str| match gpu.capture(view) {
            Ok(rgba) => {
                let _ = client::write_png(&out.join(format!("{name}.png")), w, h, &rgba);
                eprintln!("QA3 capture {name}");
                rgba
            }
            Err(e) => {
                eprintln!("QA3 capture {name}: {e}");
                Vec::new()
            }
        };
        let players = sim::PlayerSetup { races: [Some(0), Some(1), Some(2), Some(4), None, None, None, None], teams: [0, 1, 2, 3, 4, 5, 6, 7].map(Some), control: [3, 3, 3, 3, 0, 0, 0, 0], game_type: 0, local: 0, players: Some(4), race_goods: [0; 7], decided: false, start_resources: 2 };
        let args = Args { players: Some(players), no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Multiplayer/Choices", &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        g.autosave_interval = None;
        let local = g.sim.local;
        let t = Instant::now();
        while g.cur.tick < 8000 {
            g.sim.step(500);
            g.redraw(&mut gpu, centre, None);
        }
        eprintln!("QA3: Choices at {} in {:.0} s: {} settlers, {} buildings", g.cur.tick, t.elapsed().as_secs_f64(), g.cur.population.settlers.len(), g.cur.population.buildings.len());
        let size = g.scene.source.w;
        let home = |g: &Game| g.cur.population.starts.get(g.sim.local as usize).copied().map_or((100, 100), |s| (s.0 as usize, s.1 as usize));
        let jump = |g: &mut Game, gpu: &mut Gpu, node: (usize, usize)| {
            g.cam.jump(view_on(&g.scene, Some(node), gpu.size(), g.sim.local), &screen(gpu, size));
            g.redraw(gpu, centre, None);
        };
        let st = home(&g);
        jump(&mut g, &mut gpu, st);

        // 1. The monitor.
        {
            use monitor::{Feed, Monitor};
            let walkers: Vec<u16> = g.cur.population.settlers.iter().filter(|u| u.owner == local && u.task == 6).map(|u| u.id).take(12).collect();
            let mut last_tick = Instant::now();
            let frames = |g: &mut Game, gpu: &mut Gpu, n: usize, sel: &[u16], last_tick: &mut Instant| {
                for _ in 0..n {
                    let f = Instant::now();
                    if last_tick.elapsed().as_millis() >= 71 {
                        g.sim.step(1);
                        *last_tick = Instant::now();
                    }
                    let scene = &g.scene;
                    let world = |x: i32, y: i32| scene.node_world(x.max(0) as usize, y.max(0) as usize);
                    Monitor::of(gpu, None).frame(None, Some(egui::RawInput::default()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local, selection: sel, sim: Some(&g.sim.monitor), world: Some(&world) });
                    g.redraw(gpu, centre, None);
                    gpu.wait().unwrap();
                    let spent = f.elapsed();
                    if spent < std::time::Duration::from_millis(16) {
                        std::thread::sleep(std::time::Duration::from_millis(16) - spent);
                    }
                }
            };
            // The overlay's layout is drawn by the next draw (a capture draws too): lay it out,
            // then capture.
            let monitor_shot = |g: &mut Game, gpu: &mut Gpu, sel: &[u16], name: &str| {
                let scene = &g.scene;
                let world = |x: i32, y: i32| scene.node_world(x.max(0) as usize, y.max(0) as usize);
                Monitor::of(gpu, None).frame(None, Some(egui::RawInput::default()), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local, selection: sel, sim: Some(&g.sim.monitor), world: Some(&world) });
                shot(gpu, g.cam.view(), name);
            };
            frames(&mut g, &mut gpu, 3, &walkers, &mut last_tick);
            monitor_shot(&mut g, &mut gpu, &walkers, "1a_monitor_hidden");
            Monitor::of(&mut gpu, None).toggle();
            for (k, name) in ["system", "simulation", "economy", "paths", "units", "ai", "queues"].iter().enumerate() {
                Monitor::of(&mut gpu, None).qa_tab(k);
                if k == 3 {
                    Monitor::of(&mut gpu, None).qa_paths(2);
                }
                frames(&mut g, &mut gpu, if k == 0 { 150 } else { 50 }, &walkers, &mut last_tick);
                if k == 3 {
                    let n = Monitor::of(&mut gpu, None).qa_paths(2);
                    eprintln!("QA3: paths tab: {n} lines ({} selected walkers + player 2's)", walkers.len());
                }
                monitor_shot(&mut g, &mut gpu, &walkers, &format!("1b_monitor_{k}_{name}"));
            }
            // The minimize button, then F3.
            if let Some(r) = Monitor::of(&mut gpu, None).qa_buttons()[0] {
                for pressed in [true, false] {
                    let mut raw = egui::RawInput::default();
                    raw.events.push(egui::Event::PointerMoved(r.center()));
                    raw.events.push(egui::Event::PointerButton { pos: r.center(), button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
                    let scene = &g.scene;
                    let world = |x: i32, y: i32| scene.node_world(x.max(0) as usize, y.max(0) as usize);
                    Monitor::of(&mut gpu, None).frame(None, Some(raw), (w, h), 1.0, &Feed { snapshot: Some(&g.cur), local, selection: &walkers, sim: Some(&g.sim.monitor), world: Some(&world) });
                }
            }
            frames(&mut g, &mut gpu, 5, &walkers, &mut last_tick);
            eprintln!("QA3: after the minimize button: {:?}", Monitor::of(&mut gpu, None).mode());
            monitor_shot(&mut g, &mut gpu, &walkers, "1c_monitor_minimized");
            Monitor::of(&mut gpu, None).toggle();
            frames(&mut g, &mut gpu, 3, &walkers, &mut last_tick);
            eprintln!("QA3: after F3: {:?}", Monitor::of(&mut gpu, None).mode());
            monitor_shot(&mut g, &mut gpu, &walkers, "1d_monitor_f3_hidden");
        }

        // 2. Shift / Alt + click among three ranks of swordsmen and bowmen.
        let spot = (st.0 + 14, st.1 + 10);
        g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
            for (k, kind) in [29u16, 30, 31, 32].into_iter().enumerate() {
                s4_sim::create::add_settlers_around(w, spot.0 as i32 + 3 * k as i32, spot.1 as i32, local, kind, 6);
            }
        }));
        g.sim.step(2);
        jump(&mut g, &mut gpu, spot);
        g.sim.set_speed(0);
        let pop = g.cur.population.clone();
        let near: Vec<_> = pop.settlers.iter().filter(|u| u.owner == local && (29..=34).contains(&u.kind) && s4_sim::spiral::grid_distance(u.x as i32 - spot.0 as i32, u.y as i32 - spot.1 as i32) <= 12).copied().collect();
        eprintln!("QA3: soldiers near the spot: {:?}", near.iter().map(|u| u.kind).fold(std::collections::BTreeMap::<u16, usize>::new(), |mut m, k| {
            *m.entry(k).or_default() += 1;
            m
        }));
        let kinds_of = |ids: &[u16]| ids.iter().filter_map(|id| pop.settlers.iter().find(|u| u.id == *id).map(|u| u.kind)).collect::<std::collections::BTreeSet<_>>();
        if let Some(target) = near.iter().find(|u| u.kind == 30) {
            let p = node_screen(&g.scene, g.cam.view(), (target.x as usize, target.y as usize));
            let p = (p.0, p.1 - 6.0);
            for (alt, name) in [(false, "2a_shift_click_rank2"), (true, "2b_alt_click_rank2")] {
                g.pointer.shift = !alt;
                g.pointer.alt = alt;
                g.cursor_moved(&mut gpu, p);
                g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Pressed);
                g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Released);
                g.pointer.shift = false;
                g.pointer.alt = false;
                let ids = g.pointer.selection.ids.clone();
                eprintln!("QA3: {name}: {} selected, kinds {:?}", ids.len(), kinds_of(&ids));
                g.redraw(&mut gpu, p, None);
                shot(&mut gpu, g.cam.view(), name);
            }
        } else {
            eprintln!("QA3: no rank-2 swordsman near the spot");
        }

        // 3. 300 swordsmen selected and sent.
        let camp = (st.0 + 30, st.1 - 20);
        g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
            s4_sim::create::add_settlers_around(w, camp.0 as i32, camp.1 as i32, local, 29, 300);
        }));
        g.sim.step(2);
        g.redraw(&mut gpu, centre, None);
        let pop = g.cur.population.clone();
        let army: Vec<u16> = pop.settlers.iter().filter(|u| u.owner == local && u.kind == 29 && s4_sim::spiral::grid_distance(u.x as i32 - camp.0 as i32, u.y as i32 - camp.1 as i32) <= 25).map(|u| u.id).collect();
        client::select_ids(&mut gpu, &pop, &mut g.pointer.selection, army.clone());
        jump(&mut g, &mut gpu, camp);
        shot(&mut gpu, g.cam.view(), "3a_army_selected");
        // The target: land near the player's start (the camera there, the node clicked).
        let goal = (st.0 + 4, st.1 + 6);
        jump(&mut g, &mut gpu, goal);
        let target = node_screen(&g.scene, g.cam.view(), goal);
        g.cursor_moved(&mut gpu, target);
        g.mouse(&mut gpu, target, MouseButton::Right, ElementState::Pressed);
        g.mouse(&mut gpu, target, MouseButton::Right, ElementState::Released);
        let to = client::hover_node(&mut gpu, &g.scene, g.cam.view(), (target.0 as f32, target.1 as f32));
        eprintln!("QA3: {} selected of {} put, sent to {to:?}", g.pointer.selection.ids.len(), army.len());
        g.sim.step(900);
        g.redraw(&mut gpu, centre, None);
        if let Some(to) = to {
            jump(&mut g, &mut gpu, to);
            let pop = g.cur.population.clone();
            let pos: Vec<(u16, u16)> = army.iter().filter_map(|id| pop.settlers.iter().find(|u| u.id == *id).map(|u| (u.x, u.y))).collect();
            let distinct: std::collections::HashSet<_> = pos.iter().collect();
            let far = pos.iter().map(|p| s4_sim::spiral::grid_distance(p.0 as i32 - to.0 as i32, p.1 as i32 - to.1 as i32)).max().unwrap_or(0);
            let arrived = pos.iter().filter(|p| s4_sim::spiral::grid_distance(p.0 as i32 - to.0 as i32, p.1 as i32 - to.1 as i32) <= 15).count();
            let ground = |n: (u16, u16)| pop.ground.get(n.1 as usize * pop.size + n.0 as usize).copied().unwrap_or(0);
            eprintln!("QA3: after 900 ticks: {} soldiers on {} nodes, {arrived} within 15 of the target (ground {}), farthest {far}", pos.len(), distinct.len(), ground((to.0 as u16, to.1 as u16)));
            shot(&mut gpu, g.cam.view(), "3b_army_formation");
        }

        // 4. The wind on a forest: the densest tree area near the start, at two zooms.
        let pop = g.cur.population.clone();
        let trees: Vec<(u16, u16)> = pop.decos.iter().filter(|d| (1..=22).contains(&d.id)).map(|d| (d.x, d.y)).collect();
        let mut best = (0usize, st);
        for &(x, y) in trees.iter().step_by(7) {
            let n = trees.iter().filter(|t| (t.0 as i32 - x as i32).abs() < 8 && (t.1 as i32 - y as i32).abs() < 8).count();
            if n > best.0 {
                best = (n, (x as usize, y as usize));
            }
        }
        eprintln!("QA3: forest at {:?} ({} trees in 16×16)", best.1, best.0);
        g.pointer.selection.ids.clear();
        jump(&mut g, &mut gpu, best.1);
        for (zoom, units) in [("out", -1000), ("in", 1000)] {
            g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), units, size), &screen(&gpu, size));
            g.redraw(&mut gpu, centre, None);
            let mut shots = Vec::new();
            for k in 0..3 {
                g.sim.step(3);
                g.redraw(&mut gpu, centre, None);
                shots.push(shot(&mut gpu, g.cam.view(), &format!("4_wind_zoom_{zoom}_{k}")));
            }
            let diff = |a: &[u8], b: &[u8]| a.chunks(4).zip(b.chunks(4)).filter(|(p, q)| p[..3] != q[..3]).count();
            if shots.iter().all(|s| !s.is_empty()) {
                eprintln!("QA3: wind zoom {zoom}: changed px between captures {} and {}", diff(&shots[0], &shots[1]), diff(&shots[1], &shots[2]));
            }
        }
        drop(g);

        // 5. Tower bowmen in the user's siege.
        let save = progress::path().and_then(|p| p.parent().map(|d| d.join("saves").join("towerbug.s4rs")));
        if let Some(save) = save.filter(|p| save::read_header(p).is_ok_and(|h| save::compatibility(&h, save::current_rules()).is_ok())) {
            let map = save::read_header(&save).unwrap().map;
            let args = Args { load: Some(save.to_string_lossy().into_owned()), no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start(&map, &args, 0, &mut gpu, None).unwrap();
            g.progress_file = None;
            g.sim.set_speed(0);
            let mut found = None;
            let towers = g.cur.population.buildings.iter().filter(|b| b.inhabitants.iter().any(|i| (32..=34).contains(&i.kind))).count();
            eprintln!("QA3: towerbug: {towers} buildings with bowmen");
            for _ in 0..2000 {
                g.sim.step(1);
                g.redraw(&mut gpu, centre, None);
                found = g.cur.population.buildings.iter().find_map(|b| b.inhabitants.iter().filter_map(|i| i.unit).find(|u| (32..=34).contains(&u.kind) && u.task == 16).map(|u| (b.id, b.x, b.y, u)));
                if found.is_some() {
                    break;
                }
            }
            match found {
                Some((b, x, y, u)) => {
                    eprintln!("QA3: towerbug: building {b} at ({x},{y}): bowman {} shooting at tick {} (job {} dir {} cycle {})", u.id, g.cur.tick, u.job, u.dir, u.cycle);
                    g.cam.jump(view_on(&g.scene, Some((x as usize, y as usize)), (w, h), g.sim.local), &screen(&gpu, g.scene.source.w));
                    g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), 20, g.scene.source.w), &screen(&gpu, g.scene.source.w));
                    for k in 0..4 {
                        g.redraw(&mut gpu, centre, None);
                        shot(&mut gpu, g.cam.view(), &format!("5_siege_bowman_{k}"));
                        g.sim.step(2);
                    }
                }
                None => eprintln!("QA3: towerbug: no tower bowman shot within 2000 ticks"),
            }
        } else {
            eprintln!("QA3: no towerbug.s4rs");
        }
        let _ = std::fs::remove_dir_all(&saves);
    }

    /// Playtest B2: a tower bowman animates when it shoots. On the first map with a tower of
    /// the local player holding a bowman (roman01..), enemy swordsmen are put 7 nodes from the
    /// tower; once the bowman's tower logic takes its shot (task WORK 16, anim list 1, an arrow
    /// launched), the garrison soldier carries the simulation's job, frame and direction into the
    /// population, and the drawn frame (the scene's pose) advances over the next ticks. Captures
    /// four quarters of six ticks around the tower into `out/viewer/b2/`.
    #[test]
    fn tower_bowman_animates_when_it_shoots() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        gpu.set_offscreen(true);
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/b2");
        let _ = std::fs::create_dir_all(&out);
        let mut found = None;
        for map in ["Campaign/roman01", "Campaign/roman02", "Campaign/viking01", "Campaign/maya01"] {
            let args = Args { no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
            let g = Game::start(map, &args, 0, &mut gpu, None).unwrap();
            let pop = g.cur.population.clone();
            let t = pop.buildings.iter().find(|b| b.owner == g.sim.local && b.inhabitants.iter().any(|i| (32..=34).contains(&i.kind) && i.unit.is_some()));
            if let Some(t) = t {
                found = Some((g, t.id, (t.x, t.y)));
                break;
            }
        }
        let Some((mut g, tower, (tx, ty))) = found else { panic!("no tower with a bowman on the maps tried") };
        let local = g.sim.local;
        let bowman = |g: &Game| -> Option<s4_viewer::population::Unit> {
            g.cur.population.buildings.iter().find(|b| b.id == tower)?.inhabitants.iter().filter_map(|i| i.unit).find(|u| (32..=34).contains(&u.kind))
        };
        let enemy = if local == 1 { 2 } else { 1 };
        g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
            s4_sim::create::add_settlers_around(w, tx as i32 + 7, ty as i32 + 5, enemy, 29, 4);
        }));
        g.cam.jump(view_on(&g.scene, Some((tx as usize, ty as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        // Step until the bowman shoots.
        let mut shot_at = None;
        for _ in 0..600 {
            g.sim.step(1);
            g.redraw(&mut gpu, (640.0, 512.0), None);
            if bowman(&g).is_some_and(|u| u.task == 16) {
                shot_at = Some(g.cur.tick);
                break;
            }
        }
        let shot_at = shot_at.expect("the tower bowman shot within 600 ticks");
        let u0 = bowman(&g).unwrap();
        eprintln!("B2: tower {tower} at ({tx},{ty}), bowman {} kind {} shoots at tick {shot_at}: job {} frame {} dir {} cycle {} role {}", u0.id, u0.kind, u0.job, u0.frame, u0.dir, u0.cycle, u0.role);
        assert!(u0.cycle > 1, "the shot's anim list has frames: {u0:?}");
        // The drawn frames over the next ticks, four quarters each.
        let mut frames = Vec::new();
        for t in 0..6u32 {
            for q in 0..4u32 {
                g.alpha = Some(q as f32 / 4.0);
                g.redraw(&mut gpu, (640.0, 512.0), None);
                let pose = g.scene.animator.poses(&g.cur.population, g.cur.tick as f64 + q as f64 / 4.0);
                let f = pose.get(&u0.id).and_then(|p| p.frame);
                frames.push((g.cur.tick, f));
                if let Ok(rgba) = gpu.capture(g.cam.view()) {
                    let _ = client::write_png(&out.join(format!("shot_t{t}_q{q}.png")), w, h, &rgba);
                }
            }
            g.sim.step(1);
        }
        eprintln!("B2: drawn frames {frames:?}");
        let distinct: std::collections::BTreeSet<_> = frames.iter().filter_map(|f| f.1).collect();
        assert!(distinct.len() >= 3, "the bowman's frame advances while it shoots: {frames:?}");
    }

    /// Playtest P2: soldiers of another race in a captured military building keep their race
    /// and their slot. In the user's `afterwin.s4rs` / `towerbug.s4rs` (when present): every
    /// garrison soldier's race in the population is its entity's, its slot the simulation's;
    /// the buildings with soldiers of another race than theirs are listed and captured
    /// (`out/viewer/p2/`).
    #[test]
    fn garrison_soldiers_keep_their_race() {
        let dir = progress::path().and_then(|p| p.parent().map(|d| d.join("saves")));
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/p2");
        let _ = std::fs::create_dir_all(&out);
        for name in ["afterwin.s4rs", "towerbug.s4rs"] {
            let Some(save) = dir.as_ref().map(|d| d.join(name)).filter(|p| save::read_header(p).is_ok_and(|h| save::compatibility(&h, save::current_rules()).is_ok())) else { continue };
            let map = save::read_header(&save).unwrap().map;
            let args = Args { load: Some(save.to_string_lossy().into_owned()), no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start(&map, &args, 0, &mut gpu, None).unwrap();
            g.sim.set_speed(0);
            g.sim.step(1);
            g.redraw(&mut gpu, (640.0, 512.0), None);
            let pop = g.cur.population.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
                let mut v = Vec::new();
                for (id, e) in w.entities.iter() {
                    for (k, &s) in s4_sim::buildings::military::slots(w, id).iter().enumerate() {
                        if let Some(se) = (s != 0).then(|| w.entities.by_index(s as u32)).flatten() {
                            v.push((id, e.race, k as u8, se.kind, se.race));
                        }
                    }
                }
                let _ = tx.send(v);
            }));
            g.sim.step(1);
            let truth = rx.recv().unwrap();
            let mut mixed = Vec::new();
            for &(id, brace, k, kind, race) in &truth {
                let b = pop.buildings.iter().find(|b| b.id == id).expect("the building in the population");
                assert!(b.inhabitants.iter().any(|i| (i.kind, i.race, i.slot) == (kind, race, Some(k))), "{name}: building {id} slot {k}: {:?}", b.inhabitants);
                if race != brace && !mixed.contains(&id) {
                    mixed.push(id);
                }
            }
            eprintln!("{name}: {} garrison soldiers, buildings with soldiers of another race: {mixed:?}", truth.len());
            for (i, id) in mixed.iter().take(3).enumerate() {
                let b = pop.buildings.iter().find(|b| b.id == *id).unwrap();
                eprintln!("  building {id} type {} race {} owner {} at ({},{}): {:?}", b.kind, b.race, b.owner, b.x, b.y, b.inhabitants);
                let v = view_on(&g.scene, Some((b.x as usize, b.y as usize)), (1280, 1024), g.sim.local);
                let size = g.scene.source.w;
                g.cam.jump(v, &screen(&gpu, size));
                zoom_camera(&mut g.cam, &mut gpu, 20, Some((640.0, 512.0)), size);
                g.cam.settle(&screen(&gpu, size));
                g.redraw(&mut gpu, (640.0, 512.0), None);
                if let Ok(rgba) = gpu.capture(g.cam.view()) {
                    let _ = client::write_png(&out.join(format!("{}_{i}.png", name.trim_end_matches(".s4rs"))), 1280, 1024, &rgba);
                }
            }
        }
    }

    /// Bug B4 (a black square on Roman geologists): six Roman geologists sent to the mountain
    /// nearest the player on roman01, stepped and drawn tick by tick for `B4_TICKS` (default
    /// 4000). Every frame is searched for a solid black 12×12 square; a hit prints the sprite
    /// instances under it with their images and the geologists' state. The cause was effect 0
    /// (the rich sign's sound, drawn as file 6 job 0, a 60×60 black block; 3 hits in this run
    /// before the fix). Asserts no block and no opaque black image in the audit; prints the drawn
    /// (task, gfx job) counts; writes `out/viewer/b4/cycle.png`, the first geologist every 3rd
    /// tick through searching, hammering and placing signs.
    #[test]
    #[ignore]
    fn b4_geologist_jobs() {
        let (w, h) = (640usize, 480usize);
        let mut gpu = Gpu::headless(w as u32, h as u32).unwrap();
        gpu.set_offscreen(true);
        let ticks: u32 = std::env::var("B4_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(4000);
        let args = Args { no_fog: true, delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let home = g.cur.population.buildings.iter().find(|b| b.owner == local).map(|b| (b.x as i32, b.y as i32)).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(Box::new(move |w: &mut s4_sim::World| {
            let size = w.grid.size() as i32;
            let d2 = |x: i32, y: i32| (x - home.0).pow(2) + (y - home.1).pow(2);
            let mountain = (0..size * size).map(|n| (n % size, n / size)).filter(|&(x, y)| w.layers.ground[(y * size + x) as usize] & 0xf0 == 0x20).min_by_key(|&(x, y)| d2(x, y)).unwrap();
            let before: Vec<u16> = w.entities.iter().map(|(id, _)| id).collect();
            s4_sim::create::add_settlers_around(w, mountain.0, mountain.1, local, 49, 6);
            let ids: Vec<u16> = w.entities.iter().map(|(id, _)| id).filter(|id| !before.contains(id)).collect();
            s4_sim::group::send_to(w, &ids, mountain.0, mountain.1, 0);
            let _ = tx.send((mountain, ids));
        }));
        g.sim.step(1);
        let (mountain, ids) = rx.recv().unwrap();
        eprintln!("B4: geologists {ids:?} sent to the mountain at {mountain:?}");
        g.cam.jump(view_on(&g.scene, Some((mountain.0 as usize - 4, mountain.1 as usize + 6)), (w as u32, h as u32), local), &screen(&gpu, g.scene.source.w));
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/b4");
        let _ = std::fs::create_dir_all(&out);
        s4_viewer::audit::take();
        s4_viewer::audit::enable(true);
        let mut seen: std::collections::BTreeMap<(u8, Option<u16>), u32> = Default::default();
        let mut blocks = 0;
        // The cycle sheet: 40 crops of 64×72 around the first geologist.
        let (cw, ch, cols) = (64usize, 72usize, 10usize);
        let mut crops: Vec<Vec<u8>> = Vec::new();
        let mut cycle_from = None;
        for _ in 0..ticks {
            g.sim.step(1);
            g.redraw(&mut gpu, (320.0, 240.0), None);
            let Ok(rgba) = gpu.capture(g.cam.view()) else { continue };
            // A solid black 12×12 square anywhere on the screen.
            let dark = |x: usize, y: usize| rgba[(y * w + x) * 4..][..3].iter().map(|&c| c as u32).sum::<u32>() < 12;
            let hit = (0..h - 12).step_by(6).flat_map(|y| (0..w - 12).step_by(6).map(move |x| (x, y))).find(|&(x, y)| (0..12).all(|dy| (0..12).all(|dx| dark(x + dx, y + dy))));
            if let Some(at) = hit {
                blocks += 1;
                if blocks <= 8 {
                    eprintln!("B4: black block at {at:?} on tick {}", g.cur.tick);
                    let v = g.cam.view();
                    let (px, py) = (at.0 as f32 + 6.0, at.1 as f32 + 6.0);
                    let sp = g.scene.sprites.as_mut().unwrap();
                    let under: Vec<s4_viewer::sprites::SpriteInstance> = sp
                        .instances()
                        .iter()
                        .filter(|i| {
                            let (x0, y0) = ((i.pos[0] - v.x) * v.zoom, (i.pos[1] - v.y) * v.zoom);
                            px >= x0 && py >= y0 && px < x0 + i.size[0] * v.zoom && py < y0 + i.size[1] * v.zoom
                        })
                        .copied()
                        .collect();
                    for i in under {
                        eprintln!("B4:   instance {:?} image {:?}", i, sp.slot_key(i.origin, i.layer));
                    }
                    for s in g.cur.population.settlers.iter().filter(|s| s.kind == 49) {
                        eprintln!("B4:   geologist {} at {},{} task {} job {} frame {} dir {}", s.id, s.x, s.y, s.task, s.job, s.frame, s.dir);
                    }
                    let _ = client::write_png(&out.join(format!("block_{}.png", g.cur.tick)), w as u32, h as u32, &rgba);
                }
            }
            for s in g.cur.population.settlers.iter().filter(|s| s.kind == 49 && s.drawn()) {
                *seen.entry((s.task, s.gfx_job())).or_default() += 1;
            }
            let first = g.cur.population.settlers.iter().find(|s| s.id == ids[0]).copied();
            if let Some(s) = first.filter(|s| s.task == 31 || cycle_from.is_some()) {
                let from = *cycle_from.get_or_insert(g.cur.tick);
                if (g.cur.tick - from).is_multiple_of(3) && crops.len() < 40 {
                    let (sx, sy) = node_screen(&g.scene, g.cam.view(), (s.x as usize, s.y as usize));
                    let (x0, y0) = ((sx as i64 - cw as i64 / 2).clamp(0, (w - cw) as i64) as usize, (sy as i64 - 52).clamp(0, (h - ch) as i64) as usize);
                    crops.push((0..ch).flat_map(|y| rgba[((y0 + y) * w + x0) * 4..][..cw * 4].to_vec()).collect());
                }
            }
        }
        s4_viewer::audit::enable(false);
        let misses = s4_viewer::audit::take();
        eprintln!("B4: (task, gfx job) -> drawn ticks: {seen:?}");
        eprintln!("B4: {blocks} frames with a black block; audit {misses:?}");
        let rows = crops.len().div_ceil(cols).max(1);
        let mut sheet = vec![0u8; cols * cw * rows * ch * 4];
        for (k, c) in crops.iter().enumerate() {
            let (ox, oy) = ((k % cols) * cw, (k / cols) * ch);
            for y in 0..ch {
                let o = ((oy + y) * cols * cw + ox) * 4;
                sheet[o..o + cw * 4].copy_from_slice(&c[y * cw * 4..][..cw * 4]);
            }
        }
        let _ = client::write_png(&out.join("cycle.png"), (cols * cw) as u32, (rows * ch) as u32, &sheet);
        assert_eq!(blocks, 0, "a solid black block was drawn");
        assert!(!misses.keys().any(|k| k.starts_with("opaque black")), "{misses:?}");
        assert!(seen.keys().any(|k| k.0 == 16) && seen.keys().any(|k| k.0 == 31), "the geologists searched and worked: {seen:?}");
    }

    /// Diagnosis of playtest P7 (soldiers drawn walking towards a node, then snapped back): the
    /// user's `towerbug.s4rs` (dark01, a siege) stepped tick by tick, each tick drawn at four
    /// quarters through the animator as `present` does; every settler whose drawn position jumps
    /// by more than 6 px between two consecutive frames is reported with its last snapshots
    /// (node, direction, distance, task, walk, counter, job). Prints; `out/viewer/p7/trace.txt`.
    #[test]
    #[ignore]
    fn p7_snapping_soldiers() {
        use std::fmt::Write as _;
        let save = progress::path().and_then(|p| p.parent().map(|d| d.join("saves").join(std::env::var("P7_SAVE").unwrap_or("towerbug.s4rs".into()))));
        let Some(save) = save.filter(|p| p.exists()) else { return };
        let ticks: u32 = std::env::var("P7_TICKS").ok().and_then(|t| t.parse().ok()).unwrap_or(600);
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let map = save::read_header(&save).unwrap().map;
        let args = Args { load: Some(save.to_string_lossy().into_owned()), delay: 0, hash_interval: 64, ..Default::default() };
        let g = Game::start(&map, &args, 0, &mut gpu, None).unwrap();
        g.sim.set_speed(0);
        let mut anim = s4_viewer::animate::Animator::default();
        let mut hist: HashMap<u16, std::collections::VecDeque<String>> = HashMap::new();
        let mut last: HashMap<u16, (f32, f32)> = HashMap::new();
        let mut trace = String::new();
        let mut jumps = 0;
        let mut reverts = 0;
        let mut nodes: HashMap<u16, std::collections::VecDeque<(u16, u16)>> = HashMap::new();
        for i in 0..ticks {
            g.sim.step(1);
            let snap = g.sim.snapshot();
            let pop = &snap.population;
            let h = |x: u16, y: u16| pop.height.get(y as usize * pop.size + x as usize).copied().unwrap_or(0);
            for u in pop.settlers.iter().filter(|u| u.id != 0 && u.drawn()) {
                let e = hist.entry(u.id).or_default();
                e.push_back(format!("t{} node ({},{}) dir {} dist {} task {} walk {:#x} counter {} job {} role {} flags {:#x} kind {}", pop.tick, u.x, u.y, u.dir, u.distance, u.task, u.walk, u.counter, u.job, u.role, u.flags, u.kind));
                if e.len() > 6 && std::env::var("P7_ID").ok().and_then(|v| v.parse::<u16>().ok()) != Some(u.id) {
                    e.pop_front();
                }
                // A node left and returned to within three ticks: a reverted step.
                let n = nodes.entry(u.id).or_default();
                if n.back() != Some(&(u.x, u.y)) {
                    if n.len() >= 2 && n[n.len() - 2] == (u.x, u.y) && i > 20 && reverts < 40 {
                        reverts += 1;
                        let _ = writeln!(trace, "settler {} kind {} back at ({},{}):", u.id, u.kind, u.x, u.y);
                        for l in &hist[&u.id] {
                            let _ = writeln!(trace, "    {l}");
                        }
                    }
                    n.push_back((u.x, u.y));
                    if n.len() > 3 {
                        n.pop_front();
                    }
                }
            }
            // Not drawn (inside a building): the next appearance is no jump.
            let shown: std::collections::HashSet<u16> = pop.settlers.iter().filter(|u| u.id != 0 && u.drawn()).map(|u| u.id).collect();
            last.retain(|id, _| shown.contains(id));
            for q in 0..4 {
                let t = pop.tick as f64 + q as f64 / 4.0;
                let poses = anim.poses(pop, t);
                for u in pop.settlers.iter().filter(|u| u.id != 0 && u.drawn()) {
                    let a = s4_viewer::terrain::node_pos(u.x as usize, u.y as usize, h(u.x, u.y));
                    let p = poses.get(&u.id).copied().unwrap_or_default();
                    let at = (a[0] + p.dx, a[1] + p.dy);
                    if std::env::var("P7_ID").ok().and_then(|v| v.parse::<u16>().ok()) == Some(u.id) {
                        let _ = writeln!(trace, "pose {} t {t:.2}: node ({},{}) pose {:.2} {:.2} at {:.1} {:.1}", u.id, u.x, u.y, p.dx, p.dy, at.0, at.1);
                    }
                    if let Some(&b) = last.get(&u.id) {
                        let d = ((at.0 - b.0).powi(2) + (at.1 - b.1).powi(2)).sqrt();
                        if d > 6.0 && i > 20 && jumps < 60 {
                            jumps += 1;
                            let _ = writeln!(trace, "settler {} kind {} jumps {d:.1} px at t {t:.2} (pose {:.1} {:.1}):", u.id, u.kind, p.dx, p.dy);
                            for l in &hist[&u.id] {
                                let _ = writeln!(trace, "    {l}");
                            }
                        }
                    }
                    last.insert(u.id, at);
                }
            }
        }
        let _ = writeln!(trace, "{jumps} jumps over 6 px, {reverts} reverted steps in {ticks} ticks");
        if let Some(id) = std::env::var("P7_ID").ok().and_then(|v| v.parse::<u16>().ok()) {
            for l in hist.get(&id).into_iter().flatten() {
                let _ = writeln!(trace, "{id}: {l}");
            }
        }
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/p7");
        let _ = std::fs::create_dir_all(&out);
        let _ = std::fs::write(out.join("trace.txt"), &trace);
        eprintln!("{trace}");
    }

    /// User playtest P6: after a win the whole map shows (event 0x37 → `CFogging::SetRWM(1)`,
    /// 0x51744e): roman01 won through "!win" has every node visible a few fog cycles later; a
    /// lost single-player game keeps its fog (0x5174ab: only in a network game). The user's own
    /// save after a win (`afterwin.s4rs`, when present and loadable) shows everything too
    /// (the load's 0x513851).
    #[test]
    fn the_map_is_revealed_after_a_win() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let hidden = |g: &Game| {
            let fog = g.cur.fog.as_ref().expect("the local player's fog");
            let n = g.scene.source.w;
            (0..n * n).filter(|&i| !fog.is_position_visible(n, (i % n) as i32, (i / n) as i32)).count()
        };
        // Steps and frames: the fog's cycles run with the ticks.
        let run = |g: &mut Game, gpu: &mut Gpu, ticks: u32| {
            for _ in 0..ticks / 20 {
                g.sim.step(20);
                g.redraw(gpu, (640.0, 512.0), None);
            }
        };
        for outcome in [script::WON, script::LOST] {
            let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
            g.progress_file = None;
            run(&mut g, &mut gpu, 200);
            let before = hidden(&g);
            assert!(before > 0, "roman01 starts fogged");
            g.sim.outcome.store(outcome, std::sync::atomic::Ordering::Relaxed);
            run(&mut g, &mut gpu, 1000);
            let after = hidden(&g);
            eprintln!("outcome {outcome}: hidden nodes {before} -> {after}");
            if outcome == script::WON {
                assert_eq!(after, 0, "won: every node visible");
            } else {
                assert!(after > 0, "lost in single player: the fog stays");
            }
        }
        let save = progress::path().and_then(|p| p.parent().map(|d| d.join("saves").join("afterwin.s4rs")));
        if let Some(save) = save.filter(|p| save::read_header(p).is_ok_and(|h| save::compatibility(&h, save::current_rules()).is_ok())) {
            let map = save::read_header(&save).unwrap().map;
            let args = Args { load: Some(save.to_string_lossy().into_owned()), delay: 0, hash_interval: 64, ..Default::default() };
            let mut g = Game::start(&map, &args, 0, &mut gpu, None).unwrap();
            g.progress_file = None;
            g.sim.set_speed(0);
            run(&mut g, &mut gpu, 1000);
            eprintln!("afterwin.s4rs: outcome {}, hidden nodes {}", g.decided, hidden(&g));
            let size = g.scene.source.w;
            g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), -1000, size), &screen(&gpu, size));
            g.redraw(&mut gpu, (640.0, 512.0), None);
            if let Ok(rgba) = gpu.capture(g.cam.view()) {
                let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/reveal");
                let _ = std::fs::create_dir_all(&out);
                let _ = client::write_png(&out.join("afterwin.png"), 1280, 1024, &rgba);
            }
            if g.decided == script::WON {
                assert_eq!(hidden(&g), 0, "afterwin.s4rs: every node visible");
            }
        }
    }

    /// The HE zoom slider (dialog 0: area 612, knob 0x265): the game start shows the zoom on it
    /// (`CStateGame` 0x513190, event 0x25d: 17 for 0x180000); a click on the area jumps the knob
    /// and sets the zoom (0x49b6b0 → event 0x12 → `EventManagement` case 0x12); a zoom step moves
    /// the knob (0x519870 → 0x5199a0 → 0x25d).
    #[test]
    fn zoom_slider_shows_and_sets_the_zoom() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let knob = |gpu: &Gpu| gpu.ui.as_ref().and_then(|u| u.state.knob_value(0, 0x265)).unwrap() as i32;
        assert_eq!(client::camera_zoom(&gpu, g.cam.view()), 0x180000);
        assert_eq!(knob(&gpu), 17);
        let (x, y) = client::control_point(&gpu, 0, 612).unwrap();
        let p = (x + 40.0 * gpu.gui_scale as f64, y);
        g.cursor_moved(&mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Pressed);
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Released);
        let v = knob(&gpu);
        assert!(v > 60, "the knob jumped right: {v}");
        // The camera eases to the slider's zoom.
        for _ in 0..4 {
            g.last = Instant::now() - std::time::Duration::from_millis(250);
            g.redraw(&mut gpu, p, None);
        }
        assert!((client::camera_zoom(&gpu, g.cam.view()) - client::zoom_of_slider(v)).abs() <= 1, "zoom {:#x} for {v}", client::camera_zoom(&gpu, g.cam.view()));
        // A zoom key moves the knob to the wanted zoom.
        let size = g.scene.source.w;
        zoom_camera(&mut g.cam, &mut gpu, -5, None, size);
        assert_eq!(knob(&gpu) as u32, client::slider_of_zoom((g.cam.zoom_target() * 24.0 * 65536.0).round() as i32));
        assert!(knob(&gpu) < v);
    }

    /// The minimap as the release drives it:
    /// * a left press on it jumps the camera (EventManagement case 7, +0xd4), moves with the
    ///   button held do nothing (case 5 skips everything while +0xd4 is set), the release ends it;
    /// * the view rectangle (0x665940) follows the camera;
    /// * a ground change (path wear) shows within 64 rendered frames, block by block (0x664f80 /
    ///   0x665ce0), not at once. Captures `out/viewer/minimap/{before,after}.png`.
    #[test]
    fn minimap_click_rectangle_and_updates() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let scale = gpu.gui_scale as f64;
        let size = g.scene.source.w;
        g.redraw(&mut gpu, (640.0, 512.0), None);
        // Left press on the minimap point (u, v) = (60, 70).
        let (u, v) = (60i32, 70i32);
        let p = ((15 + u + (160 - v) / 2) as f64 * scale + 0.5, (8 + v) as f64 * scale + 0.5);
        assert_eq!(client::minimap_point(&gpu, p), Some((u, v)));
        let want = client::view_from_minimap(&gpu, g.cam.view(), (u, v), size);
        g.cursor_moved(&mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Left, ElementState::Pressed);
        assert_eq!((g.cam.view().x, g.cam.view().y), (want.x, want.y), "the press jumps the camera");
        let q = (p.0 + 30.0, p.1 + 12.0);
        g.cursor_moved(&mut gpu, q);
        g.redraw(&mut gpu, q, None);
        assert_eq!((g.cam.view().x, g.cam.view().y), (want.x, want.y), "a move with the button held does not scroll");
        g.mouse(&mut gpu, q, MouseButton::Left, ElementState::Released);
        // The rectangle at the camera's top-left in nodes.
        let ((vx, vy), rect) = client::minimap_view(&gpu, g.cam.view(), size);
        let shear = 160.0 / size as f32;
        let (ry, rx) = ((vy as f32 * shear) as i32, (vx as f32 * shear) as i32);
        let img = g.scene.minimap((vx, vy), rect, 0, g.sim.local).unwrap();
        let white = |x: i32, y: i32| {
            let o = ((y as u32 * s4_viewer::minimap::WIDTH + x as u32) * 4) as usize;
            img[o..o + 4] == [255, 255, 255, 255]
        };
        assert!(white(80 + rx + 1, ry), "the rectangle's top edge at ({vx}, {vy}) nodes");
        assert!(rect.0 > 0 && rect.1 > 0, "{rect:?}");
        // A block worn to road at the camera's node.
        let (cx, cy) = client::camera_node(&gpu, g.cam.view(), size).unwrap();
        let block: Vec<usize> = (0..12).flat_map(|dy| (0..12).map(move |dx| (cy + dy) * size + cx + dx)).collect();
        let b2 = block.clone();
        g.sim.with_world(move |w| {
            for &n in &b2 {
                w.layers.ground[n] = 0x1d;
            }
        });
        g.sim.step(1);
        let (mv, mr) = client::minimap_view(&gpu, g.cam.view(), size);
        // The landscape pixels whose sample (0x664f80: map row `(int)(i * k + 0.5)`, column
        // likewise) lies in the block.
        let k = size as f32 / 160.0;
        let inside = |r: usize, c: usize| (cy..cy + 12).contains(&r) && (cx..cx + 12).contains(&c);
        let pixels: Vec<(i32, i32)> = (0..160usize)
            .flat_map(|i| (0..160usize).map(move |j| (i, j)))
            .filter(|&(i, j)| inside((i as f32 * k + 0.5) as usize, (j as f32 * k + 0.5) as usize))
            .map(|(i, j)| (80 - i as i32 / 2 + j as i32, i as i32))
            .collect();
        assert!(!pixels.is_empty());
        let at = |img: &[u8], (x, y): (i32, i32)| {
            let o = ((y as u32 * s4_viewer::minimap::WIDTH + x as u32) * 4) as usize;
            [img[o], img[o + 1], img[o + 2]]
        };
        let base = g.scene.minimap(mv, mr, 0, g.sim.local).unwrap();
        let old: Vec<[u8; 3]> = pixels.iter().map(|&q| at(&base, q)).collect();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/minimap");
        let _ = std::fs::create_dir_all(&out);
        let _ = client::write_png(&out.join("before.png"), s4_viewer::minimap::WIDTH, s4_viewer::minimap::HEIGHT, &base);
        let uniq: Vec<((i32, i32), [u8; 3])> = pixels.iter().zip(&old).map(|(q, o)| (*q, *o)).collect();
        let changed = |img: &[u8]| uniq.iter().filter(|(q, o)| at(img, *q) != *o).count();
        let mut counts = Vec::new();
        for f in 1..=128 {
            g.redraw(&mut gpu, (640.0, 512.0), None);
            let img = g.scene.minimap(mv, mr, 0, g.sim.local).unwrap();
            counts.push(changed(&img));
            if f == 64 {
                let _ = client::write_png(&out.join("after.png"), s4_viewer::minimap::WIDTH, s4_viewer::minimap::HEIGHT, &img);
            }
        }
        let full = counts[63];
        assert_eq!(full, uniq.len(), "the worn block shows after 64 frames");
        assert_eq!(counts[127], full, "complete after one round of 64 frames");
        assert!(counts[0] < full && counts.windows(2).all(|w| w[0] <= w[1]), "block by block: {counts:?}");
    }

    /// The minimap's object groups through the toggles on dialog 0 (0x49b6b0 → `MinimapViewFlags`
    /// → event 0x6a → 0x59a9a0): buildings (614) in `MINIMAP_COLORS`' "other" colour, sectors
    /// (617) in the owners' colours; they appear within 64 frames and go again within a round
    /// after the toggle is switched off. Captures `out/viewer/minimap/groups_*.png`.
    #[test]
    fn minimap_object_groups_by_the_toggles() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let size = g.scene.source.w;
        let local = g.sim.local;
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert_eq!(g.scene.minimap_layer.flags, 0, "every group off at the start");
        let other = g.scene.minimap_layer.colors[3];
        let toggle = |g: &mut Game, gpu: &mut Gpu, link: u16| {
            let p = client::control_point(gpu, 0, link).unwrap();
            g.cursor_moved(gpu, p);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Pressed);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Released);
        };
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/minimap");
        let _ = std::fs::create_dir_all(&out);
        let frames = |g: &mut Game, gpu: &mut Gpu, n: u32, name: &str| -> Vec<u8> {
            for _ in 0..n {
                g.redraw(gpu, (640.0, 512.0), None);
            }
            let (v, r) = client::minimap_view(gpu, g.cam.view(), size);
            let img = g.scene.minimap(v, r, 0, local).unwrap();
            let _ = client::write_png(&out.join(format!("groups_{name}.png")), s4_viewer::minimap::WIDTH, s4_viewer::minimap::HEIGHT, &img);
            img
        };
        // The colour as stored (RGB565, expanded).
        let expand = |c: [u8; 3]| {
            let (r5, g6, b5) = ((c[0] as f32 * 0.121_568_63) as u32, (c[1] as f32 * 0.247_058_82) as u32, (c[2] as f32 * 0.121_568_63) as u32);
            [(r5 << 3 | r5 >> 2) as u8, (g6 << 2 | g6 >> 4) as u8, (b5 << 3 | b5 >> 2) as u8]
        };
        let count = |img: &[u8], c: [u8; 3]| img.chunks(4).filter(|p| p[..3] == c && p[3] == 255).count();
        let none = frames(&mut g, &mut gpu, 64, "none");
        assert_eq!(count(&none, expand(other)), 0);
        toggle(&mut g, &mut gpu, 614);
        assert_eq!(g.scene.minimap_layer.flags, 0x1, "614: buildings");
        let b = frames(&mut g, &mut gpu, 64, "buildings");
        let n_b = count(&b, expand(other));
        assert!(n_b > 10, "buildings shown: {n_b}");
        toggle(&mut g, &mut gpu, 617);
        assert_eq!(g.scene.minimap_layer.flags, 0x9, "617: sectors");
        let s = frames(&mut g, &mut gpu, 64, "buildings_sectors");
        let p1 = expand(s4_viewer::sprites::PLAYER_COLORS[0]);
        assert!(count(&s, p1) > 200, "player 1's sector filled: {}", count(&s, p1));
        toggle(&mut g, &mut gpu, 614);
        toggle(&mut g, &mut gpu, 617);
        assert_eq!(g.scene.minimap_layer.flags, 0);
        let off = frames(&mut g, &mut gpu, 128, "off");
        assert_eq!(count(&off, expand(other)), 0, "the buildings are gone a round later");
    }

    /// EventManagement case 9 on the minimap (0x514ce0): GUI event 0x1e0 with the minimap's node
    /// at once (no release needed): the selected soldiers walk there.
    #[test]
    fn minimap_right_click_sends_the_selection() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let soldier = |u: &s4_viewer::population::Unit| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xf));
        let snap = g.sim.snapshot();
        let first = *snap.population.settlers.iter().find(|u| soldier(u)).expect("roman01: player 1 has soldiers");
        g.cam.jump(view_on(&g.scene, Some((first.x as usize, first.y as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        let frames = |g: &mut Game, gpu: &mut Gpu, p: (f64, f64)| {
            for _ in 0..3 {
                g.redraw(gpu, p, None);
            }
        };
        let c = node_screen(&g.scene, g.cam.view(), (first.x as usize, first.y as usize));
        let (a, b) = ((c.0 - 150.0, c.1 - 150.0), (c.0 + 150.0, c.1 + 150.0));
        g.cursor_moved(&mut gpu, a);
        g.mouse(&mut gpu, a, MouseButton::Left, ElementState::Pressed);
        frames(&mut g, &mut gpu, a);
        g.cursor_moved(&mut gpu, b);
        let scale = gpu.gui_scale as f64;
        g.pointer.moved(&mut gpu, b, scale);
        frames(&mut g, &mut gpu, b);
        g.mouse(&mut gpu, b, MouseButton::Left, ElementState::Released);
        frames(&mut g, &mut gpu, b);
        let ids = g.pointer.selection.ids.clone();
        assert!(!ids.is_empty(), "box selected the soldiers");
        // The minimap point of a node 16 / 12 nodes down-right, and the node it stands for.
        let size = g.scene.source.w as i32;
        let (u, v) = ((first.x as i32 + 16) * 160 / size, (first.y as i32 + 12) * 160 / size);
        let target = ((u * size / 160) as u16, (v * size / 160) as u16);
        let p = ((15 + u + (160 - v) / 2) as f64 * scale + 0.5, (8 + v) as f64 * scale + 0.5);
        assert_eq!(client::minimap_point(&gpu, p), Some((u, v)));
        let before: HashMap<u16, (u16, u16)> = g.sim.snapshot().population.settlers.iter().map(|u| (u.id, (u.x, u.y))).collect();
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Pressed);
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Released);
        // Paused (speed 0): the sim only moves when stepped, so the run is the same every time.
        g.sim.step(400);
        g.redraw(&mut gpu, p, None);
        let after: HashMap<u16, (u16, u16)> = g.sim.snapshot().population.settlers.iter().map(|u| (u.id, (u.x, u.y))).collect();
        let dist = |q: (u16, u16)| (q.0 as i32 - target.0 as i32).abs().max((q.1 as i32 - target.1 as i32).abs());
        for id in &ids {
            let (b, a) = (before[id], after[id]);
            assert!(a != b && dist(a) <= 6, "soldier {id} walked to the minimap's {target:?}: {b:?} → {a:?}");
        }
    }

    /// User report: deselecting buildings / clicking away behaved oddly. The original's sequences
    /// through the window path on MCD2_roman1 (the tool smith and a woodcutter, found by position), paused:
    /// - a click on nothing (`DotSelection` 0x4eb3b0, no Ctrl): 0x191 (leave handler 0x4edb10:
    ///   0x260) and 0x25b (0x481000: the build tab 606 clicked unless pressed), also with
    ///   nothing selected and another main tab open; with Ctrl nothing happens;
    /// - a click on a building, also the one already selected: `RequestDialog` 0x4ee730 (0x260
    ///   closes the sidebar, the role fill with 0 → 0x25a creates the dialog anew, so the crush
    ///   confirmation gives way to it);
    /// - a right click with a building selected: `SendTo` 0x4ec960 → `sub_EDC90` (0x25b);
    /// - Esc: 0x489440 clicks the extras tab 611 (its 0x192 deselects) and the quit tab 930.
    #[test]
    fn deselect_like_the_original() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/MCD2_roman1", &args, 0, &mut gpu, None).unwrap();
        g.cam.jump(view_on(&g.scene, Some((190, 280)), (w, h), g.sim.local), &screen(&gpu, g.scene.source.w));
        let click = |g: &mut Game, gpu: &mut Gpu, p: (f64, f64)| {
            g.cursor_moved(gpu, p);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Pressed);
            g.redraw(gpu, p, None);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Released);
            g.redraw(gpu, p, None);
        };
        let at = |g: &Game, node: (usize, usize)| node_screen(&g.scene, g.cam.view(), node);
        let node = |g: &mut Game, gpu: &mut Gpu, n: (usize, usize)| {
            let p = at(g, n);
            click(g, gpu, p);
        };
        let ctl = |g: &mut Game, gpu: &mut Gpu, d: usize, l: u16| {
            let p = client::control_point(gpu, d, l).unwrap();
            click(g, gpu, p);
        };
        let open = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.open.clone();
        let pressed = |gpu: &Gpu, l: u16| {
            let ui = gpu.ui.as_ref().unwrap();
            ui.state.index_of(0, l).is_some_and(|i| ui.state.is_pressed(0, i))
        };
        let build_tab = |gpu: &Gpu| pressed(gpu, 606) && open(gpu).contains(&3) && open(gpu).contains(&9);
        let (smith, wood, empty) = ((190, 289), (187, 259), (205, 280));
        // The building nearest a node (entity ids shift whenever the sim adds entities at load).
        let near = |g: &Game, n: (usize, usize)| {
            g.cur.population.buildings.iter().min_by_key(|b| (b.x as i32 - n.0 as i32).abs() + (b.y as i32 - n.1 as i32).abs()).map(|b| b.id).unwrap()
        };
        // Another main tab, then a click on nothing: back to the build tab.
        ctl(&mut g, &mut gpu, 0, 607);
        assert!(pressed(&gpu, 607) && !build_tab(&gpu));
        node(&mut g, &mut gpu, empty);
        assert!(build_tab(&gpu), "a click on nothing: 0x25b → tab 606: {:?}", open(&gpu));
        // The tool smith and its sidebar (D19 → D47).
        node(&mut g, &mut gpu, smith);
        assert_eq!(g.pointer.selection.ids, vec![near(&g, smith)]);
        ctl(&mut g, &mut gpu, 19, 1138);
        assert!(open(&gpu).contains(&47), "sidebar: {:?}", open(&gpu));
        // The same building again: the sidebar closed, D19 created anew.
        node(&mut g, &mut gpu, smith);
        assert!(open(&gpu).contains(&19) && !open(&gpu).contains(&47), "re-click: {:?}", open(&gpu));
        // Another building; its crush confirmation; the building again: D18 back.
        node(&mut g, &mut gpu, wood);
        assert!(open(&gpu).contains(&18));
        ctl(&mut g, &mut gpu, 18, 1085);
        assert!(open(&gpu).contains(&43));
        node(&mut g, &mut gpu, wood);
        assert!(open(&gpu).contains(&18) && !open(&gpu).contains(&43), "D43 gives way: {:?}", open(&gpu));
        // Ctrl + a click on nothing: nothing changes.
        g.pointer.ctrl = true;
        node(&mut g, &mut gpu, empty);
        g.pointer.ctrl = false;
        assert_eq!(g.pointer.selection.ids, vec![near(&g, wood)]);
        assert!(open(&gpu).contains(&18));
        // A click on nothing: deselected, build tab.
        node(&mut g, &mut gpu, empty);
        assert!(g.pointer.selection.ids.is_empty() && build_tab(&gpu) && !open(&gpu).contains(&18));
        // Right click with the tool smith and its sidebar: `sub_EDC90`.
        node(&mut g, &mut gpu, smith);
        ctl(&mut g, &mut gpu, 19, 1138);
        let p = at(&g, empty);
        g.cursor_moved(&mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Pressed);
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Released);
        g.redraw(&mut gpu, p, None);
        assert!(g.pointer.selection.ids.is_empty() && build_tab(&gpu) && !open(&gpu).contains(&47), "right click: {:?}", open(&gpu));
        // `SendTo` emptied the selection without a transition: still state 1 (0x1ba is dropped).
        assert!(g.bdlg.held_one);
        assert_eq!(input::state(false, false, false, false, g.bdlg.held_one), 1);
        // Esc with the woodcutter selected: the extras tab (deselects) and its submenu.
        node(&mut g, &mut gpu, wood);
        assert!(!g.bdlg.held_one, "a map click ends the held state 1");
        g.key(&mut gpu, KeyCode::Escape, true, false);
        g.redraw(&mut gpu, p, None);
        assert!(g.pointer.selection.ids.is_empty() && pressed(&gpu, 611) && open(&gpu).contains(&8), "Esc: {:?}", open(&gpu));
        // 930 (0x489815): the quit page (34), not the save page.
        assert!(open(&gpu).contains(&34) && !open(&gpu).contains(&33), "Esc page: {:?}", open(&gpu));
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/deselect/esc.png");
        if let Ok(rgba) = gpu.capture(g.cam.view()) {
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, w, h, &rgba);
        }
    }

    /// Jumping fish and ambient creatures through the window path (ambient.rs, owned by the
    /// game since the move out of the sound side): MCD2_roman1, the camera over fish water,
    /// paused and stepped. Every fish starts on a node with more than `FISH_MIN` fish, not dark,
    /// in the cells around the centre's (0x5301c0), lives its job's frames in direction 2 and is
    /// drawn (`Gfx\8.gfx` job 36); butterflies / birds / seagulls are drawn through the deferred
    /// list. Captures `out/viewer/fish/`.
    #[test]
    fn jumping_fish_and_creatures_are_drawn() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/MCD2_roman1", &args, 0, &mut gpu, None).unwrap();
        let snap = g.sim.snapshot();
        let land = snap.ambient.clone().expect("the land");
        let pop = &snap.population;
        let size = pop.size;
        let fishy = |n: usize| pop.resource[n] & 0xf0 == 0 && (pop.resource[n] & 0xf) as i32 > land.fish_min && pop.flags[n] & 4 == 0;
        // The visible fish node with the most fish nodes in its 5×5 cells.
        let fog = snap.fog.clone().expect("fog");
        let cells: Vec<usize> = (0..size * size).filter(|&n| fishy(n) && fog.is_position_visible(size, (n % size) as i32, (n / size) as i32)).collect();
        let n = *cells.iter().max_by_key(|&&n| cells.iter().filter(|&&m| ((m % size) as i32 / 16 - (n % size) as i32 / 16).abs() <= 2 && ((m / size) as i32 / 16 - (n / size) as i32 / 16).abs() <= 2).count()).expect("fish on the map");
        let node = (n % size, n / size);
        g.cam.jump(view_on(&g.scene, Some(node), (w, h), g.sim.local), &screen(&gpu, g.scene.source.w));
        let p = (640.0, 512.0);
        g.redraw(&mut gpu, p, None);
        let centre = sound::centre_node(&snap.population, &sound::Listener { camera: g.cam.view(), draw_zoom: client::eff_zoom(&gpu, g.cam.view()), size: (w, h) }).unwrap();
        let mut seen_fish = 0;
        let mut seen_creatures = 0;
        let mut shot = None;
        for t in 0..300 {
            g.sim.step(1);
            g.redraw(&mut gpu, p, None);
            let a = g.ambient.as_ref().expect("the ambient part runs");
            // The water animals (0x52ffe0) share the record type; only the fish are checked.
            for f in a.fish.values().filter(|f| f.job == ambient::FISH_JOB) {
                let m = f.y as usize * size + f.x as usize;
                assert!(fishy(m), "fish at {},{}", f.x, f.y);
                assert!(((f.x >> 4) - (centre.0 >> 4)).abs() <= 2 && ((f.y >> 4) - (centre.1 >> 4)).abs() <= 2);
                assert!(f.dir < 6);
            }
            let sprites = a.sprites(g.cur.tick);
            let fish = sprites.iter().filter(|s| matches!(s, s4_viewer::sprites::AmbientSprite::Fish { .. })).count();
            seen_fish += fish;
            seen_creatures += sprites.len() - fish;
            if fish > 0 && shot.is_none() && t > 20 {
                let f = *a.fish.values().next().unwrap();
                shot = Some((f.x, f.y, node_screen(&g.scene, g.cam.view(), (f.x as usize, f.y as usize)), t));
            }
            // The first fish's life, frame by frame.
            if let Some((_, _, _, t0)) = shot
                && t - t0 < 12
            {
                let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../out/viewer/fish/fish_{:02}.png", t - t0));
                if let Ok(rgba) = gpu.capture(g.cam.view()) {
                    let _ = std::fs::create_dir_all(out.parent().unwrap());
                    let _ = client::write_png(&out, w, h, &rgba);
                }
            }
        }
        let a = g.ambient.as_ref().unwrap();
        eprintln!("centre {centre:?}: {} fish started, {seen_fish} fish frames drawn, {seen_creatures} creature frames; first shot at {shot:?}; {:?}", a.log.fish, a.log);
        assert!(a.log.fish > 3, "fish jump every third update near fish water");
        assert!(seen_fish > 0 && seen_creatures > 0);
    }

    /// Vehicles (roman02, a cart and a catapult of player 1 put on free catapult ground): drawn
    /// as `CCart` (0x55ba20) / `CWheeler` (0x59a5c0) say: the empty cart's job +0x26 + 1, picked
    /// by its image box (0x4626d0), the icons after a click (0x49; the catapult's ammo and health),
    /// and a move: the turn sprites first (0x594b50 / 0x55bbd0: +0x5d ticks), then the glide from
    /// the previous node (`DecDistance(256 / +0x62)` per tick). Captures `out/viewer/vehicles/`.
    #[test]
    fn vehicles_drawn_turning_and_selected() {
        use s4_viewer::vehicle_gfx;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (800, 600);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(move |w| {
            let free = |w: &s4_sim::World, x: i32, y: i32| {
                s4_sim::spiral::walk(w.grid, x, y, 20).find(|&(px, py)| {
                    let ok = |qx: i32, qy: i32| {
                        w.grid.checked_index(qx, qy).is_some_and(|n| w.layers.map_object[n] == 0 && w.tiling.catapult[n] != 0 && w.tiling.catapult[n] < 0xc000 && w.layers.move_cost(n) < 7)
                    };
                    // Room for the footprint and a straight drive 8 nodes up-left.
                    (0..=8).all(|k| ok(px - k, py - k)) && [(1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)].iter().all(|&(dx, dy)| ok(px + dx, py + dy))
                })
            };
            let pl = w.player(local).cloned().unwrap();
            let (sx, sy) = free(w, pl.start_x as i32 + 8, pl.start_y as i32 + 8).unwrap();
            let cart = s4_sim::vehicles::add_vehicle(w, sx, sy, local, vehicle_gfx::CART, 0, false).unwrap();
            let (cx, cy) = free(w, sx + 6, sy - 3).unwrap();
            let cat = s4_sim::vehicles::add_vehicle(w, cx, cy, local, 4, 0, false).unwrap();
            let _ = tx.send((cart, cat, (sx, sy), (cx, cy)));
        });
        g.sim.step(1);
        let (cart, cat, at, cat_at) = rx.recv().unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/vehicles");
        let _ = std::fs::create_dir_all(&out);
        let shot = |g: &Game, gpu: &mut Gpu, name: &str| {
            if let Ok(rgba) = gpu.capture(g.cam.view()) {
                let _ = client::write_png(&out.join(name), w, h, &rgba);
            }
        };
        g.cam.jump(view_on(&g.scene, Some((at.0 as usize, at.1 as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        let centre = (w as f64 / 2.0, h as f64 / 2.0);
        g.redraw(&mut gpu, centre, None);
        let unit = |g: &Game, id: u16| *g.cur.population.vehicles.iter().find(|u| u.id == id).unwrap();
        let c = unit(&g, cart);
        let frames = g.scene.sprites.as_ref().unwrap().vehicle_frames().clone();
        assert!(frames.get(&cart).is_some_and(|&n| n > 0), "the cart's +0x61: {frames:?}");
        shot(&g, &mut gpu, "parked.png");
        // A click a few px above the cart's node picks it by its image box, then the catapult.
        let click = |g: &mut Game, gpu: &mut Gpu, node: (i32, i32)| {
            let p = node_screen(&g.scene, g.cam.view(), (node.0 as usize, node.1 as usize));
            let p = (p.0, p.1 - 8.0);
            g.cursor_moved(gpu, p);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Pressed);
            g.mouse(gpu, p, MouseButton::Left, ElementState::Released);
            g.redraw(gpu, p, None);
        };
        click(&mut g, &mut gpu, cat_at);
        assert_eq!(g.pointer.selection.ids, vec![cat], "the catapult picked");
        let k = unit(&g, cat);
        let icons: Vec<u8> = g.pointer.selection.markers(&g.cur.population).iter().map(|m| m.3).collect();
        assert_eq!(icons, vec![vehicle_gfx::SELECTED, 64, 65], "0x49, no ammo (60 + 4), full health (65): {k:?}");
        shot(&g, &mut gpu, "catapult_selected.png");
        // The cart's record flags carry 0x4000: it is selected as "other" (0x5a), and a right
        // click clears such a selection (0x4ec960).
        click(&mut g, &mut gpu, at);
        assert_eq!(g.pointer.selection.ids, vec![cart], "the cart picked");
        let icons: Vec<u8> = g.pointer.selection.markers(&g.cur.population).iter().map(|m| m.3).collect();
        assert_eq!(icons, vec![vehicle_gfx::OTHER]);
        shot(&g, &mut gpu, "cart_selected.png");
        // Catapults do not move in the simulation yet (their `CWarriorBehavior` tasks): the cart
        // drives, sent as the command 5014 its dialog would send. Up-left (direction 4) from
        // direction 0: counter-clockwise from 16-way 0 to 10, 5 ticks.
        let target = (at.0 - 8, at.1 - 8);
        let command = s4_sim::Command::SendTo { x: target.0 as u16, y: target.1 as u16, mode: 0, entities: vec![cart] };
        g.sim.command(Stamped { tick: execution_tick(g.cur.tick), command: s4_sim::PlayerCommand { player: local, command } });
        let mut turned = 0;
        let mut glide = Vec::new();
        for i in 0..60 {
            g.sim.step(1);
            g.redraw(&mut gpu, centre, None);
            let u = unit(&g, cart);
            let mut an = s4_viewer::animate::Animator::default();
            let pose = an.vehicle_poses(&g.cur.population, g.cur.tick as f64 + 0.5, &frames)[&cart];
            if pose.turn.is_some() {
                turned += 1;
            }
            if (u.x as i32, u.y as i32) != at {
                glide.push((g.cur.tick, u.x, u.y, pose.dx, pose.turn));
            }
            if i < 32 {
                shot(&g, &mut gpu, &format!("move_{i:02}.png"));
            }
        }
        assert!(turned >= 4, "turn sprites shown ({turned} ticks): {glide:?}");
        let (_, x1, y1, _, _) = glide[0];
        let first: Vec<f32> = glide.iter().filter(|s| (s.1, s.2) == (x1, y1)).map(|s| s.3).collect();
        // A full node (12 px for an up-left step) while turning, then `256 / 8` per tick.
        assert!(first[..5].iter().all(|&d| d == 12.0) && first[5] == 11.25 && first[6] == 9.75, "the cart glides towards its node: {glide:?}");
        assert!(c.vehicle.is_some_and(|v| !v.loaded), "an empty cart");
        // Goods on board (0x55c690): the loaded cart's sprites (`has_cargo` 0x55ca10).
        shot(&g, &mut gpu, "empty.png");
        g.sim.with_world(move |w| {
            s4_sim::vehicles::cart::load(w, cart, s4_sim::vehicles::cart::good::BOARD, 4);
        });
        g.sim.step(1);
        g.redraw(&mut gpu, centre, None);
        assert!(unit(&g, cart).vehicle.is_some_and(|v| v.loaded), "a loaded cart");
        shot(&g, &mut gpu, "loaded.png");
    }

    /// `SetSettings` 0x514d30: extended tooltips on in a tutorial with the setting off.
    #[test]
    fn tutorial_forces_extended_tooltips() {
        if !s4_formats::game_dir().join("Map/Tutorial").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1024, 768).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let g = Game::start("Tutorial/Tutorial01", &args, 0, &mut gpu, None).unwrap();
        assert!(extended_tooltips(&settings::Settings { extended_tooltip: false, ..g.settings.clone() }, &g.sim));
    }

    /// Tutorial T2 (viewer): the zoom's events reach `CTutorial` (the start's 0x25d, a zoom step's
    /// 0x25d, `Tutorial.SetZoom`'s 0x12), the marker opens dialog 57 at its panel point and the
    /// world cursor draws effect 0x39 at its node until it is deleted. Captures
    /// `out/viewer/tutorial/{marker_arrow,cleared}.png`.
    #[test]
    fn tutorial_marker_arrow_and_zoom() {
        if !s4_formats::game_dir().join("Map/Tutorial").exists() {
            return;
        }
        let (w, h) = (1024, 768);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Tutorial/Tutorial01", &args, 0, &mut gpu, None).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/tutorial");
        let _ = std::fs::create_dir_all(&out);
        let centre = (w as f64 / 2.0, h as f64 / 2.0);
        let tut = g.sim.tutorial.clone();
        let zoom_events = |t: &tutorial::Shared| t.lock().unwrap().pending.iter().filter(|e| e.0 == s4_script::events::ZOOM_FACTOR_CHANGED).map(|e| e.1[0]).collect::<Vec<_>>();
        g.redraw(&mut gpu, centre, None);
        // The start's event 0x25d (after `SetupTutorialEvent`, 0x513468 / 0x51352d): slider 17.
        assert_eq!(tut.lock().unwrap().zoom, 17);
        assert!(zoom_events(&tut).contains(&17), "{:?}", tut.lock().unwrap().pending);
        // A zoom step out: 0x25d with the new slider value (Tutorial01 waits for <= 15).
        let size = g.scene.source.w;
        g.cam.jump(client::zoom_view(&mut gpu, g.cam.view(), -5, size), &screen(&gpu, g.scene.source.w));
        g.redraw(&mut gpu, centre, None);
        let v = client::slider_of_zoom(client::camera_zoom(&gpu, g.cam.view())) as i32;
        assert!(v <= 15, "{v}");
        assert_eq!(tut.lock().unwrap().zoom, v);
        // `Tutorial.SetZoom(17)`: event 0x12, the zoom of slider value 17, heard by `CTutorial`.
        tut.lock().unwrap().requests.push(tutorial::Request::SetZoom(17));
        g.redraw(&mut gpu, centre, None);
        assert_eq!(client::camera_zoom(&gpu, g.cam.view()), client::zoom_of_slider(17));
        assert_eq!(tut.lock().unwrap().zoom, 17);
        // `SetMarker(154, 309)` (Tutorial01 step 19) and `SetWorldCursor` on the view's centre node.
        let node = client::camera_node(&gpu, g.cam.view(), size).unwrap();
        {
            let mut t = tut.lock().unwrap();
            t.marker = Some((154, 309));
            t.arrow = Some((node.0 as u16, node.1 as u16));
        }
        g.redraw(&mut gpu, centre, None);
        let ui = gpu.ui.as_ref().unwrap();
        assert!(ui.state.open.contains(&0x39), "dialog 57 open: {:?}", ui.state.open);
        let d = ui.state.set.dialogs[0x39].as_ref().unwrap();
        assert_eq!((d.x, d.y), (154, 309));
        let arrow_on = gpu.capture(g.cam.view()).unwrap();
        let _ = client::write_png(&out.join("marker_arrow.png"), w, h, &arrow_on);
        // Cleared: the dialog closes, the effect is removed.
        {
            let mut t = tut.lock().unwrap();
            t.marker = None;
            t.arrow = None;
        }
        g.redraw(&mut gpu, centre, None);
        assert!(!gpu.ui.as_ref().unwrap().state.open.contains(&0x39));
        let cleared = gpu.capture(g.cam.view()).unwrap();
        let _ = client::write_png(&out.join("cleared.png"), w, h, &cleared);
        // The arrow's pixels near the centre node differ between the two frames.
        let p = node_screen(&g.scene, g.cam.view(), node);
        let differs = (-40..0).flat_map(|dy| (-20..20).map(move |dx| (dx, dy))).any(|(dx, dy)| {
            let (x, y) = ((p.0 as i32 + dx) as usize, (p.1 as i32 + dy) as usize);
            let i = (y * w as usize + x) * 4;
            arrow_on[i..i + 3] != cleared[i..i + 3]
        });
        assert!(differs, "effect 0x39 drawn at {node:?} ({p:?})");
    }

    /// Water edges for the user's report (coasts and rivers looking buggy): roman02's coast and
    /// river, at zoom 14 / 24 / 43 / 72 px, over consecutive ticks, with and without fog.
    /// Manual: `cargo test --release -p s4-game water_edges -- --ignored`, captures in
    /// `out/viewer/water/`.
    #[test]
    #[ignore]
    fn water_edges() {
        let (w, h) = (800, 600);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/water");
        let _ = std::fs::create_dir_all(&out);
        for (no_fog, tag) in [(true, "nofog"), (false, "fog")] {
            let args = Args { delay: 0, hash_interval: 64, no_fog, ..Default::default() };
            let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
            for (name, node) in [("coast", (412usize, 296usize)), ("river", (324, 206)), ("coast2", (208, 144))] {
                for px in [14.0f32, 24.0, 43.0, 72.0] {
                    let zoom = px / 24.0;
                    let c = g.scene.node_world(node.0, node.1);
                    g.cam.jump(View { x: c[0] - w as f32 / 2.0 / zoom, y: c[1] - h as f32 / 2.0 / zoom, zoom }, &screen(&gpu, g.scene.source.w));
                    for t in 0..3 {
                        g.sim.step(1);
                        g.redraw(&mut gpu, (400.0, 300.0), None);
                        if let Ok(rgba) = gpu.capture(g.cam.view()) {
                            let _ = client::write_png(&out.join(format!("{name}_{tag}_z{px}_t{t}.png")), w, h, &rgba);
                        }
                    }
                }
            }
        }
    }

    /// `Filtering` (the graphics page, default 1) picks the landscape filter at the game's start
    /// (device setup 0x65c980: stage 0 MAG / MIN = LINEAR or POINT).
    #[test]
    fn landscape_filter_from_the_settings() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(320, 240).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let _g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let want = if settings::Settings::load().filtering { s4_viewer::terrain_gpu::Filter::Linear } else { s4_viewer::terrain_gpu::Filter::Point };
        assert_eq!(gpu.terrain.as_ref().unwrap().filter, want);
        let mut st = settings::Settings::load();
        st.filtering = !st.filtering;
        options::apply_graphics(&mut gpu, &st);
        assert_ne!(gpu.terrain.as_ref().unwrap().filter, want);
    }

    /// Point (`Filtering = 0`) vs bilinear (`Filtering = 1`) landscape at zoom 14 / 43 / 72 px on
    /// roman02's coast and river (manual, `--ignored`; `out/viewer/filtering/`).
    #[test]
    #[ignore]
    fn landscape_filtering_captures() {
        use s4_viewer::terrain_gpu::Filter;
        let (w, h) = (800, 600);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/filtering");
        let _ = std::fs::create_dir_all(&out);
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(1);
        for (name, node) in [("coast", (412usize, 296usize)), ("river", (324, 206))] {
            for px in [14.0f32, 43.0, 72.0] {
                for (filter, tag) in [(Filter::Point, "point"), (Filter::Linear, "linear")] {
                    gpu.terrain.as_mut().unwrap().filter = filter;
                    let zoom = px / 24.0;
                    let c = g.scene.node_world(node.0, node.1);
                    g.cam.jump(View { x: c[0] - w as f32 / 2.0 / zoom, y: c[1] - h as f32 / 2.0 / zoom, zoom }, &screen(&gpu, g.scene.source.w));
                    g.redraw(&mut gpu, (400.0, 300.0), None);
                    if let Ok(rgba) = gpu.capture(g.cam.view()) {
                        let _ = client::write_png(&out.join(format!("{name}_z{px}_{tag}.png")), w, h, &rgba);
                    }
                }
            }
        }
    }

    /// A height the simulation changes at run time re-shades the node and the nodes above and
    /// below in the client's terrain (0x65a530 → 0x66d7b0), and the ground it changes is drawn.
    #[test]
    fn changed_nodes_are_reshaded() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(320, 240).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let size = g.scene.source.w;
        let (x, y) = (size / 2, size / 2);
        let n = y * size + x;
        let h0 = g.scene.source.nodes[n].height;
        g.sim.with_world(move |w| {
            w.layers.height[n] = h0.wrapping_add(12);
            w.layers.ground[n] = 0x1c;
        });
        g.sim.step(1);
        g.redraw(&mut gpu, (160.0, 120.0), None);
        let src = &g.scene.source;
        assert_eq!((src.nodes[n].height, src.nodes[n].ground), (h0.wrapping_add(12), 0x1c));
        for yy in [y - 1, y, y + 1] {
            let up = src.nodes[(yy - 1) * size + x].height;
            let down = src.nodes[(yy + 1) * size + x].height;
            assert_eq!(src.nodes[yy * size + x].gradient & 15, s4_formats::gfx::landscape::shade_index(up, down), "row {yy}");
        }
    }

    /// Path wear at run time (sim `wear.rs`, the original redraws a changed node through
    /// 0x65a530): roman02 for 20 000 ticks, the area whose ground changed most, captured before and
    /// after (manual, `--ignored`; `out/viewer/wear/`).
    #[test]
    #[ignore]
    fn path_wear_redrawn() {
        let (w, h) = (800, 600);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/wear");
        let _ = std::fs::create_dir_all(&out);
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        const TICKS: u32 = 20_000;
        // Pass 1: where does the ground change?
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(1);
        g.redraw(&mut gpu, (400.0, 300.0), None);
        let before = g.cur.population.ground.clone();
        g.sim.step(TICKS);
        g.redraw(&mut gpu, (400.0, 300.0), None);
        let after = g.cur.population.ground.clone();
        let size = g.cur.population.size;
        let changed: Vec<usize> = (0..before.len()).filter(|&i| before[i] != after[i]).collect();
        let kinds = changed.iter().fold(std::collections::BTreeMap::new(), |mut m, &i| {
            *m.entry((before[i], after[i])).or_insert(0) += 1;
            m
        });
        println!("{} nodes changed: {kinds:?}", changed.len());
        assert!(!changed.is_empty());
        // The scene's terrain source follows the snapshot.
        assert!(changed.iter().all(|&i| g.scene.source.nodes[i].ground == after[i]));
        let best = *changed
            .iter()
            .max_by_key(|&&c| changed.iter().filter(|&&o| ((o % size) as i32 - (c % size) as i32).abs() <= 8 && ((o / size) as i32 - (c / size) as i32).abs() <= 8).count())
            .unwrap();
        let node = (best % size, best / size);
        println!("centre {node:?}");
        let shoot = |g: &mut Game, gpu: &mut Gpu, name: &str| {
            let c = g.scene.node_world(node.0, node.1);
            let zoom = 2.0;
            g.cam.jump(View { x: c[0] - w as f32 / 2.0 / zoom, y: c[1] - h as f32 / 2.0 / zoom, zoom }, &screen(gpu, g.scene.source.w));
            g.redraw(gpu, (400.0, 300.0), None);
            let rgba = gpu.capture(g.cam.view()).unwrap();
            let _ = client::write_png(&out.join(name), w, h, &rgba);
            rgba
        };
        let after_img = shoot(&mut g, &mut gpu, "after.png");
        drop(g);
        // Pass 2: the same place at the start.
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(1);
        let before_img = shoot(&mut g, &mut gpu, "before.png");
        assert_ne!(before_img, after_img);
    }

    /// Water edges under a moving camera: the same tick drawn with the view shifted by whole
    /// and fractional pixels, at zoom 1 and 1.5 (manual, `--ignored`; `out/viewer/water/shift_*`).
    #[test]
    #[ignore]
    fn water_edges_shifted() {
        let (w, h) = (800, 600);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/water");
        let _ = std::fs::create_dir_all(&out);
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(1);
        for (name, node, linear) in [("coast", (412usize, 296usize), false), ("river", (334, 210), false), ("coastlin", (412, 296), true)] {
            if let Some(t) = gpu.terrain.as_mut() {
                t.filter = if linear { s4_viewer::terrain_gpu::Filter::Linear } else { s4_viewer::terrain_gpu::Filter::Point };
            }
            for zoom in [1.0f32, 1.5] {
                let c = g.scene.node_world(node.0, node.1);
                for (k, (dx, dy)) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (7.0, 3.0), (0.5, 0.0), (0.25, 0.75)].iter().enumerate() {
                    g.cam.jump(View { x: c[0] - w as f32 / 2.0 / zoom + dx, y: c[1] - h as f32 / 2.0 / zoom + dy, zoom }, &screen(&gpu, g.scene.source.w));
                    g.redraw(&mut gpu, (400.0, 300.0), None);
                    if let Ok(rgba) = gpu.capture(g.cam.view()) {
                        let _ = client::write_png(&out.join(format!("shift_{name}_z{zoom}_{k}.png")), w, h, &rgba);
                    }
                }
            }
        }
    }

    /// Mushroom farms (dark03, player 2's eight farms, no fog): a pull starts slot 0 (OPEN), its
    /// arrival slot 1 (FLOW), a sphere shows in slot 2 (GROW at `spheres − 1`); each pull puts
    /// effect 0x14 on the mushroom and the flight 0x15 to the farm on the scene (0x4e3b90).
    /// Captures `out/viewer/mushfarm/`.
    #[test]
    fn mushroom_farm_patches_and_pull_flight() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (800, 600);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/dark03", &args, 0, &mut gpu, None).unwrap();
        let farm_of = |g: &Game| g.sim.snapshot().population.buildings.iter().find(|b| b.mushroom.is_some_and(|m| m.last_pull > 0)).cloned();
        // The farms seen before any pull; then stepped (no frames) to the first pull.
        g.redraw(&mut gpu, (0.0, 0.0), None);
        let mut farm = None;
        for _ in 0..2000 {
            g.sim.step(4);
            if let Some(b) = farm_of(&g) {
                farm = Some(b);
                break;
            }
        }
        g.redraw(&mut gpu, (0.0, 0.0), None);
        let b = farm.expect("a pull within 8000 ticks");
        let m = b.mushroom.unwrap();
        g.cam.jump(view_on(&g.scene, Some((b.x as usize, b.y as usize)), (w, h), g.sim.local), &screen(&gpu, g.scene.source.w));
        let open = g.patches.houses[&b.id].slots[0];
        assert!(open.active && open.job != 0, "OPEN plays from the pull: {open:?}");
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/mushfarm");
        let _ = std::fs::create_dir_all(&out);
        let mut flow_seen = false;
        for i in 0..400 {
            let p = (w as f64 / 2.0, h as f64 / 2.0);
            g.redraw(&mut gpu, p, None);
            if i % 8 == 0
                && i < 96
                && let Ok(rgba) = gpu.capture(g.cam.view())
            {
                let _ = client::write_png(&out.join(format!("pull_{i:02}.png")), w, h, &rgba);
            }
            let s = &g.patches.houses[&b.id].slots;
            if s[1].active {
                flow_seen = true;
                assert!(!s[0].active, "OPEN stops when the pull arrives");
            }
            // Arrival is seen at the next OPEN-frames update, the sphere FLOW − 1 ticks later.
            if g.cur.tick >= m.pull_end + 80 {
                break;
            }
            g.sim.step(1);
        }
        assert!(flow_seen, "FLOW after the pull's arrival");
        let s = g.patches.houses[&b.id].slots;
        let spheres = g.cur.population.buildings.iter().find(|x| x.id == b.id).and_then(|x| x.mushroom).unwrap().spheres;
        assert!(spheres >= 1);
        assert_eq!((s[2].job == s[2].own_job, s[2].frame), (true, spheres as u32 - 1), "GROW shows the spheres");
    }

    #[test]
    fn roman01_won_by_the_default_rule() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1024, 768).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        // Paused and stepped: no wall clock.
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let file = std::env::temp_dir().join(format!("s4rs-test-progress-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&file);
        g.progress_file = Some(file.clone());
        g.campaign = Some((1, 0));
        g.sim.with_world(|w| {
            let towers: Vec<u16> = w
                .entities
                .iter()
                .filter(|(_, e)| e.obj_type == s4_sim::entity::ObjType::Building && e.owner == 2 && e.kind == 46)
                .map(|(i, _)| i)
                .collect();
            assert_eq!(towers.len(), 3);
            for t in towers {
                w.kill(t, 1);
            }
        });
        for _ in 0..3000 {
            g.redraw(&mut gpu, (0.0, 0.0), None);
            if g.decided != 0 {
                break;
            }
            g.sim.step(1);
        }
        assert_eq!(g.decided, script::WON, "decided after tick {}", g.sim.snapshot().tick);
        let ui = gpu.ui.as_ref().unwrap();
        assert!(ui.state.open.contains(&72));
        assert_eq!(ui.state.texts.get(&(72, 1795)).map(|s| s.as_str()), Some("You have won!"));
        // 0x4807f8: centred over the map view right of the panel, 10 px from the top.
        let d = ui.state.set.dialog(72).unwrap();
        assert_eq!((d.x as i32, d.y), ((1024 - 210) / 2 - (d.width as i32 + 1) / 2 + 210, 10));
        let p = progress::Progress::load_from(&file);
        assert_eq!(p.roman, 1, "mission 1 recorded");
        // The end record: decided, the local player's team won (event 0x37's team).
        let rec = g.end_record();
        assert!(rec.decided);
        assert_eq!(rec.winner, rec.team[g.sim.local as usize]);
        assert_ne!(rec.winner, rec.team[2]);
        // Saved and loaded: still decided, the same winner (`HostState::winner`).
        let save = std::env::temp_dir().join(format!("s4rs-test-decided-{}.s4rs", std::process::id()));
        g.sim.save(&save, "decided", g.client_state(), Vec::new()).unwrap();
        let winner = rec.winner;
        drop(g);
        let loaded = Args { delay: 0, hash_interval: 64, load: Some(save.to_string_lossy().into_owned()), ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &loaded, 0, &mut gpu, None).unwrap();
        g.redraw(&mut gpu, (0.0, 0.0), None);
        let rec = g.end_record();
        assert!(rec.decided && rec.winner == winner, "{} {}", rec.decided, rec.winner);
        let _ = std::fs::remove_file(&save);
        // Evidence (best effort): the screen with dialog 72.
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/roman01_won.png");
        if let Ok(rgba) = gpu.capture(g.cam.view()) {
            let _ = client::write_png(&out, 1024, 768, &rgba);
        }
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn network_time_delta_from_the_install() {
        if s4_formats::game_dir().join("Config").join("Network.cfg").exists() {
            assert_eq!(super::network_time_delta(), 500);
            assert_eq!(s4_net::commands::dt_for_delay(500), 7);
        }
    }

    /// The soldier dialog D45's buttons (0x4a6b80) on roman01: the groups toggle opens D55 with
    /// nothing in it; Ctrl + group 3 stores the selection (5041), the sidebar shows group 3's
    /// soldier image; a plain click on group 3 with an empty selection recalls it; "hold" (0x19c)
    /// sends mode 8; the target mode (0x19e) takes the next map click and sends the soldiers there.
    #[test]
    fn soldier_dialog_buttons_and_groups() {
        use s4_viewer::guistate::UiEvent;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let snap = g.sim.snapshot();
        let soldiers: Vec<u16> = snap.population.settlers.iter().filter(|u| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xf))).map(|u| u.id).take(5).collect();
        assert!(soldiers.len() >= 2, "roman01: player 1 has soldiers");
        let first = *snap.population.settlers.iter().find(|u| u.id == soldiers[0]).unwrap();
        client::select_ids(&mut gpu, &snap.population, &mut g.pointer.selection, soldiers.clone());
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&45), "the soldier dialog");
        let click = |g: &mut Game, gpu: &mut Gpu, ev: UiEvent| {
            let mut inp = Input { gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.ui_events(&[ev]);
        };
        // The groups toggle.
        click(&mut g, &mut gpu, UiEvent::Command { dialog: 45, link: 1440, effects: 1 });
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&55), "groups sidebar open");
        let hidden = |gpu: &Gpu, l: u16| {
            let s = &gpu.ui.as_ref().unwrap().state;
            s.index_of(55, l).map(|i| s.effects.get(&(55, i)).copied().unwrap_or(0) & 8 != 0)
        };
        assert_eq!(hidden(&gpu, 884), Some(true), "group 3 empty");
        // Ctrl + group 3: stored (5041), shown with the soldiers' image.
        g.pointer.ctrl = true;
        click(&mut g, &mut gpu, UiEvent::Command { dialog: 55, link: 875, effects: 0 });
        g.pointer.ctrl = false;
        g.sim.step(10);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let pop = g.sim.snapshot().population.clone();
        assert_eq!(pop.groups[local as usize * s4_sim::group::GROUPS + 3], soldiers);
        gpu.ui.as_mut().unwrap().update_pages(&pop, Default::default());
        assert_eq!(hidden(&gpu, 884), Some(false));
        assert_eq!(gpu.ui.as_ref().unwrap().state.images.get(&(55, 884)), Some(&(0x163, 0)));
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/d45_groups.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // "Hold" (0x19c): mode 8 for the selection; the target mode: the next map click.
        click(&mut g, &mut gpu, UiEvent::Command { dialog: 45, link: 1439, effects: 0 });
        click(&mut g, &mut gpu, UiEvent::Command { dialog: 45, link: 1438, effects: 0 });
        assert!(g.bdlg.target && g.bdlg.state_of(false, &g.pointer) == 9);
        let target = (first.x as usize + 12, first.y as usize + 8);
        assert!(g.bdlg.target_click(&g.sim, &g.pointer, target));
        assert!(!g.bdlg.target);
        g.sim.step(400);
        let after = g.sim.snapshot().population.clone();
        let moved = after.settlers.iter().find(|u| u.id == first.id).unwrap();
        assert!((moved.x as i32 - target.0 as i32).abs().max((moved.y as i32 - target.1 as i32).abs()) <= 6, "walked to the target: {:?}", (moved.x, moved.y));
        // Recall group 3 with an empty selection: D55 stays reachable through the selection.
        client::select_ids(&mut gpu, &after, &mut g.pointer.selection, vec![soldiers[0]]);
        click(&mut g, &mut gpu, UiEvent::Command { dialog: 55, link: 875, effects: 0 });
        assert_eq!(g.pointer.selection.ids, soldiers, "group 3 recalled");
    }

    /// The build pages' go-to (s4_viewer::build_goto) on roman02 with two barracks added: a right
    /// click on the barracks button centres the view on them in list order (0x1b6, the selection
    /// kept), Ctrl + right click selects the cursor's barracks without advancing (0x1e6) and its
    /// dialog presses the sidebar toggle (0x12938c7 → D22's 344 → the barracks sidebar D48); the
    /// dialog's "next" (0x198) advances the same cursor.
    #[test]
    fn build_page_go_to() {
        use s4_viewer::guistate::UiEvent;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        const BARRACKS: u8 = 24;
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(move |w| {
            let pl = w.player(local).cloned().unwrap();
            let (cx, cy) = (pl.start_x as i32, pl.start_y as i32);
            let size = w.grid.size() as i32;
            let mut ids = Vec::new();
            for _ in 0..2 {
                let sites = s4_sim::build::possible_sites(w, local, BARRACKS, (0, 0), (size, size));
                let &(x, y, _) = sites.iter().min_by_key(|s| (s.0 as i32 - cx).abs() + (s.1 as i32 - cy).abs()).unwrap();
                ids.push(s4_sim::create::add_building(w, x as i32, y as i32, local, BARRACKS).unwrap());
            }
            let _ = tx.send(ids);
        });
        g.sim.step(1);
        let ids = rx.recv().unwrap();
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let pos = |g: &Game, id: u16| g.cur.population.buildings.iter().find(|b| b.id == id).map(|b| (b.x, b.y));
        // Newest (highest id) first: [inf] the sim keeps no `CBuildingMgr` list.
        let (newer, older) = (ids[0].max(ids[1]), ids[0].min(ids[1]));
        let right = |g: &mut Game, gpu: &mut Gpu, ctrl: bool| {
            g.pointer.ctrl = ctrl;
            let mut inp = Input { gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.ui_events(&[UiEvent::Context { dialog: 13, link: 509 }]);
            g.pointer.ctrl = false;
        };
        right(&mut g, &mut gpu, false);
        assert_eq!(g.bdlg.centre.take(), pos(&g, newer), "0x1b6: the head");
        assert!(g.pointer.selection.ids.is_empty(), "0x1b6 selects nothing");
        right(&mut g, &mut gpu, false);
        assert_eq!(g.bdlg.centre.take(), pos(&g, older), "0x1b6: the next");
        // Ctrl: the cursor's barracks (the older one) selected, its dialog D22; the deferred press
        // opens the sidebar on the following frames.
        right(&mut g, &mut gpu, true);
        assert_eq!(g.pointer.selection.ids, vec![older], "0x1e6: the cursor's building");
        assert_eq!(g.bdlg.centre.take(), pos(&g, older));
        assert!(gpu.ui.as_ref().unwrap().deferred_press);
        for _ in 0..3 {
            g.redraw(&mut gpu, (640.0, 512.0), None);
        }
        let ui = gpu.ui.as_ref().unwrap();
        assert!(ui.state.open.contains(&22), "the barracks dialog");
        assert!(!ui.deferred_press, "consumed by D22's update");
        assert!(ui.state.is_pressed_link(22, 344) && ui.state.open.contains(&48), "the sidebar toggle pressed, D48 open");
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/goto_barracks.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // The dialog's "next" (0x198): the cursor advances from the older one to the head.
        let snap = g.sim.snapshot();
        let f = snap.building.clone().expect("the barracks' facts");
        assert_eq!(f.id, older);
        g.bdlg.select_next_building(&mut gpu, &g.sim, &mut g.pointer, BARRACKS);
        assert_eq!(g.pointer.selection.ids, vec![newer]);
    }

    /// D44's "back to carriers" (0x5bb → UI 0x1a7 → 0x4ecb90 → net 5021 `MorphToCarrier`) on
    /// roman02 with three pioneers added on the player's land: they become carriers.
    #[test]
    fn specialists_back_to_carriers() {
        use s4_viewer::guistate::UiEvent;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        const PIONEER: u16 = 46;
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(move |w| {
            let pl = w.player(local).cloned().unwrap();
            let free: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, pl.start_x as i32 + 6, pl.start_y as i32 + 6, 12)
                .filter(|&(x, y)| w.grid.checked_index(x, y).is_some_and(|n| w.layers.map_object[n] == 0 && w.owner_of(n) == local && w.layers.move_cost(n) < 7))
                .step_by(3)
                .take(3)
                .collect();
            let ids: Vec<u16> = free.iter().filter_map(|&(x, y)| s4_sim::create::add_settler(w, x, y, local, PIONEER)).collect();
            let _ = tx.send(ids);
        });
        g.sim.step(2);
        let ids = rx.recv().unwrap();
        assert_eq!(ids.len(), 3);
        let snap = g.sim.snapshot();
        client::select_ids(&mut gpu, &snap.population, &mut g.pointer.selection, ids.clone());
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&44), "the specialists' dialog");
        bdlg::SENT.with(|s| s.borrow_mut().clear());
        let mut inp = Input { gpu: &mut gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
        inp.ui_events(&[UiEvent::Command { dialog: 44, link: 1467, effects: 0 }]);
        assert_eq!(bdlg::SENT.with(|s| s.borrow().clone()), vec![s4_sim::Command::MorphToCarrier { entities: ids.clone() }]);
        g.sim.step(10);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let kinds: Vec<u16> = ids.iter().filter_map(|id| g.cur.population.settlers.iter().find(|u| u.id == *id).map(|u| u.kind)).collect();
        assert_eq!(kinds, vec![1, 1, 1], "carriers now");
    }

    /// The town page's eye catchers sidebar (s4_viewer::eyecatcher_sidebar) on roman02: the toggle
    /// 0x224 opens dialog 52 with zero counts, a button enters the build mode for its type
    /// (0x1b5), a placed eye catcher shows as "+1" (a site), the release closes the sidebar.
    #[test]
    fn eyecatcher_sidebar() {
        use s4_viewer::eyecatcher_sidebar::{DIALOG, TOGGLE};
        use s4_viewer::guistate::UiEvent;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let ev = |g: &mut Game, gpu: &mut Gpu, e: UiEvent| {
            let mut inp = Input { gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.ui_events(&[e]);
        };
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(DIALOG, l)).cloned().unwrap_or_default();
        let hidden = |gpu: &Gpu, l: u16| {
            let s = &gpu.ui.as_ref().unwrap().state;
            s.index_of(DIALOG, l).map(|i| s.effects.get(&(DIALOG, i)).copied().unwrap_or(0) & 8 != 0)
        };
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: TOGGLE.0, link: TOGGLE.1, effects: 1 });
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&DIALOG), "dialog 52 open");
        assert_eq!((text(&gpu, 0x2a5), hidden(&gpu, 0x2ae)), ("0".into(), Some(true)), "type 0x40: none");
        assert_eq!(text(&gpu, 0x2be), "0", "no costs without a hovered button");
        // Button 0x2a4 (type 0x40): the build mode with the sites.
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: DIALOG, link: 0x2a4, effects: 1 });
        assert_eq!(g.build.building, Some(0x40));
        let mut site = None;
        for _ in 0..50 {
            g.sim.step(1);
            let snap = g.sim.snapshot();
            if let Some(m) = snap.markers.as_ref().filter(|m| m.building == 0x40 && !m.sites.is_empty()) {
                site = Some((m.sites[0].0 as usize, m.sites[0].1 as usize));
                break;
            }
        }
        let site = site.expect("eye catcher sites");
        let snap = g.sim.snapshot();
        assert!(g.build.click(site, &snap, &mut gpu, &g.sim).is_some());
        g.sim.step(10);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert_eq!((text(&gpu, 0x2ae), hidden(&gpu, 0x2ae)), ("+1".into(), Some(false)), "the site counted");
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/eyecatchers.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: TOGGLE.0, link: TOGGLE.1, effects: 0 });
        assert!(!gpu.ui.as_ref().unwrap().state.open.contains(&DIALOG), "closed");
    }

    /// The goods race D77 (s4_viewer::goods_race) in a game of type 3 on roman02 (player 1 alone
    /// in alliance 1): the extras' 929 opens it in place of the briefing page with the sums of
    /// the players' `CStatistic +0x1078`; the holder refreshes it every 16 ticks until a tab's
    /// 0x192 (928, the save page) empties it.
    #[test]
    fn goods_race_dialog() {
        use s4_viewer::goods_race::{BUTTON, DIALOG};
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let goods = [3, 4, 7, 8, 9, 10, 11];
        let mut teams = [Some(1u8); 8];
        teams[0] = Some(0);
        let players = sim::PlayerSetup { game_type: 3, race_goods: goods, teams, ..Default::default() };
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, players: Some(players), ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(20);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let click = |g: &mut Game, gpu: &mut Gpu, dialog: usize, link: u16| {
            let p = client::control_point(gpu, dialog, link).unwrap();
            let mut inp = Input { gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.left(p, true);
            inp.left(p, false);
        };
        click(&mut g, &mut gpu, 0, 611);
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&BUTTON.0), "the extras submenu");
        click(&mut g, &mut gpu, BUTTON.0, BUTTON.1);
        let open = |gpu: &Gpu, d: usize| gpu.ui.as_ref().unwrap().state.open.contains(&d);
        assert!(open(&gpu, DIALOG) && !open(&gpu, 35), "D77 instead of the briefing");
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(DIALOG, l)).cloned().unwrap_or_default();
        let pop = g.sim.snapshot().population.clone();
        assert_eq!(pop.economy.race_goods, goods);
        assert_eq!(&pop.teams[1..3], &[1, 2]);
        let sum = |g: u8, mine: bool| -> i32 { (1..9).filter(|&p| pop.teams[p] != 0 && (pop.teams[p] == 1) == mine).map(|p| pop.economy.players[p].goods_start[g as usize]).sum() };
        for (i, &good) in goods.iter().enumerate() {
            let i = i as u16;
            assert_eq!((text(&gpu, 649 + i), text(&gpu, 656 + i)), (sum(good, true).to_string(), sum(good, false).to_string()), "good {good}");
        }
        assert!(goods.iter().any(|&g| sum(g, true) > 0), "player 1 has some of the goods");
        assert_eq!((text(&gpu, 670), text(&gpu, 671)), ("I".into(), "II".into()));
        assert!(!text(&gpu, 646).is_empty(), "the title (string 0x7fa)");
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/goods_race.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // The refresh at the next `tick & 0xf == 0xe`.
        let before = gpu.ui.as_ref().unwrap().race_holder.unwrap();
        g.sim.step(16);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(gpu.ui.as_ref().unwrap().race_holder.unwrap() > before);
        assert!(open(&gpu, DIALOG));
        // The save tab: 0x192 empties the holder, page 33 replaces D77 for good.
        click(&mut g, &mut gpu, 8, 928);
        assert!(gpu.ui.as_ref().unwrap().race_holder.is_none());
        g.sim.step(32);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(!open(&gpu, DIALOG) && open(&gpu, 33));
    }

    /// The end statistics (frontend/endstat.rs) after Roman mission 1: dialog 13 with our name,
    /// the game time and the sim's statistic (mode 1 kills / strengths, mode 0 land / mana), the
    /// mode radios switch the table, OK goes on to the campaign screen (dialog 8).
    #[test]
    fn end_statistics_screen() {
        use frontend::endstat::DIALOG;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.campaign = Some((1, 0));
        g.sim.step(300);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let rec = g.end_record();
        assert_eq!(rec.tick, g.cur.tick);
        assert!(rec.players >= 2, "{}", rec.players);
        assert_eq!(rec.names[1], g.settings.player_name);
        assert_eq!(rec.path, r"Map\Campaign\roman01.map");
        // The strengths start at the base (0x4f86b0) and the land share is set.
        assert!(rec.stats[1].at(0x1074) > 0 && rec.stats[1].at(0x106c) > 0, "{} {}", rec.stats[1].at(0x1074), rec.stats[1].at(0x106c));
        drop(g);
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::end_statistic(&mut gpu, 0, rec.clone(), Some(frontend::victory::Outcome { campaign: 1, level: 0, won: false })).unwrap();
        let open = |gpu: &Gpu, d: usize| gpu.ui.as_ref().unwrap().state.open.contains(&d);
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(DIALOG, l)).cloned().unwrap_or_default();
        assert!(open(&gpu, DIALOG));
        assert_eq!(text(&gpu, 0xa08), rec.names[1]);
        assert_eq!(text(&gpu, 0xa66), frontend::endstat::time(rec.tick));
        assert_eq!(text(&gpu, 0xa65), r"Campaign\roman01");
        // Mode 1: kills, foreign strength %.
        assert_eq!(text(&gpu, 2618), rec.stats[1].at(0xd28).to_string());
        assert_eq!(text(&gpu, 2626), format!("{}%", (rec.stats[1].at(0x1074) * 100) >> 8));
        let click = |f: &mut frontend::Frontend, gpu: &mut Gpu, link: u16| {
            let d = if gpu.ui.as_ref().unwrap().state.open.contains(&DIALOG) { DIALOG } else { frontend::endstat::DETAIL };
            let p = client::control_point(gpu, d, link).unwrap();
            f.mouse(gpu, p, GuiMouse::LeftDown);
            f.mouse(gpu, p, GuiMouse::LeftUp)
        };
        click(&mut f, &mut gpu, frontend::endstat::MODE0);
        assert_eq!(text(&gpu, 2626), format!("{}%", rec.stats[1].at(0x106c)));
        frontend::render(&mut gpu);
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/endstat13.png");
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // Player 1's detail (event 0x73 → dialog 21), scrolled, back (0x75: dialog 13, mode 1).
        use frontend::endstat::DETAIL;
        click(&mut f, &mut gpu, 0xa08);
        assert!(open(&gpu, DETAIL) && !open(&gpu, DIALOG));
        let dtext = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(DETAIL, l)).cloned().unwrap_or_default();
        assert_eq!(dtext(&gpu, 0x83e), rec.names[1]);
        assert_eq!(dtext(&gpu, 0x84f), rec.stats[1].at(0x21c).to_string());
        frontend::render(&mut gpu);
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/endstat21.png");
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        let first = dtext(&gpu, 0x843);
        click(&mut f, &mut gpu, frontend::endstat::DOWN);
        assert_eq!(dtext(&gpu, 0x843), "-".repeat(0x76), "one row on (from {first})");
        click(&mut f, &mut gpu, frontend::endstat::BACK);
        assert!(open(&gpu, DIALOG) && !open(&gpu, DETAIL));
        assert_eq!(text(&gpu, 2626), format!("{}%", (rec.stats[1].at(0x1074) * 100) >> 8), "mode 1 again");
        click(&mut f, &mut gpu, frontend::endstat::OK);
        assert!(!open(&gpu, DIALOG) && open(&gpu, 8), "the Roman campaign's screen");
    }

    /// The players' names (`chat_names`): the map's (Aeneas: player 2 "Latinus"), ours for the
    /// local player; the end record takes them.
    #[test]
    fn player_names_from_the_map() {
        if !s4_formats::game_dir().join("Map").join("Singleplayer").join("Aeneas.map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let g = Game::start("Singleplayer/Aeneas", &args, 0, &mut gpu, None).unwrap();
        let names = gpu.ui.as_ref().unwrap().state.page_info.chat.names.clone();
        assert_eq!((names[1].as_str(), names[2].as_str(), names[3].as_str(), names[4].as_str()), (g.settings.player_name.as_str(), "Latinus", "Rutuler", "Turnus"));
        let rec = g.end_record();
        assert_eq!(rec.names[2], "Latinus");
    }

    /// In a campaign mission (`GlobalGameStateObject +0x3c` = 3) the settler production page 30
    /// hides the saboteur's row (0x4a4630) and the search page 29 its tab 1242 (0x4a3cf0); a free
    /// game shows them.
    #[test]
    fn campaign_pages_hide_rows() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        for campaign in [false, true] {
            let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
            if campaign {
                g.campaign = Some((1, 0));
            }
            g.redraw(&mut gpu, (640.0, 512.0), None);
            let hidden = |gpu: &Gpu, d: usize, l: u16| {
                let s = &gpu.ui.as_ref().unwrap().state;
                s.index_of(d, l).and_then(|i| s.effects.get(&(d, i))).is_some_and(|e| e & 8 != 0)
            };
            g.click_control(&mut gpu, 0, 607);
            g.click_control(&mut gpu, 4, 745);
            assert!(gpu.ui.as_ref().unwrap().state.open.contains(&30), "{:?}", gpu.ui.as_ref().unwrap().state.open);
            assert_eq!(hidden(&gpu, 30, 1253), campaign, "saboteur row, campaign {campaign}");
            assert!(hidden(&gpu, 30, 1267), "the thief's tool always");
            g.click_control(&mut gpu, 4, 748);
            assert!(gpu.ui.as_ref().unwrap().state.open.contains(&29));
            assert_eq!(hidden(&gpu, 29, 1242), campaign, "search tab 1242, campaign {campaign}");
        }
    }

    /// The wheel over the transport page (dialog 25, code 10 of 0x4acf20): the selected good one
    /// rank up (UI event 0x1be, −1 → `SetTransportPriority`).
    #[test]
    fn transport_page_wheel() {
        use s4_viewer::guistate::UiEvent;
        if !s4_formats::game_dir().join("Map").join("Campaign").join("roman01.map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(3);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let eco = s4_viewer::client::own_eco_at_centre(&mut gpu, &g.scene, g.cam.view(), &g.cur.population);
        let rank = |g: &Game, good: usize| g.sim.snapshot().population.economy.sector(eco).map(|e| e.transport_prio[good]).unwrap();
        // The good ranked third.
        let prio = g.cur.population.economy.sector(eco).unwrap().transport_prio;
        let mut order: Vec<usize> = (1..43).filter(|&k| prio[k] > 0).collect();
        order.sort_by_key(|&k| prio[k]);
        let good = order[2];
        if let Some(ui) = gpu.ui.as_mut() {
            ui.state.open(25, &mut Vec::new());
            ui.state.page_info.transport.target = good as i32;
        }
        let before = rank(&g, good);
        g.wheel_events(&mut gpu, &[UiEvent::Wheel { dialog: 25, delta: 120 }]);
        g.sim.step(20);
        assert!(rank(&g, good) < before, "{} → {}", before, rank(&g, good));
    }

    /// `Game.DisableLastNPlayersInStatistic(1)` (MCD2_maya1's `stats` on its first tick): the
    /// GUI's 0x11b589c, which the statistics pages and the end record read.
    #[test]
    fn statistics_hide_the_last_players() {
        if !s4_formats::game_dir().join("Map").join("Campaign").join("MCD2_maya1.map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/MCD2_maya1", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(3);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert_eq!(gpu.ui.as_ref().unwrap().hidden_last, 1);
    }

    /// Game type 3's end screen (frontend/race_end.rs) on roman02 after D77 was opened: dialog 22,
    /// team 1 on its first player's row with D77's sums, mode 1 greyed and hidden, OK → menu.
    #[test]
    fn goods_race_end_screen() {
        use frontend::race_end::DIALOG;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let mut teams = [Some(1u8); 8];
        teams[0] = Some(0);
        let players = sim::PlayerSetup { game_type: 3, race_goods: [3, 4, 7, 8, 9, 10, 11], teams, ..Default::default() };
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, players: Some(players), ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(20);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        g.click_control(&mut gpu, 0, 611);
        g.click_control(&mut gpu, 8, 929);
        let rows = gpu.ui.as_ref().unwrap().race_rows.expect("D77 filled");
        let rec = g.end_record();
        let goods: frontend::race_end::Goods = std::array::from_fn(|k| (rows[k].good as i32, rows[k].team, rows[k].rest));
        drop(g);
        frontend::Frontend::fit(&mut gpu);
        let mut f = frontend::Frontend::race_end(&mut gpu, 0, rec.clone(), goods, None).unwrap();
        let ui = gpu.ui.as_ref().unwrap();
        assert!(ui.state.open.contains(&DIALOG));
        let text = |l: u16| ui.state.texts.get(&(DIALOG, l)).cloned().unwrap_or_default();
        let first = (1..9).find(|&i| rec.team[i] == 1).unwrap() as u16;
        assert_eq!(text(2779 + first - 1), "Team 1");
        assert_eq!(text(2715 + first - 1), (rows[0].team as u32).to_string());
        let fx = |l: u16| ui.state.index_of(DIALOG, l).and_then(|i| ui.state.effects.get(&(DIALOG, i)).copied()).unwrap_or(0);
        assert_eq!(fx(frontend::race_end::MODE1) & 0xc, 0xc, "mode 1 greyed and hidden");
        frontend::render(&mut gpu);
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/race_end22.png");
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        let p = client::control_point(&gpu, DIALOG, frontend::race_end::OK).unwrap();
        f.mouse(&mut gpu, p, GuiMouse::LeftDown);
        f.mouse(&mut gpu, p, GuiMouse::LeftUp);
        let ui = gpu.ui.as_ref().unwrap();
        assert!(!ui.state.open.contains(&DIALOG) && ui.state.open.contains(&0), "the main menu");
    }

    /// The clock (s4_viewer::clock): dark04's script calls `Game.ShowClock(5400)` at its first
    /// tick: dialog 74 in the top right corner counts down to 5400 s of game time and F7 leaves
    /// it; on roman01 F7 shows the game time and F7 again closes it.
    #[test]
    fn clock_dialog() {
        use s4_viewer::clock::{DIALOG, LINK};
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let open = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.open.contains(&DIALOG);
        let text = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.texts.get(&(DIALOG, LINK)).cloned().unwrap_or_default();
        let mut g = Game::start("Campaign/dark04", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(30);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(open(&gpu), "Game.ShowClock opened the clock");
        let c = gpu.ui.as_ref().unwrap().clock;
        assert_eq!((c.countdown, c.seconds, c.locked), (true, 5400, true));
        let d = gpu.ui.as_ref().unwrap().state.set.dialog(DIALOG).unwrap();
        assert_eq!((d.x as u32 + d.width as u32, d.y), (1280 - 1, 0), "the top right corner");
        let tick = g.cur.tick;
        let ui = gpu.ui.as_mut().unwrap();
        ui.clock.last = 0;
        ui.clock.timer(&mut ui.state, tick);
        assert_eq!(text(&gpu), s4_viewer::clock::text(tick, true, 5400).0);
        g.key(&mut gpu, KeyCode::F7, true, false);
        assert!(open(&gpu), "F7 leaves a script's countdown");
        // roman01: F7 toggles the game time.
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(20);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(!open(&gpu));
        g.key(&mut gpu, KeyCode::F7, true, false);
        assert!(open(&gpu));
        assert_eq!(text(&gpu), s4_viewer::clock::text(g.cur.tick, false, 0).0);
        g.key(&mut gpu, KeyCode::F7, true, false);
        assert!(!open(&gpu));
    }

    /// The mission goal (briefing.rs) and `IngameKeybindings` 0x4890b0 on roman01: without its
    /// campaign the map has no goal (no '@' in its description), so the extras' 929 is greyed
    /// and 'M' does nothing; as Roman mission 1 the goal is string 3363: 'M' opens page 35 with
    /// string 880 and the goal. The letters open their pages; Alt+Q the quit page; nothing in a
    /// tutorial.
    #[test]
    fn briefing_page_and_hotkeys() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let open = |gpu: &Gpu, d: usize| gpu.ui.as_ref().unwrap().state.open.contains(&d);
        let greyed = |gpu: &Gpu, d: usize, l: u16| {
            let s = &gpu.ui.as_ref().unwrap().state;
            s.index_of(d, l).map(|i| s.effects.get(&(d, i)).copied().unwrap_or(0) & 4 != 0)
        };
        assert_eq!(gpu.ui.as_ref().unwrap().state.page_info.briefing.1, "");
        g.char_key(&mut gpu, 'm');
        assert!(!open(&gpu, 8) && !open(&gpu, 35), "no goal: 'M' does nothing");
        g.click_control(&mut gpu, 0, 611);
        assert!(open(&gpu, 8) && open(&gpu, 33));
        assert_eq!(greyed(&gpu, 8, 929), Some(true));
        // The letters: (character, page).
        for (c, page) in [('f', 29), ('c', 25), ('x', 26), ('y', 27), ('Z', 27), ('q', 9), ('e', 11), ('r', 12), ('t', 13)] {
            g.char_key(&mut gpu, c);
            assert!(open(&gpu, page), "'{c}' → page {page}");
        }
        // W / A / S / D are the camera's (user's request): none opens the release's page 10 / 31 / 32 / 30.
        for c in ['w', 'W', 'a', 'A', 's', 'S', 'd', 'D'] {
            g.char_key(&mut gpu, c);
            assert!(open(&gpu, 13) && ![10, 30, 31, 32].iter().any(|&p| open(&gpu, p)), "'{c}' opened a page");
        }
        g.alt = true;
        g.char_key(&mut gpu, 'q');
        g.alt = false;
        assert!(open(&gpu, 34), "Alt+Q: the quit page");
        // Roman mission 1.
        g.campaign = Some((1, 0));
        g.set_briefing(&mut gpu);
        let (header, goal) = gpu.ui.as_ref().unwrap().state.page_info.briefing.clone();
        let ui = gpu.ui.as_ref().unwrap();
        assert_eq!((header.as_str(), goal.as_str()), (ui.string(880).as_str(), ui.string(3363).as_str()));
        assert!(!goal.is_empty());
        g.char_key(&mut gpu, 'q');
        g.char_key(&mut gpu, 'M');
        assert!(open(&gpu, 35));
        assert_eq!(greyed(&gpu, 8, 929), Some(false));
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(35, l)).cloned().unwrap_or_default();
        assert_eq!((text(&gpu, 369), text(&gpu, 370)), (header, goal));
        g.redraw(&mut gpu, (640.0, 512.0), None);
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/briefing35.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // A tutorial: no hotkeys.
        g.sim.tutorial.lock().unwrap().active = true;
        g.char_key(&mut gpu, 'f');
        assert!(!open(&gpu, 29) && open(&gpu, 35));
    }

    /// The camera window's dialog 2 (s4_viewer::mini_view) on roman01 at 1280 × 1024: it starts
    /// at the local player's start with scale 24; 0x283 / 0x285 set 10 / 48; 0x282 enters state 10
    /// (cursor 0x22), whose map click on a soldier follows it (then state 1); 0x282's right click
    /// centres the main view on the soldier; a right click on the map leaves state 10.
    #[test]
    fn camera_window_pick_mode() {
        use s4_viewer::guistate::UiEvent;
        use s4_viewer::mini_view::{DIALOG, FOLLOW};
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(5);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let local = g.sim.local;
        let snap = g.sim.snapshot();
        let start = snap.population.starts[local as usize];
        let mv = |gpu: &Gpu| gpu.ui.as_ref().unwrap().mini_view;
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&DIALOG));
        assert_eq!((mv(&gpu).node, mv(&gpu).scale, mv(&gpu).follow), ((start.0 as i32, start.1 as i32), 24, None));
        let ev = |g: &mut Game, gpu: &mut Gpu, e: UiEvent| {
            let mut inp = Input { gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.ui_events(&[e]);
        };
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: DIALOG, link: 0x283, effects: 0 });
        assert_eq!(mv(&gpu).scale, 10);
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: DIALOG, link: 0x285, effects: 0 });
        assert_eq!(mv(&gpu).scale, 48);
        // The pick mode, left by a right click on the map.
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: DIALOG, link: FOLLOW, effects: 0 });
        assert_eq!(g.bdlg.state_of(false, &g.pointer), 10);
        assert_eq!(cursor::choose(0, 10, false, None, Default::default()), Some(cursor::shape::TARGET));
        {
            let mut inp = Input { gpu: &mut gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.right_click_node(Some((10, 10)));
        }
        assert_eq!(g.bdlg.state_of(false, &g.pointer), 1);
        assert_eq!(mv(&gpu).follow, None);
        // Again, and a click on a soldier: followed.
        let soldier = *snap.population.settlers.iter().find(|u| u.owner == local && (1..=5).contains(&(u.flags & 0xf)) && u.job != 0).expect("roman01: a soldier");
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: DIALOG, link: FOLLOW, effects: 0 });
        {
            let mut inp = Input { gpu: &mut gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.mini_pick((soldier.x as usize, soldier.y as usize));
        }
        assert_eq!(g.bdlg.state_of(false, &g.pointer), 1);
        assert_eq!(mv(&gpu).follow, Some((soldier.id, soldier.uid)));
        // 0x282's right click: event 0x13 to the soldier.
        ev(&mut g, &mut gpu, UiEvent::Context { dialog: DIALOG, link: FOLLOW });
        assert_eq!(g.pages.centre.take(), Some((soldier.x, soldier.y)));
        // A click on an empty node: the node, nothing followed.
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: DIALOG, link: FOLLOW, effects: 0 });
        {
            let mut inp = Input { gpu: &mut gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.mini_pick((3, 3));
        }
        assert_eq!((mv(&gpu).node, mv(&gpu).follow), ((3, 3), None));
    }

    /// The camera window follows (0x4fdc60, `place_camera_window`) on roman01 at 1280 × 1024: a
    /// soldier picked in mode 10 and sent 12 nodes away; at every tick the window's view is the
    /// formula at the soldier's node and walk state, and it ends on the soldier's last node.
    /// Captures `out/viewer/camwin/walk_tNNN.png` (the window at the panel's bottom left).
    #[test]
    fn camera_window_follows_a_walking_soldier() {
        use s4_viewer::mini_view::{FOLLOW, MiniView};
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.sim.step(5);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let local = g.sim.local;
        let snap = g.sim.snapshot();
        let soldier = *snap.population.settlers.iter().find(|u| u.owner == local && (1..=5).contains(&(u.flags & 0xf)) && u.job != 0).expect("roman01: a soldier");
        {
            let mut inp = Input { gpu: &mut gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.ui_events(&[s4_viewer::guistate::UiEvent::Command { dialog: s4_viewer::mini_view::DIALOG, link: FOLLOW, effects: 0 }]);
            inp.mini_pick((soldier.x as usize, soldier.y as usize));
        }
        assert_eq!(gpu.ui.as_ref().unwrap().mini_view.follow, Some((soldier.id, soldier.uid)));
        let target = (soldier.x + 12, soldier.y + 4);
        let command = s4_sim::Command::SendTo { x: target.0, y: target.1, mode: 0, entities: vec![soldier.id] };
        g.sim.command(Stamped { tick: execution_tick(g.sim.snapshot().tick), command: s4_sim::PlayerCommand { player: local, command } });
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/viewer/camwin");
        let _ = std::fs::create_dir_all(&out);
        let mut walked = 0;
        for t in 0..160 {
            g.sim.step(1);
            g.redraw(&mut gpu, (640.0, 512.0), None);
            let pop = &g.cur.population;
            let u = *pop.settlers.iter().find(|u| u.id == soldier.id).expect("the soldier");
            walked += (u.distance != 0) as u32;
            // The same placement from the soldier's state, on a copy of the window's state.
            let mut m = MiniView { node: (0, 0), follow: Some((soldier.id, soldier.uid)), centred: None, scale: 24 };
            let f = s4_viewer::mini_view::Followed { uid: u.uid, on_board: false, node: (u.x as i32, u.y as i32), walk: Some((u.dir, u.distance)) };
            let size = pop.size;
            let hgt = |x: i32, y: i32, s: i32| s4_formats::gfx::landscape::height_offset_fixed(pop.height[y as usize * size + x as usize], s << 16) >> 16;
            let p = m.place(&|_| Some(f), &|_, _| true, &hgt);
            let want = m.view(p);
            let (got, rect) = gpu.camera_window.expect("the camera window");
            assert_eq!((got.x, got.y, got.zoom), (want.x, want.y, want.zoom), "tick {t}: {u:?}");
            assert_eq!(rect, s4_viewer::mini_view::RECT);
            if t % 20 == 0 {
                let rgba = gpu.capture(g.cam.view()).unwrap();
                s4_viewer::client::write_png(&out.join(format!("walk_t{t:03}.png")), w, h, &rgba).unwrap();
            }
        }
        assert!(walked > 10, "the soldier walked ({walked} ticks with a distance)");
        let u = *g.cur.population.settlers.iter().find(|u| u.id == soldier.id).unwrap();
        let moved = (u.x as i32 - soldier.x as i32).abs().max((u.y as i32 - soldier.y as i32).abs());
        assert!(moved >= 6, "moved {moved} nodes from ({}, {}) towards {target:?}: {u:?}", soldier.x, soldier.y);
        assert_eq!(gpu.ui.as_ref().unwrap().mini_view.node, (u.x as i32, u.y as i32));
    }

    /// A right drag shows cursor 1 and locks the shape (0x518186 / 0x65e590): a pause meanwhile
    /// does not change it; after the release the per-tick choice (0x4f2900) takes over again.
    #[test]
    fn right_drag_cursor_is_locked() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        g.sim.set_speed(1);
        let frame = |g: &mut Game, gpu: &mut Gpu| {
            g.sim.step(1);
            g.redraw(gpu, (640.0, 512.0), None);
        };
        frame(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::ARROW);
        let p = (640.0, 512.0);
        g.cursor_moved(&mut gpu, p);
        g.mouse(&mut gpu, p, MouseButton::Right, ElementState::Pressed);
        frame(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::ARROW, "pressed, no drag yet");
        g.cursor_moved(&mut gpu, (700.0, 512.0));
        frame(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::DRAG, "dragging");
        g.sim.set_speed(0);
        frame(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::DRAG, "a pause cannot change it");
        g.mouse(&mut gpu, (700.0, 512.0), MouseButton::Right, ElementState::Released);
        frame(&mut g, &mut gpu);
        assert_eq!(g.cursor_shape, cursor::shape::PAUSE, "unlocked: the pause shows");
    }

    /// The drag box is not drawn while the cursor shape is 3, 4, 6 or 7 (0x5172f0 → 0x65e650), but
    /// it is kept and still selects on release; with the arrow it is drawn.
    #[test]
    fn drag_box_hidden_for_some_cursor_shapes() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/roman01", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let snap = g.sim.snapshot();
        let first = *snap.population.settlers.iter().find(|u| u.owner == local && u.flags & 0x2000 != 0 && (1..=5).contains(&(u.flags & 0xf))).expect("a soldier");
        g.cam.jump(view_on(&g.scene, Some((first.x as usize, first.y as usize)), (w, h), local), &screen(&gpu, g.scene.source.w));
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let c = node_screen(&g.scene, g.cam.view(), (first.x as usize, first.y as usize));
        let (a, b) = ((c.0 - 150.0, c.1 - 150.0), (c.0 + 150.0, c.1 + 150.0));
        let scale = gpu.gui_scale as f64;
        for (shape, drawn) in [(cursor::shape::ARROW, true), (3, false), (4, false), (6, false), (7, false)] {
            client::clear_selection(&mut gpu, &snap.population, &mut g.pointer.selection);
            g.pointer.cursor_shape = shape;
            g.cursor_moved(&mut gpu, a);
            g.mouse(&mut gpu, a, MouseButton::Left, ElementState::Pressed);
            g.pointer.moved(&mut gpu, b, scale);
            assert!(g.pointer.rect.is_some(), "shape {shape}: kept");
            assert_eq!(gpu.gui.as_ref().unwrap().selection_box().is_some(), drawn, "shape {shape}");
            g.cursor_moved(&mut gpu, b);
            g.mouse(&mut gpu, b, MouseButton::Left, ElementState::Released);
            assert!(gpu.gui.as_ref().unwrap().selection_box().is_none(), "shape {shape}: cleared");
            assert!(g.pointer.selection.ids.contains(&first.id), "shape {shape}: the box selected");
        }
    }

    /// The spells sidebar D56 (s4_viewer::spell_sidebar) on roman02 with two priests and 200 mana:
    /// D45's 0x5a4 opens it (0x1c1, state 8) with the casts and costs, a spell button chooses
    /// the spell (0x1da), a right click on the map casts it with the nearest priest (5048).
    #[test]
    fn spells_sidebar_casts() {
        use s4_viewer::guistate::UiEvent;
        use s4_viewer::spell_sidebar::{DIALOG, TOGGLE};
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(move |w| {
            let pl = w.player(local).cloned().unwrap();
            let free: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, pl.start_x as i32 + 6, pl.start_y as i32 + 6, 12)
                .filter(|&(x, y)| w.grid.checked_index(x, y).is_some_and(|n| w.layers.map_object[n] == 0 && w.owner_of(n) == local && w.layers.move_cost(n) < 7))
                .step_by(5)
                .take(2)
                .collect();
            let ids: Vec<u16> = free.iter().filter_map(|&(x, y)| s4_sim::create::add_settler(w, x, y, local, 45)).collect();
            w.magic.players[local as usize].mana = 200;
            let costs = w.magic.players[local as usize].costs;
            let _ = tx.send((ids, free, costs));
        });
        g.sim.step(2);
        let (ids, at, costs) = rx.recv().unwrap();
        assert_eq!(ids.len(), 2);
        let snap = g.sim.snapshot();
        client::select_ids(&mut gpu, &snap.population, &mut g.pointer.selection, ids.clone());
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&45), "the soldier dialog for priests");
        let ev = |g: &mut Game, gpu: &mut Gpu, e: UiEvent| {
            let mut inp = Input { gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.ui_events(&[e]);
        };
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: TOGGLE.0, link: TOGGLE.1, effects: 1 });
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&DIALOG), "D56 open");
        assert_eq!(g.bdlg.state_of(false, &g.pointer), 8);
        let text = |gpu: &Gpu, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(DIALOG, l)).cloned().unwrap_or_default();
        assert_eq!(text(&gpu, 0x3b4), costs[0].max(1).to_string(), "spell 0's cost");
        assert!(text(&gpu, 0x3ac).parse::<i32>().unwrap() >= 1, "200 mana pays for spell 0");
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/d56_spells.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // No spell chosen: a right click is `SendTo`. Spell 0 chosen: 5048 by the nearer priest.
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: DIALOG, link: 0x3a4, effects: 1 });
        assert_eq!(g.bdlg.spell, 0);
        bdlg::SENT.with(|s| s.borrow_mut().clear());
        let target = (at[1].0 as usize + 2, at[1].1 as usize + 1);
        let pos: Vec<(u16, u16, u16)> = ids.iter().map(|id| g.sim.snapshot().population.settlers.iter().find(|u| u.id == *id).map(|u| (u.id, u.x, u.y)).unwrap()).collect();
        let hex = |p: &(u16, u16, u16)| {
            let (dx, dy) = (target.0 as i32 - p.1 as i32, target.1 as i32 - p.2 as i32);
            if (dx ^ dy) < 0 { dx.abs() + dy.abs() } else { dx.abs().max(dy.abs()) }
        };
        let nearest = pos.iter().min_by_key(|p| hex(p)).unwrap().0;
        let mut inp = Input { gpu: &mut gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
        inp.right_click_node(Some(target));
        let sent = bdlg::SENT.with(|s| s.borrow().clone());
        assert_eq!(sent, vec![s4_sim::Command::CastSpell { priest: nearest, spell: 0, x: target.0 as u16, y: target.1 as u16 }]);
        assert_eq!(g.bdlg.state_of(false, &g.pointer), 8, "state 8 stays");
        // The toggle released: state 1, the sidebar closed.
        ev(&mut g, &mut gpu, UiEvent::Command { dialog: TOGGLE.0, link: TOGGLE.1, effects: 0 });
        assert!(!gpu.ui.as_ref().unwrap().state.open.contains(&DIALOG));
        assert_eq!(g.bdlg.state_of(false, &g.pointer), 1);
    }

    /// D42's contents (0x58e450 → `s4_sim::vehicles::ferry::dialog`): on roman02 a ferry off the
    /// coast takes a pioneer, a swordsman and a cart aboard (one `SendTo` onto it, as
    /// s4-sim/tests/sim_ferry.rs); its dialog then shows 1 in their slots.
    #[test]
    fn ferry_dialog_shows_its_passengers() {
        use s4_sim::vehicles::{self, kind};
        use s4_viewer::vehicle_group::FERRY;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        assert_eq!(local, 1, "roman02's player 1 owns the coast at (178, 92)");
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(move |w| {
            let ground = |w: &s4_sim::World, x: i32, y: i32| w.layers.ground[w.grid.index(x as u32, y as u32)];
            let fp = s4_sim::spiral::walk(w.grid, 186, 90, 12)
                .find(|&(px, py)| vehicles::can_place(w, px, py, kind::FERRY) && s4_sim::spiral::walk(w.grid, px, py, 6).any(|(a, b)| ground(w, a, b) > 8))
                .unwrap();
            let land = |w: &s4_sim::World, x: i32, y: i32| {
                s4_sim::spiral::walk(w.grid, x, y, 6)
                    .find(|&(px, py)| {
                        let n = w.grid.index(px as u32, py as u32);
                        ground(w, px, py) > 8 && w.layers.map_object[n] == 0 && w.layers.flags[n] & 0x7f == 0 && w.tiling.sector_of(n) != 0
                    })
                    .unwrap()
            };
            let f = vehicles::add_vehicle(w, fp.0, fp.1, 1, kind::FERRY, 3, false).unwrap();
            let lp = land(w, 178, 92);
            let p = s4_sim::create::add_settler(w, lp.0, lp.1, 1, 0x2f).unwrap();
            let ls = land(w, 176, 88);
            let s = s4_sim::create::add_settler(w, ls.0, ls.1, 1, 29).unwrap();
            let lc = s4_sim::spiral::walk(w.grid, 172, 94, 10).find(|&(x, y)| vehicles::can_place(w, x, y, kind::CART)).unwrap();
            let c = vehicles::add_vehicle(w, lc.0, lc.1, 1, kind::CART, 0, false).unwrap();
            let _ = tx.send((f, fp, vec![p, s, c]));
        });
        g.sim.step(5);
        let (f, fp, ids) = rx.recv().unwrap();
        let command = s4_sim::Command::SendTo { x: fp.0 as u16, y: fp.1 as u16, mode: 0, entities: ids };
        g.sim.command(Stamped { tick: execution_tick(g.sim.snapshot().tick), command: s4_sim::PlayerCommand { player: local, command } });
        let mut counts = [0; 12];
        for _ in 0..60 {
            g.sim.step(10);
            counts = g.sim.snapshot().population.economy.ferries.get(&f).copied().unwrap_or_default();
            if counts[3] + counts[5] + counts[11] == 3 {
                break;
            }
        }
        assert_eq!(counts, [0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 1], "pioneer, swordsman, cart aboard");
        let snap = g.sim.snapshot();
        client::select_ids(&mut gpu, &snap.population, &mut g.pointer.selection, vec![f]);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let text = |l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(FERRY, l)).cloned().unwrap_or_default();
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&FERRY));
        assert_eq!((text(734), text(736), text(741), text(731)), ("1".into(), "1".into(), "1".into(), "0".into()));
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/d42_loaded.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
    }

    /// A transport ship's D41 (code 0x16, `CTransportShip` vtbl+0x94 0x591dc0) on roman02: its
    /// cargo slots as rows (good icon, amount), the empty ones hidden, the unload / found /
    /// destroy buttons hidden, the title string 0x8c6.
    #[test]
    fn transport_ship_dialog() {
        use s4_viewer::vehicle_dialog::DIALOG;
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(move |w| {
            use s4_sim::vehicles::{add_vehicle, can_place, kind, transport};
            let (x, y) = s4_sim::spiral::walk(w.grid, 200, 90, 12).find(|&(px, py)| can_place(w, px, py, kind::TRANSPORT_SHIP)).unwrap();
            let id = add_vehicle(w, x, y, local, kind::TRANSPORT_SHIP, 0, false).unwrap();
            let d = transport::data_mut(w, id).unwrap();
            d.slots[0] = [1, 12];
            d.slots[2] = [5, 3];
            let _ = tx.send(id);
        });
        g.sim.step(1);
        let id = rx.recv().unwrap();
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let snap = g.sim.snapshot();
        client::select_ids(&mut gpu, &snap.population, &mut g.pointer.selection, vec![id]);
        for _ in 0..3 {
            g.redraw(&mut gpu, (640.0, 512.0), None);
        }
        let ui = gpu.ui.as_ref().unwrap();
        assert!(ui.state.open.contains(&DIALOG), "D41 for a transport ship");
        let text = |l: u16| ui.state.texts.get(&(DIALOG, l)).cloned().unwrap_or_default();
        let hidden = |l: u16| ui.state.index_of(DIALOG, l).and_then(|i| ui.state.effects.get(&(DIALOG, i))).is_some_and(|e| e & 8 != 0);
        assert_eq!((text(1718), text(1720)), ("12".into(), "3".into()));
        assert!(hidden(1719) && hidden(1735) && !hidden(1718) && !hidden(1734));
        assert!(hidden(0x6b5) && hidden(0x6d6) && hidden(0x6d7) && hidden(0x6d8));
        assert_eq!(text(0x6b2), ui.string(0x8c6));
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/d41_ship.png");
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
    }

    /// The vehicle group dialog D40 and the ferry's D42 (s4_viewer::vehicle_group) on roman02
    /// with two catapults and two ferries added: what `RequestDialog` opens, the counts and
    /// buttons, 0x1a2 → 5010, 0x1c0 → 5049, D42's 0x2ca (next ferry) and 0x2e6 (to D40).
    #[test]
    fn vehicle_group_and_ferry_dialogs() {
        use s4_viewer::guistate::UiEvent;
        use s4_viewer::vehicle_group::{FERRY, GROUP};
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280, 1024);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, no_fog: true, ..Default::default() };
        let mut g = Game::start("Campaign/roman02", &args, 0, &mut gpu, None).unwrap();
        let local = g.sim.local;
        let (tx, rx) = std::sync::mpsc::channel();
        g.sim.with_world(move |w| {
            use s4_sim::vehicles::{add_vehicle, can_place, kind};
            let pl = w.player(local).cloned().unwrap();
            let land = |w: &s4_sim::World, x: i32, y: i32| {
                s4_sim::spiral::walk(w.grid, x, y, 30).find(|&(px, py)| [(0, 0), (1, 0), (1, 1), (0, 1), (-1, 0), (-1, -1), (0, -1)].iter().all(|&(dx, dy)| w.grid.checked_index(px + dx, py + dy).is_some_and(|n| w.layers.map_object[n] == 0 && w.tiling.catapult[n] != 0 && w.tiling.catapult[n] < 0xc000 && w.layers.move_cost(n) < 7)))
            };
            let (ax, ay) = land(w, pl.start_x as i32 + 8, pl.start_y as i32 + 8).unwrap();
            let a = add_vehicle(w, ax, ay, local, kind::WAR_MACHINE, 0, false).unwrap();
            let (bx, by) = land(w, ax + 5, ay).unwrap();
            let b = add_vehicle(w, bx, by, local, kind::WAR_MACHINE, 0, false).unwrap();
            let water = |w: &s4_sim::World, x: i32, y: i32| s4_sim::spiral::walk(w.grid, x, y, 12).find(|&(px, py)| can_place(w, px, py, kind::FERRY));
            let (fx, fy) = water(w, 200, 90).unwrap();
            let f = add_vehicle(w, fx, fy, local, kind::FERRY, 0, false).unwrap();
            let (gx, gy) = water(w, fx + 12, fy + 6).unwrap();
            let f2 = add_vehicle(w, gx, gy, local, kind::FERRY, 0, false).unwrap();
            let _ = tx.send((a, b, f, f2));
        });
        g.sim.step(1);
        let (a, b, f, f2) = rx.recv().unwrap();
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let click = |g: &mut Game, gpu: &mut Gpu, ev: UiEvent| {
            let mut inp = Input { gpu, scene: &mut g.scene, view: g.cam.view(), sim: &g.sim, build: &mut g.build, pointer: &mut g.pointer, bdlg: &mut g.bdlg, pages: &mut g.pages };
            inp.ui_events(&[ev]);
        };
        let select = |g: &mut Game, gpu: &mut Gpu, ids: Vec<u16>| {
            let snap = g.sim.snapshot();
            client::select_ids(gpu, &snap.population, &mut g.pointer.selection, ids);
            g.redraw(gpu, (640.0, 512.0), None);
        };
        let text = |gpu: &Gpu, d: usize, l: u16| gpu.ui.as_ref().unwrap().state.texts.get(&(d, l)).cloned().unwrap_or_default();
        let fx = |gpu: &Gpu, d: usize, l: u16| {
            let s = &gpu.ui.as_ref().unwrap().state;
            s.index_of(d, l).and_then(|i| s.effects.get(&(d, i)).copied()).unwrap_or(0xff)
        };
        let open = |gpu: &Gpu, d: usize| gpu.ui.as_ref().unwrap().state.open.contains(&d);
        // Two catapults: D40 with "2" war machines, only the ammo button enabled, no destroy.
        select(&mut g, &mut gpu, vec![a, b]);
        assert!(open(&gpu, GROUP), "D40 for two catapults");
        assert_eq!((text(&gpu, GROUP, 1757), text(&gpu, GROUP, 1758), text(&gpu, GROUP, 1759)), ("2".into(), "0".into(), "0".into()));
        assert_eq!((fx(&gpu, GROUP, 1762) & 4, fx(&gpu, GROUP, 1763) & 4, fx(&gpu, GROUP, 1764) & 4), (4, 4, 0), "greyed: warships, ferries; enabled: ammo");
        assert_eq!(fx(&gpu, GROUP, 1765) & 8, 8, "destroy hidden for two");
        assert!(!text(&gpu, GROUP, 1753).is_empty(), "title (string 0x377)");
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/d40_two.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // The ammo button: 5010 for each selected (no stone offered on roman02: no carrier comes).
        bdlg::SENT.with(|s| s.borrow_mut().clear());
        click(&mut g, &mut gpu, UiEvent::Command { dialog: GROUP, link: 1764, effects: 0 });
        let sent = bdlg::SENT.with(|s| s.borrow().clone());
        assert_eq!(sent, vec![s4_sim::Command::CatapultLoad { vehicle: a as u32 }, s4_sim::Command::CatapultLoad { vehicle: b as u32 }]);
        // The warships' button 0x1a0: no transition, nothing sent.
        bdlg::SENT.with(|s| s.borrow_mut().clear());
        click(&mut g, &mut gpu, UiEvent::Command { dialog: GROUP, link: 1762, effects: 0 });
        assert!(bdlg::SENT.with(|s| s.borrow().is_empty()));
        // One catapult: destroy shown; it destroys and deselects.
        select(&mut g, &mut gpu, vec![b]);
        assert_eq!(text(&gpu, GROUP, 1757), "1");
        assert_eq!(fx(&gpu, GROUP, 1765) & 8, 0, "destroy shown for one");
        click(&mut g, &mut gpu, UiEvent::Command { dialog: GROUP, link: 1765, effects: 0 });
        assert!(g.pointer.selection.ids.is_empty(), "deselected");
        g.sim.step(10);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(!g.cur.population.vehicles.iter().any(|u| u.id == b), "destroyed");
        // A single ferry: D42 (title string 0x507, nothing on board).
        select(&mut g, &mut gpu, vec![f]);
        assert!(open(&gpu, FERRY) && !open(&gpu, GROUP), "D42 for one ferry");
        assert!(!text(&gpu, FERRY, 715).is_empty());
        assert_eq!(text(&gpu, FERRY, 731), "0");
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/d42_ferry.png");
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // 0x2ca → 0x1ab: the next ferry of the list, selected alone, the view to it.
        click(&mut g, &mut gpu, UiEvent::Command { dialog: FERRY, link: 714, effects: 0 });
        assert_eq!(g.pointer.selection.ids, vec![f2], "the next ferry");
        let at = g.cur.population.vehicles.iter().find(|u| u.id == f2).map(|u| (u.x, u.y));
        assert_eq!(g.bdlg.centre, at);
        // 0x2e6 → 0x1a6: D40 for the selection (one ferry: "1", the unload button enabled).
        click(&mut g, &mut gpu, UiEvent::Command { dialog: FERRY, link: 742, effects: 0 });
        assert!(open(&gpu, GROUP) && !open(&gpu, FERRY), "D40 in place of D42");
        assert_eq!(text(&gpu, GROUP, 1759), "1");
        assert_eq!(fx(&gpu, GROUP, 1763) & 4, 0);
        // Two ferries: D40.
        select(&mut g, &mut gpu, vec![f, f2]);
        assert!(open(&gpu, GROUP));
        assert_eq!(text(&gpu, GROUP, 1759), "2");
        if let Ok(img) = gpu.ui.as_mut().unwrap().compose() {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/d40_ferries.png");
            let _ = client::write_png(&out, img.width, img.height, &img.rgba);
        }
        // One of the two sunk (`CMapObjectMgr::Kill` 0x56d030 → `RemoveFromSelection` 0x4eda10):
        // out of the selection, D40 stays with "1" (not swapped for the other's D42).
        g.sim.with_world(move |w| w.kill(f2, 0));
        g.sim.step(1);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert_eq!(g.pointer.selection.ids, vec![f]);
        assert!(open(&gpu, GROUP) && !open(&gpu, FERRY), "D40 stays");
        assert_eq!(text(&gpu, GROUP, 1759), "1");
        // The last one sunk: the selection empty, 0x4edb10 + `sub_EDC90`: D40 closes.
        g.sim.with_world(move |w| w.kill(f, 0));
        g.sim.step(1);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(g.pointer.selection.ids.is_empty());
        assert!(!open(&gpu, GROUP) && !open(&gpu, FERRY), "D40 closed");
    }
}

#[cfg(test)]
mod mana_tests {
    use super::*;

    /// B1 (viking01, priests casting): the extension bar shows the local player's mana — link
    /// 639, "%u" of `CStatistic +0x1060` (0x4f8b30 at 0x4f8d47 → 0x47c560), the copy of the pool
    /// `Statistic::Update` makes every tick (0x4f87f0). It showed "0" whatever the mana. The
    /// spells sidebar's casts (0x56a670) follow the pool too. Capture `out/gui/mana_ext_bar.png`.
    #[test]
    fn viking01_mana_on_the_extension_bar() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let (w, h) = (1280u32, 1024u32);
        let mut gpu = Gpu::headless(w, h).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/viking01", &args, 0, &mut gpu, None).unwrap();
        g.progress_file = None;
        let local = g.sim.local;
        g.sim.step(10);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let text = |gpu: &Gpu| gpu.ui.as_ref().unwrap().state.texts.get(&(1, 639)).cloned().unwrap_or_default();
        assert!(gpu.ui.as_ref().unwrap().state.open.contains(&1), "the extension bar is open at 1024 px");
        assert_eq!(text(&gpu), "0");
        // The temple's donations (or a script) raise the mana: 37 here.
        g.sim.with_world(move |w| w.magic.players[local as usize].mana = 37);
        g.sim.step(2);
        for _ in 0..3 {
            g.redraw(&mut gpu, (640.0, 512.0), None);
        }
        assert_eq!(text(&gpu), "37", "the extension bar's mana");
        let p = &g.cur.population.economy.players[local as usize];
        assert_eq!((p.mana, p.stat_mana), (37, 37));
        // Viking spell 0 costs 8 now (and more with each cast, 0x4f5a50): some casts.
        assert!(p.spells[0].1 == 8 && p.spells[0].0 > 0, "{:?}", p.spells);
        if let Ok(rgba) = gpu.capture(g.cam.view()) {
            let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/gui/mana_ext_bar.png");
            let _ = std::fs::create_dir_all(out.parent().unwrap());
            client::write_png(&out, w, h, &rgba).unwrap();
        }
    }

    /// F5 turns the cloud shadows on and off and the settings keep it; their clock is real time
    /// while the game runs and stands in a pause.
    #[test]
    fn f5_toggles_the_clouds() {
        if !s4_formats::game_dir().join("Map").exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("s4rs-clouds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        settings::use_test_file(Some(dir.join("settings.toml")));
        let mut gpu = Gpu::headless(1280, 1024).unwrap();
        let args = Args { delay: 0, hash_interval: 64, ..Default::default() };
        let mut g = Game::start("Campaign/dark04", &args, 0, &mut gpu, None).unwrap();
        let mut app = settings::Settings::load();
        g.key(&mut gpu, KeyCode::F5, true, false);
        assert!(gpu.effects.clouds && settings::Settings::load().effects.clouds);
        // Leaving the game saves the app's older copy.
        save_app_settings(&mut app, Some(&gpu));
        assert!(settings::Settings::load().effects.clouds);
        g.key(&mut gpu, KeyCode::F5, true, true);
        g.key(&mut gpu, KeyCode::F5, false, false);
        assert!(gpu.effects.clouds, "held down or released, nothing changes");
        g.redraw(&mut gpu, (640.0, 512.0), None);
        let t = gpu.cloud_time;
        g.last = Instant::now() - std::time::Duration::from_millis(100);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert_eq!(t, gpu.cloud_time, "paused");
        g.sim.set_speed(1);
        for _ in 0..200 {
            if g.cur.tick_ms != 0 {
                break;
            }
            g.sim.step(1);
            std::thread::sleep(std::time::Duration::from_millis(5));
            g.redraw(&mut gpu, (640.0, 512.0), None);
        }
        let t = gpu.cloud_time;
        g.last = Instant::now() - std::time::Duration::from_millis(100);
        g.redraw(&mut gpu, (640.0, 512.0), None);
        assert!(gpu.cloud_time >= t + 0.09, "{t} {}", gpu.cloud_time);
        g.key(&mut gpu, KeyCode::F5, true, false);
        assert!(!gpu.effects.clouds && !settings::Settings::load().effects.clouds);
        settings::use_test_file(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
