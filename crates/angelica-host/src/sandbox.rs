//! Plugin instances: the wasmtime engine, the imports the host provides, and
//! the time, fuel, and memory each call may use.
#![forbid(unsafe_code)]
use crate::{
    chat::Chat,
    commands::Commands,
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
    /// Guest instructions, in fuel units, for the calls of one frame.
    pub fuel: u64,
    /// Guest time for the calls of one frame; time spent in host calls is
    /// not counted.
    pub call_time: Duration,
    /// Guest time for `start` and `stop`.
    pub start_time: Duration,
    /// Time for the calls of one frame including host calls, so host
    /// operations cannot hold the game's main thread without end.
    pub frame_wall: Duration,
    /// Time for `start` and `stop` including host calls.
    pub start_wall: Duration,
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
            frame_wall: Duration::from_secs(2),
            start_wall: Duration::from_secs(30),
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
    pub files: Arc<crate::file_replacements::FileReplacements>,
    pub excel: Arc<ExcelText>,
    pub case: Arc<TextCase>,
    pub commands: Arc<Commands>,
    pub chat: Arc<Chat>,
    /// Names of sheet columns from the game map.
    pub sheets: Arc<crate::sheets::Sheets>,
    /// The game map's object model for the running build, for Lens.
    pub lens: Arc<crate::lens::Lens>,
    /// Events for the next frame and the plugins' subscriptions.
    pub events: Arc<crate::events::Events>,
    /// Rules for the game's own windows.
    pub ui: Arc<crate::game_ui::GameUi>,
    /// What plugins draw over the game world.
    pub draw: Arc<crate::draw::Drawings>,
    /// Messages between plugins.
    pub messages: Arc<crate::messages::Messages>,
    /// Text plugins put on the clipboard this frame.
    pub clipboard: Arc<crate::clipboard::Clipboard>,
    pub game: Arc<dyn crate::game::Game>,
    /// True while the host starts plugins before the game's code runs.
    pub starting: AtomicBool,
    /// Whether Angelica's interface is in Russian; English otherwise.
    pub russian: AtomicBool,
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
    /// The plugin's name from its manifest.
    pub name: String,
    /// Permissions the manifest declares and the player granted.
    pub permissions: BTreeSet<Permission>,
    /// The domains the manifest's `[lens] read` lists and the player
    /// granted.
    pub lens_read: BTreeSet<String>,
    /// The domains it may act in through Lens.
    pub lens_act: BTreeSet<String>,
    /// The domains it may change through Lens.
    pub lens_change: BTreeSet<String>,
    /// The plugins it may send messages to.
    pub message_to: BTreeSet<String>,
    pub memory_bytes: u64,
    pub storage: Arc<Storage>,
    pub services: Arc<Services>,
    pub log: Arc<Log>,
    pub jobs: Arc<Jobs>,
    /// Requests to the approved hosts; `None` without `http.get`.
    pub http: Option<Arc<crate::http::Client>>,
}

/// Which instance a store belongs to.
pub enum Role {
    Main,
    Job(Arc<Slot>),
}

/// The time the calls of one budget may use: guest time, which excludes
/// host calls, and wall time, which includes them.
struct Budget {
    /// When the current call began.
    started: Instant,
    /// Host time within the current call.
    host: Duration,
    /// Guest time of the budget's earlier calls.
    guest_before: Duration,
    /// Wall time of the budget's earlier calls.
    wall_before: Duration,
    limit: Duration,
    wall: Duration,
}

impl Budget {
    fn new(limit: Duration, wall: Duration) -> Self {
        Self {
            started: Instant::now(),
            host: Duration::ZERO,
            guest_before: Duration::ZERO,
            wall_before: Duration::ZERO,
            limit,
            wall,
        }
    }

    fn guest(&self) -> Duration {
        self.guest_before + self.started.elapsed().saturating_sub(self.host)
    }

    fn wall(&self) -> Duration {
        self.wall_before + self.started.elapsed()
    }
}

/// Which limit interrupted a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exceeded {
    Guest,
    Wall,
}

