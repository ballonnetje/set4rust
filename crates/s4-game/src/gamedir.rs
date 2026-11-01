//! The game install s4rs reads its data from (The Settlers IV History Edition, the player's
//! own copy): detection at start ([`s4_formats::install`]), else a folder picker on the first
//! start, the choice kept in `settings.toml`; without one, a message box and the end.

use std::path::PathBuf;

/// The install for this run: `S4_DIR`, the settings' folder, the registry, the usual folders;
/// with none and a window (`interactive`), the player picks the folder (kept in the settings).
/// `None`: nothing usable, the player was told.
pub fn resolve(interactive: bool) -> Option<PathBuf> {
    let mut settings = crate::settings::Settings::load();
    s4_formats::install::set_configured(settings.game_dir.clone());
    if let Some(d) = s4_formats::install::located() {
        tracing::info!("game data: {}", d.display());
        return Some(d);
    }
    if !interactive {
        eprintln!("{REQUIRED}\nSet S4_DIR to the game's folder (the one with S4_Main.exe).");
        return None;
    }
    loop {
        let Some(dir) = pick_folder() else {
            message_box("S4RS", REQUIRED);
            return None;
        };
        let missing = s4_formats::install::missing(&dir);
        if missing.is_empty() {
            settings.game_dir = Some(dir.clone());
            settings.save();
            s4_formats::install::set_configured(Some(dir.clone()));
            tracing::info!("game data: {} (chosen)", dir.display());
            return Some(dir);
        }
        message_box("S4RS", &format!("{}\n\nis not a The Settlers IV History Edition folder; it lacks {}.\n\nChoose the folder that holds S4_Main.exe.", dir.display(), missing.join(", ")));
    }
}

#[cfg(windows)]
const REQUIRED: &str = "The Settlers IV History Edition is required.\n\nS4RS uses the game data of your own copy (Ubisoft Connect). Install the game, or choose its folder (the one with S4_Main.exe) when asked at the start.";
#[cfg(not(windows))]
const REQUIRED: &str = "The Settlers IV History Edition is required.\n\nS4RS uses the game data of your own copy (Ubisoft Connect): its folder in a Wine, Proton or Lutris prefix, or a copy of that folder. Choose it (the one with S4_Main.exe) when asked at the start, or set S4_DIR to it.";

/// A message box (Windows), else standard error and the desktop's dialog (zenity / kdialog).
pub fn message_box(title: &str, text: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (t, m) = (w(title), w(text));
        // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
        unsafe { MessageBoxW(0, m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR) };
    }
    #[cfg(not(windows))]
    s4_formats::desktop::message(title, text);
}

/// The shell's folder dialog (Windows); `None` when cancelled. (Declared here: windows-sys
/// 0.52 gates `SHBrowseForFolderW` behind a feature it does not offer.)
#[cfg(windows)]
fn pick_folder() -> Option<PathBuf> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStringExt;
    #[repr(C)]
    struct BrowseInfoW {
        owner: isize,
        root: *const c_void,
        display_name: *mut u16,
        title: *const u16,
        flags: u32,
        callback: *const c_void,
        lparam: isize,
        image: i32,
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHBrowseForFolderW(bi: *const BrowseInfoW) -> *mut c_void;
        fn SHGetPathFromIDListW(pidl: *const c_void, path: *mut u16) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoTaskMemFree(p: *const c_void);
    }
    const BIF_RETURNONLYFSDIRS: u32 = 0x1;
    const BIF_NEWDIALOGSTYLE: u32 = 0x40;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    let title: Vec<u16> = "Choose The Settlers IV History Edition folder (the one with S4_Main.exe)".encode_utf16().chain(std::iter::once(0)).collect();
    let mut name = [0u16; 260];
    let bi = BrowseInfoW {
        owner: 0,
        root: std::ptr::null(),
        display_name: name.as_mut_ptr(),
        title: title.as_ptr(),
        flags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        callback: std::ptr::null(),
        lparam: 0,
        image: 0,
    };
    // SAFETY: COM is initialised for this thread first; `bi` points at buffers that live across
    // the call; the returned ID list is freed with CoTaskMemFree.
    unsafe {
        CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
        let pidl = SHBrowseForFolderW(&bi);
        if pidl.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let ok = SHGetPathFromIDListW(pidl, path.as_mut_ptr());
        CoTaskMemFree(pidl);
        if ok == 0 {
            return None;
        }
        let n = path.iter().position(|&c| c == 0).unwrap_or(path.len());
        Some(PathBuf::from(std::ffi::OsString::from_wide(&path[..n])))
    }
}

/// zenity / kdialog's folder dialog; `None` when cancelled or there is none (standard error says
/// to set S4_DIR).
#[cfg(not(windows))]
fn pick_folder() -> Option<PathBuf> {
    s4_formats::desktop::pick_folder("Choose The Settlers IV History Edition folder (the one with S4_Main.exe)")
}
