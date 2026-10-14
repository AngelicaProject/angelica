//! Running a prepared query: reads of the game's memory along the plan,
//! which fail instead of faulting. Memory is read a page at a time, each
//! page once per run, so the values of one run come from one moment of one
//! frame.
#![forbid(unsafe_code)]
use super::plan::{
    ArgAt, ArrayStep, Base, Check, ColumnMeaning, Condition, Count, Hop, Number, Prepared, Read,
    Start, Step, Stored,
};
use super::query::Op;
use crate::error::{Error, Kind, Result};
use angelica_map::ValueKind;
use std::cell::RefCell;
use std::collections::HashMap;

/// The most rows one run answers.
pub const MOST_ROWS: usize = 4096;
/// The most pages of memory one run reads.
pub const MOST_PAGES: usize = 4096;
/// The most rows one firing of an event answers to a watch.
pub const MOST_CALL_ROWS: usize = 256;
/// The most pages of memory one firing of an event reads for a watch.
pub const MOST_CALL_PAGES: usize = 64;

/// The registers of a call when an event fires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Call {
    /// `rax`, `rbx`, `rcx`, `rdx`, `rsi`, `rdi`, `rbp`, `r8` to `r15`.
    pub general: [u64; 15],
    /// The low 64 bits of `xmm0` to `xmm15`.
    pub xmm: [u64; 16],
    /// The stack pointer at the event's instruction.
    pub rsp: u64,
}
const PAGE: usize = 4096;
const PAGE_BYTES: u64 = 4096;

/// Reads of the game's memory that fail instead of faulting.
pub trait Memory {
    /// Fills `buffer` from `address`; false when the range is not readable.
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool;
}

/// A value of a row.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Boolean(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    /// Text as the game stores it, without its terminator.
    Text(Vec<u8>),
    /// A row of a sheet of the game.
    Row {
        sheet: String,
        row: u32,
    },
    /// The name of an enum's value.
    Named(String),
    /// The names of the set bits that have one, and every bit.
    Flags {
        names: Vec<String>,
        bits: u64,
    },
}

/// One answer: the positions in the arrays the path went through, and a
/// value per column, `None` where a pointer on its way is zero or the
/// memory is unreadable.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub indexes: Vec<u32>,
    pub values: Vec<Option<Value>>,
}

/// Memory read by page, each page at most once.
struct Pages<'a> {
    memory: &'a dyn Memory,
    loaded: RefCell<HashMap<u64, Option<Box<[u8; PAGE]>>>>,
    most: usize,
    /// The event's call, for a watch.
    call: Option<&'a Call>,
}

impl Pages<'_> {
    fn read(&self, address: u64, buffer: &mut [u8]) -> Result<bool> {
        let mut done = 0;
        while done < buffer.len() {
            let Some(at) = address.checked_add(done as u64) else {
                return Ok(false);
            };
            let page = at / PAGE_BYTES * PAGE_BYTES;
            let start = usize::try_from(at - page).unwrap_or_default();
            let length = (PAGE - start).min(buffer.len() - done);
            let mut pages = self.loaded.borrow_mut();
            if !pages.contains_key(&page) {
                if pages.len() >= self.most {
                    return Err(Error::new(
                        Kind::LimitExceeded,
                        format!(
                            "the query reads more than {} pages of the game's memory; narrow it with filters",
                            self.most
                        ),
                    ));
                }
                let mut bytes = Box::new([0u8; PAGE]);
                let readable = self.memory.read(page, &mut bytes[..]);
                pages.insert(page, readable.then_some(bytes));
            }
            let Some(bytes) = &pages[&page] else {
                return Ok(false);
            };
            buffer[done..done + length].copy_from_slice(&bytes[start..start + length]);
            done += length;
        }
        Ok(true)
    }

    /// The 64 bits of an argument of the call.
    fn argument(&self, at: ArgAt) -> Result<Option<u64>> {
        let Some(call) = self.call else {
            return Ok(None);
        };
        Ok(match at {
            ArgAt::Integer(index) => call.general.get(usize::from(index)).copied(),
            ArgAt::Float(index) => call.xmm.get(usize::from(index)).copied(),
            ArgAt::Stack(offset) => {
                let mut bytes = [0u8; 8];
                call.rsp
                    .checked_add(u64::from(offset))
                    .map_or(Ok(false), |address| self.read(address, &mut bytes))?
                    .then(|| u64::from_le_bytes(bytes))
            }
        })
    }

    fn pointer(&self, address: u64) -> Result<Option<u64>> {
        let mut bytes = [0u8; 8];
        Ok(self
            .read(address, &mut bytes)?
            .then(|| u64::from_le_bytes(bytes))
            .filter(|&value| value != 0))
    }
}

