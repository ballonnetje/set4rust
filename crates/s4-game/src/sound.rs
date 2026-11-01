//! From simulation and camera to s4-audio: the client half of `CSoundManager`.
//!
//! [`GameSound`] owns the [`Audio`] service for a running game and is fed once per logic tick
//! ([`GameSound::tick`]) with the tick's `SimEvent`s, the snapshot after it and the camera:
//!
//! * **Listener.** Positional sounds are placed by `CalcPanningAndVolume` (release 0x50d6f0) from
//!   the node's screen position (`GetScreenOffsetsByMapIndices`, with terrain height), the output
//!   size, the zoom (`GetZoomFactor`, 0x514860: `(triangle px − 14)·100/58`) and the local fog
//!   (`CFogging::IsPositionVisible`, 0x5aee70). [`listener`] builds that view from the snapshot.
//! * **Job sounds** (`ISettlerRole::Update`, release 0x586a00; sound at 0x586b1f..0x586b8f): after
//!   advancing the animation by the ticks since the last call, if the frame is the sound's frame,
//!   `random == 100` or `rand()%100 <= random`, then `IsPositionVisible`, then
//!   `PlayEnvironmentSound` (0x50c310 = `CalcPanningAndVolume` + `PlaySoundFX`). That `rand()` is
//!   the C runtime's, not the game RNG, so the client rolls it ([`JobSoundRoll`]). `Update` runs
//!   from two places: the role's logic (the sim's `SimEvent::JobSound`) and **drawing**:
//!   `CSettler::GetGfxInfo` (0x5810a0, `CSettler` vtable +0x14) and the buildings' `FillGfxInfo`
//!   for their house workers (0x54d0e0, 0x55a3a0, ...) call it for every settler they draw, every
//!   frame. The logic updates of a working settler are far apart (its frame jumps over the sound
//!   frame), so almost every job sound in the original comes from drawing.
//!   [`GameSound::draw`] is that draw-time call.
//! * **Effects** (`CEffects`, 0x4e33b0): an effect's sound plays while the effect is drawn, the
//!   first time it is iterated if that is its start tick, through `CalcPanningAndVolume` +
//!   `PlaySoundFX` (no separate fog check, so a fogged effect takes a voice at volume 0).
//! * **Logic sounds** (`SimEvent::Sound`): `IsPositionVisible`, then `CalcPanningAndVolume` +
//!   `PlaySoundFX` (the falling tree, `CTree::LogicUpdate` 0x593c50).
//! * **Ambient** (`Sound::Update`, 0x50d860, every logic tick): the loops for the `Flags1` byte of
//!   the 16×16 square under the screen centre ([`ambient_flags`], 0x43ba20).
//! * **Music**: at game start the local race's playlists, SETTLE title 0 ([`GameSound::start`],
//!   0x5155b0 → 0x50d2d0); each tick `CLogic::HandleSoundtracks` (0x4f2bc0) with the dark-land
//!   flag at the view centre ([`music_centre`]).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use kira::backend::Backend;
use s4_audio::{Audio, AudioEvent, MusicStyle, Played, Race, SoundId};
use s4_sim::event::SimEvent;
use s4_viewer::gpu::View as Camera;
use s4_viewer::guistate::UiEvent;
use s4_viewer::population::{Population, Unit};

use crate::sim::Snapshot;

pub mod animals;
pub mod gui;
pub mod rotor;

/// Take the GUI's dialog-procedure calls since the last drain (`GuiState::calls`).
pub fn drain_gui_calls(gpu: &mut s4_viewer::gpu::Gpu) -> Vec<UiEvent> {
    gpu.ui.as_mut().map(|u| std::mem::take(&mut u.state.calls)).unwrap_or_default()
}

/// Triangle width in pixels at camera zoom 1 (`CGameStateHandler`: `SetTriangleSize(24 << 16)`).
const TRIANGLE_PX: f32 = 24.0;

/// The camera as the sound code needs it: the view, the zoom the scene is drawn with
/// (`client::eff_zoom`) and the output size in pixels.
#[derive(Clone, Copy, Debug)]
pub struct Listener {
    pub camera: Camera,
    pub draw_zoom: f32,
    pub size: (u32, u32),
}

impl Listener {
    /// Screen pixel position of a zoom-1 world point.
    fn screen(&self, w: [f32; 2]) -> (f32, f32) {
        ((w[0] - self.camera.x) * self.draw_zoom, (w[1] - self.camera.y) * self.draw_zoom)
    }
    /// Zoom-1 world point at the output centre.
    fn world_centre(&self) -> (f32, f32) {
        let z = self.draw_zoom.max(1e-3);
        (self.camera.x + self.size.0 as f32 / 2.0 / z, self.camera.y + self.size.1 as f32 / 2.0 / z)
    }
}

/// `CalcPanningAndVolume`'s view of the map for this tick: node screen positions with terrain
/// height, the output size, the triangle size and the local fog.
///
/// [inf] The triangle size for the zoom factor is `24 · camera zoom` (`m_uZoom` is the triangle
/// width in pixels: 0x5134ea multiplies node coordinates by its integer part); a HiDPI draw scale
/// is not counted, positions and output size are in drawn pixels (the volume only depends on
/// their ratio).
pub fn listener(snap: &Arc<Snapshot>, lis: &Listener) -> s4_audio::View {
    let size = snap.population.size;
    let (w, h) = (lis.size.0 as i32, lis.size.1 as i32);
    let at = *lis;
    let s = snap.clone();
    let screen = move |x: i32, y: i32| {
        let n = y as usize * size + x as usize;
        at.screen(s4_viewer::terrain::node_pos(x as usize, y as usize, s.population.height.get(n).copied().unwrap_or(0)))
    };
    let origin = at.screen([0.0, 0.0]);
    let fog = snap.fog.as_ref().map(|f| {
        let f = f.clone();
        Arc::new(move |x: i32, y: i32| f.is_position_visible(size, x, y)) as Arc<dyn Fn(i32, i32) -> bool + Send + Sync>
    });
    s4_audio::View {
        width: w,
        height: h,
        origin,
        triangle_px: TRIANGLE_PX * lis.camera.zoom,
        map_size: size as i32,
        fog,
        screen: Some(Arc::new(screen)),
    }
}

/// `IGfxEngine::GetClosestMapPoint(w/2, h/2)`: the node drawn at the output centre.
pub fn centre_node(pop: &Population, lis: &Listener) -> Option<(i32, i32)> {
    let size = pop.size;
    let height = |x: i32, y: i32| pop.height.get(y.clamp(0, size as i32 - 1) as usize * size + x.clamp(0, size as i32 - 1) as usize).copied().unwrap_or(0);
    let (wx, wy) = lis.world_centre();
    s4_viewer::picking::search_map_point(size, &height, wx.floor() as i32, wy.floor() as i32)
}

