//! What Anatomia describes: entries that are found in a build (functions,
//! locations, globals, virtual tables, and fields of structures) and the
//! types they have.

use std::collections::BTreeMap;

use angelica_pe::Signature;
use toml::Value;

use crate::table::Keys;
use crate::ty::{FnType, Primitive, Ty};

/// The thread a function or location runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thread {
    Main,
    Any,
}

impl Thread {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Any => "any",
        }
    }
}

/// Which operand of an instruction a step or a check takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandKind {
    /// The displacement of a memory operand with a base register other
    /// than RIP: a field's offset.
    Disp,
    /// An immediate.
    Imm,
    /// The address a branch goes to or a RIP-relative operand reaches.
    Target,
}

impl OperandKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Disp => "disp",
            Self::Imm => "imm",
            Self::Target => "target",
        }
    }

    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "disp" => Ok(Self::Disp),
            "imm" => Ok(Self::Imm),
            "target" => Ok(Self::Target),
            _ => Err(format!(
                "`operand` is `disp`, `imm`, or `target`, not `{text}`"
            )),
        }
    }
}

/// A slot of a virtual table, by index or by its name in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    Index(u32),
    Name(String),
}

/// One step of a `find` pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The single match in the code sections, or the data sections, or the
    /// code of the function the step before named.
    Scan {
        signature: Signature,
        data: bool,
    },
    /// Restricts the next scan to a function's code.
    In(String),
    Offset(i64),
    /// The target of the relative operand at the cursor plus `n`.
    Rel32(i64),
    Rel8(i64),
    /// An operand of the instruction at the cursor; `index` is the
    /// operand's position, or the first of its kind.
    Operand {
        kind: OperandKind,
        index: Option<u32>,
    },
    /// The pointer stored at the cursor, in a data section.
    Deref,
    Vtable {
        vtable: String,
        slot: Slot,
    },
    /// The value of another entry.
    Entry(String),
    /// The import address table slot of a function of a DLL.
    Import {
        dll: String,
        function: String,
    },
}

impl Step {
    /// The step kind's name, for messages.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Scan { .. } => "scan",
            Self::In(_) => "in",
            Self::Offset(_) => "offset",
            Self::Rel32(_) => "rel32",
            Self::Rel8(_) => "rel8",
            Self::Operand { .. } => "operand",
            Self::Deref => "deref",
            Self::Vtable { .. } => "vtable",
            Self::Entry(_) => "entry",
            Self::Import { .. } => "import",
        }
    }

    fn reference(&self) -> Option<&str> {
        match self {
            Self::In(name) | Self::Entry(name) | Self::Vtable { vtable: name, .. } => Some(name),
            _ => None,
        }
    }
}

/// A condition that holds for a resolved entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// The bytes at the value plus `at`.
    Bytes { bytes: Vec<u8>, at: i64 },
    /// The code of the entry's function, or of `function`, matches once.
    Contains {
        signature: Signature,
        function: Option<String>,
    },
    /// Another pipeline resolves to the entry's value.
    Find(Vec<Step>),
    /// Another entry has the same value.
    Equals(String),
    /// The value is slot `slot` of a table outside code whose first
    /// `code_slots` slots point into code.
    InVtable { slot: u32, code_slots: u32 },
    /// The instruction at the value plus `at` has this immediate.
    Immediate { at: i64, equals: i64 },
}

/// What an entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Function {
        ty: FnType,
        thread: Thread,
        member_of: Option<String>,
    },
    Location {
        thread: Thread,
        registers: BTreeMap<String, Ty>,
    },
    Global {
        ty: Ty,
    },
    Vtable {
        /// How many slots from the first all point into code.
        code_slots: u32,
        slots: BTreeMap<String, (u32, Option<Ty>)>,
    },
    /// A field of a structure that is found in a build: by `at`, a
    /// constant offset, or by `find`.
    Field {
        of: String,
        field: String,
        at: Option<u32>,
    },
}

impl Kind {
    /// The table name of the kind in a file.
    #[must_use]
    pub fn table(&self) -> &'static str {
        match self {
            Self::Function { .. } => "function",
            Self::Location { .. } => "location",
            Self::Global { .. } => "global",
            Self::Vtable { .. } => "vtable",
            Self::Field { .. } => "field",
        }
    }
}

/// One entry: something whose value a build gives, an address or an
/// offset.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub find: Vec<Step>,
    pub checks: Vec<Check>,
    pub depends: Vec<String>,
    pub doc: Option<String>,
    /// Extension attributes by extension name, which the core ignores.
    pub ext: BTreeMap<String, Value>,
}