/// The data of one instance's store.
pub struct State {
    pub(crate) context: Arc<Context>,
    pub(crate) role: Role,
    limits: StoreLimits,
    budget: Budget,
    exceeded: Option<Exceeded>,
    /// The Lens queries this instance prepared.
    queries: crate::lens::Queries,
    /// True while the instance handles an action of the player.
    player_action: bool,
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
            budget: Budget::new(Duration::ZERO, Duration::ZERO),
            exceeded: None,
            queries: crate::lens::Queries::default(),
            player_action: false,
        },
    );
    store.limiter(|state| &mut state.limits);
    store.epoch_deadline_callback(|mut context| {
        let state = context.data_mut();
        let cancelled = matches!(&state.role, Role::Job(slot) if slot.cancelled());
        if state.budget.guest() > state.budget.limit {
            state.exceeded = Some(Exceeded::Guest);
        } else if state.budget.wall() > state.budget.wall {
            state.exceeded = Some(Exceeded::Wall);
        }
        if cancelled || state.exceeded.is_some() {
            Ok(UpdateDeadline::Interrupt)
        } else {
            Ok(UpdateDeadline::Continue(1))
        }
    });
    store
}

/// Starts a budget for the next calls: `time` of guest time, `wall` of
/// time including host calls, and `fuel` instructions, shared by every call
/// until the next budget. Wrap each call in [`call`].
pub fn budget(store: &mut Store<State>, time: Duration, wall: Duration, fuel: u64) {
    let state = store.data_mut();
    state.budget = Budget::new(time, wall);
    state.exceeded = None;
    let _ = store.set_fuel(fuel);
}

/// Makes one call of the guest within the current budget.
pub fn call<T>(store: &mut Store<State>, work: impl FnOnce(&mut Store<State>) -> T) -> T {
    let budget = &mut store.data_mut().budget;
    budget.started = Instant::now();
    budget.host = Duration::ZERO;
    store.set_epoch_deadline(1);
    let result = work(store);
    let budget = &mut store.data_mut().budget;
    budget.guest_before = budget.guest();
    budget.wall_before = budget.wall();
    budget.started = Instant::now();
    budget.host = Duration::ZERO;
    result
}

/// [`call`] for a call that handles an action of the player: a
/// `ui-event` or a `command`.
pub fn call_for_player<T>(
    store: &mut Store<State>,
    work: impl FnOnce(&mut Store<State>) -> T,
) -> T {
    store.data_mut().player_action = true;
    let result = call(store, work);
    store.data_mut().player_action = false;
    result
}

/// The wall time the calls of the current budget took.
#[must_use]
pub fn spent(store: &Store<State>) -> Duration {
    store.data().budget.wall_before
}

impl State {
    #[must_use]
    pub fn context(&self) -> &Context {
        &self.context
    }

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

