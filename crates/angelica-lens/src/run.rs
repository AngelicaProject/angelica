//! Runs a plan through handles: every place in the plan is a field of
//! Anatomia, read with the core's checked reads, never an address.
use angelica_query::plan::{
    ArgAt, ArrayStep, Base, Check, ColumnMeaning, Condition, Count, Number, Prepared, Read, Start,
    Step, Stored,
};
use angelica_query::query::Op;
use angelica_sdk::api::intercepts::Value as CallValue;
use angelica_sdk::api::lens::{FlagSet, Row, SheetRow, Value};
use angelica_sdk::api::memory::{self, Value as Memory};
use angelica_sdk::{Error, ErrorKind};

/// The most rows a query answers.
pub const MOST_ROWS: usize = 10_000;
/// The most rows one firing of an event answers.
pub const MOST_CALL_ROWS: usize = 256;

/// A value as a plan compares and answers it.
#[derive(Debug, Clone, PartialEq)]
enum Raw {
    Boolean(bool),
    Integer(i128),
    Float(f64),
    Text(Vec<u8>),
}

/// Where the objects of a plan's path are: handles, or a call's arguments.
#[derive(Clone, Copy)]
enum Object {
    Handle(u64),
    /// The call itself, before the path leaves it.
    Call,
}

/// What a plan runs over.
pub struct Context<'a> {
    /// The globals the schema numbers in place of addresses.
    pub globals: &'a [String],
    /// The call's arguments, for a watch or a rule.
    pub call: Option<&'a [CallValue]>,
}

fn limit(most: usize) -> Error {
    Error {
        kind: ErrorKind::LimitExceeded,
        message: format!("the query answers more than {most} rows; narrow it with filters"),
    }
}

/// The position of the call's argument at `at`.
fn position(at: ArgAt) -> Option<usize> {
    match at {
        ArgAt::Integer(register) => [2u8, 3, 7, 8].iter().position(|r| *r == register),
        ArgAt::Float(position) => Some(usize::from(position)),
        ArgAt::Stack(offset) => usize::try_from(offset.checked_sub(8)? / 8).ok(),
    }
}

fn argument<'a>(context: &Context<'a>, at: ArgAt) -> Option<&'a CallValue> {
    context.call?.get(position(at)?)
}

/// The bits of a call's argument.
fn bits(value: &CallValue) -> Option<u64> {
    Some(match value {
        CallValue::Signed(number) => number.cast_unsigned(),
        CallValue::Unsigned(number) => *number,
        CallValue::Boolean(flag) => u64::from(*flag),
        CallValue::Float32(number) => u64::from(number.to_bits()),
        CallValue::Float64(number) => number.to_bits(),
        CallValue::None => 0,
        CallValue::Object(_) | CallValue::Text(_) | CallValue::Pointer(_) => return None,
    })
}

fn raw(value: Memory) -> Result<Option<Raw>, Error> {
    Ok(Some(match value {
        Memory::None | Memory::Array(_) | Memory::Pointer(_) => return Ok(None),
        Memory::Signed(number) => Raw::Integer(i128::from(number)),
        Memory::Unsigned(number) => Raw::Integer(i128::from(number)),
        Memory::Boolean(flag) => Raw::Boolean(flag),
        Memory::Float32(number) => Raw::Float(f64::from(number)),
        Memory::Float64(number) => Raw::Float(number),
        Memory::Text(text) => Raw::Text(text),
        Memory::Object(handle) => Raw::Text(memory::text(handle)?),
    }))
}

/// The handle a field of `object` points to or holds.
fn hop(object: u64, field: u32) -> Result<Option<u64>, Error> {
    Ok(match memory::read(object, field) {
        Ok(Memory::Object(handle)) => Some(handle),
        Ok(_) => None,
        Err(error) if error.kind == ErrorKind::Unavailable => None,
        Err(error) => return Err(error),
    })
}

