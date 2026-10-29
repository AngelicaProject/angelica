//! Plugins' direct access to the game's objects that Anatomia describes
//! (`wit/memory.wit`): the central description, converted from the game
//! map the host loaded, and each plugin's own entries.
//!
//! A plugin holds handles, never addresses. A handle stands for what the
//! game gave the plugin: the object of a global, a field it read, an
//! element of an array, an argument of a call it intercepts. It carries
//! the Anatomia type of what it points to, the domain renames in force
//! below it, and the path it was reached by from a global, so a held handle
//! can be checked against the game again at each use. Reads are checked
//! copies of the game's memory; reading a central field needs its domain
//! (`[lens] read`), and writing one is limited to the changes the Lens
//! catalog offers in memory, which the host puts back when the plugin
//! stops.
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
use crate::intercepts::Returned;
use crate::sandbox::bindings::angelica::plugin::calls as calls_api;
use crate::sandbox::bindings::angelica::plugin::memory as api;
use crate::sandbox::{Role, State};
use angelica_anatomia::{
    Absence, Anatomia, Enum, Kind as EntryKind, Partition, Place, Primitive, Resolution, Struct,
    StructField, Text, Thread, Ty, TypeDef, Value as Resolved,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

/// The most handles one frame or one call issues.
pub const MOST_HANDLES: usize = 65_536;
/// The most fields one instance looks up.
pub const MOST_FIELDS: usize = 4096;
/// The most reads of one batch.
pub const MOST_BATCH: usize = 4096;
/// The most handles one plugin holds.
pub const MOST_HELD: usize = 256;
/// The most bytes of text behind a pointer whose field gives no limit.
pub const TEXT_MOST: usize = 64 * 1024;
/// How deep a structure's bases go.
const MOST_BASES: usize = 8;
/// How deep a structure's text may lie in structures it holds.
const TEXT_DEPTH: usize = 4;
/// How many aliases a type may go through.
const MOST_ALIASES: usize = 8;

/// A description resolved for the running game, as a read needs it.
#[derive(Clone, Copy)]
pub struct Source<'a> {
    pub anatomia: &'a Anatomia,
    pub resolution: &'a Resolution,
    /// Where the game's executable is loaded.
    pub base: u64,
    /// The plugin's own entries; the central description otherwise.
    pub own: bool,
}

impl<'a> Source<'a> {
    /// The offset of a present field entry.
    fn offset(self, entry: &str) -> Result<u64> {
        match self.resolution.get(entry) {
            Some(Ok(Resolved::Offset(offset))) => Ok(u64::from(*offset)),
            Some(Ok(Resolved::Address(_))) => {
                Err(Error::invalid(format!("{entry} is not a field")))
            }
            Some(Err(absence)) => Err(absent(entry, absence)),
            None => Err(Error::new(
                Kind::Unavailable,
                format!("{entry} is not resolved in this game"),
            )),
        }
    }

    /// The address of a present entry that has one.
    pub(crate) fn address(self, entry: &str) -> Result<u64> {
        match self.resolution.get(entry) {
            Some(Ok(Resolved::Address(rva))) => Ok(self.base + u64::from(*rva)),
            Some(Ok(Resolved::Offset(_))) => Err(Error::invalid(format!(
                "{entry} is a field, not an address"
            ))),
            Some(Err(absence)) => Err(absent(entry, absence)),
            None => Err(Error::new(
                Kind::Unavailable,
                format!("{entry} is not resolved in this game"),
            )),
        }
    }

    /// A structure the description can use.
    #[must_use]
    pub fn structure(self, name: &str) -> Option<&'a Struct> {
        match self.anatomia.types().get(name) {
            Some(TypeDef::Struct(structure)) if !self.anatomia.broken().contains_key(name) => {
                Some(structure)
            }
            _ => None,
        }
    }

    fn enumeration(self, name: &str) -> Option<&'a Enum> {
        match self.anatomia.types().get(name) {
            Some(TypeDef::Enum(enumeration)) => Some(enumeration),
            _ => None,
        }
    }

    /// `ty` with its aliases followed.
    fn plain(self, ty: &'a Ty) -> Result<&'a Ty> {
        let mut ty = ty;
        for _ in 0..MOST_ALIASES {
            match ty {
                Ty::Named(name, args) if args.is_empty() => match self.anatomia.types().get(name) {
                    Some(TypeDef::Alias { ty: inner, .. }) => ty = inner,
                    _ => return Ok(ty),
                },
                _ => return Ok(ty),
            }
        }
        Err(Error::invalid(format!(
            "{ty} goes through too many aliases"
        )))
    }

    /// The primitive a value of `ty` is stored as: a primitive, an enum
    /// with one, or a `char8`.
    fn primitive(self, ty: &'a Ty) -> Option<Primitive> {
        match self.plain(ty).ok()? {
            Ty::Primitive(primitive) => Some(*primitive),
            Ty::Char8 => Some(Primitive::U8),
            Ty::Named(name, args) if args.is_empty() => self.enumeration(name)?.ty,
            _ => None,
        }
    }

    /// The size in bytes of a value of `ty`, where the description gives
    /// it.
    fn size(self, ty: &'a Ty) -> Option<u64> {
        match self.plain(ty).ok()? {
            Ty::Primitive(primitive) => Some(u64::from(primitive.size())),
            Ty::Char8 => Some(1),
            Ty::Char16 => Some(2),
            Ty::Ptr(_) | Ty::Fn(_) => Some(8),
            Ty::Array(inner, length) => self.size(inner)?.checked_mul(u64::from(*length)),
            Ty::Named(name, args) if args.is_empty() => match self.anatomia.types().get(name)? {
                TypeDef::Struct(structure) => structure.size.map(u64::from),
                TypeDef::Enum(enumeration) => enumeration.ty.map(|ty| u64::from(ty.size())),
                _ => None,
            },
            _ => None,
        }
    }

    /// How far into an object of `from` its base `to` lies, when `from` is
    /// `to` or extends it.
    fn base_offset(self, from: &str, to: &str) -> Option<u64> {
        fn walk(source: Source<'_>, from: &str, to: &str, depth: usize) -> Option<u64> {
            if from == to {
                return Some(0);
            }
            if depth == MOST_BASES {
                return None;
            }
            source
                .structure(from)?
                .extends
                .iter()
                .find_map(|(base, at)| Some(u64::from(*at) + walk(source, base, to, depth + 1)?))
        }
        walk(self, from, to, 0)
    }
}

fn absent(entry: &str, absence: &Absence) -> Error {
    Error::new(
        Kind::Unavailable,
        format!("{entry} is absent in this game: {absence}"),
    )
}

/// A change of a central field the Lens catalog offers, written in memory.
#[derive(Debug, Clone, PartialEq)]
struct Change {
    range: Option<(f64, f64)>,
}

/// A Lens event at a function of the central description: what a plugin
/// that intercepts the function reads and changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    pub event: String,
    /// The event's domain, as its `whose` and `as` rename it.
    pub domain: String,
    /// Below the call's pointers, each domain of the objects they point to
    /// belongs to the argument's.
    pub renames: BTreeMap<String, String>,
    /// The structure each pointer argument of the event points to, by its
    /// position, where the function's type says only `ptr`.
    pub objects: BTreeMap<usize, String>,
}

/// The central description, converted from the game map the host loaded,
/// with what its entries resolved to in the running game.
pub struct Described {
    pub anatomia: Arc<Anatomia>,
    pub resolution: Resolution,
    pub base: u64,
    /// The game map's files as the host read them, `map.toml` first, for
    /// the Lens library.
    pub files: Arc<Vec<(String, String)>>,
    /// The Lens events at functions, by the function's name; the host's
    /// derived events are not among them.
    pub events: BTreeMap<String, Watched>,
    /// The domains renamed below each global a Lens root starts at.
    roots: BTreeMap<String, BTreeMap<String, String>>,
    /// The fields the Lens catalog offers as changes written in memory.
    changes: BTreeMap<String, Change>,
}

