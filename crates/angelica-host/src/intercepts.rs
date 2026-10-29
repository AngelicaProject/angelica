//! Handlers of plugins in the game's calls of functions their own Anatomia
//! entries describe (`wit/intercepts.wit`).
//!
//! A handler runs in its plugin's intercept instance on the thread that
//! called the game's function: one instance per plugin, which game threads
//! enter one at a time. A snapshot the main instance publishes reaches a
//! new instance, prepared in the background, which then takes the place of
//! the one in use; handlers never wait for it. A handler sees the
//! call's arguments as numbers and handles, never as addresses, and the
//! latest snapshot the main instance published. `before` handlers may
//! change the arguments or return at once; `after` handlers may change the
//! result. A handler that traps or runs past its limit disables every
//! handler of its plugin, and the call goes on as if it had not run.
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
pub use crate::lens::act::Returned;
use crate::sandbox::{self, Context, Role, State};
use angelica_anatomia::{Anatomia, Kind as EntryKind, Primitive, Resolution, Ty, TypeDef};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};
use wasmtime::Store;
use wasmtime::component::{HasSelf, Linker};

pub mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "intercept-handler",
        with: {
            "angelica:plugin/types": crate::sandbox::bindings::angelica::plugin::types,
            "angelica:plugin/intercepts": crate::sandbox::bindings::angelica::plugin::intercepts,
        },
    });
}
use crate::sandbox::bindings::angelica::plugin::intercepts as api;

/// The longest a handler runs.
pub const HANDLER_TIME: Duration = Duration::from_millis(50);
/// The longest an intercept instance prepares a snapshot.
pub const PREPARE_TIME: Duration = Duration::from_secs(30);
/// The largest snapshot.
pub const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;
/// The most arguments an intercepted function has.
pub const MAX_ARGS: usize = crate::lens::act::MOST_ARGS;
/// How many arguments the convention passes in registers.
pub const REGISTER_ARGS: usize = 4;
/// Guest instructions of one handler call, in fuel units; the time limit
/// is what stops a handler, this only keeps fuel from running out first.
const HANDLER_FUEL: u64 = u64::MAX / 2;

/// The code of a function the game runs, as a hook checks it: every range
/// of it, and the length of its prologue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub start: u64,
    pub ranges: Vec<std::ops::Range<u64>>,
    pub prologue: u64,
}

/// A plugin's own Anatomia entries, resolved against the running game.
pub struct Own {
    pub anatomia: Arc<Anatomia>,
    pub resolution: Resolution,
    /// Where the game's executable is loaded.
    pub base: u64,
    /// The code of each present function, by name.
    code: BTreeMap<String, Code>,
}

impl Own {
    /// Resolves `anatomia` against the game's executable loaded at `base`,
    /// as `image` is mapped there. A field at a constant offset is the
    /// author's word, as everything in the plugin's own entries is: no
    /// build record confirms it.
    #[must_use]
    pub fn resolve(anatomia: Arc<Anatomia>, base: u64, image: &angelica_pe::Image<'_>) -> Self {
        let constants = anatomia
            .entries()
            .filter(|entry| matches!(entry.kind, EntryKind::Field { at: Some(_), .. }))
            .map(|entry| entry.name.clone())
            .collect();
        let resolution = anatomia.resolve(image, base, &constants);
        let mut code = BTreeMap::new();
        for (name, value) in resolution.present() {
            if !matches!(
                anatomia.get(name).map(|entry| &entry.kind),
                Some(EntryKind::Function { .. })
            ) {
                continue;
            }
            if let Some(found) = angelica_map::function_code(image, value.get()) {
                let at = |rva: u32| base + u64::from(rva);
                code.insert(
                    name.to_owned(),
                    Code {
                        start: at(found.start),
                        ranges: found
                            .ranges
                            .iter()
                            .map(|range| at(range.start)..at(range.end))
                            .collect(),
                        prologue: u64::from(found.prologue.unwrap_or(0)),
                    },
                );
            }
        }
        Self {
            anatomia,
            resolution,
            base,
            code,
        }
    }

    /// The entries that are absent in this build, and why.
    pub fn absent(&self) -> impl Iterator<Item = (&str, String)> {
        self.resolution
            .absent()
            .map(|(name, absence)| (name, absence.to_string()))
    }

    /// The entries as direct access reads them.
    #[must_use]
    pub fn source(&self) -> crate::memory::Source<'_> {
        crate::memory::Source {
            anatomia: &self.anatomia,
            resolution: &self.resolution,
            base: self.base,
            own: true,
        }
    }

    fn structure(&self, name: &str) -> Option<&angelica_anatomia::Struct> {
        self.source().structure(name)
    }
}

