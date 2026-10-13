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
    package::{FileSet, Trust},
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

/// Angelica's own updates: checked on a thread of their own when this is a
/// release build with a release key; a development build only says so.
fn self_update(config: &Config, root: &Path) -> Arc<crate::self_update::SelfUpdate> {
    use crate::self_update::{Http, SelfUpdate, Setup, Status, build_channel, spawn, spawn_map};
    let updates = Arc::new(SelfUpdate::default());
    updates.log_to(Arc::new(|line: &str| super::log(line)));
    let (Some(channel), Some(key), Ok(running)) = (
        build_channel(),
        RELEASE_KEY,
        env!("CARGO_PKG_VERSION").parse::<semver::Version>(),
    ) else {
        updates.set(Status::Development);
        updates.set_map(Status::Development);
        return updates;
    };
    spawn_map(
        config.map_directory.clone(),
        key,
        updates.clone(),
        Arc::new(Http::new()),
    );
    let settings = crate::language::Settings::path(&directories(config, root).state);
    spawn(
        Setup {
            installation: root.to_path_buf(),
            repository: crate::self_update::REPOSITORY.into(),
            key,
            running,
        },
        updates.clone(),
        Arc::new(Http::new()),
        move || {
            crate::language::Settings::load(&settings)
                .channel
                .unwrap_or(channel)
        },
    );
    updates
}

/// The catalog's index, read with the release key; none without one.
fn catalog_source() -> Option<crate::catalog_index::Source> {
    let catalog = Arc::new(crate::catalog_index::Catalog::default());
    catalog.log_to(Arc::new(|line: &str| super::log(line)));
    Some(crate::catalog_index::Source {
        catalog,
        fetch: Arc::new(crate::self_update::Http::new()),
        key: RELEASE_KEY?,
    })
}

/// The first line of a start in the log: Angelica, the game, the process.
fn log_start(base: usize, hash: &str) -> Result<(), String> {
    super::log(&format!(
        "start: Angelica {} ({}); game {}, executable sha256 {}; process {}",
        env!("CARGO_PKG_VERSION"),
        crate::self_update::build_channel().map_or("development", |c| c.name()),
        super::game::WindowsGame::read_version(&module_path(base)?),
        hash,
        std::process::id()
    ));
    Ok(())
}

/// The overlay's hooks as `diagnostics.json` asks: none when it is off.
fn install_overlay(
    host: &Arc<Host>,
    map_status: Vec<String>,
    diagnostics: crate::config::Diagnostics,
) -> Option<OverlayHooks> {
    use crate::config::OverlayMode;
    if diagnostics.overlay == OverlayMode::Off {
        return None;
    }
    match OverlayHooks::install(
        host.clone(),
        map_status,
        diagnostics.overlay == OverlayMode::On,
    ) {
        Ok(hooks) => {
            super::log("overlay hooks installed");
            Some(hooks)
        }
        Err(error) => {
            host.diagnostic(format!("overlay unavailable: {error}"));
            None
        }
    }
}

/// What `diagnostics.json` turns off, said in the log.
fn diagnostics(root: &Path) -> crate::config::Diagnostics {
    let (diagnostics, problem) = crate::config::Diagnostics::load(root);
    if let Some(problem) = problem {
        super::log(&problem);
    }
    if diagnostics != crate::config::Diagnostics::default() {
        super::log(&format!(
            "diagnostics.json turns parts off: {diagnostics:?}"
        ));
    }
    diagnostics
}

