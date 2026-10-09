//! Installed plugins and their lifecycle: approval, enabling and disabling,
//! faults, and the calls of every frame. Main-instance calls happen only on
//! the thread that created the host, the game's main thread.
#![forbid(unsafe_code)]
use crate::{
    chat::{Span, Tone},
    commands::Outcome,
    jobs::Jobs,
    manifest::{Grants, Manifest, Permission},
    sandbox::{self, Bindings, Context, Limits, Log, PluginPre, Role, Sandbox, Services, State},
    storage::{DEFAULT_QUOTA, Storage},
    ui,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::ThreadId,
};
use wasmtime::{Store, component::Component};

pub use sandbox::bindings::angelica::plugin::ui::{Event, Window};

/// Where the host keeps what it persists.
#[derive(Debug, Clone)]
pub struct Directories {
    pub grants: PathBuf,
    pub state: PathBuf,
    pub storage: PathBuf,
}

/// A plugin's state as the menu shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Installed; the player has not approved it.
    AwaitingApproval,
    /// Approved and turned off by the player.
    Disabled,
    Running,
    /// Stopped because it failed; it stays off until the player retries.
    Faulted(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    format: u32,
    enabled: bool,
}

struct Running {
    store: Store<State>,
    bindings: Bindings,
}

pub struct Plugin {
    pub manifest: Arc<Manifest>,
    pre: PluginPre<State>,
    pub storage: Arc<Storage>,
    pub log: Arc<Log>,
    grants: Mutex<Option<Grants>>,
    /// The player's earlier approval, also when it does not cover this
    /// version.
    granted: Option<Grants>,
    /// The player's choice, kept across game starts.
    enabled: AtomicBool,
    status: Mutex<Status>,
    running: Mutex<Option<Running>>,
    jobs: Mutex<Option<Arc<Jobs>>>,
    windows: Mutex<Vec<Window>>,
    /// When the menu was last told that the plugin held up a frame.
    slow: Mutex<Option<std::time::Instant>>,
}

/// What a frame brings every plugin.
#[derive(Clone, Copy)]
struct Frame<'a> {
    elapsed: f64,
    commands: &'a [(String, Outcome)],
    game_events: &'a (Vec<crate::events::GameEvent>, u32),
    events: &'a [(String, Event)],
}

/// A plugin whose calls take longer in one frame is noted in the menu.
const SLOW_FRAME: std::time::Duration = std::time::Duration::from_millis(100);
const SLOW_NOTE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

impl Plugin {
    #[must_use]
    pub fn status(&self) -> Status {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[must_use]
    pub fn windows(&self) -> Vec<Window> {
        self.windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// What the player is asked to approve, by class: with an earlier
    /// approval, only what it does not cover.
    #[must_use]
    pub fn requests(&self) -> Vec<(crate::manifest::Class, Vec<crate::manifest::Request>)> {
        crate::manifest::requests(&self.manifest, self.granted.as_ref())
    }

    /// Whether the player approved an earlier version of the plugin.
    #[must_use]
    pub fn approved_before(&self) -> bool {
        self.granted
            .as_ref()
            .is_some_and(|grants| grants.plugin_id == self.manifest.id())
    }

    fn set_status(&self, status: Status) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
    }
}

/// What changes on disk while the game runs: plugin packages and the game
/// map. Called every frame on the main thread; an implementation looks at
/// the disk as often as it sees fit.
pub trait Updates: Send {
    /// The packages that changed since the last call: each authenticated
    /// and loaded, or the reason it could not be.
    fn packages(&mut self) -> Vec<Result<(Manifest, wasmtime::component::Component), String>>;

    /// When the game map changed since the last call, what reloading it
    /// did, or why it could not be reloaded.
    fn map(&mut self) -> Option<Result<String, String>> {
        None
    }
}

/// A player action from the menu, applied on the main thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Approve(String),
    Enable(String),
    Disable(String),
    OpenFolder(String),
}

