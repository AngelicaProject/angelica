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
    /// What plugins draw over the game world.
    pub draw: Arc<crate::draw::Drawings>,
    /// Messages between plugins.
    pub messages: Arc<crate::messages::Messages>,
    /// Text plugins put on the clipboard this frame.
    pub clipboard: Arc<crate::clipboard::Clipboard>,
    /// Pages plugins open in the player's browser this frame.
    pub links: Arc<crate::links::Links>,
    /// Plugins' entries in the game's menus, and what the player chose.
    pub menus: Arc<crate::menus::Menus>,
    /// Plugins' notifications on the screen.
    pub notifications: Arc<crate::notifications::Notifications>,
    /// When plugins last played a sound.
    pub sounds: Arc<crate::sound::Sounds>,
    /// Plugins' WebSocket connections.
    pub sockets: Arc<crate::sockets::Sockets>,
    pub game: Arc<dyn crate::game::Game>,
    /// True while the host starts plugins before the game's code runs.
    pub starting: AtomicBool,
    /// Whether Angelica's interface is in Russian; English otherwise.
    pub russian: AtomicBool,
    /// Plugins' handlers in the game's calls.
    pub intercepts: Arc<crate::intercepts::Intercepts>,
    /// The game map's description for plugins' direct access, and the
    /// central values they wrote.
    pub memory: Arc<crate::memory::Central>,
    /// Plugins' surfaces and GPU objects, and what the renderer has yet
    /// to do.
    pub gpu: Arc<crate::gpu::Gpu>,
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
    /// The sites whose pages it may open; empty without `links.open`.
    pub link_hosts: Vec<String>,
    pub memory_bytes: u64,
    pub storage: Arc<Storage>,
    pub services: Arc<Services>,
    pub log: Arc<Log>,
    pub jobs: Arc<Jobs>,
    /// Requests to the approved hosts; `None` without `http.get`.
    pub http: Option<Arc<crate::http::Client>>,
    /// The minor version of plugin API 1 the manifest names, which decides
    /// the kinds of places and parts it may use.
    pub api_minor: u16,
    /// The plugin's own Anatomia entries resolved for this game, when it
    /// has them and holds `unreviewed`.
    pub own: Option<Arc<crate::intercepts::Own>>,
    /// The plugin's component, for its intercept instances.
    pub component: Option<Component>,
    /// Where its handlers go among other plugins' handlers.
    pub order: crate::intercepts::Order,
}

/// Which instance a store belongs to.
pub enum Role {
    Main,
    Job(Arc<Slot>),
    /// An intercept instance, with the call its handler is in.
    Intercept(Option<crate::intercepts::Frame>),
    /// An intercept instance preparing the snapshot it will run with.
    Prepare(Arc<Vec<u8>>),
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
    /// The handles and fields of its direct access to the game.
    pub(crate) memory: crate::memory::Instance,
}

/// A store with its instance's state.
#[must_use]
pub fn store(sandbox: &Sandbox, context: Arc<Context>, role: Role) -> Store<State> {
    store_in(&sandbox.engine, context, role)
}

/// A store for an instance of `context`'s component, in its engine.
///
/// # Panics
/// When the context has no component.
#[must_use]
pub fn store_for(context: &Arc<Context>, role: Role) -> Store<State> {
    let engine = context
        .component
        .as_ref()
        .expect("a context with intercepts has its component")
        .engine()
        .clone();
    store_in(&engine, context.clone(), role)
}