fn initialize() -> Result<(), String> {
    let runtime_path = module_path(MODULE.load(Ordering::Acquire))?;
    let root = runtime_path.parent().ok_or("runtime has no directory")?;
    // `versions/<version>/runtime.dll` is a version `current.json` named.
    let root = match root.parent() {
        Some(versions) if versions.file_name().is_some_and(|name| name == "versions") => versions
            .parent()
            .ok_or("versions has no installation directory")?,
        _ => root,
    };
    let initial_logs = root.join("logs");
    std::fs::create_dir_all(&initial_logs).map_err(|e| e.to_string())?;
    let config = Config::load(root).inspect_err(|_| {
        let _ = LOG_DIRECTORY.set(initial_logs);
    })?;
    std::fs::create_dir_all(&config.log_directory).map_err(|e| e.to_string())?;
    super::rotate_log(&config.log_directory);
    let _ = LOG_DIRECTORY.set(config.log_directory.clone());

    let (base, snapshot, hash) = executable_snapshot()?;
    log_start(base, &hash)?;
    let diagnostics = diagnostics(root);
    let (catalog, map, map_status) = load_map(&config, base, &snapshot, hash);
    for line in &map_status {
        super::log(line);
    }
    let catalog = Arc::new(catalog);
    let services = services(&config, root, &catalog, map.as_ref(), base)?;
    let sandbox = Arc::new(Sandbox::new()?);
    let mut host = Host::new(
        sandbox.clone(),
        config.limits(),
        services,
        directories(&config, root),
        Arc::new(|line: &str| super::log(line)),
        Box::new(|folder: &Path| super::open_folder(folder)),
    );
    let (trust, problem) = trust(&config, root);
    if let Some(problem) = problem {
        host.diagnostic(problem);
    }
    let installer = Arc::new(crate::plugin_installer::PluginInstaller::new(
        runtime_path
            .parent()
            .ok_or("runtime has no directory")?
            .join("plugin-installer.exe"),
        RELEASE_KEY,
        root.to_path_buf(),
        Arc::new(|line: &str| super::log(line)),
    ));
    host.set_plugin_installer(installer.clone());
    if diagnostics.updates {
        host.set_self_update(self_update(&config, root));
        if let Some(source) = catalog_source() {
            host.set_catalog(source);
        }
    }
    // Without the program, only what the player asks for reports that.
    installer.start(
        (diagnostics.updates && installer.program_exists())
            .then_some(crate::plugin_installer::UPDATE_INTERVAL),
    );
    if diagnostics.plugins {
        discover_plugins(&config, &sandbox, &trust, &mut host);
    }
    let map_data = MapData {
        directory: config.map_directory.clone(),
        seen: None,
        sheets: host.services().sheets.clone(),
        state: host.services().state.clone(),
        ui: host.services().ui.clone(),
    };
    if diagnostics.plugins {
        host.watch(Box::new(FolderPackages::new(
            config.plugin_directory.clone(),
            sandbox.clone(),
            trust,
            installer,
            map_data,
        )));
    }

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
    let overlay = install_overlay(&host, map_status, diagnostics);
    let _ = STATE.set(State {
        host,
        overlay: Mutex::new(overlay),
    });
    Ok(())
}

/// The host's services over the game loaded at `base`.
fn services(
    config: &Config,
    root: &Path,
    catalog: &Arc<Catalog>,
    map: Option<&angelica_map::Map>,
    base: usize,
) -> Result<Arc<Services>, String> {
    let excel = Arc::new(ExcelText::default());

    let case = Arc::new(TextCase::default());
    let files = Arc::new(crate::file_replacements::FileReplacements::new(
        &config
            .cache_directory
            .clone()
            .unwrap_or_else(|| root.join("cache"))
            .join("files"),
    ));
    let commands = Arc::new(crate::commands::Commands::default());
    let events = Arc::new(crate::events::Events::default());
    let executable = module_path(base)?;
    let game = Arc::new(WindowsGame {
        catalog: catalog.clone(),
        base,
        excel: excel.clone(),
        case: case.clone(),
        files: files.clone(),
        commands: commands.clone(),
        events: events.clone(),
        version: WindowsGame::read_version(&executable),
    });
    let (sheets, state, ui) = map_services(map, catalog, base);
    Ok(Arc::new(Services {
        // The executable is in `game/`; the archives are in `game/sqpack`.
        data: Arc::new(crate::game_data::GameData::new(
            executable.parent().and_then(Path::parent),
        )),
        files,
        excel,
        case,
        commands,
        chat: Arc::new(crate::chat::Chat::default()),
        sheets,
        state,
        events,
        ui,
        game,
        starting: AtomicBool::new(false),
        russian: AtomicBool::new(false),
    }))
}

