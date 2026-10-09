//! Plugin instances: the wasmtime engine, the imports the host provides, and
//! the time, fuel, and memory each call may use.
#![forbid(unsafe_code)]
use crate::{
    error::{Error, Kind},
    excel_text::ExcelText,
    jobs::{JobState, Jobs, Slot},
    manifest::Permission,
    storage::Storage,
    text_case::TextCase,
};
use std::{
    collections::{BTreeSet, VecDeque},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use wasmtime::{
    Engine, Store, StoreLimits, StoreLimitsBuilder, UpdateDeadline,
    component::{Component, HasSelf, Linker},
};

pub mod bindings {
    wasmtime::component::bindgen!({ path: "../../wit", world: "plugin" });
}
use bindings::angelica::plugin::{self as api, types::ErrorKind};
pub use bindings::{Plugin as Bindings, PluginPre};

/// Limits of plugin calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Memory of an instance when the manifest requests none.
    pub memory_bytes: u64,
    /// Guest instructions, in fuel units, per frame callback.
    pub fuel: u64,
    /// Guest time per frame callback; time spent in host calls is not
    /// counted.
    pub call_time: Duration,
    /// Guest time for `start` and `stop`.
    pub start_time: Duration,
    /// Guest time of a job.
    pub job_time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024,
            fuel: 100_000_000,
            call_time: Duration::from_millis(20),
            start_time: Duration::from_secs(2),
            job_time: Duration::from_secs(600),
        }
    }
}

/// The wasmtime engine and the thread that advances its epoch every
/// millisecond.
pub struct Sandbox {
    pub engine: Engine,
    stopped: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

impl Sandbox {
    /// # Errors
    /// Fails when the engine or its epoch thread cannot be created.
    pub fn new() -> Result<Self, String> {
        let mut config = wasmtime::Config::new();
        config
            .wasm_component_model(true)
            .consume_fuel(true)
            .epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|e| e.to_string())?;
        let stopped = Arc::new(AtomicBool::new(false));
        let ticker = {
            let engine = engine.clone();
            let stopped = stopped.clone();
            std::thread::Builder::new()
                .name("angelica-epoch".into())
                .spawn(move || {
                    while !stopped.load(Ordering::Acquire) {
                        std::thread::sleep(Duration::from_millis(1));
                        engine.increment_epoch();
                    }
                })
                .map_err(|e| e.to_string())?
        };
        Ok(Self {
            engine,
            stopped,
            ticker: Some(ticker),
        })
    }

    /// Links the host's imports for `component` and checks that it imports
    /// nothing else. Of the WASI imports Rust's standard library adds,
    /// random numbers and clocks work (`crate::wasi`); the others trap.
    ///
    /// # Errors
    /// Names an import the host does not provide.
    pub fn prepare(&self, component: &Component) -> Result<PluginPre<State>, String> {
        let mut names = Vec::new();
        for (name, _) in component.component_type().imports(&self.engine) {
            let ours = name.starts_with("angelica:plugin/") && name.ends_with("@1.0.0");
            if !ours && !name.starts_with("wasi:") {
                return Err(format!(
                    "the plugin imports {name}, which this host does not provide"
                ));
            }
            names.push(name.to_owned());
        }
        let mut linker = Linker::<State>::new(&self.engine);
        Bindings::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)
            .map_err(|e| e.to_string())?;
        crate::wasi::define(&mut linker, &names).map_err(|e| format!("{e:#}"))?;
        linker
            .define_unknown_imports_as_traps(component)
            .map_err(|e| e.to_string())?;
        let pre = linker
            .instantiate_pre(component)
            .map_err(|e| e.to_string())?;
        PluginPre::new(pre).map_err(|e| e.to_string())
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }
    }
}

/// The game-facing services every plugin shares.
pub struct Services {
    pub data: Arc<crate::game_data::GameData>,
    pub excel: Arc<ExcelText>,
    pub case: Arc<TextCase>,
    pub game: Arc<dyn crate::game::Game>,
    /// True while the host starts plugins before the game's code runs.
    pub starting: AtomicBool,
}

/// One log line of a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub level: api::runtime::Level,
    pub text: String,
}

/// The lines a plugin logged most recently.
pub struct Log {
    lines: Mutex<VecDeque<LogLine>>,
    sink: Box<dyn Fn(&str) + Send + Sync>,
}

impl Log {
    pub const KEPT: usize = 200;
    pub const MAX_LINE: usize = 4096;

    #[must_use]
    pub fn new(sink: Box<dyn Fn(&str) + Send + Sync>) -> Self {
        Self {
            lines: Mutex::new(VecDeque::new()),
            sink,
        }
    }