/// The value `read` reads from `object`; none where it cannot be read.
fn read_value(context: &Context<'_>, object: Object, read: &Read) -> Result<Option<Raw>, Error> {
    if !read.guards.is_empty() && !passes(context, object, &read.guards)? {
        return Ok(None);
    }
    let start = match (read.base, object) {
        (Base::Value { at, mask }, _) => {
            let Some(value) = argument(context, at).and_then(bits) else {
                return Ok(None);
            };
            let masked = value & mask;
            return Ok(Some(match read.stored {
                Stored::Value(angelica_map::ValueKind::Bool) => Raw::Boolean(masked != 0),
                Stored::Value(angelica_map::ValueKind::F32 | angelica_map::ValueKind::F64) => {
                    match argument(context, at) {
                        Some(CallValue::Float32(number)) => Raw::Float(f64::from(*number)),
                        Some(CallValue::Float64(number)) => Raw::Float(*number),
                        _ => return Ok(None),
                    }
                }
                _ => match argument(context, at) {
                    Some(CallValue::Signed(number)) => Raw::Integer(i128::from(*number)),
                    _ => Raw::Integer(i128::from(masked)),
                },
            }));
        }
        (Base::Pointer(at), _) => match argument(context, at) {
            Some(CallValue::Object(handle) | CallValue::Text(handle)) => *handle,
            _ => return Ok(None),
        },
        (Base::Object, Object::Handle(handle)) => handle,
        (Base::Object, Object::Call) => return Ok(None),
    };
    let mut at = start;
    if let Stored::Text(_) = read.stored {
        return text(at, read);
    }
    for step in &read.hops {
        match hop(at, step.offset)? {
            Some(next) => at = next,
            None => return Ok(None),
        }
    }
    let value = match memory::read(at, read.offset) {
        Ok(value) => value,
        Err(error) if error.kind == ErrorKind::Unavailable => return Ok(None),
        Err(error) => return Err(error),
    };
    let value = raw(value)?;
    Ok(match (read.stored, value) {
        (Stored::Bit { index, .. }, Some(Raw::Integer(bits))) => {
            Some(Raw::Boolean(bits >> index & 1 != 0))
        }
        (Stored::Bit { index, .. }, Some(Raw::Boolean(flag))) => {
            Some(Raw::Boolean(index == 0 && flag))
        }
        (_, value) => value,
    })
}