/// One argument of a call as the Windows x64 convention passes it: the
/// first four in a general register and an `xmm` register, each read as
/// the function's type says; the rest on the stack, where both are the
/// same slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Arg {
    /// `rcx`, `rdx`, `r8`, `r9`, or the stack slot.
    pub int: u64,
    /// The low 64 bits of `xmm0` to `xmm3`, or the stack slot.
    pub float: u64,
}

impl Arg {
    /// An argument in one stack slot, or the same in both registers.
    #[must_use]
    pub fn both(value: u64) -> Self {
        Self {
            int: value,
            float: value,
        }
    }
}

/// What a handler receives for one parameter or the result.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Shape {
    Signed(u32),
    Unsigned(u32),
    Boolean,
    F32,
    F64,
    /// A pointer to a structure of the plugin's entries.
    Object(String),
    /// A pointer to 8-bit text.
    Text,
    /// Another pointer, by what it points to; empty for unknown memory.
    Pointer(String),
}

impl Shape {
    fn float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    fn pointer(&self) -> bool {
        matches!(self, Self::Object(_) | Self::Text | Self::Pointer(_))
    }
}

fn shape(ty: &Ty, own: &Own) -> Option<Shape> {
    match ty {
        Ty::Primitive(primitive) => Some(primitive_shape(*primitive)),
        Ty::Named(name, args) if args.is_empty() => match own.anatomia.types().get(name)? {
            TypeDef::Enum(enumeration) => enumeration.ty.map(primitive_shape),
            TypeDef::Alias { ty, .. } => shape(ty, own),
            _ => None,
        },
        Ty::Ptr(Some(inner)) => match inner.as_ref() {
            Ty::Named(name, args) if args.is_empty() && own.structure(name).is_some() => {
                Some(Shape::Object(name.clone()))
            }
            Ty::Char8 => Some(Shape::Text),
            other => Some(Shape::Pointer(other.to_string())),
        },
        Ty::Ptr(None) => Some(Shape::Pointer("ptr".to_owned())),
        _ => None,
    }
}

fn primitive_shape(primitive: Primitive) -> Shape {
    match primitive {
        Primitive::F32 => Shape::F32,
        Primitive::F64 => Shape::F64,
        Primitive::Bool => Shape::Boolean,
        Primitive::I8 | Primitive::I16 | Primitive::I32 | Primitive::I64 => {
            Shape::Signed(primitive.size() * 8)
        }
        Primitive::U8 | Primitive::U16 | Primitive::U32 | Primitive::U64 => {
            Shape::Unsigned(primitive.size() * 8)
        }
    }
}

/// One plugin's handler of one function in one phase.
struct Handler {
    plugin: Arc<Registered>,
    function: String,
    number: u32,
    phase: api::Phase,
    params: Arc<[Shape]>,
    returns: Option<Shape>,
}

/// The order a plugin's manifest asks for its handlers among other
/// plugins': after the handlers of `after`, before those of `before`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Order {
    pub after: Vec<String>,
    pub before: Vec<String>,
}

/// A plugin with handlers.
struct Registered {
    context: Arc<Context>,
    pre: bindings::InterceptHandlerPre<State>,
    enabled: AtomicBool,
    /// The intercept instance handlers run in.
    instance: RwLock<Arc<Mutex<Instance>>>,
    /// How many snapshots were published, and how many of them are still
    /// being prepared.
    published: AtomicU64,
    preparing: AtomicU64,
    calls: AtomicU64,
    nanos: AtomicU64,
}

impl Registered {
    fn id(&self) -> &str {
        &self.context.id
    }

    /// Whether this plugin's handlers come after `other`'s by either
    /// plugin's wish.
    fn wants_after(&self, other: &Self) -> bool {
        self.context.order.after.iter().any(|id| id == other.id())
            || other.context.order.before.iter().any(|id| id == self.id())
    }
}

/// What a plugin's handlers cost so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cost {
    pub calls: u64,
    pub time: Duration,
}

/// The handlers of one function, in their order, with how many arguments
/// the longest of their types has.
#[derive(Default, Clone)]
struct Chain {
    handlers: Vec<Arc<Handler>>,
    arity: usize,
}

/// Every plugin's handlers, by the address of the function they intercept.
#[derive(Default)]
pub struct Intercepts {
    plugins: Mutex<HashMap<String, Arc<Registered>>>,
    targets: RwLock<HashMap<u64, Chain>>,
    hooked: Mutex<std::collections::HashSet<u64>>,
}