    /// The plugin's HTTP client and the job that calls it.
    fn http(&self) -> crate::error::Result<(Arc<crate::http::Client>, Arc<Slot>)> {
        let Role::Job(slot) = &self.role else {
            return Err(Error::new(
                Kind::WrongContext,
                "requests are made from jobs",
            ));
        };
        self.permitted(Permission::HttpGet)?;
        let client = self
            .context
            .http
            .clone()
            .ok_or_else(|| Error::new(Kind::PermissionDenied, "the plugin lists no hosts"))?;
        Ok((client, slot.clone()))
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
                Kind::Network => ErrorKind::Network,
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

    fn language(&mut self) -> String {
        if self.context.services.russian.load(Ordering::Acquire) {
            "ru"
        } else {
            "en"
        }
        .to_owned()
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

/// The values of the columns at `indexes` in a row.
fn sheet_row(row: &crate::game_data::Row, indexes: &[u16]) -> api::excel::SheetRow {
    use crate::game_data::{Cell, Value as V};
    use api::excel::Value;
    api::excel::SheetRow {
        key: api::excel::RowKey {
            row: row.row,
            subrow: row.subrow,
        },
        values: indexes
            .iter()
            .filter_map(|&index| row.cells.get(usize::from(index)))
            .map(|cell| match cell {
                Cell::Text(bytes) => Value::Text(bytes.clone()),
                Cell::Value(V::Boolean(value)) => Value::Boolean(*value),
                Cell::Value(V::Signed(value)) => Value::Signed(*value),
                Cell::Value(V::Unsigned(value)) => Value::Unsigned(*value),
                Cell::Value(V::Float(value)) => Value::Float(*value),
            })
            .collect(),
    }
}

fn column_kind(kind: angelica_map::ColumnType) -> api::excel::ColumnKind {
    use angelica_map::ColumnType as T;
    use api::excel::ColumnKind;
    match kind {
        T::Text => ColumnKind::Text,
        T::Bool => ColumnKind::Boolean,
        T::Int8 => ColumnKind::Int8,
        T::Uint8 => ColumnKind::Uint8,
        T::Int16 => ColumnKind::Int16,
        T::Uint16 => ColumnKind::Uint16,
        T::Int32 => ColumnKind::Int32,
        T::Uint32 => ColumnKind::Uint32,
        T::Float32 => ColumnKind::Float32,
        T::Int64 => ColumnKind::Int64,
        T::Uint64 => ColumnKind::Uint64,
        T::PackedBool(bit) => ColumnKind::PackedBoolean(bit),
    }
}

impl State {
    /// The indexes of `columns` in the running game's layout of `sheet`.
    fn column_indexes(&self, sheet: &str, columns: &[String]) -> crate::error::Result<Vec<u16>> {
        if columns.is_empty() {
            return Ok(Vec::new());
        }
        let services = &self.context.services;
        let fields = services.sheets.fields(sheet, &services.data)?;
        columns
            .iter()
            .map(|name| {
                fields
                    .iter()
                    .find(|field| field.name == *name)
                    .map(|field| field.index)
                    .ok_or_else(|| {
                        Error::new(
                            Kind::NotFound,
                            format!("the game map names no column {name} of sheet {sheet}"),
                        )
                    })
            })
            .collect()
    }
}

impl api::excel::Host for State {
    fn sheets(&mut self) -> ApiResult<Vec<String>> {
        self.host(|state| api(state.context.services.data.sheets()))
    }

    fn sheet(&mut self, name: String) -> ApiResult<api::excel::SheetInfo> {
        let (info, fields) = self.host(|state| {
            let services = &state.context.services;
            let info = api(services.data.sheet(&name))?;
            let fields = services
                .sheets
                .fields(&name, &services.data)
                .unwrap_or_default();
            Ok::<_, api::types::Error>((info, fields))
        })?;
        let layout = info.layout();
        Ok(api::excel::SheetInfo {
            name: info.name,
            variant: if info.subrows {
                api::excel::SheetVariant::Subrows
            } else {
                api::excel::SheetVariant::Rows
            },
            columns: layout
                .columns
                .iter()
                .enumerate()
                .map(|(index, &(kind, offset))| api::excel::Column {
                    index: u16::try_from(index).unwrap_or(u16::MAX),
                    kind: column_kind(kind),
                    offset,
                })
                .collect(),
            fields: fields
                .into_iter()
                .map(|field| api::excel::Field {
                    name: field.name,
                    kind: column_kind(field.column_type),
                    index: field.index,
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
        columns: Vec<String>,
        after: Option<api::excel::RowKey>,
        limit: u32,
    ) -> ApiResult<Vec<api::excel::SheetRow>> {
        let after = after.map(|key| (key.row, key.subrow));
        self.host(|state| {
            let indexes = api(state.column_indexes(&name, &columns))?;
            let rows = api(state
                .context
                .services
                .data
                .rows(&name, language, after, limit))?;
            Ok(rows.iter().map(|row| sheet_row(row, &indexes)).collect())
        })
    }

    fn row(
        &mut self,
        name: String,
        language: u8,
        columns: Vec<String>,
        key: api::excel::RowKey,
    ) -> ApiResult<Option<api::excel::SheetRow>> {
        self.host(|state| {
            let indexes = api(state.column_indexes(&name, &columns))?;
            let row = api(state
                .context
                .services
                .data
                .row(&name, language, (key.row, key.subrow)))?;
            Ok(row.as_ref().map(|row| sheet_row(row, &indexes)))
        })
    }

    fn shown(
        &mut self,
        name: String,
        language: u8,
        column: String,
        key: api::excel::RowKey,
    ) -> ApiResult<Option<Vec<u8>>> {
        self.host(|state| {
            let indexes = api(state.column_indexes(&name, std::slice::from_ref(&column)))?;
            let services = &state.context.services;
            let row = api(services.data.row(&name, language, (key.row, key.subrow)))?;
            let Some(row) = row else {
                return Ok(None);
            };
            let Some(crate::game_data::Cell::Text(original)) =
                row.cells.get(usize::from(indexes[0]))
            else {
                return Err(Error::invalid(format!(
                    "{column} of sheet {name} is not a text column"
                ))
                .into());
            };
            Ok(Some(
                services
                    .excel
                    .shown(&name, language, key.row, key.subrow, indexes[0], original)
                    .unwrap_or_else(|| original.clone()),
            ))
        })
    }
}

impl api::file_replacements::Host for State {
    fn replace(&mut self, game_path: String, storage_path: String) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::FilesReplace))?;
        self.host(|state| {
            let services = &state.context.services;
            services
                .game
                .enable_file_replacements()
                .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
            let contents = state.context.storage.read(&storage_path)?;
            services
                .files
                .replace(&state.context.id, &game_path, &contents, &services.data)
        })
        .map_err(Into::into)
    }

    fn restore(&mut self, game_path: String) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::FilesReplace))?;
        if self
            .context
            .services
            .files
            .restore(&self.context.id, &game_path)
        {
            Ok(())
        } else {
            Err(Error::new(
                Kind::NotFound,
                format!("the plugin does not replace {game_path}"),
            )
            .into())
        }
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
            let size = state.context.storage.size(&path)?;
            if size > crate::excel_text::MAX_TABLE_SIZE {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!(
                        "{path}: a table has at most {} bytes",
                        crate::excel_text::MAX_TABLE_SIZE
                    ),
                ));
            }
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

fn target_from_api(element: api::game_ui::Element) -> crate::game_ui::Target {
    crate::game_ui::Target {
        window: element.window,
        element: element.name,
    }
}

fn targets_to_api(targets: Vec<crate::game_ui::Target>) -> Vec<api::game_ui::Element> {
    targets
        .into_iter()
        .map(|target| api::game_ui::Element {
            window: target.window,
            name: target.element,
        })
        .collect()
}

impl State {
    /// Fails when the game build lacks what rules for its windows need.
    fn screen_available(&self) -> crate::error::Result<()> {
        self.context
            .services
            .game
            .with_screen(&mut |_| {})
            .map_err(|reason| Error::new(Kind::Unavailable, reason))
    }
}

impl api::game_ui::Host for State {
    fn elements(&mut self, window: String) -> ApiResult<Vec<api::game_ui::ElementInfo>> {
        api(self.main_only())?;
        self.host(|state| {
            let services = &state.context.services;
            services.ui.elements(&window, &services.data).map(|named| {
                named
                    .into_iter()
                    .map(|named| api::game_ui::ElementInfo {
                        name: named.name,
                        text: named.kind == angelica_map::ElementKind::Text,
                    })
                    .collect()
            })
        })
        .map_err(Into::into)
    }

