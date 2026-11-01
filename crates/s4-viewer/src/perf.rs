//! Frame-time accounting for benchmarks (s4-game `--bench`): named spans summed over all threads
//! (the sim thread's too). Off by default; a span then costs one atomic load.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static ON: AtomicBool = AtomicBool::new(false);
static SUMS: Mutex<BTreeMap<&'static str, Sum>> = Mutex::new(BTreeMap::new());

/// A span's total time, count and longest single time.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sum {
    pub total: Duration,
    pub count: u32,
    pub max: Duration,
}

/// Start or stop recording.
pub fn enable(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// A running span; its time is added to `name` when dropped.
pub struct Span {
    name: &'static str,
    start: Option<Instant>,
}

/// Time the rest of the scope as `name` (nothing when recording is off).
pub fn span(name: &'static str) -> Span {
    Span { name, start: ON.load(Ordering::Relaxed).then(Instant::now) }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(s) = self.start {
            add(self.name, s.elapsed());
        }
    }
}

/// Add `d` to `name`.
pub fn add(name: &'static str, d: Duration) {
    let mut m = SUMS.lock().unwrap_or_else(|e| e.into_inner());
    let e = m.entry(name).or_default();
    e.total += d;
    e.count += 1;
    e.max = e.max.max(d);
}

/// Count `n` things under `name` (its calls are the count; no time).
pub fn add_n(name: &'static str, n: u32) {
    if ON.load(Ordering::Relaxed) {
        let mut m = SUMS.lock().unwrap_or_else(|e| e.into_inner());
        m.entry(name).or_default().count += n;
    }
}

/// The sums so far, cleared.
pub fn take() -> BTreeMap<&'static str, Sum> {
    std::mem::take(&mut *SUMS.lock().unwrap_or_else(|e| e.into_inner()))
}
