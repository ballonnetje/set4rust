//! [ours] Video playback in the window: the install's Bink files, converted on the first launch
//! into `<user dir>\video` (s4-video's `convert`), decoded by s4-video and drawn by the viewer's
//! video layer; the sound plays on its own kira manager and is the clock. When the original plays
//! which video is in docs/VIDEO.md.
//!
//! A video asked for while its conversion is still to come (the first launch) waits for it with a
//! progress bar (skippable like the video); one that cannot be had (no ffmpeg, a failed
//! conversion, no such file) is skipped at once.

use std::path::{Path, PathBuf};
use std::time::Instant;

use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use s4_video::{Converter, Source, Stream};
use s4_viewer::gpu::Gpu;

/// Video `n`'s file for `language` (the ctor's tables at 0x528537: 1 German 0x1019d54, 2 French
/// 0x1019a5c, else English 0x1019d28). No fallback to another folder: a missing file is skipped.
pub fn file(n: u8, language: u32) -> Option<PathBuf> {
    const NAMES: [&str; 11] = ["S4", "BB_Logo", "Intro", "Cut1", "Cut2", "Cut3", "Extro", "UbiSoft_Logo", "TroIntro", "TroScen5", "TroExtro"];
    const ENGLISH: [&str; 11] = ["Vorintro_US", "", "Intro_US", "Cut01", "Cut02", "Cut03", "extro_US", "", "TroIntro", "TroScen5", "TroExtro"];
    let name = *NAMES.get(n as usize)?;
    let (dir, name) = match (n, language) {
        (1 | 7, _) => ("Video", name),
        (_, 1) => ("Video", name),
        (_, 2) => ("VideoFR", name),
        _ => ("VideoEN", ENGLISH[n as usize]),
    };
    Some(Path::new(dir).join(format!("{name}.bik")))
}

/// Whether `file` (`Video\BB_Logo.bik`) is the source `folder` / `stem` (any case).
pub fn names(file: &Path, folder: &str, stem: &str) -> bool {
    let s = Source::of(file);
    s.folder.eq_ignore_ascii_case(folder) && s.stem.eq_ignore_ascii_case(stem)
}

/// The game start's videos (0x503a58, after the empty `upsell` slideshow): UbiSoft_Logo,
/// BB_Logo, then video 0; the main menu follows.
pub const STARTUP: [u8; 3] = [7, 1, 0];

/// The videos' cache and its background conversion.
pub struct Videos {
    /// Where the `Video*` folders are: the game folder, or (the portable build, whose imported
    /// data leave the videos out) the install it came from.
    root: Option<PathBuf>,
    cache: Option<PathBuf>,
    conv: Option<Converter>,
}

/// The folder with the install's `Video` folder: [`s4_formats::game_dir`], else the first
/// detected install that has one.
fn root() -> Option<PathBuf> {
    let has = |d: &Path| s4_formats::path::resolve(d.join("Video")).is_dir();
    let game = s4_formats::game_dir();
    if has(&game) {
        return Some(game);
    }
    s4_formats::install::candidates(None).into_iter().find(|d| has(d) && s4_formats::install::is_game_dir(d))
}

impl Videos {
    /// The cache, and the conversion of what it lacks started, the files `first` names first
    /// (folder, stem). Off (no cache: every video is skipped) when not `on` (`show_videos`), in
    /// test builds and with `S4RS_NO_VIDEO` set.
    pub fn start(on: bool, first: impl Fn(&str, &str) -> bool) -> Videos {
        if !on || cfg!(test) || std::env::var_os("S4RS_NO_VIDEO").is_some_and(|v| v != "0") {
            return Videos { root: None, cache: None, conv: None };
        }
        let root = root();
        if root.is_none() {
            tracing::info!("video: no Video folder in the game folder or a detected install");
        }
        let cache = std::env::var_os("S4RS_VIDEO_CACHE").map(PathBuf::from).or_else(|| crate::settings::user_dir().map(|d| d.join("video")));
        let conv = match (&root, &cache) {
            (Some(r), Some(c)) => Converter::start(r, c, first).inspect_err(|e| tracing::warn!("video: {e:#}")).ok().flatten(),
            _ => None,
        };
        Videos { root, cache, conv }
    }

    /// Play `file` of the install (`Video\Intro.bik`, any case), or None when it cannot be had.
    pub fn open(&self, file: &Path) -> Option<Playing> {
        let cache = self.cache.as_ref()?;
        let path = s4_formats::path::resolve(self.root.as_ref()?.join(file));
        if !path.is_file() {
            tracing::info!("video: {} not in the install", file.display());
            return None;
        }
        let src = Source::of(&path);
        let waiting = s4_video::convert::cached(cache, &src).is_none();
        if waiting && !self.conv.as_ref().is_some_and(|c| c.pending(&src.folder, &src.stem)) {
            tracing::info!("video: {} not converted", file.display());
            return None;
        }
        Some(Playing { src, stream: None, sound: None, started: Instant::now(), paused: None })
    }