/// The call a handler of an intercept instance is in, and what the handler
/// changed in it.
pub struct Frame {
    phase: api::Phase,
    /// What every handle of the call stands for.
    pub(crate) handles: crate::memory::Handles,
    snapshot: Arc<Vec<u8>>,
    params: Arc<[Shape]>,
    returns: Option<Shape>,
    /// The arguments, with the handler's changes.
    args: Vec<Arg>,
    /// Before: what the function returns without running.
    skip: Option<Returned>,
    /// After: the result, with the handler's change.
    result: Returned,
}

thread_local! {
    /// True while this thread runs a handler: a call of an intercepted
    /// function from inside one runs no handlers.
    static IN_HANDLER: Cell<bool> = const { Cell::new(false) };
    static SERIAL: Cell<u64> = const { Cell::new(0) };
}

/// While it lives, this thread runs handlers; dropping it clears the flag,
/// also when a handler's host code panics.
struct Running;

impl Running {
    fn start() -> Self {
        IN_HANDLER.with(|flag| flag.set(true));
        Self
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        IN_HANDLER.with(|flag| flag.set(false));
    }
}

/// An intercept instance, with the snapshot it prepared.
struct Instance {
    store: Store<State>,
    bindings: bindings::InterceptHandler,
    snapshot: Arc<Vec<u8>>,
}

/// Makes an intercept instance of `plugin` and has it prepare `snapshot`.
fn prepare(plugin: &Registered, snapshot: Arc<Vec<u8>>) -> std::result::Result<Instance, String> {
    let mut store = sandbox::store_for(&plugin.context, Role::Prepare(snapshot.clone()));
    sandbox::budget(&mut store, PREPARE_TIME, PREPARE_TIME, HANDLER_FUEL);
    let bindings = sandbox::call(&mut store, |store| plugin.pre.instantiate(store))
        .map_err(|e| format!("the intercept instance cannot be made: {e}"))?;
    let prepared = sandbox::call(&mut store, |store| {
        bindings.angelica_plugin_handler().call_prepare(&mut *store)
    })
    .map_err(|trap| sandbox::describe_trap(&store, &trap))?;
    prepared.map_err(|reason| format!("the snapshot is not prepared: {reason}"))?;
    store.data_mut().role = Role::Intercept(None);
    Ok(Instance {
        store,
        bindings,
        snapshot,
    })
}

impl Intercepts {
    /// Makes `context`'s plugin handle calls of `function` of its own
    /// entries with `handler` in `phase`. Returns the function's address
    /// and code when no plugin intercepted it before, so the caller hooks
    /// it.
    ///
    /// # Errors
    /// A plugin without the permission or its own entries, a function that
    /// is not one of them, is absent, has a type a handler cannot take, or
    /// is intercepted by this plugin in this phase already.
    #[allow(clippy::too_many_lines)]
    pub fn add(
        &self,
        context: &Arc<Context>,
        function: &str,
        handler: u32,
        phase: api::Phase,
    ) -> Result<Option<(u64, Code)>> {
        if !context
            .permissions
            .contains(&crate::manifest::Permission::Unreviewed)
        {
            return Err(Error::new(
                Kind::PermissionDenied,
                "intercepting needs the unreviewed permission",
            ));
        }
        let own = context.own.clone().ok_or_else(|| {
            Error::new(
                Kind::Unavailable,
                "the plugin's own entries are not resolved in this game",
            )
        })?;
        let name = own.anatomia.canonical(function).to_owned();
        let entry = own.anatomia.get(&name).ok_or_else(|| {
            Error::new(
                Kind::NotFound,
                format!("{function} is not an entry of the plugin"),
            )
        })?;
        let EntryKind::Function { ty, .. } = &entry.kind else {
            return Err(Error::new(
                Kind::InvalidArgument,
                format!("{function} is not a function"),
            ));
        };
        let address = match own.resolution.get(&name) {
            Some(Ok(value)) => own.base + u64::from(value.get()),
            Some(Err(absence)) => {
                return Err(Error::new(
                    Kind::Unavailable,
                    format!("{function} is absent: {absence}"),
                ));
            }
            None => {
                return Err(Error::new(
                    Kind::Unavailable,
                    format!("{function} was not resolved"),
                ));
            }
        };
        let code = own.code.get(&name).cloned().ok_or_else(|| {
            Error::new(
                Kind::Unavailable,
                format!("the code of {function} is not known exactly"),
            )
        })?;
        if ty.params.len() > MAX_ARGS {
            return Err(Error::new(
                Kind::InvalidArgument,
                format!("{function} has more than {MAX_ARGS} arguments"),
            ));
        }
        let params = ty
            .params
            .iter()
            .map(|param| shape(&param.ty, &own))
            .collect::<Option<Arc<[Shape]>>>()
            .ok_or_else(|| {
                Error::new(
                    Kind::InvalidArgument,
                    format!(
                        "an argument of {function} is not a number, a bool, an enum with a type, or a pointer"
                    ),
                )
            })?;
        let returns = match &ty.ret {
            None => None,
            Some(ret) => Some(shape(ret, &own).ok_or_else(|| {
                Error::new(
                    Kind::InvalidArgument,
                    format!("{function} returns what a handler cannot take"),
                )
            })?),
        };

        let plugin = self.registered(context)?;
        let mut targets = self.targets.write().unwrap_or_else(PoisonError::into_inner);
        let chain = targets.entry(address).or_default();
        if chain
            .handlers
            .iter()
            .any(|existing| Arc::ptr_eq(&existing.plugin, &plugin) && existing.phase == phase)
        {
            return Err(Error::new(
                Kind::InvalidArgument,
                format!("{function} is intercepted in this phase already"),
            ));
        }
        let at = place(chain, &plugin, function);
        chain.handlers.insert(
            at,
            Arc::new(Handler {
                plugin,
                function: function.to_owned(),
                number: handler,
                phase,
                params: params.clone(),
                returns,
            }),
        );
        chain.arity = chain.arity.max(params.len());
        drop(targets);
        let first = self
            .hooked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(address);
        Ok(first.then_some((address, code)))
    }

