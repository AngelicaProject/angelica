//! Handlers of plugins in the game's calls of functions their own Anatomia
//! entries describe (`wit/intercepts.wit`).
//!
//! A handler runs in an intercept instance of its plugin on the thread
//! that called the game's function: one instance per plugin and thread,
//! made on the first call and kept while the plugin runs. It sees the
//! call's arguments as numbers and handles, never as addresses, and the
//! latest snapshot the main instance published. `before` handlers may
//! change the arguments or return at once; `after` handlers may change the
//! result. A handler that traps or runs past its limit disables every
//! handler of its plugin, and the call goes on as if it had not run.
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
pub use crate::lens::act::Returned;
use crate::sandbox::{self, Context, Role, State};
use angelica_anatomia::{
    Anatomia, Kind as EntryKind, Place, Primitive, Resolution, Text, Ty, TypeDef,
};
use std::cell::{Cell, RefCell};
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
/// The largest snapshot.
pub const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;
/// The most arguments an intercepted function has.
pub const MAX_ARGS: usize = crate::lens::act::MOST_ARGS;
/// How many arguments the convention passes in registers.
pub const REGISTER_ARGS: usize = 4;
/// The most bytes a handler reads of 8-bit text a pointer argument gives.
const TEXT_MOST: usize = 64 * 1024;
/// Guest instructions of one handler call, in fuel units; the time limit
/// is what stops a handler, this only keeps fuel from running out first.
const HANDLER_FUEL: u64 = u64::MAX / 2;
/// How deep a structure's text may lie in structures it holds.
const TEXT_DEPTH: usize = 4;

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
    /// as `image` is mapped there.
    #[must_use]
    pub fn resolve(anatomia: Arc<Anatomia>, base: u64, image: &angelica_pe::Image<'_>) -> Self {
        let resolution = anatomia.resolve(image, base, &std::collections::BTreeSet::new());
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

    fn offset(&self, name: &str) -> Option<u64> {
        match self.resolution.get(name) {
            Some(Ok(angelica_anatomia::Value::Offset(offset))) => Some(u64::from(*offset)),
            _ => None,
        }
    }

    fn structure(&self, name: &str) -> Option<&angelica_anatomia::Struct> {
        match self.anatomia.types().get(name) {
            Some(TypeDef::Struct(structure)) if !self.anatomia.broken().contains_key(name) => {
                Some(structure)
            }
            _ => None,
        }
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
            other => Some(Shape::Pointer(format!("{other:?}"))),
        },
        Ty::Ptr(None) => Some(Shape::Pointer(String::new())),
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
    /// Tells this registration's instances apart from a later one's.
    key: u64,
    context: Arc<Context>,
    own: Arc<Own>,
    pre: bindings::InterceptHandlerPre<State>,
    enabled: AtomicBool,
    snapshot: RwLock<Arc<Vec<u8>>>,
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
    next_key: AtomicU64,
    /// How many plugins stopped intercepting.
    removals: AtomicU64,
}

/// The call a handler of an intercept instance is in, and what the handler
/// changed in it.
pub struct Frame {
    phase: api::Phase,
    /// What every handle of the call stands for, by its number.
    objects: Vec<(u64, Shape)>,
    serial: u64,
    snapshot: Arc<Vec<u8>>,
    own: Arc<Own>,
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
    /// This thread's intercept instances, by registration.
    static INSTANCES: RefCell<HashMap<u64, Instance>> = RefCell::new(HashMap::new());
    /// True while this thread runs a handler: a call of an intercepted
    /// function from inside one runs no handlers.
    static IN_HANDLER: Cell<bool> = const { Cell::new(false) };
    static SERIAL: Cell<u64> = const { Cell::new(0) };
    /// The count of removals this thread last dropped instances for.
    static REMOVALS_SEEN: Cell<u64> = const { Cell::new(0) };
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

struct Instance {
    store: Store<State>,
    bindings: bindings::InterceptHandler,
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

        let plugin = self.registered(context, &own)?;
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

