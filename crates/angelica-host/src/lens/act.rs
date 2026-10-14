//! Actions: plugins call functions of the game for the player as the game
//! map's actions describe them. [`prepare`] checks the action's name, the
//! plugin's domain, and every parameter against the map; [`Acting::take`]
//! checks that the player has not stopped acting, the action's rate and its
//! conditions, then makes the call through the game. Platform independent:
//! the Windows adapter only makes the call.
#![forbid(unsafe_code)]
use super::plan::{Prepared, prepare_objects};
use super::read::{Memory, Value, objects};
use super::schema::{RootAt, Schema};
use crate::error::{Error, Kind, Result};
use angelica_map::{ActSource, Meaning, Number, Param, ValueKind};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// The most arguments a function of an action has.
pub const MOST_ARGS: usize = 16;
/// How long the screen shows a plugin acting after its last action.
pub const SHOWN_FOR: Duration = Duration::from_secs(3);

/// Where an action's function and globals are in the running game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAt {
    pub function: u64,
    /// The function's return kind.
    pub ret: ValueKind,
    /// The places of the globals its arguments name.
    pub globals: BTreeMap<String, RootAt>,
}

/// One argument of a call, before the moment of the call.
#[derive(Debug, Clone, PartialEq)]
enum Slot {
    /// The 64 bits passed: an integer sign- or zero-extended, a float's bits
    /// in the low bits.
    Bits(u64),
    /// A global's address, or the pointer it holds.
    Global(RootAt),
    /// The first object the plan reaches.
    Object(Prepared),
}

/// A condition planned for the running build.
#[derive(Debug, Clone, PartialEq)]
struct Planned {
    plan: Prepared,
    says: String,
}

/// An action checked against the map, ready to be taken.
#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    pub action: String,
    pub function: u64,
    slots: Vec<Slot>,
    /// The function's return kind.
    pub ret: ValueKind,
    ok: Option<Number>,
    when: Vec<Planned>,
    rate: u8,
}

/// What the game's function returned: `rax` and the low bits of `xmm0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Returned {
    pub integer: u64,
    pub float: u64,
}

/// Checks a plugin's call of `name` with `given` parameters against the
/// schema. `may_act` says whether the plugin may act in a domain.
///
/// # Errors
/// `not-found` for an unknown action, `permission-denied` for a domain the
/// plugin may not act in, `invalid-argument` for a parameter that does not
/// fit, `unavailable` when the running build lacks what the action needs.
pub fn prepare(
    schema: &Schema,
    name: &str,
    given: &[(String, Value)],
    may_act: &dyn Fn(&str) -> bool,
) -> Result<Ready> {
    let Some(action) = schema.objects.actions.get(name) else {
        let known = schema
            .objects
            .actions
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let mut message = format!("the game map has no action {name}");
        if !known.is_empty() {
            message.push_str("; its actions: ");
            message.push_str(&known);
        }
        return Err(Error::new(Kind::NotFound, message));
    };
    if !may_act(&action.domain) {
        return Err(Error::new(
            Kind::PermissionDenied,
            format!(
                "action {name} acts in domain {}: list it in the manifest's [lens] act",
                action.domain
            ),
        ));
    }
    let at = schema
        .actions
        .get(name)
        .cloned()
        .unwrap_or_else(|| Err("not in the schema".into()))
        .map_err(|reason| {
            Error::new(
                Kind::Unavailable,
                format!("action {name} is not available in this game build: {reason}"),
            )
        })?;
    let seen = given_params(action, given)?;
    let unavailable = |what: &str, error: Error| {
        Error::new(
            Kind::Unavailable,
            format!("action {name}: {what}: {}", error.message),
        )
    };
    let mut slots = Vec::new();
    for arg in &action.args {
        let slot = match &arg.source {
            ActSource::Value(number) => Slot::Bits(bits(arg.kind, *number)),
            ActSource::Global(global) => Slot::Global(at.globals[global]),
            ActSource::Object(path) => Slot::Object(
                prepare_objects(schema, path).map_err(|error| unavailable(path, error))?,
            ),
            ActSource::Param(param) => {
                let value = match (seen.get(arg.name.as_str()), param.default) {
                    (Some(value), _) => encode(schema, &arg.name, param, value)?,
                    (None, Some(default)) => default,
                    (None, None) => {
                        return Err(Error::invalid(format!(
                            "action {name} needs parameter {}: {}",
                            arg.name, param.doc
                        )));
                    }
                };
                Slot::Bits(bits(param.kind, value))
            }
        };
        slots.push(slot);
    }
    let when = action
        .when
        .iter()
        .map(|condition| {
            Ok(Planned {
                plan: prepare_objects(schema, &condition.path)
                    .map_err(|error| unavailable(&condition.path, error))?,
                says: condition.says.clone(),
            })
        })
        .collect::<Result<_>>()?;
    Ok(Ready {
        action: name.to_owned(),
        function: at.function,
        slots,
        ret: at.ret,
        ok: action.ok,
        when,
        rate: action.rate,
    })
}