    /// Forgets that `address` was hooked, when hooking it failed.
    pub fn unhooked(&self, address: u64) {
        self.hooked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&address);
        self.targets
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&address);
    }

    fn registered(&self, context: &Arc<Context>) -> Result<Arc<Registered>> {
        let mut plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = plugins.get(&context.id) {
            return Ok(found.clone());
        }
        let component = context.component.clone().ok_or_else(|| {
            Error::new(Kind::Unavailable, "the plugin's component is not at hand")
        })?;
        let pre =
            handler_pre(&component).map_err(|reason| Error::new(Kind::Unavailable, reason))?;
        let mut store = sandbox::store_for(context, Role::Intercept(None));
        sandbox::budget(&mut store, PREPARE_TIME, PREPARE_TIME, HANDLER_FUEL);
        let bindings = sandbox::call(&mut store, |store| pre.instantiate(store)).map_err(|e| {
            Error::new(
                Kind::Unavailable,
                format!("the intercept instance cannot be made: {e}"),
            )
        })?;
        let registered = Arc::new(Registered {
            context: context.clone(),
            pre,
            enabled: AtomicBool::new(true),
            instance: RwLock::new(Arc::new(Mutex::new(Instance {
                store,
                bindings,
                snapshot: Arc::new(Vec::new()),
            }))),
            published: AtomicU64::new(0),
            preparing: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
        });
        plugins.insert(context.id.clone(), registered.clone());
        Ok(registered)
    }

    /// Has a new intercept instance of `context`'s plugin prepare `bytes`
    /// in the background; it takes the place of the one handlers run in
    /// when it is ready, unless a later snapshot was published meanwhile.
    /// When preparing fails, handlers go on in the instance they had, and
    /// the plugin's log says why.
    ///
    /// # Errors
    /// Bytes over the limit, and a plugin whose handlers cannot be made.
    pub fn publish(&self, context: &Arc<Context>, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > MAX_SNAPSHOT {
            return Err(Error::new(
                Kind::LimitExceeded,
                "a snapshot holds at most 64 MiB",
            ));
        }
        if context.own.is_none() {
            return Err(Error::new(
                Kind::Unavailable,
                "the plugin's own entries are not resolved in this game",
            ));
        }
        let plugin = self.registered(context)?;
        let generation = plugin.published.fetch_add(1, Ordering::AcqRel) + 1;
        plugin.preparing.fetch_add(1, Ordering::AcqRel);
        let snapshot = Arc::new(bytes);
        std::thread::Builder::new()
            .name(format!("angelica prepare {}", plugin.id()))
            .spawn(move || {
                let prepared = prepare(&plugin, snapshot);
                match prepared {
                    Ok(instance)
                        if plugin.published.load(Ordering::Acquire) == generation
                            && plugin.enabled.load(Ordering::Acquire) =>
                    {
                        *plugin
                            .instance
                            .write()
                            .unwrap_or_else(PoisonError::into_inner) =
                            Arc::new(Mutex::new(instance));
                    }
                    Ok(_) => {}
                    Err(reason) => plugin.context.log.push(
                        crate::sandbox::bindings::angelica::plugin::runtime::Level::Error,
                        &format!("handlers keep the snapshot they had: {reason}"),
                    ),
                }
                plugin.preparing.fetch_sub(1, Ordering::AcqRel);
            })
            .map_err(|e| Error::new(Kind::Unavailable, format!("no thread to prepare: {e}")))?;
        Ok(())
    }

    /// Whether every snapshot `plugin` published was prepared; for tests.
    #[must_use]
    pub fn prepared(&self, plugin: &str) -> bool {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(plugin)
            .is_none_or(|found| found.preparing.load(Ordering::Acquire) == 0)
    }

    /// Removes every handler of `plugin`. The hooks stay and call only the
    /// game's code.
    pub fn remove(&self, plugin: &str) {
        let removed = self
            .plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin);
        if let Some(removed) = removed {
            removed.enabled.store(false, Ordering::Release);
        }
        for chain in self
            .targets
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .values_mut()
        {
            chain
                .handlers
                .retain(|handler| handler.plugin.id() != plugin);
        }
    }

    /// What `plugin`'s handlers cost since it started intercepting.
    #[must_use]
    pub fn cost(&self, plugin: &str) -> Cost {
        self.plugins
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(plugin)
            .map(|found| Cost {
                calls: found.calls.load(Ordering::Relaxed),
                time: Duration::from_nanos(found.nanos.load(Ordering::Relaxed)),
            })
            .unwrap_or_default()
    }

    /// The address `plugin` intercepts `function` at; for tests.
    #[must_use]
    pub fn address_of(&self, plugin: &str, function: &str) -> Option<u64> {
        self.targets
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|(_, chain)| {
                chain
                    .handlers
                    .iter()
                    .any(|handler| handler.plugin.id() == plugin && handler.function == function)
            })
            .map(|(address, _)| *address)
    }

    /// How many arguments the handlers of the function at `target` read:
    /// none when it has no handlers, or this thread is in a handler.
    #[must_use]
    pub fn arity(&self, target: u64) -> Option<usize> {
        if IN_HANDLER.with(Cell::get) {
            return None;
        }
        self.targets
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&target)
            .filter(|chain| !chain.handlers.is_empty())
            .map(|chain| chain.arity)
    }

    /// The game called the function at `target` with `args`, as many as
    /// [`Intercepts::arity`] said: runs its `before` handlers, which may
    /// change `args`; then, when it has `after` handlers, the game's code
    /// through `original` and the `after` handlers. Returns what the
    /// function returns when that is decided here, or none when the game's
    /// code runs as usual with `args`.
    pub fn call(
        &self,
        target: u64,
        args: &mut [Arg],
        original: impl FnOnce(&[Arg]) -> Returned,
    ) -> Option<Returned> {
        if IN_HANDLER.with(Cell::get) {
            return None;
        }
        let handlers: Vec<Arc<Handler>> = match self
            .targets
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&target)
        {
            Some(chain) if !chain.handlers.is_empty() => chain.handlers.clone(),
            _ => return None,
        };
        let enabled = |handler: &&Arc<Handler>, phase: api::Phase| {
            handler.phase == phase && handler.plugin.enabled.load(Ordering::Acquire)
        };

        let mut skipped = None;
        for handler in handlers
            .iter()
            .filter(|handler| enabled(handler, api::Phase::Before))
        {
            let frame = {
                let _running = Running::start();
                run(handler, args, None, false)
            };
            if let Some(frame) = frame {
                args.copy_from_slice(&frame.args[..args.len()]);
                if frame.skip.is_some() {
                    skipped = frame.skip;
                    break;
                }
            }
        }

        let afters: Vec<&Arc<Handler>> = handlers
            .iter()
            .rev()
            .filter(|handler| enabled(handler, api::Phase::After))
            .collect();
        if afters.is_empty() {
            return skipped;
        }
        let mut result = match skipped {
            Some(returned) => returned,
            None => original(args),
        };
        for handler in afters {
            let frame = {
                let _running = Running::start();
                run(handler, args, Some(result), skipped.is_some())
            };
            if let Some(frame) = frame {
                result = frame.result;
            }
        }
        Some(result)
    }
}

