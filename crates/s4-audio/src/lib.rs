//! Sound effects and music with the original's rules, on `kira`.
//!
//! Mirrors `CSoundManager` (decompiled debug build, `Main\CSoundManager.cpp`), the Miles-based
//! `CSoundSystem` / `CPlaylist` (`SoundEngine\`) and `CLogic::HandleSoundtracks`. The rules
//! themselves are in [`rules`]; this module maps them onto kira and the game data:
//!
//! * **Effects** come from `Snd\0.snd` (`s4_formats::snd`). Each bank slot is decoded once and
//!   cached. `PlaySoundFX(id)` picks the slot `LUT[id].first + rand() % LUT[id].count`. Its volume
//!   is `CalcFinalVolume(sound setting, Config\SoundFX.cfg [SOUND_VOLUMES] id (default 100), call
//!   volume)`. Positional sounds (`PlayEnvironmentSound`) get their call volume and pan from
//!   `CalcPanningAndVolume` relative to the [`View`]. At most [`rules::MAX_SOUND_VOICES`] play at
//!   once; further ones are dropped, as in the original.
//! * **Music** uses the `Config\SoundTracks.cfg` playlists (`LoadRaceTitles`). `start` begins title 0
//!   of a style without a fade. A style change (`ChangePlaylist`) starts the same title index in the
//!   new playlist, then fades the old stream to 0 and the new one up to the music volume over the
//!   new title's fade time, in 71 ms steps. When a title ends, a different random title follows.
//!   MP3s are streamed.
//! * **Style choice** ([`rules::StyleDirector`], from `CLogic::HandleSoundtracks`): FIGHT until 32 logic
//!   ticks after the last war action, otherwise DARK while the screen centre is on dark land,
//!   otherwise SETTLE.
//!
//! The simulation never calls this crate. It emits events, and the client turns them into
//! [`AudioEvent`]s and passes them to [`Audio::handle`].
//!
//! Volumes and pans stay in Miles units (0..=127) up to the last step, then become the gains the
//! shipped Miles 6.1c (`mss32.dll`) mixer uses ([`miles_mix`]).

pub mod cfg;
pub mod offline;
pub mod rules;

use cfg::Cfg;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::backend::Backend;
use kira::{AudioManager, Decibels, Easing, Frame, Panning, StartTime, Tween};
pub use rules::{MusicStyle, Race};
use rules::{Fade, FadeStep, Playlist, Rand, ScreenProbe, StyleDirector};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Formats(#[from] s4_formats::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("audio: {0}")]
    Kira(String),
}
pub type Result<T> = std::result::Result<T, Error>;

/// A `SIV_SOUNDS` id (index into `s4_formats::snd::SOUNDS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SoundId(pub u16);

impl SoundId {
    pub fn by_name(name: &str) -> Option<SoundId> {
        s4_formats::snd::sound_id_by_name(name).map(|i| SoundId(i as u16))
    }
    pub fn name(self) -> &'static str {
        s4_formats::snd::SOUNDS.get(self.0 as usize).map_or("?", |s| s.name)
    }
}

/// [`View::screen`]: node → pixel position on the output.
pub type ScreenFn = Arc<dyn Fn(i32, i32) -> (f32, f32) + Send + Sync>;

/// The local view the listener is attached to, used to place positional sounds on the screen.
/// Without [`View::screen`] it uses the game's isometric layout without terrain height: node
/// (x, y) appears at `(tri·x − tri/2·y, tri/2·y)` relative to node (0, 0).
#[derive(Clone)]
pub struct View {
    pub width: i32,
    pub height: i32,
    /// Screen position of node (0, 0), in pixels.
    pub origin: (f32, f32),
    /// Triangle size in pixels (`m_uZoom`, 14..=72).
    pub triangle_px: f32,
    /// Map side in nodes (positions outside give "outside the map").
    pub map_size: i32,
    /// `g_pFogging->IsVisible(x, y)`; `None` = everything visible.
    pub fog: Option<Arc<dyn Fn(i32, i32) -> bool + Send + Sync>>,
    /// `IGfxEngine::GetScreenOffsetsByMapIndices(x, y)`: the node's pixel position on the output
    /// (with terrain height), for nodes inside the map. `None` = the flat layout above.
    pub screen: Option<ScreenFn>,
}

impl Default for View {
    fn default() -> Self {
        View { width: 1024, height: 768, origin: (0.0, 0.0), triangle_px: 24.0, map_size: 0, fog: None, screen: None }
    }
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View")
            .field("size", &(self.width, self.height))
            .field("origin", &self.origin)
            .field("triangle_px", &self.triangle_px)
            .finish()
    }
}

