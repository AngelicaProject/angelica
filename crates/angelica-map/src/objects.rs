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
}

/// A structure of the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    pub name: String,
    pub doc: String,
    pub domain: String,
    /// A type whose fields this one has too, at the same offsets.
    pub extends: Option<String>,
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

    /// Every entry the object model refers to.
    pub fn references(&self) -> impl Iterator<Item = &str> {
        self.types
            .values()
            .flat_map(|object| object.fields.values().map(|field| field.at.as_str()))
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
    #[serde(default)]
    fields: BTreeMap<String, RawField>,
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
    unit: Option<String>,
    domain: Option<String>,
    #[serde(default)]
    forbidden: bool,
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
        roots.insert(
            name.clone(),
            Root {
                name,
                doc: root.doc,
                global: root.global,
                r#type: root.r#type,
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
        types.push(Type {
            name,
            doc: object.doc,
            domain: object.domain,
            extends: object.extends,
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
        + usize::from(raw.flags.is_some());
    if meanings > 1 {
        return Err("a value means at most one of sheet, enum, and flags".into());
    }
    if shapes == 1 && (meanings > 0 || raw.unit.is_some()) {
        return Err("sheet, enum, flags, and unit belong to values only".into());
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
    Ok(Field {
        name: name.to_owned(),
        doc: raw.doc,
        at: raw.at.unwrap_or_else(|| format!("{type_name}.{name}")),
        shape,
        domain: raw.domain.unwrap_or_else(|| domain.to_owned()),
        forbidden: raw.forbidden,
    })
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
                Meaning::Sheet(_) | Meaning::Plain => {}
            }
            if !matches!(meaning, Meaning::Plain) && !is_integer(*value) {
                return Err("a sheet row, enum, or flags is an integer".into());
            }
            *kind = *value;
        }
        Shape::Text(_) => {
            if *value != ValueKind::Struct {
                return Err(format!(
                    "text is stored inline: `{}` has the type struct",
                    field.at
                ));
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
    fn types_that_contain_each_other_are_refused() {
        let cycle = format!(
            "{TYPES}\n[types.\"inventory.manager\".fields.self_copy]\ndoc = \"x\"\nat = \"inventory.manager.containers\"\nembed = \"inventory.manager\"\n"
        );
        // The entry is a pointer, so the embed fails first; a struct entry would loop.
        assert!(map(&cycle, ROOTS).is_err());
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