/// Where a handler of `plugin` goes in `chain`: next to the plugin's other
/// handler of the function, else as late as the wishes of the plugins in
/// the chain allow. Wishes that contradict are ignored, the handler goes
/// last, and the plugin's log says so.
fn place(chain: &Chain, plugin: &Arc<Registered>, function: &str) -> usize {
    if let Some(at) = chain
        .handlers
        .iter()
        .position(|handler| Arc::ptr_eq(&handler.plugin, plugin))
    {
        return at;
    }
    let mut lowest = 0;
    let mut highest = chain.handlers.len();
    for (at, handler) in chain.handlers.iter().enumerate() {
        if plugin.wants_after(&handler.plugin) {
            lowest = lowest.max(at + 1);
        }
        if handler.plugin.wants_after(plugin) {
            highest = highest.min(at);
        }
    }
    if lowest <= highest {
        return highest;
    }
    plugin.context.log.push(
        crate::sandbox::bindings::angelica::plugin::runtime::Level::Warning,
        &format!(
            "the order of handlers of {function} the plugins ask for contradicts itself; this plugin's run last"
        ),
    );
    chain.handlers.len()
}

/// Links an intercept instance's imports for `component`: `runtime`, this
/// interface, `memory`, and the WASI the standard library needs;
/// everything else traps.
fn handler_pre(
    component: &wasmtime::component::Component,
) -> std::result::Result<bindings::InterceptHandlerPre<State>, String> {
    let engine = component.engine();
    let mut linker = Linker::<State>::new(engine);
    crate::sandbox::bindings::angelica::plugin::runtime::add_to_linker::<_, HasSelf<_>>(
        &mut linker,
        |state| state,
    )
    .map_err(|e| e.to_string())?;
    api::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state).map_err(|e| e.to_string())?;
    crate::sandbox::bindings::angelica::plugin::memory::add_to_linker::<_, HasSelf<_>>(
        &mut linker,
        |state| state,
    )
    .map_err(|e| e.to_string())?;
    let names: Vec<String> = component
        .component_type()
        .imports(engine)
        .map(|(name, _)| name.to_owned())
        .collect();
    crate::wasi::define(&mut linker, &names).map_err(|e| format!("{e:#}"))?;
    linker
        .define_unknown_imports_as_traps(component)
        .map_err(|e| e.to_string())?;
    let pre = linker
        .instantiate_pre(component)
        .map_err(|e| e.to_string())?;
    bindings::InterceptHandlerPre::new(pre)
        .map_err(|e| format!("the plugin does not export a handler: {e}"))
}