impl View {
    /// `CStateGame::GetZoomFactor`: `(zoom − 14) · 100 / 58`.
    pub fn zoom_factor(&self) -> f32 {
        (self.triangle_px - 14.0) * 100.0 / 58.0
    }
    /// A view of `width × height` pixels centred on node `(cx, cy)`.
    pub fn centred_on(cx: f32, cy: f32, width: i32, height: i32, triangle_px: f32, map_size: i32) -> View {
        let t = triangle_px;
        let (sx, sy) = (t * cx - t / 2.0 * cy, t / 2.0 * cy);
        View { width, height, origin: (width as f32 / 2.0 - sx, height as f32 / 2.0 - sy), triangle_px, map_size, fog: None, screen: None }
    }
    pub fn probe(&self, x: i32, y: i32) -> ScreenProbe {
        let inside = x >= 0 && y >= 0 && x < self.map_size && y < self.map_size;
        let t = self.triangle_px;
        let screen = inside.then(|| {
            let (sx, sy) = match &self.screen {
                Some(f) => f(x, y),
                None => (self.origin.0 + t * x as f32 - t / 2.0 * y as f32, self.origin.1 + t / 2.0 * y as f32),
            };
            (sx.floor() as i32, sy.floor() as i32)
        });
        ScreenProbe {
            visible: self.fog.as_ref().is_none_or(|f| f(x, y)),
            screen,
            width: self.width,
            height: self.height,
            zoom_factor: self.zoom_factor(),
        }
    }
}

/// What the client feeds in, translated from simulation / UI events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioEvent {
    /// `PlayEnvironmentSound(id, x, y)`: a sound at a map node (work animations, effects, ...).
    Fx { id: SoundId, x: i32, y: i32 },
    /// `PlaySoundFX(id)`: a non-positional sound (GUI clicks, messages).
    Ui { id: SoundId },
    /// `CLogic::SetWarAction`: a warrior animated this tick.
    WarAction,
    /// Once per logic tick, `CSoundManager::Update`: the map node at the screen centre and the
    /// `Flags1` byte of its 16×16 resource square (see [`Audio::update_ambient`]).
    Ambient { x: i32, y: i32, flags: u8 },
    /// End of a logic tick, `CLogic::HandleSoundtracks`. `centre_dark` is the dark-land flag of
    /// the screen-centre node (`None` if the centre is outside the map).
    LogicTick { centre_dark: Option<bool> },
}

/// What happened to one `play_*` request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Played {
    /// Playing on a voice (`volume`, `pan` in Miles units; volume 0 still takes a voice, as in
    /// the original).
    Voice { slot: u16, volume: i32, pan: i32 },
    /// All [`rules::MAX_SOUND_VOICES`] voices busy: dropped.
    NoVoice,
    /// Id 0, out of range, or an unmapped bank slot (e.g. `SOUND_WORK_HEAL`).
    Silent,
}

/// A started effect voice ([`Audio::last_voice`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId(pub u64);

/// A live music stream: (track id, playlist it belongs to or `None` once detached, Miles volume).
pub type StreamInfo = (u32, Option<usize>, i32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub played: u32,
    pub dropped_no_voice: u32,
    pub silent: u32,
    pub style_changes: u32,
    pub titles_started: u32,
}

/// One of the six ambient slots (`dword_3D89C6C[3*i]` handle, `+4` fading out, `+5` fading in).
#[derive(Default)]
struct AmbientLoop {
    handle: Option<StaticSoundHandle>,
    volume: i32,
    pan: i32,
    fading_out: bool,
    fading_in: bool,
    fade: Option<Fade>,
}

struct Stream {
    id: u32,
    handle: StreamingSoundHandle<FromFileError>,
    /// `CSoundSystem` stream slot `+14`: the playlist it belongs to, `None` once detached by a
    /// style change (it then plays out its fade and stops, without a successor).
    list: Option<usize>,
    volume: i32,
    pan: i32,
}

/// The four `CSoundSystem` streams, the six playlists and the fade jobs.
struct Music {
    lists: Vec<Playlist>,
    /// `CPlaylist::TrackID` per list.
    track_of: [Option<u32>; 6],
    /// `CSoundSystem + 0x40`: the list `ChangePlaylist` treats as current (not set by
    /// `StartPlaylist`, as in the original).
    current_list: usize,
    race: Option<Race>,
    streams: Vec<Stream>,
    fades: Vec<(u32, Fade)>,
    next_id: u32,
}

impl Music {
    fn new() -> Music {
        Music {
            lists: vec![Playlist::default(); 6],
            track_of: [None; 6],
            current_list: 0,
            race: None,
            streams: Vec::new(),
            fades: Vec::new(),
            next_id: 1,
        }
    }
    fn stream(&mut self, id: u32) -> Option<&mut Stream> {
        self.streams.iter_mut().find(|s| s.id == id)
    }
}

/// The audio service.
pub struct Audio<B: Backend = kira::DefaultBackend> {
    manager: AudioManager<B>,
    game_dir: PathBuf,
    bank: s4_formats::snd::SoundBank,
    cache: Vec<Option<Option<StaticSoundData>>>,
    id_volume: Vec<i32>,
    tracks: Cfg,
    sound: i32,
    music_setting: i32,
    mute: i32,
    view: View,
    /// Playing effects with their pan and [`VoiceId`].
    voices: Vec<(StaticSoundHandle, i32, VoiceId)>,
    /// The id of the next started effect voice, and of the last one ([`Audio::last_voice`]).
    next_voice: u64,
    last_voice: Option<VoiceId>,
    ambient: Vec<AmbientLoop>,
    rng: Rand,
    music: Music,
    director: StyleDirector,
    user_mp3: bool,
    fade_clock: Duration,
    pub stats: Stats,
}

