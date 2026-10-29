//! The game's objects for Lens: a test names a game map's object model and
//! sets values by paths through it. The objects are laid out in memory as
//! the map describes them, and plugins' queries read that memory through
//! the same planner and reader the game process uses, so a test checks a
//! plugin's queries and its permissions together.
use angelica_map::{EntryKind, Field, Length, Map, Meaning, Shape, Value as Resolved, ValueKind};
use runtime::lens::{
    Memory, Schema,
    query::{self, Filter, Literal, Op, Test},
};
use std::collections::BTreeMap;

/// Where the objects start in the scripted game's memory.
pub(crate) const BASE: u64 = 0x1000_0000;
const PAGE: usize = 4096;

/// An argument of a call a test makes.
#[derive(Debug, Clone, PartialEq)]
pub enum Fire {
    Value(Set),
    /// A pointer to the object a path names, as [`Objects::set`] names it.
    Object(String),
    /// A pointer to NUL-terminated text.
    Text(String),
    /// A pointer to a string object whose first word points to
    /// NUL-terminated text, as the game's strings hold their bytes.
    String(String),
}

impl<T: Into<Set>> From<T> for Fire {
    fn from(value: T) -> Self {
        Self::Value(value.into())
    }
}

/// A value a test sets.
#[derive(Debug, Clone, PartialEq)]
pub enum Set {
    Integer(i128),
    Float(f64),
    Boolean(bool),
    Text(String),
    /// A name of an enum's value, or of a flag.
    Name(String),
}

impl From<i64> for Set {
    fn from(value: i64) -> Self {
        Self::Integer(i128::from(value))
    }
}

impl From<i32> for Set {
    fn from(value: i32) -> Self {
        Self::Integer(i128::from(value))
    }
}

impl From<u32> for Set {
    fn from(value: u32) -> Self {
        Self::Integer(i128::from(value))
    }
}