/// The bits of a value of `shape` in its register or stack slot.
fn raw(arg: Arg, shape: &Shape) -> u64 {
    if shape.float() { arg.float } else { arg.int }
}

impl Frame {
    /// What a handler sees of `raw`, a value of `shape`; a pointer gets a
    /// handle of this call.
    fn value(&mut self, raw: u64, shape: &Shape) -> api::Value {
        match shape {
            Shape::Signed(bits) => {
                let shift = 64 - bits;
                #[allow(clippy::cast_possible_wrap)]
                let signed = ((raw << shift) as i64) >> shift;
                api::Value::Signed(signed)
            }
            Shape::Unsigned(bits) => api::Value::Unsigned(if *bits == 64 {
                raw
            } else {
                raw & ((1 << bits) - 1)
            }),
            Shape::Boolean => api::Value::Boolean(raw & 0xFF != 0),
            #[allow(clippy::cast_possible_truncation)]
            Shape::F32 => api::Value::Float32(f32::from_bits(raw as u32)),
            Shape::F64 => api::Value::Float64(f64::from_bits(raw)),
            _ if raw == 0 => api::Value::None,
            Shape::Object(_) | Shape::Text | Shape::Pointer(_) => {
                let handle = self.handles.issue_called(raw, target(shape));
                match shape {
                    Shape::Object(_) => api::Value::Object(handle),
                    Shape::Text => api::Value::Text(handle),
                    _ => api::Value::Pointer(handle),
                }
            }
        }
    }

    /// The bits of `value` as a value of `shape`.
    fn bits(&self, value: &api::Value, shape: &Shape) -> Result<u64> {
        let wrong = || {
            Err(Error::new(
                Kind::InvalidArgument,
                format!("the value is not of the type {shape:?}"),
            ))
        };
        match (shape, value) {
            (Shape::Signed(bits), api::Value::Signed(number)) => {
                let shift = 64 - bits;
                if (number << shift) >> shift == *number {
                    Ok(number.cast_unsigned())
                } else {
                    wrong()
                }
            }
            (Shape::Unsigned(bits), api::Value::Unsigned(number)) => {
                if *bits == 64 || *number >> bits == 0 {
                    Ok(*number)
                } else {
                    wrong()
                }
            }
            (Shape::Boolean, api::Value::Boolean(flag)) => Ok(u64::from(*flag)),
            (Shape::F32, api::Value::Float32(number)) => Ok(u64::from(number.to_bits())),
            (Shape::F64, api::Value::Float64(number)) => Ok(number.to_bits()),
            (pointer, api::Value::None) if pointer.pointer() => Ok(0),
            (
                pointer,
                api::Value::Object(handle) | api::Value::Text(handle) | api::Value::Pointer(handle),
            ) if pointer.pointer() => {
                let slot = self.handles.get(*handle)?;
                if slot.target == target(pointer) {
                    Ok(slot.address)
                } else {
                    Err(Error::new(
                        Kind::InvalidArgument,
                        format!("the handle is a {:?}, not a {pointer:?}", slot.target),
                    ))
                }
            }
            _ => wrong(),
        }
    }
}