/// The channel gains the shipped Miles 6.1c mixer gives a sample of volume `v` and pan `p`
/// (`mss32.dll` 0x21114470, called by `AIL_set_sample_volume` / `_pan` through 0x21114660): the
/// volume is clamped to 0..=127 and scaled linearly by the digital master volume (`·master/127`;
/// [inf] the game never sets it, so the Miles default 127), then each channel gets
/// `table[·]·v/8` out of 0x7f0 with the table at 0x2114c380: `t[i] = 2i` for i < 63, 128 from 63
/// on; right = `t[p]`, left = `t[127 − p]`. So the pan is a balance: the near channel stays at
/// full gain from the centre outwards, the far one falls linearly to 0 (no equal-power boost).
/// Returns (left, right) as linear gains, 1.0 = full scale.
pub fn miles_gains(v: i32, p: i32) -> (f32, f32) {
    let t = |i: i32| if i < 63 { 2 * i } else { 128 };
    let (v, p) = (v.clamp(0, 127), p.clamp(0, 127));
    let gain = |i: i32| (t(i) * v / 8) as f32 / 0x7f0 as f32;
    (gain(127 - p), gain(p))
}

/// [`miles_gains`] as kira volume and panning. Kira pans with an equal-power law (left
/// `√(1−m)·√2`, right `√m·√2`, `m = (panning+1)/2`), so the panning that gives the Miles ratio is
/// `m = R²/(L²+R²)` and the volume `√(L²+R²)/√2`.
fn miles_mix(v: i32, p: i32) -> (Decibels, Panning) {
    let (l, r) = miles_gains(v, p);
    let power = l * l + r * r;
    if power <= 0.0 {
        return (Decibels::SILENCE, Panning::CENTER);
    }
    let amp = (power / 2.0).sqrt();
    (Decibels(20.0 * amp.log10()), Panning(2.0 * r * r / power - 1.0))
}

fn instant() -> Tween {
    Tween { start_time: StartTime::Immediate, duration: Duration::ZERO, easing: Easing::Linear }
}

/// A file name from the cfg (`snd/TROJANs_settle_01.mp3`) under the install, matched
/// case-insensitively like Windows does (`s4_formats::path::resolve`).
fn resolve_case_insensitive(root: &Path, rel: &str) -> Option<PathBuf> {
    let p = rel.split(['/', '\\']).filter(|s| !s.is_empty()).fold(root.to_path_buf(), |p, part| p.join(part));
    let p = s4_formats::path::resolve(p);
    p.is_file().then_some(p)
}

impl<B: Backend> Audio<B> {
    /// `CSoundManager::OpenSoundFiles` + config: bank from `<game>\Snd`, per-id volumes from
    /// `Config\SoundFX.cfg`, playlists from `Config\SoundTracks.cfg`.
    pub fn new(manager: AudioManager<B>, game_dir: impl AsRef<Path>) -> Result<Self> {
        let game_dir = game_dir.as_ref().to_path_buf();
        let bank = s4_formats::snd::SoundBank::open(&game_dir)?;
        let read_cfg = |name: &str| -> Cfg {
            resolve_case_insensitive(&game_dir, &format!("Config/{name}"))
                .and_then(|p| std::fs::read(p).ok())
                .map(|b| Cfg::parse(&String::from_utf8_lossy(&b)))
                .unwrap_or_default()
        };
        let fx = read_cfg("SoundFX.cfg");
        let tracks = read_cfg("SoundTracks.cfg");
        let id_volume = s4_formats::snd::SOUNDS.iter().map(|s| fx.int("SOUND_VOLUMES", s.name, 100)).collect();
        Ok(Audio {
            manager,
            game_dir,
            cache: vec![None; bank.len()],
            bank,
            id_volume,
            tracks,
            sound: rules::DEFAULT_VOLUME,
            music_setting: rules::DEFAULT_VOLUME,
            mute: 100,
            view: View::default(),
            voices: Vec::new(),
            next_voice: 0,
            last_voice: None,
            ambient: (0..6).map(|_| AmbientLoop::default()).collect(),
            rng: Rand::new(0x5eed),
            music: Music::new(),
            director: StyleDirector::default(),
            user_mp3: false,
            fade_clock: Duration::ZERO,
            stats: Stats::default(),
        })
    }

    pub fn manager(&mut self) -> &mut AudioManager<B> {
        &mut self.manager
    }
    /// `[SOUND_VOLUMES]` value of an id.
    pub fn id_volume(&self, id: SoundId) -> i32 {
        self.id_volume.get(id.0 as usize).copied().unwrap_or(100)
    }
    pub fn set_listener(&mut self, view: View) {
        self.view = view;
    }
    pub fn listener(&self) -> &View {
        &self.view
    }
    /// Voices currently busy.
    /// The voice the last [`AudioEvent::Fx`] / [`AudioEvent::Ui`] started (`PlaySound`'s
    /// returned handle), `None` if it started none.
    pub fn last_voice(&self) -> Option<VoiceId> {
        self.last_voice
    }

