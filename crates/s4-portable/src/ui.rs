//! The launcher's Windows UI: a small progress window with a cancel button for the import, the
//! shell's folder dialog and message boxes.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{COLOR_BTNFACE, DEFAULT_GUI_FONT, GetStockObject, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, InitCommonControlsEx};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, IDC_ARROW, KillTimer, LoadCursorW, MB_ICONERROR, MB_OK,
    MSG, MessageBoxW, PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetTimer, SetWindowTextW, ShowWindow, TranslateMessage, WM_CLOSE, WM_COMMAND,
    WM_DESTROY, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_VISIBLE,
};

use crate::import;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn message_box(title: &str, text: &str) {
    let (t, m) = (w(title), w(text));
    // SAFETY: NUL-terminated UTF-16 buffers that outlive the call.
    unsafe { MessageBoxW(0, m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR) };
}

/// The shell's folder dialog (declared here: windows-sys 0.52 gates `SHBrowseForFolderW`
/// behind a feature it does not offer); `None` when cancelled.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
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
    let title = w(title);
    let mut name = [0u16; 260];
    let bi = BrowseInfoW { owner: 0, root: std::ptr::null(), display_name: name.as_mut_ptr(), title: title.as_ptr(), flags: 0x1 | 0x40, callback: std::ptr::null(), lparam: 0, image: 0 };
    // SAFETY: COM initialised for the thread first; `bi` points at buffers that outlive the call;
    // the ID list is freed with CoTaskMemFree.
    unsafe {
        CoInitializeEx(std::ptr::null_mut(), 0x2);
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

/// What the import thread reports to the window.
#[derive(Default)]
struct State {
    done: u64,
    total: u64,
    file: String,
    result: Option<Result<(usize, u64), String>>,
}

struct Shared {
    state: Mutex<State>,
    cancel: AtomicBool,
    label: Mutex<HWND>,
    bar: Mutex<HWND>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
const CANCEL_ID: usize = 1;
const TIMER_ID: usize = 1;
const PBM_SETRANGE32: u32 = 0x0406;
const PBM_SETPOS: u32 = 0x0402;

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(sh) = SHARED.get() else {
        // SAFETY: the default procedure for messages before the state exists.
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    match msg {
        WM_COMMAND if wp & 0xffff == CANCEL_ID => {
            sh.cancel.store(true, Ordering::Relaxed);
            0
        }
        WM_CLOSE => {
            sh.cancel.store(true, Ordering::Relaxed);
            0
        }
        WM_TIMER => {
            let st = sh.state.lock().unwrap();
            let text = if st.total == 0 {
                "Looking at the game's files…".to_string()
            } else {
                format!("Copying the game data from your The Settlers IV install ({} of {} MB)\n{}", st.done >> 20, st.total >> 20, st.file)
            };
            let permille = (st.done * 1000).checked_div(st.total).unwrap_or(0) as usize;
            let finished = st.result.is_some();
            drop(st);
            let label = *sh.label.lock().unwrap();
            let bar = *sh.bar.lock().unwrap();
            let t = w(&text);
            // SAFETY: valid child windows of this window; `t` outlives the calls.
            unsafe {
                SetWindowTextW(label, t.as_ptr());
                SendMessageW(bar, PBM_SETPOS, permille, 0);
                if finished {
                    KillTimer(hwnd, TIMER_ID);
                    DestroyWindow(hwnd);
                }
            }
            0
        }
        WM_DESTROY => {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            0
        }
        // SAFETY: the default procedure for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// [`import::import`] on a worker thread under a progress window with a cancel button.
pub fn import_with_progress(src: &Path, data: &Path) -> Result<(usize, u64), String> {
    let sh = SHARED.get_or_init(|| Arc::new(Shared { state: Mutex::default(), cancel: AtomicBool::new(false), label: Mutex::new(0), bar: Mutex::new(0) })).clone();
    let worker = {
        let (sh, src, data) = (sh.clone(), src.to_path_buf(), data.to_path_buf());
        std::thread::spawn(move || {
            let r = import::import(&src, &data, &sh.cancel, |p| {
                let mut st = sh.state.lock().unwrap();
                st.done = p.done;
                st.total = p.total;
                st.file = p.file.to_string();
            });
            sh.state.lock().unwrap().result = Some(r);
        })
    };
    let class = w("S4RSImport");
    let title = w("S4RS: importing the game data");
    // SAFETY: plain Win32 window creation and a message loop on this thread; every string
    // buffer outlives the calls that read it.
    unsafe {
        let icc = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_PROGRESS_CLASS };
        InitCommonControlsEx(&icc);
        let inst = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: 0,
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: (COLOR_BTNFACE + 1) as HBRUSH,
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(0, class.as_ptr(), title.as_ptr(), WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU, CW_USEDEFAULT, CW_USEDEFAULT, 560, 190, 0, 0, inst, std::ptr::null());
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let (st, pb, bt) = (w("STATIC"), w("msctls_progress32"), w("BUTTON"));
        let (empty, cancel) = (w(""), w("Cancel"));
        let label = CreateWindowExW(0, st.as_ptr(), empty.as_ptr(), WS_CHILD | WS_VISIBLE, 16, 14, 520, 44, hwnd, 0, inst, std::ptr::null());
        let bar = CreateWindowExW(0, pb.as_ptr(), empty.as_ptr(), WS_CHILD | WS_VISIBLE, 16, 66, 520, 22, hwnd, 0, inst, std::ptr::null());
        let button = CreateWindowExW(0, bt.as_ptr(), cancel.as_ptr(), WS_CHILD | WS_VISIBLE, 436, 104, 100, 28, hwnd, CANCEL_ID as isize, inst, std::ptr::null());
        for c in [label, button] {
            SendMessageW(c, WM_SETFONT, font as usize, 1);
        }
        SendMessageW(bar, PBM_SETRANGE32, 0, 1000);
        *sh.label.lock().unwrap() = label;
        *sh.bar.lock().unwrap() = bar;
        ShowWindow(hwnd, SW_SHOW);
        SetTimer(hwnd, TIMER_ID, 100, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let _ = worker.join();
    sh.state.lock().unwrap().result.take().unwrap_or_else(|| Err("cancelled".into()))
}
