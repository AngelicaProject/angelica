//! The object model: what the game's objects are, for Lens.
//!
//! Entries say where things are in a build; the object model says what they
//! mean. A type is a structure of the game whose fields are `field` entries
//! of that structure, each read as a value, text, a reference to another
//! object, an embedded object, or an array of objects. A root is where a
//! path through the objects starts: a `global` entry holding an object. Each
//! type belongs to a domain, the unit of a plugin's permission, and a field
//! may belong to another domain or be forbidden to every plugin. Enums name
//! the values of a field.
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{Entry, EntryKind, ValueKind};

/// How far a domain reaches, as the player is told: the most serious first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DomainClass {
    /// Acts in the game toward other players.
    ActsOnOthers,
    /// Acts in the game for the player.
    Acts,
    /// Sees what other players say or do.
    Others,
    /// Changes what the player sees of the game.
    Display,
    /// Only about the player.
    You,
}

/// What the player is told, in each language of Angelica's interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wording {
    pub en: String,
    pub ru: String,
}

/// A domain: the unit of a plugin's permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain {
    pub name: String,
    pub doc: String,
    pub class: DomainClass,
    /// What reading it allows; every domain has it.
    pub read: Wording,
    /// What changing it allows, for `set` and `decide`.
    pub change: Option<Wording>,
    /// What acting in it allows, for `act`.
    pub act: Option<Wording>,
}

/// Named values of a field; with `flags`, names of single bits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enum {
    pub name: String,
    pub doc: String,
    pub flags: bool,
    pub values: BTreeMap<String, u64>,
}

/// What a number means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meaning {
    /// Only the number.
    Plain,
    /// A row of the game's sheet with this name.
    Sheet(String),
    /// One of the values of an enum.
    Enum(String),
    /// A set of the bits of a flags enum.
    Flags(String),
    /// The id of an element of one of the game's windows, named by the
    /// map's window schema of the window whose name the text field
    /// `<type>.<field>` holds.
    Element(String),
}

/// How many elements an array has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Length {
    /// Always this many.
    Fixed(u16),
    /// As many as a field of the same object holds, at most `most`.
    Field { field: String, most: u16 },
}

/// An array of objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Array {
    /// The type of the elements.
    pub of: String,
    pub length: Length,
    /// Bytes from one element to the next; 8 when `pointers`.
    pub stride: u32,
    /// Whether every element is a pointer to the object; zero pointers are
    /// skipped.
    pub pointers: bool,
    /// A field of the element: an element whose value of it is zero, false,
    /// empty, or none is skipped.
    pub present: Option<String>,
    /// The elements' domains by the value of one of their fields.
    pub partition: Option<Partition>,
}

/// An array whose elements belong to different domains by the value of a
/// field of the element, such as the containers of every kind of item.
/// An element whose value has no name in the field's enum is never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    /// A field of the element whose meaning is an enum.
    pub field: String,
    /// The domain of the elements with each named value; a named value not
    /// listed belongs to the array field's domain.
    pub domains: BTreeMap<String, String>,
}

/// How a field is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// A value of the entry's type.
    Value {
        kind: ValueKind,
        meaning: Meaning,
        unit: Option<String>,
    },
    /// NUL-terminated UTF-8 of at most this many bytes, stored inline.
    Text(u16),
    /// A pointer to an object of this type.
    Ref(String),
    /// An object of this type embedded at the field's offset.
    Embed(String),
    /// An array, at the field's offset (an embedded field) or behind its
    /// pointer.
    Array { array: Array, behind_pointer: bool },
}

/// A field of a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub doc: String,
    /// The `field` entry that locates it.
    pub at: String,
    pub shape: Shape,
    /// Its domain: the type's, unless the field names another.
    pub domain: String,
    /// Described for completeness, never read by any plugin.
    pub forbidden: bool,
    /// Whether a plugin may change it, and within which values: only a
    /// value that exists on the player's computer alone.
    pub set: Option<Settable>,
    /// For a value read as a `bool` from one bit of an integer entry.
    pub bit: Option<Bit>,
    /// For text in a structure of the game that holds a pointer to it, such
    /// as a game string: the `field` entry of that pointer within the
    /// structure at the field's entry.
    pub pointer: Option<String>,
    /// For a reference or an array: whose objects it reaches, and the
    /// domains renamed below it.
    pub entering: crate::whose::Entering,
}

/// One bit of an integer entry, read as a `bool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bit {
    /// The bit's position, from 0 for the lowest.
    pub index: u8,
    /// The entry's kind, known once the map is checked.
    pub kind: ValueKind,
}

/// How a plugin may change a field: any value of its kind, or within a
/// range; written in memory or handed to the game's own function; once or
/// held against the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settable {
    pub range: Option<(crate::Number, crate::Number)>,
    /// A `function` entry of the main thread, `(ptr object, value)`, that
    /// changes the value as the game itself does; without it the value is
    /// written in memory.
    pub call: Option<String>,
    /// Whether the game sets the value itself again, so the host applies
    /// the plugin's value again every frame while the path reaches it.
    pub hold: bool,
}

/// The objects of a base type that are of a type extending it, by the
/// value of a field of the base, such as the text elements of a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    /// An enum value field of the base.
    pub field: String,
    /// The values of the field that make an object of this type.
    pub values: Vec<String>,
}

/// A structure of the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    pub name: String,
    pub doc: String,
    pub domain: String,
    /// A type whose fields this one has too, at the same offsets.
    pub extends: Option<String>,
    /// For a type that extends another: which objects of the base are of
    /// this type, so a path reaches this type's fields from the base.
    pub variant: Option<Variant>,
    pub fields: BTreeMap<String, Field>,
}

/// Where a path through the objects starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub name: String,
    pub doc: String,
    /// A `global` entry: the object itself (`struct`) or a pointer to it
    /// (`ptr`).
    pub global: String,
    pub r#type: String,
    /// Whose objects it holds, and the domains renamed below it.
    pub entering: crate::whose::Entering,
}

/// The whole object model of a map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Objects {
    pub domains: BTreeMap<String, Domain>,
    pub enums: BTreeMap<String, Enum>,
    pub types: BTreeMap<String, Type>,
    pub roots: BTreeMap<String, Root>,
    pub events: BTreeMap<String, crate::events::Event>,
    pub actions: BTreeMap<String, crate::actions::Action>,
}