impl Described {
    /// The description of the v2 map of `map_toml` and `files`, for a game
    /// loaded at `base` where `entry` gives the value of each of the map's
    /// entries, or why it is absent.
    ///
    /// # Errors
    /// A map that does not convert.
    pub fn from_map(
        map_toml: &str,
        files: &[(String, String)],
        base: u64,
        entry: impl Fn(&str) -> std::result::Result<angelica_map::Value, String>,
    ) -> std::result::Result<Self, String> {
        let converted = angelica_anatomia::convert_sources(map_toml, files)?;
        let map =
            angelica_map::Map::from_sources(map_toml, files).map_err(|error| error.to_string())?;
        let events = watched(&map, &converted.renamed);
        let anatomia = converted.anatomia()?;
        let back: BTreeMap<&str, &str> = converted
            .renamed
            .iter()
            .map(|(old, new)| (new.as_str(), old.as_str()))
            .collect();
        let results = anatomia
            .entries()
            .map(|item| {
                let name = item.name.as_str();
                let value = match entry(back.get(name).copied().unwrap_or(name)) {
                    Ok(angelica_map::Value::Offset(offset)) => Ok(Resolved::Offset(offset)),
                    Ok(angelica_map::Value::Address(rva)) => Ok(Resolved::Address(rva)),
                    Err(reason) => Err(Absence(reason)),
                };
                (name.to_owned(), value)
            })
            .collect();
        let table = |path: &str, kind: &str| -> BTreeMap<String, toml::Table> {
            converted
                .files
                .get(path)
                .and_then(|text| text.parse::<toml::Table>().ok())
                .and_then(|mut file| file.remove(kind))
                .and_then(|items| match items {
                    toml::Value::Table(items) => Some(items),
                    _ => None,
                })
                .into_iter()
                .flatten()
                .filter_map(|(name, item)| match item {
                    toml::Value::Table(item) => Some((name, item)),
                    _ => None,
                })
                .collect()
        };
        let roots = table("lens/roots.toml", "roots")
            .into_values()
            .filter_map(|root| {
                let global = root.get("global")?.as_str()?.to_owned();
                let renames = root
                    .get("as")
                    .and_then(toml::Value::as_table)
                    .into_iter()
                    .flatten()
                    .filter_map(|(from, to)| Some((from.clone(), to.as_str()?.to_owned())))
                    .collect();
                Some((global, renames))
            })
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let number = |value: &toml::Value| {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|n| n as f64))
        };
        let changes = table("lens/changes.toml", "changes")
            .into_iter()
            .filter(|(_, change)| !change.contains_key("call"))
            .map(|(name, change)| {
                let range = change
                    .get("range")
                    .and_then(toml::Value::as_array)
                    .and_then(|range| Some((number(range.first()?)?, number(range.get(1)?)?)));
                (name, Change { range })
            })
            .collect();
        let mut all = vec![("map.toml".to_owned(), map_toml.to_owned())];
        all.extend(files.iter().cloned());
        Ok(Self {
            anatomia: Arc::new(anatomia),
            resolution: Resolution::from_results(results),
            base,
            files: Arc::new(all),
            events,
            roots,
            changes,
        })
    }

    #[must_use]
    pub fn source(&self) -> Source<'_> {
        Source {
            anatomia: &self.anatomia,
            resolution: &self.resolution,
            base: self.base,
            own: false,
        }
    }
}

/// The Lens events of `map` at functions, by the function's name in the
/// converted description.
fn watched(
    map: &angelica_map::Map,
    renamed: &BTreeMap<String, String>,
) -> BTreeMap<String, Watched> {
    let objects = map.objects();
    objects
        .events
        .values()
        .filter(|event| event.derived.is_none())
        .filter(|event| {
            matches!(
                map.get(&event.at).map(|entry| &entry.kind),
                Some(angelica_map::EntryKind::Function { .. })
            )
        })
        .map(|event| {
            let mut outer = angelica_map::Renames::default();
            outer.enter(&event.entering);
            let mut below = BTreeMap::new();
            let mut pointees = BTreeMap::new();
            for arg in event.args.values() {
                if let angelica_map::Shape::Ref(to) = &arg.shape
                    && let Some(target) = objects.types.get(to)
                {
                    below.insert(target.domain.clone(), outer.domain(&arg.domain).to_owned());
                    if let angelica_map::ArgSource::Argument(position) = arg.source {
                        pointees.insert(usize::from(position), to.clone());
                    }
                }
            }
            let function = renamed.get(&event.at).unwrap_or(&event.at).clone();
            (
                function,
                Watched {
                    event: event.name.clone(),
                    domain: outer.domain(&event.domain).to_owned(),
                    renames: below,
                    objects: pointees,
                },
            )
        })
        .collect()
}

/// A central value a plugin wrote, to put back when it stops.
struct Written {
    address: u64,
    original: Vec<u8>,
    value: Vec<u8>,
}

/// The central description every plugin reads through, and the central
/// values plugins wrote.
#[derive(Default)]
pub struct Central {
    described: RwLock<Option<Arc<Described>>>,
    written: Mutex<BTreeMap<String, Vec<Written>>>,
}

impl Central {
    /// Reads through `described` from now on; none without a game map.
    pub fn replace(&self, described: Option<Described>) {
        *self
            .described
            .write()
            .unwrap_or_else(PoisonError::into_inner) = described.map(Arc::new);
    }

    #[must_use]
    pub fn current(&self) -> Option<Arc<Described>> {
        self.described
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, plugin: &str, address: u64, original: Vec<u8>, written: Vec<u8>) {
        let mut all = self.written.lock().unwrap_or_else(PoisonError::into_inner);
        let values = all.entry(plugin.to_owned()).or_default();
        match values.iter_mut().find(|value| value.address == address) {
            Some(value) if value.original.len() == written.len() => value.value = written,
            _ => values.push(Written {
                address,
                original,
                value: written,
            }),
        }
    }

    /// Puts back the central values `plugin` wrote, where they still hold
    /// what it wrote: the game may have changed them since.
    pub fn restore(&self, plugin: &str, game: &dyn crate::game::Game) {
        let values = self
            .written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin)
            .unwrap_or_default();
        for value in values.into_iter().rev() {
            let mut current = vec![0; value.value.len()];
            if game.memory().read(value.address, &mut current) && current == value.value {
                game.write(value.address, &value.original);
            }
        }
    }
}

/// What a handle points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// An object of a structure of the plugin's own entries or of the
    /// central description.
    Struct {
        own: bool,
        name: String,
    },
    /// 8-bit text that ends with a NUL, of at most `most` bytes.
    Text {
        most: usize,
    },
    Array(Arc<ArrayOf>),
    /// Memory of a type no read goes through, by the type's name.
    Opaque(String),
    /// A thunk of the plugin's, of this type.
    Function(angelica_anatomia::FnType),
}

/// An array a handle points to.
#[derive(Debug, PartialEq, Eq)]
pub struct ArrayOf {
    own: bool,
    element: Ty,
    stride: u64,
    /// The elements in use.
    used: u32,
    /// The domains its elements belong to by a field of theirs, and the
    /// domain of an element that names none of them.
    partition: Option<(Partition, String)>,
}

/// A field a plugin looked up: its structure and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRef {
    own: bool,
    structure: String,
    name: String,
}

impl FieldRef {
    fn entry(&self) -> String {
        format!("{}.{}", self.structure, self.name)
    }
}

/// One step from a global to an object.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Field(FieldRef),
    Element(u32),
    Cast { own: bool, name: String },
}

/// How an object was reached from a global.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Path {
    own: bool,
    global: String,
    steps: Vec<Step>,
}

/// What a handle stands for.
#[derive(Debug, Clone)]
pub struct Slot {
    pub address: u64,
    pub target: Target,
    /// The domain renames in force for fields read through it.
    renames: Arc<BTreeMap<String, String>>,
    /// How it was reached from a global; none through a call.
    path: Option<Arc<Path>>,
}

impl Slot {
    /// An object a call gave a handler.
    #[must_use]
    pub fn called(address: u64, target: Target) -> Self {
        Self {
            address,
            target,
            renames: Arc::default(),
            path: None,
        }
    }

    /// Reached from this one by `step`.
    fn below(
        &self,
        address: u64,
        target: Target,
        step: Step,
        renames: Arc<BTreeMap<String, String>>,
    ) -> Self {
        Self {
            address,
            target,
            renames,
            path: self.path.as_ref().map(|path| {
                let mut path = Path::clone(path);
                path.steps.push(step);
                Arc::new(path)
            }),
        }
    }
}

/// A thunk's handle: its slot from 1, with no lifetime bits.
const THUNK: u64 = 0;
const CALL: u64 = 1;
const FRAME: u64 = 2;
const HELD: u64 = 3;
const GENERATION: u64 = (1 << 30) - 1;

/// The handles of one frame of a main instance, or of one call of a
/// handler. A handle is its lifetime in the top two bits, the frame's or
/// call's generation in the next thirty, and its number from 1.
pub struct Handles {
    tag: u64,
    generation: u64,
    slots: Vec<Slot>,
}

impl Default for Handles {
    fn default() -> Self {
        Self {
            tag: FRAME,
            generation: 0,
            slots: Vec::new(),
        }
    }
}

impl Handles {
    /// The handles of the call `serial`.
    #[must_use]
    pub fn call(serial: u64) -> Self {
        Self {
            tag: CALL,
            generation: serial & GENERATION,
            slots: Vec::new(),
        }
    }

    /// Ends every handle and starts the next frame's.
    fn next(&mut self) {
        self.generation = (self.generation + 1) & GENERATION;
        self.slots.clear();
    }

    fn handle(&self, index: usize) -> u64 {
        (self.tag << 62) | (self.generation << 32) | (index as u64 + 1)
    }

