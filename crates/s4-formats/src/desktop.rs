//! Off Windows: the desktop's dialogs for the game, the launcher and the Asset Studio, through
//! `zenity`, else `kdialog`, on macOS through `osascript`. Everything also goes to standard error,
//! so without them (or without a desktop) the player still reads it in the terminal.

use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

/// `prog args`'s output; `None` when the program is not installed.
fn run(prog: &str, args: &[&str]) -> Option<std::process::Output> {
    Command::new(prog).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()
}

/// `s` as an AppleScript string.
#[cfg(target_os = "macos")]
fn apple_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The dialog program's answer: the chosen path on standard output.
#[cfg(target_os = "macos")]
fn folder_dialog(title: &str) -> Option<std::process::Output> {
    run("osascript", &["-e", &format!("POSIX path of (choose folder with prompt {})", apple_string(title))])
}

#[cfg(not(target_os = "macos"))]
fn folder_dialog(title: &str) -> Option<std::process::Output> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let zenity_title = format!("--title={title}");
    run("zenity", &["--file-selection", "--directory", &zenity_title]).or_else(|| run("kdialog", &["--title", title, "--getexistingdirectory", &home]))
}

/// A folder the player chooses. `None`: cancelled, no dialog program or no desktop; standard error
/// then says the way around it (`S4_DIR`).
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    let answer = folder_dialog(title);
    let chosen = answer.as_ref().filter(|o| o.status.success()).and_then(|o| {
        let mut path = o.stdout.as_slice();
        while let [rest @ .., b'\n' | b'\r'] = path {
            path = rest;
        }
        (!path.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(path)))
    });
    if chosen.is_none() {
        let why = if answer.is_none() { "no folder dialog (osascript, zenity or kdialog)" } else { "no folder chosen" };
        eprintln!("{title}: {why}. Set S4_DIR to the game's folder (the one with S4_Main.exe).");
    }
    chosen
}

/// An error message: standard error, and a dialog when there is a dialog program.
#[cfg(target_os = "macos")]
pub fn message(title: &str, text: &str) {
    eprintln!("{title}: {text}");
    let script = format!("display dialog {} with title {} buttons {{\"OK\"}} default button 1 with icon stop", apple_string(text), apple_string(title));
    let _ = Command::new("osascript").args(["-e", &script]).stderr(Stdio::null()).status();
}

/// An error message: standard error, and a dialog when there is a dialog program.
#[cfg(not(target_os = "macos"))]
pub fn message(title: &str, text: &str) {
    eprintln!("{title}: {text}");
    let shown = Command::new("zenity").args(["--error", "--no-markup", "--width=480", &format!("--title={title}"), &format!("--text={text}")]).stderr(Stdio::null()).status().is_ok();
    if !shown {
        let _ = Command::new("kdialog").args(["--title", title, "--error", text]).stderr(Stdio::null()).status();
    }
}

/// A progress window (`zenity --progress`, with Cancel) next to a percentage on standard error.
pub struct Progress {
    zenity: Option<(Child, ChildStdin)>,
    shown: Option<u32>,
}

impl Progress {
    /// The window only on a desktop (`DISPLAY` / `WAYLAND_DISPLAY`) with zenity installed.
    pub fn start(title: &str, text: &str) -> Progress {
        eprintln!("{title}");
        let desktop = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
        let zenity = desktop
            .then(|| {
                Command::new("zenity")
                    .args(["--progress", "--auto-close", "--no-markup", "--width=520", &format!("--title={title}"), &format!("--text={text}")])
                    .stdin(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .ok()
            })
            .flatten()
            .and_then(|mut c| c.stdin.take().map(|i| (c, i)));
        Progress { zenity, shown: None }
    }

    /// `percent` (0..=100) and the line under it. `false`: the player pressed Cancel or closed the
    /// window (zenity's exit code 1); a window that failed otherwise is dropped, the work goes on.
    pub fn update(&mut self, percent: u32, line: &str) -> bool {
        let percent = percent.min(100);
        if self.shown != Some(percent) {
            eprint!("\r{percent:3} %");
            self.shown = Some(percent);
        }
        let Some((child, input)) = self.zenity.as_mut() else { return true };
        // 100 closes the window (--auto-close); the end is `finish`'s.
        let line: String = line.chars().filter(|c| !c.is_control()).collect();
        let sent = writeln!(input, "{}\n# {line}", percent.min(99)).and_then(|_| input.flush()).is_ok();
        let ended = match child.try_wait() {
            Ok(None) if sent => return true,
            Ok(None) => child.wait().ok(),
            Ok(Some(status)) => Some(status),
            Err(_) => None,
        };
        self.zenity = None;
        ended.and_then(|s| s.code()) != Some(1)
    }

    /// Done: the window closes.
    pub fn finish(mut self) {
        eprintln!();
        if let Some((mut child, mut input)) = self.zenity.take() {
            let _ = writeln!(input, "100");
            drop(input);
            let _ = child.wait();
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        if let Some((mut child, input)) = self.zenity.take() {
            drop(input);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
