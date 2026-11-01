//! [ours] The first launch's conversion of the install's Bink videos (`Video*\*.bik`, RAD's codec,
//! which nothing modern plays) into the user's cache: AV1 in IVF plus Ogg Vorbis, by ffmpeg.
//! Nothing converted is shipped; each player converts their own install's files once.
//!
//! The cache is `<cache>\<folder>\<stem>.ivf` / `.ogg` with `<stem>.stamp` written last (the
//! format version and the source's size and modified time): a file whose stamp differs, or has
//! none, is converted again. ffmpeg comes from `S4RS_FFMPEG`, next to the exe, PATH, or the usual
//! Windows package folders (winget, Chocolatey, Scoop); without one the videos are skipped.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};

/// Bump when the conversion's settings change: every cached video is converted again.
pub const FORMAT: u32 = 1;

/// A video of the install: the file, and its place in the cache (folder and stem as on disk).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub folder: String,
    pub stem: String,
}

/// The converted files of a source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Converted {
    pub video: PathBuf,
    /// None when the source has no sound.
    pub audio: Option<PathBuf>,
}

/// Every `*.bik` in the install's `Video*` folders (`Video`, `VideoEN`, `VideoFR`, ...).
pub fn sources(game: &Path) -> Vec<Source> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(game) else { return out };
    let mut dirs: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
    dirs.sort_by_key(|e| e.file_name());
    for d in dirs {
        let folder = d.file_name().to_string_lossy().into_owned();
        if !folder.to_ascii_lowercase().starts_with("video") {
            continue;
        }
        let Ok(files) = std::fs::read_dir(d.path()) else { continue };
        let mut files: Vec<_> = files.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("bik"))).collect();
        files.sort();
        for path in files {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            out.push(Source { path, folder: folder.clone(), stem });
        }
    }
    out
}

fn stamp_text(src: &Path) -> Option<String> {
    let m = std::fs::metadata(src).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("s4rs-video {FORMAT}\n{} {t}\n", m.len()))
}

fn out_paths(cache: &Path, s: &Source) -> (PathBuf, PathBuf, PathBuf) {
    let d = cache.join(&s.folder);
    (d.join(format!("{}.ivf", s.stem)), d.join(format!("{}.ogg", s.stem)), d.join(format!("{}.stamp", s.stem)))
}

/// The source's converted files when the cache has them for this very file.
pub fn cached(cache: &Path, s: &Source) -> Option<Converted> {
    let (video, audio, stamp) = out_paths(cache, s);
    let have = std::fs::read_to_string(&stamp).ok()?;
    let want = stamp_text(&s.path)?;
    // The stamp's last line says whether the source had sound.
    let (body, sound) = have.rsplit_once("audio ")?;
    if body != want || !video.is_file() {
        return None;
    }
    let audio = (sound.trim() == "1" && audio.is_file()).then_some(audio);
    Some(Converted { video, audio })
}

/// The encoders an ffmpeg build offers for us: (AV1 arguments, Vorbis arguments).
#[derive(Clone, Debug)]
pub struct Ffmpeg {
    pub exe: PathBuf,
    video_args: Vec<&'static str>,
    audio_args: Vec<&'static str>,
}

impl Ffmpeg {
    /// Check `exe`: it runs and has an AV1 and a Vorbis encoder.
    pub fn probe(exe: &Path) -> anyhow::Result<Ffmpeg> {
        let out = quiet(Command::new(exe).args(["-hide_banner", "-encoders"])).stdin(Stdio::null()).stderr(Stdio::null()).output().with_context(|| exe.display().to_string())?;
        let list = String::from_utf8_lossy(&out.stdout);
        let has = |name: &str| list.lines().any(|l| l.split_whitespace().nth(1) == Some(name));
        // SVT-AV1 preset 8 / CRF 26: ~8x real time on 4 cores at 640×480; PSNR ~44 dB against
        // the Bink decode (Intro.bik). libaom is far slower at any setting.
        let video_args = if has("libsvtav1") {
            vec!["-c:v", "libsvtav1", "-preset", "8", "-crf", "26", "-g", "250"]
        } else if has("libaom-av1") {
            vec!["-c:v", "libaom-av1", "-cpu-used", "8", "-row-mt", "1", "-crf", "28", "-b:v", "0", "-g", "250"]
        } else if has("librav1e") {
            vec!["-c:v", "librav1e", "-speed", "10", "-qp", "80"]
        } else {
            bail!("{}: no AV1 encoder (libsvtav1, libaom-av1, librav1e)", exe.display());
        };
        let audio_args = if has("libvorbis") {
            vec!["-c:a", "libvorbis", "-q:a", "5"]
        } else if has("vorbis") {
            vec!["-c:a", "vorbis", "-strict", "-2", "-q:a", "5"]
        } else {
            bail!("{}: no Vorbis encoder", exe.display());
        };
        Ok(Ffmpeg { exe: exe.to_path_buf(), video_args, audio_args })
    }