/// The parameters a plugin gave, by name, each a parameter of the action
/// and given once.
fn given_params<'g>(
    action: &angelica_map::Action,
    given: &'g [(String, Value)],
) -> Result<BTreeMap<&'g str, &'g Value>> {
    let mut seen = BTreeMap::new();
    for (key, value) in given {
        if !action.params().any(|(param, _)| param == key) {
            let known = action
                .params()
                .map(|(param, _)| param)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Error::invalid(format!(
                "action {} has no parameter {key}; its parameters: {known}",
                action.name
            )));
        }
        if seen.insert(key.as_str(), value).is_some() {
            return Err(Error::invalid(format!("parameter {key} is given twice")));
        }
    }
    Ok(seen)
}

/// A parameter's value as a number of its kind, checked.
fn encode(schema: &Schema, name: &str, param: &Param, value: &Value) -> Result<Number> {
    let wrong = |what: &str| Error::invalid(format!("parameter {name} is {what}: {}", param.doc));
    let number = match (&param.meaning, value) {
        (Meaning::Enum(enum_name), Value::Named(value_name)) => {
            let values = &schema.objects.enums[enum_name].values;
            let Some(found) = values.get(value_name) else {
                let known = values.keys().cloned().collect::<Vec<_>>().join(", ");
                return Err(Error::invalid(format!(
                    "parameter {name} has no value {value_name}; its values: {known}"
                )));
            };
            integer(*found)
        }
        (Meaning::Flags(enum_name), Value::Flags { names, bits }) => {
            let values = &schema.objects.enums[enum_name].values;
            let mut all = *bits;
            for flag in names {
                all |= values.get(flag).copied().ok_or_else(|| {
                    Error::invalid(format!("parameter {name} has no flag {flag}"))
                })?;
            }
            integer(all)
        }
        (Meaning::Sheet(sheet), Value::Row { sheet: given, row }) => {
            if given != sheet {
                return Err(wrong(&format!("a row of {sheet}, not of {given}")));
            }
            Number::Integer(i64::from(*row))
        }
        (_, Value::Signed(value)) => Number::Integer(*value),
        (_, Value::Unsigned(value)) => integer(*value),
        (_, Value::Boolean(value)) if param.kind == ValueKind::Bool => {
            Number::Integer(i64::from(*value))
        }
        (_, Value::Float(value)) if matches!(param.kind, ValueKind::F32 | ValueKind::F64) => {
            Number::Float(value.to_bits())
        }
        _ => return Err(wrong("of another kind")),
    };
    let float = matches!(param.kind, ValueKind::F32 | ValueKind::F64);
    let number = match number {
        Number::Integer(value) if float => {
            #[allow(clippy::cast_precision_loss)] // a float parameter given as an integer
            Number::Float((value as f64).to_bits())
        }
        other => other,
    };
    if let Number::Integer(value) = number
        && !fits(param.kind, value)
    {
        return Err(wrong(
            &format!("{value}, out of the range of a {:?}", param.kind).to_lowercase(),
        ));
    }
    if let Some((least, most)) = param.range
        && !(least.float()..=most.float()).contains(&number.float())
    {
        return Err(Error::invalid(format!(
            "parameter {name} is {}, outside {} to {}",
            number.float(),
            least.float(),
            most.float()
        )));
    }
    Ok(number)
}

fn integer(value: u64) -> Number {
    Number::Integer(i64::try_from(value).unwrap_or(i64::MAX))
}

/// Whether an integer fits a value kind.
fn fits(kind: ValueKind, value: i64) -> bool {
    match kind {
        ValueKind::I8 => i8::try_from(value).is_ok(),
        ValueKind::I16 => i16::try_from(value).is_ok(),
        ValueKind::I32 => i32::try_from(value).is_ok(),
        ValueKind::U8 | ValueKind::Bool => u8::try_from(value).is_ok(),
        ValueKind::U16 => u16::try_from(value).is_ok(),
        ValueKind::U32 => u32::try_from(value).is_ok(),
        ValueKind::U64 => value >= 0,
        _ => true,
    }
}