    fn set_hidden(
        &mut self,
        elements: Vec<api::game_ui::Element>,
    ) -> ApiResult<Vec<api::game_ui::Element>> {
        api(self.main_only())?;
        api(self.permitted(Permission::GameUiHide))?;
        self.host(|state| {
            if !elements.is_empty() {
                state.screen_available()?;
            }
            let services = &state.context.services;
            services
                .ui
                .set_hidden(
                    &state.context.id,
                    elements.into_iter().map(target_from_api).collect(),
                    &services.data,
                )
                .map(targets_to_api)
        })
        .map_err(Into::into)
    }

    fn set_texts(
        &mut self,
        rules: Vec<api::game_ui::TextRule>,
    ) -> ApiResult<Vec<api::game_ui::Element>> {
        use crate::game_ui::Placement;
        api(self.main_only())?;
        api(self.permitted(Permission::GameUiText))?;
        self.host(|state| {
            if !rules.is_empty() {
                state.screen_available()?;
            }
            let services = &state.context.services;
            let rules = rules
                .into_iter()
                .map(|rule| crate::game_ui::TextRule {
                    target: target_from_api(rule.element),
                    text: rule.text,
                    placement: match rule.placement {
                        api::game_ui::Placement::After => Placement::After,
                        api::game_ui::Placement::Before => Placement::Before,
                        api::game_ui::Placement::Instead => Placement::Instead,
                    },
                })
                .collect();
            services
                .ui
                .set_texts(&state.context.id, rules, &services.data)
                .map(targets_to_api)
        })
        .map_err(Into::into)
    }
}

impl api::commands::Host for State {
    fn define(&mut self, commands: Vec<api::commands::Command>) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::ChatCommands))?;
        let commands = commands.into_iter().map(command_from_api).collect();
        self.host(|state| {
            let services = &state.context.services;
            services
                .game
                .enable_commands()
                .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
            services
                .commands
                .define(&state.context.id, commands, &services.data)
        })
        .map_err(Into::into)
    }
}

fn command_from_api(command: api::commands::Command) -> crate::commands::Command {
    use crate::commands::{Parameter, ValueKind};
    use api::commands::ValueKind as Kind;
    crate::commands::Command {
        path: command.path,
        aliases: command.aliases,
        description: command.description,
        parameters: command
            .parameters
            .into_iter()
            .map(|parameter| Parameter {
                name: parameter.name,
                description: parameter.description,
                kind: match parameter.kind {
                    Kind::Word => ValueKind::Word,
                    Kind::Text => ValueKind::Text,
                    Kind::Integer(bounds) => ValueKind::Integer {
                        min: bounds.min,
                        max: bounds.max,
                    },
                    Kind::Number => ValueKind::Number,
                    Kind::Choice(words) => ValueKind::Choice(words),
                    Kind::Switch => ValueKind::Switch,
                },
                optional: parameter.optional,
            })
            .collect(),
    }
}