/// The node `CLogic::HandleSoundtracks` (0x4f2bc0) tests: from the view position
/// (`GameManager +0x44 / +0x48`, pixels at the current zoom) without terrain height,
/// `y = py / (zoom/2)`, `x = px / zoom + y/2`, `None` unless both are below the map size
/// (unsigned). [inf] `+0x44 / +0x48` is the view centre (0x5134ea sets it from the start
/// position, on which the view is centred).
pub fn music_centre(size: usize, lis: &Listener) -> Option<(usize, usize)> {
    let (wx, wy) = lis.world_centre();
    let y = wy.floor() as i32 / (TRIANGLE_PX as i32 / 2);
    let x = wx.floor() as i32 / TRIANGLE_PX as i32 + y / 2;
    ((x as u32 as usize) < size && (y as u32 as usize) < size).then_some((x as usize, y as usize))
}

/// `CAIResourceData::Flags1` of the 16×16 square containing node (x, y), as `sub_43BA20`
/// (release 0x43ba20) computes it from the ground types (gfx map byte 1, high nibble) of its 256
/// nodes: 0x01 not all water, 0x02 any water (0x0_), 0x08 desert (0x4_), 0x10 swamp (0x5_), 0x04
/// river (0x6_), 0x20 type 0x7_, 0x40 snow (0x8_). The AI map keeps it per square and recomputes
/// a square when its ground changes (0x43c3c0 from `CAIMain`, 0x4386fa); computing it on demand
/// from the current ground gives the same byte.
pub fn ambient_flags(pop: &Population, x: i32, y: i32) -> u8 {
    let size = pop.size;
    let (sx, sy) = ((x.max(0) as usize >> 4) << 4, (y.max(0) as usize >> 4) << 4);
    let mut count = [0u32; 16];
    for row in sy..(sy + 16).min(size) {
        for col in sx..(sx + 16).min(size) {
            count[(pop.ground.get(row * size + col).copied().unwrap_or(0) >> 4) as usize] += 1;
        }
    }
    let bit = |n: usize, b: u8| if count[n] != 0 { b } else { 0 };
    bit(8, 0x40) | bit(7, 0x20) | bit(6, 0x04) | bit(5, 0x10) | bit(4, 0x08) | bit(0, 0x02) | u8::from(count[0] != 256)
}

/// The client-side work-sound chance with the MSVC `rand()`.
pub struct JobSoundRoll {
    state: u32,
}

impl JobSoundRoll {
    pub fn new(seed: u32) -> Self {
        JobSoundRoll { state: seed }
    }

    /// Seeded as the original's CRT `rand()` is: `srand(timeGetTime())` once at startup
    /// (0x45c5e0). Note: the original then reseeds the same CRT state while drawing,
    /// `srand(y·width + x)` per wave node in the wave helper (0x663af0), so the roll a job sound
    /// gets depends on the last frame's visible water; that is not reproduced (the roll is not
    /// part of the simulation, so only its rate matters).
    /// The CRT state (seeds the patches' own `rand()`, patches.rs).
    pub fn state(&self) -> u32 {
        self.state
    }

    pub fn from_clock() -> Self {
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u32);
        Self::new(ms)
    }

    fn rand(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.state >> 16) & 0x7fff
    }

    /// The animals' roll (0x52bc5f): 100 plays without drawing a number, otherwise
    /// `rand() % 100 < interval` (strictly less, unlike the job sounds).
    pub fn below(&mut self, interval: u8) -> bool {
        interval == 100 || self.rand() % 100 < interval as u32
    }

    /// Whether a job sound with `random` (percent) plays this time: 100 plays without drawing
    /// a number, otherwise `random >= rand() % 100` (0x586b41..0x586b58).
    pub fn roll(&mut self, random: i32) -> bool {
        random == 100 || (self.rand() % 100) as i32 <= random
    }
}

/// What happened to the sounds of one `SIV_SOUNDS` id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdCount {
    /// Requests that reached `PlaySoundFX`.
    pub requested: u32,
    /// Started on a voice with volume > 0.
    pub audible: u32,
    /// Started on a voice at volume 0 (fogged, off-screen left or above, too far).
    pub mute: u32,
    /// All voices busy.
    pub dropped: u32,
    /// Unmapped id.
    pub silent: u32,
    /// Sum of the Miles volumes of the audible ones.
    pub volume_sum: u64,
}

/// Counters of the sound decisions, for logs and tests.
#[derive(Clone, Debug, Default)]
pub struct SoundLog {
    pub per_id: BTreeMap<u16, IdCount>,
    pub job_events: u64,
    /// Of those, from the draw-time `Update`.
    pub job_drawn: u64,
    /// Job sounds that lost the `random` roll.
    pub job_not_rolled: u64,
    /// Job sounds on fogged nodes (`IsPositionVisible` false).
    pub job_fogged: u64,
    pub effects: u64,
    /// Effect sounds not played because the effect was not drawn at its start tick.
    pub effects_not_drawn: u64,
    pub war_actions: u64,
    /// Building patch sounds (patches.rs).
    pub patch_sounds: u64,
    /// Animal sounds ([`animals`]).
    pub animal_sounds: u64,
    /// Manakopter rotor sounds started ([`rotor`]).
    pub rotor_sounds: u64,
    /// Bird and seagull sounds ([`crate::ambient`]).
    pub ambient_sounds: u64,
    pub unported: u64,
    pub style_changes: u32,
    /// Ambient loops: ticks running and the highest volume, per sound id.
    pub ambient: BTreeMap<u16, (u64, i32)>,
    /// Ticks per music style (`SIV_MUSICSTYLES` index).
    pub styles: [u64; 6],
    pub ticks: u64,
}

impl SoundLog {
    fn played(&mut self, id: SoundId, p: Played) {
        let c = self.per_id.entry(id.0).or_default();
        c.requested += 1;
        match p {
            Played::Voice { volume, .. } if volume > 0 => {
                c.audible += 1;
                c.volume_sum += volume as u64;
            }
            Played::Voice { .. } => c.mute += 1,
            Played::NoVoice => c.dropped += 1,
            Played::Silent => c.silent += 1,
        }
    }