/// A value as read, before its meaning.
#[derive(Debug, Clone, PartialEq)]
enum Raw {
    Boolean(bool),
    Integer(i128),
    Float(f64),
    Text(Vec<u8>),
}

fn hops(pages: &Pages<'_>, start: u64, hops: &[Hop]) -> Result<Option<u64>> {
    let mut address = start;
    for hop in hops {
        let Some(at) = address.checked_add(u64::from(hop.offset)) else {
            return Ok(None);
        };
        address = if hop.pointer {
            match pages.pointer(at)? {
                Some(target) => target,
                None => return Ok(None),
            }
        } else {
            at
        };
    }
    Ok(Some(address))
}

/// A value of a kind from its bits as a register or memory holds them.
fn from_bits(kind: ValueKind, bits: u64) -> Option<Raw> {
    let bytes = bits.to_le_bytes();
    Some(match kind {
        ValueKind::Bool => Raw::Boolean(bytes[0] != 0),
        ValueKind::U8 => Raw::Integer(i128::from(bytes[0])),
        ValueKind::U16 => Raw::Integer(i128::from(u16::from_le_bytes([bytes[0], bytes[1]]))),
        ValueKind::U32 => Raw::Integer(i128::from(u32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))),
        ValueKind::U64 | ValueKind::Ptr => Raw::Integer(i128::from(bits)),
        ValueKind::I8 => Raw::Integer(i128::from(i8::from_le_bytes([bytes[0]]))),
        ValueKind::I16 => Raw::Integer(i128::from(i16::from_le_bytes([bytes[0], bytes[1]]))),
        ValueKind::I32 => Raw::Integer(i128::from(i32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))),
        ValueKind::I64 => Raw::Integer(i128::from(i64::from_le_bytes(bytes))),
        ValueKind::F32 => Raw::Float(f64::from(f32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))),
        ValueKind::F64 => Raw::Float(f64::from_le_bytes(bytes)),
        ValueKind::Struct | ValueKind::Void => return None,
    })
}

