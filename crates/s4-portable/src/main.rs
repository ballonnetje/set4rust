//! The portable build's launcher, shipped as `S4RS.exe` next to the unchanged `s4-game.exe`:
//!
//! ```text
//! S4RS-Portable\  S4RS.exe  s4-game.exe  S4RS-README.txt  data\ (the import)  user\ (settings, saves, logs)
//! ```
//!
//! On Linux the same as `S4RS` next to `s4-game` (tools/package/linux.sh), its dialogs and the
//! import's progress through zenity / kdialog and standard error (`s4_formats::desktop`).
//!
//! 1. `data\` holds a verified import ([`import::verify`]: the manifest's sizes and times; with
//!    `--verify` every SHA-256), else the import runs: the player's install (the s4-formats
//!    detection, else a folder picker), checked to be the History Edition, then only the files
//!    s4rs reads are copied ([`import`]), with a progress window that can cancel.
//!    `--reimport` forces it; `--import-only <install>` only imports (no window, no game).
//!    With `S4RS_NO_DETECT` (tests) only `S4_DIR` is searched and nothing is asked: exit 2.
//! 2. `s4-game.exe` runs with `S4_DIR=data` and `S4RS_HOME=user`; the other arguments go to it,
//!    its exit code is the launcher's.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod import;
mod sha256;
#[cfg(windows)]
mod ui;

use std::path::{Path, PathBuf};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reimport = args.iter().any(|a| a == "--reimport");
    let full = args.iter().any(|a| a == "--verify");
    let pass: Vec<&String> = args.iter().filter(|a| *a != "--reimport" && *a != "--verify").collect();
    let here = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."));
    let (data, user) = (here.join("data"), here.join("user"));
    if let Some(i) = args.iter().position(|a| a == "--import-only") {
        std::process::exit(import_only(args.get(i + 1).map(PathBuf::from), &data));
    }
    let ok = !reimport && import::verify(&data, full).is_ok();
    if !ok && !run_import(&data) {
        std::process::exit(2);
    }
    let _ = std::fs::create_dir_all(&user);
    let game = here.join(format!("s4-game{}", std::env::consts::EXE_SUFFIX));
    let mut cmd = std::process::Command::new(&game);
    cmd.args(pass).env("S4_DIR", &data).env("S4RS_HOME", &user).current_dir(&here);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window for the console-subsystem game; its log goes to user\logs.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.status() {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            message(&format!("{}: {e}", game.display()));
            std::process::exit(1);
        }
    }
}

/// `--import-only <install>`: the import from that folder into `data`, checked with every SHA-256,
/// no window and no game start (tools/package/linux.sh fills its private package with it). The
/// exit code: 0 imported, 1 failed, 2 no usable install given.
fn import_only(src: Option<PathBuf>, data: &Path) -> i32 {
    let Some(src) = src else {
        eprintln!("--import-only <the game's folder (the one with S4_Main.exe)>");
        return 2;
    };
    if let Some(why) = import::check_install(&src) {
        eprintln!("{why}");
        return 2;
    }
    let mut shown = None;
    let r = import::import(&src, data, &std::sync::atomic::AtomicBool::new(false), |p| {
        let percent = (p.done * 100).checked_div(p.total).unwrap_or(0);
        if shown != Some(percent) {
            eprint!("\rimporting {}: {percent:3} %", src.display());
            shown = Some(percent);
        }
    });
    eprintln!();
    match r.and_then(|_| import::verify(data, true)) {
        Ok(n) => {
            println!("{n} files imported into {}", data.display());
            0
        }
        Err(e) => {
            eprintln!("the import failed: {e}");
            1
        }
    }
}

/// Finds the install and imports it into `data` (true: done).
fn run_import(data: &Path) -> bool {
    // The detection without our own data folder (S4_DIR may point there from an earlier run).
    let found = s4_formats::install::candidates(None).into_iter().find(|d| !same(d, data) && import::check_install(d).is_none());
    let mut src = match found {
        Some(d) => d,
        // A test's start with no install: fail instead of asking.
        None if s4_formats::install::no_detect() => {
            eprintln!("no verified import in {} and no install (S4RS_NO_DETECT)", data.display());
            return false;
        }
        None => match pick() {
            Some(d) => d,
            None => return false,
        },
    };
    loop {
        match import::check_install(&src) {
            None => break,
            Some(why) => {
                message(&why);
                match pick() {
                    Some(d) => src = d,
                    None => return false,
                }
            }
        }
    }
    #[cfg(windows)]
    let r = ui::import_with_progress(&src, data);
    #[cfg(not(windows))]
    let r = import_with_progress(&src, data);
    match r {
        Ok(_) => true,
        Err(e) if e == "cancelled" => false,
        Err(e) => {
            message(&format!("The import of the game data failed:\n\n{e}"));
            false
        }
    }
}

fn same(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

#[cfg(not(windows))]
fn import_with_progress(src: &Path, data: &Path) -> Result<(usize, u64), String> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut window = s4_formats::desktop::Progress::start("S4RS: importing the game data", &format!("Copying the game data from {}", src.display()));
    let r = import::import(src, data, &cancel, |p| {
        let percent = (p.done * 100).checked_div(p.total).unwrap_or(0) as u32;
        if !window.update(percent, p.file) {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    window.finish();
    r
}

#[cfg(windows)]
const REQUIRED: &str = "S4RS needs The Settlers IV History Edition (your own copy, Ubisoft Connect). Its game data are copied once into the data folder next to S4RS.exe.\n\nChoose the game's folder (the one with S4_Main.exe).";
#[cfg(not(windows))]
const REQUIRED: &str = "S4RS needs The Settlers IV History Edition (your own copy, Ubisoft Connect; in a Wine, Proton or Lutris prefix, or a copy of its folder). Its game data are copied once into the data folder next to S4RS.\n\nChoose the game's folder (the one with S4_Main.exe).";

fn pick() -> Option<PathBuf> {
    #[cfg(windows)]
    return ui::pick_folder(REQUIRED);
    #[cfg(not(windows))]
    {
        eprintln!("{REQUIRED}");
        s4_formats::desktop::pick_folder("S4RS: choose The Settlers IV History Edition folder (the one with S4_Main.exe)")
    }
}

fn message(text: &str) {
    #[cfg(windows)]
    ui::message_box("S4RS", text);
    #[cfg(not(windows))]
    s4_formats::desktop::message("S4RS", text);
}
