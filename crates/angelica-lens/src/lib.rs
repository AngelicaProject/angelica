//! Lens as a library in the plugin: queries over the game map's objects,
//! watches of its events, rules at its decision points, changes, and
//! actions, by name, as plugin API 1's Lens offers them, built on the
//! core's primitives. A query runs through handles and the core's checked
//! reads; a watch and a rule run in the plugin's intercept instance at the
//! event's function; a change and an action go through the core, which
//! keeps their guarantees: only the changes the game map offers, put back
//! when the plugin stops, and actions as the game's own interface takes
//! them, with their conditions, their rate, and the player's stop key.
//!
//! The core checks every read against the domains the player granted; the
//! library has no privilege of its own.
//!
//! A plugin that watches or decides passes its intercept instance's calls
//! on to the library:
//!
//! ```ignore
//! impl angelica_sdk::Plugin for Chat {
//!     fn start() -> Result<Self, String> {
//!         let watch = angelica_lens::Watch::new("chat_message_added { kind, message }")?;
//!         Ok(Self { watch })
//!     }
//!
//!     fn tick(&mut self, _: f64) -> Result<(), String> {
//!         for row in self.watch.take()? { /* … */ }
//!         Ok(())
//!     }
//!
//!     fn prepare() -> Result<(), String> {
//!         angelica_lens::prepare()
//!     }
//!
//!     fn intercepted(call: angelica_sdk::intercepts::Call) {
//!         angelica_lens::intercepted(&call);
//!     }
//! }
//! ```

mod run;
mod wire;

use angelica_query::plan::{self, Prepared};
use angelica_query::schema::Schema;
use angelica_sdk::api::intercepts as icept;
use angelica_sdk::{Error, ErrorKind};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

pub use angelica_sdk::api::lens::{Column, FlagSet, Row, SheetRow, Value};

/// The handler numbers the library takes, one per function it intercepts.
pub const HANDLERS: u32 = 0xF000_0000;

/// The game map as Lens plans with it, in this instance.
struct Model {
    map: angelica_map::Map,
    schema: Schema,
    /// The globals the schema numbers in place of addresses.
    globals: Vec<String>,
}

fn error(kind: ErrorKind, message: impl Into<String>) -> Error {
    Error {
        kind,
        message: message.into(),
    }
}

fn from_query(error: angelica_query::error::Error) -> Error {
    use angelica_query::error::Kind;
    let kind = match error.kind {
        Kind::InvalidArgument => ErrorKind::InvalidArgument,
        Kind::NotFound => ErrorKind::NotFound,
        Kind::PermissionDenied => ErrorKind::PermissionDenied,
        Kind::WrongContext => ErrorKind::WrongContext,
        Kind::Unavailable => ErrorKind::Unavailable,
        Kind::LimitExceeded => ErrorKind::LimitExceeded,
        Kind::Io => ErrorKind::Io,
        Kind::Network => ErrorKind::Network,
    };
    Error {
        kind,
        message: error.message,
    }
}