/// Text the plan reads from `at`. Text behind a pointer is the last step
/// of the plan, whose field reads as text, or the object a call's text
/// argument points to.
fn text(at: u64, read: &Read) -> Result<Option<Raw>, Error> {
    let Some((last, before)) = read.hops.split_last() else {
        return match read.base {
            Base::Pointer(_) => Ok(Some(Raw::Text(memory::text(at)?))),
            _ => match memory::read(at, read.offset) {
                Ok(value) => raw(value),
                Err(error) if error.kind == ErrorKind::Unavailable => Ok(None),
                Err(error) => Err(error),
            },
        };
    };
    let mut at = at;
    for step in before {
        match hop(at, step.offset)? {
            Some(next) => at = next,
            None => return Ok(None),
        }
    }
    match memory::read(at, last.offset) {
        Ok(value) => raw(value),
        Err(error) if error.kind == ErrorKind::Unavailable => Ok(None),
        Err(error) => Err(error),
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

fn passes(context: &Context<'_>, object: Object, conditions: &[Condition]) -> Result<bool, Error> {
    for condition in conditions {
        let Some(value) = read_value(context, object, &condition.read)? else {
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

fn empty(value: Option<&Raw>) -> bool {
    match value {
        None | Some(Raw::Boolean(false) | Raw::Integer(0)) => true,
        Some(Raw::Float(value)) => *value == 0.0,
        Some(Raw::Text(text)) => text.is_empty(),
        Some(_) => false,
    }
}

/// The elements of an array of `object` that pass, with their positions.
fn elements(
    context: &Context<'_>,
    object: u64,
    array: &ArrayStep,
) -> Result<Vec<(u32, u64)>, Error> {
    let handle = match memory::read(object, array.offset) {
        Ok(Memory::Array(handle)) => handle,
        Ok(_) => return Ok(Vec::new()),
        Err(error) if error.kind == ErrorKind::Unavailable => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let length = memory::length(handle)?;
    let (Count::Fixed(most) | Count::Field { most, .. }) = &array.count;
    let used = length.min(u32::from(*most));
    let positions: Vec<u32> = match array.index {
        Some(index) if index < used => vec![index],
        Some(_) => Vec::new(),
        None => (0..used).collect(),
    };
    let mut out = Vec::new();
    for position in positions {
        let element = match memory::element(handle, position) {
            Ok(Memory::Object(element)) => element,
            Ok(_) => continue,
            // An element of a domain the plugin does not hold is left out,
            // as an element the game does not use.
            Err(error)
                if matches!(
                    error.kind,
                    ErrorKind::PermissionDenied | ErrorKind::Unavailable
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        };
        let at = Object::Handle(element);
        if let Some(present) = &array.present
            && empty(read_value(context, at, present)?.as_ref())
        {
            continue;
        }
        if passes(context, at, &array.conditions)? {
            out.push((position, element));
        }
    }
    Ok(out)
}

/// The objects a plan's path reaches from `start`, with their positions.
fn walk(
    context: &Context<'_>,
    prepared: &Prepared,
    start: Object,
    most: usize,
) -> Result<Vec<(Vec<u32>, Object)>, Error> {
    let mut objects = vec![(Vec::new(), start)];
    for step in &prepared.steps {
        let mut next = Vec::new();
        for (indexes, object) in objects {
            match (step, object) {
                (Step::FromArg(at), _) => {
                    if let Some(CallValue::Object(handle) | CallValue::Text(handle)) =
                        argument(context, *at)
                    {
                        next.push((indexes, Object::Handle(*handle)));
                    }
                }
                (Step::Hop(step), Object::Handle(handle)) => {
                    if let Some(target) = hop(handle, step.offset)? {
                        next.push((indexes, Object::Handle(target)));
                    }
                }
                (Step::Filter(conditions), object) => {
                    if passes(context, object, conditions)? {
                        next.push((indexes, object));
                    }
                }
                (Step::Array(array), Object::Handle(handle)) => {
                    for (position, element) in elements(context, handle, array)? {
                        let mut indexes = indexes.clone();
                        indexes.push(position);
                        next.push((indexes, Object::Handle(element)));
                        if next.len() > most {
                            return Err(limit(most));
                        }
                    }
                }
                (Step::Hop(_) | Step::Array(_), Object::Call) => {}
            }
        }
        objects = next;
    }
    if objects.len() > most {
        return Err(limit(most));
    }
    Ok(objects)
}

fn answer(raw: Raw, signed: bool, meaning: &ColumnMeaning) -> Value {
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
        (Raw::Integer(value), ColumnMeaning::Plain) if signed => {
            Value::Signed(i64::try_from(value).unwrap_or_default())
        }
        (Raw::Integer(value), ColumnMeaning::Sheet(sheet)) => match u32::try_from(value) {
            Ok(row) => Value::Sheet(SheetRow {
                sheet: sheet.clone(),
                row,
            }),
            Err(_) => integer(value),
        },
        (Raw::Integer(value), ColumnMeaning::Enum(names)) => u64::try_from(value)
            .ok()
            .and_then(|key| names.get(&key))
            .map_or_else(|| integer(value), |name| Value::Named(name.clone())),
        (Raw::Integer(value), ColumnMeaning::Flags(names)) => {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let bits = value as u64;
            Value::Flags(FlagSet {
                names: names
                    .iter()
                    .filter(|(bit, _)| bits & **bit != 0)
                    .map(|(_, name)| name.clone())
                    .collect(),
                bits,
            })
        }
        (Raw::Integer(value), ColumnMeaning::Plain) => integer(value),
    }
}

fn signed(stored: Stored) -> bool {
    use angelica_map::ValueKind;
    matches!(
        stored,
        Stored::Value(ValueKind::I8 | ValueKind::I16 | ValueKind::I32 | ValueKind::I64)
    )
}

/// Runs a plan: from its root for a query, from the call in `context` for
/// a watch or a rule.
///
/// # Errors
/// Past the limits of rows, and what the core refuses.
pub fn rows(context: &Context<'_>, prepared: &Prepared) -> Result<Vec<Row>, Error> {
    let (start, most) = match &prepared.start {
        Start::Root(root) => {
            let Some(global) = usize::try_from(root.address)
                .ok()
                .and_then(|index| context.globals.get(index))
            else {
                return Ok(Vec::new());
            };
            match memory::global(global)? {
                Memory::Object(handle) => (Object::Handle(handle), MOST_ROWS),
                _ => return Ok(Vec::new()),
            }
        }
        Start::Event(_) => (Object::Call, MOST_CALL_ROWS),
    };
    walk(context, prepared, start, most)?
        .into_iter()
        .map(|(indexes, object)| {
            let values = prepared
                .columns
                .iter()
                .map(|column| {
                    Ok(read_value(context, object, &column.read)?
                        .map(|raw| answer(raw, signed(column.stored), &column.meaning)))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            Ok(Row { indexes, values })
        })
        .collect()
}