    /// One line per id, then the totals.
    pub fn report(&self) -> String {
        let mut s = String::new();
        for (id, c) in &self.per_id {
            let mean = if c.audible > 0 { c.volume_sum as f64 / c.audible as f64 } else { 0.0 };
            s += &format!(
                "  {:3} {:32} requested {:5}  audible {:5} (mean vol {:5.1})  vol 0 {:5}  no voice {:4}  unmapped {:4}\n",
                id,
                SoundId(*id).name(),
                c.requested,
                c.audible,
                mean,
                c.mute,
                c.dropped,
                c.silent
            );
        }
        for (id, (ticks, max)) in &self.ambient {
            s += &format!("  {:3} {:32} ambient loop for {} ticks, up to volume {}
", id, SoundId(*id).name(), ticks, max);
        }
        s += &format!("  music ticks per style (SETTLE, FIGHT, SEA, DARK): {:?}
", &self.styles[..4]);
        s += &format!(
            "  {} ticks; job sound events {} ({} at draw time; lost the roll {}, fogged {}); effects {} (not drawn {}); war actions {}; patch sounds {}; animal sounds {}; rotor sounds {}; bird / seagull sounds {}; style changes {}; unported calls {}",
            self.ticks,
            self.job_events,
            self.job_drawn,
            self.job_not_rolled,
            self.job_fogged,
            self.effects,
            self.effects_not_drawn,
            self.war_actions,
            self.patch_sounds,
            self.animal_sounds,
            self.rotor_sounds,
            self.ambient_sounds,
            self.style_changes,
            self.unported
        );
        s
    }
}

/// The nodes the object pass visits in one frame (`DrawObjects`, release 0x660070, with the view
/// set by 0x658ad0). With the triangle width `zA` and row height `zB` in 16.16 pixels and the
/// scroll position `(sx, sy)` in output pixels: `vx = (sx << 15) / (zA/2)`, `vy = (sy << 15) /
/// (zB/2)`, `across = ((w << 14) + zA)·4 / zA` (the left offset 0x116923c is 0 outside the
/// miniview, 0x65a6c0), `rows = ((h << 16) + zB) / zB`.
/// * Rows `vy − 7 ..< end + 11`, `end = vy + 2 + rows`, plus `maxh[end] >> 3` when `0 < end <
///   size` (0x145baa8: the highest node of each row, 0x6589d0); only rows inside the map.
/// * In row y the nodes `y/2 + vx − 30 + odd ..< y/2 + vx + across + 9 + odd` inside the map
///   (`odd` = y & 1).
/// * A `m_cMapObjectMap` entry (settler, building, animal) gets its `GetGfxInfo` /
///   `FillGfxInfo` call when the node's object fog is above 59 (0x6606b8 `cmp 0x3b; jbe`).
/// * The effects (`CEffects` vtbl+0 = 0x4e3310 at 0x66033a, then vtbl+4 = 0x4e33b0 with each
///   drawn row): the 16-node squares of the drawn rows, columns `(x0 >> 4) − 1 ..= (x1 >> 4) + 1`
///   within 0..=63, `x0 = y0/2 + vx − 30`, `x1 = vx + y0/2 + (end + 11 − y0)/2 + x0 + across + 9`
///   (the original adds `vx + y0/2` twice), both clamped to 0..=0x3ff. No fog test.
#[derive(Clone)]
pub struct DrawRange {
    vx: i32,
    vy: i32,
    across: i32,
    end: i32,
    size: i32,
    /// Object fog above 59 per node (`None`: no fog).
    solid: Option<Arc<dyn Fn(i32, i32) -> bool + Send + Sync>>,
}

impl DrawRange {
    pub fn new(snap: &Arc<Snapshot>, lis: &Listener) -> DrawRange {
        let pop = &snap.population;
        let size = pop.size as i32;
        let za = ((TRIANGLE_PX * lis.draw_zoom * 65536.0) as i64).max(4);
        let zb = za / 2;
        let (sx, sy) = ((lis.camera.x * lis.draw_zoom).floor() as i64, (lis.camera.y * lis.draw_zoom).floor() as i64);
        let vx = ((sx << 15) / (za / 2)) as i32;
        let vy = ((sy << 15) / (zb / 2)) as i32;
        let across = ((((lis.size.0 as i64) << 14) + za) * 4 / za) as i32;
        let rows = ((((lis.size.1 as i64) << 16) + zb) / zb) as i32;
        let mut end = vy + 2 + rows;
        if 0 < end && end < size {
            let row = pop.height.get(end as usize * pop.size..(end as usize + 1) * pop.size).unwrap_or(&[]);
            end += (row.iter().copied().max().unwrap_or(0) >> 3) as i32;
        }
        let solid = snap.fog.as_ref().map(|f| {
            let (gfx, fade, border, n) = (f.gfx.clone(), f.fade, f.border, pop.size);
            Arc::new(move |x: i32, y: i32| {
                let g = gfx.get(y as usize * n + x as usize).copied().unwrap_or(0) as u32;
                s4_viewer::terrain::object_fog(s4_viewer::terrain::row_fade(y as usize, fade, border), g & 7, (g >> 3) & 7) > 59
            }) as Arc<dyn Fn(i32, i32) -> bool + Send + Sync>
        });
        DrawRange { vx, vy, across, end, size, solid }
    }

    fn rows(&self) -> std::ops::Range<i32> {
        (self.vy - 7).max(0)..(self.end + 11).min(self.size)
    }

    /// Whether the object pass visits node (x, y).
    pub fn visits(&self, x: i32, y: i32) -> bool {
        let (half, odd) = (y / 2, y & 1);
        self.rows().contains(&y) && x >= 0 && x < self.size && x >= half + self.vx - 30 + odd && x < half + self.vx + self.across + 9 + odd
    }

    /// Whether the map-object entry at (x, y) gets its `GetGfxInfo` call this frame.
    pub fn object_drawn(&self, x: i32, y: i32) -> bool {
        self.visits(x, y) && self.solid.as_ref().is_none_or(|f| f(x, y))
    }

    /// Whether `CEffects` iterates the effects of node (x, y)'s square this frame.
    pub fn effect_drawn(&self, x: i32, y: i32) -> bool {
        let y0 = self.vy - 7;
        let x0 = y0 / 2 + self.vx - 30;
        let x1 = self.vx + y0 / 2 + (self.end + 11 - y0) / 2 + x0 + self.across + 9;
        let (c0, c1) = (x0.clamp(0, 0x3ff) >> 4, x1.clamp(0, 0x3ff) >> 4);
        let (first, last) = (if c0 < 1 { c0 } else { c0 - 1 }, if c1 > 0x3e { c1 } else { c1 + 1 });
        let r = self.rows();
        !r.is_empty() && (r.start >> 4..=(r.end - 1) >> 4).contains(&(y >> 4)) && (first..=last).contains(&(x >> 4))
    }
}

/// `jobSoundList.xml` by settler gfx job (`SIV_JOBS`): `(sound, frame, random)`, sound 0 = none
/// (`CEntityToDoListMgr`'s table at release 0x129b0c0: i32 sound, u8 random, u8 frame).
pub fn job_sounds(data: &s4_formats::gamedata::GameData) -> Vec<(u16, u8, u8)> {
    let mut t = vec![(0, 0, 0); 4096];
    for j in &data.job_sounds {
        if let Some(slot) = t.get_mut(j.job as usize) {
            *slot = (j.sound.map_or(0, |v| v as u16), j.frame.clamp(0, 255) as u8, j.random.clamp(0, 255) as u8);
        }
    }
    t
}

/// A drawn settler's animation as the draw-time `Update` keeps it.
#[derive(Clone, Copy, Debug)]
struct DrawTrack {
    job: u16,
    task: u8,
    cycle: u8,
    forward: bool,
    frame: u8,
    tick: u32,
}

/// The tasks whose `Update` plays the gfx job and checks the job sound (0x586a42 jump table).
fn animated(task: u8) -> bool {
    matches!(task, 1 | 3 | 13 | 16 | 25 | 28..=31)
}

/// `kira`'s manager on the default output device, made on a short-lived thread of its own.
///
/// cpal (WASAPI) initializes COM on the calling thread and activates the default endpoint there
/// (`default_output_config`); kira then runs the stream on a thread it spawns. Made on the UI
/// thread, the outcome depends on what that thread's COM apartment already holds (winit's OLE
/// STA, IME / text services, other libraries) — a user session failed with RPC_E_CHANGED_MODE
/// ("Failed to get audio client"). A fresh thread has no COM state; a failure is retried once
/// after 250 ms on another one (transient device changes).
///
/// Off (an error, so every caller runs silent) in test builds and when `S4RS_NO_AUDIO` is set
/// to anything but `0`: test launches and agents' QA runs must not touch the user's sound
/// device. [ours]
pub fn new_manager() -> anyhow::Result<kira::AudioManager> {
    if audio_off() {
        anyhow::bail!("audio off (test build or S4RS_NO_AUDIO)");
    }
    open_manager()
}

/// Whether the sound device stays closed: see [`new_manager`].
pub fn audio_off() -> bool {
    cfg!(test) || std::env::var_os("S4RS_NO_AUDIO").is_some_and(|v| v != "0")
}

fn open_manager() -> anyhow::Result<kira::AudioManager> {
    fn attempt() -> anyhow::Result<kira::AudioManager> {
        let open = || kira::AudioManager::<kira::DefaultBackend>::new(kira::AudioManagerSettings::default()).map_err(|e| e.to_string());
        let r = std::thread::Builder::new().name("audio-open".into()).spawn(open)?.join().map_err(|_| anyhow::anyhow!("audio open thread panicked"))?;
        r.map_err(|e| anyhow::anyhow!("audio device: {e}"))
    }
    attempt().or_else(|e| {
        tracing::warn!("{e}; retrying");
        std::thread::sleep(std::time::Duration::from_millis(250));
        attempt()
    })
}

/// The audio service on the default output device, or `None` (logged) without one.
pub fn open_device() -> Option<Audio> {
    new_manager()
        .and_then(|m| Ok(Audio::new(m, s4_formats::game_dir())?))
        .inspect_err(|e| tracing::warn!("no audio: {e}"))
        .ok()
}

/// The start screens' music. `OpenMainMenu` (release 0x522540) and the end statistics
/// (0x512c50), if `Snd\Romans*.mp3` exists and the MENU playlists are not loaded yet
/// (`CSoundManager +0xc != 5`): load them (`sub_50C8C0(5)`) and start STARTMENU title 0
/// (`StartPlaylist(4, music volume)`); otherwise the music goes on. The campaign briefing does
/// not stop it; starting the mission does (`CStateBriefing` event 0x70, 0x510a08: all streams and
/// samples stopped), then the game starts its race's music.
pub struct MenuMusic {
    audio: Audio,
    last: std::time::Instant,
}

impl MenuMusic {
    /// Keep `slot` in step with the screens: started while `shown`, dropped (stopped) otherwise;
    /// advance its fades and playlist.
    pub fn follow(slot: &mut Option<MenuMusic>, shown: bool, settings: &crate::settings::Settings) {
        if !shown {
            *slot = None;
            return;
        }
        if slot.is_none() {
            let Some(mut audio) = open_device() else { return };
            audio.set_volumes(settings.sound_level(), settings.music_level(), false);
            if let Err(e) = audio.set_music_state(Race::Menu, MusicStyle::StartMenu) {
                tracing::warn!("menu music: {e}");
            }
            *slot = Some(MenuMusic { audio, last: std::time::Instant::now() });
        }
        if let Some(m) = slot {
            let now = std::time::Instant::now();
            if let Err(e) = m.audio.update(now - m.last) {
                tracing::debug!("audio: {e}");
            }
            m.last = now;
        }
    }

    /// The credits screen (state 0x511cf0) shown or left: its own playlist (5) on entering,
    /// every stream stopped on leaving (the destructor 0x511ab6) and the menu's playlist (4)
    /// again, as the main menu's init does after it (0x512d26).
    pub fn credits(slot: &mut Option<MenuMusic>, on: bool) {
        let Some(m) = slot else { return };
        let r = if on {
            m.audio.switch_menu_playlist(MusicStyle::Credits)
        } else {
            m.audio.stop_menu_music();
            m.audio.switch_menu_playlist(MusicStyle::StartMenu)
        };
        if let Err(e) = r {
            tracing::warn!("menu music: {e}");
        }
    }

    /// The start-screen procedures' GUI sounds (`Frontend::take_sounds`).
    pub fn play(slot: &mut Option<MenuMusic>, ids: &[u16]) {
        let Some(m) = slot else { return };
        for &id in ids {
            if let Err(e) = m.audio.handle(&AudioEvent::Ui { id: SoundId(id) }) {
                tracing::debug!("audio: {e}");
            }
        }
    }
}

/// The sound of a running game.
pub struct GameSound<B: Backend = kira::DefaultBackend> {
    pub audio: Audio<B>,
    roll: JobSoundRoll,
    pub log: SoundLog,
    jobs: Vec<(u16, u8, u8)>,
    tracks: HashMap<u16, DrawTrack>,
    /// Settlers drawn in the last frame: their animation (and job sound) runs at draw time.
    drawn: HashSet<u16>,
    /// The land animals' draw-time sounds ([`animals`]).
    animals: animals::AnimalSounds,
    /// The manakopters' rotor handles and drawn flags ([`rotor`]).
    rotors: rotor::Rotors,
    /// `SimEvent::EffectLater`s not started yet: (start tick, sound, x, y).
    later: Vec<(u32, u16, i32, i32)>,
}

/// The ambient creatures' view of the sound side ([`crate::ambient::Sink`]): the game owns the
/// creatures (`crate::ambient::Ambient`) and ticks them with this after [`GameSound::tick`].
/// Without sound (no device, headless without `--sound`) only the screen test remains.
pub struct AmbientSink<'a, B: Backend> {
    snd: Option<&'a mut GameSound<B>>,
    view: s4_audio::View,
    range: DrawRange,
}

impl<'a, B: Backend> AmbientSink<'a, B> {
    pub fn new(snd: Option<&'a mut GameSound<B>>, snap: &Arc<Snapshot>, lis: &Listener) -> Self {
        AmbientSink { snd, view: listener(snap, lis), range: DrawRange::new(snap, lis) }
    }
}

impl<B: Backend> crate::ambient::Sink for AmbientSink<'_, B> {
    fn playing(&self, v: s4_audio::VoiceId) -> bool {
        self.snd.as_ref().is_some_and(|s| s.audio.voice_playing(v))
    }
    fn roll(&mut self, interval: u8) -> bool {
        self.snd.as_mut().is_some_and(|s| s.roll.below(interval))
    }
    fn visible(&self, x: i32, y: i32) -> bool {
        self.view.fog.as_ref().is_none_or(|f| f(x, y))
    }
    fn play(&mut self, sound: u16, x: i32, y: i32) -> Option<s4_audio::VoiceId> {
        let s = self.snd.as_mut()?;
        s.log.ambient_sounds += 1;
        let id = SoundId(sound);
        match s.audio.handle(&AudioEvent::Fx { id, x, y }) {
            Ok(Some(p)) => {
                s.log.played(id, p);
                s.audio.last_voice()
            }
            Ok(None) => None,
            Err(e) => {
                tracing::debug!("audio: {e}");
                None
            }
        }
    }
    /// The vanish effect: its sound plays like a sim effect's, when its square is drawn.
    fn effect(&mut self, _effect: u16, sound: u16, x: i32, y: i32) {
        let Some(s) = self.snd.as_mut() else { return };
        s.log.effects += 1;
        if sound == 0 {
        } else if self.range.effect_drawn(x, y) {
            let id = SoundId(sound);
            if let Ok(Some(p)) = s.audio.handle(&AudioEvent::Fx { id, x, y }) {
                s.log.played(id, p);
            }
        } else {
            s.log.effects_not_drawn += 1;
        }
    }
    fn on_screen(&self, x: i32, y: i32) -> bool {
        self.view.probe(x, y).screen.is_some_and(|(sx, sy)| sx >= 0 && sy >= 0 && sx < self.view.width && sy < self.view.height)
    }
}