    /// A new handle of `slot`.
    ///
    /// # Errors
    /// Past [`MOST_HANDLES`].
    pub fn issue(&mut self, slot: Slot) -> Result<u64> {
        if self.slots.len() >= MOST_HANDLES {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a frame or call has at most {MOST_HANDLES} handles"),
            ));
        }
        self.slots.push(slot);
        Ok(self.handle(self.slots.len() - 1))
    }

    /// The handle of what a call gave at `address`, the same for the same
    /// object.
    pub fn issue_called(
        &mut self,
        address: u64,
        target: Target,
        renames: Arc<BTreeMap<String, String>>,
    ) -> u64 {
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.address == address && slot.target == target)
        {
            return self.handle(index);
        }
        self.slots.push(Slot {
            address,
            target,
            renames,
            path: None,
        });
        self.handle(self.slots.len() - 1)
    }

    /// What `handle` stands for.
    ///
    /// # Errors
    /// A handle of another frame or call.
    pub fn get(&self, handle: u64) -> Result<&Slot> {
        let index = usize::try_from(handle & u64::from(u32::MAX)).unwrap_or(0);
        if handle >> 62 != self.tag || (handle >> 32) & GENERATION != self.generation || index == 0
        {
            return Err(stale(handle));
        }
        self.slots.get(index - 1).ok_or_else(|| stale(handle))
    }
}

/// `function` without the names of its parameters, which do not change
/// how it is called.
#[must_use]
pub fn unnamed(function: &angelica_anatomia::FnType) -> angelica_anatomia::FnType {
    angelica_anatomia::FnType {
        params: function
            .params
            .iter()
            .map(|param| angelica_anatomia::Param {
                name: None,
                ty: param.ty.clone(),
            })
            .collect(),
        ret: function.ret.clone(),
    }
}

/// The handle of the thunk in `slot`.
#[must_use]
pub fn thunk_handle(slot: usize) -> u64 {
    slot as u64 + 1
}

fn stale(handle: u64) -> Error {
    let lifetime = match handle >> 62 {
        CALL => "of an earlier call",
        FRAME => "of an earlier frame",
        HELD => "released",
        _ => "not one the host issued",
    };
    Error::invalid(format!("the handle is {lifetime}"))
}

/// A handle the plugin holds: how to reach its object again, and where
/// it was.
struct Held {
    path: Arc<Path>,
    address: u64,
    target: Target,
}

/// An instance's handles and fields.
#[derive(Default)]
pub struct Instance {
    handles: Handles,
    held: Vec<Option<Held>>,
    fields: Vec<FieldRef>,
}

impl Instance {
    /// Ends the handles of the frame before.
    pub fn next_frame(&mut self) {
        self.handles.next();
    }
}

/// What a read found: a value, or something a handle stands for.
enum Got {
    Plain(api::Value),
    Object(Slot),
    Array(Slot),
    Pointer(Slot),
}

impl Got {
    fn slot(self) -> Option<Slot> {
        match self {
            Self::Plain(_) => None,
            Self::Object(slot) | Self::Array(slot) | Self::Pointer(slot) => Some(slot),
        }
    }
}

/// Gives the slots of `got` the step that reached them from `from`, and
/// the renames in force below.
fn reached(got: Got, from: &Slot, step: Step, renames: Arc<BTreeMap<String, String>>) -> Got {
    let below = |slot: Slot| from.below(slot.address, slot.target, step, renames);
    match got {
        Got::Plain(value) => Got::Plain(value),
        Got::Object(slot) => Got::Object(below(slot)),
        Got::Array(slot) => Got::Array(below(slot)),
        Got::Pointer(slot) => Got::Pointer(below(slot)),
    }
}

fn denied(message: impl Into<String>) -> Error {
    Error::new(Kind::PermissionDenied, message)
}

fn unreadable(at: u64) -> Error {
    Error::new(
        Kind::Unavailable,
        format!("the game's memory at {at:#x} cannot be read"),
    )
}

fn gone(error: &Error) -> Error {
    Error::new(
        Kind::Unavailable,
        format!("the held object is gone: {}", error.message),
    )
}

/// The value of `primitive` whose bits, zero-extended, are `bits`.
fn plain_value(bits: u64, primitive: Primitive) -> api::Value {
    let width = primitive.size() * 8;
    match primitive {
        Primitive::Bool => api::Value::Boolean(bits & 0xFF != 0),
        #[allow(clippy::cast_possible_truncation)]
        Primitive::F32 => api::Value::Float32(f32::from_bits(bits as u32)),
        Primitive::F64 => api::Value::Float64(f64::from_bits(bits)),
        Primitive::I8 | Primitive::I16 | Primitive::I32 | Primitive::I64 => {
            let shift = 64 - width;
            api::Value::Signed((bits << shift).cast_signed() >> shift)
        }
        Primitive::U8 | Primitive::U16 | Primitive::U32 | Primitive::U64 => {
            api::Value::Unsigned(bits)
        }
    }
}

/// The bytes of `value` as a value of `primitive`, within `range`.
fn encode(value: &api::Value, primitive: Primitive, range: Option<(f64, f64)>) -> Result<Vec<u8>> {
    let size = primitive.size() as usize;
    let wrong = || Error::invalid(format!("the value is not a {}", primitive.name()));
    #[allow(clippy::cast_precision_loss)]
    let (bits, number) = match (primitive, value) {
        (Primitive::Bool, api::Value::Boolean(flag)) => {
            (u64::from(*flag), f64::from(u8::from(*flag)))
        }
        (Primitive::F32, api::Value::Float32(number)) => {
            (u64::from(number.to_bits()), f64::from(*number))
        }
        (Primitive::F64, api::Value::Float64(number)) => (number.to_bits(), *number),
        (
            Primitive::I8 | Primitive::I16 | Primitive::I32 | Primitive::I64,
            api::Value::Signed(number),
        ) => {
            let shift = 64 - size * 8;
            if (number << shift) >> shift != *number {
                return Err(wrong());
            }
            (number.cast_unsigned(), *number as f64)
        }
        (
            Primitive::U8 | Primitive::U16 | Primitive::U32 | Primitive::U64,
            api::Value::Unsigned(number),
        ) => {
            if size < 8 && number >> (size * 8) != 0 {
                return Err(wrong());
            }
            (*number, *number as f64)
        }
        _ => return Err(wrong()),
    };
    if let Some((low, high)) = range
        && !(low..=high).contains(&number)
    {
        return Err(Error::invalid(format!(
            "the value is outside {low} to {high}, the range the Lens catalog gives"
        )));
    }
    Ok(bits.to_le_bytes()[..size].to_vec())
}

/// The bytes of a text up to its terminator.
pub(crate) fn read_text(memory: &dyn crate::lens::Memory, at: u64, most: usize) -> Result<Vec<u8>> {
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

/// What reads and writes need: the game, the descriptions, and what the
/// plugin may do.
struct Reader<'a> {
    game: &'a dyn crate::game::Game,
    own: Option<Source<'a>>,
    central: Option<Source<'a>>,
    described: Option<&'a Described>,
    may_read: &'a dyn Fn(&str) -> bool,
    may_change: &'a dyn Fn(&str) -> bool,
    unreviewed: bool,
    /// Where central writes are kept, and whose they are.
    values: &'a Central,
    plugin: &'a str,
}

type Renames = Arc<BTreeMap<String, String>>;