/// An invocation as the guest's `command` export takes it.
#[must_use]
pub fn invocation_to_api(invocation: crate::commands::Invocation) -> api::commands::Invocation {
    use crate::commands::Value as V;
    use api::commands::Value;
    api::commands::Invocation {
        path: invocation.path,
        arguments: invocation
            .arguments
            .into_iter()
            .map(|(name, value)| api::commands::Argument {
                name,
                value: match value {
                    V::Word(word) => Value::Word(word),
                    V::Text(text) => Value::Text(text),
                    V::Integer(value) => Value::Integer(value),
                    V::Number(value) => Value::Number(value),
                    V::Choice(word) => Value::Choice(word),
                    V::Switch(on) => Value::Switch(on),
                },
            })
            .collect(),
    }
}

fn http_request(request: api::http::Request) -> crate::http::Request {
    crate::http::Request {
        url: request.url,
        headers: request
            .headers
            .into_iter()
            .map(|header| (header.name, header.value))
            .collect(),
    }
}

fn http_head(head: crate::http::Head) -> api::http::Head {
    api::http::Head {
        status: head.status,
        headers: head
            .headers
            .into_iter()
            .map(|(name, value)| api::http::Header { name, value })
            .collect(),
    }
}

impl api::http::Host for State {
    fn get(&mut self, request: api::http::Request) -> ApiResult<api::http::Response> {
        let (client, slot) = api(self.http())?;
        let request = http_request(request);
        let (head, body) = self.host(|_| api(client.get(&request, move || slot.cancelled())))?;
        Ok(api::http::Response {
            head: http_head(head),
            body,
        })
    }

    fn send(
        &mut self,
        method: api::http::Method,
        request: api::http::Request,
        body: Vec<u8>,
    ) -> ApiResult<api::http::Response> {
        let (client, slot) = api(self.http())?;
        api(self.permitted(Permission::HttpSend))?;
        let method = match method {
            api::http::Method::Post => crate::http::Method::Post,
            api::http::Method::Put => crate::http::Method::Put,
            api::http::Method::Patch => crate::http::Method::Patch,
            api::http::Method::Delete => crate::http::Method::Delete,
        };
        let request = http_request(request);
        let (head, body) = self
            .host(|_| api(client.send_data(method, &request, &body, move || slot.cancelled())))?;
        Ok(api::http::Response {
            head: http_head(head),
            body,
        })
    }

    fn download(
        &mut self,
        request: api::http::Request,
        path: String,
    ) -> ApiResult<(api::http::Head, u64)> {
        let (client, slot) = api(self.http())?;
        let request = http_request(request);
        self.host(|state| {
            let storage = &state.context.storage;
            storage.check(&path)?;
            let (head, mut body) = client.send(
                &request,
                crate::http::DOWNLOAD_TIME,
                crate::http::MAX_DOWNLOAD,
                move || slot.cancelled(),
            )?;
            if !(200..300).contains(&head.status) {
                return Ok((http_head(head), 0));
            }
            let ((), size) =
                storage.write_with(&path, |file, room| copy_body(&mut body, file, room, &path))?;
            Ok((http_head(head), size))
        })
        .map_err(|error: Error| error.into())
    }
}

/// Copies a response body into a file, at most `room` bytes.
fn copy_body(
    body: &mut crate::http::BodyReader,
    file: &mut std::fs::File,
    room: u64,
    path: &str,
) -> crate::error::Result<()> {
    use std::io::{Read, Write};
    let mut buffer = vec![0; 64 * 1024];
    let mut written = 0u64;
    loop {
        let read = body.read(&mut buffer).map_err(crate::http::read_failure)?;
        if read == 0 {
            return Ok(());
        }
        written += read as u64;
        if written > room {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("{path}: the plugin's storage would exceed its quota"),
            ));
        }
        file.write_all(&buffer[..read])
            .map_err(|e| Error::new(Kind::Io, format!("{path}: {e}")))?;
    }
}