    /// The first working ffmpeg of the places listed in the module header.
    pub fn find() -> Option<Ffmpeg> {
        let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        let mut c: Vec<PathBuf> = Vec::new();
        if let Some(p) = std::env::var_os("S4RS_FFMPEG") {
            c.push(p.into());
        }
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            c.extend([dir.join(exe_name), dir.join("ffmpeg").join(exe_name), dir.join("tools").join(exe_name)]);
        }
        if let Some(path) = std::env::var_os("PATH") {
            c.extend(std::env::split_paths(&path).map(|d| d.join(exe_name)));
        }
        if cfg!(windows) {
            // Installed after the game's environment was made (winget's PATH change needs a new
            // shell): the package managers' own folders.
            if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
                c.push(local.join("Microsoft").join("WinGet").join("Links").join(exe_name));
                let pk = local.join("Microsoft").join("WinGet").join("Packages");
                for e in std::fs::read_dir(&pk).into_iter().flatten().flatten() {
                    if e.file_name().to_string_lossy().to_ascii_lowercase().contains("ffmpeg") {
                        for sub in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                            c.push(sub.path().join("bin").join(exe_name));
                        }
                    }
                }
            }
            if let Some(pd) = std::env::var_os("ProgramData") {
                c.push(PathBuf::from(pd).join("chocolatey").join("bin").join(exe_name));
            }
            if let Some(home) = std::env::var_os("USERPROFILE") {
                c.push(PathBuf::from(home).join("scoop").join("shims").join(exe_name));
            }
        }
        c.into_iter().filter(|p| p.is_file()).find_map(|p| Ffmpeg::probe(&p).inspect_err(|e| tracing::info!("video: {e}")).ok())
    }
}

/// No console window for ffmpeg in the windowed build, at a lower priority than the game.
fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    cmd
}

