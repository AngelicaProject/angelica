use crate::{
    RELEASE_KEY,
    trust::{self, Config, LoadError},
};
use std::{
    ffi::c_void,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
    },
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use windows_sys::{
    Win32::{
        Foundation::{FreeLibrary, HINSTANCE},
        System::{
            Diagnostics::Debug::{GetThreadErrorMode, SEM_FAILCRITICALERRORS, SetThreadErrorMode},
            LibraryLoader::{
                GetModuleFileNameW, GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
            },
            SystemInformation::GetSystemDirectoryW,
            Threading::{GetCurrentThread, QueueUserAPC},
        },
    },
    core::GUID,
};

static MODULE: AtomicUsize = AtomicUsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
static STARTUP_FAILED: AtomicBool = AtomicBool::new(false);
static SYSTEM: OnceLock<Result<usize, String>> = OnceLock::new();
static LOADER_LOG: OnceLock<Option<PathBuf>> = OnceLock::new();
static LOADER_START: OnceLock<Instant> = OnceLock::new();
const E_FAIL: i32 = 0x8000_4005_u32.cast_signed();

fn boundary<T>(fallback: T, work: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) => value,
        Err(payload) => {
            // A panic payload can itself panic when dropped.
            std::mem::forget(payload);
            fallback
        }
    }
}

fn wide(path: &Path) -> Result<Vec<u16>, LoadError> {
    let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(LoadError::Platform("path contains NUL".into()));
    }
    value.push(0);
    Ok(value)
}

fn module_path(module: usize) -> Result<PathBuf, LoadError> {
    let mut buffer = vec![0u16; 32768];
    // SAFETY: module is from Windows, and buffer has the passed capacity.
    let len =
        unsafe { GetModuleFileNameW(module as HINSTANCE, buffer.as_mut_ptr(), 32768) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err(os_error("GetModuleFileNameW"));
    }
    Ok(std::ffi::OsString::from_wide(&buffer[..len]).into())
}

fn os_error(call: &str) -> LoadError {
    LoadError::Platform(format!("{call}: {}", std::io::Error::last_os_error()))
}

fn log(message: &str) {
    boundary((), || {
        let path = LOADER_LOG.get_or_init(loader_log_path);
        if let Some(path) = path
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
        {
            let utc = utc_now();
            let elapsed = LOADER_START.get_or_init(Instant::now).elapsed().as_millis();
            let line = format!(
                "{utc} pid={} elapsed_ms={elapsed} {message}\n",
                std::process::id()
            );
            let _ = file.write_all(line.as_bytes());
        }
    });
}

fn utc_now() -> String {
    let mut time = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    // SAFETY: Windows only writes the owned SYSTEMTIME.
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

fn loader_log_path() -> Option<PathBuf> {
    let proxy = module_path(MODULE.load(Ordering::Acquire)).ok();
    let game = proxy.as_deref().and_then(Path::parent);
    let installation = game
        .and_then(|folder| Config::read(folder).ok())
        .map(|config| config.installation_dir.join("logs"));
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(path).join("Angelica/logs"));
    game.map(Path::to_owned)
        .into_iter()
        .chain(installation)
        .chain(local)
        .find_map(|folder| {
            std::fs::create_dir_all(&folder).ok()?;
            let path = folder.join("angelica-loader.log");
            if std::fs::metadata(&path).is_ok_and(|file| file.len() > 1024 * 1024) {
                let _ = std::fs::rename(&path, folder.join("angelica-loader.old.log"));
            }
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()?;
            Some(path)
        })
}

fn system_module() -> Result<usize, LoadError> {
    let result = SYSTEM.get_or_init(|| {
        let mut buffer = vec![0u16; 32768];
        // SAFETY: buffer has the stated capacity.
        let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), 32768) } as usize;
        if len == 0 || len >= buffer.len() {
            return Err(os_error("GetSystemDirectoryW").to_string());
        }
        let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len])).join("dinput8.dll");
        let name = wide(&path).map_err(|e| e.to_string())?;
        // SAFETY: full system path is NUL terminated; only system dependencies are searched.
        let module = unsafe {
            LoadLibraryExW(
                name.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            Err(os_error("load system dinput8.dll").to_string())
        } else {
            log("system-dinput8: loaded system module for forwarding");
            Ok(module as usize)
        }
    });
    result
        .as_ref()
        .copied()
        .map_err(|error| LoadError::Platform(error.clone()))
}

