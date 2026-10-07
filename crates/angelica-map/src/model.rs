use std::collections::BTreeMap;

use angelica_pe::Signature;
use serde::Deserialize;

/// The thread kind a function or location runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thread {
    /// Only the game's main thread.
    Main,
    /// Any thread.
    Any,
}

/// A value kind of the Windows x64 calling convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Ptr,
    Bool,
    Void,
}

impl ValueKind {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            "ptr" => Self::Ptr,
            "bool" => Self::Bool,
            "void" => Self::Void,
            _ => return None,
        })
    }
}

/// Arguments and return kind of a function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Abi {
    pub args: Vec<ValueKind>,
    pub ret: ValueKind,
}

/// How a signature locator turns its match into an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    /// The target of the 32-bit relative operand at the located address.
    Rel32,
}

/// How an entry is found in a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locator {
    /// The single match of a signature in the code sections, plus `at`, and
    /// optionally the target of the rel32 operand there.
    Signature {
        signature: Signature,
        at: i64,
        follow: Option<Follow>,
    },
    /// The function in a slot of a vtable entry.
    VtableSlot { vtable: String, slot: u32 },
    /// The displacement of the memory operand of the single instruction
    /// matching a signature inside a function entry's range.
    Operand {
        function: String,
        signature: Signature,
    },
}

/// A condition that must hold for a resolved entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Another locator resolves to the same value.
    Locator(Locator),
    /// Another entry has the same value.
    Equals(String),
    /// The address is slot `slot` of a table outside code whose first
    /// `code_slots` slots all point into code.
    InVtable { slot: u32, code_slots: u32 },
}

/// Where an entry came from, when it started from external data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub source: String,
    pub commit: String,
}

/// What an entry describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Function {
        abi: Abi,
        thread: Thread,
    },
    Location {
        thread: Thread,
    },
    Global {
        value: ValueKind,
    },
    Vtable {
        slots: u32,
    },
    Field {
        of: String,
        value: ValueKind,
        offset: Option<u32>,
    },
    Patch {
        original: Vec<u8>,
        replacement: Vec<u8>,
    },
}

impl EntryKind {
    /// The table name of the kind in an entries file.
    #[must_use]
    pub fn table(&self) -> &'static str {
        match self {
            Self::Function { .. } => "function",
            Self::Location { .. } => "location",
            Self::Global { .. } => "global",
            Self::Vtable { .. } => "vtable",
            Self::Field { .. } => "field",
            Self::Patch { .. } => "patch",
        }
    }
}

/// One entry of the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    /// `None` only for a field with a constant offset.
    pub locator: Option<Locator>,
    pub checks: Vec<Check>,
    pub depends: Vec<String>,
    pub doc: Option<String>,
    pub origin: Option<Origin>,
}

impl Entry {
    /// Every entry this one needs, in the order they are named.
    #[must_use]
    pub fn references(&self) -> Vec<&str> {
        fn locator_reference(locator: &Locator) -> Option<&str> {
            match locator {
                Locator::VtableSlot { vtable, .. } => Some(vtable),
                Locator::Operand { function, .. } => Some(function),
                Locator::Signature { .. } => None,
            }
        }

        let mut references = Vec::new();
        references.extend(self.locator.as_ref().and_then(locator_reference));
        for check in &self.checks {
            match check {
                Check::Locator(locator) => references.extend(locator_reference(locator)),
                Check::Equals(name) => references.push(name),
                Check::InVtable { .. } => {}
            }
        }
        references.extend(self.depends.iter().map(String::as_str));
        references
    }
}

/// An entries file as written.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFile {
    #[serde(default)]
    function: BTreeMap<String, RawEntry>,
    #[serde(default)]
    location: BTreeMap<String, RawEntry>,
    #[serde(default)]
    global: BTreeMap<String, RawEntry>,
    #[serde(default)]
    vtable: BTreeMap<String, RawEntry>,
    #[serde(default)]
    field: BTreeMap<String, RawEntry>,
    #[serde(default)]
    patch: BTreeMap<String, RawEntry>,
}

