//! Textures of reference strips and pack frames: decoded on worker threads, uploaded a few per
//! frame on the UI thread, kept in a least-recently-used cache bounded by size.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

use egui::{ColorImage, Context, TextureHandle, TextureId, TextureOptions};

use crate::model::{Group, Seq, Slot, Spec};
use crate::pack::{Pack, frame_path, mask_path};
use crate::pngio::{Rgba, decode_rgba, parse_ramp, recolour, tint};

/// GPU memory the cache may hold; only images in use may exceed it.
const BUDGET: usize = 1 << 30;
/// Bytes uploaded per UI frame, so a big sequence does not stall playback.
const UPLOAD_PER_FRAME: usize = 48 << 20;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// A reference strip, recoloured for a player where `team` is set.
    Strip { rel: Arc<str>, team: Option<u8> },
    /// A pack frame; `team` where a mask tints it. The stamps invalidate replaced files.
    Frame { rel: Arc<str>, stamp: u64, mask_stamp: u64, team: u8 },
}

#[derive(Clone, Debug)]
enum Work {
    Strip { path: PathBuf, recolour: Option<(PathBuf, [u8; 3], String)> },
    Frame { path: PathBuf, mask: Option<(PathBuf, [u8; 3])> },
}

struct Request {
    key: Key,
    work: Work,
}

struct Decoded {
    key: Key,
    image: Result<ColorImage, String>,
}