    /// 0x50d6a0: whether the voice still plays: `IsRunning` (0x69ad00, the streams) or
    /// `GetVolume` (0x69acb0: the sample record holding the handle, with the Miles status 4,
    /// playing). Handles come from a counter (`SoundSystem +0x20`, 0x69a2c2), so a finished
    /// sound answers false even when its record is reused.
    pub fn voice_playing(&self, v: VoiceId) -> bool {
        self.voices.iter().any(|(h, _, id)| *id == v && h.state() != PlaybackState::Stopped)
    }

    pub fn active_voices(&mut self) -> usize {
        self.voices.retain(|v| v.0.state() != PlaybackState::Stopped);
        self.voices.len() + self.ambient.iter().filter(|a| a.handle.is_some()).count()
    }

    /// Decoded bank slot, cached (`None` = unmapped).
    fn slot_data(&mut self, slot: usize) -> Result<Option<StaticSoundData>> {
        if let Some(Some(d)) = self.cache.get(slot) {
            return Ok(d.clone());
        }
        let data = match self.bank.raw_wav(slot)? {
            None => None,
            Some(wav) => {
                let s = s4_formats::snd::decode_wav(wav)?;
                let frames: Arc<[Frame]> = if s.channels >= 2 {
                    s.pcm.chunks_exact(s.channels as usize).map(|c| Frame::new(c[0] as f32 / 32768.0, c[1] as f32 / 32768.0)).collect()
                } else {
                    s.pcm.iter().map(|&v| Frame::from_mono(v as f32 / 32768.0)).collect()
                };
                Some(StaticSoundData { sample_rate: s.rate, frames, settings: StaticSoundSettings::default(), slice: None })
            }
        };
        if let Some(c) = self.cache.get_mut(slot) {
            *c = Some(data.clone());
        }
        Ok(data)
    }

    /// Decode every slot now instead of on first use.
    pub fn preload(&mut self) -> Result<usize> {
        let mut n = 0;
        for slot in 0..self.bank.len() {
            n += self.slot_data(slot)?.is_some() as usize;
        }
        Ok(n)
    }

    /// `CSoundManager::PlaySoundFX(id, call_vol, pan, 100, looped)`.
    fn play_sound_fx(&mut self, id: SoundId, call_vol: i32, pan: i32, looped: bool) -> Result<Played> {
        let (played, handle) = self.start_voice(id, call_vol, pan, looped)?;
        self.last_voice = None;
        if let Some(h) = handle {
            let v = VoiceId(self.next_voice);
            self.next_voice += 1;
            self.voices.push((h, pan, v));
            self.last_voice = Some(v);
        }
        Ok(played)
    }

    /// `CSoundManager::PlaySoundFX(id, call_vol, pan, 100, looped)`; the caller keeps the handle.
    fn start_voice(&mut self, id: SoundId, call_vol: i32, pan: i32, looped: bool) -> Result<(Played, Option<StaticSoundHandle>)> {
        let Some(entry) = s4_formats::snd::SOUNDS.get(id.0 as usize).filter(|_| id.0 != 0) else {
            self.stats.silent += 1;
            return Ok((Played::Silent, None));
        };
        let slot = entry.first as usize + (self.rng.rand() % entry.count.max(1) as u32) as usize;
        let Some(mut data) = self.slot_data(slot)? else {
            self.stats.silent += 1;
            return Ok((Played::Silent, None));
        };
        if self.active_voices() >= rules::MAX_SOUND_VOICES {
            self.stats.dropped_no_voice += 1;
            return Ok((Played::NoVoice, None));
        }
        if looped {
            data = data.loop_region(..); // LoopSampleCB restarts the sample at its end
        }
        let volume = rules::fx_volume(self.sound, self.id_volume(id), call_vol, self.mute);
        let handle = self
            .manager
            .play({
                let (db, pn) = miles_mix(volume, pan);
                data.volume(db).panning(pn)
            })
            .map_err(|e| Error::Kira(e.to_string()))?;
        self.stats.played += 1;
        Ok((Played::Voice { slot: slot as u16, volume, pan }, Some(handle)))
    }