impl api::lens::Host for State {
    fn prepare(&mut self, query: String) -> ApiResult<api::lens::Prepared> {
        api(self.main_only())?;
        let context = self.context.clone();
        let permitted = |domain: &str| context.lens_read.contains(domain);
        let (handle, prepared) = self
            .host(|state| {
                state
                    .queries
                    .prepare(&context.services.lens, &query, &permitted)
            })
            .map_err(api::types::Error::from)?;
        Ok(api::lens::Prepared {
            query: handle,
            columns: prepared
                .columns
                .into_iter()
                .map(|column| api::lens::Column {
                    name: column.name,
                    doc: column.doc,
                })
                .collect(),
        })
    }

    fn run(&mut self, query: u32) -> ApiResult<Vec<api::lens::Row>> {
        api(self.main_only())?;
        let context = self.context.clone();
        let permitted = |domain: &str| context.lens_read.contains(domain);
        let rows = self
            .host(|state| {
                let prepared = state
                    .queries
                    .plan(&context.services.lens, query, &permitted)?;
                context.services.game.query(&prepared)
            })
            .map_err(api::types::Error::from)?;
        Ok(rows.into_iter().map(row_to_api).collect())
    }

    fn watch(&mut self, query: String) -> ApiResult<api::lens::Prepared> {
        api(self.main_only())?;
        let context = self.context.clone();
        let permitted = |domain: &str| context.lens_read.contains(domain);
        let services = &context.services;
        let (handle, prepared) = self
            .host(|state| state.queries.watch(&services.lens, &query, &permitted))
            .map_err(api::types::Error::from)?;
        let columns = prepared
            .columns
            .iter()
            .map(|column| api::lens::Column {
                name: column.name.clone(),
                doc: column.doc.clone(),
            })
            .collect();
        let started = self.host(|_| {
            services.lens.watch(
                &context.id,
                handle,
                &query,
                context.lens_read.clone(),
                prepared,
                &|point, entry, address| services.game.enable_watch(point, entry, address),
            )
        });
        if let Err(error) = started {
            self.queries.release(handle);
            return Err(error.into());
        }
        Ok(api::lens::Prepared {
            query: handle,
            columns,
        })
    }

    fn act(&mut self, action: String, params: Vec<(String, api::lens::Value)>) -> ApiResult<()> {
        api(self.main_only())?;
        let context = self.context.clone();
        let services = &context.services;
        if services.starting.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::new(
                Kind::Unavailable,
                "the game is still starting: act from tick or a player's action",
            )
            .into());
        }
        let params: Vec<(String, crate::lens::Value)> = params
            .into_iter()
            .map(|(name, value)| (name, value_from_api(value)))
            .collect();
        self.host(|_| {
            let (schema, _) = services.lens.schema();
            let ready = crate::lens::act::prepare(&schema, &action, &params, &|domain| {
                context.lens_act.contains(domain)
            })?;
            services.lens.acting.take(
                &context.id,
                &ready,
                &GameCaller(services.game.as_ref()),
                Instant::now(),
            )
        })
        .map_err(Into::into)
    }

    fn set(&mut self, path: String, value: api::lens::Value) -> ApiResult<u32> {
        api(self.main_only())?;
        let context = self.context.clone();
        let services = &context.services;
        let value = value_from_api(value);
        self.host(|_| {
            let (schema, _) = services.lens.schema();
            let ready = crate::lens::set::prepare(
                &schema,
                &path,
                &value,
                &|domain| context.lens_read.contains(domain),
                &|domain| context.lens_change.contains(domain),
            )?;
            services
                .lens
                .changes
                .set(&context.id, &ready, &GameCaller(services.game.as_ref()))
                .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
        })
        .map_err(Into::into)
    }

    fn restore(&mut self, path: String) -> ApiResult<()> {
        api(self.main_only())?;
        let context = self.context.clone();
        let services = &context.services;
        self.host(|_| {
            services
                .lens
                .changes
                .restore(&context.id, &path, &GameCaller(services.game.as_ref()));
        });
        Ok(())
    }

    fn decide(&mut self, rule: String, outcome: String) -> ApiResult<u32> {
        api(self.main_only())?;
        let context = self.context.clone();
        let services = &context.services;
        let permitted: BTreeSet<String> = context
            .lens_read
            .union(&context.lens_change)
            .cloned()
            .collect();
        let may_change = |domain: &str| context.lens_change.contains(domain);
        services
            .lens
            .may_decide(&rule, &may_change)
            .map_err(api::types::Error::from)?;
        let (handle, prepared) = self
            .host(|state| {
                state
                    .queries
                    .rule(&services.lens, &rule, &|domain| permitted.contains(domain))
            })
            .map_err(api::types::Error::from)?;
        let started = self.host(|_| {
            services.lens.decide(
                crate::lens::decide::Request {
                    plugin: &context.id,
                    handle,
                    text: &rule,
                    outcome: &outcome,
                    permitted: permitted.clone(),
                },
                &may_change,
                prepared,
                &|point, entry, address| services.game.enable_decision(point, entry, address),
            )
        });
        if let Err(error) = started {
            self.queries.release(handle);
            return Err(error.into());
        }
        Ok(handle)
    }

    fn release(&mut self, query: u32) {
        use crate::lens::HeldKind;
        let lens = &self.context.services.lens;
        match self.queries.release(query) {
            Some(HeldKind::Watch) => lens.watches.unsubscribe(&self.context.id, query),
            Some(HeldKind::Rule) => lens.decisions.remove(&self.context.id, query),
            Some(HeldKind::Query) | None => {}
        }
    }
}

