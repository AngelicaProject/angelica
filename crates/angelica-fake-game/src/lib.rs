//! A scripted game: the host's services and plugins run as in the game
//! process, and test code plays the game's part. Rows go through the same
//! [`SheetBinding`] the Windows adapter uses; letter case through the same
//! [`Letters`].
//!
//! [`SheetBinding`]: runtime::excel_text::SheetBinding
//! [`Letters`]: runtime::text_case::Letters
/// Column kinds and languages of a synthetic sheet ([`game_files`]).
pub use angelica_sqpack::excel::{ColumnKind, Language};
pub use angelica_sqpack::testing as game_files;
use angelica_text::table::Column;
pub use objects::{Fire, Set};

mod objects;
/// The host the tests run plugins in.
pub use runtime;
/// The windows and window events of the plugin API.
pub use runtime::sandbox::bindings::angelica::plugin::ui;
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
    pub commands_available: AtomicBool,
    /// False until the chat log exists.
    pub chat_available: AtomicBool,
    /// Every message printed in the chat log, as the game's text.
    pub printed: Mutex<Vec<Vec<u8>>>,
    /// The game's objects, laid out as the game map a test named
    /// describes them ([`TestHost::use_map`]).
    objects: Mutex<objects::Objects>,
    /// False when the hooks of watched events cannot be installed.
    pub watch_available: AtomicBool,
    /// The map entries hooked for watched events, in order.
    pub watched: Mutex<Vec<String>>,
    /// False when the game build lacks what rules for its windows need.
    pub windows_available: AtomicBool,
    /// The elements of the game's own windows, by window name and id.
    pub windows: Mutex<std::collections::BTreeMap<(String, u32), WindowElement>>,
    /// Every call of the game's functions an action made, in order: the
    /// function's map entry and each argument's 64 bits.
    pub acted: Mutex<Vec<(String, Vec<u64>)>>,
    /// What the game's functions return to actions: `1` (taken) unless a
    /// test says otherwise.
    pub act_returns: Mutex<u64>,
    /// The map entries hooked for decision points, in order.
    pub decided: Mutex<Vec<String>>,
    /// The game camera's view-projection matrix; none unless a test sets
    /// one.
    pub view_projection: Mutex<Option<[f32; 16]>>,
}

/// An element of one of the game's own windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowElement {
    /// Changes when the game builds the window again.
    pub handle: usize,
    pub visible: bool,
    /// The text of a text element; `None` for any other element.
    pub text: Option<Vec<u8>>,
}

struct ScriptedScreen<'a>(
    std::sync::MutexGuard<'a, std::collections::BTreeMap<(String, u32), WindowElement>>,
);

impl ScriptedScreen<'_> {
    fn at(&mut self, element: runtime::game_ui::Element) -> Option<&mut WindowElement> {
        self.0
            .values_mut()
            .find(|found| found.handle == element.handle)
    }
}

impl runtime::game_ui::Screen for ScriptedScreen<'_> {
    fn element(&mut self, window: &str, id: u32, text: bool) -> Option<runtime::game_ui::Element> {
        self.0
            .get(&(window.to_owned(), id))
            .filter(|element| element.text.is_some() || !text)
            .map(|element| runtime::game_ui::Element {
                handle: element.handle,
            })
    }

    fn visible(&mut self, element: runtime::game_ui::Element) -> bool {
        self.at(element).is_some_and(|element| element.visible)
    }

    fn set_visible(&mut self, element: runtime::game_ui::Element, visible: bool) {
        if let Some(element) = self.at(element) {
            element.visible = visible;
        }
    }

    fn text(&mut self, element: runtime::game_ui::Element) -> Vec<u8> {
        self.at(element)
            .and_then(|element| element.text.clone())
            .unwrap_or_default()
    }

    fn set_text(&mut self, element: runtime::game_ui::Element, text: &[u8]) {
        if let Some(element) = self.at(element) {
            element.text = Some(text.to_vec());
        }
    }
}

impl Default for ScriptedGame {
    fn default() -> Self {
        Self {
            version: "2026.09.15.0000.0000".into(),
            excel_available: AtomicBool::new(true),
            case_available: AtomicBool::new(true),
            files_available: AtomicBool::new(true),
            commands_available: AtomicBool::new(true),
            chat_available: AtomicBool::new(true),
            printed: Mutex::new(Vec::new()),
            objects: Mutex::new(objects::Objects::default()),
            watch_available: AtomicBool::new(true),
            watched: Mutex::new(Vec::new()),
            windows_available: AtomicBool::new(true),
            windows: Mutex::new(std::collections::BTreeMap::new()),
            acted: Mutex::new(Vec::new()),
            act_returns: Mutex::new(1),
            decided: Mutex::new(Vec::new()),
            view_projection: Mutex::new(None),
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

    fn enable_commands(&self) -> Result<(), String> {
        if self.commands_available.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("map entry shell.command_module.execute_command_inner is absent".into())
        }
    }

    fn enable_chat_messages(&self) -> Result<(), String> {
        if self.chat_available.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("map entry ui.log_module.print_message is absent".into())
        }
    }

