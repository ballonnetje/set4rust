//! [ours] The videos: the install's Bink files converted once into the user's cache ([`convert`]),
//! played back from there (AV1 in IVF through rav1d, [`Stream`]; the Ogg Vorbis sound is the
//! caller's, through its audio engine). The original plays them with RAD's `binkw32.dll`
//! (`CStateVideo`); see docs/VIDEO.md for when.

pub mod av1;
pub mod convert;
pub mod ivf;

use std::path::Path;
use std::sync::mpsc;

pub use convert::{Converted, Converter, Source};

/// A decoded picture: 8-bit 4:2:0 planes, tightly packed (`u` / `v` are half size, rounded up).
#[derive(Clone)]
pub struct Frame {
    /// When it is shown, in seconds from the start.
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl Source {
    /// The source for an install file (`<game>\VideoEN\Intro_US.bik` → `VideoEN`, `Intro_US`).
    pub fn of(path: &Path) -> Source {
        let name = |p: Option<&std::ffi::OsStr>| p.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Source { path: path.to_path_buf(), folder: name(path.parent().and_then(Path::file_name)), stem: name(path.file_stem()) }
    }
}

/// A converted video being decoded ahead on its own thread.
pub struct Stream {
    rx: mpsc::Receiver<anyhow::Result<Frame>>,
    next: Option<Frame>,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    ended: bool,
}

impl Stream {
    pub fn open(path: &Path) -> anyhow::Result<Stream> {
        let ivf = ivf::Ivf::open(path)?;
        if &ivf.fourcc != b"AV01" {
            anyhow::bail!("{}: not AV1", path.display());
        }
        let (width, height, duration) = (ivf.width, ivf.height, ivf.duration());
        // A few frames ahead: decoding never stalls the window's thread.
        let (tx, rx) = mpsc::sync_channel(6);
        std::thread::Builder::new().name("video decode".into()).spawn(move || {
            let mut dec = match av1::Decoder::new() {
                Ok(d) => d,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            for i in 0..ivf.len() {
                let (t, data) = ivf.frame(i);
                match dec.decode(data, t) {
                    Ok(Some(f)) => {
                        if tx.send(Ok(f)).is_err() {
                            return; // the stream was dropped
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                }
            }
        })?;
        Ok(Stream { rx, next: None, width, height, duration, ended: false })
    }

    /// The newest frame due at `time` seconds that was not returned yet (older ones are dropped:
    /// a slow window skips pictures, never the clock). None when nothing new is due.
    pub fn frame_at(&mut self, time: f64) -> Option<Frame> {
        let mut out = None;
        loop {
            if self.next.is_none() && !self.ended {
                match self.rx.try_recv() {
                    Ok(Ok(f)) => self.next = Some(f),
                    Ok(Err(e)) => {
                        tracing::warn!("video: {e:#}");
                        self.ended = true;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => self.ended = true,
                    Err(mpsc::TryRecvError::Empty) => return out,
                }
            }
            match self.next.take() {
                Some(f) if f.time <= time => out = Some(f),
                other => {
                    self.next = other;
                    return out;
                }
            }
        }
    }

    /// Every frame was handed out (or decoding failed).
    pub fn ended(&self) -> bool {
        self.ended && self.next.is_none()
    }
}