pub struct Host {
    sandbox: Arc<Sandbox>,
    limits: Limits,
    services: Arc<Services>,
    directories: Directories,
    plugins: std::sync::RwLock<BTreeMap<String, Arc<Plugin>>>,
    /// What changes on disk while the game runs.
    updates: Mutex<Option<Box<dyn Updates>>>,
    main_thread: ThreadId,
    actions: Mutex<VecDeque<Action>>,
    events: Mutex<VecDeque<(String, Event)>>,
    diagnostics: Mutex<VecDeque<String>>,
    log_sink: Arc<dyn Fn(&str) + Send + Sync>,
    opener: Box<dyn Fn(&Path) + Send + Sync>,
}

impl Host {
    /// A host whose main thread is the calling thread.
    #[must_use]
    pub fn new(
        sandbox: Arc<Sandbox>,
        limits: Limits,
        services: Arc<Services>,
        directories: Directories,
        log_sink: Arc<dyn Fn(&str) + Send + Sync>,
        opener: Box<dyn Fn(&Path) + Send + Sync>,
    ) -> Self {
        Self {
            sandbox,
            limits,
            services,
            directories,
            plugins: std::sync::RwLock::new(BTreeMap::new()),
            updates: Mutex::new(None),
            main_thread: std::thread::current().id(),
            actions: Mutex::new(VecDeque::new()),
            events: Mutex::new(VecDeque::new()),
            diagnostics: Mutex::new(VecDeque::new()),
            log_sink,
            opener,
        }
    }

    /// The engine components must be compiled with.
    #[must_use]
    pub fn engine(&self) -> &wasmtime::Engine {
        &self.sandbox.engine
    }

    #[must_use]
    pub fn services(&self) -> &Arc<Services> {
        &self.services
    }

    pub fn diagnostic(&self, message: impl Into<String>) {
        let message = message.into();
        (self.log_sink)(&message);
        let mut diagnostics = self
            .diagnostics
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if diagnostics.len() == 64 {
            diagnostics.pop_front();
        }
        diagnostics.push_back(message);
    }