    fn with_screen(
        &self,
        work: &mut dyn FnMut(&mut dyn runtime::game_ui::Screen),
    ) -> Result<(), String> {
        if !self.windows_available.load(Ordering::Acquire) {
            return Err("map entry ui.stage is absent".into());
        }
        work(&mut ScriptedScreen(
            self.windows.lock().unwrap_or_else(PoisonError::into_inner),
        ));
        Ok(())
    }

    fn query(
        &self,
        prepared: &runtime::lens::Prepared,
    ) -> runtime::error::Result<Vec<runtime::lens::Row>> {
        runtime::lens::run(
            &*self.objects.lock().unwrap_or_else(PoisonError::into_inner),
            prepared,
        )
    }

    fn memory(&self) -> &dyn runtime::lens::Memory {
        self
    }

    fn call(&self, address: u64, args: &[u64]) -> Result<runtime::lens::act::Returned, String> {
        let entry = self
            .objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .code
            .get(&address)
            .cloned()
            .ok_or_else(|| format!("nothing in the scripted game at {address:#x}"))?;
        self.acted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((entry, args.to_vec()));
        let returned = *self
            .act_returns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Ok(runtime::lens::act::Returned {
            integer: returned,
            float: 0,
        })
    }

    fn view_projection(&self) -> Option<[f32; 16]> {
        *self
            .view_projection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_data(address, bytes)
    }

    fn enable_decision(&self, _point: usize, entry: &str, _address: u64) -> Result<(), String> {
        self.decided
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry.to_owned());
        Ok(())
    }

    fn enable_watch(&self, _point: usize, entry: &str, _address: u64) -> Result<(), String> {
        if self.watch_available.load(Ordering::Acquire) {
            self.watched
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(entry.to_owned());
            Ok(())
        } else {
            Err(format!(
                "map entry {entry} cannot be hooked in the scripted build"
            ))
        }
    }

    fn print(&self, message: &[u8]) -> Result<(), String> {
        if !self.chat_available.load(Ordering::Acquire) {
            return Err("the chat log does not exist yet".into());
        }
        self.printed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message.to_vec());
        Ok(())
    }
}

impl runtime::lens::Memory for ScriptedGame {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .read(address, buffer)
    }
}

impl ScriptedGame {
    /// Sets `values` of the object `path` names in the game map a test
    /// named, making everything on the way that does not exist yet. An
    /// array's element is named by position, `[3]`, or by values,
    /// `[kind = bag_1]`: the first element with them, or else the first
    /// empty one, which gets them. A dotted value name goes through
    /// references and embedded objects.
    ///
    /// ```ignore
    /// test.game.set(
    ///     "inventory.containers[kind = bag_1].items[0]",
    ///     &[("item", 5057.into()), ("quantity", 12.into())],
    /// );
    /// ```
    ///
    /// # Panics
    /// When the path or a value does not fit the map.
    pub fn set(&self, path: &str, values: &[(&str, Set)]) {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set(path, values)
            .unwrap_or_else(|error| panic!("{path}: {error}"));
    }

    /// The game has no object at `root` now, as before the player logs in:
    /// the pointer its global holds is zero.
    ///
    /// # Panics
    /// When the root is unknown or not behind a pointer.
    pub fn clear(&self, root: &str) {
        self.objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear(root)
            .unwrap_or_else(|error| panic!("{root}: {error}"));
    }

