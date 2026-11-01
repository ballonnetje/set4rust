//! Windows: the shell's folder dialog and message boxes (the same calls as [`desktop.rs`] offers
//! elsewhere), declared here so this crate needs no Windows bindings.

use std::ffi::c_void;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A folder the player chooses; `None` when cancelled.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
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
    let title = wide(title);
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

/// A message box, and the text on standard error.
pub fn message(title: &str, text: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(owner: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    const MB_ICONERROR: u32 = 0x10;
    eprintln!("{title}: {text}");
    let (t, m) = (wide(title), wide(text));
    // SAFETY: both strings are NUL-terminated UTF-16 buffers that outlive the call.
    unsafe { MessageBoxW(0, m.as_ptr(), t.as_ptr(), MB_ICONERROR) };
}