impl<'a> Reader<'a> {
    fn source(&self, own: bool) -> Result<Source<'a>> {
        if own {
            self.own.ok_or_else(|| {
                Error::new(
                    Kind::Unavailable,
                    "the plugin's own entries are not resolved in this game",
                )
            })
        } else {
            self.central
                .ok_or_else(|| Error::new(Kind::Unavailable, "no game map is loaded"))
        }
    }

    fn bytes(&self, at: u64, length: usize) -> Result<Vec<u8>> {
        let mut bytes = vec![0; length];
        if self.game.memory().read(at, &mut bytes) {
            Ok(bytes)
        } else {
            Err(unreadable(at))
        }
    }

    /// The bits of a value of `primitive` at `at`, zero-extended.
    fn raw(&self, at: u64, primitive: Primitive) -> Result<u64> {
        let mut bytes = [0u8; 8];
        let size = primitive.size() as usize;
        if self.game.memory().read(at, &mut bytes[..size]) {
            Ok(u64::from_le_bytes(bytes))
        } else {
            Err(unreadable(at))
        }
    }

    fn word(&self, at: u64) -> Result<u64> {
        self.raw(at, Primitive::U64)
    }

    /// Looks up the field `name`, the plugin's own structures first.
    fn lookup(&self, name: &str) -> Result<FieldRef> {
        for source in self.own.into_iter().chain(self.central) {
            let canonical = source.anatomia.canonical(name);
            let Some((structure, field)) = canonical.rsplit_once('.') else {
                continue;
            };
            let Some(found) = source
                .structure(structure)
                .and_then(|definition| definition.fields.get(field))
            else {
                continue;
            };
            let located = match &found.place {
                Place::Located => canonical.to_owned(),
                Place::Bits { field: holder, .. } => format!("{structure}.{holder}"),
            };
            source.offset(&located)?;
            return Ok(FieldRef {
                own: source.own,
                structure: structure.to_owned(),
                name: field.to_owned(),
            });
        }
        Err(Error::new(
            Kind::NotFound,
            format!("{name} is not a field the plugin's entries or the game map describe"),
        ))
    }

    /// The domain a central field belongs to, unless it is never read.
    fn domain(source: Source<'_>, field: &FieldRef) -> Result<String> {
        let entry = field.entry();
        let meaning = &source.anatomia.meaning;
        let about = meaning.fields.get(&entry);
        if about.is_some_and(|about| about.forbidden) {
            return Err(denied(format!("{entry} is never read by a plugin")));
        }
        about
            .and_then(|about| about.domain.clone())
            .or_else(|| meaning.types.get(&field.structure).cloned())
            .ok_or_else(|| {
                denied(format!(
                    "{entry} belongs to no domain, and no plugin reads it"
                ))
            })
    }

    /// Checks that the plugin may read `field` below `renames`. Gives the
    /// renames in force below it, and how its elements divide by domain.
    fn check_read(
        &self,
        source: Source<'_>,
        field: &FieldRef,
        renames: &Renames,
    ) -> Result<(Renames, Option<(Partition, String)>)> {
        if source.own {
            return if self.unreviewed {
                Ok((renames.clone(), None))
            } else {
                Err(denied(
                    "reading the plugin's own entries needs the unreviewed permission",
                ))
            };
        }
        let domain = Self::domain(source, field)?;
        let about = source.anatomia.meaning.fields.get(&field.entry());
        let partition = about.and_then(|about| about.partition.clone());
        if partition.is_none() {
            let belongs = renames.get(&domain).unwrap_or(&domain);
            if !(self.may_read)(belongs) {
                return Err(denied(format!(
                    "reading {} needs the domain {belongs} in [lens] read",
                    field.entry()
                )));
            }
        }
        let below = match about {
            Some(about) if !about.renames.is_empty() => {
                let mut below = BTreeMap::clone(renames);
                below.extend(about.renames.clone());
                Arc::new(below)
            }
            _ => renames.clone(),
        };
        Ok((below, partition.map(|partition| (partition, domain))))
    }

    /// The description of `field`, where in `slot`'s object the structure
    /// that declares it lies, and the structure and field.
    fn object_of(
        &self,
        slot: &Slot,
        field: &FieldRef,
    ) -> Result<(Source<'a>, u64, &'a Struct, &'a StructField)> {
        let Target::Struct { own, name } = &slot.target else {
            return Err(Error::invalid("the handle is not an object"));
        };
        if *own != field.own {
            return Err(Error::invalid(format!(
                "{} is a field of another description than the object's",
                field.entry()
            )));
        }
        let source = self.source(field.own)?;
        let base = source.base_offset(name, &field.structure).ok_or_else(|| {
            Error::invalid(format!(
                "the object is a {name}, which is not {} and does not extend it",
                field.structure
            ))
        })?;
        let object = slot
            .address
            .checked_add(base)
            .ok_or_else(|| unreadable(slot.address))?;
        let definition = source.structure(&field.structure).ok_or_else(|| {
            Error::new(
                Kind::Unavailable,
                format!("{} cannot be used", field.structure),
            )
        })?;
        let found = definition.fields.get(&field.name).ok_or_else(|| {
            Error::new(Kind::NotFound, format!("{} is not a field", field.entry()))
        })?;
        Ok((source, object, definition, found))
    }

    /// The integer field `name` of an object of `structure` at `object`,
    /// read whatever its domain: a count or a partition's field only says
    /// how much the plugin reads.
    fn integer(&self, source: Source<'_>, object: u64, structure: &str, name: &str) -> Result<u64> {
        let entry = format!("{structure}.{name}");
        let field = source
            .structure(structure)
            .and_then(|definition| definition.fields.get(name))
            .ok_or_else(|| Error::new(Kind::NotFound, format!("{entry} is not a field")))?;
        let primitive = source
            .primitive(&field.ty)
            .filter(|primitive| primitive.is_integer() || *primitive == Primitive::Bool)
            .ok_or_else(|| Error::invalid(format!("{entry} is not an integer")))?;
        self.raw(object + source.offset(&entry)?, primitive)
    }

    /// The holder of a field in bits: its address and type.
    fn holder(
        source: Source<'_>,
        object: u64,
        field: &FieldRef,
        definition: &Struct,
        holder: &str,
    ) -> Result<(u64, Primitive)> {
        let primitive = definition
            .fields
            .get(holder)
            .and_then(|holder| source.primitive(&holder.ty))
            .ok_or_else(|| {
                Error::invalid(format!("{} is not bits of an integer", field.entry()))
            })?;
        let offset = source.offset(&format!("{}.{holder}", field.structure))?;
        Ok((object + offset, primitive))
    }

    /// The value of `field` of the object `slot` stands for.
    fn read_field(&self, slot: &Slot, field: &FieldRef) -> Result<Got> {
        let (source, object, definition, found) = self.object_of(slot, field)?;
        let (renames, partition) = self.check_read(source, field, &slot.renames)?;
        let got = match &found.place {
            Place::Bits {
                field: holder,
                at,
                width,
            } => {
                let (address, primitive) = Self::holder(source, object, field, definition, holder)?;
                let bits = self.raw(address, primitive)?;
                let mask = if *width >= 64 {
                    u64::MAX
                } else {
                    (1 << width) - 1
                };
                let value = (bits >> at) & mask;
                let boolean = matches!(source.plain(&found.ty)?, Ty::Primitive(Primitive::Bool));
                Got::Plain(if boolean {
                    api::Value::Boolean(value != 0)
                } else {
                    api::Value::Unsigned(value)
                })
            }
            Place::Located => {
                let at = object + source.offset(&field.entry())?;
                let used = found
                    .count
                    .as_ref()
                    .map(|count| self.integer(source, object, &field.structure, count))
                    .transpose()?;
                self.typed(source, at, &found.ty, found.text, used, partition)?
            }
        };
        Ok(reached(got, slot, Step::Field(field.clone()), renames))
    }
}

