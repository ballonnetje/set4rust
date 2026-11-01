//! The missing-graphics audit (s4-game `--audit`): every draw that finds no image, or falls
//! back (a frame past its job's end, a job or direction the file lacks), and every placeholder
//! image drawn (opaque black, `sprites::opaque_black`), counted by what it was. Off by default; a check then costs one atomic load.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
static MISSES: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

/// Start or stop recording.
pub fn enable(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether the audit is recording (checks that cost more than building a string ask first).
pub fn recording() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Count one miss described by `what` (built only while recording).
pub fn miss(what: impl FnOnce() -> String) {
    if ON.load(Ordering::Relaxed) {
        *MISSES.lock().unwrap_or_else(|e| e.into_inner()).entry(what()).or_default() += 1;
    }
}

/// The misses so far, cleared.
pub fn take() -> BTreeMap<String, u64> {
    std::mem::take(&mut *MISSES.lock().unwrap_or_else(|e| e.into_inner()))
}