/// What a handle of a pointer of `shape` points to.
fn target(shape: &Shape) -> crate::memory::Target {
    use crate::memory::Target;
    match shape {
        Shape::Object(name) => Target::Struct {
            own: true,
            name: name.clone(),
        },
        Shape::Text => Target::Text {
            most: crate::memory::TEXT_MOST,
        },
        Shape::Pointer(name) => Target::Opaque(name.clone()),
        other => Target::Opaque(format!("{other:?}")),
    }
}

impl Handler {
    /// What the handler is given of a call with `args` and, after the
    /// game's code, its result.
    fn call(
        &self,
        frame: &mut Frame,
        args: &[Arg],
        result: Option<Returned>,
        skipped: bool,
    ) -> api::Call {
        let args = self
            .params
            .iter()
            .zip(args)
            .map(|(shape, arg)| frame.value(raw(*arg, shape), shape))
            .collect();
        let returned = match (result, &self.returns) {
            (Some(result), Some(shape)) => {
                let bits = if shape.float() {
                    result.float
                } else {
                    result.integer
                };
                frame.value(bits, shape)
            }
            _ => api::Value::None,
        };
        api::Call {
            function: self.function.clone(),
            handler: self.number,
            phase: self.phase,
            args,
            returned,
            skipped,
        }
    }
}