impl api::messages::Host for State {
    fn send(
        &mut self,
        to: String,
        name: String,
        payload: Vec<u8>,
        in_reply_to: Option<u32>,
    ) -> ApiResult<u32> {
        api(self.main_only())?;
        if in_reply_to.is_none() {
            api(self.permitted(Permission::MessagesSend))?;
        }
        let context = self.context.clone();
        self.host(|_| {
            context.services.messages.send(
                &context.id,
                crate::messages::Outgoing {
                    to: &to,
                    name: &name,
                    payload,
                    in_reply_to,
                },
                &|receiver| context.message_to.contains(receiver),
            )
        })
        .map_err(Into::into)
    }
}

impl api::clipboard::Host for State {
    fn copy(&mut self, text: String) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::ClipboardWrite))?;
        if !self.player_action {
            return Err(Error::new(
                Kind::WrongContext,
                "text is copied only for the player's action: in ui-event or command",
            )
            .into());
        }
        let context = self.context.clone();
        self.host(|_| context.services.clipboard.copy(&text))
            .map_err(Into::into)
    }
}

impl api::draw::Host for State {
    fn set(&mut self, shapes: Vec<api::draw::Shape>) -> ApiResult<()> {
        use crate::draw::Shape;
        use api::draw::Shape as Api;
        api(self.main_only())?;
        api(self.permitted(Permission::DrawWorld))?;
        let point = |point: api::draw::Point| [point.x, point.y, point.z];
        let shapes = shapes
            .into_iter()
            .map(|shape| match shape {
                Api::Line(line) => Shape::Line {
                    from: point(line.start),
                    to: point(line.end),
                    color: line.color,
                    thickness: line.thickness,
                },
                Api::Circle(circle) => Shape::Circle {
                    center: point(circle.center),
                    radius: circle.radius,
                    color: circle.color,
                    thickness: circle.thickness,
                    filled: circle.filled,
                },
                Api::Text(label) => Shape::Text {
                    at: point(label.at),
                    text: label.text,
                    color: label.color,
                },
            })
            .collect();
        let context = self.context.clone();
        self.host(|_| context.services.draw.set(&context.id, shapes))
            .map_err(Into::into)
    }
}

impl api::events::Host for State {
    fn subscribe(&mut self, kinds: Vec<api::events::EventKind>) -> ApiResult<()> {
        use crate::events::EventKind;
        api(self.main_only())?;
        let kinds: BTreeSet<EventKind> = kinds
            .into_iter()
            .map(|kind| match kind {
                api::events::EventKind::ChatMessage => EventKind::ChatMessage,
            })
            .collect();
        for kind in &kinds {
            api(self.permitted(kind.permission()))?;
        }
        self.host(|state| {
            let services = &state.context.services;
            if kinds.contains(&EventKind::ChatMessage) {
                services
                    .game
                    .enable_chat_messages()
                    .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
            }
            services.events.subscribe(&state.context.id, kinds);
            Ok::<_, Error>(())
        })
        .map_err(Into::into)
    }
}

/// A Lens row as the API carries it.
#[must_use]
pub fn row_to_api(row: crate::lens::Row) -> api::lens::Row {
    use crate::lens::Value as V;
    use api::lens::Value;
    api::lens::Row {
        indexes: row.indexes,
        values: row
            .values
            .into_iter()
            .map(|value| {
                value.map(|value| match value {
                    V::Boolean(value) => Value::Boolean(value),
                    V::Signed(value) => Value::Signed(value),
                    V::Unsigned(value) => Value::Unsigned(value),
                    V::Float(value) => Value::Float(value),
                    V::Text(bytes) => Value::Text(bytes),
                    V::Row { sheet, row } => Value::Sheet(api::lens::SheetRow { sheet, row }),
                    V::Named(name) => Value::Named(name),
                    V::Flags { names, bits } => Value::Flags(api::lens::FlagSet { names, bits }),
                })
            })
            .collect(),
    }
}