    fn registered(&self, context: &Arc<Context>, own: &Arc<Own>) -> Result<Arc<Registered>> {
        let mut plugins = self.plugins.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = plugins.get(&context.id) {
            return Ok(found.clone());
        }
        let component = context.component.clone().ok_or_else(|| {
            Error::new(Kind::Unavailable, "the plugin's component is not at hand")
        })?;
        let pre =
            handler_pre(&component).map_err(|reason| Error::new(Kind::Unavailable, reason))?;
        let registered = Arc::new(Registered {
            key: self.next_key.fetch_add(1, Ordering::Relaxed),
            context: context.clone(),
            own: own.clone(),
            pre,
            enabled: AtomicBool::new(true),
            snapshot: RwLock::new(Arc::new(Vec::new())),
            calls: AtomicU64::new(0),
            nanos: AtomicU64::new(0),
        });
        plugins.insert(context.id.clone(), registered.clone());
        Ok(registered)
    }

    /// The bytes `plugin`'s handlers see from the next call on.
    ///
    /// # Errors
    /// Bytes over the limit, and a plugin that intercepts nothing.
    pub fn publish(&self, context: &Arc<Context>, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > MAX_SNAPSHOT {
            return Err(Error::new(
                Kind::LimitExceeded,
                "a snapshot holds at most 64 MiB",
            ));
        }
        let own = context.own.clone().ok_or_else(|| {
            Error::new(
                Kind::Unavailable,
                "the plugin's own entries are not resolved in this game",
            )
        })?;
        let plugin = self.registered(context, &own)?;
        *plugin
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner) = Arc::new(bytes);
        Ok(())
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
            self.removals.fetch_add(1, Ordering::AcqRel);
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
        let removals = self.removals.load(Ordering::Acquire);
        let alive: Vec<u64> = if REMOVALS_SEEN.with(Cell::get) == removals {
            Vec::new()
        } else {
            self.plugins
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .map(|plugin| plugin.key)
                .collect()
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
                run(handler, args, None, false, removals, &alive)
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
                run(
                    handler,
                    args,
                    Some(result),
                    skipped.is_some(),
                    removals,
                    &alive,
                )
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
/// interface, and the WASI the standard library needs; everything else
/// traps.
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
                let found = self
                    .objects
                    .iter()
                    .position(|(address, kind)| *address == raw && kind == shape);
                let handle = if let Some(at) = found {
                    at + 1
                } else {
                    self.objects.push((raw, shape.clone()));
                    self.objects.len()
                };
                let handle = (self.serial << 8) | handle as u64;
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
                let (address, kind) = self.object(*handle)?;
                if kind == pointer {
                    Ok(*address)
                } else {
                    Err(Error::new(
                        Kind::InvalidArgument,
                        format!("the handle is a {kind:?}, not a {pointer:?}"),
                    ))
                }
            }
            _ => wrong(),
        }
    }

    fn object(&self, handle: u64) -> Result<&(u64, Shape)> {
        let index = usize::try_from(handle & 0xFF).unwrap_or(0);
        if handle >> 8 != self.serial || index == 0 {
            return Err(Error::new(
                Kind::InvalidArgument,
                "the handle is not of this call",
            ));
        }
        self.objects
            .get(index - 1)
            .ok_or_else(|| Error::new(Kind::InvalidArgument, "the handle is not of this call"))
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

/// Runs one handler with `args` and, after the game's code, its result.
/// Returns the call as the handler left it, or none when the handler
/// failed and was disabled. `removals` counts the plugins that stopped
/// intercepting, and `alive` is the registrations still running: a thread
/// drops its instances of the others when the count changed.
fn run(
    handler: &Handler,
    args: &[Arg],
    result: Option<Returned>,
    skipped: bool,
    removals: u64,
    alive: &[u64],
) -> Option<Frame> {
    let plugin = &handler.plugin;
    let serial = SERIAL.with(|serial| {
        let next = serial.get().wrapping_add(1) & ((1 << 56) - 1);
        serial.set(next);
        next
    });
    let mut frame = Frame {
        phase: handler.phase,
        objects: Vec::new(),
        serial,
        snapshot: plugin
            .snapshot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
        own: plugin.own.clone(),
        params: handler.params.clone(),
        returns: handler.returns.clone(),
        args: args.to_vec(),
        skip: None,
        result: result.unwrap_or_default(),
    };
    let call = handler.call(&mut frame, args, result, skipped);
    let started = Instant::now();
    let outcome = INSTANCES.with(|instances| {
        let mut instances = instances.borrow_mut();
        if REMOVALS_SEEN.with(Cell::get) != removals {
            instances.retain(|key, _| alive.contains(key));
            REMOVALS_SEEN.with(|seen| seen.set(removals));
        }
        let instance = match instances.entry(plugin.key) {
            std::collections::hash_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::hash_map::Entry::Vacant(place) => {
                let mut store = sandbox::store_for(&plugin.context, Role::Intercept(None));
                sandbox::budget(
                    &mut store,
                    HANDLER_TIME * 20,
                    HANDLER_TIME * 20,
                    HANDLER_FUEL,
                );
                let bindings = sandbox::call(&mut store, |store| plugin.pre.instantiate(store))
                    .map_err(|e| format!("the intercept instance cannot be made: {e}"))?;
                place.insert(Instance { store, bindings })
            }
        };
        instance.store.data_mut().role = Role::Intercept(Some(frame));
        sandbox::budget(
            &mut instance.store,
            HANDLER_TIME,
            HANDLER_TIME,
            HANDLER_FUEL,
        );
        let result = sandbox::call(&mut instance.store, |store| {
            instance
                .bindings
                .angelica_plugin_handler()
                .call_intercepted(&mut *store, &call)
        });
        let frame =
            match std::mem::replace(&mut instance.store.data_mut().role, Role::Intercept(None)) {
                Role::Intercept(frame) => frame,
                _ => None,
            };
        result
            .map_err(|trap| sandbox::describe_trap(&instance.store, &trap))
            .and_then(|()| frame.ok_or_else(|| "the call's frame is gone".to_owned()))
    });
    plugin.calls.fetch_add(1, Ordering::Relaxed);
    plugin.nanos.fetch_add(
        u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
    match outcome {
        Ok(frame) => Some(frame),
        Err(reason) => {
            plugin.enabled.store(false, Ordering::Release);
            INSTANCES.with(|instances| instances.borrow_mut().remove(&plugin.key));
            plugin.context.log.push(
                crate::sandbox::bindings::angelica::plugin::runtime::Level::Error,
                &format!("the handler of {} is disabled: {reason}", handler.function),
            );
            None
        }
    }
}

/// Where the text of an object of `structure` at `object` is: its bytes
/// start at the returned address and hold at most the returned length.
fn text_location(
    memory: &dyn crate::lens::Memory,
    own: &Own,
    structure: &str,
    object: u64,
) -> Result<(u64, usize)> {
    let mut structure = structure.to_owned();
    let mut object = object;
    for _ in 0..TEXT_DEPTH {
        let definition = own.structure(&structure).ok_or_else(|| {
            Error::new(
                Kind::InvalidArgument,
                format!("{structure} is not a structure of the plugin"),
            )
        })?;
        let field_name = definition.text.as_ref().ok_or_else(|| {
            Error::new(
                Kind::InvalidArgument,
                format!("{structure} does not read as text"),
            )
        })?;
        let field = &definition.fields[field_name];
        let located = match &field.place {
            Place::Located => format!("{structure}.{field_name}"),
            Place::Bits { .. } => {
                return Err(Error::new(Kind::InvalidArgument, "text is not in bits"));
            }
        };
        let offset = own
            .offset(&located)
            .ok_or_else(|| Error::new(Kind::Unavailable, format!("{located} is absent")))?;
        let at = object
            .checked_add(offset)
            .ok_or_else(|| Error::new(Kind::Unavailable, "the text is outside memory"))?;
        match (&field.ty, field.text) {
            (Ty::Array(inner, length), Some(Text::Terminated)) if **inner == Ty::Char8 => {
                return Ok((at, *length as usize));
            }
            (Ty::Ptr(_), Some(Text::Most(most))) => {
                let mut pointer = [0u8; 8];
                if !memory.read(at, &mut pointer) {
                    return Err(Error::new(
                        Kind::Unavailable,
                        "the text's pointer cannot be read",
                    ));
                }
                let pointer = u64::from_le_bytes(pointer);
                if pointer == 0 {
                    return Ok((0, 0));
                }
                return Ok((pointer, most as usize));
            }
            (Ty::Named(inner, _), Some(Text::Most(_))) => {
                structure.clone_from(inner);
                object = at;
            }
            _ => {
                return Err(Error::new(
                    Kind::InvalidArgument,
                    format!("{located} is not text"),
                ));
            }
        }
    }
    Err(Error::new(
        Kind::InvalidArgument,
        "the text lies too deep in structures",
    ))
}

/// The bytes of a text up to its terminator.
fn read_text(memory: &dyn crate::lens::Memory, at: u64, most: usize) -> Result<Vec<u8>> {
    let mut text = Vec::new();
    let mut chunk = [0u8; 64];
    while text.len() < most {
        let want = chunk.len().min(most - text.len());
        let address = at
            .checked_add(text.len() as u64)
            .ok_or_else(|| Error::new(Kind::Unavailable, "the text is outside memory"))?;
        // A chunk may cross into memory that cannot be read; byte by byte
        // finds the terminator before it.
        if memory.read(address, &mut chunk[..want]) {
            match chunk[..want].iter().position(|byte| *byte == 0) {
                Some(end) => {
                    text.extend_from_slice(&chunk[..end]);
                    return Ok(text);
                }
                None => text.extend_from_slice(&chunk[..want]),
            }
        } else {
            let mut byte = [0u8; 1];
            if !memory.read(address, &mut byte) {
                return Err(Error::new(Kind::Unavailable, "the text cannot be read"));
            }
            if byte[0] == 0 {
                return Ok(text);
            }
            text.push(byte[0]);
        }
    }
    Err(Error::new(
        Kind::Unavailable,
        "the text has no terminator within its limit",
    ))
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

    /// Where the text of a handle of this call is, and the most bytes it
    /// holds.
    fn text_at(&self, of: api::Handle) -> Result<(u64, usize)> {
        let frame = self.frame()?;
        let (address, shape) = frame.object(of)?;
        let memory = self.context.services.game.memory();
        match shape {
            Shape::Object(structure) => text_location(memory, &frame.own, structure, *address),
            Shape::Text => Ok((*address, TEXT_MOST)),
            _ => Err(Error::new(
                Kind::InvalidArgument,
                "the handle is neither text nor an object that reads as text",
            )),
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
        self.frame()
            .map(|frame| frame.snapshot.to_vec())
            .unwrap_or_default()
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
        self.text_at(of)
            .and_then(|(at, most)| {
                if at == 0 {
                    return Ok(Vec::new());
                }
                read_text(self.context.services.game.memory(), at, most)
            })
            .map_err(Into::into)
    }

    fn edit_text(
        &mut self,
        of: api::Handle,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), api::Error> {
        self.text_at(of)
            .and_then(|(at, most)| {
                let game = &self.context.services.game;
                let current = if at == 0 {
                    Vec::new()
                } else {
                    read_text(game.memory(), at, most)?
                };
                if bytes.len() != current.len() || bytes.contains(&0) {
                    return Err(Error::new(
                        Kind::InvalidArgument,
                        format!(
                            "the new text has {} bytes or a NUL; the text has {} bytes",
                            bytes.len(),
                            current.len()
                        ),
                    ));
                }
                if bytes.is_empty() || bytes == current {
                    return Ok(());
                }
                if game.write(at, &bytes) {
                    Ok(())
                } else {
                    Err(Error::new(
                        Kind::Unavailable,
                        "the text is not in writable memory",
                    ))
                }
            })
            .map_err(Into::into)
    }
}