/// The 64 bits a number is passed in for an argument of `kind`.
fn bits(kind: ValueKind, number: Number) -> u64 {
    match (kind, number) {
        #[allow(clippy::cast_possible_truncation)] // an f32 argument
        (ValueKind::F32, number) => u64::from((number.float() as f32).to_bits()),
        (ValueKind::F64, number) => number.float().to_bits(),
        (_, Number::Integer(value)) => value.cast_unsigned(),
        (_, Number::Float(bits)) => bits,
    }
}

/// The game, as an action needs it: its memory and its functions.
pub trait Caller {
    fn memory(&self) -> &dyn Memory;

    /// Calls the function at `address` with `args`, on the game's main
    /// thread, as the game's own code calls it.
    ///
    /// # Errors
    /// Why the call cannot be made.
    fn call(&self, address: u64, args: &[u64]) -> std::result::Result<Returned, String>;
}

/// Whether plugins may act, how often each acted, and who acted last.
#[derive(Default)]
pub struct Acting {
    stopped: AtomicBool,
    /// When each plugin took each action in the last second.
    taken: Mutex<HashMap<(String, String), VecDeque<Instant>>>,
    /// The plugin that acted last, and when.
    last: Mutex<Option<(String, Instant)>>,
}

impl Acting {
    /// Stops every plugin's acting, or lets them act again.
    pub fn set_stopped(&self, stopped: bool) {
        self.stopped.store(stopped, Ordering::Release);
    }

    #[must_use]
    pub fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// The plugin that acted within [`SHOWN_FOR`] of `now`, for the screen.
    #[must_use]
    pub fn acting(&self, now: Instant) -> Option<String> {
        self.last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .filter(|(_, when)| now.saturating_duration_since(*when) < SHOWN_FOR)
            .map(|(plugin, _)| plugin.clone())
    }

    /// Forgets a stopped plugin's rate.
    pub fn remove_plugin(&self, plugin: &str) {
        self.taken
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(owner, _), _| owner != plugin);
    }

    /// Takes a ready action for `plugin` at `now`.
    ///
    /// # Errors
    /// `permission-denied` while the player stopped acting; `limit-exceeded`
    /// past the action's rate; `unavailable` when a condition fails, an
    /// object an argument needs does not exist, or the game refuses.
    pub fn take(&self, plugin: &str, ready: &Ready, game: &dyn Caller, now: Instant) -> Result<()> {
        if self.stopped() {
            return Err(Error::new(
                Kind::PermissionDenied,
                "the player stopped plugins from acting",
            ));
        }
        {
            let mut taken = self.taken.lock().unwrap_or_else(PoisonError::into_inner);
            let times = taken
                .entry((plugin.to_owned(), ready.action.clone()))
                .or_default();
            while times
                .front()
                .is_some_and(|when| now.saturating_duration_since(*when) >= Duration::from_secs(1))
            {
                times.pop_front();
            }
            if times.len() >= usize::from(ready.rate) {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!(
                        "action {} is at its rate of {} a second",
                        ready.action, ready.rate
                    ),
                ));
            }
            times.push_back(now);
        }
        let memory = game.memory();
        for condition in &ready.when {
            if objects(memory, &condition.plan, 0)?.is_empty() {
                return Err(Error::new(
                    Kind::Unavailable,
                    format!("action {}: {}", ready.action, condition.says),
                ));
            }
        }
        let mut args = Vec::with_capacity(ready.slots.len());
        for slot in &ready.slots {
            args.push(match slot {
                Slot::Bits(bits) => *bits,
                Slot::Global(at) if !at.pointer => at.address,
                Slot::Global(at) => {
                    let mut bytes = [0u8; 8];
                    if !memory.read(at.address, &mut bytes) || bytes == [0; 8] {
                        return Err(Error::new(
                            Kind::Unavailable,
                            format!(
                                "action {}: the game has not made what it acts on yet",
                                ready.action
                            ),
                        ));
                    }
                    u64::from_le_bytes(bytes)
                }
                Slot::Object(plan) => match objects(memory, plan, 0)?.first() {
                    Some((_, address, _)) => *address,
                    None => {
                        return Err(Error::new(
                            Kind::Unavailable,
                            format!(
                                "action {}: the game has not made what it acts on yet",
                                ready.action
                            ),
                        ));
                    }
                },
            });
        }
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Some((plugin.to_owned(), now));
        let returned = game.call(ready.function, &args).map_err(|reason| {
            Error::new(
                Kind::Unavailable,
                format!("action {}: {reason}", ready.action),
            )
        })?;
        if let Some(ok) = ready.ok
            && !returned_ok(ready.ret, returned, ok)
        {
            return Err(Error::new(
                Kind::Unavailable,
                format!("action {}: the game refused it now", ready.action),
            ));
        }
        Ok(())
    }
}

