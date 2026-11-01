//! The log testers send with bug reports: everything traced also goes to
//! `%APPDATA%\s4rs\logs\s4rs-<date>.log` (appended per day, the newest 10 kept), and a panic is
//! logged with its backtrace before a message box points at the file.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// How many daily log files stay.
const KEEP: usize = 10;

static FILE: OnceLock<PathBuf> = OnceLock::new();

/// The log folder (`%APPDATA%\s4rs\logs`; `S4RS_LOG_DIR` overrides it).
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("S4RS_LOG_DIR") {
        return Some(d.into());
    }
    crate::progress::path().and_then(|p| p.parent().map(|d| d.join("logs")))
}

/// This run's log file, once [`init`] opened it.
pub fn file() -> Option<&'static Path> {
    FILE.get().map(PathBuf::as_path)
}

/// `YYYY-MM-DD` (UTC) of a Unix time (days → civil date, proleptic Gregorian).
fn date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Removes all but the newest `keep` `s4rs-*.log` files of `dir` (by name: the date).
fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut logs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("s4rs-") && n.ends_with(".log")))
        .collect();
    logs.sort();
    let n = logs.len().saturating_sub(keep);
    for p in &logs[..n] {
        let _ = std::fs::remove_file(p);
    }
}

/// Tracing to standard error and the day's log file; the panic hook. `gui`: a panic also shows
/// a message box.
pub fn init(gui: bool) {
    let stderr = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
    let opened = dir().and_then(|d| {
        std::fs::create_dir_all(&d).ok()?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |t| t.as_secs());
        let path = d.join(format!("s4rs-{}.log", date(now)));
        let f = std::fs::OpenOptions::new().create(true).append(true).open(&path).ok()?;
        prune(&d, KEEP);
        let _ = FILE.set(path);
        Some(f)
    });
    let file_layer = opened.map(|f| tracing_subscriber::fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(f)));
    // INFO and up, as `fmt::init` had it (the GPU libraries trace a lot below that).
    let _ = tracing_subscriber::registry().with(stderr).with(file_layer).with(tracing_subscriber::filter::LevelFilter::INFO).try_init();
    tracing::info!("s4rs {} ({}), log {}", env!("CARGO_PKG_VERSION"), std::env::consts::ARCH, file().map_or("none".into(), |p| p.display().to_string()));
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        tracing::error!("panic in thread '{thread}': {info}\n{bt}");
        if gui {
            let log = file().map_or("(no log file)".into(), |p| p.display().to_string());
            crate::gamedir::message_box("S4RS crashed", &format!("S4RS stopped with an error:\n\n{info}\n\nPlease send this log file with your bug report:\n{log}"));
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(1_792_800_000), "2026-10-24");
    }

    #[test]
    fn the_newest_ten_stay() {
        let d = std::env::temp_dir().join(format!("s4rs-logs-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for day in 1..=13 {
            std::fs::write(d.join(format!("s4rs-2026-10-{day:02}.log")), b"x").unwrap();
        }
        std::fs::write(d.join("other.txt"), b"x").unwrap();
        prune(&d, KEEP);
        let mut left: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left.len(), 11);
        assert_eq!(left[0], "other.txt");
        assert_eq!(left[1], "s4rs-2026-10-04.log");
        let _ = std::fs::remove_dir_all(&d);
    }
}
