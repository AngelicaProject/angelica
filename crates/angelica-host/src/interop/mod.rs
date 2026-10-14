//! Windows calls and game-memory access are confined to this module.
mod act;
mod chat;
mod clipboard;
mod component;
mod crash;
mod decide;
mod entries;
mod excel_rows;
mod file_reads;
mod game;
mod game_ui;
mod input;
pub mod memory;
mod overlay;
mod painter;
pub mod proxy;
mod sound;
mod startup;
mod text_case;
mod watch;

pub(super) fn guard<T>(fallback: T, work: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) => value,
        Err(payload) => {
            std::mem::forget(payload);
            fallback
        }
    }
}
/// Appends a line to `host.log`, after the local time.
pub(super) fn log(message: &str) {
    guard((), || {
        use std::io::Write;
        if let Some(directory) = startup::LOG_DIRECTORY.get()
            && let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("host.log"))
        {
            let _ = writeln!(file, "{} {message}", now());
        }
    });
}

/// The local time to the millisecond, as `2026-10-13 17:54:03.120`.
fn now() -> String {
    let mut time = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    // SAFETY: GetLocalTime only writes the owned SYSTEMTIME.
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&raw mut time) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}

/// Keeps `host.log` small: a log over `LOG_LIMIT` bytes at start becomes
/// `host.old.log`, replacing the one before.
pub(super) fn rotate_log(directory: &std::path::Path) {
    const LOG_LIMIT: u64 = 2 * 1024 * 1024;
    let log = directory.join("host.log");
    if std::fs::metadata(&log).is_ok_and(|meta| meta.len() > LOG_LIMIT) {
        let _ = std::fs::rename(&log, directory.join("host.old.log"));
    }
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