fn raw(pages: &Pages<'_>, object: u64, read: &Read) -> Result<Option<Raw>> {
    let object = match read.base {
        Base::Object => object,
        Base::Value(at) => {
            let Stored::Value(kind) = read.stored else {
                return Ok(None);
            };
            return Ok(pages.argument(at)?.and_then(|bits| from_bits(kind, bits)));
        }
        Base::Pointer(at) => match pages.argument(at)? {
            Some(pointer) if pointer != 0 => pointer,
            _ => return Ok(None),
        },
    };
    let Some(address) = hops(pages, object, &read.hops)?
        .and_then(|address| address.checked_add(u64::from(read.offset)))
    else {
        return Ok(None);
    };
    let fixed = |length: usize| -> Result<Option<[u8; 8]>> {
        let mut bytes = [0u8; 8];
        Ok(pages.read(address, &mut bytes[..length])?.then_some(bytes))
    };
    let kind = match read.stored {
        Stored::Text(length) => {
            let mut bytes = vec![0u8; usize::from(length)];
            if !pages.read(address, &mut bytes)? {
                return Ok(None);
            }
            if let Some(end) = bytes.iter().position(|&byte| byte == 0) {
                bytes.truncate(end);
            }
            return Ok(Some(Raw::Text(bytes)));
        }
        Stored::Value(kind) => kind,
    };
    let size = match kind {
        ValueKind::Bool | ValueKind::U8 | ValueKind::I8 => 1,
        ValueKind::U16 | ValueKind::I16 => 2,
        ValueKind::U32 | ValueKind::I32 | ValueKind::F32 => 4,
        ValueKind::U64 | ValueKind::I64 | ValueKind::F64 => 8,
        ValueKind::Ptr | ValueKind::Struct | ValueKind::Void => return Ok(None),
    };
    let Some(bytes) = fixed(size)? else {
        return Ok(None);
    };
    let unsigned = u64::from_le_bytes(bytes);
    Ok(Some(match kind {
        ValueKind::Bool => Raw::Boolean(bytes[0] != 0),
        ValueKind::U8 | ValueKind::U16 | ValueKind::U32 | ValueKind::U64 => {
            Raw::Integer(i128::from(unsigned))
        }
        ValueKind::I8 => Raw::Integer(i128::from(i8::from_le_bytes([bytes[0]]))),
        ValueKind::I16 => Raw::Integer(i128::from(i16::from_le_bytes([bytes[0], bytes[1]]))),
        ValueKind::I32 => Raw::Integer(i128::from(i32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))),
        ValueKind::I64 => Raw::Integer(i128::from(i64::from_le_bytes(bytes))),
        ValueKind::F32 => Raw::Float(f64::from(f32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ]))),
        ValueKind::F64 => Raw::Float(f64::from_le_bytes(bytes)),
        ValueKind::Ptr | ValueKind::Struct | ValueKind::Void => unreachable!("returned above"),
    }))
}

/// Whether a value is zero, false, empty, or missing.
fn empty(value: Option<&Raw>) -> bool {
    match value {
        None | Some(Raw::Boolean(false) | Raw::Integer(0)) => true,
        Some(Raw::Float(value)) => *value == 0.0,
        Some(Raw::Text(text)) => text.is_empty(),
        Some(_) => false,
    }
}

fn compare(value: &Raw, number: Number) -> Option<std::cmp::Ordering> {
    match (value, number) {
        (Raw::Integer(value), Number::Integer(number)) => Some(value.cmp(&number)),
        (Raw::Boolean(value), Number::Integer(number)) => Some(i128::from(*value).cmp(&number)),
        #[allow(clippy::cast_precision_loss)]
        (Raw::Integer(value), Number::Float(number)) => (*value as f64).partial_cmp(&number),
        (Raw::Float(value), Number::Float(number)) => value.partial_cmp(&number),
        #[allow(clippy::cast_precision_loss)]
        (Raw::Float(value), Number::Integer(number)) => value.partial_cmp(&(number as f64)),
        _ => None,
    }
}