    /// `CSoundManager::Update`, once per logic tick: the ambient loops for the resource square
    /// under the screen centre. `(x, y)` is the map node at the screen centre
    /// (`GetClosestMapPoint(w/2, h/2)`), `flags` that square's `CAIResourceData::Flags1`.
    ///
    /// Volume and pan come from `CalcPanningAndVolume(x, y)`, with volumes below 5 counted as 0.
    /// Note that the per-id `SoundFX.cfg` volume only affects the loop's start (which is at call
    /// volume 0 anyway). The fade target and later updates use the raw centre volume, as in the
    /// original.
    ///
    /// For each of the 6 masks in [`rules::AMBIENT`]:
    /// - flag set, loop not running: start it looped at volume 0 and fade it in to `v` over 500 ms;
    /// - flag set, loop running: once `v` is at or below its volume the fade-in counts as over,
    ///   and from then on volume and pan follow every tick;
    /// - flag clear, loop running: fade it out over 500 ms (once); it stops at 0.
    ///
    /// Simplification: when the fade-in counts as over, its job is dropped here. The original
    /// lets the job run on alongside `ChangeVolume`.
    pub fn update_ambient(&mut self, x: i32, y: i32, flags: u8) -> Result<()> {
        let probe = self.view.probe(x, y);
        let (mut v, pan) = rules::calc_pan_and_volume(&probe, self.sound, self.mute);
        if v < rules::AMBIENT_MIN_VOLUME {
            v = 0;
        }
        for (i, &(mask, sound)) in rules::AMBIENT.iter().enumerate() {
            let running = self.ambient[i].handle.as_ref().is_some_and(|h| h.state() != PlaybackState::Stopped);
            if flags & mask != 0 {
                if running {
                    let a = &mut self.ambient[i];
                    if v <= a.volume {
                        a.fading_in = false;
                        a.fade = None;
                    }
                    if !a.fading_in
                        && let Some(h) = a.handle.as_mut()
                    {
                        let (db, pn) = miles_mix(v, pan);
                        h.set_panning(pn, instant());
                        h.set_volume(db, instant());
                        a.volume = v;
                        a.pan = pan;
                    }
                } else {
                    let (_, handle) = self.start_voice(SoundId(sound), 0, pan, true)?;
                    let a = &mut self.ambient[i];
                    if let Some(h) = handle {
                        a.handle = Some(h);
                        a.pan = pan;
                        a.volume = 0;
                        a.fade = Some(Fade::new(0.0, v, rules::AMBIENT_FADE_MS));
                        a.fading_in = true;
                    }
                }
                self.ambient[i].fading_out = false;
            } else if running && !self.ambient[i].fading_out {
                let a = &mut self.ambient[i];
                let from = a.fade.map_or(a.volume as f64, |f| f.current);
                a.fade = Some(Fade::new(from, 0, rules::AMBIENT_FADE_MS));
                a.fading_out = true;
                a.fading_in = false;
            }
            let a = &mut self.ambient[i];
            if !a.handle.as_ref().is_some_and(|h| h.state() != PlaybackState::Stopped) {
                if let Some(mut h) = a.handle.take() {
                    h.stop(instant());
                }
                *a = AmbientLoop::default();
            }
        }
        Ok(())
    }

    /// Ambient loops: (sound id, Miles volume) of each running one.
    pub fn ambient_loops(&self) -> Vec<(SoundId, i32)> {
        self.ambient
            .iter()
            .zip(rules::AMBIENT)
            .filter(|(a, _)| a.handle.is_some())
            .map(|(a, (_, id))| (SoundId(id), a.volume))
            .collect()
    }

    /// `PlayEnvironmentSound(id, x, y)`: a sound at map node (x, y), placed by the listener view.
    pub fn play_fx(&mut self, id: SoundId, x: i32, y: i32) -> Result<Played> {
        let probe = self.view.probe(x, y);
        self.play_fx_probe(id, &probe)
    }

    /// As [`Audio::play_fx`] with an explicit screen probe (exact screen position, fog).
    pub fn play_fx_probe(&mut self, id: SoundId, probe: &ScreenProbe) -> Result<Played> {
        let (vol, pan) = rules::calc_pan_and_volume(probe, self.sound, self.mute);
        self.play_sound_fx(id, vol, pan, false)
    }

    /// `PlayEnvironmentSound(id, x, y, 1)`: as [`Audio::play_fx`], looped. The last argument of
    /// `PlaySound` (0x50c390) reaches `PlaySoundFromMemory` (0x69a260), which then registers
    /// `EndOfSoundCallback` (0x69ac20): at the sample's end it starts it again while its record
    /// still holds the handle, i.e. until [`Audio::stop_voice`]. The voice is
    /// [`Audio::last_voice`].
    pub fn play_fx_looped(&mut self, id: SoundId, x: i32, y: i32) -> Result<Played> {
        let probe = self.view.probe(x, y);
        let (vol, pan) = rules::calc_pan_and_volume(&probe, self.sound, self.mute);
        self.play_sound_fx(id, vol, pan, true)
    }

    /// `ISoundEngine::StopSample` (0x69a910): stops the voice and clears its record's handle (so
    /// [`Audio::voice_playing`] answers false). An unknown or finished voice: nothing.
    pub fn stop_voice(&mut self, v: VoiceId) {
        for (h, _, id) in &mut self.voices {
            if *id == v {
                h.stop(instant());
            }
        }
        self.voices.retain(|(_, _, id)| *id != v);
    }

    /// `PlaySoundFX(id)`: non-positional, call volume 100, centre pan.
    pub fn play_ui(&mut self, id: SoundId) -> Result<Played> {
        self.play_sound_fx(id, 100, rules::PAN_CENTER, false)
    }

