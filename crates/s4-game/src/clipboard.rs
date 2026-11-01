//! The Windows clipboard, as the release's clipboard object (vtable at 0x11b58b4: +0 / +4 set the
//! text, +8 gets at most `n` characters) uses it for the random map keys. [inf] Unicode text
//! here; elsewhere than Windows nothing.

/// Puts `text` on the clipboard.
pub fn set_text(text: &str) {
    #[cfg(windows)]
    // SAFETY: the clipboard calls with a moveable global block of the text and its NUL; the
    // block belongs to the clipboard once `SetClipboardData` took it, and is freed by us only
    // when it did not.
    unsafe {
        use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
        use windows_sys::Win32::Foundation::GlobalFree;
        use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
        use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        if OpenClipboard(0) == 0 {
            return;
        }
        EmptyClipboard();
        let block = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        if !block.is_null() {
            let p = GlobalLock(block) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                GlobalUnlock(block);
                if SetClipboardData(CF_UNICODETEXT as u32, block as isize) == 0 {
                    GlobalFree(block);
                }
            } else {
                GlobalFree(block);
            }
        }
        CloseClipboard();
    }
    #[cfg(not(windows))]
    let _ = text;
}

/// The clipboard's text, at most `max` characters; `None` without text.
pub fn get_text(max: usize) -> Option<String> {
    #[cfg(windows)]
    // SAFETY: the clipboard's handle is read while it is open and locked; the text is copied up
    // to its NUL.
    unsafe {
        use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
        use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
        use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
        if OpenClipboard(0) == 0 {
            return None;
        }
        let h = GetClipboardData(CF_UNICODETEXT as u32);
        let mut out = None;
        if h != 0 {
            let p = GlobalLock(h as *mut core::ffi::c_void) as *const u16;
            if !p.is_null() {
                let mut v = Vec::new();
                while v.len() < max && *p.add(v.len()) != 0 {
                    v.push(*p.add(v.len()));
                }
                out = Some(String::from_utf16_lossy(&v));
                GlobalUnlock(h as *mut core::ffi::c_void);
            }
        }
        CloseClipboard();
        out
    }
    #[cfg(not(windows))]
    {
        let _ = max;
        None
    }
}

#[cfg(test)]
mod tests {
    /// Where a clipboard is open to this process (an interactive session), the text comes back.
    #[test]
    fn round_trip() {
        let before = super::get_text(4096);
        super::set_text("RD_H8GQE2M0");
        if let Some(t) = super::get_text(8) {
            assert_eq!(t, "RD_H8GQE");
        }
        if let Some(b) = before {
            super::set_text(&b);
        }
    }
}
