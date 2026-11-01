//! Collects the original game's single-player autosaves into series for the AI oracles.
//! The release overwrites one file per game (`AutoSave.sav` / `<name>_autoSave.sav`, strings at
//! 0xc18dc8) every `AutoSaveInterval` minutes (GameSettings.cfg; the check 0x5158c0 compares
//! `(tick - last) / 14` with `interval * 60`, at least 60). This watches the Save folder (not its
//! subfolders), waits until a changed autosave is complete, reads its tick (chunk 130), and copies
//! it to `<out>/<series>/<series>_t<tick>.sav`. A new series starts when the tick goes back.
//! `cargo run --release -p s4-sim --example ai_save_collect -- [--out DIR] [--series NAME] [--save DIR]`
//! Default out: `Documents\TheSettlers4\AiSaves` — outside Save, so the save oracles that walk
//! Save do not pick the series up until someone moves them there on purpose.
use s4_sim::save::SaveGame;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn is_autosave(p: &Path) -> bool {
    let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    n.ends_with(".sav") && (n == "autosave.sav" || n.ends_with("_autosave.sav"))
}

fn stamp(p: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.modified().ok()?, m.len()))
}

fn main() {
    let home = PathBuf::from(std::env::var("USERPROFILE").expect("USERPROFILE"));
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let save_dir = arg("--save").map(PathBuf::from).unwrap_or_else(|| home.join(r"Documents\TheSettlers4\Save"));
    let out = arg("--out").map(PathBuf::from).unwrap_or_else(|| home.join(r"Documents\TheSettlers4\AiSaves"));
    let base = arg("--series").unwrap_or_else(|| "ai".into());
    // Existing autosaves are old games: remember them so only new writes are collected.
    let mut seen: std::collections::HashMap<PathBuf, (SystemTime, u64)> = std::fs::read_dir(&save_dir)
        .expect("Save folder")
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_autosave(p))
        .filter_map(|p| stamp(&p).map(|s| (p, s)))
        .collect();
    let next_series = |out: &Path| {
        (1..).map(|i| format!("{base}{i}")).find(|s| !out.join(s).exists()).unwrap()
    };
    let mut series: Option<(String, u32)> = None;
    println!("watching {} -> {}", save_dir.display(), out.display());
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let Ok(dir) = std::fs::read_dir(&save_dir) else { continue };
        for p in dir.flatten().map(|e| e.path()).filter(|p| is_autosave(p)) {
            let Some(s1) = stamp(&p) else { continue };
            if seen.get(&p) == Some(&s1) {
                continue;
            }
            // Complete when size and time stay the same for 3 s and the file parses.
            std::thread::sleep(Duration::from_secs(3));
            if stamp(&p) != Some(s1) {
                continue;
            }
            let save = match SaveGame::open(&p) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: not readable yet ({e})", p.display());
                    continue;
                }
            };
            seen.insert(p.clone(), s1);
            let new = match &series {
                Some((_, last)) => save.tick <= *last,
                None => true,
            };
            if new {
                let name = next_series(&out);
                std::fs::create_dir_all(out.join(&name)).expect("create series folder");
                println!("series {name} (from {})", p.display());
                series = Some((name, 0));
            }
            let (name, last) = series.as_mut().unwrap();
            *last = save.tick;
            let dst = out.join(&*name).join(format!("{name}_t{:07}.sav", save.tick));
            std::fs::copy(&p, &dst).expect("copy");
            let mins = save.tick as f64 / 840.0;
            let su = if save.settlers_united { "  SETTLERS UNITED (not vanilla)" } else { "" };
            println!("  tick {:>7} (~{mins:.1} min) -> {}{su}", save.tick, dst.display());
        }
    }
}