impl<B: Backend> GameSound<B> {
    pub fn new(audio: Audio<B>, roll: JobSoundRoll) -> Self {
        let data = s4_formats::gamedata::GameData::load_default().inspect_err(|e| tracing::warn!("job sounds, patches: {e}")).ok();
        let jobs = data.as_ref().map(job_sounds).unwrap_or_default();
        GameSound { audio, roll, log: SoundLog::default(), jobs, tracks: HashMap::new(), drawn: HashSet::new(), animals: animals::AnimalSounds::load(), rotors: rotor::Rotors::default(), later: Vec::new() }
    }

    /// A job sound due at (x, y): the `random` roll, `IsPositionVisible`, `PlayEnvironmentSound`.
    fn job_sound(&mut self, view: &s4_audio::View, sound: u16, chance: u8, x: i32, y: i32) {
        if !self.roll.roll(chance as i32) {
            self.log.job_not_rolled += 1;
        } else if !view.fog.as_ref().is_none_or(|f| f(x, y)) {
            self.log.job_fogged += 1;
        } else {
            self.handle(AudioEvent::Fx { id: SoundId(sound), x, y });
        }
    }

    /// One drawn frame: the draw-time `ISettlerRole::Update` of every settler the renderer draws
    /// (`GetGfxInfo`) and of the house workers their buildings draw. Each advances its animated
    /// task by the ticks since its last update (so at high game speed frames are skipped, as in
    /// the original), plays the job sound when it lands on the sound's frame and, for a warrior,
    /// signals the war action (fight music). A settler seen
    /// for the first time, or whose job changed, takes the snapshot's frame without a check.
    pub fn draw(&mut self, snap: &Arc<Snapshot>, lis: &Listener) {
        let pop = &snap.population;
        let tick = pop.tick;
        let view = listener(snap, lis);
        self.audio.set_listener(view.clone());
        let range = DrawRange::new(snap, lis);
        // A settler is drawn at its node; a house worker by its building's `FillGfxInfo`.
        let units: Vec<(&Unit, u16, u16)> = pop
            .settlers
            .iter()
            .filter(|u| u.id != 0 && u.drawn())
            .map(|u| (u, u.x, u.y))
            .chain(pop.buildings.iter().filter(|b| b.id != 0).filter_map(|b| b.worker.as_ref().map(|w| (w, b.x, b.y))))
            .collect();
        let mut drawn = HashSet::with_capacity(units.len());
        for (u, nx, ny) in units {
            if !range.object_drawn(nx as i32, ny as i32) {
                continue;
            }
            drawn.insert(u.id);
            if !animated(u.task) || u.cycle == 0 {
                self.tracks.remove(&u.id);
                continue;
            }
            let fresh = DrawTrack { job: u.job, task: u.task, cycle: u.cycle, forward: u.forward, frame: u.frame, tick };
            let tr = self.tracks.entry(u.id).or_insert(fresh);
            if (tr.job, tr.task, tr.cycle, tr.forward) != (u.job, u.task, u.cycle, u.forward) {
                *tr = fresh;
                continue;
            }
            // `(u16)GetTickCounter() - LastUpdateTick()`
            let n = tick.wrapping_sub(tr.tick) & 0xffff;
            if n == 0 {
                continue;
            }
            let (c, f) = (tr.cycle as u32, tr.frame as u32);
            tr.frame = if tr.forward { ((f + n) % c) as u8 } else { ((f + c - n % c) % c) as u8 };
            tr.tick = tick;
            let frame = tr.frame;
            let (sound, at, random) = self.jobs.get(u.job as usize).copied().unwrap_or_default();
            if sound > 0 && at == frame {
                self.log.job_events += 1;
                self.log.job_drawn += 1;
                self.job_sound(&view, sound, random, u.x as i32, u.y as i32);
            }
            // `CLogic::SetWarAction` for a warrior of type 2..=5 (0x586b8f..).
            let warrior = u.flags & s4_sim::entity::flag::WARRIOR_TYPE_MASK;
            if (1u32 << warrior) & 0x3C != 0 {
                self.log.war_actions += 1;
                self.handle(AudioEvent::WarAction);
            }
        }
        self.tracks.retain(|id, _| drawn.contains(id));
        self.drawn = drawn;
        self.draw_animals(pop, &view, &range);
        self.rotors.draw(|x, y| range.effect_drawn(x, y));
    }