fn address(name: &[u8]) -> Result<usize, LoadError> {
    let module = system_module()?;
    // SAFETY: retained system module and static NUL-terminated export name.
    unsafe { GetProcAddress(module as HINSTANCE, name.as_ptr()) }
        .map(|f| f as usize)
        .ok_or_else(|| os_error("GetProcAddress(system dinput8.dll)"))
}

fn forward<T: Copy>(fallback: T, work: impl FnOnce() -> Result<T, LoadError>) -> T {
    boundary(fallback, || {
        if STARTUP_FAILED.swap(false, Ordering::AcqRel) {
            log("the start of the host could not be queued; runtime startup disabled");
        }
        match work() {
            Ok(value) => value,
            Err(error) => {
                log(&error.to_string());
                fallback
            }
        }
    })
}

/// # Safety
/// Windows supplies valid DLL notification arguments.
#[unsafe(no_mangle)]
unsafe extern "system" fn DllMain(module: HINSTANCE, reason: u32, reserved: *mut c_void) -> i32 {
    boundary(1, || {
        if reason == 1 {
            MODULE.store(module as usize, Ordering::Release);
            // A non-null reserved pointer identifies static process initialization.
            // A dynamically loaded proxy is too late: the executable already runs.
            if !reserved.is_null() {
                // Windows delivers the initial thread's queued calls when
                // process initialization ends: after every library is
                // initialized and the loader lock is released, and before
                // the executable's entry point.
                // SAFETY: a static function with the documented signature,
                // queued to the current thread.
                let queued =
                    unsafe { QueueUserAPC(Some(after_initialization), GetCurrentThread(), 0) };
                STARTUP_FAILED.store(queued == 0, Ordering::Release);
            }
        }
        1
    })
}

/// Runs on the initial thread when process initialization has ended and the
/// executable's entry point has not run yet: starts the host once. The
/// executable's code and its entry point stay as they are, so a tool that
/// rewrites or checks the entry point is not disturbed.
unsafe extern "system" fn after_initialization(_: usize) {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let completed = boundary(false, || {
        log(&format!(
            "bootstrap: begin version={} architecture={}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH
        ));
        if let Err(error) = load_host() {
            startup_failure(&error);
        }
        true
    });
    if !completed {
        boundary((), || {
            startup_failure(&LoadError::Platform(
                "runtime startup panicked; Angelica disabled".into(),
            ));
        });
    }
}

fn startup_failure(error: &LoadError) {
    use windows_sys::Win32::{
        Globalization::GetUserDefaultUILanguage,
        UI::WindowsAndMessaging::{MB_ICONWARNING, MB_SETFOREGROUND, MessageBoxW},
    };
    log(&error.to_string());
    let Ok(game) = module_path(0) else { return };
    if !game
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("ffxiv_dx11.exe"))
    {
        return;
    }
    let Some(directory) = game.parent() else {
        return;
    };
    let config = Config::read(directory).ok();
    let root = config
        .as_ref()
        .map(|config| config.installation_dir.join("reports"));
    let fallback = std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(path).join("Angelica/reports"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file = format!("loader-{stamp}-{}.json", std::process::id());
    let (call, code) = match error {
        LoadError::Windows { call, source } => (Some(*call), source.raw_os_error()),
        LoadError::Io { source, .. } => (None, source.raw_os_error()),
        _ => (None, None),
    };
    let report = serde_json::json!({
        "stage": "load-runtime",
        "time_unix_ms": stamp,
        "process_id": std::process::id(),
        "loader_version": env!("CARGO_PKG_VERSION"),
        "runtime_path": config.and_then(|config| config.runtime_path().ok()),
        "windows_call": call,
        "windows_error": code,
        "error": error.to_string(),
        "modules": loaded_modules(),
    });
    if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
        for folder in root
            .into_iter()
            .chain(fallback)
            .chain([directory.join("angelica-reports")])
        {
            if std::fs::create_dir_all(&folder)
                .and_then(|()| std::fs::write(folder.join(&file), &bytes))
                .is_ok()
            {
                break;
            }
        }
    }
    // SAFETY: This runs after process initialization, outside the loader lock.
    let russian = unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x19;
    let message = if russian {
        "Angelica не запустилась. Игра продолжит запуск без плагинов. Чтобы сохранить диагностику, откройте «Angelica Report» из меню «Пуск» или «Отчёт Angelica» в установщике."
    } else {
        "Angelica could not start. The game will continue without its plugins. Open Angelica Report from the Start menu to save the diagnostics."
    };
    let message: Vec<_> = message.encode_utf16().chain([0]).collect();
    // SAFETY: The title and message are NUL-terminated, and startup has no owner window.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            windows_sys::w!("Angelica"),
            MB_ICONWARNING | MB_SETFOREGROUND,
        )
    };
}