impl From<f64> for Set {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for Set {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<&str> for Set {
    /// Text; [`Set::Name`] names an enum's value.
    fn from(value: &str) -> Self {
        Self::Text(value.into())
    }
}

/// The scripted game's objects: one stretch of memory from [`BASE`] in
/// whole pages, so that every page the reader asks for is readable.
#[derive(Debug, Default)]
pub struct Objects {
    map: Option<Map>,
    memory: Vec<u8>,
    /// The bytes allocated so far.
    used: usize,
    /// The address of each root's global, by entry name.
    globals: BTreeMap<String, u64>,
    /// The entry of each function an action calls, by its address.
    pub code: BTreeMap<u64, String>,
    /// The field each function that sets a value sets, by its address: its
    /// type and name.
    setters: BTreeMap<u64, (String, String)>,
    /// Every entry of the map in the scripted build, or why it is absent.
    resolved: BTreeMap<String, Result<Resolved, String>>,
}

impl Memory for Objects {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        let Some(start) = address
            .checked_sub(BASE)
            .and_then(|start| usize::try_from(start).ok())
        else {
            return false;
        };
        match self.memory.get(start..start + buffer.len()) {
            Some(bytes) => {
                buffer.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

fn size(kind: ValueKind) -> usize {
    match kind {
        ValueKind::Bool | ValueKind::U8 | ValueKind::I8 => 1,
        ValueKind::U16 | ValueKind::I16 => 2,
        ValueKind::U32 | ValueKind::I32 | ValueKind::F32 => 4,
        ValueKind::U64 | ValueKind::I64 | ValueKind::F64 | ValueKind::Ptr => 8,
        ValueKind::Struct | ValueKind::Void => 0,
    }
}

fn most(length: &Length) -> usize {
    match length {
        Length::Fixed(length) | Length::Field { most: length, .. } => usize::from(*length),
    }
}

impl Objects {
    /// The objects of `map`, none yet: every root's global is zero. Entries
    /// in `absent` are absent in the scripted build. Fields need constant
    /// offsets; one bound to code is absent.
    #[must_use]
    pub fn new(map: Map, absent: &[&str]) -> (Self, Schema) {
        let mut objects = Self::default();
        for root in map.objects().roots.values() {
            let length = if pointer(&map, &root.global) {
                8
            } else {
                size_of(&map, &root.r#type)
            };
            let at = objects.allocate(length);
            objects.globals.insert(root.global.clone(), at);
        }
        // Events' code gets a place of its own; nothing runs there.
        for event in map.objects().events.values() {
            let at = objects.allocate(16);
            objects.globals.insert(event.at.clone(), at);
        }
        // Actions' functions get a place too, and their globals an object:
        // 256 bytes for a structure, and for a pointer one to such.
        for action in map.objects().actions.values() {
            let at = objects.allocate(16);
            objects.globals.insert(action.call.clone(), at);
            objects.code.insert(at, action.call.clone());
            for arg in &action.args {
                if let angelica_map::ActSource::Global(global) = &arg.source
                    && !objects.globals.contains_key(global)
                {
                    let at = if pointer(&map, global) {
                        let at = objects.allocate(8);
                        let target = objects.allocate(256);
                        objects.write(at, &target.to_le_bytes());
                        at
                    } else {
                        objects.allocate(256)
                    };
                    objects.globals.insert(global.clone(), at);
                }
            }
        }
        // Functions that set fields get a place, and set the field in the
        // scripted memory when called.
        for object in map.objects().types.values() {
            for field in object.fields.values() {
                if let Some(call) = field.set.as_ref().and_then(|set| set.call.as_ref())
                    && !objects.globals.contains_key(call)
                {
                    let at = objects.allocate(16);
                    objects.globals.insert(call.clone(), at);
                    objects
                        .setters
                        .insert(at, (object.name.clone(), field.name.clone()));
                }
            }
        }
        let globals = objects.globals.clone();
        let entry = |name: &str| {
            if absent.contains(&name) {
                return Err("absent in the scripted build".into());
            }
            match map.get(name).map(|entry| &entry.kind) {
                Some(EntryKind::Field {
                    offset: Some(offset),
                    ..
                }) => Ok(Resolved::Offset(*offset)),
                Some(EntryKind::Field { .. }) => {
                    Err("the scripted build knows only constant offsets".into())
                }
                _ => globals
                    .get(name)
                    .and_then(|at| u32::try_from(at - BASE).ok())
                    .map(Resolved::Address)
                    .ok_or_else(|| "not a root's global, an event's code, or an action's".into()),
            }
        };
        let schema = Schema::new(&map, BASE, entry);
        objects.resolved = map
            .entries()
            .map(|item| (item.name.clone(), entry(&item.name)))
            .collect();
        objects.map = Some(map);
        (objects, schema)
    }

    /// The value of the map's entry `name` in the scripted build, or why
    /// it is absent.
    ///
    /// # Errors
    /// An entry the scripted build lacks.
    pub fn resolved(&self, name: &str) -> Result<Resolved, String> {
        self.resolved
            .get(name)
            .cloned()
            .unwrap_or_else(|| Err(format!("{name} is not an entry of the map")))
    }

    /// Zeroed memory of `length` bytes, 16-byte aligned.
    /// Puts `bytes` in the game's memory and returns where.
    pub fn place(&mut self, bytes: &[u8]) -> u64 {
        let at = self.allocate(bytes.len());
        self.write(at, bytes);
        at
    }

    fn allocate(&mut self, length: usize) -> u64 {
        let start = self.used.next_multiple_of(16);
        self.used = start + length.max(1);
        let pages = self.used.next_multiple_of(PAGE);
        if self.memory.len() < pages {
            self.memory.resize(pages, 0);
        }
        BASE + start as u64
    }

    /// Writes `bytes` at `address` when the scripted game's memory holds
    /// the whole range, as a Lens change does.
    pub fn write_data(&mut self, address: u64, bytes: &[u8]) -> bool {
        let Some(start) = address
            .checked_sub(BASE)
            .and_then(|start| usize::try_from(start).ok())
        else {
            return false;
        };
        if start + bytes.len() > self.used {
            return false;
        }
        self.write(address, bytes);
        true
    }

    fn write(&mut self, address: u64, bytes: &[u8]) {
        let start = usize::try_from(address - BASE).unwrap_or(usize::MAX);
        self.memory[start..start + bytes.len()].copy_from_slice(bytes);
    }

    /// The game's function at `function` sets its field of the object at
    /// `object` to `value`, as a Lens change calls it.
    pub fn set_through(
        &mut self,
        function: u64,
        object: u64,
        value: &runtime::lens::set::Setting,
    ) -> bool {
        use runtime::lens::set::Setting;
        let (Some((type_name, name)), Ok(map)) = (self.setters.get(&function).cloned(), self.map())
        else {
            return false;
        };
        let Ok(field) = field_of(&map, &type_name, &name) else {
            return false;
        };
        let set = match value {
            Setting::Text(text) => Set::Text(String::from_utf8_lossy(text).into_owned()),
            Setting::Number(bits) => match &field.shape {
                Shape::Value {
                    kind: ValueKind::Bool,
                    ..
                } => Set::Boolean(*bits & 0xFF != 0),
                Shape::Value {
                    kind: ValueKind::F32,
                    ..
                } => Set::Float(f64::from(f32::from_bits(
                    u32::try_from(*bits & 0xFFFF_FFFF).unwrap_or_default(),
                ))),
                Shape::Value {
                    kind: ValueKind::F64,
                    ..
                } => Set::Float(f64::from_bits(*bits)),
                _ => Set::Integer(i128::from(*bits)),
            },
        };
        self.assign(&map, &type_name, object, &[(name.as_str(), set)])
            .is_ok()
    }

    fn pointer(&self, address: u64) -> u64 {
        let mut bytes = [0u8; 8];
        self.read(address, &mut bytes);
        u64::from_le_bytes(bytes)
    }

    /// The object the pointer at `address` points to, made when it is zero.
    fn target(&mut self, address: u64, length: usize) -> u64 {
        match self.pointer(address) {
            0 => {
                let target = self.allocate(length);
                self.write(address, &target.to_le_bytes());
                target
            }
            target => target,
        }
    }

    fn map(&self) -> Result<Map, String> {
        self.map
            .clone()
            .ok_or_else(|| "the test named no game map".into())
    }

    /// The pointer of a root whose global holds one becomes zero: the
    /// game has no such object now, as before the player logs in.
    ///
    /// # Errors
    /// An unknown root, or one whose global is the object itself.
    pub fn clear(&mut self, root: &str) -> Result<(), String> {
        let map = self.map()?;
        let root = map
            .objects()
            .roots
            .get(root)
            .ok_or_else(|| format!("the map has no root {root}"))?;
        if !pointer(&map, &root.global) {
            return Err(format!("root {} is not behind a pointer", root.name));
        }
        let at = self.globals[&root.global];
        self.write(at, &0u64.to_le_bytes());
        Ok(())
    }

    /// Sets `values` of the object `path` names, made with everything on
    /// the way that does not exist yet. An array's element is named by its
    /// position, `[3]`, or by values, `[kind = bag_1]`: the first element
    /// that has them, or else the first empty one, which gets them.
    ///
    /// # Errors
    /// A path or value that does not fit the map.
    pub fn set(&mut self, path: &str, values: &[(&str, Set)]) -> Result<(), String> {
        let map = self.map()?;
        let (object, current) = self.locate(&map, path)?;
        self.assign(&map, &current, object, values)
    }

    /// The registers of a call of `event` with `args`: each argument by
    /// name, a value, an object by its path, or text the call points to.
    /// An argument left out is zero.
    ///
    /// # Errors
    /// An unknown event or argument, or a value that does not fit it.
    pub fn call(
        &mut self,
        event: &str,
        args: &[(&str, Fire)],
    ) -> Result<runtime::lens::Call, String> {
        use runtime::lens::plan::{ArgAt, arg_at};
        let map = self.map()?;
        let model = map.objects();
        let event = model
            .events
            .get(event)
            .ok_or_else(|| format!("the map has no event {event}"))?;
        let mut call = runtime::lens::Call::default();
        let stack = self.allocate(8 + 8 * 32);
        call.rsp = stack;
        for (name, value) in args {
            let arg = event
                .args
                .get(*name)
                .ok_or_else(|| format!("event {} has no argument {name}", event.name))?;
            let bits = match value {
                Fire::Object(path) => self.locate(&map, path)?.0,
                Fire::Text(text) => {
                    let at = self.allocate(text.len() + 1);
                    self.write(at, text.as_bytes());
                    at
                }
                Fire::String(text) => {
                    let bytes = self.allocate(text.len() + 1);
                    self.write(bytes, text.as_bytes());
                    let string = self.allocate(0x68);
                    self.write(string, &bytes.to_le_bytes());
                    string
                }
                Fire::Value(value) => {
                    let field = Field {
                        name: arg.name.clone(),
                        doc: arg.doc.clone(),
                        at: String::new(),
                        shape: arg.shape.clone(),
                        domain: arg.domain.clone(),
                        forbidden: false,
                        set: None,
                        bit: None,
                        pointer: None,
                        entering: angelica_map::Entering::default(),
                    };
                    let bytes = encode(&map, &field, value)?;
                    let mut wide = [0u8; 8];
                    wide[..bytes.len()].copy_from_slice(&bytes);
                    u64::from_le_bytes(wide)
                }
            };
            match arg_at(arg) {
                ArgAt::Integer(index) => call.general[usize::from(index)] = bits,
                ArgAt::Float(index) => call.xmm[usize::from(index)] = bits,
                ArgAt::Stack(offset) => self.write(stack + u64::from(offset), &bits.to_le_bytes()),
            }
        }
        Ok(call)
    }

    /// The address and type of the object `path` names, made with
    /// everything on the way that does not exist yet, as [`Objects::set`]
    /// names it.
    fn locate(&mut self, map: &Map, path: &str) -> Result<(u64, String), String> {
        let map = map.clone();
        let model = map.objects();
        let parsed = query::parse(path)?;
        let root = model
            .roots
            .get(&parsed.root)
            .ok_or_else(|| format!("the map has no root {}", parsed.root))?;
        let global = self.globals[&root.global];
        let mut object = if pointer(&map, &root.global) {
            self.target(global, size_of(&map, &root.r#type))
        } else {
            global
        };
        let mut current = root.r#type.clone();
        for segment in &parsed.path {
            let field = field_of(&map, &current, &segment.field)?;
            let offset = u64::from(offset_of(&map, &field)?);
            match &field.shape {
                Shape::Ref(to) => {
                    object = self.target(object + offset, size_of(&map, to));
                    current.clone_from(to);
                }
                Shape::Embed(of) => {
                    object += offset;
                    current.clone_from(of);
                }
                Shape::Array {
                    array,
                    behind_pointer,
                } => {
                    let place = Place {
                        holder: &current,
                        object,
                        offset,
                        behind_pointer: *behind_pointer,
                    };
                    object = self.element(&map, &place, segment, array)?;
                    current.clone_from(&array.of);
                }
                Shape::Value { .. } | Shape::Text(_) => {
                    return Err(format!(
                        "{current}.{} is a value: name it with the values to set",
                        segment.field
                    ));
                }
            }
        }
        Ok((object, current))
    }

    /// The element of an array a path's segment names, made when needed.
    fn element(
        &mut self,
        map: &Map,
        place: &Place<'_>,
        segment: &query::Segment,
        array: &angelica_map::Array,
    ) -> Result<u64, String> {
        let length = most(&array.length);
        let base = if place.behind_pointer {
            self.target(place.object + place.offset, length * array.stride as usize)
        } else {
            place.object + place.offset
        };
        let slots = Slots {
            base,
            stride: array.stride as usize,
            pointers: array.pointers,
            size: size_of(map, &array.of),
        };
        let position = match &segment.filter {
            Some(Filter::Index(index)) => *index as usize,
            Some(Filter::Conditions(conditions)) => {
                let wanted = conditions
                    .iter()
                    .map(|condition| match &condition.test {
                        Test::Compare(Op::Equal, literal) if condition.field.len() == 1 => {
                            Ok((condition.field[0].as_str(), set_of(literal)))
                        }
                        _ => Err(
                            "an element is named by [index] or by [field = value and ...]"
                                .to_owned(),
                        ),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.find(map, array, &slots, length, &wanted)?
                    .ok_or_else(|| {
                        format!(
                            "{}.{} has no empty element left",
                            place.holder, segment.field
                        )
                    })?
            }
            None => {
                return Err(format!(
                    "{}.{} is an array: name the element, [index] or [field = value]",
                    place.holder, segment.field
                ));
            }
        };
        if position >= length {
            return Err(format!(
                "{}.{} has at most {length} elements",
                place.holder, segment.field
            ));
        }
        if let Length::Field { field: count, .. } = &array.length {
            let counter = field_of(map, place.holder, count)?;
            let at = place.object + u64::from(offset_of(map, &counter)?);
            let used = self.bytes(at, &counter);
            let mut wide = [0u8; 8];
            wide[..used.len()].copy_from_slice(&used);
            if u64::from_le_bytes(wide) <= position as u64 {
                let raised = Set::Integer(i128::from(position as u64 + 1));
                self.assign(map, place.holder, place.object, &[(count.as_str(), raised)])?;
            }
        }
        Ok(slots.element(self, position))
    }

    /// The position of the first element with the `wanted` values, or of
    /// the first empty one, which gets them; `None` when every element is
    /// taken.
    fn find(
        &mut self,
        map: &Map,
        array: &angelica_map::Array,
        slots: &Slots,
        length: usize,
        wanted: &[(&str, Set)],
    ) -> Result<Option<usize>, String> {
        let mut empty = None;
        for position in 0..length {
            if slots.pointers && self.pointer(slots.slot(position)) == 0 {
                empty = empty.or(Some(position));
                continue;
            }
            let at = slots.element(self, position);
            let mut matches = true;
            for (name, value) in wanted {
                let field = field_of(map, &array.of, name)?;
                matches &= encode(map, &field, value)?
                    == self.bytes(at + u64::from(offset_of(map, &field)?), &field);
            }
            if matches {
                return Ok(Some(position));
            }
            let blank = match &array.present {
                Some(present) => {
                    let field = field_of(map, &array.of, present)?;
                    self.bytes(at + u64::from(offset_of(map, &field)?), &field)
                        .iter()
                        .all(|&byte| byte == 0)
                }
                None => self.bytes_at(at, slots.size).iter().all(|&byte| byte == 0),
            };
            if blank {
                empty = empty.or(Some(position));
            }
        }
        if let Some(position) = empty {
            let at = slots.element(self, position);
            self.assign(map, &array.of, at, wanted)?;
        }
        Ok(empty)
    }

    fn bytes_at(&self, address: u64, length: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; length];
        self.read(address, &mut bytes);
        bytes
    }

    /// The bytes a field takes at `address`.
    fn bytes(&self, address: u64, field: &Field) -> Vec<u8> {
        let length = match &field.shape {
            Shape::Value { kind, .. } => size(*kind),
            Shape::Text(length) => usize::from(*length),
            _ => 8,
        };
        self.bytes_at(address, length)
    }

    /// Writes `values` of the object of `type_name` at `object`; a dotted
    /// name goes through references and embedded objects, made as needed.
    fn assign(
        &mut self,
        map: &Map,
        type_name: &str,
        object: u64,
        values: &[(&str, Set)],
    ) -> Result<(), String> {
        for (name, value) in values {
            let mut current = type_name.to_owned();
            let mut at = object;
            let names: Vec<&str> = name.split('.').collect();
            for (position, part) in names.iter().enumerate() {
                let field = field_of(map, &current, part)?;
                let offset = u64::from(offset_of(map, &field)?);
                if position + 1 == names.len() {
                    if let Some(bit) = field.bit {
                        // One bit of the entry's integer.
                        let place = at + offset;
                        let mut bytes = self.bytes_at(place, size(bit.kind));
                        let byte = usize::from(bit.index / 8);
                        let mask = 1u8 << (bit.index % 8);
                        if matches!(value, Set::Boolean(true)) {
                            bytes[byte] |= mask;
                        } else {
                            bytes[byte] &= !mask;
                        }
                        self.write(place, &bytes);
                        break;
                    }
                    let bytes = encode(map, &field, value)?;
                    if let Some(pointer) = &field.pointer {
                        // Text in a game string: the string's pointer leads
                        // to a place of its own.
                        let inner = u64::from(entry_offset(map, pointer)?);
                        let text = self.allocate(bytes.len());
                        self.write(text, &bytes);
                        self.write(at + offset + inner, &text.to_le_bytes());
                    } else if matches!(field.shape, Shape::Text(_))
                        && behind_pointer(map, &field.at)
                    {
                        // Text behind a pointer gets a place of its own.
                        let text = self.allocate(bytes.len());
                        self.write(text, &bytes);
                        self.write(at + offset, &text.to_le_bytes());
                    } else {
                        self.write(at + offset, &bytes);
                    }
                    break;
                }
                match &field.shape {
                    Shape::Ref(to) => {
                        at = self.target(at + offset, size_of(map, to));
                        current.clone_from(to);
                    }
                    Shape::Embed(of) => {
                        at += offset;
                        current.clone_from(of);
                    }
                    _ => return Err(format!("{current}.{part} has no fields")),
                }
            }
        }
        Ok(())
    }
}

/// Where an array is: in an object of `holder` at `object`, at `offset`,
/// in place or behind a pointer.
struct Place<'a> {
    holder: &'a str,
    object: u64,
    offset: u64,
    behind_pointer: bool,
}

/// The slots of an array from `base`.
struct Slots {
    base: u64,
    stride: usize,
    pointers: bool,
    /// The size of an element.
    size: usize,
}

impl Slots {
    fn slot(&self, position: usize) -> u64 {
        self.base + (position * self.stride) as u64
    }

    /// The element at `position`, made when its pointer is zero.
    fn element(&self, objects: &mut Objects, position: usize) -> u64 {
        if self.pointers {
            objects.target(self.slot(position), self.size)
        } else {
            self.slot(position)
        }
    }
}

/// The bytes of `value` for `field`.
fn encode(map: &Map, field: &Field, value: &Set) -> Result<Vec<u8>, String> {
    let wrong = || format!("{} does not take {value:?}", field.name);
    match &field.shape {
        Shape::Text(length) => match value {
            Set::Text(text) if text.len() < usize::from(*length) => {
                let mut bytes = vec![0u8; usize::from(*length)];
                bytes[..text.len()].copy_from_slice(text.as_bytes());
                Ok(bytes)
            }
            Set::Text(_) => Err(format!("{} holds at most {} bytes", field.name, length - 1)),
            _ => Err(wrong()),
        },
        Shape::Value { kind, meaning, .. } => {
            let number = match (value, meaning) {
                (Set::Name(name), Meaning::Enum(enum_name) | Meaning::Flags(enum_name)) => {
                    i128::from(
                        *map.objects().enums[enum_name]
                            .values
                            .get(name)
                            .ok_or_else(wrong)?,
                    )
                }
                (Set::Integer(value), _) => *value,
                (Set::Boolean(value), _) => i128::from(*value),
                (Set::Float(value), _) => {
                    return match kind {
                        #[allow(clippy::cast_possible_truncation)]
                        ValueKind::F32 => Ok((*value as f32).to_le_bytes().to_vec()),
                        ValueKind::F64 => Ok(value.to_le_bytes().to_vec()),
                        _ => Err(wrong()),
                    };
                }
                _ => return Err(wrong()),
            };
            if matches!(kind, ValueKind::F32 | ValueKind::F64) {
                #[allow(clippy::cast_precision_loss)]
                return encode(map, field, &Set::Float(number as f64));
            }
            let bytes = number.to_le_bytes();
            Ok(bytes[..size(*kind)].to_vec())
        }
        _ => Err(format!("{} is not a value", field.name)),
    }
}

/// Whether a field's entry holds a pointer, as text behind one does.
fn behind_pointer(map: &Map, field: &str) -> bool {
    matches!(
        map.get(field).map(|entry| &entry.kind),
        Some(EntryKind::Field {
            value: ValueKind::Ptr,
            ..
        })
    )
}

fn pointer(map: &Map, global: &str) -> bool {
    matches!(
        map.get(global).map(|entry| &entry.kind),
        Some(EntryKind::Global {
            value: ValueKind::Ptr
        })
    )
}

/// A field of the type, of a base, or of a variant of it.
fn field_of(map: &Map, type_name: &str, name: &str) -> Result<Field, String> {
    map.objects()
        .field(type_name, name)
        .map(|(_, field)| field.clone())
        .or_else(|| {
            map.objects()
                .variant_field(type_name, name)
                .map(|(_, _, field)| field.clone())
        })
        .ok_or_else(|| format!("{type_name} has no field {name}"))
}

fn offset_of(map: &Map, field: &Field) -> Result<u32, String> {
    entry_offset(map, &field.at)
}

fn entry_offset(map: &Map, entry: &str) -> Result<u32, String> {
    match map.get(entry).map(|entry| &entry.kind) {
        Some(EntryKind::Field {
            offset: Some(offset),
            ..
        }) => Ok(*offset),
        _ => Err(format!("{entry} has no constant offset")),
    }
}

/// The bytes an object of `type_name` takes: up to the end of its last
/// field, or of a variant's, at least 8.
fn size_of(map: &Map, type_name: &str) -> usize {
    let model = map.objects();
    let variants = model
        .types
        .values()
        .filter(|object| object.variant.is_some() && object.extends.as_deref() == Some(type_name))
        .flat_map(|object| object.fields.values());
    model
        .fields(type_name)
        .into_values()
        .chain(variants)
        .map(|field| {
            let at = offset_of(map, field).unwrap_or(0) as usize;
            at + match &field.shape {
                Shape::Value { .. } if field.bit.is_some() => {
                    field.bit.map_or(0, |bit| size(bit.kind))
                }
                Shape::Text(_) if field.pointer.is_some() => field
                    .pointer
                    .as_deref()
                    .and_then(|pointer| entry_offset(map, pointer).ok())
                    .map_or(8, |inner| inner as usize + 8),
                Shape::Value { kind, .. } => size(*kind),
                Shape::Text(_) if behind_pointer(map, &field.at) => 8,
                Shape::Text(length) => usize::from(*length),
                Shape::Ref(_)
                | Shape::Array {
                    behind_pointer: true,
                    ..
                } => 8,
                Shape::Embed(of) => size_of(map, of),
                Shape::Array { array, .. } => most(&array.length) * array.stride as usize,
            }
        })
        .max()
        .unwrap_or(0)
        .max(8)
}

/// What a test means by a value in a path's filter.
fn set_of(literal: &Literal) -> Set {
    match literal {
        Literal::Integer(value) => Set::Integer(*value),
        Literal::Float(value) => Set::Float(*value),
        Literal::Boolean(value) => Set::Boolean(*value),
        Literal::Name(name) => Set::Name(name.clone()),
        Literal::Text(text) => Set::Text(text.clone()),
    }
}