    /// The building patches' sounds due (patches.rs, stepped by the game for the drawing too):
    /// `FillGfxInfo`'s steps of the drawn buildings and `TakeJobTrigger`'s frame-0 sounds.
    pub fn patch_sounds(&mut self, due: &[crate::patches::Due]) {
        for d in due {
            self.log.patch_sounds += 1;
            self.handle(AudioEvent::Fx { id: SoundId(d.sound), x: d.x, y: d.y });
        }
    }

    /// The drawn land animals' `CAnimal` draw-time update ([`animals`]). Hidden animals are not
    /// skipped: the object pass calls `GetGfxInfo` (0x56dac0 → vtbl+0x14 `FillGfxInfo` 0x52bb30)
    /// for every map object above object fog 59, `FillGfxInfo` runs the update (vtbl+0x78
    /// 0x52bb90) first and only then sets the gfx info's visible byte (`(flags & 0x80100) ==
    /// 0x100`), which `DrawObjects` (0x6606d7) tests to skip the drawing.
    fn draw_animals(&mut self, pop: &Population, view: &s4_audio::View, range: &DrawRange) {
        let tick = pop.tick;
        let seen: Vec<animals::Seen> = pop
            .animals
            .iter()
            .filter(|a| range.object_drawn(a.x as i32, a.y as i32))
            .map(|a| animals::Seen { id: a.id, kind: a.kind, task: a.task_type, state5: a.state == 5, x: a.x as i32, y: a.y as i32 })
            .collect();
        let ids: HashSet<u16> = seen.iter().map(|a| a.id).collect();
        self.animals.retain(|id| ids.contains(&id));
        for a in &seen {
            let audio = &self.audio;
            let roll = &mut self.roll;
            let fog = view.fog.as_ref();
            let Some(sound) = self.animals.update(a, tick, &mut |i| roll.below(i), |v| audio.voice_playing(v), |x, y| fog.is_none_or(|f| f(x, y))) else { continue };
            self.log.animal_sounds += 1;
            self.handle(AudioEvent::Fx { id: SoundId(sound), x: a.x, y: a.y });
            let v = self.audio.last_voice();
            self.animals.played(a.id, v);
        }
    }