    /// `SetSoundVolume` / `SetMusicVolume` / `Mute`. Like `CSoundSystem::ChangeSoundVolume`, a new
    /// sound setting is applied as-is to every sound already playing (0 stops them). Likewise
    /// `ChangeStreamVolume` sets every stream, including fading ones, and pauses at 0.
    pub fn set_volumes(&mut self, sound: i32, music: i32, mute: bool) {
        self.sound = sound;
        self.music_setting = music;
        self.mute = if mute { 0 } else { 100 };
        let fx = rules::setting_volume(sound, self.mute);
        for (v, pan, _) in &mut self.voices {
            if fx == 0 {
                v.stop(instant());
            } else {
                v.set_volume(miles_mix(fx, *pan).0, instant());
            }
        }
        let mv = rules::setting_volume(music, self.mute);
        for s in &mut self.music.streams {
            if mv == 0 {
                s.handle.pause(instant());
            } else {
                if s.volume == 0 {
                    s.handle.resume(instant());
                }
                s.handle.set_volume(miles_mix(mv, s.pan).0, instant());
            }
            s.volume = mv;
        }
    }

    fn music_volume(&self) -> i32 {
        rules::setting_volume(self.music_setting, self.mute)
    }

    /// `CSoundSystem::PlayStream(title, vol, pan, list)`.
    fn play_stream(&mut self, title: &str, volume: i32, pan: i32, list: Option<usize>, start: f64) -> Result<Option<u32>> {
        if self.music.streams.len() >= rules::MAX_STREAMS {
            return Ok(None); // "No free stream handle"
        }
        let Some(path) = resolve_case_insensitive(&self.game_dir, title) else {
            return Ok(None); // "Failed to load stream"
        };
        let data = StreamingSoundData::from_file(&path)
            .map_err(|e| Error::Kira(format!("{}: {e}", path.display())))?
            .volume(miles_mix(volume, pan).0)
            .panning(miles_mix(127, pan).1)
            .start_position(start);
        let handle = self.manager.play(data).map_err(|e| Error::Kira(e.to_string()))?;
        let id = self.music.next_id;
        self.music.next_id += 1;
        self.music.streams.push(Stream { id, handle, list, volume, pan });
        if let Some(l) = list {
            self.music.track_of[l] = Some(id);
        }
        self.stats.titles_started += 1;
        Ok(Some(id))
    }

    /// `CSoundSystem::StopSample(id)` for a stream: remember the position in its playlist (only
    /// while it still belongs to one), then close it.
    fn stop_stream(&mut self, id: u32) {
        if let Some(i) = self.music.streams.iter().position(|s| s.id == id) {
            let mut s = self.music.streams.remove(i);
            if let Some(l) = s.list {
                self.music.lists[l].stored_pos = s.handle.position();
            }
            s.handle.stop(instant());
        }
    }

    /// `CSoundManager::LoadRaceTitles(race)`.
    fn load_race_titles(&mut self, race: Race) {
        for style in MusicStyle::ALL {
            let section = format!("{}SOUNDTRACKS_{}", race.name(), style.name());
            let n = self.tracks.entry_count(&section);
            if n == 0 {
                continue;
            }
            let fade_section = format!("{section}_FADETIME");
            let mut list = Playlist::default(); // InitPlaylist
            for j in 0..n {
                let key = format!("{}{j}", race.name());
                let file = self.tracks.get(&section, &key).unwrap_or("");
                if !file.is_empty() {
                    list.titles.push((file.to_string(), self.tracks.int(&fade_section, &key, 0)));
                }
            }
            self.music.lists[style as usize] = list;
        }
    }

    fn stop_music(&mut self) {
        let ids: Vec<u32> = self.music.streams.iter().map(|s| s.id).collect();
        for id in ids {
            self.stop_stream(id);
        }
        self.music.fades.clear();
    }

    /// The playlist a style currently holds (for inspection).
    pub fn playlist(&self, style: MusicStyle) -> &Playlist {
        &self.music.lists[style as usize]
    }

    /// `CSoundManager::PlayBackgroundMusic(race, style)` when the race changes (playlists of
    /// `race` + MENU, title 0 of `style`, no fade); otherwise `ChangeMusicStyle(style)`.
    pub fn set_music_state(&mut self, race: Race, style: MusicStyle) -> Result<()> {
        if self.music.race == Some(race) {
            let current_style = self.director.style;
            if current_style != style {
                self.director.style = style;
                self.change_style(style)?;
            }
            return Ok(());
        }
        self.stop_music();
        self.music.lists = vec![Playlist::default(); 6]; // CreatePlaylists(6)
        self.music.track_of = [None; 6];
        self.load_race_titles(race);
        self.load_race_titles(Race::Menu);
        self.music.race = Some(race);
        self.director = StyleDirector { war: 0, style };
        // CSoundSystem::StartPlaylist
        let list = &mut self.music.lists[style as usize];
        list.set_title(0);
        if let Some(title) = list.active().map(str::to_string) {
            let vol = self.music_volume();
            self.play_stream(&title, vol, rules::PAN_CENTER, Some(style as usize), 0.0)?;
        }
        Ok(())
    }