impl Reader<'_> {
    /// What a value of `ty` at `at` gives.
    fn typed(
        &self,
        source: Source<'_>,
        at: u64,
        ty: &Ty,
        text: Option<Text>,
        used: Option<u64>,
        partition: Option<(Partition, String)>,
    ) -> Result<Got> {
        let object = |name: &str, at: u64| {
            Got::Object(Slot::called(
                at,
                Target::Struct {
                    own: source.own,
                    name: name.to_owned(),
                },
            ))
        };
        let opaque = |name: String, at: u64| Got::Pointer(Slot::called(at, Target::Opaque(name)));
        match source.plain(ty)? {
            Ty::Primitive(primitive) => Ok(Got::Plain(plain_value(
                self.raw(at, *primitive)?,
                *primitive,
            ))),
            Ty::Char8 => Ok(Got::Plain(api::Value::Unsigned(
                self.raw(at, Primitive::U8)?,
            ))),
            Ty::Named(name, args) if args.is_empty() => match source.anatomia.types().get(name) {
                Some(TypeDef::Enum(enumeration)) => {
                    let primitive = enumeration
                        .ty
                        .ok_or_else(|| Error::invalid(format!("the enum {name} has no type")))?;
                    Ok(Got::Plain(plain_value(self.raw(at, primitive)?, primitive)))
                }
                Some(TypeDef::Struct(_)) if source.structure(name).is_some() => {
                    Ok(object(name, at))
                }
                _ => Ok(opaque(name.clone(), at)),
            },
            Ty::Array(inner, length) => {
                if matches!(source.plain(inner)?, Ty::Char8) {
                    let bytes = self.bytes(at, *length as usize)?;
                    let end = bytes
                        .iter()
                        .position(|byte| *byte == 0)
                        .unwrap_or(bytes.len());
                    return Ok(Got::Plain(api::Value::Text(bytes[..end].to_vec())));
                }
                Self::array(source, at, inner, *length, used, partition)
            }
            Ty::Ptr(None) => match self.word(at)? {
                0 => Ok(Got::Plain(api::Value::None)),
                pointer => Ok(opaque("ptr".to_owned(), pointer)),
            },
            Ty::Ptr(Some(inner)) => {
                let pointer = self.word(at)?;
                if pointer == 0 {
                    return Ok(Got::Plain(api::Value::None));
                }
                match source.plain(inner)? {
                    Ty::Char8 => {
                        let most = match text {
                            Some(Text::Most(most)) => most as usize,
                            _ => TEXT_MOST,
                        };
                        let text = read_text(self.game.memory(), pointer, most)?;
                        Ok(Got::Plain(api::Value::Text(text)))
                    }
                    Ty::Named(name, args)
                        if args.is_empty() && source.structure(name).is_some() =>
                    {
                        Ok(object(name, pointer))
                    }
                    Ty::Array(element, length) => {
                        Self::array(source, pointer, element, *length, used, partition)
                    }
                    other => Ok(opaque(other.to_string(), pointer)),
                }
            }
            Ty::Opaque => Ok(opaque("opaque".to_owned(), at)),
            Ty::Fn(function) => match self.word(at)? {
                0 => Ok(Got::Plain(api::Value::None)),
                pointer => Ok(opaque(Ty::Fn(function.clone()).to_string(), pointer)),
            },
            other => Err(Error::invalid(format!("a value of {other} cannot be read"))),
        }
    }

    fn array(
        source: Source<'_>,
        at: u64,
        element: &Ty,
        length: u32,
        used: Option<u64>,
        partition: Option<(Partition, String)>,
    ) -> Result<Got> {
        let stride = source
            .size(element)
            .filter(|size| *size > 0)
            .ok_or_else(|| {
                Error::invalid(format!(
                    "elements of {element} have no size in the description"
                ))
            })?;
        let used = used.map_or(length, |used| {
            u32::try_from(used).unwrap_or(u32::MAX).min(length)
        });
        Ok(Got::Array(Slot::called(
            at,
            Target::Array(Arc::new(ArrayOf {
                own: source.own,
                element: element.clone(),
                stride,
                used,
                partition,
            })),
        )))
    }

    /// How many elements of the array `slot` stands for are used.
    fn length(slot: &Slot) -> Result<u32> {
        match &slot.target {
            Target::Array(array) => Ok(array.used),
            _ => Err(Error::invalid("the handle is not an array")),
        }
    }

    /// The element at `index` of the array `slot` stands for.
    fn element(&self, slot: &Slot, index: u32) -> Result<Got> {
        let Target::Array(array) = &slot.target else {
            return Err(Error::invalid("the handle is not an array"));
        };
        if index >= array.used {
            return Err(Error::invalid(format!(
                "element {index} is past the {} elements in use",
                array.used
            )));
        }
        let source = self.source(array.own)?;
        let at = array
            .stride
            .checked_mul(u64::from(index))
            .and_then(|offset| slot.address.checked_add(offset))
            .ok_or_else(|| unreadable(slot.address))?;
        let renames = match &array.partition {
            Some((partition, default)) => self.partitioned(
                source,
                at,
                index,
                &array.element,
                partition,
                default,
                &slot.renames,
            )?,
            None => slot.renames.clone(),
        };
        let got = self.typed(source, at, &array.element, None, None, None)?;
        Ok(reached(got, slot, Step::Element(index), renames))
    }

    /// Checks that the plugin may read the element at `at` of an array its
    /// description divides by domain. Gives the renames in force below it:
    /// its fields of the array's own domain belong to the element's.
    #[allow(clippy::too_many_arguments)]
    fn partitioned(
        &self,
        source: Source<'_>,
        at: u64,
        index: u32,
        element: &Ty,
        partition: &Partition,
        default: &str,
        renames: &Renames,
    ) -> Result<Renames> {
        let Ty::Named(structure, _) = source.plain(element)? else {
            return Err(Error::invalid("a divided array's elements are structures"));
        };
        let field = source
            .structure(structure)
            .and_then(|definition| definition.fields.get(&partition.field))
            .ok_or_else(|| {
                Error::invalid(format!("{structure} has no field {}", partition.field))
            })?;
        let value = self.integer(source, at, structure, &partition.field)?;
        let enumeration = field
            .r#enum
            .clone()
            .or_else(|| match source.plain(&field.ty) {
                Ok(Ty::Named(name, _)) => Some(name.clone()),
                _ => None,
            })
            .and_then(|name| source.enumeration(&name))
            .ok_or_else(|| {
                Error::invalid(format!("{structure}.{} is not an enum", partition.field))
            })?;
        let named = enumeration
            .values
            .iter()
            .find(|(_, number)| **number == value)
            .map(|(name, _)| name)
            .ok_or_else(|| {
                denied(format!(
                    "element {index} has a {} of {value}, which has no name: such an element is never read",
                    partition.field
                ))
            })?;
        let domain = partition
            .domains
            .iter()
            .find(|(_, values)| values.contains(named))
            .map_or(default, |(domain, _)| domain.as_str());
        let belongs = renames
            .get(domain)
            .map_or(domain, String::as_str)
            .to_owned();
        if !(self.may_read)(&belongs) {
            return Err(denied(format!(
                "element {index} is a {named}, and reading it needs the domain {belongs} in [lens] read"
            )));
        }
        if belongs == default {
            return Ok(renames.clone());
        }
        let mut below = BTreeMap::clone(renames);
        below.insert(default.to_owned(), belongs);
        Ok(Arc::new(below))
    }
}