    /// The conversion's progress (done, total), while it runs.
    fn progress(&self) -> Option<(usize, usize)> {
        let st = self.conv.as_ref()?.status();
        (!st.finished).then_some((st.done, st.total))
    }
}

/// `--convert-videos`: what the first launch converts in the background, now, with a line per
/// file.
pub fn convert_all() -> anyhow::Result<()> {
    let v = Videos::start(true, |_, _| false);
    let Some(conv) = v.conv else {
        println!("nothing to convert ({})", v.cache.map_or("no cache folder".into(), |c| format!("all in {}", c.display())));
        return Ok(());
    };
    let st = conv.join();
    println!("{} converted, {} failed", st.done - st.failed.len(), st.failed.len());
    for (s, e) in &st.failed {
        println!("failed: {}: {e}", s.path.display());
    }
    if st.failed.is_empty() { Ok(()) } else { anyhow::bail!("{} videos failed", st.failed.len()) }
}

type Sound = (kira::AudioManager, StreamingSoundHandle<kira::sound::FromFileError>);

/// A video on screen (or waiting for its conversion).
pub struct Playing {
    src: Source,
    stream: Option<Stream>,
    sound: Option<Sound>,
    /// The start of playback (or of the wait), moved on by the time spent paused.
    started: Instant,
    /// Paused since (the window inactive: 0x528760 on `WM_ACTIVATEAPP` → `BinkPause`).
    paused: Option<Instant>,
}

impl Playing {
    /// Advance to the frame due now and put it on `gpu`'s video layer (made here). False when the
    /// video is over (or cannot play after all): the caller takes the layer away
    /// ([`Playing::close`]).
    pub fn tick(&mut self, gpu: &mut Gpu, videos: &Videos, volume: f64) -> bool {
        if gpu.video.is_none() {
            gpu.video = Some(s4_viewer::video::VideoLayer::new(&gpu.device, gpu.format()));
        }
        if self.stream.is_none() {
            let Some(cache) = videos.cache.as_ref() else { return false };
            match s4_video::convert::cached(cache, &self.src) {
                Some(c) => {
                    if !self.begin(&c.video, c.audio.as_deref(), volume) {
                        return false;
                    }
                }
                None if videos.conv.as_ref().is_some_and(|c| c.pending(&self.src.folder, &self.src.stem)) => {
                    let (done, total) = videos.progress().unwrap_or((0, 1));
                    self.waiting(gpu, done as f32 / total.max(1) as f32);
                    return true;
                }
                None => return false,
            }
        }
        let Some(stream) = self.stream.as_mut() else { return false };
        let wall = (self.paused.unwrap_or_else(Instant::now) - self.started).as_secs_f64();
        // The sound's position once it runs (a stalled window does not drift from it).
        let t = self.sound.as_ref().map(|s| s.1.position()).filter(|&p| p > 0.0).unwrap_or(wall);
        if let Some(f) = stream.frame_at(t)
            && let Some(layer) = gpu.video.as_mut()
        {
            layer.set_frame(&gpu.device, &gpu.queue, (f.width, f.height), &f.y, &f.u, &f.v);
        }
        !(stream.ended() && t >= stream.duration)
    }

    fn begin(&mut self, video: &Path, audio: Option<&Path>, volume: f64) -> bool {
        match Stream::open(video) {
            Ok(s) => self.stream = Some(s),
            Err(e) => {
                tracing::warn!("video: {e:#}");
                return false;
            }
        }
        self.sound = audio.and_then(|a| {
            let r = (|| -> anyhow::Result<Sound> {
                let mut m = crate::sound::new_manager()?;
                let data = StreamingSoundData::from_file(a)?.volume(kira::Decibels(amplitude_db(volume)));
                let h = m.play(data)?;
                Ok((m, h))
            })();
            r.inspect_err(|e| tracing::debug!("video sound: {e:#}")).ok()
        });
        self.started = Instant::now();
        true
    }

    /// Pause while the window is inactive, go on when it is active again.
    pub fn pause(&mut self, on: bool) {
        match (on, self.paused) {
            (true, None) => {
                self.paused = Some(Instant::now());
                if let Some((_, h)) = self.sound.as_mut() {
                    h.pause(Default::default());
                }
            }
            (false, Some(since)) => {
                self.started += since.elapsed();
                self.paused = None;
                if let Some((_, h)) = self.sound.as_mut() {
                    h.resume(Default::default());
                }
            }
            _ => {}
        }
    }