    /// `CStateGame` start (0x5155b0): `PlayBackgroundMusic(race of the local player, SETTLE)`
    /// (0x50d2d0): the race's playlists and MENU's, title 0 of SETTLE at full music volume. The
    /// original first checks that `Snd\Romans*.mp3` exists; a missing file here just means no
    /// stream. "User MP3" (settings +0x13f → 0x50c5b0) is not offered.
    pub fn start(&mut self, race: u8) {
        let Some(race) = Race::from_index(race).filter(|r| *r != Race::Menu) else { return };
        if let Err(e) = self.audio.set_music_state(race, MusicStyle::Settle) {
            tracing::warn!("music: {e}");
        }
    }

    fn handle(&mut self, ev: AudioEvent) {
        match self.audio.handle(&ev) {
            Ok(Some(p)) => {
                if let AudioEvent::Fx { id, .. } | AudioEvent::Ui { id } = ev {
                    self.log.played(id, p);
                }
            }
            Ok(None) => {}
            Err(e) => tracing::debug!("audio: {e}"),
        }
    }

    /// An effect starting this tick (0x4e33b0): its sound if it has one and its square is drawn.
    fn effect_sound(&mut self, range: &DrawRange, sound: u16, x: i32, y: i32) {
        self.log.effects += 1;
        if sound == 0 {
        } else if range.effect_drawn(x, y) {
            self.handle(AudioEvent::Fx { id: SoundId(sound), x, y });
        } else {
            self.log.effects_not_drawn += 1;
        }
    }

    /// One logic tick: its sim events, then `Sound::Update` and `HandleSoundtracks`, against
    /// `snap` (the world after the tick) and the camera `lis`.
    pub fn tick(&mut self, events: &[SimEvent], snap: &Arc<Snapshot>, lis: &Listener) {
        let view = listener(snap, lis);
        let range = DrawRange::new(snap, lis);
        self.audio.set_listener(view.clone());
        let pop = &snap.population;
        // Delayed effects whose start tick has come: their sound as an effect starting now.
        let due: Vec<_> = self.later.iter().filter(|l| l.0 <= pop.tick).copied().collect();
        self.later.retain(|l| l.0 > pop.tick);
        for (_, sound, x, y) in due {
            self.effect_sound(&range, sound, x, y);
        }
        for e in events {
            match *e {
                // A drawn settler's `Update` runs at draw time ([`GameSound::draw`]), so the
                // logic call finds no ticks to advance.
                SimEvent::JobSound { settler, .. } if self.drawn.contains(&settler) => {}
                SimEvent::JobSound { x, y, sound, chance, .. } => {
                    self.log.job_events += 1;
                    self.job_sound(&view, sound, chance, x, y);
                }
                SimEvent::Effect { sound, x, y, .. } | SimEvent::TribeEffect { sound, x, y, .. } => self.effect_sound(&range, sound, x, y),
                SimEvent::EffectLater { sound, x, y, delay, .. } => {
                    if delay == 0 {
                        self.effect_sound(&range, sound, x, y);
                    } else {
                        self.later.push((pop.tick + delay, sound, x, y));
                    }
                }
                SimEvent::Sound { sound, x, y } => {
                    if view.fog.as_ref().is_none_or(|f| f(x, y)) {
                        self.handle(AudioEvent::Fx { id: SoundId(sound), x, y });
                    }
                }
                SimEvent::WarAction => {
                    self.log.war_actions += 1;
                    self.handle(AudioEvent::WarAction);
                }
                SimEvent::Unported { .. } => self.log.unported += 1,
                // The message list's sounds come from `messages.rs` ([`GameSound::ui`]).
                SimEvent::Message { .. } | SimEvent::Attacked { .. } => {}
                // `TakeJobTrigger` (patches.rs): the game applies it to its patches and hands a
                // sound on the patch's frame 0 to [`GameSound::patch_sounds`] at once
                // (`PlayEnvironmentSound` without a visibility test).
                SimEvent::JobTrigger { .. } => {}
                // The AI's / script's notice of a spell; a spell's sound is its effect's.
                SimEvent::SpellCast { .. } => {}
                // A flying effect has no sound (0x4e2310 clears its byte 2).
                SimEvent::Projectile { .. } => {}
                // The ambient manager makes it (`ambient::Ambient::butterfly`); its sound is the
                // butterfly's own (none).
                SimEvent::Butterfly { .. } => {}
                SimEvent::Rotor { id, x, y, ordered } => {
                    let audio = &self.audio;
                    let acts = self.rotors.update(id, (x, y), ordered, |x, y| range.effect_drawn(x, y), |v| audio.voice_playing(v));
                    for a in acts {
                        match a {
                            rotor::Act::Stop(v) => self.audio.stop_voice(v),
                            rotor::Act::Play { sound, looped } => {
                                self.log.rotor_sounds += 1;
                                let played = if looped { self.audio.play_fx_looped(SoundId(sound), x, y) } else { self.audio.play_fx(SoundId(sound), x, y) };
                                match played {
                                    Ok(p) => self.log.played(SoundId(sound), p),
                                    Err(e) => tracing::debug!("audio: {e}"),
                                }
                                let v = self.audio.last_voice();
                                self.rotors.played(id, sound, v);
                            }
                        }
                    }
                }
                SimEvent::RotorStop { id } => {
                    for v in self.rotors.delete(id) {
                        self.audio.stop_voice(v);
                    }
                }
            }
        }
        // `Animal::Update`'s ambient creatures: the game ticks them after this with an
        // [`AmbientSink`] (the CRT rolls of their sounds come after the job sounds').
        let centre = centre_node(pop, lis);
        if let Some((x, y)) = centre {
            self.handle(AudioEvent::Ambient { x, y, flags: ambient_flags(pop, x, y) });
        }
        // `m_cFlagMap` bit 0x04 (dark land, changed at run time by `CDarkTribe`).
        let dark = music_centre(pop.size, lis).map(|(x, y)| pop.flags.get(y * pop.size + x).is_some_and(|f| f & 0x04 != 0));
        let before = self.audio.stats.style_changes;
        self.handle(AudioEvent::LogicTick { centre_dark: dark });
        self.log.style_changes += self.audio.stats.style_changes - before;
        self.log.ticks += 1;
        for (id, v) in self.audio.ambient_loops() {
            let a = self.log.ambient.entry(id.0).or_default();
            a.0 += 1;
            a.1 = a.1.max(v);
        }
        self.log.styles[self.audio.music_style() as usize] += 1;
    }