#[derive(Default)]
struct Queue {
    /// Wanted right now: newest first.
    hot: Vec<Request>,
    /// Preloading: oldest first.
    cold: VecDeque<Request>,
    quit: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

enum Entry {
    Loading,
    Ready { handle: TextureHandle, size: [usize; 2], used: u64 },
    Failed,
}

/// A texture and its size in pixels.
#[derive(Clone, Copy, Debug)]
pub struct Tex {
    pub id: TextureId,
    pub w: f32,
    pub h: f32,
}

/// One of your frames.
#[derive(Clone, Copy, Debug)]
pub enum Yours {
    /// Not in the pack.
    None,
    Loading,
    Ready(Tex),
}

pub struct Images {
    ctx: Context,
    shared: Arc<Shared>,
    results: Receiver<Decoded>,
    entries: HashMap<Key, Entry>,
    in_flight: usize,
    tick: u64,
    bytes: usize,
    workers: Vec<std::thread::JoinHandle<()>>,
}

fn decode(work: &Work) -> Result<Rgba, String> {
    let read = |p: &PathBuf| -> Result<Rgba, String> {
        let bytes = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        decode_rgba(&bytes).map_err(|e| format!("{}: {e:#}", p.display()))
    };
    match work {
        Work::Strip { path, recolour: rc } => {
            let mut strip = read(path)?;
            if let Some((mask, player, hex)) = rc {
                recolour(&mut strip, &read(mask)?, *player, &parse_ramp(hex));
            }
            Ok(strip)
        }
        Work::Frame { path, mask } => {
            let mut frame = read(path)?;
            if let Some((m, player)) = mask {
                tint(&mut frame, &read(m)?, *player);
            }
            Ok(frame)
        }
    }
}

fn worker(shared: Arc<Shared>, tx: Sender<Decoded>, ctx: Context) {
    loop {
        let req = {
            let mut q = shared.queue.lock().expect("image queue");
            loop {
                if q.quit {
                    return;
                }
                if let Some(r) = q.hot.pop().or_else(|| q.cold.pop_front()) {
                    break r;
                }
                q = shared.wake.wait(q).expect("image queue");
            }
        };
        let image = decode(&req.work).map(|i| ColorImage::from_rgba_unmultiplied([i.w as usize, i.h as usize], &i.px));
        if tx.send(Decoded { key: req.key, image }).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

impl Images {
    pub fn new(ctx: &Context) -> Images {
        let shared = Arc::new(Shared { queue: Mutex::new(Queue::default()), wake: Condvar::new() });
        let (tx, results) = channel();
        let n = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(2, 8));
        let workers = (0..n)
            .map(|_| {
                let (s, t, c) = (shared.clone(), tx.clone(), ctx.clone());
                std::thread::spawn(move || worker(s, t, c))
            })
            .collect();
        Images { ctx: ctx.clone(), shared, results, entries: HashMap::new(), in_flight: 0, tick: 0, bytes: 0, workers }
    }

    /// Drops everything queued: the page changed.
    pub fn new_page(&mut self) {
        let mut guard = self.shared.queue.lock().expect("image queue");
        let q = &mut *guard;
        let dropped: Vec<Key> = q.hot.drain(..).map(|r| r.key).chain(q.cold.drain(..).map(|r| r.key)).collect();
        drop(guard);
        for k in dropped {
            self.entries.remove(&k);
            self.in_flight -= 1;
        }
    }

    /// Images still queued or decoding.
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.in_flight
    }

    fn request(&mut self, key: Key, work: Work, hot: bool) {
        self.entries.insert(key.clone(), Entry::Loading);
        self.in_flight += 1;
        let mut q = self.shared.queue.lock().expect("image queue");
        if hot {
            q.hot.push(Request { key, work });
        } else {
            q.cold.push_back(Request { key, work });
        }
        drop(q);
        self.shared.wake.notify_one();
    }

    fn touch(&mut self, key: &Key) -> Option<Option<Tex>> {
        let tick = self.tick;
        match self.entries.get_mut(key)? {
            Entry::Ready { handle, size, used } => {
                *used = tick;
                Some(Some(Tex { id: handle.id(), w: size[0] as f32, h: size[1] as f32 }))
            }
            _ => Some(None),
        }
    }

    /// A sequence's reference strip, in the player's colours where the group has team ramps. While
    /// those load, the plain strip stands in.
    pub fn strip(&mut self, spec: &Spec, g: &Group, seq: &Seq, team: usize) -> Option<Tex> {
        let reference = seq.reference.as_deref()?;
        let rel: Arc<str> = reference.into();
        let plain = Key::Strip { rel: rel.clone(), team: None };
        let mut coloured = None;
        if g.has_team_ramp(seq)
            && let (Some(mask), Some(pal)) = (seq.mask.as_deref(), seq.pal)
            && let Some(hex) = g.team_ramps.get(&pal.to_string()).and_then(|r| r.get(team))
        {
            let key = Key::Strip { rel: rel.clone(), team: Some(team as u8) };
            match self.touch(&key) {
                Some(found) => coloured = found,
                None => {
                    let work = Work::Strip { path: spec.ref_path(reference), recolour: Some((spec.ref_path(mask), spec.player_colors[team], hex.clone())) };
                    self.request(key, work, true);
                }
            }
        }
        if coloured.is_some() {
            return coloured;
        }
        match self.touch(&plain) {
            Some(found) => found,
            None => {
                self.request(plain, Work::Strip { path: spec.ref_path(reference), recolour: None }, true);
                None
            }
        }
    }

    /// Your frame `j`, tinted for the player where it has a team mask. `hot`: it is on show now
    /// (else it is preloaded behind everything that is).
    #[allow(clippy::too_many_arguments)]
    pub fn frame(&mut self, spec: &Spec, pack: &Pack, g: &Group, slot: &Slot, seq: &Seq, j: usize, team: usize, hot: bool) -> Yours {
        let rel = frame_path(g, slot, seq, j);
        let Some(rec) = pack.file(&rel) else { return Yours::None };
        let mask = mask_path(g, slot, seq, j);
        let m = pack.file(&mask);
        let key = Key::Frame { rel: rel.as_str().into(), stamp: rec.stamp, mask_stamp: m.map_or(0, |f| f.stamp), team: if m.is_some() { team as u8 } else { 0 } };
        match self.touch(&key) {
            Some(Some(t)) => Yours::Ready(t),
            Some(None) => Yours::Loading,
            None => {
                let work = Work::Frame { path: pack.abs(&rel), mask: m.map(|_| (pack.abs(&mask), spec.player_colors[team])) };
                self.request(key, work, hot);
                Yours::Loading
            }
        }
    }

    /// Call once per UI frame: uploads finished images and trims the cache.
    pub fn end_frame(&mut self) {
        self.tick += 1;
        let mut uploaded = 0;
        while uploaded < UPLOAD_PER_FRAME {
            let Ok(d) = self.results.try_recv() else { break };
            self.in_flight -= 1;
            match d.image {
                Ok(img) => {
                    let size = img.size;
                    let n = size[0] * size[1] * 4;
                    uploaded += n;
                    self.bytes += n;
                    let handle = self.ctx.load_texture("studio-image", img, TextureOptions::NEAREST);
                    self.entries.insert(d.key, Entry::Ready { handle, size, used: self.tick });
                }
                Err(e) => {
                    eprintln!("studio: {e}");
                    self.entries.insert(d.key, Entry::Failed);
                }
            }
        }
        if self.in_flight > 0 {
            self.ctx.request_repaint();
        }
        if self.bytes > BUDGET {
            self.trim();
        }
    }

    fn trim(&mut self) {
        let mut ready: Vec<(u64, Key, usize)> = self
            .entries
            .iter()
            .filter_map(|(k, e)| match e {
                Entry::Ready { size, used, .. } => Some((*used, k.clone(), size[0] * size[1] * 4)),
                _ => None,
            })
            .collect();
        ready.sort_by_key(|r| r.0);
        for (used, key, n) in ready {
            if self.bytes <= BUDGET * 3 / 4 || used + 2 >= self.tick {
                break;
            }
            self.entries.remove(&key);
            self.bytes -= n;
        }
    }
}

impl Drop for Images {
    fn drop(&mut self) {
        self.shared.queue.lock().expect("image queue").quit = true;
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}