/// The sheet schemas, state tables, and window schemas of the map, resolved
/// for the game loaded at `base`; none without a map.
fn map_services(
    map: Option<&angelica_map::Map>,
    catalog: &Catalog,
    base: usize,
) -> (
    Arc<crate::sheets::Sheets>,
    Arc<crate::state::StateTables>,
    Arc<crate::game_ui::GameUi>,
) {
    let sheets = crate::sheets::Sheets::new(map.iter().flat_map(|map| map.sheets().cloned()));
    let state = map.map_or_else(Default::default, |map| {
        crate::state::StateTables::from_map(map, catalog, base as u64)
    });
    let ui = crate::game_ui::GameUi::new(map.iter().flat_map(|map| map.windows().cloned()));
    (Arc::new(sheets), Arc::new(state), Arc::new(ui))
}

/// The signed map resolved for this executable, the map, and the lines the
/// menu shows about it. Without a usable map every entry is absent and no
/// sheet or state table is offered.
fn load_map(
    config: &Config,
    base: usize,
    snapshot: &[u8],
    hash: String,
) -> (Catalog, Option<angelica_map::Map>, Vec<String>) {
    let mut map_status = Vec::new();
    let mut loaded = None;
    let catalog = match FileSet::load(&config.map_directory, RELEASE_KEY.as_ref())
        .map_err(|e| e.to_string())
        .and_then(|files| Catalog::from_signed_files(&files, snapshot, base as u64, hash.clone()))
    {
        Ok((map, catalog)) => {
            loaded = Some(map);
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
    (catalog, loaded, map_status)
}

/// An authenticated package from its folder in the plugin directory.
/// A plugin from its author whose compiled component this runtime cannot
/// load is added to `repairs`, to be compiled again outside the game.
fn load_package(
    folder: &Path,
    sandbox: &Sandbox,
    trust: &Trust,
    repairs: &mut Vec<String>,
) -> Result<(Manifest, wasmtime::component::Component), String> {
    let package = crate::package::authenticate_plugin(folder, trust)?;
    // SAFETY: authenticate_plugin verified every byte with the release
    // signing key, whose packages hold compiler artifacts only from the
    // trusted build pipeline, or with the installation key, which signs only
    // artifacts the Angelica installer compiled on this computer from the
    // author's component. Native code never comes from a plugin author.
    let component =
        unsafe { super::component::load_authenticated(&sandbox.engine, &package.artifact) }
            .inspect_err(|_| {
                if package.compiled_here {
                    repairs.push(package.manifest.id().to_owned());
                }
            })?;
    Ok((package.manifest, component))
}

/// The keys plugin packages may be signed with; without a readable
/// installation key, only release packages load.
fn trust(config: &Config, root: &Path) -> (Trust, Option<String>) {
    let keys = config
        .keys_directory
        .clone()
        .unwrap_or_else(|| root.join("keys"));
    let (installation, problem) = match crate::author::installation_key(&keys) {
        Ok(key) => (key, None),
        Err(error) => (None, Some(error)),
    };
    (
        Trust {
            release: RELEASE_KEY,
            installation,
            keys,
        },
        problem,
    )
}

/// The folders of the plugin directory.
fn package_folders(directory: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let listing = match std::fs::read_dir(directory) {
        Ok(listing) => listing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("plugin directory: {e}")),
    };
    let mut folders = Vec::new();
    for item in listing {
        let item = item.map_err(|e| e.to_string())?;
        if item.file_type().map_err(|e| e.to_string())?.is_dir() {
            folders.push(item.path());
        }
    }
    folders.sort();
    Ok(folders)
}

fn discover_plugins(config: &Config, sandbox: &Sandbox, trust: &Trust, host: &mut Host) {
    let mut repairs = Vec::new();
    let folders = match package_folders(&config.plugin_directory) {
        Ok(folders) => folders,
        Err(error) => {
            host.diagnostic(error);
            return;
        }
    };
    for folder in folders {
        let result = load_package(&folder, sandbox, trust, &mut repairs)
            .and_then(|(manifest, component)| host.install(manifest, &component));
        if let Err(error) = result {
            host.diagnostic(format!("{}: {error}", folder.display()));
        }
    }
    if let Some(installer) = host.plugin_installer()
        && !repairs.is_empty()
    {
        installer.repair(repairs);
    }
}

/// The plugin directory's packages and the game map, reloaded when their
/// signed file list changes.
struct FolderPackages {
    directory: std::path::PathBuf,
    sandbox: Arc<Sandbox>,
    trust: Trust,
    installer: Arc<crate::plugin_installer::PluginInstaller>,
    /// The signed file list of every package folder, as last seen.
    seen: std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    checked: std::time::Instant,
    map: MapData,
}

/// The game map's data the host uses without hooks: sheet schemas, state
/// tables, and window schemas, which a reloaded map replaces.
struct MapData {
    directory: std::path::PathBuf,
    seen: Option<Vec<u8>>,
    sheets: Arc<crate::sheets::Sheets>,
    state: Arc<crate::state::StateTables>,
    ui: Arc<crate::game_ui::GameUi>,
}

impl MapData {
    fn changed(&mut self) -> bool {
        let list = std::fs::read(self.directory.join("files.json")).ok();
        if list.is_none() || list == self.seen {
            return false;
        }
        self.seen = list;
        true
    }

    fn reload(&self) -> Result<String, String> {
        let (base, snapshot, hash) = executable_snapshot()?;
        let files =
            FileSet::load(&self.directory, RELEASE_KEY.as_ref()).map_err(|e| e.to_string())?;
        let (map, catalog) = Catalog::from_signed_files(&files, &snapshot, base as u64, hash)?;
        self.sheets.set(map.sheets().cloned());
        self.state.replace_with(crate::state::StateTables::from_map(
            &map,
            &catalog,
            base as u64,
        ));
        self.ui.set(map.windows().cloned());
        Ok(format!(
            "game map reloaded: {} sheet schemas, {} state tables, and {} window schemas; entries that hooks use change at the next game start",
            map.sheets().count(),
            map.state_tables().count(),
            map.windows().count()
        ))
    }
}

impl FolderPackages {
    fn new(
        directory: std::path::PathBuf,
        sandbox: Arc<Sandbox>,
        trust: Trust,
        installer: Arc<crate::plugin_installer::PluginInstaller>,
        mut map: MapData,
    ) -> Self {
        // The map and the packages present now were loaded at start.
        map.changed();
        let mut packages = Self {
            directory,
            sandbox,
            trust,
            installer,
            seen: std::collections::BTreeMap::new(),
            checked: std::time::Instant::now(),
            map,
        };
        let _ = packages.look();
        packages
    }

    /// The folders whose signed file list changed since the last look.
    fn look(&mut self) -> Vec<std::path::PathBuf> {
        let mut changed = Vec::new();
        let folders = package_folders(&self.directory).unwrap_or_default();
        // A removed package is forgotten, so the same package installed
        // again loads.
        self.seen.retain(|folder, _| folders.contains(folder));
        for folder in folders {
            // The installer writes the file list last; a package without one
            // is still being written.
            let Ok(list) = std::fs::read(folder.join("files.json")) else {
                continue;
            };
            if self.seen.get(&folder) != Some(&list) {
                self.seen.insert(folder.clone(), list);
                changed.push(folder);
            }
        }
        changed
    }
}

impl crate::host::Updates for FolderPackages {
    fn map(&mut self) -> Option<Result<String, String>> {
        if self.checked.elapsed() < std::time::Duration::from_secs(1) {
            return None;
        }
        self.map.changed().then(|| self.map.reload())
    }

    fn packages(&mut self) -> Vec<Result<(Manifest, wasmtime::component::Component), String>> {
        if self.checked.elapsed() < std::time::Duration::from_secs(1) {
            return Vec::new();
        }
        self.checked = std::time::Instant::now();
        // The installer creates the installation's key with the first plugin
        // it installs, which may be after the game started; a key read at
        // start is never replaced.
        if self.trust.installation.is_none()
            && let Ok(Some(key)) = crate::author::installation_key(&self.trust.keys)
        {
            self.trust.installation = Some(key);
        }
        let mut repairs = Vec::new();
        let packages = self
            .look()
            .into_iter()
            .map(|folder| {
                load_package(&folder, &self.sandbox, &self.trust, &mut repairs)
                    .map_err(|error| format!("{}: {error}", folder.display()))
            })
            .collect();
        if !repairs.is_empty() {
            self.installer.repair(repairs);
        }
        packages
    }
}

/// Where the host keeps plugins and what the player chose for them.
fn directories(config: &Config, root: &Path) -> crate::host::Directories {
    crate::host::Directories {
        plugins: config.plugin_directory.clone(),
        grants: config.grants_directory.clone(),
        state: config
            .state_directory
            .clone()
            .unwrap_or_else(|| root.join("state")),
        storage: config.storage_directory.clone(),
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