    #[must_use]
    pub fn diagnostics(&self) -> Vec<String> {
        self.diagnostics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn on_main_thread(&self) -> bool {
        std::thread::current().id() == self.main_thread
    }

    /// Every plugin, sorted by id.
    #[must_use]
    pub fn plugins(&self) -> Vec<Arc<Plugin>> {
        self.plugins
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn plugin(&self, id: &str) -> Option<Arc<Plugin>> {
        self.plugins
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    /// Reloads what `updates` reports as changed, in later frames.
    pub fn watch(&self, updates: Box<dyn Updates>) {
        *self.updates.lock().unwrap_or_else(PoisonError::into_inner) = Some(updates);
    }

    /// Registers an authenticated package. It starts with [`Host::start_all`]
    /// when the player approved it and left it enabled.
    ///
    /// # Errors
    /// Refuses a repeated id, imports the host does not provide, and an
    /// unreadable grant or settings file.
    pub fn install(&mut self, manifest: Manifest, component: &Component) -> Result<(), String> {
        let id = manifest.id().to_owned();
        if self.plugin(&id).is_some() {
            return Err(format!("{id}: installed twice"));
        }
        let plugin = self.create(manifest, component)?;
        self.plugins
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, plugin);
        Ok(())
    }

    /// Replaces a plugin's package with a new one, or adds a new plugin,
    /// while the game runs. The new package is prepared first; when that
    /// fails, the old one keeps running. The old one is stopped as when the
    /// player disables it, and the new one starts when the player approved
    /// what it declares and left the plugin enabled; one that declares more
    /// waits for approval. Called on the main thread.
    ///
    /// # Errors
    /// As [`Host::install`] for the new package; a package from the
    /// catalog in place of one from its author, or the reverse.
    pub fn replace(&self, manifest: Manifest, component: &Component) -> Result<(), String> {
        let id = manifest.id().to_owned();
        let version = manifest.plugin.version.clone();
        if let Some(old) = self.plugin(&id)
            && old.manifest.origin != manifest.origin
        {
            return Err(format!(
                "{id}: the new package comes from elsewhere than the installed one"
            ));
        }
        let fresh = self.create(manifest, component)?;
        if let Some(old) = self.plugin(&id)
            && old.status() == Status::Running
        {
            self.stop(&old, Status::Disabled);
        }
        self.plugins
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.clone(), fresh.clone());
        if fresh.status() == Status::Disabled && fresh.enabled.load(Ordering::Acquire) {
            self.start(&fresh);
        }
        self.diagnostic(format!("{id}: loaded version {version}"));
        Ok(())
    }

    fn create(&self, manifest: Manifest, component: &Component) -> Result<Arc<Plugin>, String> {
        let id = manifest.id().to_owned();
        let pre = self
            .sandbox
            .prepare(component)
            .map_err(|e| format!("{id}: {e}"))?;
        let grants = match std::fs::read(self.directories.grants.join(format!("{id}.json"))) {
            Ok(bytes) => Some(
                serde_json::from_slice::<Grants>(&bytes)
                    .map_err(|e| format!("{id}: grants: {e}"))?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{id}: grants: {e}")),
        };
        let enabled = match std::fs::read(self.directories.state.join(format!("{id}.json"))) {
            Ok(bytes) => {
                let settings: Settings =
                    serde_json::from_slice(&bytes).map_err(|e| format!("{id}: settings: {e}"))?;
                if settings.format != 1 {
                    return Err(format!(
                        "{id}: settings format {} is unsupported",
                        settings.format
                    ));
                }
                settings.enabled
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => return Err(format!("{id}: settings: {e}")),
        };
        let approved = grants.as_ref().is_some_and(|g| g.cover(&manifest));
        let storage = Arc::new(
            Storage::new(&self.directories.storage.join(&id), DEFAULT_QUOTA)
                .map_err(|e| format!("{id}: {e}"))?,
        );
        let sink = self.log_sink.clone();
        let prefix = id.clone();
        let log = Arc::new(Log::new(Box::new(move |line| {
            sink(&format!("[{prefix}] {line}"));
        })));
        let status = if approved {
            Status::Disabled
        } else {
            Status::AwaitingApproval
        };
        let plugin = Arc::new(Plugin {
            manifest: Arc::new(manifest),
            pre,
            storage,
            log,
            granted: grants.clone(),
            grants: Mutex::new(grants.filter(|_| approved)),
            enabled: AtomicBool::new(enabled),
            status: Mutex::new(status),
            running: Mutex::new(None),
            jobs: Mutex::new(None),
            windows: Mutex::new(Vec::new()),
            slow: Mutex::new(None),
        });
        Ok(plugin)
    }

    /// Starts every approved and enabled plugin, before the game's code
    /// runs: `runtime.started-with-game` is true during these starts.
    pub fn start_all(&self) {
        self.services.starting.store(true, Ordering::Release);
        for plugin in self.plugins() {
            if plugin.status() == Status::Disabled && plugin.enabled.load(Ordering::Acquire) {
                self.start(&plugin);
            }
        }
        self.services.starting.store(false, Ordering::Release);
    }

    fn start(&self, plugin: &Arc<Plugin>) {
        let id = plugin.manifest.id().to_owned();
        let Some(grants) = plugin
            .grants
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        else {
            plugin.set_status(Status::AwaitingApproval);
            return;
        };
        let jobs = Arc::new(Jobs::new(
            self.sandbox.clone(),
            plugin.pre.clone(),
            self.limits.job_time,
        ));
        let context = Arc::new(Context {
            id: id.clone(),
            name: plugin.manifest.plugin.name.clone(),
            permissions: plugin
                .manifest
                .permissions()
                .intersection(&grants.permissions)
                .copied()
                .collect(),
            memory_bytes: plugin
                .manifest
                .plugin
                .memory_bytes
                .unwrap_or(self.limits.memory_bytes),
            storage: plugin.storage.clone(),
            services: self.services.clone(),
            log: plugin.log.clone(),
            jobs: jobs.clone(),
            http: grants.permissions.contains(&Permission::HttpGet).then(|| {
                Arc::new(crate::http::Client::new(
                    plugin.manifest.http_hosts().into_iter().collect(),
                    &id,
                    &plugin.manifest.plugin.version,
                ))
            }),
        });
        let mut store = sandbox::store(&self.sandbox, context, Role::Main);
        sandbox::budget(
            &mut store,
            self.limits.start_time,
            self.limits.start_wall,
            self.limits.fuel.saturating_mul(100),
        );
        let result = sandbox::call(&mut store, |store| {
            let bindings = plugin
                .pre
                .instantiate(&mut *store)
                .map_err(|e| sandbox::describe_trap(store, &e))?;
            match bindings.angelica_plugin_guest().call_start(&mut *store) {
                Ok(Ok(())) => Ok(bindings),
                Ok(Err(reason)) => Err(format!("start failed: {reason}")),
                Err(trap) => Err(format!(
                    "start trapped: {}",
                    sandbox::describe_trap(store, &trap)
                )),
            }
        });
        match result {
            Ok(bindings) => {
                *plugin.jobs.lock().unwrap_or_else(PoisonError::into_inner) = Some(jobs);
                *plugin
                    .running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(Running { store, bindings });
                plugin.set_status(Status::Running);
            }
            Err(reason) => {
                jobs.cancel_all();
                self.remove_effects(&id);
                self.diagnostic(format!("{id}: {reason}"));
                plugin.set_status(Status::Faulted(reason));
            }
        }
    }

    /// Stops a running plugin: its `stop`, then removal of its tables,
    /// letters, jobs, and windows.
    fn stop(&self, plugin: &Plugin, status: Status) {
        let running = plugin
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(mut running) = running
            && !matches!(status, Status::Faulted(_))
        {
            sandbox::budget(
                &mut running.store,
                self.limits.start_time,
                self.limits.start_wall,
                self.limits.fuel.saturating_mul(100),
            );
            let guest = running.bindings.angelica_plugin_guest();
            if let Err(trap) = sandbox::call(&mut running.store, |store| guest.call_stop(store)) {
                self.diagnostic(format!(
                    "{}: stop trapped: {}",
                    plugin.manifest.id(),
                    sandbox::describe_trap(&running.store, &trap)
                ));
            }
        }
        if let Some(jobs) = plugin
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            jobs.cancel_all();
        }
        self.remove_effects(plugin.manifest.id());
        plugin
            .windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        plugin.set_status(status);
    }

    fn remove_effects(&self, id: &str) {
        self.services.excel.remove_plugin(id);
        self.services.case.remove_plugin(id);
        self.services.files.remove_plugin(id);
        self.services.commands.remove_plugin(id);
        self.services.chat.remove_plugin(id);
        self.services.events.remove_plugin(id);
    }

    fn fault(&self, plugin: &Plugin, reason: String) {
        self.diagnostic(format!("{}: disabled: {reason}", plugin.manifest.id()));
        self.stop(plugin, Status::Faulted(reason));
    }

    /// Stops every plugin, at game exit.
    pub fn stop_all(&self) {
        for plugin in self.plugins() {
            if plugin.status() == Status::Running {
                self.stop(&plugin, Status::Disabled);
            }
        }
    }

    /// The id, version, and state of every plugin, without waiting on a
    /// lock another thread may hold; for crash reports.
    #[must_use]
    pub fn crash_snapshot(&self) -> Vec<(String, String, String)> {
        let Ok(plugins) = self.plugins.try_read() else {
            return Vec::new();
        };
        plugins
            .values()
            .map(|plugin| {
                let status = plugin
                    .status
                    .try_lock()
                    .map_or_else(|_| "unknown".into(), |status| format!("{:?}", *status));
                (
                    plugin.manifest.id().to_owned(),
                    plugin.manifest.plugin.version.clone(),
                    status,
                )
            })
            .collect()
    }

    /// Queues a player action; it applies during the next frame.
    pub fn queue(&self, action: Action) {
        self.actions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(action);
    }

    /// Queues a player action in a plugin window.
    pub fn queue_event(&self, plugin: &str, event: Event) {
        let mut events = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        if events.len() < 256 {
            events.push_back((plugin.into(), event));
        }
    }

    /// Runs one frame on the main thread: queued actions, then for every
    /// running plugin `tick`, the commands the player ran, queued window
    /// events, and `ui`.
    pub fn frame(&self, elapsed: f64) {
        if !self.on_main_thread() {
            self.diagnostic("a frame was requested outside the game's main thread");
            return;
        }
        let (map, changed) = self
            .updates
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .map(|updates| (updates.map(), updates.packages()))
            .unwrap_or_default();
        match map {
            Some(Ok(note)) => self.diagnostic(note),
            Some(Err(reason)) => self.diagnostic(format!("game map not reloaded: {reason}")),
            None => {}
        }
        for package in changed {
            if let Err(reason) =
                package.and_then(|(manifest, component)| self.replace(manifest, &component))
            {
                self.diagnostic(format!("reload: {reason}"));
            }
        }
        let actions: Vec<_> = self
            .actions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        for action in actions {
            if let Err(reason) = self.apply(&action) {
                self.diagnostic(reason);
            }
        }
        let events: Vec<_> = self
            .events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        let mut commands: Vec<_> = self
            .services
            .commands
            .take()
            .iter()
            .filter_map(|line| self.services.commands.resolve(line))
            .collect();
        commands.extend(self.services.commands.take_overflowed());
        let game_events = self.services.events.take();
        let frame = Frame {
            elapsed,
            commands: &commands,
            game_events: &game_events,
            events: &events,
        };
        for plugin in self.plugins() {
            self.frame_plugin(&plugin, &frame);
        }
    }

    fn frame_plugin(&self, plugin: &Arc<Plugin>, frame: &Frame<'_>) {
        let mut running = plugin
            .running
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(instance) = running.as_mut() else {
            return;
        };
        // Every call of the frame shares one budget.
        sandbox::budget(
            &mut instance.store,
            self.limits.call_time,
            self.limits.frame_wall,
            self.limits.fuel,
        );
        let fault = self.frame_calls(plugin, instance, frame);
        let spent = sandbox::spent(&instance.store);
        drop(running);
        if let Some(reason) = fault {
            self.fault(plugin, reason);
        } else if spent > SLOW_FRAME {
            self.slow(plugin, spent);
        }
    }

    /// The plugin's calls of a frame; the reason to fault it, if any.
    fn frame_calls(
        &self,
        plugin: &Plugin,
        instance: &mut Running,
        frame: &Frame<'_>,
    ) -> Option<String> {
        let id = plugin.manifest.id();
        let Frame {
            elapsed,
            commands,
            game_events,
            events,
        } = *frame;
        let guest = instance.bindings.angelica_plugin_guest();
        let store = &mut instance.store;
        match sandbox::call(store, |store| guest.call_tick(store, elapsed)) {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => return Some(format!("tick failed: {reason}")),
            Err(trap) => {
                return Some(format!(
                    "tick trapped: {}",
                    sandbox::describe_trap(store, &trap)
                ));
            }
        }
        for (_, outcome) in commands.iter().filter(|(target, _)| target == id) {
            let invocation = match outcome {
                Outcome::Reply(spans) => {
                    self.reply(store.data().context(), spans);
                    continue;
                }
                Outcome::Run(invocation) => sandbox::invocation_to_api(invocation.clone()),
            };
            match sandbox::call(store, |store| guest.call_command(store, &invocation)) {
                Ok(Ok(())) => {}
                Ok(Err(reason)) => {
                    self.reply(store.data().context(), &[Span::new(reason, Tone::Bad)]);
                }
                Err(trap) => {
                    return Some(format!(
                        "command trapped: {}",
                        sandbox::describe_trap(store, &trap)
                    ));
                }
            }
        }
        let subscribed = self.services.events.subscribed(id);
        if !subscribed.is_empty() {
            let (delivered, dropped) = game_events;
            let mut calls: Vec<_> = Vec::new();
            if *dropped > 0 {
                calls.push(
                    sandbox::bindings::angelica::plugin::events::GameEvent::Dropped(*dropped),
                );
            }
            calls.extend(
                delivered
                    .iter()
                    .filter(|event| subscribed.contains(&event.kind()))
                    .cloned()
                    .map(sandbox::event_to_api),
            );
            for event in &calls {
                if let Err(trap) =
                    sandbox::call(store, |store| guest.call_on_game_event(store, event))
                {
                    return Some(format!(
                        "on-game-event trapped: {}",
                        sandbox::describe_trap(store, &trap)
                    ));
                }
            }
        }
        for (_, event) in events.iter().filter(|(target, _)| target == id) {
            if let Err(trap) = sandbox::call(store, |store| guest.call_ui_event(store, event)) {
                return Some(format!(
                    "ui-event trapped: {}",
                    sandbox::describe_trap(store, &trap)
                ));
            }
        }
        match sandbox::call(store, |store| guest.call_ui(store)) {
            Ok(windows) => match ui::validate(&windows) {
                Ok(()) => {
                    *plugin
                        .windows
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = windows;
                }
                Err(reason) => return Some(format!("invalid windows: {reason}")),
            },
            Err(trap) => {
                return Some(format!(
                    "ui trapped: {}",
                    sandbox::describe_trap(store, &trap)
                ));
            }
        }
        None
    }

    /// Notes that a plugin's calls held up a frame, at most once every few
    /// seconds per plugin.
    fn slow(&self, plugin: &Plugin, spent: std::time::Duration) {
        let mut noted = plugin.slow.lock().unwrap_or_else(PoisonError::into_inner);
        if noted.is_some_and(|at| at.elapsed() < SLOW_NOTE_INTERVAL) {
            return;
        }
        *noted = Some(std::time::Instant::now());
        drop(noted);
        self.diagnostic(format!(
            "{}: its calls held up a frame for {} ms",
            plugin.manifest.id(),
            spent.as_millis()
        ));
    }

    /// Prints the host's answer to a plugin's command in the chat log, as
    /// the plugin's message: help, a usage error, or the error the command
    /// returned. Text past the message limit is cut.
    fn reply(&self, context: &Context, spans: &[Span]) {
        let mut kept = Vec::new();
        let mut left = crate::chat::MAX_TEXT;
        for span in spans {
            let text: String = span
                .text
                .chars()
                .filter(|&c| c == '\n' || !c.is_control())
                .collect();
            if text.len() > left {
                kept.push(Span::new("...", Tone::Muted));
                break;
            }
            left -= text.len();
            kept.push(Span {
                text,
                ..span.clone()
            });
        }
        let message = crate::chat::encode(&context.name, &kept);
        if let Err(reason) = self.services.game.print(&message) {
            self.diagnostic(format!(
                "{}: {reason}: {}",
                context.id,
                crate::chat::plain(&message)
            ));
        }
    }

    fn apply(&self, action: &Action) -> Result<(), String> {
        let id = match action {
            Action::Approve(id)
            | Action::Enable(id)
            | Action::Disable(id)
            | Action::OpenFolder(id) => id,
        };
        let plugin = self
            .plugin(id)
            .ok_or_else(|| format!("{id}: not installed"))?;
        let plugin = &plugin;
        match action {
            Action::Approve(_) => {
                let grants = Grants::for_manifest(&plugin.manifest);
                write_json(&self.directories.grants, id, &grants)?;
                *plugin.grants.lock().unwrap_or_else(PoisonError::into_inner) = Some(grants);
                self.set_enabled(plugin, true)?;
                self.start(plugin);
            }
            Action::Enable(_) => {
                if plugin.status() != Status::Running {
                    self.set_enabled(plugin, true)?;
                    self.start(plugin);
                }
            }
            Action::Disable(_) => {
                self.set_enabled(plugin, false)?;
                if plugin.status() == Status::Running {
                    self.stop(plugin, Status::Disabled);
                } else if matches!(plugin.status(), Status::Faulted(_)) {
                    plugin.set_status(Status::Disabled);
                }
            }
            Action::OpenFolder(_) => (self.opener)(plugin.storage.root()),
        }
        Ok(())
    }

    fn set_enabled(&self, plugin: &Plugin, enabled: bool) -> Result<(), String> {
        plugin.enabled.store(enabled, Ordering::Release);
        write_json(
            &self.directories.state,
            plugin.manifest.id(),
            &Settings { format: 1, enabled },
        )
    }
}

fn write_json(directory: &Path, id: &str, value: &impl Serialize) -> Result<(), String> {
    let path = directory.join(format!("{id}.json"));
    let temporary = directory.join(format!("{id}.json.tmp"));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(directory)
        .and_then(|()| std::fs::write(&temporary, bytes))
        .and_then(|()| std::fs::rename(&temporary, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}
