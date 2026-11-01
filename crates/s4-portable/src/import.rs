//! The import: the files s4rs reads, copied from the player's install into `data/`, and
//! `data/manifest.txt` (per file: SHA-256, size, modification time, path).
//!
//! What is copied is what the runtime reads (the installer's runtime audit): `S4_Main.exe`
//! (cursors and string names, `s4_formats::cursor` / `text`), `GameData`, `Gfx`, `Snd`, `Map`,
//! `Menu`, `Config`, `Txt`, `Script` (mission scripts, start resources), `Logo` (player logos)
//! and the manuals the GUI opens (`Handbook`, `Handbuch`: Config\MiscData1.cfg's URLs). Not the
//! videos, the editor, the launcher's DLLs or anything else.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use crate::sha256::Sha256;

pub const FILES: [&str; 1] = ["S4_Main.exe"];
pub const DIRS: [&str; 11] = ["GameData", "Gfx", "Snd", "Map", "Menu", "Config", "Txt", "Script", "Logo", "Handbook", "Handbuch"];
const MANIFEST: &str = "manifest.txt";
const HEADER: &str = "s4rs-data 1";

/// `S4_Main.exe`'s PE time stamp in the History Edition s4rs was made against (its tables and
/// strings are read at fixed places).
pub const HE_TIMESTAMP: u32 = 1_563_878_175;

/// The PE header's `TimeDateStamp` of an exe.
pub fn pe_timestamp(exe: &Path) -> Option<u32> {
    let mut f = std::fs::File::open(exe).ok()?;
    let mut head = vec![0u8; 0x400];
    let n = f.read(&mut head).ok()?;
    head.truncate(n);
    if head.get(..2)? != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(head.get(0x3c..0x40)?.try_into().ok()?) as usize;
    if head.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    Some(u32::from_le_bytes(head.get(pe + 8..pe + 12)?.try_into().ok()?))
}

/// Why `dir` is not a usable History Edition install (`None`: it is).
pub fn check_install(dir: &Path) -> Option<String> {
    let missing = s4_formats::install::missing(dir);
    if !missing.is_empty() {
        return Some(format!("{} is not a The Settlers IV folder: it lacks {}.", dir.display(), missing.join(", ")));
    }
    let exe = s4_formats::path::resolve(dir.join("S4_Main.exe"));
    match pe_timestamp(&exe) {
        Some(HE_TIMESTAMP) => None,
        Some(t) => Some(format!("{} is another build of the game (time stamp {t}); S4RS needs The Settlers IV History Edition (Ubisoft Connect, build {HE_TIMESTAMP}).", exe.display())),
        None => Some(format!("{} is no Windows program.", exe.display())),
    }
}

/// The files to copy, relative to the install, in a stable order (forward slashes), named as on
/// disk (found ignoring case off Windows, `s4_formats::path`).
pub fn file_list(src: &Path) -> Vec<String> {
    let rel = |p: &Path| p.strip_prefix(src).ok().map(|r| r.to_string_lossy().replace('\\', "/"));
    let mut out: Vec<String> = FILES.iter().map(|f| s4_formats::path::resolve(src.join(f))).filter(|p| p.is_file()).filter_map(|p| rel(&p)).collect();
    for d in DIRS {
        let mut stack = vec![s4_formats::path::resolve(src.join(d))];
        let mut found = Vec::new();
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(r) = rel(&p) {
                    found.push(r);
                }
            }
        }
        found.sort();
        out.extend(found);
    }
    out
}

/// One manifest line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub sha256: String,
    pub size: u64,
    pub mtime: u64,
    pub path: String,
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// The manifest of `data` (`None`: none or unreadable).
pub fn read_manifest(data: &Path) -> Option<(PathBuf, Vec<Entry>)> {
    let text = std::fs::read_to_string(data.join(MANIFEST)).ok()?;
    let mut lines = text.lines();
    if lines.next()? != HEADER {
        return None;
    }
    let source = PathBuf::from(lines.next()?.strip_prefix("source ")?);
    let mut out = Vec::new();
    for l in lines {
        let mut p = l.splitn(4, ' ');
        let (sha256, size, mtime, path) = (p.next()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?);
        out.push(Entry { sha256: sha256.into(), size, mtime, path: path.into() });
    }
    Some((source, out))
}

fn write_manifest(data: &Path, source: &Path, entries: &[Entry]) -> std::io::Result<()> {
    let mut s = format!("{HEADER}\nsource {}\n", source.display());
    for e in entries {
        s.push_str(&format!("{} {} {} {}\n", e.sha256, e.size, e.mtime, e.path));
    }
    let tmp = data.join("manifest.tmp");
    std::fs::write(&tmp, s)?;
    std::fs::rename(tmp, data.join(MANIFEST))
}