impl RawFile {
    /// Every entry with the table it is in.
    pub(crate) fn into_entries(self) -> impl Iterator<Item = (&'static str, String, RawEntry)> {
        let tables: [(&'static str, BTreeMap<String, RawEntry>); 6] = [
            ("function", self.function),
            ("location", self.location),
            ("global", self.global),
            ("vtable", self.vtable),
            ("field", self.field),
            ("patch", self.patch),
        ];
        tables.into_iter().flat_map(|(table, entries)| {
            entries
                .into_iter()
                .map(move |(name, entry)| (table, name, entry))
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawEntry {
    locator: Option<RawLocator>,
    abi: Option<RawAbi>,
    thread: Option<String>,
    #[serde(rename = "type")]
    value_type: Option<String>,
    slots: Option<u32>,
    of: Option<String>,
    offset: Option<u32>,
    original: Option<String>,
    replacement: Option<String>,
    #[serde(default)]
    checks: Vec<RawCheck>,
    #[serde(default)]
    depends: Vec<String>,
    doc: Option<String>,
    origin: Option<RawOrigin>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAbi {
    args: Vec<String>,
    ret: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOrigin {
    source: String,
    commit: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLocator {
    signature: Option<String>,
    at: Option<i64>,
    follow: Option<String>,
    vtable: Option<String>,
    slot: Option<u32>,
    #[serde(rename = "in")]
    function: Option<String>,
    operand: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    signature: Option<String>,
    at: Option<i64>,
    follow: Option<String>,
    vtable: Option<String>,
    slot: Option<u32>,
    #[serde(rename = "in")]
    function: Option<String>,
    operand: Option<String>,
    equals: Option<String>,
    vtable_slot: Option<u32>,
    code_slots: Option<u32>,
}

impl RawEntry {
    /// Refuses a field that does not belong to the kind of entry.
    fn reject_foreign_fields(&self, table: &str) -> Result<(), String> {
        let present = [
            ("locator", self.locator.is_some()),
            ("abi", self.abi.is_some()),
            ("thread", self.thread.is_some()),
            ("type", self.value_type.is_some()),
            ("slots", self.slots.is_some()),
            ("of", self.of.is_some()),
            ("offset", self.offset.is_some()),
            ("original", self.original.is_some()),
            ("replacement", self.replacement.is_some()),
        ];
        let allowed: &[&str] = match table {
            "function" => &["locator", "abi", "thread"],
            "location" => &["locator", "thread"],
            "global" => &["locator", "type"],
            "vtable" => &["locator", "slots"],
            "field" => &["locator", "of", "offset", "type"],
            "patch" => &["locator", "original", "replacement"],
            _ => unreachable!("tables come from RawFile"),
        };
        if let Some((field, _)) = present
            .iter()
            .find(|(field, present)| *present && !allowed.contains(field))
        {
            return Err(format!("`{field}` does not apply to a {table}"));
        }
        Ok(())
    }

    /// Validates the fields of an entry of the given table. The message of
    /// an error names the field.
    pub(crate) fn into_entry(self, table: &str, name: String) -> Result<Entry, String> {
        self.reject_foreign_fields(table)?;
        let locator = self.locator.map(convert_locator).transpose()?;
        let kind = match table {
            "function" => EntryKind::Function {
                abi: convert_abi(&required(self.abi, "abi")?)?,
                thread: convert_thread(&required(self.thread, "thread")?)?,
            },
            "location" => EntryKind::Location {
                thread: convert_thread(&required(self.thread, "thread")?)?,
            },
            "global" => EntryKind::Global {
                value: convert_value_type(&required(self.value_type, "type")?)?,
            },
            "vtable" => {
                let slots = required(self.slots, "slots")?;
                if slots == 0 {
                    return Err("`slots` must be at least 1".to_owned());
                }
                EntryKind::Vtable { slots }
            }
            "field" => EntryKind::Field {
                of: required(self.of, "of")?,
                value: convert_value_type(&required(self.value_type, "type")?)?,
                offset: self.offset,
            },
            "patch" => {
                let original = convert_bytes(&required(self.original, "original")?, "original")?;
                let replacement =
                    convert_bytes(&required(self.replacement, "replacement")?, "replacement")?;
                if original.len() != replacement.len() {
                    return Err("`original` and `replacement` must have the same length".to_owned());
                }
                EntryKind::Patch {
                    original,
                    replacement,
                }
            }
            _ => unreachable!("tables come from RawFile"),
        };

        check_locator(&kind, locator.as_ref())?;

        let checks = self
            .checks
            .into_iter()
            .enumerate()
            .map(|(index, check)| {
                convert_check(check).map_err(|message| format!("check {index}: {message}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let origin = self
            .origin
            .map(|origin| {
                if origin.source.trim().is_empty() || origin.commit.trim().is_empty() {
                    Err("`origin` needs a `source` and a `commit`".to_owned())
                } else {
                    Ok(Origin {
                        source: origin.source,
                        commit: origin.commit,
                    })
                }
            })
            .transpose()?;

        Ok(Entry {
            name,
            kind,
            locator,
            checks,
            depends: self.depends,
            doc: self.doc,
            origin,
        })
    }
}

/// Whether the kind of entry and its locator go together.
fn check_locator(kind: &EntryKind, locator: Option<&Locator>) -> Result<(), String> {
    match (kind, locator) {
        (
            EntryKind::Field {
                offset: Some(_), ..
            },
            Some(_),
        ) => {
            return Err("a field has either `locator` or `offset`, not both".to_owned());
        }
        (EntryKind::Field { offset: None, .. }, None) => {
            return Err("a field needs `locator` or `offset`".to_owned());
        }
        (EntryKind::Field { .. }, Some(locator)) if !matches!(locator, Locator::Operand { .. }) => {
            return Err("the locator of a field is an `in` locator".to_owned());
        }
        (EntryKind::Field { .. }, _) => {}
        (_, None) => return Err("`locator` is required".to_owned()),
        (_, Some(Locator::Operand { .. })) => {
            return Err("an `in` locator locates only a field".to_owned());
        }
        (_, Some(_)) => {}
    }
    Ok(())
}

fn required<T>(value: Option<T>, field: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("`{field}` is required"))
}

fn convert_thread(text: &str) -> Result<Thread, String> {
    match text {
        "main" => Ok(Thread::Main),
        "any" => Ok(Thread::Any),
        _ => Err(format!("`thread` is `main` or `any`, not `{text}`")),
    }
}

fn convert_value_type(text: &str) -> Result<ValueKind, String> {
    match ValueKind::parse(text) {
        Some(ValueKind::Void) => Err("`type` cannot be `void`".to_owned()),
        Some(kind) => Ok(kind),
        None => Err(format!("`{text}` is not a value kind")),
    }
}

fn convert_abi(abi: &RawAbi) -> Result<Abi, String> {
    let args = abi
        .args
        .iter()
        .map(|arg| match ValueKind::parse(arg) {
            Some(ValueKind::Void) => Err("an argument cannot be `void`".to_owned()),
            Some(kind) => Ok(kind),
            None => Err(format!("`{arg}` is not a value kind")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ret =
        ValueKind::parse(&abi.ret).ok_or_else(|| format!("`{}` is not a value kind", abi.ret))?;
    Ok(Abi { args, ret })
}

fn convert_signature(text: &str) -> Result<Signature, String> {
    Signature::parse(text).map_err(|error| format!("signature: {error}"))
}

/// Hexadecimal bytes without wildcards.
fn convert_bytes(text: &str, field: &str) -> Result<Vec<u8>, String> {
    let bytes = text
        .split_whitespace()
        .map(|token| {
            (token.len() == 2 && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| u8::from_str_radix(token, 16).ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("`{field}` is hexadecimal bytes separated by spaces"))?;
    if bytes.is_empty() {
        return Err(format!("`{field}` is empty"));
    }
    Ok(bytes)
}

fn convert_check(raw: RawCheck) -> Result<Check, String> {
    let locator_fields = raw.signature.is_some()
        || raw.at.is_some()
        || raw.follow.is_some()
        || raw.vtable.is_some()
        || raw.slot.is_some()
        || raw.function.is_some()
        || raw.operand.is_some();
    match (raw.equals, raw.vtable_slot, raw.code_slots) {
        (Some(name), None, None) if !locator_fields => Ok(Check::Equals(name)),
        (None, Some(slot), Some(code_slots)) if !locator_fields => {
            if code_slots <= slot {
                Err("`code_slots` must be greater than `vtable_slot`".to_owned())
            } else {
                Ok(Check::InVtable { slot, code_slots })
            }
        }
        (None, None, None) => convert_locator(RawLocator {
            signature: raw.signature,
            at: raw.at,
            follow: raw.follow,
            vtable: raw.vtable,
            slot: raw.slot,
            function: raw.function,
            operand: raw.operand,
        })
        .map(Check::Locator),
        _ => Err("a check is a locator, `equals`, or `vtable_slot` with `code_slots`".to_owned()),
    }
}

fn convert_locator(raw: RawLocator) -> Result<Locator, String> {
    let RawLocator {
        signature,
        at,
        follow,
        vtable,
        slot,
        function,
        operand,
    } = raw;
    if let Some(function) = function {
        if at.is_some() || follow.is_some() || vtable.is_some() || slot.is_some() {
            return Err("an `in` locator has only `in`, `signature`, and `operand`".to_owned());
        }
        if operand.as_deref() != Some("disp") {
            return Err("an `in` locator needs `operand = \"disp\"`".to_owned());
        }
        let signature = convert_signature(&required(signature, "signature")?)?;
        return Ok(Locator::Operand {
            function,
            signature,
        });
    }
    if operand.is_some() {
        return Err("`operand` belongs to an `in` locator".to_owned());
    }
    if let Some(vtable) = vtable {
        if signature.is_some() || at.is_some() || follow.is_some() {
            return Err("a `vtable` locator has only `vtable` and `slot`".to_owned());
        }
        return Ok(Locator::VtableSlot {
            vtable,
            slot: required(slot, "slot")?,
        });
    }
    if slot.is_some() {
        return Err("`slot` belongs to a `vtable` locator".to_owned());
    }
    let signature = convert_signature(
        &signature.ok_or_else(|| "a locator needs `signature`, `vtable`, or `in`".to_owned())?,
    )?;
    let follow = match follow.as_deref() {
        None => None,
        Some("rel32") => Some(Follow::Rel32),
        Some(other) => return Err(format!("`follow` is `rel32`, not `{other}`")),
    };
    Ok(Locator::Signature {
        signature,
        at: at.unwrap_or(0),
        follow,
    })
}