    /// Forgets the printed messages.
    pub fn clear_printed(&self) {
        self.printed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    /// The printed messages as plain text, without colors.
    #[must_use]
    pub fn printed_text(&self) -> Vec<String> {
        self.printed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|message| runtime::chat::plain(message))
            .collect()
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
    limits: Limits,
    /// The packages installed with [`TestHost::install`], for a restart.
    installed: Vec<(String, Vec<u8>)>,
    /// The window schemas tests named, as the game map would hold them.
    window_schemas: Mutex<Vec<angelica_map::WindowSchema>>,
}

/// One frame at 60 frames a second.
const FRAME: f64 = 1.0 / 60.0;

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
            commands: Arc::new(runtime::commands::Commands::default()),
            chat: Arc::new(runtime::chat::Chat::default()),
            sheets: Arc::new(runtime::sheets::Sheets::default()),
            lens: Arc::new(runtime::lens::Lens::default()),
            events: Arc::new(runtime::events::Events::default()),
            ui: Arc::new(runtime::game_ui::GameUi::default()),
            draw: Arc::new(runtime::draw::Drawings::default()),
            messages: Arc::new(runtime::messages::Messages::default()),
            game: game.clone(),
            starting: AtomicBool::new(false),
            russian: AtomicBool::new(false),
        });
        let opened = Arc::new(Mutex::new(Vec::new()));
        let recorded = opened.clone();
        let root = folder.path();
        let host = Host::new(
            Arc::new(Sandbox::new().expect("sandbox")),
            limits,
            services,
            Directories {
                plugins: root.join("plugins"),
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
            limits,
            installed: Vec::new(),
            window_schemas: Mutex::new(Vec::new()),
        }
    }

    /// The game restarts: a new host over the same folders (storage, grants,
    /// settings, game files), with the same packages installed, starts the
    /// approved and enabled plugins as at the game's start. The scripted
    /// game starts empty again.
    ///
    /// # Errors
    /// When a package is refused.
    pub fn restart(self) -> Result<Self, String> {
        let mut test = Self::in_folder(self.folder, self.limits);
        for (manifest, component) in self.installed {
            test.install(&manifest, &component)?;
        }
        test.host.start_all();
        Ok(test)
    }

    /// Installs a component built from a package's `angelica.toml` and
    /// component bytes.
    ///
    /// # Errors
    /// When the manifest or component is refused.
    pub fn install(&mut self, manifest: &str, component: &[u8]) -> Result<(), String> {
        let parsed = Manifest::parse(manifest).map_err(|e| e.to_string())?;
        let compiled = wasmtime::component::Component::new(&self.host_engine(), component)
            .map_err(|e| e.to_string())?;
        self.host.install(parsed, &compiled)?;
        self.installed
            .push((manifest.to_owned(), component.to_vec()));
        Ok(())
    }

    /// A new package of a plugin appears while the game runs: the host
    /// loads it in the next frame, which this runs, as it does with a
    /// package written to the plugin directory.
    ///
    /// # Errors
    /// An invalid manifest or component; the host's own refusals are
    /// diagnostics, as in the game.
    pub fn reload(&self, manifest: &str, component: &[u8]) -> Result<(), String> {
        struct Once(Option<(Manifest, wasmtime::component::Component)>);
        impl runtime::host::Updates for Once {
            fn packages(
                &mut self,
            ) -> Vec<Result<(Manifest, wasmtime::component::Component), String>> {
                self.0.take().into_iter().map(Ok).collect()
            }
        }
        let manifest = Manifest::parse(manifest).map_err(|e| e.to_string())?;
        let component = wasmtime::component::Component::new(&self.host_engine(), component)
            .map_err(|e| e.to_string())?;
        self.host.watch(Box::new(Once(Some((manifest, component)))));
        self.host.frame(FRAME);
        Ok(())
    }

    fn host_engine(&self) -> wasmtime::Engine {
        self.host.engine().clone()
    }

    /// Runs `count` frames of 1/60 second each.
    pub fn frames(&self, count: usize) {
        for _ in 0..count {
            self.host.frame(FRAME);
        }
    }

    /// Runs one frame in which `elapsed` seconds passed, so a plugin that
    /// sums `tick`'s `elapsed` sees that much time go by.
    pub fn advance(&self, elapsed: f64) {
        self.host.frame(elapsed);
    }

    /// Runs frames, sleeping between them, until `done` holds or ten
    /// seconds pass, for work in jobs; whether `done` held.
    pub fn wait_until(&self, done: impl Fn(&Self) -> bool) -> bool {
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(10) {
            if done(self) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            self.host.frame(FRAME);
        }
        done(self)
    }

    /// The player clicks a button; the frame runs.
    pub fn click(&self, plugin: &str, window: &str, id: &str) {
        self.event(
            plugin,
            Event::Clicked(ui::WidgetRef {
                window: window.into(),
                id: id.into(),
            }),
        );
    }

    /// The player types `value` into a text input; the frame runs.
    pub fn edit(&self, plugin: &str, window: &str, id: &str, value: &str) {
        self.event(
            plugin,
            Event::Edited(ui::Edited {
                window: window.into(),
                id: id.into(),
                value: value.into(),
            }),
        );
    }

    /// The player sets a checkbox; the frame runs.
    pub fn toggle(&self, plugin: &str, window: &str, id: &str, checked: bool) {
        self.event(
            plugin,
            Event::Toggled(ui::Toggled {
                window: window.into(),
                id: id.into(),
                checked,
            }),
        );
    }

    /// The player picks the option at `index` of a choice; the frame runs.
    pub fn choose(&self, plugin: &str, window: &str, id: &str, index: u32) {
        self.event(
            plugin,
            Event::Chosen(ui::Chosen {
                window: window.into(),
                id: id.into(),
                index,
            }),
        );
    }

    /// The player sets a number field or slider; the frame runs.
    pub fn set_number(&self, plugin: &str, window: &str, id: &str, value: f64) {
        self.event(
            plugin,
            Event::Set(ui::NumberSet {
                window: window.into(),
                id: id.into(),
                value,
            }),
        );
    }

    /// The headings and texts of a plugin's windows as last drawn, in order;
    /// empty when it is not installed.
    #[must_use]
    pub fn texts(&self, plugin: &str) -> Vec<String> {
        self.host
            .plugin(plugin)
            .map(|plugin| plugin.windows())
            .unwrap_or_default()
            .iter()
            .flat_map(|window| &window.widgets)
            .filter_map(|widget| match widget {
                ui::Widget::Heading(text) => Some(text.clone()),
                ui::Widget::Text(text) => Some(text.text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The game adds a message to the chat log: plugins that subscribed
    /// receive it in the next frame, which this runs.
    pub fn chat_message(&self, info: u16, sender: &str, message: &str) {
        let events = &self.services().events;
        if events.wanted(runtime::events::EventKind::ChatMessage) {
            events.push(runtime::events::GameEvent::ChatMessage(
                runtime::events::ChatMessage {
                    info,
                    sender: sender.as_bytes().to_vec(),
                    message: message.as_bytes().to_vec(),
                    timestamp: 1_800_000_000,
                },
            ));
        }
        self.host.frame(FRAME);
    }

    /// The game calls the function of the decision point `event` with
    /// `args`: what the plugins' rules make it return at once, or `None`
    /// when it runs. Watches of the event are fired too.
    ///
    /// # Panics
    /// When the event or an argument does not fit the map.
    pub fn decide(&self, event: &str, args: &[(&str, Fire)]) -> Option<u64> {
        let services = self.services();
        let (schema, _) = services.lens.schema();
        let at = schema.objects.events.get(event)?.at.clone();
        let decided = services.lens.decisions.point_of(&at).and_then(|point| {
            let mut objects = self
                .game
                .objects
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let call = objects
                .call(event, args)
                .unwrap_or_else(|error| panic!("{event}: {error}"));
            services.lens.decisions.decide(point, &call, &*objects)
        });
        if decided.is_none() {
            self.fire(event, args);
        }
        decided
    }

    /// The game reaches the point of `event` with `args`: every plugin's
    /// watch of the event reads its rows now, from the scripted game's
    /// objects, and receives them in the next frame. Nothing happens when
    /// no plugin watches it.
    ///
    /// # Panics
    /// When the event or an argument does not fit the map.
    pub fn fire(&self, event: &str, args: &[(&str, Fire)]) {
        let services = self.services();
        let watches = &services.lens.watches;
        let (schema, _) = services.lens.schema();
        let Some(point) = schema
            .objects
            .events
            .get(event)
            .and_then(|found| watches.point_of(&found.at))
        else {
            return;
        };
        let mut objects = self
            .game
            .objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let call = objects
            .call(event, args)
            .unwrap_or_else(|error| panic!("{event}: {error}"));
        watches.fire(point, &call, &*objects);
    }

    /// The game map's object model for Lens: `files` as a map's
    /// `entries/`, `types/`, `enums/`, `domains.toml`, and `roots.toml`
    /// files hold them, with constant field offsets. Entries in `absent`
    /// are absent in the scripted build. The game's objects start empty;
    /// [`ScriptedGame::set`] fills them.
    ///
    /// # Panics
    /// When the files are not a valid map.
    pub fn use_map(&self, files: &[(&str, &str)], absent: &[&str]) {
        let files: Vec<(String, String)> = files
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect();
        let map =
            angelica_map::Map::from_sources(&format!("format = {}", angelica_map::FORMAT), &files)
                .unwrap_or_else(|error| panic!("the test's map: {error}"));
        let (objects, schema) = objects::Objects::new(map, absent);
        *self
            .game
            .objects
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = objects;
        self.services().lens.replace(schema);
    }

    /// Names columns of a sheet of the game in `game_folder`, as a game map
    /// schema that knows the sheet's layout there would.
    ///
    /// # Panics
    /// When the game has no such sheet or no column at an index.
    pub fn name_columns(&self, sheet: &str, names: &[(&str, u16)]) {
        let services = self.services();
        let layout = services
            .data
            .sheet(sheet)
            .unwrap_or_else(|error| panic!("sheet {sheet}: {}", error.message))
            .layout();
        let columns = names
            .iter()
            .map(|(name, index)| ((*name).to_owned(), *index))
            .collect::<std::collections::BTreeMap<_, _>>();
        services.sheets.insert(angelica_map::SheetSchema {
            sheet: sheet.to_owned(),
            doc: "A test schema.".into(),
            columns: columns
                .iter()
                .map(|(name, index)| {
                    (
                        name.clone(),
                        angelica_map::NamedColumn {
                            column_type: layout.columns[usize::from(*index)].0,
                            doc: "A test column.".into(),
                        },
                    )
                })
                .collect(),
            layouts: vec![angelica_map::KnownLayout {
                fingerprint: layout.fingerprint(),
                columns,
            }],
        });
    }

    /// Names elements of the game's window `window` as a game map would,
    /// for a layout file this writes nothing for: the game's files must
    /// already have `ui/uld/<layout>.uld`. Each element is a name, its id,
    /// and whether it shows text.
    ///
    /// # Panics
    /// When the game's files lack the layout file.
    pub fn name_elements(&self, window: &str, layout: &str, names: &[(&str, u32, bool)]) {
        let services = self.services();
        let path = format!("ui/uld/{layout}.uld");
        let file = services
            .data
            .read(&path)
            .unwrap_or_else(|error| panic!("{path}: {}", error.message));
        let mut schemas: Vec<angelica_map::WindowSchema> = self
            .window_schemas
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        schemas.retain(|schema| schema.window != window);
        schemas.push(angelica_map::WindowSchema {
            window: window.to_owned(),
            doc: "A test schema.".into(),
            layout: path,
            elements: names
                .iter()
                .map(|(name, _, text)| {
                    (
                        (*name).to_owned(),
                        angelica_map::NamedElement {
                            kind: if *text {
                                angelica_map::ElementKind::Text
                            } else {
                                angelica_map::ElementKind::Other
                            },
                            doc: "A test element.".into(),
                        },
                    )
                })
                .collect(),
            layouts: vec![angelica_map::KnownWindowLayout {
                fingerprint: angelica_map::window_layout_fingerprint(&file),
                elements: names
                    .iter()
                    .map(|(name, id, _)| ((*name).to_owned(), *id))
                    .collect(),
            }],
        });
        services.ui.set(schemas.clone());
        *self
            .window_schemas
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = schemas;
    }

    /// The game builds an element of one of its windows: visible, with
    /// `text` when it is a text element. Building it again gives it a new
    /// handle, as when the game opens the window again.
    pub fn build_element(&self, window: &str, id: u32, text: Option<&str>) {
        let mut windows = self
            .game
            .windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let handle = windows
            .values()
            .map(|element| element.handle)
            .max()
            .unwrap_or(0)
            + 1;
        windows.insert(
            (window.to_owned(), id),
            WindowElement {
                handle,
                visible: true,
                text: text.map(|text| text.as_bytes().to_vec()),
            },
        );
    }

    /// An element of one of the game's windows as it is now.
    ///
    /// # Panics
    /// When the game has not built the element.
    #[must_use]
    pub fn element(&self, window: &str, id: u32) -> WindowElement {
        self.game
            .windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(window.to_owned(), id))
            .unwrap_or_else(|| panic!("no element {id} in {window}"))
            .clone()
    }

    /// The game writes a text element's text again.
    ///
    /// # Panics
    /// When the game has not built the element.
    pub fn game_writes(&self, window: &str, id: u32, text: &str) {
        self.game
            .windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&(window.to_owned(), id))
            .unwrap_or_else(|| panic!("no element {id} in {window}"))
            .text = Some(text.as_bytes().to_vec());
    }

    /// The player sends `line` from the chat box: a plugin's command runs in
    /// the next frame, which this runs; false when the game would handle
    /// the line itself.
    pub fn command(&self, line: &str) -> bool {
        let taken = self.services().commands.submit(line.as_bytes());
        self.host.frame(FRAME);
        taken
    }

    pub fn event(&self, plugin: &str, event: Event) {
        self.host.queue_event(plugin, event);
        self.host.frame(FRAME);
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