    /// The start screens' switch (credits 0x511cf0, main menu 0x512d26, 0x5229cd): the
    /// race field set to -2 (so the test `race != 5` always passes), then 0x50c8c0 `(5, ..)`:
    /// every stream stopped (0x69aa60), the six playlists made anew (0x69ad60) with the MENU
    /// titles, race 5; then `ChangePlaylist(style, music volume)` (0x69af10): the credits take
    /// list 5, the menu list 4. The credits state's destructor (0x511ab6) stops every stream and
    /// sets the race to -2 again, so the menu after it switches back.
    pub fn switch_menu_playlist(&mut self, style: MusicStyle) -> Result<()> {
        self.stop_music();
        self.music.lists = vec![Playlist::default(); 6];
        self.music.track_of = [None; 6];
        self.load_race_titles(Race::Menu);
        self.music.race = Some(Race::Menu);
        self.director = StyleDirector { war: 0, style };
        self.change_style(style)
    }

    /// The credits state's destructor (0x511a9x): every stream stops (0x69aa60), race -2.
    pub fn stop_menu_music(&mut self) {
        self.stop_music();
        self.music.race = None;
    }

    /// `CSoundSystem::ChangePlaylist(style, music volume)`.
    fn change_style(&mut self, style: MusicStyle) -> Result<()> {
        let new = style as usize;
        let cur = self.music.current_list;
        let old_track = self.music.track_of[cur];
        let playing = self.music.lists[cur].current;
        let prev = self.music.lists[new].current;
        self.music.lists[new].set_title(playing);
        let start = self.music.lists[new].stored_pos;
        let title = self.music.lists[new].active().map(str::to_string);
        let new_id = match title {
            Some(t) => self.play_stream(&t, 0, rules::PAN_CENTER, Some(new), start)?,
            None => None,
        };
        let Some(new_id) = new_id else {
            self.music.lists[new].set_title(prev);
            return Ok(());
        };
        if let Some(old) = old_track.filter(|&o| o != new_id)
            && let Some(s) = self.music.stream(old)
        {
            s.list = None;
        }
        self.music.current_list = new;
        let fade_ms = self.music.lists[new].fade_in_ms(self.music.lists[new].current);
        if let Some(old) = old_track {
            self.start_fade(old, 0, fade_ms);
        }
        let vol = self.music_volume();
        self.start_fade(new_id, vol, fade_ms);
        self.stats.style_changes += 1;
        Ok(())
    }

    /// `CSoundSystem::Fade(id, target, ms)`: reuse the job of `id` if it has one.
    fn start_fade(&mut self, id: u32, target: i32, ms: i32) {
        let from = match self.music.fades.iter().position(|(i, _)| *i == id) {
            Some(k) => self.music.fades.remove(k).1.current,
            None => self.music.stream(id).map_or(0, |s| s.volume) as f64,
        };
        self.music.fades.push((id, Fade::new(from, target, ms)));
    }

    /// `CSoundSystem::UpdateFades`, one 71 ms step.
    fn step_fades(&mut self) {
        let step = Duration::from_millis(rules::FADE_STEP_MS as u64);
        let tween = Tween { start_time: StartTime::Immediate, duration: step, easing: Easing::Linear };
        for a in &mut self.ambient {
            let (Some(f), Some(h)) = (a.fade.as_mut(), a.handle.as_mut()) else { continue };
            let (v, end) = f.advance();
            a.volume = v;
            h.set_volume(miles_mix(v, a.pan).0, tween);
            match end {
                FadeStep::Continue => {}
                FadeStep::Done => a.fade = None,
                FadeStep::Stop => {
                    h.stop(instant());
                    *a = AmbientLoop::default();
                }
            }
        }
        let mut k = 0;
        while k < self.music.fades.len() {
            let (id, mut fade) = self.music.fades[k];
            let Some(s) = self.music.stream(id) else {
                // unknown id: the original deletes the job and ends this update
                self.music.fades.remove(k);
                return;
            };
            let (v, end) = fade.advance();
            s.volume = v;
            s.handle.set_volume(miles_mix(v, s.pan).0, tween);
            match end {
                FadeStep::Continue => {
                    self.music.fades[k].1 = fade;
                    k += 1;
                }
                FadeStep::Done => {
                    self.music.fades.remove(k);
                }
                FadeStep::Stop => {
                    self.music.fades.remove(k);
                    self.stop_stream(id);
                }
            }
        }
    }

    /// `CSoundSystem::PlaylistTitleEnded` for every stream that finished: the next title (random,
    /// never the same) at the same volume and pan, keeping the track id so fades continue.
    fn titles_ended(&mut self) -> Result<()> {
        let ended: Vec<u32> = self.music.streams.iter().filter(|s| s.handle.state() == PlaybackState::Stopped).map(|s| s.id).collect();
        for id in ended {
            let Some(i) = self.music.streams.iter().position(|s| s.id == id) else { continue };
            let s = self.music.streams.remove(i);
            let Some(l) = s.list else { continue };
            let r = self.rng.rand();
            self.music.lists[l].title_ended(r);
            let Some(title) = self.music.lists[l].active().map(str::to_string) else { continue };
            if let Some(new_id) = self.play_stream(&title, s.volume, s.pan, Some(l), 0.0)? {
                // the successor takes over the old id
                if let Some(n) = self.music.stream(new_id) {
                    n.id = id;
                }
                self.music.track_of[l] = Some(id);
            }
        }
        Ok(())
    }