    /// The first launch's wait: black with a bar of the conversion's progress.
    fn waiting(&mut self, gpu: &mut Gpu, part: f32) {
        let (w, h) = (640u32, 480u32);
        let mut y = vec![16u8; (w * h) as usize];
        let (x0, x1, y0, y1) = (160, 480, 300, 312);
        let fill = x0 + ((x1 - x0) as f32 * part.clamp(0.0, 1.0)) as u32;
        for row in y0..y1 {
            for x in x0..x1 {
                let edge = row == y0 || row == y1 - 1 || x == x0 || x == x1 - 1;
                y[(row * w + x) as usize] = if edge || x < fill { 180 } else { 40 };
            }
        }
        let c = vec![128u8; (w.div_ceil(2) * h.div_ceil(2)) as usize];
        if let Some(layer) = gpu.video.as_mut() {
            layer.set_frame(&gpu.device, &gpu.queue, (w, h), &y, &c, &c);
        }
    }

    /// Stop the sound and take the layer away.
    pub fn close(self, gpu: &mut Gpu) {
        if let Some((_m, mut h)) = self.sound {
            h.stop(Default::default());
        }
        gpu.video = None;
    }
}

/// A linear volume (0..=1) in decibels for kira.
fn amplitude_db(v: f64) -> f32 {
    if v <= 0.0 { kira::Decibels::SILENCE.0 } else { (20.0 * v.min(1.0).log10()) as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_by_language() {
        let f = |n, l| file(n, l).map(|p| p.to_string_lossy().replace('\\', "/"));
        assert_eq!(f(0, 1).as_deref(), Some("Video/S4.bik"));
        assert_eq!(f(0, 2).as_deref(), Some("VideoFR/S4.bik"));
        assert_eq!(f(0, 0).as_deref(), Some("VideoEN/Vorintro_US.bik"));
        assert_eq!(f(7, 0).as_deref(), Some("Video/UbiSoft_Logo.bik"));
        assert_eq!(f(1, 2).as_deref(), Some("Video/BB_Logo.bik"));
        assert_eq!(f(3, 0).as_deref(), Some("VideoEN/Cut01.bik"));
        assert_eq!(f(6, 0).as_deref(), Some("VideoEN/extro_US.bik"));
        assert_eq!(f(9, 2).as_deref(), Some("VideoFR/TroScen5.bik"));
        assert_eq!(f(11, 0), None);
        // Every name of every language is in a full install (the HE has all 29).
        let game = s4_formats::game_dir();
        if game.join("Video").is_dir() {
            for l in 0..3 {
                for n in 0..11 {
                    let p = s4_formats::path::resolve(game.join(file(n, l).unwrap()));
                    assert!(p.is_file(), "{}", p.display());
                }
            }
        }
    }

    /// A real conversion (UbiSoft_Logo, ~2 s with SVT-AV1) into a temporary cache, decoded and
    /// drawn by the video layer at 1280×720 (pillarboxed 4:3): captures
    /// `out/video/logo_<t>.png`. Skipped without the install's videos or an ffmpeg.
    #[test]
    fn converts_decodes_and_draws() {
        let Some(root) = root() else { return };
        let Some(ff) = s4_video::convert::Ffmpeg::find() else {
            eprintln!("no ffmpeg: skipped");
            return;
        };
        let cache = std::env::temp_dir().join(format!("s4rs-video-test-{}", std::process::id()));
        let src = Source::of(&s4_formats::path::resolve(root.join(file(7, 0).unwrap())));
        let c = s4_video::convert::convert(&ff, &cache, &src, &std::sync::atomic::AtomicBool::new(false)).unwrap();
        assert_eq!(s4_video::convert::cached(&cache, &src), Some(c.clone()));
        assert!(c.audio.is_some());
        let mut stream = Stream::open(&c.video).unwrap();
        assert_eq!((stream.width, stream.height), (640, 480));
        assert!((stream.duration - 9.12).abs() < 0.05, "{}", stream.duration);
        let mut gpu = Gpu::headless(1280, 720).unwrap();
        gpu.video = Some(s4_viewer::video::VideoLayer::new(&gpu.device, gpu.format()));
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/video");
        std::fs::create_dir_all(&out).unwrap();
        let mut frames = 0;
        for t in [1.0, 4.0, 8.0] {
            let deadline = Instant::now() + std::time::Duration::from_secs(20);
            let f = loop {
                if let Some(f) = stream.frame_at(t) {
                    frames += 1;
                    if f.time + 0.04 > t {
                        break f;
                    }
                    continue;
                }
                assert!(Instant::now() < deadline, "no frame at {t}");
                std::thread::sleep(std::time::Duration::from_millis(5));
            };
            gpu.video.as_mut().unwrap().set_frame(&gpu.device, &gpu.queue, (f.width, f.height), &f.y, &f.u, &f.v);
            let rgba = gpu.capture(s4_viewer::gpu::View { x: 0.0, y: 0.0, zoom: 1.0 }).unwrap();
            // Pillarboxes: (1280 − 960) / 2 = 160 px of black on each side.
            assert!(rgba[..160 * 4].iter().enumerate().all(|(i, &b)| i % 4 == 3 || b == 0));
            s4_viewer::client::write_png(&out.join(format!("logo_{t}.png")), 1280, 720, &rgba).unwrap();
        }
        assert!(frames >= 3);
        let _ = std::fs::remove_dir_all(&cache);
    }
}