    /// The dialog procedures' calls since the last frame (`GuiState::calls`): their GUI sounds
    /// ([`gui`]).
    pub fn gui(&mut self, procs: gui::Procs, calls: &[UiEvent]) {
        for c in calls {
            tracing::debug!("gui call {c:?}: sounds {:?}", gui::sounds(procs, c));
            for &id in gui::sounds(procs, c) {
                self.handle(AudioEvent::Ui { id: SoundId(id) });
            }
        }
    }

    /// GUI sounds (`PlaySound(id, 100, 64)`) from elsewhere: the message window (`messages.rs`).
    pub fn ui(&mut self, ids: &[u16]) {
        for &id in ids {
            self.handle(AudioEvent::Ui { id: SoundId(id) });
        }
    }

    /// Wall-clock time passed (fades, playlist successors).
    pub fn frame(&mut self, dt: std::time::Duration) {
        if let Err(e) = self.audio.update(dt) {
            tracing::debug!("audio: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut std::ffi::c_void, coinit: u32) -> i32;
        fn OleInitialize(reserved: *mut std::ffi::c_void) -> i32;
    }

    /// The audio device opens whatever COM state the calling thread is in: none, OLE (winit's
    /// drag and drop), STA, MTA. Skipped on a machine without an output device. Ignored: it opens
    /// the real sound device (run with `--ignored` on request).
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn new_manager_ignores_the_callers_com_apartment() {
        if open_manager().is_err() {
            eprintln!("skipping: no audio output device");
            return;
        }
        for (name, init) in [("ole", 0u8), ("sta", 1), ("mta", 2)] {
            let r = std::thread::spawn(move || {
                // SAFETY: plain COM initialization of this test thread (never uninitialized, the
                // thread ends right after).
                let hr = unsafe {
                    match init {
                        0 => OleInitialize(std::ptr::null_mut()),
                        1 => CoInitializeEx(std::ptr::null_mut(), 2),
                        _ => CoInitializeEx(std::ptr::null_mut(), 0),
                    }
                };
                assert!(hr >= 0, "{name}: COM init {hr:#x}");
                open_manager().map(|_| ()).map_err(|e| e.to_string())
            })
            .join()
            .unwrap();
            assert!(r.is_ok(), "{name}: {r:?}");
        }
    }

    #[test]
    fn roll_rate_matches_percentage() {
        let mut r = JobSoundRoll::new(1);
        let hits = (0..10_000).filter(|_| r.roll(29)).count();
        // rand()%100 <= 29 is 30 of 100 values.
        assert!((2800..3200).contains(&hits), "{hits}");
        // 100 plays without drawing a number.
        let s = r.state;
        assert!((0..100).all(|_| r.roll(100)));
        assert_eq!(r.state, s);
    }

    #[test]
    fn ambient_flags_from_ground() {
        let mut pop = Population { size: 32, ..Default::default() };
        pop.ground = vec![16; 32 * 32];
        assert_eq!(ambient_flags(&pop, 3, 3), 0x01);
        pop.ground[5 * 32 + 7] = 3; // water
        pop.ground[6 * 32 + 8] = 97; // river
        pop.ground[2 * 32 + 20] = 64; // desert, in the next square
        assert_eq!(ambient_flags(&pop, 15, 15), 0x01 | 0x02 | 0x04);
        assert_eq!(ambient_flags(&pop, 16, 0), 0x01 | 0x08);
        pop.ground = vec![7; 32 * 32];
        assert_eq!(ambient_flags(&pop, 0, 0), 0x02);
    }

    /// A 64×64 flat map, the camera centred on node (32, 32) of an 800×600 output at zoom 1,
    /// `fogged` = every node at fog level 7.
    fn scene(fogged: bool) -> (Arc<Snapshot>, Listener) {
        scene_sized(64, fogged)
    }

    /// A `side`×`side` flat map, the camera as in [`scene`].
    fn scene_sized(side: usize, fogged: bool) -> (Arc<Snapshot>, Listener) {
        let n = side * side;
        let pop = Population { size: side, height: vec![0; n], ground: vec![16; n], gradient: vec![0; n], tick: 5, ..Default::default() };
        let fog = crate::fog::FogFrame { gfx: Arc::new(vec![if fogged { 0x3f } else { 0 }; n]), fade: 0, border: 0, vis: s4_sim::fog::Visibility { phase: 1, fade_counter: 15, range_max: 0 } };
        let snap = Snapshot { tick: 5, at: std::time::Instant::now(), tick_ms: 71, population: Arc::new(pop), hash: 0, fog: Some(fog), markers: None, building: None, ambient: None };
        let c = s4_viewer::terrain::node_pos(32, 32, 0);
        let lis = Listener { camera: Camera { x: c[0] - 400.0, y: c[1] - 300.0, zoom: 1.0 }, draw_zoom: 1.0, size: (800, 600) };
        (Arc::new(snap), lis)
    }

    /// The whole client path on an offline mixer: a job sound in the screen centre plays at
    /// `CalcPanningAndVolume`'s centre volume (zoom factor 17.24 → (int)37.24 = 37, pan 63), a
    /// fogged one is not started, a crush effect plays, and SETTLE music starts.
    #[test]
    fn job_sounds_and_effects_play_through_the_listener() {
        if !s4_formats::game_dir().join("Snd").exists() {
            return;
        }
        let (settings, _r) = s4_audio::offline::offline_settings(44100);
        let m = kira::AudioManager::<s4_audio::offline::OfflineBackend>::new(settings).unwrap();
        let mut g = GameSound::new(Audio::new(m, s4_formats::game_dir()).unwrap(), JobSoundRoll::new(1));
        g.start(0);
        assert_eq!(g.audio.music_streams().len(), 1, "SETTLE title 0");
        let axe = SoundId::by_name("SOUND_WORK_AXE").unwrap();
        let (snap, lis) = scene(false);
        let ev = [
            SimEvent::JobSound { settler: 9, x: 32, y: 32, sound: axe.0, chance: 100 },
            SimEvent::Effect { effect: 0x3e, sound: 3, x: 32, y: 32 },
        ];
        g.tick(&ev, &snap, &lis);
        let c = g.log.per_id[&axe.0];
        assert_eq!((c.requested, c.audible), (1, 1));
        let want = s4_audio::rules::fx_volume(100, g.audio.id_volume(axe), 37, 100);
        assert_eq!(c.volume_sum, want as u64);
        assert_eq!(g.log.per_id[&3].audible, 1, "crush sound");
        let (snap, lis) = scene(true);
        g.tick(&ev[..1], &snap, &lis);
        assert_eq!((g.log.job_fogged, g.log.per_id[&axe.0].requested), (1, 1));
    }

    /// `SimEvent::EffectLater` (the catapult's impact): its sound at its start tick, not before.
    #[test]
    fn delayed_effect_sounds_at_its_start() {
        if !s4_formats::game_dir().join("Snd").exists() {
            return;
        }
        let (settings, _r) = s4_audio::offline::offline_settings(44100);
        let m = kira::AudioManager::<s4_audio::offline::OfflineBackend>::new(settings).unwrap();
        let mut g = GameSound::new(Audio::new(m, s4_formats::game_dir()).unwrap(), JobSoundRoll::new(1));
        let at = |tick: u32| {
            let (snap, lis) = scene(false);
            let mut snap = Arc::try_unwrap(snap).ok().unwrap();
            Arc::make_mut(&mut snap.population).tick = tick;
            (Arc::new(snap), lis)
        };
        let (snap, lis) = at(5);
        g.tick(&[SimEvent::EffectLater { effect: 0x3f, sound: 91, x: 32, y: 32, delay: 3 }], &snap, &lis);
        let (s7, _) = at(7);
        g.tick(&[], &s7, &lis);
        assert!(!g.log.per_id.contains_key(&91));
        let (s8, _) = at(8);
        g.tick(&[], &s8, &lis);
        g.tick(&[], &s8, &lis);
        assert_eq!(g.log.per_id[&91].requested, 1);
    }

    /// The manakopter's rotor sounds through the client ([`rotor`]): on screen with an order the
    /// turn sound (once while it plays), without one the rotor loop instead, off screen none, and
    /// `Delete` stops it.
    #[test]
    fn manakopter_rotor_sounds() {
        if !s4_formats::game_dir().join("Snd").exists() {
            return;
        }
        let (settings, _r) = s4_audio::offline::offline_settings(44100);
        let m = kira::AudioManager::<s4_audio::offline::OfflineBackend>::new(settings).unwrap();
        let mut g = GameSound::new(Audio::new(m, s4_formats::game_dir()).unwrap(), JobSoundRoll::new(1));
        let (snap, lis) = scene_sized(256, false);
        let rotor = |ordered| SimEvent::Rotor { id: 3, x: 32, y: 32, ordered };
        g.tick(&[rotor(true)], &snap, &lis);
        g.draw(&snap, &lis);
        g.tick(&[rotor(true)], &snap, &lis);
        assert_eq!((g.log.rotor_sounds, g.log.per_id[&rotor::TURN].audible), (1, 1), "one turn sound while it plays");
        let turn = g.audio.last_voice().unwrap();
        g.draw(&snap, &lis);
        g.tick(&[rotor(false)], &snap, &lis);
        let loop_ = g.audio.last_voice().unwrap();
        assert!(!g.audio.voice_playing(turn) && g.audio.voice_playing(loop_), "the turn sound gives way to the loop");
        assert_eq!(g.log.per_id[&rotor::ROTOR].audible, 1);
        // Not drawn since the last update: the loop stops.
        g.tick(&[rotor(false)], &snap, &lis);
        assert!(!g.audio.voice_playing(loop_));
        // Far off screen: nothing.
        let far = SimEvent::Rotor { id: 4, x: 200, y: 200, ordered: true };
        g.draw(&snap, &lis);
        g.tick(&[far], &snap, &lis);
        assert_eq!(g.log.rotor_sounds, 2);
        g.draw(&snap, &lis);
        g.tick(&[rotor(false), SimEvent::RotorStop { id: 3 }], &snap, &lis);
        assert_eq!(g.log.rotor_sounds, 3);
        assert!(!g.audio.voice_playing(g.audio.last_voice().unwrap()), "Delete stops it");
    }

    /// The start screens' music: MENU STARTMENU title 0 (`Config\SoundTracks.cfg`).
    #[test]
    fn menu_music_is_startmenu_title_0() {
        if !s4_formats::game_dir().join("Snd").exists() {
            return;
        }
        let (settings, _r) = s4_audio::offline::offline_settings(44100);
        let m = kira::AudioManager::<s4_audio::offline::OfflineBackend>::new(settings).unwrap();
        let mut a = Audio::new(m, s4_formats::game_dir()).unwrap();
        a.set_music_state(Race::Menu, MusicStyle::StartMenu).unwrap();
        assert_eq!(a.playlist(MusicStyle::StartMenu).active(), Some("snd/romans_fight_02.mp3"));
        assert_eq!(a.music_streams().len(), 1);
    }

    /// The credits (0x511cf0) switch to playlist 5; leaving them stops every stream and the menu
    /// takes playlist 4 again.
    #[test]
    fn credits_playlist_and_back() {
        if !s4_formats::game_dir().join("Snd").exists() {
            return;
        }
        let (settings, _r) = s4_audio::offline::offline_settings(44100);
        let m = kira::AudioManager::<s4_audio::offline::OfflineBackend>::new(settings).unwrap();
        let mut a = Audio::new(m, s4_formats::game_dir()).unwrap();
        a.set_music_state(Race::Menu, MusicStyle::StartMenu).unwrap();
        a.switch_menu_playlist(MusicStyle::Credits).unwrap();
        let s = a.music_streams();
        assert!(s.len() == 1 && s[0].1 == Some(MusicStyle::Credits as usize), "{s:?}");
        a.stop_menu_music();
        assert!(a.music_streams().iter().all(|s| s.1.is_none()) || a.music_streams().is_empty());
        a.switch_menu_playlist(MusicStyle::StartMenu).unwrap();
        let s = a.music_streams();
        assert!(s.iter().any(|t| t.1 == Some(MusicStyle::StartMenu as usize)), "{s:?}");
    }

    /// `DrawObjects` (0x660070) on an 800×600 output at zoom 1, the view centred on node (32, 32):
    /// scroll (−16, 84) px → vx = 0, vy = 7, across = 37, rows = 51, end = 60. Row 32 visits
    /// x = 16 − 30 ..< 16 + 37 + 9 = 62; rows 0 ..< 71. Effects: x0 = −30 → square 0,
    /// x1 = 35 − 30 + 46 = 51 → square 3 + 1.
    #[test]
    fn draw_range_is_the_object_pass() {
        let (snap, lis) = scene_sized(128, false);
        let r = DrawRange::new(&snap, &lis);
        assert_eq!((r.vx, r.vy, r.across, r.end), (0, 7, 37, 60));
        assert!(r.visits(0, 32) && r.visits(61, 32) && !r.visits(62, 32));
        // Odd rows start and end one node further right.
        assert!(r.visits(62, 33) && !r.visits(63, 33));
        assert!(r.visits(10, 0) && r.visits(40, 70) && !r.visits(40, 71));
        assert!(r.object_drawn(30, 30));
        assert!(r.effect_drawn(79, 0) && !r.effect_drawn(80, 0) && r.effect_drawn(0, 70) && !r.effect_drawn(0, 80));
        let (snap, lis) = scene_sized(128, true);
        let r = DrawRange::new(&snap, &lis);
        assert!(r.visits(30, 30) && !r.object_drawn(30, 30), "object fog not above 59");
        assert!(r.effect_drawn(30, 30), "effects ignore the fog");
    }

    #[test]
    fn music_centre_is_flat() {
        let lis = Listener { camera: Camera { x: 0.0, y: 0.0, zoom: 1.0 }, draw_zoom: 1.0, size: (480, 240) };
        // centre (240, 120): y = 120 / 12 = 10, x = 240 / 24 + 5 = 15
        assert_eq!(music_centre(64, &lis), Some((15, 10)));
        assert_eq!(music_centre(12, &lis), None);
    }
}