impl Reader<'_> {
    /// The object of the global `name`, the plugin's own entries first.
    fn global(&self, name: &str) -> Result<Got> {
        for source in self.own.into_iter().chain(self.central) {
            let canonical = source.anatomia.canonical(name);
            if source.anatomia.get(canonical).is_some() {
                return self.global_in(source, canonical);
            }
        }
        Err(Error::new(
            Kind::NotFound,
            format!("{name} is not an entry of the plugin's or of the game map"),
        ))
    }

    fn global_in(&self, source: Source<'_>, name: &str) -> Result<Got> {
        let Some(EntryKind::Global { ty }) = source.anatomia.get(name).map(|entry| &entry.kind)
        else {
            return Err(Error::invalid(format!("{name} is not a global")));
        };
        if source.own && !self.unreviewed {
            return Err(denied(
                "reading the plugin's own entries needs the unreviewed permission",
            ));
        }
        // A central global's value belongs to no domain: only the objects
        // it holds or points to are read, field by field.
        let object = |ty: &Ty| matches!(source.plain(ty), Ok(Ty::Named(inner, _)) if source.structure(inner).is_some());
        let reaches = match source.plain(ty)? {
            Ty::Ptr(Some(inner)) => object(inner),
            plain => object(plain),
        };
        if !source.own && !reaches {
            return Err(denied(format!(
                "{name} holds no object, and the game map's values are read only as fields"
            )));
        }
        let got = self.typed(source, source.address(name)?, ty, None, None, None)?;
        let renames = Arc::new(if source.own {
            BTreeMap::new()
        } else {
            self.described
                .and_then(|described| described.roots.get(name))
                .cloned()
                .unwrap_or_default()
        });
        let path = Arc::new(Path {
            own: source.own,
            global: name.to_owned(),
            steps: Vec::new(),
        });
        let start = |slot: Slot| Slot {
            renames: renames.clone(),
            path: Some(path.clone()),
            ..slot
        };
        Ok(match got {
            Got::Object(slot) => Got::Object(start(slot)),
            Got::Array(slot) => Got::Array(start(slot)),
            Got::Pointer(slot) => Got::Pointer(start(slot)),
            Got::Plain(value) => Got::Plain(value),
        })
    }

    /// The structure among `structure` and its bases that declares
    /// `field`.
    fn declaring(source: Source<'_>, structure: &str, field: &str, depth: usize) -> Option<String> {
        let definition = source.structure(structure)?;
        if definition.fields.contains_key(field) {
            return Some(structure.to_owned());
        }
        if depth == MOST_BASES {
            return None;
        }
        definition
            .extends
            .iter()
            .find_map(|(base, _)| Self::declaring(source, base, field, depth + 1))
    }

    /// The object `slot` stands for as a `structure`, when it is one.
    fn cast(&self, slot: &Slot, structure: &str) -> Result<Option<Slot>> {
        let Target::Struct { own, name } = &slot.target else {
            return Err(Error::invalid("the handle is not an object"));
        };
        let source = self.source(*own)?;
        let wanted = source.anatomia.canonical(structure);
        let definition = source
            .structure(wanted)
            .ok_or_else(|| Error::new(Kind::NotFound, format!("{structure} is not a structure")))?;
        let cast = |address: u64| {
            let target = Target::Struct {
                own: *own,
                name: wanted.to_owned(),
            };
            let step = Step::Cast {
                own: *own,
                name: wanted.to_owned(),
            };
            slot.below(address, target, step, slot.renames.clone())
        };
        if let Some(up) = source.base_offset(name, wanted) {
            return Ok(Some(cast(slot.address + up)));
        }
        let down = source.base_offset(wanted, name).ok_or_else(|| {
            Error::invalid(format!(
                "{wanted} neither extends {name} nor is extended by it"
            ))
        })?;
        let address = slot
            .address
            .checked_sub(down)
            .ok_or_else(|| unreadable(slot.address))?;
        let is = if let Some((field, values)) = &definition.variant {
            let owner = Self::declaring(source, wanted, field, 0).ok_or_else(|| {
                Error::invalid(format!("no base of {wanted} has the field {field}"))
            })?;
            let base = source.base_offset(wanted, &owner).unwrap_or(0);
            let value = self.integer(source, address + base, &owner, field)?;
            let names = source
                .structure(&owner)
                .and_then(|owner| owner.fields.get(field))
                .and_then(|field| field.r#enum.as_ref())
                .and_then(|name| source.enumeration(name));
            values.iter().any(|wanted| match wanted {
                toml::Value::Integer(number) => number.cast_unsigned() == value,
                toml::Value::String(value_name) => names
                    .and_then(|names| names.values.get(value_name))
                    .is_some_and(|number| *number == value),
                _ => false,
            })
        } else if let Some(vtable) = &definition.vtable {
            self.word(address)? == source.address(vtable)?
        } else {
            return Err(Error::invalid(format!(
                "the description cannot tell whether an object is a {wanted}"
            )));
        };
        Ok(is.then(|| cast(address)))
    }

    /// The object `path` reaches now.
    fn walk(&self, path: &Path) -> Result<Slot> {
        let lost = || Error::new(Kind::Unavailable, "the path reaches nothing now");
        let mut slot = self
            .global_in(self.source(path.own)?, &path.global)?
            .slot()
            .ok_or_else(lost)?;
        for step in &path.steps {
            slot = match step {
                Step::Field(field) => self.read_field(&slot, field)?.slot(),
                Step::Element(index) => self.element(&slot, *index)?.slot(),
                Step::Cast { name, .. } => self.cast(&slot, name)?,
            }
            .ok_or_else(lost)?;
        }
        Ok(slot)
    }

    /// Where the text of the object `slot` stands for is, and the most
    /// bytes it holds; a zero address for no text.
    fn text_at(&self, slot: &Slot) -> Result<(u64, usize)> {
        let (mut own, mut structure) = match &slot.target {
            Target::Text { most } => return Ok((slot.address, *most)),
            Target::Struct { own, name } => (*own, name.clone()),
            _ => return Err(Error::invalid("the handle is neither text nor an object")),
        };
        let mut object = slot.clone();
        for _ in 0..TEXT_DEPTH {
            let source = self.source(own)?;
            let name = source
                .structure(&structure)
                .and_then(|definition| definition.text.clone())
                .ok_or_else(|| Error::invalid(format!("{structure} does not read as text")))?;
            let field = FieldRef {
                own,
                structure: structure.clone(),
                name,
            };
            let (_, at, _, found) = self.object_of(&object, &field)?;
            let (renames, _) = self.check_read(source, &field, &object.renames)?;
            if !matches!(found.place, Place::Located) {
                return Err(Error::invalid("text is not in bits"));
            }
            let at = at + source.offset(&field.entry())?;
            match (source.plain(&found.ty)?, found.text) {
                (Ty::Array(inner, length), Some(Text::Terminated))
                    if matches!(source.plain(inner)?, Ty::Char8) =>
                {
                    return Ok((at, *length as usize));
                }
                (Ty::Ptr(_), Some(Text::Most(most))) => return Ok((self.word(at)?, most as usize)),
                (Ty::Named(inner, _), Some(Text::Most(_))) => {
                    structure.clone_from(inner);
                    own = source.own;
                    object = Slot {
                        address: at,
                        target: Target::Struct {
                            own,
                            name: structure.clone(),
                        },
                        renames,
                        path: None,
                    };
                }
                _ => return Err(Error::invalid(format!("{} is not text", field.entry()))),
            }
        }
        Err(Error::invalid("the text lies too deep in structures"))
    }

    /// The text of the object `slot` stands for.
    fn text(&self, slot: &Slot) -> Result<Vec<u8>> {
        match self.text_at(slot)? {
            (0, _) => Ok(Vec::new()),
            (at, most) => read_text(self.game.memory(), at, most),
        }
    }

    /// Replaces the text of the object `slot` stands for with as many
    /// bytes, in place: the plugin's own objects and text a call gave.
    fn edit_text(&self, slot: &Slot, bytes: &[u8]) -> Result<()> {
        match &slot.target {
            Target::Struct { own: false, .. } => {
                return Err(denied(
                    "the text of the game map's objects is changed only through Lens",
                ));
            }
            _ if !self.unreviewed => {
                return Err(denied(
                    "changing the plugin's own entries needs the unreviewed permission",
                ));
            }
            _ => {}
        }
        let (at, most) = self.text_at(slot)?;
        let current = if at == 0 {
            Vec::new()
        } else {
            read_text(self.game.memory(), at, most)?
        };
        if bytes.len() != current.len() || bytes.contains(&0) {
            return Err(Error::invalid(format!(
                "the new text has {} bytes or a NUL; the text has {} bytes",
                bytes.len(),
                current.len()
            )));
        }
        if bytes.is_empty() || bytes == current.as_slice() {
            return Ok(());
        }
        if self.game.write(at, bytes) {
            Ok(())
        } else {
            Err(Error::new(
                Kind::Unavailable,
                "the text is not in writable memory",
            ))
        }
    }
}

impl Reader<'_> {
    /// Checks that the plugin may write `field` below `renames`: the range
    /// the Lens catalog gives a central field.
    fn check_write(
        &self,
        source: Source<'_>,
        field: &FieldRef,
        renames: &Renames,
    ) -> Result<Option<(f64, f64)>> {
        if source.own {
            return if self.unreviewed {
                Ok(None)
            } else {
                Err(denied(
                    "writing the plugin's own entries needs the unreviewed permission",
                ))
            };
        }
        let entry = field.entry();
        let domain = Self::domain(source, field)?;
        let belongs = renames.get(&domain).unwrap_or(&domain);
        let change = self
            .described
            .and_then(|described| described.changes.get(&entry))
            .ok_or_else(|| {
                denied(format!(
                    "the Lens catalog offers no change of {entry} written in memory"
                ))
            })?;
        if !(self.may_change)(belongs) {
            return Err(denied(format!(
                "writing {entry} needs the domain {belongs} in [lens] change"
            )));
        }
        Ok(change.range)
    }

    /// Writes `value` into `field` of the object `slot` stands for; `given`
    /// is what a handle in `value` stands for.
    fn write(
        &self,
        slot: &Slot,
        field: &FieldRef,
        value: &api::Value,
        given: Option<&Slot>,
    ) -> Result<()> {
        let (source, object, definition, found) = self.object_of(slot, field)?;
        let range = self.check_write(source, field, &slot.renames)?;
        let wrong = || Error::invalid(format!("the value does not fit {}", field.entry()));
        let (at, bytes) = match &found.place {
            Place::Bits {
                field: holder,
                at: bit,
                width,
            } => {
                let (address, primitive) = Self::holder(source, object, field, definition, holder)?;
                let number = match value {
                    api::Value::Boolean(flag) => u64::from(*flag),
                    api::Value::Unsigned(number) => *number,
                    _ => return Err(wrong()),
                };
                let mask = if *width >= 64 {
                    u64::MAX
                } else {
                    (1 << width) - 1
                };
                if number & !mask != 0 {
                    return Err(wrong());
                }
                #[allow(clippy::cast_precision_loss)]
                if let Some((low, high)) = range
                    && !(low..=high).contains(&(number as f64))
                {
                    return Err(Error::invalid(format!(
                        "the value is outside {low} to {high}, the range the Lens catalog gives"
                    )));
                }
                let current = self.raw(address, primitive)?;
                let changed = (current & !(mask << bit)) | (number << bit);
                let size = primitive.size() as usize;
                (address, changed.to_le_bytes()[..size].to_vec())
            }
            Place::Located => {
                let at = object + source.offset(&field.entry())?;
                let bytes = if let Some(primitive) = source.primitive(&found.ty) {
                    encode(value, primitive, range)?
                } else {
                    Self::placed(source, &found.ty, value, given, &field.entry())?
                };
                (at, bytes)
            }
        };
        let original = self.bytes(at, bytes.len())?;
        if original == bytes {
            return Ok(());
        }
        if !self.game.write(at, &bytes) {
            return Err(Error::new(
                Kind::Unavailable,
                format!("{} is not in writable memory", field.entry()),
            ));
        }
        if !source.own {
            self.values.record(self.plugin, at, original, bytes);
        }
        Ok(())
    }

    /// The bytes a value of `ty`, which no primitive stores, takes in
    /// memory: text in a text field's array, or a pointer; `given` is what
    /// a handle in `value` stands for.
    fn placed(
        source: Source<'_>,
        ty: &Ty,
        value: &api::Value,
        given: Option<&Slot>,
        entry: &str,
    ) -> Result<Vec<u8>> {
        let wrong = || Error::invalid(format!("the value does not fit {entry}"));
        Ok(match (source.plain(ty)?, value) {
            (Ty::Array(inner, length), api::Value::Text(text))
                if matches!(source.plain(inner)?, Ty::Char8) =>
            {
                if text.len() >= *length as usize || text.contains(&0) {
                    return Err(Error::invalid(format!(
                        "the text needs fewer than {length} bytes and no NUL"
                    )));
                }
                let mut bytes = text.clone();
                bytes.push(0);
                bytes
            }
            (Ty::Fn(function), _) => {
                let address = match (value, given) {
                    (api::Value::None, _) => 0,
                    (_, Some(given)) if given.target == Target::Function(unnamed(function)) => {
                        given.address
                    }
                    _ => return Err(wrong()),
                };
                address.to_le_bytes().to_vec()
            }
            (Ty::Ptr(inner), _) => {
                let address = match (value, given) {
                    (api::Value::None, _) => 0,
                    (_, Some(given)) => {
                        Self::pointee(source, inner.as_deref(), given)?;
                        given.address
                    }
                    _ => return Err(wrong()),
                };
                address.to_le_bytes().to_vec()
            }
            _ => return Err(Error::invalid(format!("{entry} cannot be written"))),
        })
    }

    /// Checks that `given` may go into a pointer to `inner`: an object of
    /// its structure or of one that extends it from its start, or memory
    /// of the same other type.
    fn pointee(source: Source<'_>, inner: Option<&Ty>, given: &Slot) -> Result<()> {
        let fits = match (
            inner.map(|inner| source.plain(inner)).transpose()?,
            &given.target,
        ) {
            (Some(Ty::Named(name, _)), Target::Struct { own, name: object })
                if source.structure(name).is_some() =>
            {
                *own == source.own && source.base_offset(object, name) == Some(0)
            }
            (Some(Ty::Fn(function)), Target::Function(thunk)) => unnamed(function) == *thunk,
            (Some(other), Target::Opaque(kind)) => *kind == other.to_string(),
            (None, Target::Opaque(kind)) => kind == "ptr",
            _ => false,
        };
        if fits {
            Ok(())
        } else {
            Err(Error::invalid(
                "the handle is not of the type the pointer field points to",
            ))
        }
    }
}