/// Whether `data` holds a complete import: every needed file listed, and every listed file
/// there with its size and time (`full`: and its SHA-256). The reason when not.
///
/// A file of the right size whose time differs is hashed: with the manifest's SHA-256 it is the
/// same file and counts as unchanged. Unpacking a zip moves the times (the zip keeps local time
/// in 2 s steps, so time zone and DST shift them) and an installer may set its own. The new times
/// are then written to the manifest, so only the first start after such a copy hashes.
pub fn verify(data: &Path, full: bool) -> Result<usize, String> {
    let (source, mut entries) = read_manifest(data).ok_or("no manifest")?;
    for f in FILES {
        if !entries.iter().any(|e| e.path.eq_ignore_ascii_case(f)) {
            return Err(format!("{f} not imported"));
        }
    }
    for d in DIRS.iter().filter(|d| ["GameData", "Gfx", "Snd", "Map", "Menu", "Config", "Txt"].contains(d)) {
        if !entries.iter().any(|e| e.path.get(..d.len() + 1).is_some_and(|p| p.eq_ignore_ascii_case(&format!("{d}/")))) {
            return Err(format!("{d} not imported"));
        }
    }
    let mut moved = 0;
    for e in &mut entries {
        let p = data.join(&e.path);
        let Ok(m) = std::fs::metadata(&p) else { return Err(format!("{} missing", e.path)) };
        if m.len() != e.size {
            return Err(format!("{} changed", e.path));
        }
        let t = mtime(&p);
        if (full || t != e.mtime) && hash_file(&p).map_err(|err| format!("{}: {err}", e.path))? != e.sha256 {
            return Err(format!("{} {}", e.path, if t != e.mtime { "changed" } else { "damaged (SHA-256)" }));
        }
        if t != e.mtime {
            e.mtime = t;
            moved += 1;
        }
    }
    if moved > 0 {
        // Not fatal: the next start hashes those files again.
        match write_manifest(data, &source, &entries) {
            Ok(()) => eprintln!("{moved} file times differed from the manifest, contents checked (SHA-256): manifest updated"),
            Err(err) => eprintln!("{moved} file times differed from the manifest, contents checked (SHA-256); manifest not updated: {err}"),
        }
    }
    Ok(entries.len())
}

fn hash_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::default();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            return Ok(h.hex());
        }
        h.update(&buf[..n]);
    }
}

/// Progress of [`import`]: bytes done and in all, the file being copied.
pub struct Progress<'a> {
    pub done: u64,
    pub total: u64,
    pub file: &'a str,
}

