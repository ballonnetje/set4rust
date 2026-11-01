//! Where the installed game (The Settlers IV History Edition) is: s4rs loads its data files
//! from there and ships none of them.
//!
//! Detection order, first valid one wins, cached for the run ([`game_dir`]):
//! 1. `S4_DIR`;
//! 2. the directory the application configured ([`set_configured`], s4-game's `settings.toml`);
//! 3. Ubisoft Connect's `HKLM\SOFTWARE\[WOW6432Node\]Ubisoft\Launcher\Installs\*\InstallDir`,
//!    every entry checked by its content (no guessed ids), then GOG's `GOG.com\Games\*\path`;
//! 4. the usual folders: `Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\*`;
//! 5. off Windows, the same games folder inside Wine prefixes: `WINEPREFIX`, `~/.wine`, Lutris'
//!    `~/Games/*`, Bottles, Steam's Proton prefixes (`steamapps/compatdata/*/pfx`);
//! 6. a development checkout's `thesettlers4` next to the repository.
//!
//! A directory counts when it holds what the loaders read ([`missing`]).
//!
//! `S4RS_NO_DETECT` (set, not empty) stops after 2, for tests of a start with no install
//! (tools/package/private-windows.ps1).

use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// What the loaders read from the install: the exe (tables, texts, cursors), the game data
/// XMLs (rules), and the graphics, sound, map, menu, config and text folders.
const NEEDED_FILES: [&str; 3] = ["S4_Main.exe", "GameData/buildingInfo.xml", "GameData/jobInfo.xml"];
const NEEDED_DIRS: [&str; 6] = ["Gfx", "Snd", "Map", "Menu", "Config", "Txt"];

/// What `dir` lacks of [`NEEDED_FILES`] / [`NEEDED_DIRS`] (empty: a usable install).
pub fn missing(dir: &Path) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = NEEDED_FILES.iter().copied().filter(|f| !crate::path::resolve(dir.join(f)).is_file()).collect();
    out.extend(NEEDED_DIRS.iter().copied().filter(|d| !crate::path::resolve(dir.join(d)).is_dir()));
    out
}

pub fn is_game_dir(dir: &Path) -> bool {
    missing(dir).is_empty()
}

static CONFIGURED: RwLock<Option<PathBuf>> = RwLock::new(None);
static FOUND: RwLock<Option<Option<PathBuf>>> = RwLock::new(None);

/// The application's own choice (settings), tried after `S4_DIR`. Call it before the first
/// [`game_dir`]; a later call starts the detection again.
pub fn set_configured(dir: Option<PathBuf>) {
    *CONFIGURED.write().unwrap() = dir;
    *FOUND.write().unwrap() = None;
}

/// The detected install, `None` when there is none (detected once, then cached).
pub fn located() -> Option<PathBuf> {
    if let Some(found) = FOUND.read().unwrap().as_ref() {
        return found.clone();
    }
    let configured = CONFIGURED.read().unwrap().clone();
    let found = candidates(configured).into_iter().find(|d| is_game_dir(d));
    *FOUND.write().unwrap() = Some(found.clone());
    found
}

/// The candidates in detection order (not yet checked).
pub fn candidates(configured: Option<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    out.extend(std::env::var_os("S4_DIR").map(PathBuf::from));
    out.extend(configured);
    if no_detect() {
        return out;
    }
    out.extend(registry());
    if let Some(pf) = std::env::var_os("ProgramFiles(x86)").or_else(|| std::env::var_os("ProgramFiles")) {
        let games = PathBuf::from(pf).join("Ubisoft").join("Ubisoft Game Launcher").join("games");
        out.push(games.join("thesettlers4"));
        if let Ok(rd) = std::fs::read_dir(&games) {
            let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            dirs.sort();
            out.extend(dirs);
        }
    }
    #[cfg(not(windows))]
    out.extend(wine_prefixes());
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("..").join("thesettlers4"));
    out
}

/// `S4RS_NO_DETECT` is set: only `S4_DIR` and the configured directory are candidates.
pub fn no_detect() -> bool {
    std::env::var_os("S4RS_NO_DETECT").is_some_and(|v| !v.is_empty())
}

/// The subdirectories of `dir`, sorted.
#[cfg(not(windows))]
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