/// The model of the game map the host loaded, made once per instance.
fn model() -> Result<&'static Model, Error> {
    static MODEL: OnceLock<Result<Model, Error>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let mut files = angelica_sdk::api::game_map::files();
            if files.is_empty() {
                return Err(error(ErrorKind::Unavailable, "no game map is loaded"));
            }
            let (_, map_toml) = files.remove(0);
            let map = angelica_map::Map::from_sources(&map_toml, &files)
                .map_err(|e| error(ErrorKind::Unavailable, e.to_string()))?;
            let globals = RefCell::new(Vec::new());
            let schema = Schema::new(&map, 0, |name| {
                match map.get(name).map(|entry| &entry.kind) {
                    Some(angelica_map::EntryKind::Field { .. }) => {
                        angelica_sdk::api::memory::field(name)
                            .map(angelica_map::Value::Offset)
                            .map_err(|e| e.message)
                    }
                    Some(_) => {
                        let mut globals = globals.borrow_mut();
                        globals.push(name.to_owned());
                        u32::try_from(globals.len() - 1)
                            .map(angelica_map::Value::Address)
                            .map_err(|e| e.to_string())
                    }
                    None => Err(format!("{name} is not an entry of the game map")),
                }
            });
            Ok(Model {
                map,
                schema,
                globals: globals.into_inner(),
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Every domain: the core decides what the plugin may read.
fn any(_: &str) -> bool {
    true
}

/// A query over the game's objects.
pub struct Query {
    prepared: Prepared,
}

impl Query {
    /// Plans `text`, such as `inventory.containers[kind = bag_1] { items { item, quantity } }`.
    ///
    /// # Errors
    /// A query that breaks the syntax or the model, and what this build
    /// lacks.
    pub fn prepare(text: &str) -> Result<Self, Error> {
        let model = model()?;
        let prepared = plan::prepare(&model.schema, text, &any).map_err(from_query)?;
        Ok(Self { prepared })
    }

    /// The query's columns, in the order of each row's values.
    #[must_use]
    pub fn columns(&self) -> Vec<Column> {
        self.prepared
            .columns
            .iter()
            .map(|column| Column {
                name: column.name.clone(),
                doc: column.doc.clone(),
            })
            .collect()
    }

    /// Reads the rows now.
    ///
    /// # Errors
    /// A domain the player did not grant, and too many rows.
    pub fn run(&self) -> Result<Vec<Row>, Error> {
        let model = model()?;
        run::rows(
            &run::Context {
                globals: &model.globals,
                call: None,
            },
            &self.prepared,
        )
    }
}

/// What the library intercepts: by function, the watches and rules there.
#[derive(Default)]
struct Registry {
    /// Each function's handler number and its watches' and rules' ids.
    functions: BTreeMap<String, u32>,
    entries: Vec<wire::Entry>,
    /// Rows of each watch that the plugin has yet to take.
    rows: HashMap<u32, Vec<Row>>,
    /// How many calls each rule decided.
    decided: HashMap<u32, u64>,
    /// Reports of the plugin's own handlers.
    others: Vec<Vec<u8>>,
    /// The plugin's own part of the snapshot.
    own: Vec<u8>,
}

/// In an intercept instance: the plans of each handler number, and the
/// plugin's part of the snapshot.
#[derive(Default)]
struct Plans {
    by_handler: HashMap<u32, Vec<(wire::Entry, Prepared)>>,
    own: Vec<u8>,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
    static PREPARED: RefCell<Plans> = RefCell::new(Plans::default());
}

/// The function an event is at.
fn event_function(model: &Model, prepared: &Prepared) -> Result<String, Error> {
    let event = prepared.event().ok_or_else(|| {
        error(
            ErrorKind::InvalidArgument,
            "the query does not start at an event",
        )
    })?;
    model
        .map
        .objects()
        .events
        .get(event)
        .map(|event| event.at.clone())
        .ok_or_else(|| error(ErrorKind::NotFound, format!("{event} is not an event")))
}

/// Registers an entry at its function, intercepts the function the first
/// time, and publishes the snapshot the intercept instance plans from.
fn register(function: &str, entry: wire::Entry) -> Result<u32, Error> {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let next = HANDLERS + u32::try_from(registry.functions.len()).unwrap_or(0);
        let handler = if let Some(handler) = registry.functions.get(function) {
            *handler
        } else {
            icept::before(function, next)?;
            registry.functions.insert(function.to_owned(), next);
            next
        };
        let id = entry.id;
        registry.entries.push(wire::Entry { handler, ..entry });
        icept::publish_snapshot(&wire::snapshot(&registry.entries, &registry.own))?;
        Ok(id)
    })
}

fn next_id() -> u32 {
    REGISTRY.with(|registry| u32::try_from(registry.borrow().entries.len()).unwrap_or(0) + 1)
}

/// A watch of an event: the rows its query reads each time the event fires.
pub struct Watch {
    id: u32,
    columns: Vec<Column>,
}

impl Watch {
    /// Watches `text`, a query from an event, such as
    /// `chat_message_added { kind, message }`.
    ///
    /// # Errors
    /// As [`Query::prepare`], and what the core refuses when it intercepts
    /// the event's function: a domain the player did not grant.
    pub fn new(text: &str) -> Result<Self, Error> {
        let model = model()?;
        let prepared = plan::prepare_watch(&model.schema, text, &any).map_err(from_query)?;
        let function = event_function(model, &prepared)?;
        let columns = Query {
            prepared: prepared.clone(),
        }
        .columns();
        let id = register(
            &function,
            wire::Entry {
                id: next_id(),
                handler: 0,
                query: text.to_owned(),
                rule: None,
            },
        )?;
        Ok(Self { id, columns })
    }

    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The rows of the firings since the last call, in order.
    ///
    /// # Errors
    /// None today; reserved for the core's refusals.
    pub fn take(&self) -> Result<Vec<Row>, Error> {
        poll();
        Ok(REGISTRY.with(|registry| {
            registry
                .borrow_mut()
                .rows
                .remove(&self.id)
                .unwrap_or_default()
        }))
    }
}

/// A rule at a decision point: when its query reaches anything at a call
/// of the event's function, the function returns the outcome's value at
/// once, without running.
pub struct Rule {
    id: u32,
}

impl Rule {
    /// A rule such as `chat_message_added[kind = 57]` with the outcome
    /// `hide`.
    ///
    /// # Errors
    /// As [`Watch::new`], an outcome the event does not have, and a domain
    /// the plugin may not change.
    pub fn new(text: &str, outcome: &str) -> Result<Self, Error> {
        let model = model()?;
        let prepared = plan::prepare_rule(&model.schema, text, &any).map_err(from_query)?;
        let function = event_function(model, &prepared)?;
        let event = prepared.event().unwrap_or_default();
        let found = &model.map.objects().events[event];
        let outcome = found.outcomes.get(outcome).ok_or_else(|| {
            error(
                ErrorKind::NotFound,
                format!("{event} has no outcome {outcome}"),
            )
        })?;
        let returns = match model.map.get(&function).map(|entry| &entry.kind) {
            Some(angelica_map::EntryKind::Function { abi, .. }) => abi.ret,
            _ => angelica_map::ValueKind::Void,
        };
        let id = register(
            &function,
            wire::Entry {
                id: next_id(),
                handler: 0,
                query: text.to_owned(),
                rule: Some((
                    match outcome.returns {
                        Some(angelica_map::Number::Integer(value)) => value.cast_unsigned(),
                        Some(angelica_map::Number::Float(bits)) => bits,
                        None => 0,
                    },
                    wire::kind(returns),
                )),
            },
        )?;
        Ok(Self { id })
    }