/// Copies [`file_list`] of `src` into `data` (emptied first), hashing on the way, then writes
/// the manifest. `cancel` stops it (`Err("cancelled")`, `data` removed again).
pub fn import(src: &Path, data: &Path, cancel: &AtomicBool, mut progress: impl FnMut(Progress)) -> Result<(usize, u64), String> {
    let files = file_list(src);
    let sizes: Vec<u64> = files.iter().map(|f| std::fs::metadata(src.join(f)).map_or(0, |m| m.len())).collect();
    let total: u64 = sizes.iter().sum();
    let _ = std::fs::remove_dir_all(data);
    std::fs::create_dir_all(data).map_err(|e| format!("{}: {e}", data.display()))?;
    let mut entries = Vec::with_capacity(files.len());
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    let fail = |e: String| {
        let _ = std::fs::remove_dir_all(data);
        e
    };
    for f in &files {
        if cancel.load(Ordering::Relaxed) {
            return Err(fail("cancelled".into()));
        }
        progress(Progress { done, total, file: f });
        let (from, to) = (src.join(f), data.join(f));
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir).map_err(|e| fail(format!("{}: {e}", dir.display())))?;
        }
        let mut r = std::fs::File::open(&from).map_err(|e| fail(format!("{}: {e}", from.display())))?;
        let mut w = std::fs::File::create(&to).map_err(|e| fail(format!("{}: {e}", to.display())))?;
        let mut h = Sha256::default();
        let mut size = 0u64;
        loop {
            let n = r.read(&mut buf).map_err(|e| fail(format!("{}: {e}", from.display())))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            w.write_all(&buf[..n]).map_err(|e| fail(format!("{}: {e}", to.display())))?;
            size += n as u64;
            done += n as u64;
            if cancel.load(Ordering::Relaxed) {
                drop(w);
                return Err(fail("cancelled".into()));
            }
        }
        drop(w);
        entries.push(Entry { sha256: h.hex(), size, mtime: mtime(&to), path: f.clone() });
    }
    write_manifest(data, src, &entries).map_err(|e| fail(format!("manifest: {e}")))?;
    progress(Progress { done, total, file: "" });
    Ok((entries.len(), done))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_install(dir: &Path) {
        let mut exe = vec![0u8; 0x200];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        exe[0x80..0x84].copy_from_slice(b"PE\0\0");
        exe[0x88..0x8c].copy_from_slice(&HE_TIMESTAMP.to_le_bytes());
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("S4_Main.exe"), exe).unwrap();
        for f in ["GameData/buildingInfo.xml", "GameData/jobInfo.xml", "Gfx/0.gfx", "Snd/0.snd", "Map/Campaign/a.map", "Menu/m.dat", "Config/c.cfg", "Txt/s4_texts.dat0", "Video/intro.bik", "Editor/e.exe"] {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, f.as_bytes()).unwrap();
        }
    }

    #[test]
    fn import_copies_what_is_read_and_verifies() {
        let root = std::env::temp_dir().join(format!("s4rs-portable-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, data) = (root.join("install"), root.join("data"));
        fake_install(&src);
        assert_eq!(check_install(&src), None);
        let list = file_list(&src);
        assert!(list.contains(&"S4_Main.exe".to_string()) && list.contains(&"Map/Campaign/a.map".to_string()));
        assert!(!list.iter().any(|f| f.starts_with("Video/") || f.starts_with("Editor/")), "{list:?}");
        let mut calls = 0;
        let (n, _) = import(&src, &data, &AtomicBool::new(false), |_| calls += 1).unwrap();
        assert_eq!(n, list.len());
        assert!(calls > n);
        assert_eq!(verify(&data, true), Ok(n));
        assert!(s4_formats::install::is_game_dir(&data));
        // A changed file fails the quick check, a damaged one of the same size the full one.
        std::fs::write(data.join("Gfx/0.gfx"), b"changed, longer").unwrap();
        assert!(verify(&data, false).is_err());
        import(&src, &data, &AtomicBool::new(false), |_| {}).unwrap();
        let (_, entries) = read_manifest(&data).unwrap();
        let e = entries.iter().find(|e| e.path == "Gfx/0.gfx").unwrap();
        let p = data.join("Gfx/0.gfx");
        std::fs::write(&p, b"Gfx/0.gfX").unwrap();
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(e.mtime);
        std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
        assert_eq!(verify(&data, false), Ok(entries.len()));
        assert!(verify(&data, true).is_err());
        // Cancelled: nothing left.
        assert_eq!(import(&src, &data, &AtomicBool::new(true), |_| {}), Err("cancelled".into()));
        assert!(!data.exists());
        // Another build of the exe is refused.
        let mut exe = std::fs::read(src.join("S4_Main.exe")).unwrap();
        exe[0x88] ^= 1;
        std::fs::write(src.join("S4_Main.exe"), exe).unwrap();
        assert!(check_install(&src).is_some_and(|m| m.contains("another build")));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn shift_times(data: &Path, entries: &[Entry], secs: i64) {
        for e in entries {
            let t = UNIX_EPOCH + std::time::Duration::from_secs(e.mtime.checked_add_signed(secs).unwrap());
            std::fs::File::options().write(true).open(data.join(&e.path)).unwrap().set_modified(t).unwrap();
        }
    }

    #[test]
    fn moved_times_with_the_same_contents_heal_the_manifest() {
        let root = std::env::temp_dir().join(format!("s4rs-portable-heal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, data) = (root.join("install"), root.join("data"));
        fake_install(&src);
        let (n, _) = import(&src, &data, &AtomicBool::new(false), |_| {}).unwrap();
        for secs in [3600, 1, -3600] {
            let (source, before) = read_manifest(&data).unwrap();
            shift_times(&data, &before, secs);
            // Accepted without an import (the files are the imported ones), the times taken over.
            assert_eq!(verify(&data, false), Ok(n), "shift {secs} s");
            let (source2, after) = read_manifest(&data).unwrap();
            assert_eq!(source2, source);
            assert_eq!(after.len(), before.len());
            for (a, b) in after.iter().zip(&before) {
                assert_eq!((&a.sha256, a.size, &a.path), (&b.sha256, b.size, &b.path));
                assert_eq!(a.mtime, b.mtime.checked_add_signed(secs).unwrap(), "{}", a.path);
            }
            // Healed once: the manifest is not written again.
            let text = std::fs::read(data.join(MANIFEST)).unwrap();
            let written = std::fs::metadata(data.join(MANIFEST)).unwrap().modified().unwrap();
            assert_eq!(verify(&data, false), Ok(n));
            assert_eq!(std::fs::read(data.join(MANIFEST)).unwrap(), text);
            assert_eq!(std::fs::metadata(data.join(MANIFEST)).unwrap().modified().unwrap(), written);
            assert!(!data.join("manifest.tmp").exists());
        }
        // One changed byte, same size, moved time: rejected, the manifest left as it was.
        let (_, entries) = read_manifest(&data).unwrap();
        let text = std::fs::read(data.join(MANIFEST)).unwrap();
        let p = data.join("Gfx/0.gfx");
        let mut bytes = std::fs::read(&p).unwrap();
        bytes[0] ^= 0x20;
        std::fs::write(&p, &bytes).unwrap();
        shift_times(&data, &entries, 3600);
        assert_eq!(verify(&data, false), Err("Gfx/0.gfx changed".into()));
        assert_eq!(std::fs::read(data.join(MANIFEST)).unwrap(), text);
        let _ = std::fs::remove_dir_all(&root);
    }
}