impl State {
    /// Runs `work` with what this instance reads the game with.
    fn reading<T>(&self, work: impl FnOnce(&Reader<'_>) -> Result<T>) -> Result<T> {
        if !matches!(self.role, Role::Main | Role::Intercept(Some(_))) {
            return Err(Error::new(
                Kind::WrongContext,
                "only the main instance and handlers read the game's memory",
            ));
        }
        self.describing(work)
    }

    /// Runs `work` with what this instance reads the game with, also while
    /// an intercept instance prepares, which looks fields up and reads
    /// nothing.
    fn describing<T>(&self, work: impl FnOnce(&Reader<'_>) -> Result<T>) -> Result<T> {
        let context = &self.context;
        let own = context.own.clone();
        let described = context.services.memory.current();
        let may_read = |domain: &str| context.lens_read.contains(domain);
        let may_change = |domain: &str| context.lens_change.contains(domain);
        let reader = Reader {
            game: &*context.services.game,
            own: own.as_deref().map(crate::intercepts::Own::source),
            central: described.as_deref().map(Described::source),
            described: described.as_deref(),
            may_read: &may_read,
            may_change: &may_change,
            unreviewed: context
                .permissions
                .contains(&crate::manifest::Permission::Unreviewed),
            values: &context.services.memory,
            plugin: &context.id,
        };
        work(&reader)
    }

    /// What `handle` stands for now.
    fn slot_of(&self, reader: &Reader<'_>, handle: u64) -> Result<Slot> {
        match (handle >> 62, &self.role) {
            (CALL, Role::Intercept(Some(frame))) => frame.handles.get(handle).cloned(),
            (FRAME, Role::Main) => self.memory.handles.get(handle).cloned(),
            (HELD, Role::Main) => {
                let index = usize::try_from(handle & u64::from(u32::MAX)).unwrap_or(0);
                let held = index
                    .checked_sub(1)
                    .and_then(|index| self.memory.held.get(index))
                    .and_then(Option::as_ref)
                    .ok_or_else(|| stale(handle))?;
                let slot = reader.walk(&held.path).map_err(|error| gone(&error))?;
                if slot.address == held.address && slot.target == held.target {
                    Ok(slot)
                } else {
                    Err(Error::new(
                        Kind::Unavailable,
                        "the held object is gone: its path reaches another one now",
                    ))
                }
            }
            (THUNK, _) => {
                let thunk = usize::try_from(handle)
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|slot| self.context.services.intercepts.thunk_of(slot))
                    .filter(|(plugin, _, _)| *plugin == self.context.id)
                    .ok_or_else(|| stale(handle))?;
                Ok(Slot::called(thunk.1, Target::Function(thunk.2)))
            }
            (CALL | FRAME | HELD, _) => Err(Error::new(
                Kind::WrongContext,
                "the handle belongs to another instance",
            )),
            _ => Err(stale(handle)),
        }
    }

    /// The value of `got`, with a new handle of this frame or call for what
    /// it stands for.
    fn issue(&mut self, got: Got) -> Result<api::Value> {
        let handles = match &mut self.role {
            Role::Intercept(Some(frame)) => &mut frame.handles,
            _ => &mut self.memory.handles,
        };
        Ok(match got {
            Got::Plain(value) => value,
            Got::Object(slot) => api::Value::Object(handles.issue(slot)?),
            Got::Array(slot) => api::Value::Array(handles.issue(slot)?),
            Got::Pointer(slot) => api::Value::Pointer(handles.issue(slot)?),
        })
    }

    fn field_ref(&self, field: api::FieldId) -> Result<FieldRef> {
        usize::try_from(field)
            .ok()
            .and_then(|index| self.memory.fields.get(index))
            .cloned()
            .ok_or_else(|| Error::invalid("the field was not looked up in this instance"))
    }

    fn read_one(&mut self, of: api::Handle, field: api::FieldId) -> Result<api::Value> {
        let field = self.field_ref(field)?;
        let got = self.reading(|reader| reader.read_field(&self.slot_of(reader, of)?, &field))?;
        self.issue(got)
    }

    fn main_memory(&self) -> Result<()> {
        if matches!(self.role, Role::Main) {
            Ok(())
        } else {
            Err(Error::new(
                Kind::WrongContext,
                "only the main instance holds handles",
            ))
        }
    }
}

impl api::Host for State {
    fn global(&mut self, name: String) -> std::result::Result<api::Value, api::Error> {
        self.host(|state| {
            let got = state.reading(|reader| reader.global(&name))?;
            state.issue(got)
        })
        .map_err(Into::into)
    }

    fn field(&mut self, name: String) -> std::result::Result<api::FieldId, api::Error> {
        self.host(|state| {
            if matches!(state.role, Role::Job(_)) {
                return Err(Error::new(
                    Kind::WrongContext,
                    "a job looks up no field of the game",
                ));
            }
            let found = state.describing(|reader| reader.lookup(&name))?;
            let fields = &mut state.memory.fields;
            let index = match fields.iter().position(|field| *field == found) {
                Some(index) => index,
                None if fields.len() >= MOST_FIELDS => {
                    return Err(Error::new(
                        Kind::LimitExceeded,
                        format!("an instance looks up at most {MOST_FIELDS} fields"),
                    ));
                }
                None => {
                    fields.push(found);
                    fields.len() - 1
                }
            };
            Ok(u32::try_from(index).unwrap_or(u32::MAX))
        })
        .map_err(Into::into)
    }

    fn read(
        &mut self,
        of: api::Handle,
        field: api::FieldId,
    ) -> std::result::Result<api::Value, api::Error> {
        self.host(|state| state.read_one(of, field))
            .map_err(Into::into)
    }

    fn read_batch(
        &mut self,
        reads: Vec<(api::Handle, api::FieldId)>,
    ) -> Vec<std::result::Result<api::Value, api::Error>> {
        if reads.len() > MOST_BATCH {
            let error = Error::new(
                Kind::LimitExceeded,
                format!("a batch has at most {MOST_BATCH} reads"),
            );
            return vec![Err(error.into())];
        }
        self.host(|state| {
            reads
                .into_iter()
                .map(|(of, field)| state.read_one(of, field).map_err(Into::into))
                .collect()
        })
    }

    fn length(&mut self, of: api::Handle) -> std::result::Result<u32, api::Error> {
        self.host(|state| state.reading(|reader| Reader::length(&state.slot_of(reader, of)?)))
            .map_err(Into::into)
    }

    fn element(
        &mut self,
        of: api::Handle,
        index: u32,
    ) -> std::result::Result<api::Value, api::Error> {
        self.host(|state| {
            let got = state.reading(|reader| reader.element(&state.slot_of(reader, of)?, index))?;
            state.issue(got)
        })
        .map_err(Into::into)
    }

    fn cast(
        &mut self,
        of: api::Handle,
        structure: String,
    ) -> std::result::Result<Option<api::Handle>, api::Error> {
        self.host(|state| {
            let cast =
                state.reading(|reader| reader.cast(&state.slot_of(reader, of)?, &structure))?;
            match cast {
                Some(slot) => match state.issue(Got::Object(slot))? {
                    api::Value::Object(handle) => Ok(Some(handle)),
                    _ => Ok::<_, Error>(None),
                },
                None => Ok(None),
            }
        })
        .map_err(Into::into)
    }