fn store_in(engine: &Engine, context: Arc<Context>, role: Role) -> Store<State> {
    let memory = usize::try_from(context.memory_bytes).unwrap_or(usize::MAX);
    let limits = StoreLimitsBuilder::new()
        .memory_size(memory)
        .instances(64)
        .tables(64)
        .memories(16)
        .build();
    let mut store = Store::new(
        engine,
        State {
            context,
            role,
            limits,
            budget: Budget::new(Duration::ZERO, Duration::ZERO),
            exceeded: None,
            queries: crate::lens::Queries::default(),
            player_action: false,
            memory: crate::memory::Instance::default(),
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
    pub(crate) fn host<T>(&mut self, work: impl FnOnce(&mut Self) -> T) -> T {
        let started = Instant::now();
        let result = work(self);
        self.budget.host += started.elapsed();
        result
    }

    fn main_only(&self) -> crate::error::Result<()> {
        match self.role {
            Role::Main => Ok(()),
            Role::Job(_) | Role::Intercept(_) | Role::Prepare(_) => Err(Error::new(
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
            Role::Main | Role::Intercept(_) | Role::Prepare(_) => {
                Err(Error::new(Kind::WrongContext, "report is called from a job").into())
            }
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

fn http_method(method: api::http::Method) -> crate::http::Method {
    match method {
        api::http::Method::Post => crate::http::Method::Post,
        api::http::Method::Put => crate::http::Method::Put,
        api::http::Method::Patch => crate::http::Method::Patch,
        api::http::Method::Delete => crate::http::Method::Delete,
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
        let method = http_method(method);
        let request = http_request(request);
        let (head, body) = self
            .host(|_| api(client.send_data(method, &request, &body, move || slot.cancelled())))?;
        Ok(api::http::Response {
            head: http_head(head),
            body,
        })
    }

    fn upload(
        &mut self,
        method: api::http::Method,
        request: api::http::Request,
        path: String,
    ) -> ApiResult<api::http::Response> {
        let (client, slot) = api(self.http())?;
        api(self.permitted(Permission::HttpSend))?;
        let method = http_method(method);
        let request = http_request(request);
        let (head, body) = self.host(|state| {
            let (file, size) = api(state.context.storage.open(&path))?;
            api(client.upload(method, &request, file, size, move || slot.cancelled()))
        })?;
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

    fn decide(
        &mut self,
        rule: String,
        outcome: String,
        value: Option<api::lens::Value>,
    ) -> ApiResult<u32> {
        api(self.main_only())?;
        let (schema, _) = self.context.services.lens.schema();
        if crate::lens::derived::event(&schema, &rule).is_some() {
            let value = value.map(value_from_api);
            return self.decide_derived(&rule, &outcome, value.as_ref());
        }
        if value.is_some() {
            return Err(Error::invalid(format!("outcome {outcome} takes no value")).into());
        }
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

    fn decide_file(&mut self, event: String, path: String) -> ApiResult<api::lens::RuleFile> {
        api(self.main_only())?;
        let context = self.context.clone();
        let services = &context.services;
        let (schema, _) = services.lens.schema();
        let Some(found) = schema.objects.events.get(&event) else {
            return Err(
                Error::new(Kind::NotFound, format!("the game map has no event {event}")).into(),
            );
        };
        if found.rules.as_deref() != Some("text-table-v1") {
            return Err(Error::invalid(format!("event {event} takes no rule files")).into());
        }
        if !context.lens_change.contains(&found.domain) {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!(
                    "deciding at {event} changes domain {}, which the plugin may not change: list it in the manifest's [lens] change",
                    found.domain
                ),
            )
            .into());
        }
        let bytes = self.host(|state| {
            services
                .game
                .enable_excel_text()
                .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
            let size = state.context.storage.size(&path)?;
            if size > crate::excel_text::MAX_TABLE_SIZE {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!(
                        "{path}: a rule file has at most {} bytes",
                        crate::excel_text::MAX_TABLE_SIZE
                    ),
                ));
            }
            state.context.storage.read_unbounded(&path)
        })?;
        let handle = self.queries.derived(&format!("{event} {path}"))?;
        let info = services
            .excel
            .add_table(&context.id, handle, bytes)
            .map_err(|reason| Error::invalid(format!("{path}: {reason}")));
        match info {
            Ok(info) => Ok(api::lens::RuleFile {
                handle,
                parts: info.sheets,
                rules: info.cells,
            }),
            Err(error) => {
                self.queries.release(handle);
                Err(error.into())
            }
        }
    }

    fn decisions(&mut self, handle: u32) -> ApiResult<api::lens::DecisionStatus> {
        api(self.main_only())?;
        if self.queries.kind(handle) != Some(crate::lens::HeldKind::Derived) {
            return Err(Error::new(
                Kind::NotFound,
                format!("{handle} is not a rule at a derived event"),
            )
            .into());
        }
        let status = self
            .context
            .services
            .excel
            .status(&self.context.id, handle)
            .unwrap_or_default();
        Ok(api::lens::DecisionStatus {
            taken: status.cells_replaced,
            stale: status.guard_mismatches,
            refused: status.rows_refused,
            mismatched: status
                .sheets_mismatched
                .into_iter()
                .map(|(sheet, reason)| format!("{sheet}: {}", reason.reason()))
                .collect(),
        })
    }

    fn release(&mut self, query: u32) {
        use crate::lens::HeldKind;
        let services = &self.context.services;
        match self.queries.release(query) {
            Some(HeldKind::Watch) => services.lens.watches.unsubscribe(&self.context.id, query),
            Some(HeldKind::Rule) => services.lens.decisions.remove(&self.context.id, query),
            Some(HeldKind::Derived) => {
                services.excel.remove(&self.context.id, query);
                services.case.remove(&self.context.id, query);
            }
            Some(HeldKind::Query) | None => {}
        }
    }
}

impl State {
    /// A rule at a derived decision point, handed to the adapter of its
    /// derivation under a new handle.
    fn decide_derived(
        &mut self,
        rule: &str,
        outcome: &str,
        value: Option<&crate::lens::Value>,
    ) -> ApiResult<u32> {
        let context = self.context.clone();
        let (schema, _) = context.services.lens.schema();
        let derived = crate::lens::derived::prepare(
            &schema,
            rule,
            outcome,
            value,
            &|domain| context.lens_read.contains(domain),
            &|domain| context.lens_change.contains(domain),
        )?;
        let handle = self.queries.derived(rule)?;
        if let Err(error) = self.host(|_| add_derived(&context, handle, &derived)) {
            self.queries.release(handle);
            return Err(error.into());
        }
        Ok(handle)
    }
}

/// Hands a checked rule at a derived decision point to the adapter of its
/// derivation.
fn add_derived(
    context: &Context,
    handle: u32,
    rule: &crate::lens::derived::Rule,
) -> crate::error::Result<()> {
    use crate::lens::derived::{integer, text};
    let services = &context.services;
    let unavailable = |reason: String| Error::new(Kind::Unavailable, reason);
    let fits = |from: &str| -> crate::error::Result<Option<u64>> {
        integer(rule, from)
            .map(|value| {
                u64::try_from(value).map_err(|_| {
                    Error::invalid(format!(
                        "{}: {from} {value} is never a value of it",
                        rule.event
                    ))
                })
            })
            .transpose()
    };
    let narrow = |from: &str, most: u64| -> crate::error::Result<Option<u64>> {
        match fits(from)? {
            Some(value) if value > most => Err(Error::invalid(format!(
                "{}: {from} {value} is never a value of it",
                rule.event
            ))),
            other => Ok(other),
        }
    };
    match rule.derivation.as_str() {
        "sheet_cell" => {
            services.game.enable_excel_text().map_err(unavailable)?;
            let cell = crate::excel_text::CellRule {
                sheet: text(rule, "sheet").map(|name| String::from_utf8_lossy(name).into_owned()),
                language: narrow("language", u64::from(u8::MAX))?
                    .and_then(|value| u8::try_from(value).ok()),
                row: narrow("row", u64::from(u32::MAX))?
                    .and_then(|value| u32::try_from(value).ok()),
                subrow: narrow("subrow", u64::from(u16::MAX))?
                    .and_then(|value| u16::try_from(value).ok()),
                column: narrow("column", u64::from(u16::MAX))?
                    .and_then(|value| u16::try_from(value).ok()),
                original: text(rule, "text").map(<[u8]>::to_vec),
                text: rule.text.clone(),
            };
            services.excel.add_rule(&context.id, handle, cell);
            Ok(())
        }
        "case_letter" => {
            use crate::text_case::{Direction, LOWER_ENTRY, UPPER_ENTRY};
            let direction = match rule.at.as_str() {
                UPPER_ENTRY => Direction::Upper,
                LOWER_ENTRY => Direction::Lower,
                other => {
                    return Err(unavailable(format!(
                        "the host converts letters only at {UPPER_ENTRY} and {LOWER_ENTRY}, not at {other}"
                    )));
                }
            };
            let single = |bytes: &[u8]| {
                let text = std::str::from_utf8(bytes).ok()?;
                let mut chars = text.chars();
                let first = chars.next()?;
                chars.next().is_none().then_some(first)
            };
            let (Some(letter), Some(becomes)) =
                (text(rule, "letter").and_then(single), single(&rule.text))
            else {
                return Err(Error::invalid(format!(
                    "a rule at {} gives one letter for one letter",
                    rule.event
                )));
            };
            services.game.enable_text_case().map_err(unavailable)?;
            services
                .case
                .add(&context.id, handle, direction, letter, becomes)
                .map_err(Error::invalid)
        }
        other => Err(unavailable(format!(
            "this host does not derive {other} events"
        ))),
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

impl api::websocket::Host for State {
    fn open(
        &mut self,
        url: String,
        protocols: Vec<String>,
        authorization: Option<String>,
    ) -> ApiResult<u32> {
        api(self.main_only())?;
        api(self.permitted(Permission::HttpWebsocket))?;
        let client = api(self
            .context
            .http
            .clone()
            .ok_or_else(|| Error::new(Kind::PermissionDenied, "the plugin lists no hosts")))?;
        api(client.check_socket(&url))?;
        let context = self.context.clone();
        let dial = context.services.sockets.dial();
        let target = url.clone();
        self.host(|_| {
            context.services.sockets.open(
                &context.id,
                &url,
                &protocols,
                authorization.as_deref(),
                Box::new(move || dial(&client, &target)),
            )
        })
        .map_err(Into::into)
    }

    fn send(&mut self, socket: u32, data: api::websocket::Data) -> ApiResult<()> {
        api(self.main_only())?;
        let message = match data {
            api::websocket::Data::Text(text) => crate::sockets::Message::Text(text),
            api::websocket::Data::Binary(bytes) => crate::sockets::Message::Binary(bytes),
        };
        let context = self.context.clone();
        self.host(|_| context.services.sockets.send(&context.id, socket, message))
            .map_err(Into::into)
    }

    fn close(&mut self, socket: u32) -> ApiResult<()> {
        api(self.main_only())?;
        let context = self.context.clone();
        self.host(|_| context.services.sockets.close(&context.id, socket))
            .map_err(Into::into)
    }
}

impl api::sound::Host for State {
    fn play(&mut self, path: String, volume: f32) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::SoundPlay))?;
        let context = self.context.clone();
        self.host(|_| {
            if context.storage.size(&path)? > crate::sound::MOST_BYTES {
                return Err(Error::new(
                    Kind::InvalidArgument,
                    format!(
                        "a sound file has at most {} bytes",
                        crate::sound::MOST_BYTES
                    ),
                ));
            }
            let wav = crate::sound::prepare(&context.storage.read(&path)?, volume)?;
            context.services.sounds.allow(&context.id, Instant::now())?;
            if context.services.game.play_sound(wav) {
                Ok(())
            } else {
                Err(Error::new(
                    Kind::Unavailable,
                    "Windows did not play the sound",
                ))
            }
        })
        .map_err(Into::into)
    }
}

impl api::notifications::Host for State {
    fn show(&mut self, text: String, attention: bool) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::Notifications))?;
        let context = self.context.clone();
        self.host(|_| {
            context
                .services
                .notifications
                .show(&context.id, &text, attention, Instant::now())
        })
        .map_err(Into::into)
    }

    fn pop_up(&mut self, popup: api::ui::Place) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::Notifications))?;
        let popup = angelica_view::data::decode_popup(
            &crate::view::place_from_wit(popup),
            self.context.api_minor,
        )
        .map_err(|reason| Error::new(Kind::InvalidArgument, reason))?;
        let context = self.context.clone();
        self.host(|_| {
            context
                .services
                .notifications
                .pop_up(&context.id, popup, Instant::now())
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

impl api::links::Host for State {
    fn open(&mut self, url: String) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::LinksOpen))?;
        if !self.player_action {
            return Err(Error::new(
                Kind::WrongContext,
                "a link is opened only for the player's action: in ui-event or command",
            )
            .into());
        }
        let context = self.context.clone();
        self.host(|_| {
            crate::links::check(&url, &context.link_hosts)?;
            context
                .services
                .links
                .open(&context.id, &url, Instant::now())
        })
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

impl api::events::Host for State {}

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

    fn call_setter(&self, function: u64, object: u64, value: &crate::lens::set::Setting) -> bool {
        self.0.call_setter(function, object, value)
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

/// A span of a plugin's message as the host's chat service takes it.
fn chat_span(span: api::chat::Span) -> crate::chat::Span {
    use crate::chat::Tone;
    use api::chat::Tone as T;
    crate::chat::Span {
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
    }
}

impl api::chat::Host for State {
    fn print(&mut self, message: Vec<api::chat::Span>) -> ApiResult<()> {
        api(self.main_only())?;
        api(self.permitted(Permission::ChatPrint))?;
        let spans: Vec<crate::chat::Span> = message.into_iter().map(chat_span).collect();
        self.host(|state| print(&state.context, &spans))
            .map_err(Into::into)
    }

    fn print_pieces(&mut self, message: Vec<api::chat::Piece>) -> ApiResult<()> {
        use crate::chat::Piece;
        api(self.main_only())?;
        api(self.permitted(Permission::ChatPrint))?;
        let links = message
            .iter()
            .filter(|piece| matches!(piece, api::chat::Piece::Item(_)))
            .count();
        if links > crate::chat::MAX_LINKS {
            return Err(Error::invalid(format!(
                "a message has at most {} item links",
                crate::chat::MAX_LINKS
            ))
            .into());
        }
        self.host(|state| {
            let mut pieces = Vec::with_capacity(message.len());
            for piece in message {
                pieces.push(match piece {
                    api::chat::Piece::Span(span) => Piece::Span(chat_span(span)),
                    api::chat::Piece::Item(link) => Piece::Item(state.item_link(link)?),
                });
            }
            print_pieces(&state.context, &pieces)
        })
        .map_err(Into::into)
    }
}

impl State {
    /// An item link with the item's name as `excel.shown` gives it and its
    /// rarity, both from the `Item` sheet by the game map's names.
    fn item_link(&self, link: api::chat::ItemLink) -> crate::error::Result<crate::chat::ItemLink> {
        use crate::game_data::{Cell, Value};
        let services = &self.context.services;
        let columns = ["name".to_owned(), "rarity".to_owned()];
        let indexes = self.column_indexes("Item", &columns)?;
        let row = services
            .data
            .row("Item", link.language, (link.item, 0))?
            .ok_or_else(|| {
                Error::new(
                    Kind::NotFound,
                    format!("the Item sheet has no row {}", link.item),
                )
            })?;
        let Some(Cell::Text(original)) = row.cells.get(usize::from(indexes[0])) else {
            return Err(Error::invalid("name of sheet Item is not a text column"));
        };
        if original.is_empty() {
            return Err(Error::new(
                Kind::NotFound,
                format!("item {} has no name", link.item),
            ));
        }
        let rarity = match row.cells.get(usize::from(indexes[1])) {
            Some(Cell::Value(Value::Unsigned(rarity))) => u8::try_from(*rarity).ok(),
            Some(Cell::Value(Value::Signed(rarity))) => u8::try_from(*rarity).ok(),
            _ => None,
        }
        .ok_or_else(|| Error::invalid("rarity of sheet Item is not a small number"))?;
        let name = services
            .excel
            .shown("Item", link.language, link.item, 0, indexes[0], original)
            .unwrap_or_else(|| original.clone());
        Ok(crate::chat::ItemLink {
            item: link.item,
            hq: link.hq,
            name,
            rarity,
        })
    }
}

/// Prints a message of a plugin: checked, within the plugin's rate, with
/// its name in front.
///
/// # Errors
/// A message that breaks a rule, a plugin over its rate, and a game that
/// cannot print.
pub fn print(context: &Context, spans: &[crate::chat::Span]) -> crate::error::Result<()> {
    let pieces: Vec<crate::chat::Piece> = spans
        .iter()
        .cloned()
        .map(crate::chat::Piece::Span)
        .collect();
    print_pieces(context, &pieces)
}

/// Prints a message of spans and item links, as [`print`] does.
///
/// # Errors
/// As [`print`].
pub fn print_pieces(context: &Context, pieces: &[crate::chat::Piece]) -> crate::error::Result<()> {
    crate::chat::check_pieces(pieces).map_err(Error::invalid)?;
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
        .print(&crate::chat::encode_pieces(&context.name, pieces))
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
