use super::{
    game::WindowsGame,
    memory::{executable_snapshot, module_path},
    overlay::OverlayHooks,
};
use crate::{
    RELEASE_KEY,
    catalog::Catalog,
    config::Config,
    excel_text::ExcelText,
    host::Host,
    manifest::Manifest,
    package::FileSet,
    sandbox::{Sandbox, Services},
    text_case::TextCase,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::{
    Foundation::HINSTANCE,
    System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    },
};

static MODULE: AtomicUsize = AtomicUsize::new(0);
static STARTED: AtomicU8 = AtomicU8::new(0);
pub(super) static LOG_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
static STATE: OnceLock<State> = OnceLock::new();
struct State {
    host: Arc<Host>,
    overlay: Mutex<Option<OverlayHooks>>,
}

/// # Safety
/// Windows supplies the DLL notification arguments.
#[unsafe(no_mangle)]
unsafe extern "system" fn DllMain(module: HINSTANCE, reason: u32, _: *mut std::ffi::c_void) -> i32 {
    if reason == 1 {
        MODULE.store(module as usize, Ordering::Release);
    }
    1
}

#[unsafe(no_mangle)]
extern "system" fn angelica_start_v1() -> i32 {
    if STARTED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return if STARTED.load(Ordering::Acquire) == 2 {
            0
        } else {
            2
        };
    }
    let status = super::guard(1, || match initialize() {
        Ok(()) => {
            super::log("host started");
            0
        }
        Err(error) => {
            super::log(&format!("startup failed: {error}"));
            1
        }
    });
    STARTED.store(if status == 0 { 2 } else { 3 }, Ordering::Release);
    status
}

fn initialize() -> Result<(), String> {
    let runtime_path = module_path(MODULE.load(Ordering::Acquire))?;
    let root = runtime_path.parent().ok_or("runtime has no directory")?;
    let root = if root.file_name().is_some_and(|name| name == "staged") {
        root.parent()
            .ok_or("staged host has no installation directory")?
    } else {
        root
    };
    let initial_logs = root.join("logs");
    std::fs::create_dir_all(&initial_logs).map_err(|e| e.to_string())?;
    let config = Config::load(root).inspect_err(|_| {
        let _ = LOG_DIRECTORY.set(initial_logs);
    })?;
    std::fs::create_dir_all(&config.log_directory).map_err(|e| e.to_string())?;
    let _ = LOG_DIRECTORY.set(config.log_directory.clone());

    let (base, snapshot, hash) = executable_snapshot()?;
    let (catalog, map_status) = load_map(&config, base, &snapshot, hash);
    for line in &map_status {
        super::log(line);
    }
    let catalog = Arc::new(catalog);
    let excel = Arc::new(ExcelText::default());

    let case = Arc::new(TextCase::default());
    let executable = module_path(base)?;
    let game = Arc::new(WindowsGame {
        catalog: catalog.clone(),
        base,
        excel: excel.clone(),
        case: case.clone(),
        version: WindowsGame::read_version(&executable),
    });
    // The executable is in `game/`; the archives are in `game/sqpack`.
    let installation = executable.parent().and_then(std::path::Path::parent);
    let services = Arc::new(Services {
        data: Arc::new(crate::game_data::GameData::new(installation)),
        excel,
        case,
        game,
        starting: AtomicBool::new(false),
    });
    let sandbox = Arc::new(Sandbox::new()?);
    let mut host = Host::new(
        sandbox.clone(),
        config.limits(),
        services,
        crate::host::Directories {
            grants: config.grants_directory.clone(),
            state: config
                .state_directory
                .clone()
                .unwrap_or_else(|| root.join("state")),
            storage: config.storage_directory.clone(),
        },
        Arc::new(|line: &str| super::log(line)),
        Box::new(|folder: &Path| super::open_folder(folder)),
    );
    discover_plugins(&config, &sandbox, &mut host);

    // Keep code and callbacks mapped even if a later tool retains one of our hooks.
    let mut pinned = std::ptr::null_mut();
    // SAFETY: FROM_ADDRESS identifies this DLL from a known function address; PIN keeps
    // all callback code mapped until process exit, so concurrent detours cannot dangle.
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            (angelica_start_v1 as *const ()).cast::<u16>(),
            &raw mut pinned,
        )
    } == 0
    {
        return Err("failed to pin the native host".into());
    }

    host.start_all();
    let host = Arc::new(host);
    super::crash::install(host.clone(), catalog, config.log_directory.clone());
    let overlay = match OverlayHooks::install(host.clone(), map_status) {
        Ok(hooks) => {
            super::log("overlay hooks installed");
            Some(hooks)
        }
        Err(error) => {
            host.diagnostic(format!("overlay unavailable: {error}"));
            None
        }
    };
    let _ = STATE.set(State {
        host,
        overlay: Mutex::new(overlay),
    });
    Ok(())
}