fn passes(pages: &Pages<'_>, object: u64, conditions: &[Condition]) -> Result<bool> {
    for condition in conditions {
        // A value that cannot be read meets no condition.
        let Some(value) = raw(pages, object, &condition.read)? else {
            return Ok(false);
        };
        let met = match &condition.check {
            Check::Number(op, number) => compare(&value, *number).is_some_and(|order| match op {
                Op::Equal => order.is_eq(),
                Op::NotEqual => order.is_ne(),
                Op::Less => order.is_lt(),
                Op::LessOrEqual => order.is_le(),
                Op::Greater => order.is_gt(),
                Op::GreaterOrEqual => order.is_ge(),
            }),
            Check::NumberIn(numbers) => numbers
                .iter()
                .any(|number| compare(&value, *number).is_some_and(std::cmp::Ordering::is_eq)),
            Check::Text(equal, texts) => match &value {
                Raw::Text(text) => texts.iter().any(|candidate| candidate == text) == *equal,
                _ => false,
            },
        };
        if !met {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The elements of an array of the object at `object` that pass, with
/// their positions.
fn elements(pages: &Pages<'_>, object: u64, array: &ArrayStep) -> Result<Vec<(u32, u64)>> {
    let Some(mut base) = object.checked_add(u64::from(array.offset)) else {
        return Ok(Vec::new());
    };
    if array.behind_pointer {
        match pages.pointer(base)? {
            Some(target) => base = target,
            None => return Ok(Vec::new()),
        }
    }
    let used = match &array.count {
        Count::Fixed(length) => u32::from(*length),
        Count::Field { read, most } => match raw(pages, object, read)? {
            Some(Raw::Integer(count)) => {
                u32::try_from(count.clamp(0, i128::from(*most))).unwrap_or_default()
            }
            _ => 0,
        },
    };
    let positions: Box<dyn Iterator<Item = u32>> = match array.index {
        Some(index) if index < used => Box::new(std::iter::once(index)),
        Some(_) => Box::new(std::iter::empty()),
        None => Box::new(0..used),
    };
    let mut out = Vec::new();
    for position in positions {
        let Some(mut element) = base.checked_add(u64::from(array.stride) * u64::from(position))
        else {
            break;
        };
        if array.pointers {
            match pages.pointer(element)? {
                Some(target) => element = target,
                None => continue,
            }
        }
        if let Some(present) = &array.present
            && empty(raw(pages, element, present)?.as_ref())
        {
            continue;
        }
        if passes(pages, element, &array.conditions)? {
            out.push((position, element));
        }
    }
    Ok(out)
}

fn value(raw: Raw, meaning: &ColumnMeaning) -> Value {
    let integer = |value: i128| {
        u64::try_from(value).map_or_else(
            |_| Value::Signed(i64::try_from(value).unwrap_or_default()),
            Value::Unsigned,
        )
    };
    match (raw, meaning) {
        (Raw::Boolean(value), _) => Value::Boolean(value),
        (Raw::Float(value), _) => Value::Float(value),
        (Raw::Text(text), _) => Value::Text(text),
        (Raw::Integer(value), ColumnMeaning::Sheet(sheet)) => match u32::try_from(value) {
            Ok(row) => Value::Row {
                sheet: sheet.clone(),
                row,
            },
            Err(_) => integer(value),
        },
        (Raw::Integer(value), ColumnMeaning::Enum(names)) => u64::try_from(value)
            .ok()
            .and_then(|key| names.get(&key))
            .map_or_else(|| integer(value), |name| Value::Named(name.clone())),
        (Raw::Integer(value), ColumnMeaning::Flags(names)) => {
            // Signed flags keep their bits.
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let bits = value as u64;
            Value::Flags {
                names: names
                    .iter()
                    .filter(|(bit, _)| bits & **bit != 0)
                    .map(|(_, name)| name.clone())
                    .collect(),
                bits,
            }
        }
        (Raw::Integer(value), ColumnMeaning::Plain) => {
            if let Ok(value) = u64::try_from(value) {
                Value::Unsigned(value)
            } else {
                Value::Signed(i64::try_from(value).unwrap_or_default())
            }
        }
    }
}

/// The signed kinds answer `signed` even for values that are not negative.
fn signed(stored: Stored) -> bool {
    matches!(
        stored,
        Stored::Value(ValueKind::I8 | ValueKind::I16 | ValueKind::I32 | ValueKind::I64)
    )
}

/// Runs a prepared query over `memory`.
///
/// # Errors
/// `limit-exceeded` when the query answers more than [`MOST_ROWS`] rows or
/// reads more than [`MOST_PAGES`] pages.
pub fn run(memory: &dyn Memory, prepared: &Prepared) -> Result<Vec<Row>> {
    let pages = Pages {
        memory,
        loaded: RefCell::new(HashMap::new()),
        most: MOST_PAGES,
        call: None,
    };
    let Start::Root(root) = &prepared.start else {
        return Ok(Vec::new());
    };
    let start = if root.pointer {
        match pages.pointer(root.address)? {
            Some(object) => object,
            None => return Ok(Vec::new()),
        }
    } else {
        root.address
    };
    rows(&pages, prepared, start, MOST_ROWS)
}

/// Runs a watch's plan for one firing of its event, with the call's
/// registers; at most [`MOST_CALL_ROWS`] rows from [`MOST_CALL_PAGES`]
/// pages.
///
/// # Errors
/// `limit-exceeded` past those limits.
pub fn run_call(memory: &dyn Memory, prepared: &Prepared, call: &Call) -> Result<Vec<Row>> {
    let pages = Pages {
        memory,
        loaded: RefCell::new(HashMap::new()),
        most: MOST_CALL_PAGES,
        call: Some(call),
    };
    rows(&pages, prepared, 0, MOST_CALL_ROWS)
}

/// The objects a plan's path reaches from `start`, with their positions.
fn walk(
    pages: &Pages<'_>,
    prepared: &Prepared,
    start: u64,
    most: usize,
) -> Result<Vec<(Vec<u32>, u64)>> {
    let too_many = || {
        Error::new(
            Kind::LimitExceeded,
            format!("the query answers more than {most} rows; narrow it with filters"),
        )
    };
    let mut objects = vec![(Vec::new(), start)];
    for step in &prepared.steps {
        let mut next = Vec::new();
        for (indexes, object) in objects {
            match step {
                Step::FromArg(at) => {
                    if let Some(pointer) = pages.argument(*at)?.filter(|&pointer| pointer != 0) {
                        next.push((indexes, pointer));
                    }
                }
                Step::Hop(hop) => {
                    if let Some(target) = hops(pages, object, std::slice::from_ref(hop))? {
                        next.push((indexes, target));
                    }
                }
                Step::Filter(conditions) => {
                    if passes(pages, object, conditions)? {
                        next.push((indexes, object));
                    }
                }
                Step::Array(array) => {
                    for (position, element) in elements(pages, object, array)? {
                        let mut indexes = indexes.clone();
                        indexes.push(position);
                        next.push((indexes, element));
                        if next.len() > most {
                            return Err(too_many());
                        }
                    }
                }
            }
        }
        objects = next;
    }
    if objects.len() > most {
        return Err(too_many());
    }
    Ok(objects)
}

/// An object a path reached: its positions, its address, and its bytes.
pub type Dumped = (Vec<u32>, u64, Option<Vec<u8>>);

/// The addresses of the objects a plan's path reaches, and `bytes` bytes
/// of each, `None` where they cannot be read: for looking at the game's
/// memory while describing a type, never for plugins.
///
/// # Errors
/// As for [`run`].
pub fn objects(memory: &dyn Memory, prepared: &Prepared, bytes: usize) -> Result<Vec<Dumped>> {
    let pages = Pages {
        memory,
        loaded: RefCell::new(HashMap::new()),
        most: MOST_PAGES,
        call: None,
    };
    let Start::Root(root) = &prepared.start else {
        return Ok(Vec::new());
    };
    let start = if root.pointer {
        match pages.pointer(root.address)? {
            Some(object) => object,
            None => return Ok(Vec::new()),
        }
    } else {
        root.address
    };
    walk(&pages, prepared, start, MOST_ROWS)?
        .into_iter()
        .map(|(indexes, address)| {
            let mut buffer = vec![0u8; bytes];
            let read = pages.read(address, &mut buffer)?;
            Ok((indexes, address, read.then_some(buffer)))
        })
        .collect()
}

fn rows(pages: &Pages<'_>, prepared: &Prepared, start: u64, most: usize) -> Result<Vec<Row>> {
    walk(pages, prepared, start, most)?
        .into_iter()
        .map(|(indexes, object)| {
            let values = prepared
                .columns
                .iter()
                .map(|column| {
                    Ok(raw(pages, object, &column.read)?.map(|raw| {
                        match (raw, signed(column.stored), &column.meaning) {
                            (Raw::Integer(value), true, ColumnMeaning::Plain) => {
                                Value::Signed(i64::try_from(value).unwrap_or_default())
                            }
                            (raw, _, meaning) => value(raw, meaning),
                        }
                    }))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Row { indexes, values })
        })
        .collect()
}