impl Objects {
    /// A field of `type_name` or of a type it extends, with the type that
    /// declares it.
    #[must_use]
    pub fn field<'a>(&'a self, type_name: &str, field: &str) -> Option<(&'a Type, &'a Field)> {
        let mut current = self.types.get(type_name);
        while let Some(object) = current {
            if let Some(found) = object.fields.get(field) {
                return Some((object, found));
            }
            current = object
                .extends
                .as_deref()
                .and_then(|base| self.types.get(base));
        }
        None
    }

    /// Every field of `type_name`, its own and those of the types it
    /// extends, by name.
    #[must_use]
    pub fn fields(&self, type_name: &str) -> BTreeMap<&str, &Field> {
        let mut chain = Vec::new();
        let mut current = self.types.get(type_name);
        while let Some(object) = current {
            chain.push(object);
            current = object
                .extends
                .as_deref()
                .and_then(|base| self.types.get(base));
        }
        let mut fields = BTreeMap::new();
        for object in chain.iter().rev() {
            for (name, field) in &object.fields {
                fields.insert(name.as_str(), field);
            }
        }
        fields
    }

    /// A field of a variant of `type_name`: a type that extends it and
    /// says which of its objects are of the variant. Returns the variant's
    /// type, the variant, and the field.
    #[must_use]
    pub fn variant_field<'a>(
        &'a self,
        type_name: &str,
        field: &str,
    ) -> Option<(&'a Type, &'a Variant, &'a Field)> {
        self.types.values().find_map(|object| {
            let variant = object.variant.as_ref()?;
            if object.extends.as_deref() != Some(type_name) {
                return None;
            }
            object
                .fields
                .get(field)
                .map(|found| (object, variant, found))
        })
    }

    /// Every entry the object model refers to.
    pub fn references(&self) -> impl Iterator<Item = &str> {
        self.types
            .values()
            .flat_map(|object| {
                object.fields.values().flat_map(|field| {
                    std::iter::once(field.at.as_str())
                        .chain(field.pointer.as_deref())
                        .chain(field.set.as_ref().and_then(|set| set.call.as_deref()))
                })
            })
            .chain(self.roots.values().map(|root| root.global.as_str()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDomains {
    domains: BTreeMap<String, RawDomain>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDomain {
    doc: String,
    class: String,
    read: RawWording,
    change: Option<RawWording>,
    act: Option<RawWording>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWording {
    en: String,
    ru: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnum {
    doc: String,
    #[serde(default)]
    flags: bool,
    values: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTypes {
    types: BTreeMap<String, RawType>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawType {
    doc: String,
    domain: String,
    extends: Option<String>,
    variant: Option<RawVariant>,
    #[serde(default)]
    fields: BTreeMap<String, RawField>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVariant {
    field: String,
    values: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawField {
    doc: String,
    at: Option<String>,
    text: Option<u16>,
    r#ref: Option<String>,
    embed: Option<String>,
    array: Option<RawArray>,
    sheet: Option<String>,
    r#enum: Option<String>,
    flags: Option<String>,
    element: Option<String>,
    unit: Option<String>,
    bit: Option<u8>,
    pointer: Option<String>,
    domain: Option<String>,
    #[serde(default)]
    forbidden: bool,
    set: Option<toml::Value>,
    whose: Option<String>,
    r#as: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArray {
    of: String,
    length: Option<u16>,
    count: Option<String>,
    most: Option<u16>,
    stride: Option<u32>,
    #[serde(default)]
    pointers: bool,
    present: Option<String>,
    partition: Option<RawPartition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPartition {
    field: String,
    /// The values of each domain.
    domains: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoots {
    roots: BTreeMap<String, RawRoot>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoot {
    doc: String,
    global: String,
    r#type: String,
    whose: Option<String>,
    r#as: Option<BTreeMap<String, String>>,
}

/// The most elements an array is read with.
pub const MOST_ELEMENTS: u16 = 4096;
/// The most bytes of inline text.
pub const MOST_TEXT: u16 = 4096;

/// A lowercase letter, then lowercase letters, digits, and underscores, at
/// most 64 bytes.
pub(crate) fn is_word(word: &str) -> bool {
    word.len() <= 64
        && word.starts_with(|first: char| first.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Dotted words, at least two.
fn is_dotted(name: &str) -> bool {
    name.split('.').count() >= 2 && name.split('.').all(is_word)
}

pub(crate) fn not_blank(what: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        Err(format!("{what} is blank"))
    } else {
        Ok(())
    }
}

fn toml<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, String> {
    toml::from_str(text).map_err(|error| error.message().to_owned())
}

/// Parses `domains.toml`.
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn parse_domains(text: &str) -> Result<BTreeMap<String, Domain>, String> {
    let raw: RawDomains = toml(text)?;
    let mut domains = BTreeMap::new();
    for (name, domain) in raw.domains {
        if !is_word(&name) {
            return Err(format!("`{name}` is not a valid domain name"));
        }
        not_blank(&format!("the doc of domain `{name}`"), &domain.doc)?;
        let class = match domain.class.as_str() {
            "you" => DomainClass::You,
            "display" => DomainClass::Display,
            "others" => DomainClass::Others,
            "acts" => DomainClass::Acts,
            "acts_on_others" => DomainClass::ActsOnOthers,
            other => {
                return Err(format!(
                    "domain `{name}`: class `{other}` is not one of you, display, others, acts, acts_on_others"
                ));
            }
        };
        let wording = |what: &str, raw: RawWording| -> Result<Wording, String> {
            not_blank(&format!("domain `{name}`: the English `{what}`"), &raw.en)?;
            not_blank(&format!("domain `{name}`: the Russian `{what}`"), &raw.ru)?;
            Ok(Wording {
                en: raw.en,
                ru: raw.ru,
            })
        };
        domains.insert(
            name.clone(),
            Domain {
                name: name.clone(),
                doc: domain.doc,
                class,
                read: wording("read", domain.read)?,
                change: domain
                    .change
                    .map(|raw| wording("change", raw))
                    .transpose()?,
                act: domain.act.map(|raw| wording("act", raw)).transpose()?,
            },
        );
    }
    Ok(domains)
}

/// Parses `enums/<name>.toml`, the file of the enum `name`.
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn parse_enum(name: &str, text: &str) -> Result<Enum, String> {
    if !is_dotted(name) {
        return Err(format!("`{name}` is not a valid enum name"));
    }
    let raw: RawEnum = toml(text)?;
    not_blank("doc", &raw.doc)?;
    if raw.values.is_empty() {
        return Err("an enum names at least one value".into());
    }
    for (value_name, value) in &raw.values {
        if !is_word(value_name) {
            return Err(format!("`{value_name}` is not a valid value name"));
        }
        if raw.flags && (*value == 0 || !value.is_power_of_two()) {
            return Err(format!("`{value_name}`: a flag is a single bit"));
        }
    }
    let distinct = raw.values.values().collect::<BTreeSet<_>>();
    if distinct.len() != raw.values.len() {
        return Err("two names have the same value".into());
    }
    Ok(Enum {
        name: name.to_owned(),
        doc: raw.doc,
        flags: raw.flags,
        values: raw.values,
    })
}

/// Parses `roots.toml`.
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn parse_roots(text: &str) -> Result<BTreeMap<String, Root>, String> {
    let raw: RawRoots = toml(text)?;
    let mut roots = BTreeMap::new();
    for (name, root) in raw.roots {
        if !is_word(&name) {
            return Err(format!("`{name}` is not a valid root name"));
        }
        not_blank(&format!("the doc of root `{name}`"), &root.doc)?;
        let entering = crate::whose::Entering::parse(root.whose, root.r#as)
            .map_err(|message| format!("root `{name}`: {message}"))?;
        roots.insert(
            name.clone(),
            Root {
                name,
                doc: root.doc,
                global: root.global,
                r#type: root.r#type,
                entering,
            },
        );
    }
    Ok(roots)
}

/// Parses `types/<area>.toml`; every type's name starts with `area`.
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn parse_types(area: &str, text: &str) -> Result<Vec<Type>, String> {
    let raw: RawTypes = toml(text)?;
    let mut types = Vec::new();
    for (name, object) in raw.types {
        if !is_dotted(&name) {
            return Err(format!("`{name}` is not a valid type name"));
        }
        if name.split('.').next() != Some(area) {
            return Err(format!("`{name}` does not belong to the area of this file"));
        }
        not_blank(&format!("the doc of `{name}`"), &object.doc)?;
        let mut fields = BTreeMap::new();
        for (field_name, field) in object.fields {
            if !is_word(&field_name) {
                return Err(format!(
                    "`{name}`: `{field_name}` is not a valid field name"
                ));
            }
            let parsed = parse_field(&name, &field_name, field, &object.domain)
                .map_err(|message| format!("`{name}.{field_name}`: {message}"))?;
            fields.insert(field_name, parsed);
        }
        let variant = object
            .variant
            .map(|raw| {
                if object.extends.is_none() {
                    return Err(format!("`{name}`: a variant extends its base type"));
                }
                if raw.values.is_empty() {
                    return Err(format!("`{name}`: a variant names at least one value"));
                }
                Ok(Variant {
                    field: raw.field,
                    values: raw.values,
                })
            })
            .transpose()?;
        types.push(Type {
            name,
            doc: object.doc,
            domain: object.domain,
            extends: object.extends,
            variant,
            fields,
        });
    }
    Ok(types)
}

/// A field as written, before its entry is known: the shape keeps the value
/// kind `Void` until [`check`] reads it from the entry.
fn parse_field(type_name: &str, name: &str, raw: RawField, domain: &str) -> Result<Field, String> {
    not_blank("doc", &raw.doc)?;
    let shapes = usize::from(raw.text.is_some())
        + usize::from(raw.r#ref.is_some())
        + usize::from(raw.embed.is_some())
        + usize::from(raw.array.is_some());
    if shapes > 1 {
        return Err("a field is at most one of text, ref, embed, and array".into());
    }
    let meanings = usize::from(raw.sheet.is_some())
        + usize::from(raw.r#enum.is_some())
        + usize::from(raw.flags.is_some())
        + usize::from(raw.element.is_some());
    if meanings > 1 {
        return Err("a value means at most one of sheet, enum, flags, and element".into());
    }
    if shapes == 1 && (meanings > 0 || raw.unit.is_some() || raw.bit.is_some()) {
        return Err("sheet, enum, flags, element, unit, and bit belong to values only".into());
    }
    if raw.bit.is_some() && (meanings > 0 || raw.unit.is_some()) {
        return Err("a bit is true or false: it has no meaning or unit".into());
    }
    if raw.bit.is_some_and(|bit| bit > 63) {
        return Err("`bit` is 0 to 63".into());
    }
    if raw.pointer.is_some() && raw.text.is_none() {
        return Err("`pointer` belongs to text only".into());
    }
    let shape = if let Some(length) = raw.text {
        if !(1..=MOST_TEXT).contains(&length) {
            return Err(format!("text is 1 to {MOST_TEXT} bytes"));
        }
        Shape::Text(length)
    } else if let Some(to) = raw.r#ref {
        Shape::Ref(to)
    } else if let Some(of) = raw.embed {
        Shape::Embed(of)
    } else if let Some(array) = raw.array {
        Shape::Array {
            array: parse_array(array)?,
            behind_pointer: false,
        }
    } else {
        let meaning = if let Some(sheet) = raw.sheet {
            not_blank("sheet", &sheet)?;
            Meaning::Sheet(sheet)
        } else if let Some(name) = raw.r#enum {
            Meaning::Enum(name)
        } else if let Some(name) = raw.flags {
            Meaning::Flags(name)
        } else if let Some(window) = raw.element {
            Meaning::Element(window)
        } else {
            Meaning::Plain
        };
        if let Some(unit) = &raw.unit {
            not_blank("unit", unit)?;
        }
        Shape::Value {
            kind: ValueKind::Void,
            meaning,
            unit: raw.unit,
        }
    };
    let set = parse_settable(raw.set)?;
    if set.is_some() && raw.forbidden {
        return Err("a forbidden field is never set".into());
    }
    let entering = crate::whose::Entering::parse(raw.whose, raw.r#as)?;
    if !entering.is_empty() && !matches!(shape, Shape::Ref(_) | Shape::Array { .. }) {
        return Err("`whose` and `as` belong to references and arrays".into());
    }
    Ok(Field {
        name: name.to_owned(),
        doc: raw.doc,
        at: raw.at.unwrap_or_else(|| format!("{type_name}.{name}")),
        shape,
        domain: raw.domain.unwrap_or_else(|| domain.to_owned()),
        forbidden: raw.forbidden,
        set,
        bit: raw.bit.map(|index| Bit {
            index,
            kind: ValueKind::Void,
        }),
        pointer: raw.pointer,
        entering,
    })
}

/// `set`: true, or a table of `range`, `call`, and `hold`.
fn parse_settable(raw: Option<toml::Value>) -> Result<Option<Settable>, String> {
    let table = match raw {
        None | Some(toml::Value::Boolean(false)) => return Ok(None),
        Some(toml::Value::Boolean(true)) => {
            return Ok(Some(Settable {
                range: None,
                call: None,
                hold: false,
            }));
        }
        Some(toml::Value::Table(table)) => table,
        Some(_) => return Err("`set` is true or a table of `range`, `call`, and `hold`".into()),
    };
    let range = match table.get("range") {
        Some(toml::Value::Array(bounds)) if bounds.len() == 2 => {
            let bound = |value: &toml::Value| {
                crate::actions::number(value).ok_or("`set.range` holds two numbers")
            };
            Some((bound(&bounds[0])?, bound(&bounds[1])?))
        }
        None => None,
        Some(_) => return Err("`set.range` is [least, most]".into()),
    };
    let call = match table.get("call") {
        Some(toml::Value::String(call)) => Some(call.clone()),
        None => None,
        Some(_) => return Err("`set.call` names a function entry".into()),
    };
    let hold = match table.get("hold") {
        Some(toml::Value::Boolean(hold)) => *hold,
        None => false,
        Some(_) => return Err("`set.hold` is true or false".into()),
    };
    if table
        .keys()
        .any(|key| !["range", "call", "hold"].contains(&key.as_str()))
    {
        return Err("`set` has only `range`, `call`, and `hold`".into());
    }
    Ok(Some(Settable { range, call, hold }))
}

fn parse_array(raw: RawArray) -> Result<Array, String> {
    let length = match (raw.length, raw.count) {
        (Some(length), None) => {
            if !(1..=MOST_ELEMENTS).contains(&length) {
                return Err(format!("an array has 1 to {MOST_ELEMENTS} elements"));
            }
            if raw.most.is_some() {
                return Err("`most` belongs to an array whose length is a field".into());
            }
            Length::Fixed(length)
        }
        (None, Some(field)) => {
            let most = raw
                .most
                .ok_or("an array whose length is a field gives `most`")?;
            if !(1..=MOST_ELEMENTS).contains(&most) {
                return Err(format!("`most` is 1 to {MOST_ELEMENTS}"));
            }
            Length::Field { field, most }
        }
        _ => return Err("an array has either `length` or `count`".into()),
    };
    let stride = if raw.pointers {
        if raw.stride.is_some_and(|stride| stride != 8) {
            return Err("an array of pointers has the stride 8".into());
        }
        8
    } else {
        let stride = raw.stride.ok_or("an array of objects gives `stride`")?;
        if !(1..=65536).contains(&stride) {
            return Err("the stride is 1 to 65536".into());
        }
        stride
    };
    Ok(Array {
        of: raw.of,
        length,
        stride,
        pointers: raw.pointers,
        present: raw.present,
        partition: raw
            .partition
            .map(|partition| {
                let mut domains = BTreeMap::new();
                for (domain, values) in partition.domains {
                    for value in values {
                        if domains.insert(value.clone(), domain.clone()).is_some() {
                            return Err(format!("`partition` lists `{value}` twice"));
                        }
                    }
                }
                Ok(Partition {
                    field: partition.field,
                    domains,
                })
            })
            .transpose()?,
    })
}

/// Checks the object model against itself and the entries, and reads every
/// value field's kind from its entry.
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn check(
    objects: &mut Objects,
    entries: &BTreeMap<String, Entry>,
) -> Result<(), String> {
    let domains = objects.domains.keys().cloned().collect::<BTreeSet<_>>();
    let type_names = objects.types.keys().cloned().collect::<BTreeSet<_>>();
    let enums = objects.enums.clone();

    for object in objects.types.values_mut() {
        if !domains.contains(&object.domain) {
            return Err(format!(
                "`{}`: domain `{}` is not in domains.toml",
                object.name, object.domain
            ));
        }
        if let Some(base) = &object.extends
            && !type_names.contains(base)
        {
            return Err(format!(
                "`{}` extends `{base}`, which is not a type",
                object.name
            ));
        }
        for field in object.fields.values_mut() {
            check_field(&object.name, field, entries, &domains, &type_names, &enums)
                .map_err(|message| format!("`{}.{}`: {message}", object.name, field.name))?;
        }
    }

    check_across_types(objects, entries)?;

    // Fields of the element that an array's `present` and of the object that
    // its `count` name, now that every type is known.
    for object in objects.types.values() {
        for field in object.fields.values() {
            if let Shape::Array { array, .. } = &field.shape {
                if let Some(present) = &array.present {
                    match objects.field(&array.of, present) {
                        Some((_, element))
                            if matches!(element.shape, Shape::Value { .. } | Shape::Text(_)) => {}
                        _ => {
                            return Err(format!(
                                "`{}.{}`: `present` names `{present}`, which is not a value of `{}`",
                                object.name, field.name, array.of
                            ));
                        }
                    }
                }
                if let Some(partition) = &array.partition {
                    check_partition(objects, &domains, &array.of, partition).map_err(
                        |message| {
                            format!("`{}.{}`: `partition`: {message}", object.name, field.name)
                        },
                    )?;
                }
                if let Length::Field { field: count, .. } = &array.length {
                    match objects.field(&object.name, count) {
                        Some((_, counter)) if matches!(counter.shape, Shape::Value { kind, .. } if is_integer(kind)) =>
                            {}
                        _ => {
                            return Err(format!(
                                "`{}.{}`: `count` names `{count}`, which is not an integer field of `{}`",
                                object.name, field.name, object.name
                            ));
                        }
                    }
                }
            }
        }
    }

    no_cycles(objects)?;

    for root in objects.roots.values() {
        if !type_names.contains(&root.r#type) {
            return Err(format!(
                "root `{}`: `{}` is not a type",
                root.name, root.r#type
            ));
        }
        match entries.get(&root.global).map(|entry| &entry.kind) {
            Some(EntryKind::Global {
                value: ValueKind::Ptr | ValueKind::Struct,
            }) => {}
            Some(_) => {
                return Err(format!(
                    "root `{}`: `{}` is not a global of the type ptr or struct",
                    root.name, root.global
                ));
            }
            None => {
                return Err(format!(
                    "root `{}`: `{}` is not an entry",
                    root.name, root.global
                ));
            }
        }
    }
    Ok(())
}

/// What a field or type says of other types, now that every type is
/// known: fields a plugin may change are values whose domain says what
/// changing it allows; element names come from a window's text field;
/// variants tell their objects apart by an enum field of their base.
fn check_across_types(objects: &Objects, entries: &BTreeMap<String, Entry>) -> Result<(), String> {
    for object in objects.types.values() {
        if let Some(variant) = &object.variant {
            check_variant(objects, object, variant)
                .map_err(|message| format!("`{}`: `variant`: {message}", object.name))?;
        }
        for field in object.fields.values() {
            if let Some(settable) = &field.set {
                check_settable(objects, entries, field, settable)
                    .map_err(|message| format!("`{}.{}`: {message}", object.name, field.name))?;
            }
            if let Shape::Value {
                meaning: Meaning::Element(window),
                ..
            } = &field.shape
            {
                let text = window
                    .rsplit_once('.')
                    .and_then(|(owner, name)| objects.field(owner, name));
                if !matches!(text, Some((_, found)) if matches!(found.shape, Shape::Text(_))) {
                    return Err(format!(
                        "`{}.{}`: `element` names `{window}`, which is not a text field",
                        object.name, field.name
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The partition's field is an enum value of the element, and it names
/// values of that enum and known domains.
fn check_partition(
    objects: &Objects,
    domains: &BTreeSet<String>,
    element: &str,
    partition: &Partition,
) -> Result<(), String> {
    let Some((_, field)) = objects.field(element, &partition.field) else {
        return Err(format!(
            "`{}` is not a field of `{element}`",
            partition.field
        ));
    };
    let Shape::Value {
        meaning: Meaning::Enum(enum_name),
        ..
    } = &field.shape
    else {
        return Err(format!("`{}` is not an enum value", partition.field));
    };
    let values = &objects.enums[enum_name].values;
    for (value, domain) in &partition.domains {
        if !values.contains_key(value) {
            return Err(format!("`{enum_name}` has no value `{value}`"));
        }
        if !domains.contains(domain) {
            return Err(format!("domain `{domain}` is not in domains.toml"));
        }
    }
    Ok(())
}

/// A variant's field is an enum value of the base, and it names values of
/// that enum.
fn check_variant(objects: &Objects, object: &Type, variant: &Variant) -> Result<(), String> {
    let base = object.extends.as_deref().unwrap_or_default();
    let Some((_, field)) = objects.field(base, &variant.field) else {
        return Err(format!("`{}` is not a field of `{base}`", variant.field));
    };
    let Shape::Value {
        meaning: Meaning::Enum(enum_name),
        ..
    } = &field.shape
    else {
        return Err(format!("`{}` is not an enum value", variant.field));
    };
    for value in &variant.values {
        if !objects.enums[enum_name].values.contains_key(value) {
            return Err(format!("`{enum_name}` has no value `{value}`"));
        }
    }
    Ok(())
}

/// The function a value is set through: of the main thread, taking the
/// object and the value, returning nothing.
fn check_setter(
    entries: &BTreeMap<String, Entry>,
    call: &str,
    value: ValueKind,
) -> Result<(), String> {
    let abi = match entries.get(call).map(|entry| &entry.kind) {
        Some(EntryKind::Function {
            abi,
            thread: crate::Thread::Main,
        }) => abi,
        Some(EntryKind::Function { .. }) => {
            return Err(format!("`{call}` is not a function of the main thread"));
        }
        Some(_) => return Err(format!("`{call}` is not a function")),
        None => return Err(format!("`{call}` is not an entry")),
    };
    let fits = abi.args.len() == 2
        && abi.args[0] == ValueKind::Ptr
        && (abi.args[1] == value
            || (value == ValueKind::Bool
                && matches!(abi.args[1], ValueKind::U8 | ValueKind::Bool)))
        && abi.ret == ValueKind::Void;
    if fits {
        Ok(())
    } else {
        Err(format!(
            "`{call}` does not take (ptr object, {}) and return nothing",
            format!("{value:?}").to_lowercase()
        ))
    }
}

fn check_settable(
    objects: &Objects,
    entries: &BTreeMap<String, Entry>,
    field: &Field,
    settable: &Settable,
) -> Result<(), String> {
    let kind = match &field.shape {
        Shape::Value { kind, .. } => *kind,
        Shape::Text(_) if settable.call.is_some() => ValueKind::Ptr,
        Shape::Text(_) => return Err("text is set only through the game's function: `call`".into()),
        _ => return Err("only a value or text is set".into()),
    };
    if matches!(field.shape, Shape::Value { .. })
        && matches!(kind, ValueKind::Ptr | ValueKind::Void | ValueKind::Struct)
    {
        return Err("a pointer is never set".into());
    }
    if field.bit.is_some() && settable.call.is_none() {
        return Err("a bit is set only through the game's function: `call`".into());
    }
    if let Some(call) = &settable.call {
        check_setter(entries, call, kind)?;
    }
    if settable.range.is_some() && !matches!(field.shape, Shape::Value { .. }) {
        return Err("only a number has a range".into());
    }
    if objects
        .domains
        .get(&field.domain)
        .is_none_or(|domain| domain.change.is_none())
    {
        return Err(format!(
            "domain `{}` has no `change` wording for the player",
            field.domain
        ));
    }
    if let Some((least, most)) = settable.range {
        let float = matches!(kind, ValueKind::F32 | ValueKind::F64);
        for bound in [least, most] {
            if matches!(bound, crate::Number::Float(_)) && !float {
                return Err("an integer field has an integer range".into());
            }
        }
        if least.float() > most.float() {
            return Err("a range's least value is above its most".into());
        }
    }
    Ok(())
}

pub(crate) fn is_integer(kind: ValueKind) -> bool {
    matches!(
        kind,
        ValueKind::I8
            | ValueKind::I16
            | ValueKind::I32
            | ValueKind::I64
            | ValueKind::U8
            | ValueKind::U16
            | ValueKind::U32
            | ValueKind::U64
    )
}

fn check_field(
    type_name: &str,
    field: &mut Field,
    entries: &BTreeMap<String, Entry>,
    domains: &BTreeSet<String>,
    types: &BTreeSet<String>,
    enums: &BTreeMap<String, Enum>,
) -> Result<(), String> {
    if !domains.contains(&field.domain) {
        return Err(format!("domain `{}` is not in domains.toml", field.domain));
    }
    let Some(entry) = entries.get(&field.at) else {
        return Err(format!("`{}` is not an entry", field.at));
    };
    let EntryKind::Field { of, value, .. } = &entry.kind else {
        return Err(format!("`{}` is not a field entry", field.at));
    };
    if of != type_name {
        return Err(format!(
            "`{}` is a field of `{of}`, not of `{type_name}`",
            field.at
        ));
    }
    let known = |name: &str| -> Result<(), String> {
        if types.contains(name) {
            Ok(())
        } else {
            Err(format!("`{name}` is not a type"))
        }
    };
    match &mut field.shape {
        Shape::Value { kind, meaning, .. } => {
            if matches!(value, ValueKind::Ptr | ValueKind::Struct | ValueKind::Void) {
                return Err(format!(
                    "`{}` is not a value; a value field needs a number or bool entry",
                    field.at
                ));
            }
            if let Some(bit) = &mut field.bit {
                check_bit(&field.at, bit, *value)?;
                *kind = ValueKind::Bool;
                return Ok(());
            }
            match meaning {
                Meaning::Enum(name) => match enums.get(name) {
                    Some(found) if !found.flags => {}
                    Some(_) => return Err(format!("`{name}` holds flags; use `flags`")),
                    None => return Err(format!("`{name}` is not an enum")),
                },
                Meaning::Flags(name) => match enums.get(name) {
                    Some(found) if found.flags => {}
                    Some(_) => return Err(format!("`{name}` is not a flags enum")),
                    None => return Err(format!("`{name}` is not an enum")),
                },
                Meaning::Sheet(_) | Meaning::Plain | Meaning::Element(_) => {}
            }
            if !matches!(meaning, Meaning::Plain) && !is_integer(*value) {
                return Err("a sheet row, enum, or flags is an integer".into());
            }
            *kind = *value;
        }
        Shape::Text(_) => {
            if !matches!(value, ValueKind::Struct | ValueKind::Ptr) {
                return Err(format!(
                    "text is stored inline (`{}` has the type struct) or behind a pointer (ptr)",
                    field.at
                ));
            }
            if let Some(pointer) = &field.pointer {
                check_pointer(&field.at, pointer, *value, entries)?;
            }
        }
        Shape::Ref(to) => {
            known(to)?;
            if *value != ValueKind::Ptr {
                return Err(format!(
                    "a reference is a pointer: `{}` has the type ptr",
                    field.at
                ));
            }
        }
        Shape::Embed(of) => {
            known(of)?;
            if *value != ValueKind::Struct {
                return Err(format!(
                    "an embedded object is a struct: `{}` has the type struct",
                    field.at
                ));
            }
        }
        Shape::Array {
            array,
            behind_pointer,
        } => {
            known(&array.of)?;
            *behind_pointer = match value {
                ValueKind::Struct => false,
                ValueKind::Ptr => true,
                _ => {
                    return Err(format!(
                        "an array is embedded (struct) or behind a pointer (ptr), not `{}`",
                        field.at
                    ));
                }
            };
        }
    }
    Ok(())
}

/// A bit of an integer entry of the kind `value`: within it. Notes the
/// entry's kind.
fn check_bit(at: &str, bit: &mut Bit, value: ValueKind) -> Result<(), String> {
    let bits = match value {
        ValueKind::U8 | ValueKind::I8 => 8,
        ValueKind::U16 | ValueKind::I16 => 16,
        ValueKind::U32 | ValueKind::I32 => 32,
        ValueKind::U64 | ValueKind::I64 => 64,
        _ => return Err(format!("`{at}` is not an integer: it has no bits")),
    };
    if bit.index >= bits {
        return Err(format!("`{at}` has {bits} bits"));
    }
    bit.kind = value;
    Ok(())
}

/// Text behind `pointer` in the structure at the entry `at`, of the kind
/// `value`.
fn check_pointer(
    at: &str,
    pointer: &str,
    value: ValueKind,
    entries: &BTreeMap<String, Entry>,
) -> Result<(), String> {
    if value != ValueKind::Struct {
        return Err(format!(
            "text behind a `pointer` is in a structure: `{at}` has the type struct"
        ));
    }
    if matches!(
        entries.get(pointer).map(|entry| &entry.kind),
        Some(EntryKind::Field {
            value: ValueKind::Ptr,
            ..
        })
    ) {
        Ok(())
    } else {
        Err(format!("`{pointer}` is not a field entry of the type ptr"))
    }
}

/// No type extends or embeds itself, directly or through others: such an
/// object would be infinitely large.
fn no_cycles(objects: &Objects) -> Result<(), String> {
    fn contained(object: &Type) -> impl Iterator<Item = &str> {
        object
            .extends
            .as_deref()
            .into_iter()
            .chain(
                object
                    .fields
                    .values()
                    .filter_map(|field| match &field.shape {
                        Shape::Embed(of) => Some(of.as_str()),
                        Shape::Array {
                            array,
                            behind_pointer: false,
                        } if !array.pointers => Some(array.of.as_str()),
                        _ => None,
                    }),
            )
    }
    fn visit<'a>(
        objects: &'a Objects,
        name: &'a str,
        path: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }
        if let Some(start) = path.iter().position(|seen| *seen == name) {
            let mut cycle = path[start..].to_vec();
            cycle.push(name);
            return Err(format!("types contain each other: {}", cycle.join(" -> ")));
        }
        path.push(name);
        if let Some(object) = objects.types.get(name) {
            for inner in contained(object) {
                visit(objects, inner, path, done)?;
            }
        }
        path.pop();
        done.insert(name);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for name in objects.types.keys() {
        visit(objects, name, &mut Vec::new(), &mut done)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Map;

    const DOMAINS: &str = r#"
[domains.inventory]
doc = "The items the player carries."
class = "you"
read = { en = "Sees the items you carry", ru = "Видит предметы, которые вы носите" }
"#;

    const ENTRIES: &str = r#"
[global."inventory.manager.instance"]
locator = { signature = "48 8D 0D ?? ?? ?? ?? E8", follow = "rel32", at = 3 }
type = "struct"

[field."inventory.manager.containers"]
of = "inventory.manager"
type = "ptr"
offset = 0x1E08

[field."inventory.container.items"]
of = "inventory.container"
type = "ptr"
offset = 0x00

[field."inventory.container.kind"]
of = "inventory.container"
type = "u32"
offset = 0x08

[field."inventory.container.size"]
of = "inventory.container"
type = "u32"
offset = 0x0C

[field."inventory.item.item"]
of = "inventory.item"
type = "u32"
offset = 0x0C

[field."inventory.item.quantity"]
of = "inventory.item"
type = "u32"
offset = 0x10
"#;

    const TYPES: &str = r#"
[types."inventory.manager"]
doc = "Every container of the player's items."
domain = "inventory"

[types."inventory.manager".fields.containers]
doc = "The containers."
array = { of = "inventory.container", length = 74, stride = 24 }

[types."inventory.container"]
doc = "One container: a bag, the crystals, the currency, the equipped gear."
domain = "inventory"

[types."inventory.container".fields.kind]
doc = "Which container this is."
enum = "inventory.container_kind"

[types."inventory.container".fields.size]
doc = "How many slots it has."

[types."inventory.container".fields.items]
doc = "The slots."
array = { of = "inventory.item", count = "size", most = 200, stride = 64, present = "item" }

[types."inventory.item"]
doc = "One slot."
domain = "inventory"

[types."inventory.item".fields.item]
doc = "The item in the slot."
sheet = "Item"

[types."inventory.item".fields.quantity]
doc = "How many."
"#;

    const ENUM: &str = r#"
doc = "The kinds of container."
values = { bag_1 = 0, bag_2 = 1, crystals = 2001 }
"#;

    const ROOTS: &str = r#"
[roots.inventory]
doc = "The player's items."
global = "inventory.manager.instance"
type = "inventory.manager"
"#;

    fn map(types: &str, roots: &str) -> Result<Map, crate::MapError> {
        Map::from_sources(
            "format = 2",
            &[
                ("entries/inventory.toml".into(), ENTRIES.into()),
                ("domains.toml".into(), DOMAINS.into()),
                ("enums/inventory.container_kind.toml".into(), ENUM.into()),
                ("types/inventory.toml".into(), types.into()),
                ("roots.toml".into(), roots.into()),
            ],
        )
    }

    #[test]
    fn a_valid_model_reads_kinds_from_its_entries() {
        let map = map(TYPES, ROOTS).unwrap();
        let objects = map.objects();
        let (_, quantity) = objects.field("inventory.item", "quantity").unwrap();
        assert!(matches!(
            quantity.shape,
            Shape::Value {
                kind: ValueKind::U32,
                ..
            }
        ));
        let (_, items) = objects.field("inventory.container", "items").unwrap();
        assert!(matches!(
            &items.shape,
            Shape::Array { behind_pointer: true, array } if array.length == Length::Field { field: "size".into(), most: 200 }
        ));
        assert_eq!(objects.roots["inventory"].r#type, "inventory.manager");
        assert_eq!(objects.domains["inventory"].class, DomainClass::You);
    }

    #[test]
    fn mistakes_make_the_map_invalid() {
        let wrong = [
            TYPES.replace("sheet = \"Item\"", "ref = \"inventory.item\""),
            TYPES.replace(
                "domain = \"inventory\"\n\n[types.\"inventory.item\".fields.item]",
                "domain = \"party\"\n\n[types.\"inventory.item\".fields.item]",
            ),
            TYPES.replace(
                "enum = \"inventory.container_kind\"",
                "enum = \"inventory.nothing\"",
            ),
            TYPES.replace("present = \"item\"", "present = \"nothing\""),
            TYPES.replace("count = \"size\"", "count = \"items\""),
            TYPES.replace(
                "doc = \"How many.\"",
                "doc = \"How many.\"\nat = \"inventory.container.size\"",
            ),
        ];
        for types in wrong {
            assert!(map(&types, ROOTS).is_err(), "{types}");
        }
        assert!(
            map(
                TYPES,
                &ROOTS.replace("inventory.manager.instance", "inventory.item.item")
            )
            .is_err()
        );
    }

    #[test]
    fn only_values_of_a_domain_with_change_wording_are_set() {
        let changing = DOMAINS.replace(
            "read = { en = \"Sees the items you carry\", ru = \"Видит предметы, которые вы носите\" }",
            "read = { en = \"Sees the items you carry\", ru = \"Видит предметы, которые вы носите\" }
change = { en = \"Changes\", ru = \"Меняет\" }",
        );
        let with = |domains: &str, set: &str| {
            let types = TYPES.replace(
                "[types.\"inventory.item\".fields.quantity]
doc = \"How many.\"
",
                &format!(
                    "[types.\"inventory.item\".fields.quantity]
doc = \"How many.\"
{set}
"
                ),
            );
            Map::from_sources(
                "format = 2",
                &[
                    ("entries/inventory.toml".into(), ENTRIES.into()),
                    ("domains.toml".into(), domains.into()),
                    ("enums/inventory.container_kind.toml".into(), ENUM.into()),
                    ("types/inventory.toml".into(), types),
                    ("roots.toml".into(), ROOTS.into()),
                ],
            )
        };
        let map = with(&changing, "set = { range = [1, 99] }").unwrap();
        let (_, quantity) = map.objects().field("inventory.item", "quantity").unwrap();
        assert_eq!(
            quantity.set.as_ref().and_then(|set| set.range),
            Some((crate::Number::Integer(1), crate::Number::Integer(99)))
        );
        assert!(with(DOMAINS, "set = true").is_err(), "no change wording");
        for wrong in [
            "set = { range = [1.5, 2] }",
            "set = { range = [9, 1] }",
            "set = { most = 2 }",
            "set = 3",
        ] {
            assert!(with(&changing, wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn types_that_contain_each_other_are_refused() {
        let cycle = format!(
            "{TYPES}\n[types.\"inventory.manager\".fields.self_copy]\ndoc = \"x\"\nat = \"inventory.manager.containers\"\nembed = \"inventory.manager\"\n"
        );
        // The entry is a pointer, so the embed fails first; a struct entry would loop.
        assert!(map(&cycle, ROOTS).is_err());
    }

    const WINDOW_ENTRIES: &str = r#"
[field."ui.window.name"]
of = "ui.window"
type = "struct"
offset = 0x10

[field."ui.node.id"]
of = "ui.node"
type = "u32"
offset = 0x0

[field."ui.node.kind"]
of = "ui.node"
type = "u16"
offset = 0x4

[field."ui.node.flags"]
of = "ui.node"
type = "u32"
offset = 0x8

[field."ui.text_node.text"]
of = "ui.text_node"
type = "struct"
offset = 0x10

[field."ui.string.bytes"]
of = "ui.string"
type = "ptr"
offset = 0x0

[function."ui.node.set_visible"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u8"], ret = "void" }
thread = "main"

[function."ui.text_node.set_text"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "void" }
thread = "main"
"#;

    const WINDOW_TYPES: &str = r#"
[types."ui.window"]
doc = "A window."
domain = "windows"

[types."ui.window".fields.name]
doc = "Its name."
text = 32

[types."ui.node"]
doc = "An element."
domain = "windows"

[types."ui.node".fields.name]
doc = "Its name."
at = "ui.node.id"
element = "ui.window.name"

[types."ui.node".fields.kind]
doc = "What it is."
enum = "ui.node_kind"

[types."ui.node".fields.visible]
doc = "Whether it is shown."
at = "ui.node.flags"
bit = 20
set = { call = "ui.node.set_visible", hold = true }

[types."ui.text_node"]
doc = "A text element."
domain = "windows"
extends = "ui.node"
variant = { field = "kind", values = ["text"] }

[types."ui.text_node".fields.text]
doc = "Its text."
pointer = "ui.string.bytes"
text = 64
set = { call = "ui.text_node.set_text", hold = true }
"#;

    fn windows(entries: &str, types: &str) -> Result<Map, crate::MapError> {
        Map::from_sources(
            "format = 2",
            &[
                ("entries/ui.toml".into(), entries.into()),
                (
                    "domains.toml".into(),
                    r#"
[domains.windows]
doc = "Windows."
class = "others"
read = { en = "Sees windows", ru = "Видит окна" }
change = { en = "Changes windows", ru = "Меняет окна" }
"#
                    .into(),
                ),
                (
                    "enums/ui.node_kind.toml".into(),
                    "doc = \"Kinds.\"\nvalues = { res = 1, text = 3 }\n".into(),
                ),
                ("types/ui.toml".into(), types.into()),
            ],
        )
    }

    #[test]
    fn window_elements_are_bits_variants_and_values_set_through_functions() {
        let map = windows(WINDOW_ENTRIES, WINDOW_TYPES).unwrap();
        let objects = map.objects();
        let (_, visible) = objects.field("ui.node", "visible").unwrap();
        assert_eq!(
            visible.bit,
            Some(Bit {
                index: 20,
                kind: ValueKind::U32
            })
        );
        assert!(matches!(
            visible.shape,
            Shape::Value {
                kind: ValueKind::Bool,
                ..
            }
        ));
        let set = visible.set.as_ref().unwrap();
        assert_eq!(set.call.as_deref(), Some("ui.node.set_visible"));
        assert!(set.hold);
        let (variant, _, text) = objects.variant_field("ui.node", "text").unwrap();
        assert_eq!(variant.name, "ui.text_node");
        assert_eq!(text.pointer.as_deref(), Some("ui.string.bytes"));
        assert!(objects.variant_field("ui.node", "visible").is_none());
        assert!(
            objects
                .references()
                .any(|entry| entry == "ui.text_node.set_text")
        );
        assert!(objects.references().any(|entry| entry == "ui.string.bytes"));

        let wrong = [
            // Text is set only through a function.
            WINDOW_TYPES.replace(
                "set = { call = \"ui.text_node.set_text\", hold = true }",
                "set = true",
            ),
            // A bit too.
            WINDOW_TYPES.replace(
                "set = { call = \"ui.node.set_visible\", hold = true }",
                "set = { hold = true }",
            ),
            // The function takes text, not a bool.
            WINDOW_TYPES.replace("ui.node.set_visible", "ui.text_node.set_text"),
            WINDOW_TYPES.replace("bit = 20", "bit = 32"),
            WINDOW_TYPES.replace("bit = 20", "bit = 20\nunit = \"s\""),
            WINDOW_TYPES.replace("element = \"ui.window.name\"", "element = \"ui.node.kind\""),
            WINDOW_TYPES.replace("values = [\"text\"]", "values = [\"image\"]"),
            WINDOW_TYPES.replace(
                "variant = { field = \"kind\"",
                "variant = { field = \"name\"",
            ),
            WINDOW_TYPES.replace("extends = \"ui.node\"\n", ""),
            WINDOW_TYPES.replace("pointer = \"ui.string.bytes\"", "pointer = \"ui.node.id\""),
            WINDOW_TYPES.replace(
                "hold = true }\n\n[types.\"ui.text_node\"]",
                "hold = 1 }\n\n[types.\"ui.text_node\"]",
            ),
        ];
        for types in wrong {
            assert!(windows(WINDOW_ENTRIES, &types).is_err(), "{types}");
        }
        let any_thread = WINDOW_ENTRIES.replacen("thread = \"main\"", "thread = \"any\"", 1);
        assert!(
            windows(&any_thread, WINDOW_TYPES).is_err(),
            "a setter runs on the main thread"
        );
    }

    #[test]
    fn inherited_fields_are_found_through_the_base() {
        let base = TYPES.replace(
            "[types.\"inventory.item\"]\ndoc = \"One slot.\"\ndomain = \"inventory\"",
            "[types.\"inventory.item\"]\ndoc = \"One slot.\"\ndomain = \"inventory\"\nextends = \"inventory.container\"",
        );
        let map = map(&base, ROOTS).unwrap();
        assert!(map.objects().field("inventory.item", "size").is_some());
        assert!(map.objects().fields("inventory.item").contains_key("kind"));
    }
}
