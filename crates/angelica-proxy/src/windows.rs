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
};
use windows_sys::{
    Win32::{
        Foundation::{FreeLibrary, HINSTANCE},
        System::{
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
        if let Ok(path) = module_path(MODULE.load(Ordering::Acquire))
            && let Some(directory) = path.parent()
            && let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("angelica-loader.log"))
        {
            let _ = writeln!(file, "{message}");
        }
    });
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
        if let Err(error) = load_host() {
            log(&error.to_string());
        }
        true
    });
    if !completed {
        log("runtime startup panicked; Angelica disabled");
    }
}

fn load_host() -> Result<(), LoadError> {
    let proxy = module_path(MODULE.load(Ordering::Acquire))?;
    let directory = proxy
        .parent()
        .ok_or_else(|| LoadError::Platform("proxy has no parent directory".into()))?;
    let runtime = Config::read(directory)?.runtime_path()?;
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
    locked
        .read_to_end(&mut bytes)
        .map_err(|source| LoadError::Io {
            path: runtime.clone(),
            source,
        })?;
    let signature = trust::read(&runtime.with_extension("dll.sig"))?;
    trust::verify(&bytes, &signature, RELEASE_KEY.as_ref())?;
    let path = wide(&runtime)?;
    // SAFETY: the locked file's exact bytes have passed strict signature verification.
    // Non-system sibling dependency search is deliberately excluded.
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(os_error("LoadLibraryExW(runtime.dll)"));
    }
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
    // SAFETY: signature-verified host implements the versioned startup ABI and catches its panics.
    let status = unsafe { start() };
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