/// Off Windows, the game installed through Wine: in each prefix (`WINEPREFIX`, `~/.wine`, Lutris'
/// `~/Games/*`, Bottles' bottles, CrossOver's bottles on macOS, Steam's Proton
/// `compatdata/*/pfx`, native and Flatpak Steam) the Ubisoft games folder as on Windows:
/// `thesettlers4`, then every other game folder.
#[cfg(not(windows))]
fn wine_prefixes() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut prefixes: Vec<PathBuf> = std::env::var_os("WINEPREFIX").map(PathBuf::from).into_iter().collect();
    prefixes.push(home.join(".wine"));
    prefixes.extend(subdirs(&home.join("Games")));
    prefixes.extend(subdirs(&home.join(".local/share/bottles/bottles")));
    prefixes.extend(subdirs(&home.join("Library/Application Support/CrossOver/Bottles")));
    for steam in [".local/share/Steam", ".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
        prefixes.extend(subdirs(&home.join(steam).join("steamapps/compatdata")).into_iter().map(|d| d.join("pfx")));
    }
    let mut out = Vec::new();
    for p in prefixes {
        for pf in ["Program Files (x86)", "Program Files"] {
            let games = p.join("drive_c").join(pf).join("Ubisoft/Ubisoft Game Launcher/games");
            if games.is_dir() {
                out.push(games.join("thesettlers4"));
                out.extend(subdirs(&games));
            }
        }
    }
    out
}

/// `InstallDir` / `path` values under the Ubisoft Connect and GOG install keys (`reg query`,
/// so no registry crate; nothing off Windows).
fn registry() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (key, value) in [
        (r"HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs", "InstallDir"),
        (r"HKLM\SOFTWARE\Ubisoft\Launcher\Installs", "InstallDir"),
        (r"HKLM\SOFTWARE\WOW6432Node\GOG.com\Games", "path"),
    ] {
        out.extend(reg_values(key, value).into_iter().map(|v| PathBuf::from(v.trim_end_matches(['/', '\\']))));
    }
    out
}

#[cfg(windows)]
fn reg_values(key: &str, value: &str) -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(o) = std::process::Command::new("reg").args(["query", key, "/s", "/v", value]).creation_flags(CREATE_NO_WINDOW).output() else {
        return Vec::new();
    };
    parse_reg(&String::from_utf8_lossy(&o.stdout), value)
}

#[cfg(not(windows))]
fn reg_values(_key: &str, _value: &str) -> Vec<String> {
    Vec::new()
}

/// The data of `value` lines in `reg query` output (`    name    REG_SZ    data`).
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_reg(out: &str, value: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let rest = l.strip_prefix(value)?.trim_start();
            let rest = rest.strip_prefix("REG_SZ").or_else(|| rest.strip_prefix("REG_EXPAND_SZ"))?;
            Some(rest.trim().to_string()).filter(|s| !s.is_empty())
        })
        .collect()
}

/// Root of the installed game, e.g. `C:\...\thesettlers4` ([`located`]). With none found, a
/// path that does not exist, so loads fail (the application reports it at start).
pub fn game_dir() -> PathBuf {
    located().unwrap_or_else(|| PathBuf::from("The Settlers IV History Edition not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_install(dir: &Path) {
        for f in NEEDED_FILES {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        for d in NEEDED_DIRS {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
    }

    #[test]
    fn validation_needs_every_file_and_folder() {
        let root = std::env::temp_dir().join(format!("s4rs-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(missing(&root).len(), NEEDED_FILES.len() + NEEDED_DIRS.len());
        fake_install(&root);
        assert!(is_game_dir(&root), "{:?}", missing(&root));
        std::fs::remove_dir_all(root.join("Snd")).unwrap();
        assert_eq!(missing(&root), vec!["Snd"]);
        std::fs::create_dir_all(root.join("Snd")).unwrap();
        std::fs::remove_file(root.join("S4_Main.exe")).unwrap();
        assert_eq!(missing(&root), vec!["S4_Main.exe"]);
        // A folder named like the exe is no exe.
        std::fs::create_dir_all(root.join("S4_Main.exe")).unwrap();
        assert!(!is_game_dir(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reg_query_output_is_parsed() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs\\11785\r\n    InstallDir    REG_SZ    C:/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/games/thesettlers4/\r\n\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs\\420\r\n    InstallDir    REG_SZ    D:/Games/Far Cry 4/\r\n\r\nEnd of search: 2 match(es) found.\r\n";
        assert_eq!(parse_reg(out, "InstallDir"), vec!["C:/Program Files (x86)/Ubisoft/Ubisoft Game Launcher/games/thesettlers4/", "D:/Games/Far Cry 4/"]);
        assert!(parse_reg(out, "path").is_empty());
    }

    #[test]
    fn s4_dir_and_the_configured_dir_come_first() {
        let c = candidates(Some(PathBuf::from("configured")));
        let at = c.iter().position(|p| p == Path::new("configured")).unwrap();
        match std::env::var_os("S4_DIR") {
            Some(d) => assert_eq!((c[0].as_os_str(), at), (d.as_os_str(), 1)),
            None => assert_eq!(at, 0),
        }
    }
}