/// Run ffmpeg to the end, or kill it when `cancel` is set (the game quits).
fn run(cmd: &mut Command, cancel: &AtomicBool) -> anyhow::Result<()> {
    let mut child = quiet(cmd).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
    // stderr is read on its own thread: a full pipe would stall ffmpeg.
    let mut err = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = std::io::Read::read_to_string(e, &mut s);
        }
        s
    });
    loop {
        if let Some(st) = child.try_wait()? {
            let log = reader.join().unwrap_or_default();
            if !st.success() {
                let tail: Vec<_> = log.lines().filter(|l| !l.starts_with("Svt[")).collect();
                bail!("ffmpeg {st}: {}", tail[tail.len().saturating_sub(4)..].join(" | "));
            }
            return Ok(());
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("cancelled");
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

/// Convert one source into the cache (temporary names, renamed when complete; the stamp last).
pub fn convert(ff: &Ffmpeg, cache: &Path, s: &Source, cancel: &AtomicBool) -> anyhow::Result<Converted> {
    let (video, audio, stamp) = out_paths(cache, s);
    std::fs::create_dir_all(video.parent().unwrap())?;
    let _ = std::fs::remove_file(&stamp);
    let part = |p: &Path| p.with_extension(format!("{}.part", p.extension().unwrap().to_string_lossy()));
    let (vpart, apart) = (part(&video), part(&audio));
    let input = |c: &mut Command| {
        c.args(["-hide_banner", "-nostdin", "-v", "error", "-y", "-i"]).arg(&s.path);
    };
    let mut c = Command::new(&ff.exe);
    input(&mut c);
    c.args(["-map", "0:v:0", "-an", "-pix_fmt", "yuv420p"]).args(&ff.video_args).args(["-f", "ivf"]).arg(&vpart);
    run(&mut c, cancel).with_context(|| format!("{}: video", s.path.display()))?;
    std::fs::rename(&vpart, &video)?;
    let mut c = Command::new(&ff.exe);
    input(&mut c);
    c.args(["-map", "0:a:0", "-vn"]).args(&ff.audio_args).args(["-f", "ogg"]).arg(&apart);
    // A video without sound (or a sound ffmpeg cannot read) plays silent.
    let sound = match run(&mut c, cancel) {
        Ok(()) => std::fs::rename(&apart, &audio).is_ok(),
        Err(e) if cancel.load(Ordering::Relaxed) => return Err(e),
        Err(e) => {
            tracing::warn!("video: {}: no sound: {e:#}", s.path.display());
            let _ = std::fs::remove_file(&apart);
            false
        }
    };
    let text = stamp_text(&s.path).context("source vanished")?;
    std::fs::write(&stamp, format!("{text}audio {}\n", sound as u8))?;
    Ok(Converted { video, audio: sound.then_some(audio) })
}

/// What the background conversion has done so far.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Sources to convert in this run, and how many are through (converted or failed).
    pub total: usize,
    pub done: usize,
    /// The one being converted: (folder, stem).
    pub current: Option<(String, String)>,
    pub failed: Vec<(Source, String)>,
    pub finished: bool,
}

/// The background conversion of every source not in the cache yet, one at a time.
pub struct Converter {
    status: Arc<Mutex<Status>>,
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Converter {
    /// Start converting what `cache` lacks, the sources `first(folder, stem)` names before the
    /// others. None when nothing is missing; `Err` when something is but ffmpeg cannot be found.
    pub fn start(game: &Path, cache: &Path, first: impl Fn(&str, &str) -> bool) -> anyhow::Result<Option<Converter>> {
        let mut todo: Vec<Source> = sources(game).into_iter().filter(|s| cached(cache, s).is_none()).collect();
        if todo.is_empty() {
            return Ok(None);
        }
        todo.sort_by_key(|s| !first(&s.folder, &s.stem));
        let ff = Ffmpeg::find().context("no ffmpeg with an AV1 encoder found (install it, or set S4RS_FFMPEG): the videos are skipped")?;
        tracing::info!("video: converting {} Bink videos with {}", todo.len(), ff.exe.display());
        let status = Arc::new(Mutex::new(Status { total: todo.len(), ..Default::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (st, cn, cache) = (status.clone(), cancel.clone(), cache.to_path_buf());
        let thread = std::thread::Builder::new()
            .name("video convert".into())
            .spawn(move || {
                for s in todo {
                    if cn.load(Ordering::Relaxed) {
                        break;
                    }
                    st.lock().unwrap().current = Some((s.folder.clone(), s.stem.clone()));
                    let t = std::time::Instant::now();
                    let r = convert(&ff, &cache, &s, &cn);
                    let mut g = st.lock().unwrap();
                    match r {
                        Ok(_) => tracing::info!("video: {}\\{} converted in {:.1} s", s.folder, s.stem, t.elapsed().as_secs_f32()),
                        Err(e) => {
                            tracing::warn!("video: {e:#}");
                            g.failed.push((s, format!("{e:#}")));
                        }
                    }
                    g.done += 1;
                    g.current = None;
                }
                st.lock().unwrap().finished = true;
            })?;
        Ok(Some(Converter { status, cancel, thread: Some(thread) }))
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    /// Whether `(folder, stem)` is waiting for or in its conversion in this run.
    pub fn pending(&self, folder: &str, stem: &str) -> bool {
        let st = self.status.lock().unwrap();
        !st.finished && !st.failed.iter().any(|(s, _)| s.folder.eq_ignore_ascii_case(folder) && s.stem.eq_ignore_ascii_case(stem))
    }

    /// Wait for the end (tests, the import tool).
    pub fn join(mut self) -> Status {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.status()
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