    pub fn push(&self, level: api::runtime::Level, text: &str) {
        let mut text = text.to_owned();
        if text.len() > Self::MAX_LINE {
            let mut end = Self::MAX_LINE;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
        }
        (self.sink)(&text);
        let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == Self::KEPT {
            lines.pop_front();
        }
        lines.push_back(LogLine { level, text });
    }

    #[must_use]
    pub fn lines(&self) -> Vec<LogLine> {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

/// What every instance of one plugin shares.
pub struct Context {
    pub id: String,
    /// Permissions the manifest declares and the player granted.
    pub permissions: BTreeSet<Permission>,
    pub memory_bytes: u64,
    pub storage: Arc<Storage>,
    pub services: Arc<Services>,
    pub log: Arc<Log>,
    pub jobs: Arc<Jobs>,
}

/// Which instance a store belongs to.
pub enum Role {
    Main,
    Job(Arc<Slot>),
}

/// The time a call may still use. Host calls are not charged.
struct Budget {
    started: Instant,
    host: Duration,
    limit: Duration,
}

/// The data of one instance's store.
pub struct State {
    pub(crate) context: Arc<Context>,
    pub(crate) role: Role,
    limits: StoreLimits,
    budget: Budget,
}

/// A store with its instance's state.
#[must_use]
pub fn store(sandbox: &Sandbox, context: Arc<Context>, role: Role) -> Store<State> {
    let memory = usize::try_from(context.memory_bytes).unwrap_or(usize::MAX);
    let limits = StoreLimitsBuilder::new()
        .memory_size(memory)
        .instances(64)
        .tables(64)
        .memories(16)
        .build();
    let mut store = Store::new(
        &sandbox.engine,
        State {
            context,
            role,
            limits,
            budget: Budget {
                started: Instant::now(),
                host: Duration::ZERO,
                limit: Duration::ZERO,
            },
        },
    );
    store.limiter(|state| &mut state.limits);
    store.epoch_deadline_callback(|context| {
        let state = context.data();
        let cancelled = matches!(&state.role, Role::Job(slot) if slot.cancelled());
        let guest = state
            .budget
            .started
            .elapsed()
            .saturating_sub(state.budget.host);
        if cancelled || guest > state.budget.limit {
            Ok(UpdateDeadline::Interrupt)
        } else {
            Ok(UpdateDeadline::Continue(1))
        }
    });
    store
}

/// Gives the next call `time` of guest time and `fuel` instructions.
pub fn budget(store: &mut Store<State>, time: Duration, fuel: u64) {
    let state = store.data_mut();
    state.budget = Budget {
        started: Instant::now(),
        host: Duration::ZERO,
        limit: time,
    };
    store.set_epoch_deadline(1);
    let _ = store.set_fuel(fuel);
}

impl State {
    /// Runs a host operation without charging its time to the guest.
    fn host<T>(&mut self, work: impl FnOnce(&mut Self) -> T) -> T {
        let started = Instant::now();
        let result = work(self);
        self.budget.host += started.elapsed();
        result
    }

    fn main_only(&self) -> crate::error::Result<()> {
        match self.role {
            Role::Main => Ok(()),
            Role::Job(_) => Err(Error::new(
                Kind::WrongContext,
                "only the main instance can do this",
            )),
        }
    }

    fn permitted(&self, permission: Permission) -> crate::error::Result<()> {
        if self.context.permissions.contains(&permission) {
            Ok(())
        } else {
            Err(Error::new(
                Kind::PermissionDenied,
                format!("the plugin does not hold {}", permission.name()),
            ))
        }
    }
}

impl From<Error> for api::types::Error {
    fn from(error: Error) -> Self {
        Self {
            kind: match error.kind {
                Kind::InvalidArgument => ErrorKind::InvalidArgument,
                Kind::NotFound => ErrorKind::NotFound,
                Kind::PermissionDenied => ErrorKind::PermissionDenied,
                Kind::WrongContext => ErrorKind::WrongContext,
                Kind::Unavailable => ErrorKind::Unavailable,
                Kind::LimitExceeded => ErrorKind::LimitExceeded,
                Kind::Io => ErrorKind::Io,
            },
            message: error.message,
        }
    }
}

type ApiResult<T> = Result<T, api::types::Error>;

fn api<T>(result: crate::error::Result<T>) -> ApiResult<T> {
    result.map_err(Into::into)
}

impl api::types::Host for State {}
impl api::ui::Host for State {}

impl api::runtime::Host for State {
    fn log(&mut self, level: api::runtime::Level, message: String) {
        self.host(|state| state.context.log.push(level, &message));
    }

    fn started_with_game(&mut self) -> bool {
        self.context.services.starting.load(Ordering::Acquire)
    }

    fn game_version(&mut self) -> String {
        self.context.services.game.version()
    }
}

impl api::storage::Host for State {
    fn read(&mut self, path: String) -> ApiResult<Vec<u8>> {
        self.host(|state| api(state.context.storage.read(&path)))
    }
    fn read_range(&mut self, path: String, offset: u64, length: u32) -> ApiResult<Vec<u8>> {
        self.host(|state| api(state.context.storage.read_range(&path, offset, length)))
    }
    fn size(&mut self, path: String) -> ApiResult<u64> {
        self.host(|state| api(state.context.storage.size(&path)))
    }
    fn list(&mut self, path: String) -> ApiResult<Vec<api::storage::Entry>> {
        self.host(|state| {
            api(state.context.storage.list(&path)).map(|entries| {
                entries
                    .into_iter()
                    .map(|e| api::storage::Entry {
                        name: e.name,
                        directory: e.directory,
                        size: e.size,
                    })
                    .collect()
            })
        })
    }
    fn write(&mut self, path: String, contents: Vec<u8>) -> ApiResult<()> {
        self.host(|state| api(state.context.storage.write(&path, &contents)))
    }
    fn append(&mut self, path: String, contents: Vec<u8>) -> ApiResult<()> {
        self.host(|state| api(state.context.storage.append(&path, &contents)))
    }
    fn rename(&mut self, from: String, to: String) -> ApiResult<()> {
        self.host(|state| api(state.context.storage.rename(&from, &to)))
    }
    fn remove(&mut self, path: String) -> ApiResult<()> {
        self.host(|state| api(state.context.storage.remove(&path)))
    }
}

impl api::jobs::Host for State {
    fn start(&mut self, name: String, input: Vec<u8>) -> ApiResult<u64> {
        api(self.main_only())?;
        self.host(|state| api(state.context.jobs.start(&state.context, &name, input)))
    }
    fn poll(&mut self, id: u64) -> ApiResult<api::jobs::State> {
        api(self.main_only())?;
        let state = api(self.context.jobs.poll(id))?;
        Ok(match state {
            JobState::Running(fraction, message) => {
                api::jobs::State::Running(api::jobs::JobProgress { fraction, message })
            }
            JobState::Finished(output) => api::jobs::State::Finished(output),
            JobState::Failed(reason) => api::jobs::State::Failed(reason),
        })
    }
    fn cancel(&mut self, id: u64) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.context.jobs.cancel(id))
    }
    fn report(&mut self, fraction: f32, message: String) -> ApiResult<()> {
        match &self.role {
            Role::Job(slot) => {
                slot.report(fraction, message);
                Ok(())
            }
            Role::Main => Err(Error::new(Kind::WrongContext, "report is called from a job").into()),
        }
    }
}