impl Entry {
    /// Every entry this one needs, in the order they are named.
    #[must_use]
    pub fn references(&self) -> Vec<&str> {
        let mut references: Vec<&str> = self.find.iter().filter_map(Step::reference).collect();
        for check in &self.checks {
            match check {
                Check::Find(steps) => references.extend(steps.iter().filter_map(Step::reference)),
                Check::Equals(name)
                | Check::Contains {
                    function: Some(name),
                    ..
                } => references.push(name),
                _ => {}
            }
        }
        references.extend(self.depends.iter().map(String::as_str));
        references
    }
}

/// How a field reads as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    /// NUL-terminated UTF-8 in a `char8` array, at most its length.
    Terminated,
    /// NUL-terminated UTF-8 of at most this many bytes, behind a pointer or
    /// through the structure the field holds.
    Most(u32),
}

/// Where a field of a structure is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// Found in each build: the entry `<structure>.<field>`.
    Located,
    /// Bits of another field of the same structure.
    Bits { field: String, at: u8, width: u8 },
}

/// A field of a structure.
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub ty: Ty,
    pub place: Place,
    pub doc: Option<String>,
    pub text: Option<Text>,
    /// An integer field of the same object that says how many elements of
    /// the array are used.
    pub count: Option<String>,
    /// A field of the elements that is zero or empty in an unused one.
    pub present: Option<String>,
    pub sheet: Option<String>,
    pub r#enum: Option<String>,
    pub flags: Option<String>,
    pub element: Option<String>,
    pub unit: Option<String>,
    /// Old names of the field: a field name of the same structure, or a
    /// whole entry name for a field once named apart from its structure.
    pub aliases: Vec<String>,
    pub ext: BTreeMap<String, Value>,
}

/// A structure.
#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub name: String,
    pub doc: Option<String>,
    pub size: Option<u32>,
    pub align: Option<u32>,
    /// Bases, with their offsets.
    pub extends: Vec<(String, u32)>,
    pub vtable: Option<String>,
    /// The base's field and the values that make an object of the base one
    /// of this structure.
    pub variant: Option<(String, Vec<Value>)>,
    /// The field through which an object of this structure reads as text.
    pub text: Option<String>,
    pub fields: BTreeMap<String, StructField>,
    pub ext: BTreeMap<String, Value>,
}

/// An enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enum {
    pub name: String,
    pub doc: Option<String>,
    pub ty: Option<Primitive>,
    pub flags: bool,
    pub values: BTreeMap<String, u64>,
}

/// A type the description names.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeDef {
    Struct(Struct),
    Union {
        name: String,
        doc: Option<String>,
        fields: BTreeMap<String, Ty>,
    },
    Enum(Enum),
    Alias {
        name: String,
        doc: Option<String>,
        ty: Ty,
    },
    Template {
        name: String,
        doc: Option<String>,
        params: Vec<String>,
        size: Option<u32>,
        fields: BTreeMap<String, (Ty, u32)>,
        range: Option<(String, String)>,
    },
}

impl TypeDef {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Struct(Struct { name, .. })
            | Self::Enum(Enum { name, .. })
            | Self::Union { name, .. }
            | Self::Alias { name, .. }
            | Self::Template { name, .. } => name,
        }
    }
}

pub(crate) fn parse_thread(text: &str) -> Result<Thread, String> {
    match text {
        "main" => Ok(Thread::Main),
        "any" => Ok(Thread::Any),
        _ => Err(format!("`thread` is `main` or `any`, not `{text}`")),
    }
}

fn parse_signature(text: &str, key: &str) -> Result<Signature, String> {
    Signature::parse(text).map_err(|error| format!("`{key}`: {error}"))
}

pub(crate) fn parse_bytes(text: &str, key: &str) -> Result<Vec<u8>, String> {
    let bytes = text
        .split_whitespace()
        .map(|token| {
            (token.len() == 2 && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| u8::from_str_radix(token, 16).ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| format!("`{key}` is hexadecimal bytes separated by spaces"))?;
    if bytes.is_empty() {
        return Err(format!("`{key}` is empty"));
    }
    Ok(bytes)
}

/// Parses a `find` pipeline.
pub(crate) fn parse_steps(values: Vec<Value>) -> Result<Vec<Step>, String> {
    if values.is_empty() {
        return Err("`find` has at least one step".to_owned());
    }
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            parse_step(value).map_err(|message| format!("step {index}: {message}"))
        })
        .collect()
}