/// The signed map resolved for this executable, and the lines the menu
/// shows about it. Without a usable map every entry is absent.
fn load_map(config: &Config, base: usize, snapshot: &[u8], hash: String) -> (Catalog, Vec<String>) {
    let mut map_status = Vec::new();
    let catalog = match FileSet::load(&config.map_directory, RELEASE_KEY.as_ref())
        .map_err(|e| e.to_string())
        .and_then(|files| Catalog::from_signed_files(&files, snapshot, base as u64, hash.clone()))
    {
        Ok((_, catalog)) => {
            map_status.push(format!(
                "Game map: {} for this build, {} entries present, {} absent",
                if catalog.verified {
                    "verified"
                } else {
                    "resolved"
                },
                catalog.present.len(),
                catalog.absent.len()
            ));
            map_status.extend(
                catalog
                    .absent
                    .iter()
                    .map(|(name, reason)| format!("  {name}: {reason}")),
            );
            catalog
        }
        Err(error) => {
            map_status.push(format!("Game map unavailable: {error}"));
            Catalog {
                executable_sha256: hash,
                ..Catalog::default()
            }
        }
    };
    (catalog, map_status)
}

fn discover_plugins(config: &Config, sandbox: &Sandbox, host: &mut Host) {
    let listing = match std::fs::read_dir(&config.plugin_directory) {
        Ok(listing) => listing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            host.diagnostic(format!("plugin directory: {e}"));
            return;
        }
    };
    for item in listing {
        let result = (|| {
            let item = item.map_err(|e| e.to_string())?;
            if !item.file_type().map_err(|e| e.to_string())?.is_dir() {
                return Ok(());
            }
            let files =
                FileSet::load(&item.path(), RELEASE_KEY.as_ref()).map_err(|e| e.to_string())?;
            let manifest = std::str::from_utf8(
                files
                    .get("angelica.toml")
                    .ok_or("the package has no angelica.toml")?,
            )
            .map_err(|e| e.to_string())?;
            let manifest = Manifest::parse(manifest).map_err(|e| e.to_string())?;
            if item.file_name() != manifest.id() {
                return Err(format!(
                    "the folder name differs from the plugin id {}",
                    manifest.id()
                ));
            }
            let artifact = files.get("plugin.cwasm").ok_or(
                "the package has no compiled component; compile it outside the game first",
            )?;
            // SAFETY: FileSet authenticated every byte with the release signing key.
            // Release packages accept compiler artifacts only from the trusted build
            // pipeline, never native code supplied by a plugin author.
            let component =
                unsafe { super::component::load_authenticated(&sandbox.engine, artifact) }?;
            host.install(manifest, &component)
        })();
        if let Err(error) = result {
            host.diagnostic(format!("plugin refused: {error}"));
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn angelica_stop_v1() -> i32 {
    super::guard(1, || {
        let Some(state) = STATE.get() else {
            return 0;
        };
        state.host.stop_all();
        let mut removed = true;
        if let Ok(mut overlay) = state.overlay.lock()
            && let Some(overlay) = &mut *overlay
        {
            removed &= overlay.disable();
        }
        i32::from(!removed)
    })
}