/// Runs one handler with `args` and, after the game's code, its result,
/// in its plugin's intercept instance; another thread in it is waited for.
/// Returns the call as the handler left it, or none when the handler
/// failed and was disabled.
fn run(handler: &Handler, args: &[Arg], result: Option<Returned>, skipped: bool) -> Option<Frame> {
    let plugin = &handler.plugin;
    let serial = SERIAL.with(|serial| {
        let next = serial.get().wrapping_add(1) & ((1 << 56) - 1);
        serial.set(next);
        next
    });
    let instance = plugin
        .instance
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let mut instance = instance.lock().unwrap_or_else(PoisonError::into_inner);
    let mut frame = Frame {
        phase: handler.phase,
        handles: crate::memory::Handles::call(serial),
        snapshot: instance.snapshot.clone(),
        params: handler.params.clone(),
        returns: handler.returns.clone(),
        args: args.to_vec(),
        skip: None,
        result: result.unwrap_or_default(),
    };
    let call = handler.call(&mut frame, args, result, skipped);
    let started = Instant::now();
    let instance = &mut *instance;
    instance.store.data_mut().role = Role::Intercept(Some(frame));
    sandbox::budget(
        &mut instance.store,
        HANDLER_TIME,
        HANDLER_TIME,
        HANDLER_FUEL,
    );
    let outcome = sandbox::call(&mut instance.store, |store| {
        instance
            .bindings
            .angelica_plugin_handler()
            .call_intercepted(&mut *store, &call)
    });
    let frame = match std::mem::replace(&mut instance.store.data_mut().role, Role::Intercept(None))
    {
        Role::Intercept(frame) => frame,
        _ => None,
    };
    let outcome = outcome
        .map_err(|trap| sandbox::describe_trap(&instance.store, &trap))
        .and_then(|()| frame.ok_or_else(|| "the call's frame is gone".to_owned()));
    plugin.calls.fetch_add(1, Ordering::Relaxed);
    plugin.nanos.fetch_add(
        u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
    match outcome {
        Ok(frame) => Some(frame),
        Err(reason) => {
            plugin.enabled.store(false, Ordering::Release);
            plugin.context.log.push(
                crate::sandbox::bindings::angelica::plugin::runtime::Level::Error,
                &format!("the handler of {} is disabled: {reason}", handler.function),
            );
            None
        }
    }
}

impl State {
    fn frame(&self) -> Result<&Frame> {
        match &self.role {
            Role::Intercept(Some(frame)) => Ok(frame),
            _ => Err(Error::new(Kind::WrongContext, "only a handler can do this")),
        }
    }

    /// The call of a handler in `phase`.
    fn frame_in(&mut self, phase: api::Phase) -> Result<&mut Frame> {
        match &mut self.role {
            Role::Intercept(Some(frame)) if frame.phase == phase => Ok(frame),
            Role::Intercept(Some(_)) => Err(Error::new(
                Kind::WrongContext,
                match phase {
                    api::Phase::Before => "only a before handler can do this",
                    api::Phase::After => "only an after handler can do this",
                },
            )),
            _ => Err(Error::new(Kind::WrongContext, "only a handler can do this")),
        }
    }

    fn intercept(
        &mut self,
        function: &str,
        handler: u32,
        phase: api::Phase,
    ) -> std::result::Result<(), api::Error> {
        if !matches!(self.role, Role::Main) {
            return Err(Error::new(Kind::WrongContext, "only the main instance intercepts").into());
        }
        let context = self.context.clone();
        self.host(|_| {
            let intercepts = &context.services.intercepts;
            let hook = intercepts.add(&context, function, handler, phase)?;
            if let Some((address, code)) = hook
                && let Err(reason) = context
                    .services
                    .game
                    .enable_intercept(address, &code, intercepts)
            {
                intercepts.unhooked(address);
                return Err(Error::new(
                    Kind::Unavailable,
                    format!("{function} cannot be hooked: {reason}"),
                ));
            }
            Ok(())
        })
        .map_err(Into::into)
    }
}

impl api::Host for State {
    fn before(&mut self, function: String, handler: u32) -> std::result::Result<(), api::Error> {
        self.intercept(&function, handler, api::Phase::Before)
    }

    fn after(&mut self, function: String, handler: u32) -> std::result::Result<(), api::Error> {
        self.intercept(&function, handler, api::Phase::After)
    }

    fn publish_snapshot(&mut self, bytes: Vec<u8>) -> std::result::Result<(), api::Error> {
        if !matches!(self.role, Role::Main) {
            return Err(Error::new(Kind::WrongContext, "only the main instance publishes").into());
        }
        let context = self.context.clone();
        self.host(|_| context.services.intercepts.publish(&context, bytes))
            .map_err(Into::into)
    }

    fn snapshot(&mut self) -> Vec<u8> {
        match &self.role {
            Role::Prepare(snapshot) => snapshot.to_vec(),
            _ => self
                .frame()
                .map(|frame| frame.snapshot.to_vec())
                .unwrap_or_default(),
        }
    }

    fn set_argument(
        &mut self,
        index: u32,
        value: api::Value,
    ) -> std::result::Result<(), api::Error> {
        self.frame_in(api::Phase::Before)
            .and_then(|frame| {
                let at = usize::try_from(index).unwrap_or(usize::MAX);
                let shape = frame.params.get(at).cloned().ok_or_else(|| {
                    Error::new(
                        Kind::InvalidArgument,
                        format!("the function has {} arguments", frame.params.len()),
                    )
                })?;
                let bits = frame.bits(&value, &shape)?;
                let arg = &mut frame.args[at];
                if at >= REGISTER_ARGS {
                    *arg = Arg::both(bits);
                } else if shape.float() {
                    arg.float = bits;
                } else {
                    arg.int = bits;
                }
                Ok(())
            })
            .map_err(Into::into)
    }

    fn skip(&mut self, value: api::Value) -> std::result::Result<(), api::Error> {
        self.frame_in(api::Phase::Before)
            .and_then(|frame| {
                let returned = match frame.returns.clone() {
                    None if matches!(value, api::Value::None) => Returned::default(),
                    None => {
                        return Err(Error::new(
                            Kind::InvalidArgument,
                            "the function returns nothing",
                        ));
                    }
                    Some(shape) => {
                        let bits = frame.bits(&value, &shape)?;
                        if shape.float() {
                            Returned {
                                integer: 0,
                                float: bits,
                            }
                        } else {
                            Returned {
                                integer: bits,
                                float: 0,
                            }
                        }
                    }
                };
                frame.skip = Some(returned);
                Ok(())
            })
            .map_err(Into::into)
    }

    fn set_result(&mut self, value: api::Value) -> std::result::Result<(), api::Error> {
        self.frame_in(api::Phase::After)
            .and_then(|frame| {
                let shape = frame.returns.clone().ok_or_else(|| {
                    Error::new(Kind::InvalidArgument, "the function returns nothing")
                })?;
                let bits = frame.bits(&value, &shape)?;
                if shape.float() {
                    frame.result.float = bits;
                } else {
                    frame.result.integer = bits;
                }
                Ok(())
            })
            .map_err(Into::into)
    }

    fn text(&mut self, of: api::Handle) -> std::result::Result<Vec<u8>, api::Error> {
        if let Err(error) = self.frame() {
            return Err(error.into());
        }
        crate::sandbox::bindings::angelica::plugin::memory::Host::text(self, of)
    }

    fn edit_text(
        &mut self,
        of: api::Handle,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), api::Error> {
        if let Err(error) = self.frame() {
            return Err(error.into());
        }
        crate::sandbox::bindings::angelica::plugin::memory::Host::edit_text(self, of, bytes)
    }
}