impl api::game_files::Host for State {
    fn exists(&mut self, path: String) -> ApiResult<bool> {
        self.host(|state| api(state.context.services.data.exists(&path)))
    }
    fn read(&mut self, path: String) -> ApiResult<Vec<u8>> {
        self.host(|state| api(state.context.services.data.read(&path)))
    }
}

fn sheet_row(row: crate::game_data::Row) -> api::excel::SheetRow {
    use crate::game_data::{Cell, Value as V};
    use api::excel::Value;
    api::excel::SheetRow {
        key: api::excel::RowKey {
            row: row.row,
            subrow: row.subrow,
        },
        values: row
            .cells
            .into_iter()
            .map(|cell| match cell {
                Cell::Text(bytes) => Value::Text(bytes),
                Cell::Value(V::Boolean(value)) => Value::Boolean(value),
                Cell::Value(V::Signed(value)) => Value::Signed(value),
                Cell::Value(V::Unsigned(value)) => Value::Unsigned(value),
                Cell::Value(V::Float(value)) => Value::Float(value),
            })
            .collect(),
    }
}

impl api::excel::Host for State {
    fn sheets(&mut self) -> ApiResult<Vec<String>> {
        self.host(|state| api(state.context.services.data.sheets()))
    }

    fn sheet(&mut self, name: String) -> ApiResult<api::excel::SheetInfo> {
        use angelica_sqpack::excel::ColumnKind as K;
        use api::excel::ColumnKind;
        let info = self.host(|state| api(state.context.services.data.sheet(&name)))?;
        Ok(api::excel::SheetInfo {
            name: info.name,
            variant: if info.subrows {
                api::excel::SheetVariant::Subrows
            } else {
                api::excel::SheetVariant::Rows
            },
            columns: info
                .columns
                .iter()
                .enumerate()
                .map(|(index, &(kind, offset))| api::excel::Column {
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    kind: match kind {
                        K::String => ColumnKind::Text,
                        K::Bool => ColumnKind::Boolean,
                        K::Int8 => ColumnKind::Int8,
                        K::UInt8 => ColumnKind::Uint8,
                        K::Int16 => ColumnKind::Int16,
                        K::UInt16 => ColumnKind::Uint16,
                        K::Int32 => ColumnKind::Int32,
                        K::UInt32 => ColumnKind::Uint32,
                        K::Float32 => ColumnKind::Float32,
                        K::Int64 => ColumnKind::Int64,
                        K::UInt64 => ColumnKind::Uint64,
                        K::PackedBool(bit) => ColumnKind::PackedBoolean(bit),
                    },
                    offset,
                })
                .collect(),
            languages: info.languages,
            rows: info.rows,
        })
    }

