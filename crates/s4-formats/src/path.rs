//! Paths into the installed game as Windows would open them.
//!
//! The game's own files name each other the Windows way (`Script\roman01.txt`, `snd/TROJANs_settle_01.mp3`,
//! `Menu/GUISetRomans.dat` for `GUISetRomans.DAT`): a backslash separates, and case does not matter. On
//! Windows a path is opened as given, so [`resolve`] changes nothing there. Elsewhere it turns `\` into
//! `/` and matches every component case-insensitively against the directory's entries (an exact name
//! first); listings are cached per directory and read again when a name is not in them.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// `p` the way Windows would find it: unchanged on Windows; elsewhere the existing path whose
/// components equal `p`'s ignoring case (`\` taken as a separator), `p` itself when it exists. Where no
/// entry matches, the rest is kept as given (the open then fails with that path).
pub fn resolve(p: impl AsRef<Path>) -> PathBuf {
    let p = p.as_ref();
    if cfg!(windows) {
        return p.to_path_buf();
    }
    let s = p.to_string_lossy();
    let normal = if s.contains('\\') { PathBuf::from(s.replace('\\', "/")) } else { p.to_path_buf() };
    if normal.exists() {
        return normal;
    }
    match_case(&normal)
}

/// `rel` (Windows separators allowed) under the installed game ([`crate::game_dir`]), [`resolve`]d.
pub fn data(rel: &str) -> PathBuf {
    resolve(crate::game_dir().join(rel))
}

/// The component walk of [`resolve`] on every platform (the tests run it on Windows too): each
/// normal component is the listed entry of that name, else the first one equal ignoring case, else
/// kept as given (and so is the rest after one that does not exist).
pub fn match_case(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    let p = if s.contains('\\') && !cfg!(windows) { PathBuf::from(s.replace('\\', "/")) } else { p.to_path_buf() };
    let mut out = PathBuf::new();
    let mut lost = false;
    for c in p.components() {
        match c {
            Component::Normal(name) if !lost => match entry(&out, name) {
                Some(found) => out.push(found),
                // Not listed but there (a short 8.3 name, a directory that cannot be listed).
                None if out.join(name).exists() => out.push(name),
                None => {
                    lost = true;
                    out.push(name);
                }
            },
            _ => out.push(c.as_os_str()),
        }
    }
    out
}

/// Entries per directory: lower-cased name → names on disk (sorted, so ties pick the same one).
type Listing = HashMap<String, Vec<OsString>>;
static LISTINGS: Mutex<Option<HashMap<PathBuf, Listing>>> = Mutex::new(None);

/// The entry of `dir` named `name`: the exact name, else the first equal ignoring case.
fn entry(dir: &Path, name: &std::ffi::OsStr) -> Option<OsString> {
    // Where case counts, an existing exact name is the entry, whatever a listing cached before it
    // was created holds (on Windows any case would exist: the listing gives the name on disk).
    let listed = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    if !cfg!(windows) && std::fs::symlink_metadata(listed.join(name)).is_ok() {
        return Some(name.to_os_string());
    }
    let key = name.to_string_lossy().to_lowercase();
    let pick = |l: &Listing| l.get(&key).map(|v| v.iter().find(|n| n.as_os_str() == name).unwrap_or(&v[0]).clone());
    let mut cache = LISTINGS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(found) = cache.get(dir).and_then(pick) {
        return Some(found);
    }
    // Not cached, or created since: list again.
    let mut l = Listing::new();
    for e in std::fs::read_dir(listed).ok()?.flatten() {
        let n = e.file_name();
        l.entry(n.to_string_lossy().to_lowercase()).or_default().push(n);
    }
    l.values_mut().for_each(|v| v.sort());
    let found = pick(&l);
    cache.insert(dir.to_path_buf(), l);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_match_ignoring_case_and_backslashes() {
        let root = std::env::temp_dir().join(format!("s4rs-path-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("script").join("missioncd")).unwrap();
        std::fs::create_dir_all(root.join("menu")).unwrap();
        std::fs::write(root.join("script").join("roman01.txt"), b"x").unwrap();
        std::fs::write(root.join("script").join("missioncd").join("m.txt"), b"y").unwrap();
        std::fs::write(root.join("menu").join("guisetromans.dat"), b"z").unwrap();
        let base = match_case(&root);
        let rel = |p: &Path| p.strip_prefix(&base).unwrap().to_string_lossy().replace('\\', "/");

        assert_eq!(rel(&match_case(&root.join(r"Script\roman01.txt"))), "script/roman01.txt");
        assert_eq!(rel(&match_case(&root.join(r"SCRIPT\MissionCD\M.TXT"))), "script/missioncd/m.txt");
        assert_eq!(rel(&match_case(&root.join("Menu").join("GUISetRomans.DAT"))), "menu/guisetromans.dat");
        assert_eq!(std::fs::read(match_case(&root.join(r"Script\MissionCD\m.txt"))).unwrap(), b"y");
        // Missing: matched as far as it goes, the rest as given.
        assert_eq!(rel(&match_case(&root.join(r"Script\Nope\Roman01.txt"))), "script/Nope/Roman01.txt");
        // A file created after the directory was listed is found.
        std::fs::write(root.join("script").join("roman02.txt"), b"w").unwrap();
        assert_eq!(rel(&match_case(&root.join("Script").join("ROMAN02.txt"))), "script/roman02.txt");
        // A file named exactly as asked is the one opened, also when it was created after the
        // directory was listed next to one equal ignoring case: on a case-sensitive file system a
        // second file (the exact name wins over the cached one), on Windows the same file
        // rewritten.
        std::fs::write(root.join("menu").join("GUISetRomans.dat"), b"q").unwrap();
        assert_eq!(std::fs::read(match_case(&root.join("menu").join("GUISetRomans.dat"))).unwrap(), b"q");
        if std::fs::read_dir(root.join("menu")).unwrap().count() == 2 {
            assert_eq!(rel(&match_case(&root.join("menu").join("GUISetRomans.dat"))), "menu/GUISetRomans.dat");
            assert_eq!(std::fs::read(match_case(&root.join("MENU").join("guisetromans.dat"))).unwrap(), b"z");
        }
        // resolve: unchanged on Windows; elsewhere the same walk.
        let r = resolve(root.join(r"Script\roman01.txt"));
        if cfg!(windows) {
            assert_eq!(r, root.join(r"Script\roman01.txt"));
        } else {
            assert_eq!(rel(&r), "script/roman01.txt");
        }
        assert_eq!(std::fs::read(r).unwrap(), b"x");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