    /// "Play own MP3s" (`CGameSettings::GetUserMP3`): `CreateDirPlaylist(Snd\User)`, one playlist
    /// of every `*.mp3` with 2500 ms fades. Style switching is off in this mode.
    pub fn set_user_mp3(&mut self, on: bool) -> Result<()> {
        self.user_mp3 = on;
        if !on {
            return Ok(());
        }
        self.stop_music();
        self.music.lists = vec![Playlist::default(); 1];
        self.music.track_of = [None; 6];
        self.music.race = None;
        let dir = self.game_dir.join("Snd").join("User");
        let mut titles: Vec<String> = std::fs::read_dir(s4_formats::path::resolve(&dir))
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("mp3")))
                    .map(|e| format!("Snd/User/{}", e.file_name().to_string_lossy()))
                    .collect()
            })
            .unwrap_or_default();
        titles.sort();
        self.music.lists[0].titles = titles.into_iter().map(|t| (t, 2500)).collect();
        self.music.lists.resize(6, Playlist::default());
        if let Some(t) = self.music.lists[0].active().map(str::to_string) {
            let vol = self.music_volume();
            self.play_stream(&t, vol, rules::PAN_CENTER, Some(0), 0.0)?;
        }
        Ok(())
    }

    /// Feed one event. Returns what happened to a sound it plays (`Fx`, `Ui`).
    pub fn handle(&mut self, ev: &AudioEvent) -> Result<Option<Played>> {
        match *ev {
            AudioEvent::Fx { id, x, y } => return self.play_fx(id, x, y).map(Some),
            AudioEvent::Ui { id } => return self.play_ui(id).map(Some),
            AudioEvent::WarAction => self.director.war_action(),
            AudioEvent::Ambient { x, y, flags } => self.update_ambient(x, y, flags)?,
            AudioEvent::LogicTick { centre_dark } => {
                if !self.user_mp3
                    && self.music.race.is_some()
                    && let Some(style) = self.director.tick(centre_dark)
                {
                    self.change_style(style)?;
                }
            }
        }
        Ok(None)
    }

    /// Advance wall-clock time: fade steps every 71 ms, playlist successors, voice bookkeeping.
    /// Call once per frame (or per rendered block when rendering offline).
    pub fn update(&mut self, dt: Duration) -> Result<()> {
        self.fade_clock += dt;
        let step = Duration::from_millis(rules::FADE_STEP_MS as u64);
        while self.fade_clock >= step {
            self.fade_clock -= step;
            self.step_fades();
        }
        self.titles_ended()?;
        self.voices.retain(|v| v.0.state() != PlaybackState::Stopped);
        Ok(())
    }

    /// Current music streams.
    pub fn music_streams(&self) -> Vec<StreamInfo> {
        self.music.streams.iter().map(|s| (s.id, s.list, s.volume)).collect()
    }
    pub fn music_style(&self) -> MusicStyle {
        self.director.style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        // Miles: full volume in the centre is 128·127/8 = 2032 = 0x7f0 on both channels.
        assert_eq!(miles_gains(127, 64), (1.0, 1.0));
        assert_eq!(miles_gains(127, 127), (0.0, 1.0));
        assert_eq!(miles_gains(127, 0), (1.0, 0.0));
        // pan 32: right t[32] = 64 → half, left full; volume linear.
        assert_eq!(miles_gains(127, 32), (1.0, (64 * 127 / 8) as f32 / 2032.0));
        assert_eq!(miles_gains(64, 64).0, (128 * 64 / 8) as f32 / 2032.0);
        assert_eq!(miles_gains(0, 64), (0.0, 0.0));
        // kira reproduces the gains: L = √(1−m)·√2·amp, R = √m·√2·amp.
        for (v, p) in [(127, 64), (100, 10), (40, 120), (127, 127), (5, 63)] {
            let (l, r) = miles_gains(v, p);
            let (db, pn) = miles_mix(v, p);
            let amp = 10f32.powf(db.0 / 20.0);
            let m = (pn.0 + 1.0) / 2.0;
            assert!(((1.0 - m).sqrt() * 2f32.sqrt() * amp - l).abs() < 1e-5, "{v} {p}");
            assert!((m.sqrt() * 2f32.sqrt() * amp - r).abs() < 1e-5, "{v} {p}");
        }
        assert_eq!(miles_mix(0, 64).0, Decibels::SILENCE);
    }

    #[test]
    fn view_probe() {
        let v = View::centred_on(100.0, 100.0, 1024, 768, 24.0, 256);
        let p = v.probe(100, 100);
        assert_eq!(p.screen, Some((512, 384)));
        assert_eq!(v.probe(300, 0).screen, None);
        assert!((v.zoom_factor() - 17.241379).abs() < 1e-4);
    }
}