    fn rows(
        &mut self,
        name: String,
        language: u8,
        after: Option<api::excel::RowKey>,
        limit: u32,
    ) -> ApiResult<Vec<api::excel::SheetRow>> {
        let after = after.map(|key| (key.row, key.subrow));
        let rows = self.host(|state| {
            api(state
                .context
                .services
                .data
                .rows(&name, language, after, limit))
        })?;
        Ok(rows.into_iter().map(sheet_row).collect())
    }

    fn row(
        &mut self,
        name: String,
        language: u8,
        key: api::excel::RowKey,
    ) -> ApiResult<Option<api::excel::SheetRow>> {
        let row = self.host(|state| {
            api(state
                .context
                .services
                .data
                .row(&name, language, (key.row, key.subrow)))
        })?;
        Ok(row.map(sheet_row))
    }
}

/// Table names are 1 to 64 ASCII letters, digits, hyphens, and underscores.
fn valid_table_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl api::excel_text::Host for State {
    fn activate(
        &mut self,
        name: String,
        path: String,
        priority: i32,
    ) -> ApiResult<api::excel_text::TableInfo> {
        api(self.main_only())?;
        api(self.permitted(Permission::ExcelText))?;
        if !valid_table_name(&name) {
            return Err(Error::invalid(format!("{name:?} is not a valid table name")).into());
        }
        self.host(|state| {
            let services = &state.context.services;
            services
                .game
                .enable_excel_text()
                .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
            let bytes = state.context.storage.read_unbounded(&path)?;
            let info = services
                .excel
                .activate(&state.context.id, &name, priority, bytes)
                .map_err(|reason| Error::invalid(format!("{path}: {reason}")))?;
            Ok(api::excel_text::TableInfo {
                sheets: info.sheets,
                cells: info.cells,
            })
        })
        .map_err(|error: Error| error.into())
    }

    fn deactivate(&mut self, name: String) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::ExcelText))?;
        if self
            .context
            .services
            .excel
            .deactivate(&self.context.id, &name)
        {
            Ok(())
        } else {
            Err(Error::new(Kind::NotFound, format!("no active table {name}")).into())
        }
    }

    fn status(&mut self, name: String) -> ApiResult<api::excel_text::TableStatus> {
        api(self.main_only())?;
        api(self.permitted(Permission::ExcelText))?;
        let status = self
            .context
            .services
            .excel
            .status(&self.context.id, &name)
            .ok_or_else(|| Error::new(Kind::NotFound, format!("no active table {name}")))?;
        Ok(api::excel_text::TableStatus {
            rows_changed: status.rows_changed,
            cells_replaced: status.cells_replaced,
            guard_mismatches: status.guard_mismatches,
            rows_refused: status.rows_refused,
            sheets_mismatched: status
                .sheets_mismatched
                .into_iter()
                .map(|(sheet, reason)| api::excel_text::SheetMismatch {
                    sheet,
                    reason: match reason {
                        crate::excel_text::Mismatch::Layout => api::excel_text::Mismatch::Layout,
                        crate::excel_text::Mismatch::Variant => api::excel_text::Mismatch::Variant,
                        crate::excel_text::Mismatch::Language => {
                            api::excel_text::Mismatch::Language
                        }
                    },
                })
                .collect(),
        })
    }
}

impl api::text_case::Host for State {
    fn set_letters(&mut self, letters: Vec<api::text_case::Letter>) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::TextCase))?;
        self.host(|state| {
            let pairs: Vec<_> = letters.iter().map(|l| (l.lower, l.upper)).collect();
            if !pairs.is_empty() {
                state
                    .context
                    .services
                    .game
                    .enable_text_case()
                    .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
            }
            state
                .context
                .services
                .case
                .set(&state.context.id, &pairs)
                .map_err(Error::invalid)
        })
        .map_err(Into::into)
    }
}

/// Whether a trap came from running out of time or fuel rather than a bug
/// in the plugin.
#[must_use]
pub fn describe_trap(error: &wasmtime::Error) -> String {
    match error.downcast_ref::<wasmtime::Trap>() {
        Some(wasmtime::Trap::Interrupt) => "the call ran out of time".into(),
        Some(wasmtime::Trap::OutOfFuel) => "the call ran out of fuel".into(),
        _ => format!("{error:#}"),
    }
}
