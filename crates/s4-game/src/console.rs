//! The packaged build is a Windows-subsystem program (no console window behind the game). Started
//! from a terminal, it attaches to that terminal's console so `--headless` and the other command
//! line output still show; output redirected to files or pipes keeps its handles. Other builds:
//! nothing to do.

/// Standard output / error without a handle (not redirected) go to the parent's console, if
/// it has one.
pub fn attach() {
    #[cfg(all(windows, feature = "package"))]
    imp::attach();
}

/// A log line with the console window (0: none), so a log shows how the package build ran.
pub fn log_state() {
    #[cfg(all(windows, feature = "package"))]
    // SAFETY: GetConsoleWindow has no preconditions.
    tracing::info!("console window: {:#x}", unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() });
}

#[cfg(all(windows, feature = "package"))]
mod imp {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};

    fn missing(which: STD_HANDLE) -> bool {
        // SAFETY: GetStdHandle has no preconditions.
        let h: HANDLE = unsafe { GetStdHandle(which) };
        h == 0 || h == INVALID_HANDLE_VALUE
    }

    pub fn attach() {
        let wanted: Vec<STD_HANDLE> = [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].into_iter().filter(|&w| missing(w)).collect();
        // SAFETY: AttachConsole fails harmlessly when the parent has no console.
        if wanted.is_empty() || unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
            return;
        }
        let name: Vec<u16> = "CONOUT$".encode_utf16().chain(std::iter::once(0)).collect();
        for w in wanted {
            if !missing(w) {
                continue;
            }
            // SAFETY: opens the attached console's output buffer; the handle stays open for the
            // process's life as its standard handle.
            unsafe {
                let h = CreateFileW(name.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, 0);
                if h != INVALID_HANDLE_VALUE {
                    SetStdHandle(w, h);
                }
            }
        }
    }
}
