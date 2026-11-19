//! Windows calls and game-memory access are confined to this module.
mod act;
mod action_names;
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
mod game_string;
mod input;
pub mod memory;
mod menus;
mod outgoing_chat;
mod overlay;
mod painter;
pub mod proxy;
mod quest_say;
mod shader_compiler;
mod sound;
mod startup;
mod system_dll;
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
/// Appends a line to `host.log` with UTC time and process ID.
pub(super) fn log(message: &str) {
    if let Some(directory) = startup::LOG_DIRECTORY.get() {
        log_in(directory, "host.log", message);
        if message.starts_with("startup ") || message.starts_with("start:") {
            log_in(directory, "host-bootstrap.log", message);
        }
    }
}

pub(super) fn log_in(directory: &std::path::Path, name: &str, message: &str) {
    guard((), || {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join(name))
        {
            let line = format!("{} pid={} {message}\n", now(), std::process::id());
            let _ = file.write_all(line.as_bytes());
        }
    });
}

/// UTC time to the millisecond.
fn now() -> String {
    let mut time = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    // SAFETY: GetSystemTime only writes the owned SYSTEMTIME.
    unsafe { windows_sys::Win32::System::SystemInformation::GetSystemTime(&raw mut time) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        time.wYear,
        time.wMonth,
        time.wDay,
        time.wHour,
        time.wMinute,
        time.wSecond,
        time.wMilliseconds
    )
}

/// Keeps a previous copy of a log that exceeds the size limit at launch.
pub(super) fn rotate_log(directory: &std::path::Path, name: &str, previous: &str) {
    const LOG_LIMIT: u64 = 2 * 1024 * 1024;
    let log = directory.join(name);
    if std::fs::metadata(&log).is_ok_and(|meta| meta.len() > LOG_LIMIT) {
        let _ = std::fs::rename(&log, directory.join(previous));
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
