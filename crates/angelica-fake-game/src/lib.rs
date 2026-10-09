//! A scripted game: the host's services and plugins run as in the game
//! process, and test code plays the game's part. Rows go through the same
//! [`SheetBinding`] the Windows adapter uses; letter case through the same
//! [`Letters`].
//!
//! [`SheetBinding`]: runtime::excel_text::SheetBinding
//! [`Letters`]: runtime::text_case::Letters
pub use angelica_sqpack::testing as game_files;
use angelica_text::table::Column;
use runtime::{
    excel_text::{ExcelText, Rewrite, SheetInfo},
    game::Game,
    host::{Directories, Event, Host},
    manifest::Manifest,
    sandbox::{Limits, Sandbox, Services},
    text_case::TextCase,
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

/// The game as the host sees it. Every entry is available unless a test
/// says otherwise.
pub struct ScriptedGame {
    pub version: String,
    pub excel_available: AtomicBool,
    pub case_available: AtomicBool,
    pub files_available: AtomicBool,
}

impl Default for ScriptedGame {
    fn default() -> Self {
        Self {
            version: "2026.09.15.0000.0000".into(),
            excel_available: AtomicBool::new(true),
            case_available: AtomicBool::new(true),
            files_available: AtomicBool::new(true),
        }
    }
}

impl Game for ScriptedGame {
    fn version(&self) -> String {
        self.version.clone()
    }

    fn enable_excel_text(&self) -> Result<(), String> {
        if self.excel_available.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("map entry excel.hash_table_page_row_resolver.store_row is absent".into())
        }
    }

    fn enable_file_replacements(&self) -> Result<(), String> {
        if self.files_available.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("map entry file.file_thread.do_job is absent".into())
        }
    }

    fn enable_text_case(&self) -> Result<(), String> {
        if self.case_available.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("map entry text.utf8_string.to_upper is absent".into())
        }
    }
}

/// A version 3 row buffer: `fixed_size` bytes with a text field at each
/// column's offset, then the strings in column order.
///
/// # Panics
/// When a column's field does not fit the fixed part.
#[must_use]
pub fn row_buffer(fixed_size: usize, columns: &[Column], strings: &[&[u8]]) -> Vec<u8> {
    let mut buffer = vec![0; fixed_size];
    for (column, text) in columns.iter().zip(strings) {
        let field = usize::from(column.offset);
        let distance = u32::try_from(buffer.len() - field).expect("small row");
        let header = distance | (u32::from(runtime::excel_text::string_hash(text)) << 24);
        buffer[field..field + 4].copy_from_slice(&header.to_le_bytes());
        buffer.extend_from_slice(text);
        buffer.push(0);
    }
    buffer
}

/// The strings of a row buffer, in column order.
#[must_use]
pub fn row_strings(buffer: &[u8], fixed_size: usize) -> Vec<Vec<u8>> {
    buffer[fixed_size..]
        .split(|&b| b == 0)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>()
        .split_last()
        .map(|(_, strings)| strings.to_vec())
        .unwrap_or_default()
}

/// The game builds a row of `sheet`: the buffer the row ends up with, as
/// the Windows `StoreRow` detour would leave it.
#[must_use]
pub fn store_row(
    text: &ExcelText,
    sheet: &SheetInfo,
    row: u32,
    subrow: u16,
    buffer: Vec<u8>,
    fixed_size: usize,
) -> Vec<u8> {
    let binding = text.bind(sheet);
    if binding.is_empty() || !binding.lists(row, subrow) {
        return buffer;
    }
    match binding.rewrite(row, subrow, &buffer, fixed_size) {
        Rewrite::Changed(rebuilt) => rebuilt,
        Rewrite::Unchanged | Rewrite::Refused => buffer,
    }
}