/// Whether the return value is the one meaning the game took the action.
fn returned_ok(ret: ValueKind, returned: Returned, ok: Number) -> bool {
    let Number::Integer(ok) = ok else {
        return false;
    };
    let value = returned.integer;
    let ok = ok.cast_unsigned();
    match ret {
        ValueKind::Bool => (value & 0xFF != 0) == (ok != 0),
        ValueKind::I8 | ValueKind::U8 => value & 0xFF == ok & 0xFF,
        ValueKind::I16 | ValueKind::U16 => value & 0xFFFF == ok & 0xFFFF,
        ValueKind::I32 | ValueKind::U32 => value & 0xFFFF_FFFF == ok & 0xFFFF_FFFF,
        _ => value == ok,
    }
}

#[cfg(test)]
mod tests {
    use super::{Returned, Schema, Value, bits, encode, returned_ok};
    use angelica_map::{Enum, Meaning, Number, Param, ValueKind};

    fn param(kind: ValueKind, meaning: Meaning, range: Option<(Number, Number)>) -> Param {
        Param {
            doc: "A test parameter.".into(),
            kind,
            meaning,
            unit: None,
            default: None,
            range,
        }
    }

    fn schema() -> Schema {
        let mut schema = Schema::default();
        schema.objects.enums.insert(
            "test.kind".into(),
            Enum {
                name: "test.kind".into(),
                doc: "Kinds.".into(),
                flags: false,
                values: [("action".into(), 1), ("item".into(), 2)].into(),
            },
        );
        schema
    }

    #[test]
    fn parameters_are_checked_and_encoded_by_their_kind() {
        let schema = schema();
        let kind = param(ValueKind::U32, Meaning::Enum("test.kind".into()), None);
        assert_eq!(
            encode(&schema, "kind", &kind, &Value::Named("item".into())).unwrap(),
            Number::Integer(2)
        );
        let unknown = encode(&schema, "kind", &kind, &Value::Named("mount".into()));
        assert_eq!(
            unknown.unwrap_err().message,
            "parameter kind has no value mount; its values: action, item"
        );
        let small = param(ValueKind::U8, Meaning::Plain, None);
        assert!(encode(&schema, "small", &small, &Value::Unsigned(256)).is_err());
        assert!(encode(&schema, "small", &small, &Value::Signed(-1)).is_err());
        assert!(encode(&schema, "small", &small, &Value::Float(1.0)).is_err());
        let row = param(ValueKind::U32, Meaning::Sheet("Item".into()), None);
        let other = Value::Row {
            sheet: "Action".into(),
            row: 7,
        };
        assert!(encode(&schema, "row", &row, &other).is_err());
        let speed = param(
            ValueKind::F32,
            Meaning::Plain,
            Some((Number::Float(0.5f64.to_bits()), Number::Integer(2))),
        );
        let fast = encode(&schema, "speed", &speed, &Value::Unsigned(1)).unwrap();
        assert_eq!(bits(ValueKind::F32, fast), u64::from(1.0f32.to_bits()));
        assert!(encode(&schema, "speed", &speed, &Value::Float(2.5)).is_err());
        assert_eq!(
            bits(ValueKind::I32, Number::Integer(-1)),
            u64::MAX,
            "sign-extended; the function reads its low bits"
        );
    }

    #[test]
    fn the_return_value_says_whether_the_game_took_the_action() {
        let returned = |integer| Returned { integer, float: 0 };
        let yes = Number::Integer(1);
        assert!(returned_ok(ValueKind::Bool, returned(0xFFFF_FF01), yes));
        assert!(!returned_ok(ValueKind::Bool, returned(0xFFFF_FF00), yes));
        assert!(returned_ok(
            ValueKind::U32,
            returned(0xDEAD_0000_0000_0000),
            Number::Integer(0)
        ));
        assert!(!returned_ok(
            ValueKind::U32,
            returned(3),
            Number::Integer(0)
        ));
    }
}
