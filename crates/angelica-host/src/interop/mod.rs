//! Windows calls and game-memory access are confined to this module.
mod act;
mod chat;
mod clipboard;
mod component;
mod crash;
mod cursor;
mod decide;
mod entries;
mod excel_rows;
mod file_reads;
#[cfg(test)]
mod gallery;
mod game;
mod input;
mod intercepts;
pub mod memory;
mod menus;
mod overlay;
mod painter;
pub mod proxy;
mod sound;
mod startup;
mod system_menu;
mod text_case;
mod watch;
mod writes;

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

/// Opens a link in the player's browser on a thread of its own, so the
/// game's frame never waits for the browser. The host checked the link:
/// an `https` URL to a site the player approved.
pub(super) fn open_link(url: String) {
    let spawned = std::thread::Builder::new()
        .name("angelica-link".into())
        .spawn(move || {
            use windows_sys::Win32::System::Com::{
                COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
            };
            let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
            let verb: Vec<u16> = "open\0".encode_utf16().collect();
            // SAFETY: the thread initializes COM for itself, as the shell
            // asks of a thread calling ShellExecuteW, and uninitializes it
            // below.
            let com = unsafe {
                CoInitializeEx(
                    std::ptr::null(),
                    (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
                )
            };
            // SAFETY: both strings are NUL-terminated UTF-16 that outlive
            // the call.
            let result = unsafe {
                windows_sys::Win32::UI::Shell::ShellExecuteW(
                    std::ptr::null_mut(),
                    verb.as_ptr(),
                    wide.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                )
            };
            if com >= 0 {
                // SAFETY: CoInitializeEx succeeded on this thread above.
                unsafe { CoUninitialize() };
            }
            if (result as isize) <= 32 {
                log(&format!("cannot open a link: {}", result as isize));
            }
        });
    if spawned.is_err() {
        log("cannot open a link: no thread");
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