/// A host with its folders in a temporary directory, on the calling thread.
pub struct TestHost {
    pub host: Host,
    pub game: Arc<ScriptedGame>,
    pub folder: tempfile::TempDir,
    pub opened: Arc<Mutex<Vec<PathBuf>>>,
}

impl TestHost {
    /// # Panics
    /// When the sandbox cannot be created.
    #[must_use]
    pub fn new(limits: Limits) -> Self {
        Self::in_folder(tempfile::tempdir().expect("temporary folder"), limits)
    }

    /// A host over the folders of an earlier one, as at the next game start.
    ///
    /// # Panics
    /// When the sandbox cannot be created.
    #[must_use]
    pub fn in_folder(folder: tempfile::TempDir, limits: Limits) -> Self {
        let game = Arc::new(ScriptedGame::default());
        let services = Arc::new(Services {
            data: Arc::new(runtime::game_data::GameData::new(Some(
                &folder.path().join("game-files"),
            ))),
            files: Arc::new(runtime::file_replacements::FileReplacements::new(
                &folder.path().join("cache"),
            )),
            excel: Arc::new(ExcelText::default()),
            case: Arc::new(TextCase::default()),
            game: game.clone(),
            starting: AtomicBool::new(false),
        });
        let opened = Arc::new(Mutex::new(Vec::new()));
        let recorded = opened.clone();
        let root = folder.path();
        let host = Host::new(
            Arc::new(Sandbox::new().expect("sandbox")),
            limits,
            services,
            Directories {
                grants: root.join("grants"),
                state: root.join("state"),
                storage: root.join("storage"),
            },
            Arc::new(|_: &str| {}),
            Box::new(move |path: &Path| {
                recorded
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(path.into());
            }),
        );
        Self {
            host,
            game,
            folder,
            opened,
        }
    }

    /// Installs a component built from a package's `angelica.toml` and
    /// component bytes.
    ///
    /// # Errors
    /// When the manifest or component is refused.
    pub fn install(&mut self, manifest: &str, component: &[u8]) -> Result<(), String> {
        let manifest = Manifest::parse(manifest).map_err(|e| e.to_string())?;
        let component = wasmtime::component::Component::new(&self.host_engine(), component)
            .map_err(|e| e.to_string())?;
        self.host.install(manifest, &component)
    }

    fn host_engine(&self) -> wasmtime::Engine {
        self.host.engine().clone()
    }

    /// Runs `count` frames of `elapsed` seconds each.
    pub fn frames(&self, count: usize) {
        for _ in 0..count {
            self.host.frame(1.0 / 60.0);
        }
    }

    pub fn event(&self, plugin: &str, event: Event) {
        self.host.queue_event(plugin, event);
        self.host.frame(1.0 / 60.0);
    }

    /// The folder whose `game/sqpack` the host reads game files from; write
    /// a synthetic installation there with [`game_files::FakeGame`] before
    /// a plugin first reads.
    #[must_use]
    pub fn game_folder(&self) -> PathBuf {
        self.folder.path().join("game-files")
    }

    #[must_use]
    pub fn services(&self) -> &Arc<Services> {
        self.host.services()
    }
}

/// Builds a package of this workspace for `wasm32-wasip2` and returns the
/// component.
///
/// # Panics
/// When the build fails.
#[must_use]
pub fn build_component(package: &str) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    build_component_in(root, package)
}

/// Builds `package` of the Cargo workspace at `root` for `wasm32-wasip2`
/// and returns the component.
///
/// # Panics
/// When the build fails.
#[must_use]
pub fn build_component_in(root: &Path, package: &str) -> Vec<u8> {
    let target = root.join("target/component-tests");
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args([
            "build",
            "--offline",
            "-p",
            package,
            "--target",
            "wasm32-wasip2",
            "--release",
            "--target-dir",
        ])
        .arg(&target)
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::read(target.join(format!(
        "wasm32-wasip2/release/{}.wasm",
        package.replace('-', "_")
    )))
    .expect("component")
}