/// A value a plugin gave, as Lens takes it.
fn value_from_api(value: api::lens::Value) -> crate::lens::Value {
    use crate::lens::Value as V;
    use api::lens::Value;
    match value {
        Value::Boolean(value) => V::Boolean(value),
        Value::Signed(value) => V::Signed(value),
        Value::Unsigned(value) => V::Unsigned(value),
        Value::Float(value) => V::Float(value),
        Value::Text(bytes) => V::Text(bytes),
        Value::Sheet(row) => V::Row {
            sheet: row.sheet,
            row: row.row,
        },
        Value::Named(name) => V::Named(name),
        Value::Flags(flags) => V::Flags {
            names: flags.names,
            bits: flags.bits,
        },
    }
}

/// The game as Lens actions call it and changes write it.
pub struct GameCaller<'a>(pub &'a dyn crate::game::Game);

impl crate::lens::set::Writer for GameCaller<'_> {
    fn memory(&self) -> &dyn crate::lens::Memory {
        self.0.memory()
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        self.0.write(address, bytes)
    }
}

impl crate::lens::act::Caller for GameCaller<'_> {
    fn memory(&self) -> &dyn crate::lens::Memory {
        self.0.memory()
    }

    fn call(
        &self,
        address: u64,
        args: &[u64],
    ) -> std::result::Result<crate::lens::act::Returned, String> {
        self.0.call(address, args)
    }
}

/// An event as the guest's `game-event` export takes it.
#[must_use]
pub fn event_to_api(event: crate::events::GameEvent) -> api::events::GameEvent {
    match event {
        crate::events::GameEvent::ChatMessage(message) => {
            api::events::GameEvent::ChatMessage(api::events::ChatMessage {
                kind: message.info & 0x7F,
                info: message.info,
                sender: message.sender,
                message: message.message,
                timestamp: message.timestamp,
            })
        }
    }
}

impl api::chat::Host for State {
    fn print(&mut self, message: Vec<api::chat::Span>) -> ApiResult<()> {
        use crate::chat::{Span, Tone};
        use api::chat::Tone as T;
        api(self.main_only())?;
        api(self.permitted(Permission::ChatPrint))?;
        let spans: Vec<Span> = message
            .into_iter()
            .map(|span| Span {
                text: span.text,
                tone: match span.tone {
                    T::Normal => Tone::Normal,
                    T::Accent => Tone::Accent,
                    T::Muted => Tone::Muted,
                    T::Good => Tone::Good,
                    T::Warning => Tone::Warning,
                    T::Bad => Tone::Bad,
                },
                italic: span.italic,
            })
            .collect();
        self.host(|state| print(&state.context, &spans))
            .map_err(Into::into)
    }
}

/// Prints a message of a plugin: checked, within the plugin's rate, with
/// its name in front.
///
/// # Errors
/// A message that breaks a rule, a plugin over its rate, and a game that
/// cannot print.
pub fn print(context: &Context, spans: &[crate::chat::Span]) -> crate::error::Result<()> {
    crate::chat::check(spans).map_err(Error::invalid)?;
    let services = &context.services;
    if !services.chat.admit(&context.id, Instant::now()) {
        return Err(Error::new(
            Kind::LimitExceeded,
            format!(
                "a plugin prints at most {} messages a second",
                crate::chat::MAX_PER_WINDOW
            ),
        ));
    }
    services
        .game
        .print(&crate::chat::encode(&context.name, spans))
        .map_err(|reason| Error::new(Kind::Unavailable, reason))
}

/// Whether a trap came from running out of time or fuel rather than a bug
/// in the plugin.
#[must_use]
pub fn describe_trap(store: &Store<State>, error: &wasmtime::Error) -> String {
    match error.downcast_ref::<wasmtime::Trap>() {
        Some(wasmtime::Trap::Interrupt) if store.data().exceeded == Some(Exceeded::Wall) => {
            "the call spent too long in host operations".into()
        }
        Some(wasmtime::Trap::Interrupt) => "the call ran out of time".into(),
        Some(wasmtime::Trap::OutOfFuel) => "the call ran out of fuel".into(),
        _ => format!("{error:#}"),
    }
}