    /// How many calls the rule decided so far.
    #[must_use]
    pub fn decided(&self) -> u64 {
        poll();
        REGISTRY.with(|registry| {
            registry
                .borrow()
                .decided
                .get(&self.id)
                .copied()
                .unwrap_or(0)
        })
    }
}

/// Takes the reports of the intercept instance: rows of watches, decisions
/// of rules, and the plugin's own.
fn poll() {
    let reports = icept::reports();
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        for report in reports {
            match wire::Report::decode(&report) {
                Some(wire::Report::Rows(id, rows)) => {
                    registry.rows.entry(id).or_default().extend(rows);
                }
                Some(wire::Report::Decided(id)) => {
                    *registry.decided.entry(id).or_default() += 1;
                }
                None => registry.others.push(report),
            }
        }
    });
}

/// The reports of the plugin's own handlers, which the library passed by.
#[must_use]
pub fn reports() -> Vec<Vec<u8>> {
    poll();
    REGISTRY.with(|registry| std::mem::take(&mut registry.borrow_mut().others))
}

/// Publishes the plugin's own snapshot beside the library's.
///
/// # Errors
/// As `intercepts.publish-snapshot`.
pub fn publish_snapshot(bytes: &[u8]) -> Result<(), Error> {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.own = bytes.to_vec();
        icept::publish_snapshot(&wire::snapshot(&registry.entries, &registry.own))
    })
}

/// In an intercept instance: the plugin's own part of the snapshot.
#[must_use]
pub fn snapshot() -> Vec<u8> {
    PREPARED.with(|prepared| prepared.borrow().own.clone())
}

/// In `Plugin::prepare`: plans the watches and rules of the snapshot.
///
/// # Errors
/// A snapshot that does not decode, and a plan that no longer fits.
pub fn prepare() -> Result<(), String> {
    let bytes = icept::snapshot();
    let (entries, own) = wire::read_snapshot(&bytes).ok_or("the Lens snapshot does not decode")?;
    let model = model()?;
    let mut plans: HashMap<u32, Vec<(wire::Entry, Prepared)>> = HashMap::new();
    for entry in entries {
        let prepared = if entry.rule.is_some() {
            plan::prepare_rule(&model.schema, &entry.query, &any)
        } else {
            plan::prepare_watch(&model.schema, &entry.query, &any)
        }
        .map_err(|e| e.message)?;
        plans
            .entry(entry.handler)
            .or_default()
            .push((entry, prepared));
    }
    PREPARED.with(|prepared| {
        *prepared.borrow_mut() = Plans {
            by_handler: plans,
            own,
        };
    });
    Ok(())
}

/// In `Plugin::intercepted`: runs the library's watches and rules at the
/// call; false for a call of the plugin's own handlers.
#[allow(clippy::must_use_candidate)]
pub fn intercepted(call: &icept::Call) -> bool {
    if call.handler < HANDLERS {
        return false;
    }
    let Ok(model) = model() else {
        return true;
    };
    PREPARED.with(|prepared| {
        let prepared = prepared.borrow();
        let Some(plans) = prepared.by_handler.get(&call.handler) else {
            return;
        };
        let context = run::Context {
            globals: &model.globals,
            call: Some(&call.args),
        };
        for (entry, plan) in plans {
            let Ok(rows) = run::rows(&context, plan) else {
                continue;
            };
            match entry.rule {
                None => {
                    if !rows.is_empty() {
                        let _ = icept::report(&wire::Report::Rows(entry.id, rows).encode());
                    }
                }
                Some((returns, kind)) if !rows.is_empty() => {
                    if icept::skip(wire::returned(returns, kind)).is_ok() {
                        let _ = icept::report(&wire::Report::Decided(entry.id).encode());
                        return;
                    }
                }
                Some(_) => {}
            }
        }
    });
    true
}

/// Takes an action for the player through the core, which checks its
/// conditions, its rate, and the player's stop key.
///
/// # Errors
/// What the core refuses.
pub fn act(action: &str, params: &[(&str, Value)]) -> Result<(), Error> {
    let params: Vec<(String, Value)> = params
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect();
    angelica_sdk::api::lens::act(action, &params)
}

/// Changes a value through the core, which puts it back when the plugin
/// stops; gives how many objects it changed.
///
/// # Errors
/// What the core refuses.
pub fn set(path: &str, value: &Value) -> Result<u32, Error> {
    angelica_sdk::api::lens::set(path, value)
}

/// Puts a value changed with [`set`] back.
///
/// # Errors
/// What the core refuses.
pub fn restore(path: &str) -> Result<(), Error> {
    angelica_sdk::api::lens::restore(path)
}