    fn text(&mut self, of: api::Handle) -> std::result::Result<Vec<u8>, api::Error> {
        self.host(|state| state.reading(|reader| reader.text(&state.slot_of(reader, of)?)))
            .map_err(Into::into)
    }

    fn edit_text(
        &mut self,
        of: api::Handle,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), api::Error> {
        self.host(|state| {
            if let Role::Intercept(Some(frame)) = &state.role
                && !frame.may_change()
            {
                return Err(denied(
                    "changing this call's text needs its event's domain in [lens] change",
                ));
            }
            state.reading(|reader| reader.edit_text(&state.slot_of(reader, of)?, &bytes))
        })
        .map_err(Into::into)
    }

    fn write(
        &mut self,
        of: api::Handle,
        field: api::FieldId,
        value: api::Value,
    ) -> std::result::Result<(), api::Error> {
        self.host(|state| {
            let field = state.field_ref(field)?;
            state.reading(|reader| {
                let slot = state.slot_of(reader, of)?;
                let given = match value {
                    api::Value::Object(handle)
                    | api::Value::Array(handle)
                    | api::Value::Pointer(handle) => Some(state.slot_of(reader, handle)?),
                    _ => None,
                };
                reader.write(&slot, &field, &value, given.as_ref())
            })
        })
        .map_err(Into::into)
    }

    fn hold(&mut self, of: api::Handle) -> std::result::Result<api::Handle, api::Error> {
        self.host(|state| {
            state.main_memory()?;
            let slot = state.reading(|reader| state.slot_of(reader, of))?;
            let path = slot.path.clone().ok_or_else(|| {
                Error::invalid("a handle reached through an intercepted call cannot be held")
            })?;
            let held = Held {
                path,
                address: slot.address,
                target: slot.target,
            };
            let all = &mut state.memory.held;
            let index = if let Some(free) = all.iter().position(Option::is_none) {
                all[free] = Some(held);
                free
            } else if all.len() < MOST_HELD {
                all.push(Some(held));
                all.len() - 1
            } else {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!("a plugin holds at most {MOST_HELD} handles"),
                ));
            };
            Ok((HELD << 62) | (index as u64 + 1))
        })
        .map_err(Into::into)
    }

    fn release(&mut self, held: api::Handle) {
        if held >> 62 != HELD || !matches!(self.role, Role::Main) {
            return;
        }
        let index = usize::try_from(held & u64::from(u32::MAX)).unwrap_or(0);
        if let Some(slot) = index
            .checked_sub(1)
            .and_then(|index| self.memory.held.get_mut(index))
        {
            *slot = None;
        }
    }
}

/// The 64 bits `value` passes as an argument of the type `ty` of the
/// plugin's own entries; text the call reads is kept in `texts` until the
/// call returns.
fn argument(
    state: &State,
    reader: &Reader<'_>,
    source: Source<'_>,
    ty: &Ty,
    value: &api::Value,
    texts: &mut Vec<Vec<u8>>,
) -> Result<u64> {
    if let Some(primitive) = source.primitive(ty) {
        let mut bits = [0u8; 8];
        let bytes = encode(value, primitive, None)?;
        bits[..bytes.len()].copy_from_slice(&bytes);
        return Ok(u64::from_le_bytes(bits));
    }
    let given = match value {
        api::Value::None => return Ok(0),
        api::Value::Object(handle) | api::Value::Array(handle) | api::Value::Pointer(handle) => {
            Some(state.slot_of(reader, *handle)?)
        }
        _ => None,
    };
    let wrong = || Error::invalid(format!("the value is not a {ty}"));
    match (source.plain(ty)?, value, given) {
        (Ty::Ptr(Some(inner)), api::Value::Text(text), _)
            if matches!(source.plain(inner)?, Ty::Char8) =>
        {
            if text.contains(&0) {
                return Err(Error::invalid("text passed to a call has no NUL"));
            }
            let mut terminated = text.clone();
            terminated.push(0);
            let pointer = terminated.as_ptr() as u64;
            texts.push(terminated);
            Ok(pointer)
        }
        (Ty::Fn(function), _, Some(given)) => {
            if given.target == Target::Function(unnamed(function)) {
                Ok(given.address)
            } else {
                Err(Error::invalid(format!(
                    "the handle is not a thunk of the type {ty}"
                )))
            }
        }
        (Ty::Ptr(inner), _, Some(given)) => {
            Reader::pointee(source, inner.as_deref(), &given)?;
            Ok(given.address)
        }
        _ => Err(wrong()),
    }
}

/// What a call of a function that returns `ty` gave.
fn returned(source: Source<'_>, ty: &Ty, returned: Returned) -> Result<Got> {
    if let Some(primitive) = source.primitive(ty) {
        let bits = if matches!(primitive, Primitive::F32 | Primitive::F64) {
            returned.float
        } else {
            returned.integer
        };
        let size = primitive.size() * 8;
        let bits = if size == 64 {
            bits
        } else {
            bits & ((1 << size) - 1)
        };
        return Ok(Got::Plain(plain_value(bits, primitive)));
    }
    let address = returned.integer;
    let target = match source.plain(ty)? {
        _ if address == 0 => return Ok(Got::Plain(api::Value::None)),
        Ty::Ptr(Some(inner)) => match source.plain(inner)? {
            Ty::Named(name, args) if args.is_empty() && source.structure(name).is_some() => {
                return Ok(Got::Object(Slot::called(
                    address,
                    Target::Struct {
                        own: source.own,
                        name: name.clone(),
                    },
                )));
            }
            Ty::Char8 => Target::Text { most: TEXT_MOST },
            other => Target::Opaque(other.to_string()),
        },
        Ty::Ptr(None) => Target::Opaque("ptr".to_owned()),
        Ty::Fn(function) => Target::Opaque(Ty::Fn(function.clone()).to_string()),
        other => {
            return Err(Error::invalid(format!(
                "a call cannot return a value of {other}"
            )));
        }
    };
    Ok(Got::Pointer(Slot::called(address, target)))
}

impl calls_api::Host for State {
    fn call(
        &mut self,
        function: String,
        args: Vec<api::Value>,
    ) -> std::result::Result<api::Value, api::Error> {
        self.host(|state| {
            let context = state.context.clone();
            let own = crate::intercepts::calling(&context)?;
            let source = own.source();
            let name = own.anatomia.canonical(&function).to_owned();
            let Some(EntryKind::Function { ty, thread, .. }) =
                own.anatomia.get(&name).map(|entry| &entry.kind)
            else {
                return Err(Error::new(
                    Kind::NotFound,
                    format!("{function} is not a function of the plugin's entries"),
                ));
            };
            match (&state.role, thread) {
                (Role::Main, _) | (Role::Intercept(Some(_)), Thread::Any) => {}
                (Role::Intercept(Some(_)), Thread::Main) => {
                    return Err(Error::new(
                        Kind::WrongContext,
                        format!("{name} runs on the main thread: only the main instance calls it"),
                    ));
                }
                _ => {
                    return Err(Error::new(
                        Kind::WrongContext,
                        "only the main instance and handlers call the game",
                    ));
                }
            }
            if args.len() != ty.params.len() {
                return Err(Error::invalid(format!(
                    "{name} takes {} arguments",
                    ty.params.len()
                )));
            }
            if args.len() > crate::intercepts::MAX_ARGS {
                return Err(Error::invalid(format!(
                    "a call passes at most {} arguments",
                    crate::intercepts::MAX_ARGS
                )));
            }
            let address = own.function(&name)?;
            let mut texts = Vec::new();
            let bits = state.reading(|reader| {
                ty.params
                    .iter()
                    .zip(&args)
                    .map(|(param, value)| {
                        argument(state, reader, source, &param.ty, value, &mut texts)
                    })
                    .collect::<Result<Vec<u64>>>()
            })?;
            let result = context
                .services
                .game
                .call(address, &bits)
                .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
            drop(texts);
            let got = match &ty.ret {
                None => Got::Plain(api::Value::None),
                Some(ret) => returned(source, ret, result)?,
            };
            state.issue(got)
        })
        .map_err(Into::into)
    }

    fn thunk(
        &mut self,
        signature: String,
        handler: u32,
        fallback: Option<String>,
    ) -> std::result::Result<api::Handle, api::Error> {
        self.host(|state| {
            state.main_memory()?;
            let context = state.context.clone();
            let (slot, _) = context.services.intercepts.thunk(
                &context,
                &signature,
                handler,
                fallback.as_deref(),
            )?;
            Ok::<_, Error>(thunk_handle(slot))
        })
        .map_err(Into::into)
    }
}

impl crate::sandbox::bindings::angelica::plugin::game_map::Host for State {
    fn files(&mut self) -> Vec<(String, String)> {
        self.context
            .services
            .memory
            .current()
            .map_or_else(Vec::new, |described| described.files.as_ref().clone())
    }
}
