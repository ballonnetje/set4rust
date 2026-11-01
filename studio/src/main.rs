//! The S4RS Asset Studio: a native desktop app to browse every sprite, sound, GUI image and music
//! slot of The Settlers IV, compare the original with your replacement and validate it.
//!
//! ```text
//! s4-asset-studio [--game DIR] [--data DIR]
//! s4-asset-studio --export-spec DIR [--game DIR]     write the asset spec and stop
//! ```
//! Started without arguments it finds the installed game the way the game itself does; with none
//! found the window opens on a first-run screen that asks for the folder and remembers the choice.
//! The first start writes the asset spec into its data folder, with progress in the window.

// No console window in release builds on Windows; `--help` and friends attach to the terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio;
mod editor;
mod export;
mod geometry;
mod images;
mod json;
mod layers;
mod model;
mod pack;
mod pngio;
mod settings;
#[cfg(test)]
mod shots;
mod spec;
mod status;
mod theme;
mod ui;
mod zip;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use s4_formats::{desktop, install};

const TITLE: &str = "S4RS Asset Studio";
const USAGE: &str = "usage:
  s4-asset-studio [--game DIR] [--data DIR]
  s4-asset-studio --export-spec DIR [--game DIR]

  --game DIR         the game's folder (the one with S4_Main.exe); default: found or asked for
  --data DIR         where the asset spec, your pack and the settings live
  --export-spec DIR  write the asset spec to DIR and stop
  --version          print the version
  --help             print this";

fn main() {
    attach_console();
    if let Err(e) = run() {
        desktop::message(TITLE, &format!("{e:#}"));
        std::process::exit(1);
    }
}

/// Windows release builds have no console of their own: output goes to where it was redirected,
/// else to the terminal they were started from, if any.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_console() {
    use std::ffi::c_void;
    use std::os::windows::io::IntoRawHandle;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetStdHandle(which: u32, handle: *mut c_void) -> i32;
    }
    let missing = |which: u32| {
        // SAFETY: no pointers go in; a handle is only compared.
        let h = unsafe { GetStdHandle(which) };
        h.is_null() || h as isize == -1
    };
    let (out, err) = (missing(STD_OUTPUT_HANDLE), missing(STD_ERROR_HANDLE));
    // SAFETY: no pointers; failing (no parent console) leaves everything as it is.
    if !(out || err) || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    for (needed, which) in [(out, STD_OUTPUT_HANDLE), (err, STD_ERROR_HANDLE)] {
        if needed && let Ok(f) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            // SAFETY: the handle comes from `into_raw_handle`, so it stays open for the process.
            unsafe { SetStdHandle(which, f.into_raw_handle()) };
        }
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_console() {}

/// Text for the terminal; a closed or missing stdout is no error.
fn say(text: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut game, mut data, mut export) = (None, None, None);
    while let Some(a) = args.next() {
        let mut val = || args.next().with_context(|| format!("{a} needs a value"));
        match a.as_str() {
            "--game" => game = Some(PathBuf::from(val()?)),
            "--data" => data = Some(PathBuf::from(val()?)),
            "--export-spec" => export = Some(PathBuf::from(val()?)),
            "--version" => {
                say(&format!("{TITLE} {}", env!("CARGO_PKG_VERSION")));
                return Ok(());
            }
            "--help" | "-h" => {
                say(USAGE);
                return Ok(());
            }
            _ => bail!("unknown argument {a}\n{USAGE}"),
        }
    }
    let data = data.unwrap_or_else(default_data_dir);
    std::fs::create_dir_all(&data).with_context(|| format!("the data folder {}", data.display()))?;
    let found = find_game(game, &data)?;
    if let Some(out) = export {
        let game = found.context("The Settlers IV History Edition was not found: give its folder with --game")?;
        return spec::run(&game, &out);
    }
    let size = app::window_size(&data);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(TITLE).with_app_id("s4rs-asset-studio").with_inner_size(size).with_min_inner_size([960.0, 600.0]).with_drag_and_drop(true).with_icon(window_icon()),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(TITLE, options, Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, data, found))))).map_err(|e| anyhow::anyhow!("{e}"))
}

/// A small tower in the accent colour.
fn window_icon() -> egui::IconData {
    const S: usize = 32;
    let mut rgba = vec![0u8; S * S * 4];
    for y in 0..S {
        for x in 0..S {
            let dx = x.abs_diff(S / 2) * 2;
            let tooth = y < 11 && (x / 4) % 2 == 0;
            let body = ((7..29).contains(&y) && dx < 20) || ((3..7).contains(&y) && dx < 26 && tooth) || ((7..11).contains(&y) && dx < 26);
            if body {
                rgba[(y * S + x) * 4..(y * S + x) * 4 + 4].copy_from_slice(&[0x6b, 0x9d, 0xff, 255]);
            }
        }
    }
    egui::IconData { rgba, width: S as u32, height: S as u32 }
}

/// The per-user folder for the asset spec, the pack and the chosen game folder.
fn default_data_dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("s4rs-asset-studio")
}

/// The game's folder: `--game`, else the game's own detection with the remembered choice. `None`:
/// not found, the window asks.
fn find_game(given: Option<PathBuf>, data: &Path) -> Result<Option<PathBuf>> {
    if let Some(dir) = given {
        let missing = install::missing(&dir);
        if !missing.is_empty() {
            bail!("{} is not a The Settlers IV History Edition folder; it lacks {}", dir.display(), missing.join(", "));
        }
        return Ok(Some(dir));
    }
    let remembered = std::fs::read_to_string(data.join("game_dir.txt")).ok().map(|s| PathBuf::from(s.trim())).filter(|d| !d.as_os_str().is_empty());
    install::set_configured(remembered);
    Ok(install::located())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_without_the_game_is_refused() {
        let dir = std::env::temp_dir();
        let e = find_game(Some(dir.clone()), &dir).unwrap_err().to_string();
        assert!(e.contains("S4_Main.exe"), "{e}");
    }

    #[test]
    fn the_icon_is_a_square_of_rgba() {
        let i = window_icon();
        assert_eq!(i.rgba.len() as u32, i.width * i.height * 4);
        assert!(i.rgba.as_chunks::<4>().0.iter().any(|c| c[3] == 255));
    }
}