fn loaded_modules() -> Vec<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
            TH32CS_SNAPMODULE,
        },
    };
    // SAFETY: Zero selects the current process. The snapshot is read-only.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut module = MODULEENTRY32W {
        dwSize: u32::try_from(std::mem::size_of::<MODULEENTRY32W>())
            .expect("Windows module structure fits DWORD"),
        ..Default::default()
    };
    let mut paths = Vec::new();
    // SAFETY: The handle is live and the structure is sized and writable.
    let mut more = unsafe { Module32FirstW(snapshot, &raw mut module) };
    while more != 0 && paths.len() < 512 {
        let end = module
            .szExePath
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(module.szExePath.len());
        paths.push(String::from_utf16_lossy(&module.szExePath[..end]));
        // SAFETY: The snapshot and output structure remain live.
        more = unsafe { Module32NextW(snapshot, &raw mut module) };
    }
    // SAFETY: This is the owned snapshot handle, closed once after enumeration.
    unsafe { CloseHandle(snapshot) };
    paths
}

fn load_host() -> Result<(), LoadError> {
    log("configuration: resolving proxy directory and installation configuration");
    let proxy = module_path(MODULE.load(Ordering::Acquire))?;
    let directory = proxy
        .parent()
        .ok_or_else(|| LoadError::Platform("proxy has no parent directory".into()))?;
    let runtime = Config::read(directory)?.runtime_path()?;
    log(&format!(
        "configuration: selected runtime {}",
        runtime.display()
    ));
    let runtime = std::fs::canonicalize(&runtime).map_err(|source| LoadError::Io {
        path: runtime,
        source,
    })?;
    // Deny writes and deletion while checking and loading the same file.
    let mut locked: File = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&runtime)
        .map_err(|source| LoadError::Io {
            path: runtime.clone(),
            source,
        })?;
    let mut bytes = Vec::new();
    log("verification: reading locked runtime and detached signature");
    locked
        .read_to_end(&mut bytes)
        .map_err(|source| LoadError::Io {
            path: runtime.clone(),
            source,
        })?;
    let signature = trust::read(&runtime.with_extension("dll.sig"))?;
    trust::verify(&bytes, &signature, RELEASE_KEY.as_ref())?;
    log(&format!(
        "verification: Ed25519 passed runtime_bytes={}",
        bytes.len()
    ));
    let path = wide(&runtime)?;
    let mut previous = 0;
    // SAFETY: The error mode is local to this thread and the previous value is writable.
    let changed = unsafe {
        SetThreadErrorMode(
            GetThreadErrorMode() | SEM_FAILCRITICALERRORS,
            &raw mut previous,
        )
    };
    log("windows-loader: LoadLibraryExW(runtime.dll) begin; dependency search=System32");
    // SAFETY: the locked file's exact bytes have passed strict signature verification.
    // Non-system sibling dependency search is deliberately excluded.
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    let error = module.is_null().then(std::io::Error::last_os_error);
    if changed != 0 {
        // SAFETY: Restore the exact previous mode on the same thread.
        unsafe { SetThreadErrorMode(previous, std::ptr::null_mut()) };
    }
    if let Some(source) = error {
        return Err(LoadError::Windows {
            call: "LoadLibraryExW(runtime.dll)",
            source,
        });
    }
    log("windows-loader: runtime mapped; resolving angelica_start_v1");
    // SAFETY: module is loaded and export name is NUL terminated.
    let start = unsafe { GetProcAddress(module, c"angelica_start_v1".as_ptr().cast()) };
    let Some(start) = start else {
        // SAFETY: startup has not run, so no host callbacks have been registered.
        unsafe { FreeLibrary(module) };
        return Err(LoadError::Platform(
            "runtime.dll has no angelica_start_v1 export".into(),
        ));
    };
    // SAFETY: the signed host's documented ABI is a no-argument system function returning status.
    let start: unsafe extern "system" fn() -> i32 = unsafe { std::mem::transmute(start) };
    log("runtime-abi: calling angelica_start_v1");
    // SAFETY: signature-verified host implements the versioned startup ABI and catches its panics.
    let status = unsafe { start() };
    log(&format!(
        "runtime-abi: angelica_start_v1 returned status={status}"
    ));
    if status != 0 {
        // SAFETY: a nonzero result guarantees the host removed all callbacks and hooks.
        unsafe { FreeLibrary(module) };
        return Err(LoadError::Platform(format!(
            "angelica_start_v1 failed with status {status}"
        )));
    }
    // Keep the successful host and system forwarding module mapped until process exit.
    log("runtime.dll started");
    Ok(())
}