fn parse_step(value: Value) -> Result<Step, String> {
    let mut keys = Keys::new(value)?;
    let step = if let Some(text) = keys.string("scan")? {
        let data = match keys.string("section")?.as_deref() {
            None | Some("code") => false,
            Some("data") => true,
            Some(other) => return Err(format!("`section` is `code` or `data`, not `{other}`")),
        };
        Step::Scan {
            signature: parse_signature(&text, "scan")?,
            data,
        }
    } else if let Some(name) = keys.string("in")? {
        Step::In(name)
    } else if let Some(n) = keys.integer("offset")? {
        Step::Offset(n)
    } else if let Some(n) = keys.integer("rel32")? {
        Step::Rel32(n)
    } else if let Some(n) = keys.integer("rel8")? {
        Step::Rel8(n)
    } else if let Some(kind) = keys.string("operand")? {
        Step::Operand {
            kind: OperandKind::parse(&kind)?,
            index: keys.unsigned("index")?,
        }
    } else if let Some(kind) = keys.string("deref")? {
        if kind != "ptr" {
            return Err(format!("`deref` is `ptr`, not `{kind}`"));
        }
        Step::Deref
    } else if let Some(vtable) = keys.string("vtable")? {
        let slot = match keys.value("slot") {
            Some(Value::Integer(index)) => {
                Slot::Index(u32::try_from(index).map_err(|_| "`slot` is not negative".to_owned())?)
            }
            Some(Value::String(name)) => Slot::Name(name),
            _ => return Err("`slot` is a number or a slot's name".to_owned()),
        };
        Step::Vtable { vtable, slot }
    } else if let Some(name) = keys.string("entry")? {
        Step::Entry(name)
    } else if let Some(text) = keys.string("import")? {
        let (dll, function) = text
            .split_once('!')
            .filter(|(dll, function)| !dll.is_empty() && !function.is_empty())
            .ok_or_else(|| "`import` is `<module>!<name>`".to_owned())?;
        Step::Import {
            dll: dll.to_owned(),
            function: function.to_owned(),
        }
    } else {
        let kind = keys.first_key().unwrap_or("").to_owned();
        return Err(format!(
            "step kind `{kind}` is not one this reader of Anatomia {}.{} knows",
            crate::FORMAT_MAJOR,
            crate::FORMAT_MINOR
        ));
    };
    keys.finish()?;
    Ok(step)
}

pub(crate) fn parse_checks(values: Vec<Value>) -> Result<Vec<Check>, String> {
    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            parse_check(value).map_err(|message| format!("check {index}: {message}"))
        })
        .collect()
}

fn parse_check(value: Value) -> Result<Check, String> {
    let mut keys = Keys::new(value)?;
    let check = if let Some(text) = keys.string("bytes")? {
        Check::Bytes {
            bytes: parse_bytes(&text, "bytes")?,
            at: keys.integer("at")?.unwrap_or(0),
        }
    } else if let Some(text) = keys.string("contains")? {
        Check::Contains {
            signature: parse_signature(&text, "contains")?,
            function: keys.string("in")?,
        }
    } else if keys.has("find") {
        Check::Find(parse_steps(keys.array("find")?)?)
    } else if let Some(kind) = keys.string("operand")? {
        if kind != "imm" {
            return Err(format!("an operand check takes `imm`, not `{kind}`"));
        }
        Check::Immediate {
            at: keys.integer("at")?.unwrap_or(0),
            equals: keys
                .integer("equals")?
                .ok_or_else(|| "an operand check needs `equals`".to_owned())?,
        }
    } else if let Some(name) = keys.string("equals")? {
        Check::Equals(name)
    } else if let Some(slot) = keys.unsigned("vtable_slot")? {
        let code_slots = keys
            .unsigned("code_slots")?
            .ok_or_else(|| "`vtable_slot` needs `code_slots`".to_owned())?;
        if code_slots <= slot {
            return Err("`code_slots` must be greater than `vtable_slot`".to_owned());
        }
        Check::InVtable { slot, code_slots }
    } else {
        return Err(format!(
            "not a check kind this reader of Anatomia {}.{} knows",
            crate::FORMAT_MAJOR,
            crate::FORMAT_MINOR
        ));
    };
    keys.finish()?;
    Ok(check)
}

/// Reads the keys every entry may have: `doc`, `checks`, `depends`, and
/// `ext`.
pub(crate) struct Common {
    pub doc: Option<String>,
    pub checks: Vec<Check>,
    pub depends: Vec<String>,
    pub ext: BTreeMap<String, Value>,
}

pub(crate) fn common(keys: &mut Keys, extensions: &[String]) -> Result<Common, String> {
    Ok(Common {
        doc: keys.string("doc")?,
        checks: parse_checks(keys.array("checks")?)?,
        depends: keys.strings("depends")?,
        ext: extension_attributes(keys, extensions)?,
    })
}

pub(crate) fn extension_attributes(
    keys: &mut Keys,
    extensions: &[String],
) -> Result<BTreeMap<String, Value>, String> {
    let Some(table) = keys.table("ext")? else {
        return Ok(BTreeMap::new());
    };
    table
        .into_iter()
        .map(|(name, value)| {
            if extensions.contains(&name) {
                Ok((name, value))
            } else {
                Err(format!(
                    "extension `{name}` is not declared in anatomia.toml"
                ))
            }
        })
        .collect()
}
