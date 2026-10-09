//! Windows calls and game-memory access are confined to this module.
mod chat;
mod component;
mod crash;
mod entries;
mod excel_rows;
mod file_reads;
mod game;
mod input;
pub mod memory;
mod overlay;
mod painter;
mod startup;
mod text_case;

pub(super) fn guard<T>(fallback: T, work: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) => value,
        Err(payload) => {
            std::mem::forget(payload);
            fallback
        }
    }
}
pub(super) fn log(message: &str) {
    guard((), || {
        use std::io::Write;
        if let Some(directory) = startup::LOG_DIRECTORY.get()
            && let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("host.log"))
        {
            let _ = writeln!(file, "{message}");
        }
    });
}

/// Opens a folder in Explorer for the player.
pub(super) fn open_folder(folder: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    let path: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if (result as isize) <= 32 {
        log(&format!("cannot open {}", folder.display()));
    }
}