/// # Safety
/// Arguments must satisfy the system `DirectInput8Create` contract.
#[unsafe(no_mangle)]
unsafe extern "system" fn DirectInput8Create(
    instance: HINSTANCE,
    version: u32,
    iid: *const GUID,
    output: *mut *mut c_void,
    outer: *mut c_void,
) -> i32 {
    forward(E_FAIL, || {
        // SAFETY: the named system export has this documented signature.
        let function: unsafe extern "system" fn(
            HINSTANCE,
            u32,
            *const GUID,
            *mut *mut c_void,
            *mut c_void,
        ) -> i32 = unsafe { std::mem::transmute(address(b"DirectInput8Create\0")?) };
        // SAFETY: caller supplies the system API's arguments unchanged.
        Ok(unsafe { function(instance, version, iid, output, outer) })
    })
}

/// # Safety
/// Arguments must satisfy the system COM `DllGetClassObject` contract.
#[unsafe(no_mangle)]
unsafe extern "system" fn DllGetClassObject(
    class: *const GUID,
    iid: *const GUID,
    output: *mut *mut c_void,
) -> i32 {
    forward(E_FAIL, || {
        // SAFETY: the named system COM export has this signature.
        let function: unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> i32 =
            unsafe { std::mem::transmute(address(b"DllGetClassObject\0")?) };
        // SAFETY: pass through the caller's COM arguments unchanged.
        Ok(unsafe { function(class, iid, output) })
    })
}

macro_rules! simple_export {
    ($name:ident, $result:ty, $fallback:expr) => {
        #[unsafe(no_mangle)]
        extern "system" fn $name() -> $result {
            forward($fallback, || {
                // SAFETY: this exact system export has the specified no-argument signature.
                let function: unsafe extern "system" fn() -> $result = unsafe {
                    std::mem::transmute(address(concat!(stringify!($name), "\0").as_bytes())?)
                };
                // SAFETY: no-argument system export from the retained module.
                Ok(unsafe { function() })
            })
        }
    };
}
simple_export!(DllCanUnloadNow, i32, 1);
simple_export!(DllRegisterServer, i32, E_FAIL);
simple_export!(DllUnregisterServer, i32, E_FAIL);
simple_export!(GetdfDIJoystick, *const c_void, std::ptr::null());
