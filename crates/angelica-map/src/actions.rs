//! Actions of the object model: functions of the game a plugin calls for
//! the player, as the game's own interface calls them. Each argument of the
//! function is a parameter the plugin gives, a constant, the address of a
//! global, or an object a path reaches; conditions say when the action may
//! be taken.
use std::collections::BTreeMap;

use serde::Deserialize;

use crate::objects::{Meaning, Objects, is_integer, is_word, not_blank};
use crate::{Entry, EntryKind, Thread, ValueKind};

/// The most times a second a plugin may take one action.
pub const MOST_ACT_RATE: u8 = 20;
/// How many times a second a plugin may take an action without `rate`.
const DEFAULT_ACT_RATE: u8 = 2;
/// The most conditions an action has.
const MOST_CONDITIONS: usize = 16;

/// A number of the map: an integer or a float, kept by its bits so the
/// model stays comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Number {
    Integer(i64),
    Float(u64),
}

impl Number {
    /// The number as a float, whatever it was written as.
    #[must_use]
    pub fn float(self) -> f64 {
        match self {
            #[allow(clippy::cast_precision_loss)] // a float parameter's bound
            Self::Integer(value) => value as f64,
            Self::Float(bits) => f64::from_bits(bits),
        }
    }

    pub(crate) fn from_toml(value: &toml::Value) -> Option<Self> {
        match value {
            toml::Value::Integer(value) => Some(Self::Integer(*value)),
            toml::Value::Float(value) => Some(Self::Float(value.to_bits())),
            toml::Value::Boolean(value) => Some(Self::Integer(i64::from(*value))),
            _ => None,
        }
    }
}

/// A number of the map written as a TOML integer, float, or boolean.
pub(crate) fn number(value: &toml::Value) -> Option<Number> {
    Number::from_toml(value)
}

/// A value the plugin gives for an argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub doc: String,
    /// The argument's kind, from the function's ABI.
    pub kind: ValueKind,
    pub meaning: Meaning,
    pub unit: Option<String>,
    /// The value when the plugin gives none; without it, the plugin must.
    pub default: Option<Number>,
    /// The least and the most value the host passes on.
    pub range: Option<(Number, Number)>,
}

/// Where an argument's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Param(Param),
    /// Always this value.
    Value(Number),
    /// A `global` entry: its address for a `struct`, the pointer it holds
    /// for a `ptr`.
    Global(String),
    /// The address of the first object a path through the object model
    /// reaches; the action is unavailable while it reaches none.
    Object(String),
}

/// One argument of an action's function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActArg {
    pub name: String,
    /// The position in the function's arguments, from 0.
    pub position: u8,
    /// The argument's kind, from the function's ABI.
    pub kind: ValueKind,
    pub source: Source,
}

/// Something that must hold for the action to be taken: a path through the
/// object model that reaches an object, such as `activity[in_combat = false]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub path: String,
    /// Why the action is refused when the path reaches nothing, as the
    /// plugin is told: "you are in combat".
    pub says: String,
}

/// An action: a function of the game and how its arguments are given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub doc: String,
    /// A `function` entry of the main thread.
    pub call: String,
    pub domain: String,
    /// The most times a second one plugin takes it.
    pub rate: u8,
    /// The return value meaning the game took the action; any other is a
    /// refusal. `None` when the return says nothing.
    pub ok: Option<Number>,
    pub when: Vec<Condition>,
    /// Every argument of the function, by position.
    pub args: Vec<ActArg>,
}

impl Action {
    /// The parameters the plugin gives, by name.
    pub fn params(&self) -> impl Iterator<Item = (&str, &Param)> {
        self.args.iter().filter_map(|arg| match &arg.source {
            Source::Param(param) => Some((arg.name.as_str(), param)),
            _ => None,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawActions {
    actions: BTreeMap<String, RawAction>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    doc: String,
    call: String,
    domain: String,
    rate: Option<u8>,
    ok: Option<toml::Value>,
    #[serde(default)]
    when: Vec<RawCondition>,
    #[serde(default)]
    args: BTreeMap<String, RawArg>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCondition {
    path: String,
    says: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArg {
    arg: u8,
    doc: Option<String>,
    value: Option<toml::Value>,
    global: Option<String>,
    object: Option<String>,
    sheet: Option<String>,
    r#enum: Option<String>,
    flags: Option<String>,
    unit: Option<String>,
    default: Option<toml::Value>,
    range: Option<[toml::Value; 2]>,
}

/// Parses `actions.toml`; the kinds of the arguments come later, from the
/// function's entry ([`check`]).
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn parse(text: &str) -> Result<BTreeMap<String, Action>, String> {
    let raw: RawActions = toml::from_str(text).map_err(|error| error.message().to_owned())?;
    let mut actions = BTreeMap::new();
    for (name, action) in raw.actions {
        if !is_word(&name) {
            return Err(format!("`{name}` is not a valid action name"));
        }
        let parsed =
            parse_action(&name, action).map_err(|message| format!("action `{name}`: {message}"))?;
        actions.insert(name, parsed);
    }
    Ok(actions)
}

fn parse_action(name: &str, raw: RawAction) -> Result<Action, String> {
    not_blank("the doc", &raw.doc)?;
    let rate = raw.rate.unwrap_or(DEFAULT_ACT_RATE);
    if !(1..=MOST_ACT_RATE).contains(&rate) {
        return Err(format!("`rate` is 1 to {MOST_ACT_RATE}"));
    }
    let ok = raw
        .ok
        .map(|value| Number::from_toml(&value).ok_or("`ok` is a number or a boolean"))
        .transpose()?;
    if raw.when.len() > MOST_CONDITIONS {
        return Err(format!(
            "an action has at most {MOST_CONDITIONS} conditions"
        ));
    }
    let mut when = Vec::new();
    for condition in raw.when {
        not_blank("a condition's path", &condition.path)?;
        not_blank("a condition's `says`", &condition.says)?;
        when.push(Condition {
            path: condition.path,
            says: condition.says,
        });
    }
    let mut args = Vec::new();
    for (arg_name, arg) in raw.args {
        if !is_word(&arg_name) {
            return Err(format!("`{arg_name}` is not a valid argument name"));
        }
        let position = arg.arg;
        let source =
            parse_source(arg).map_err(|message| format!("argument `{arg_name}`: {message}"))?;
        args.push(ActArg {
            name: arg_name,
            position,
            kind: ValueKind::Void,
            source,
        });
    }
    args.sort_by_key(|arg| arg.position);
    Ok(Action {
        name: name.to_owned(),
        doc: raw.doc,
        call: raw.call,
        domain: raw.domain,
        rate,
        ok,
        when,
        args,
    })
}

fn parse_source(raw: RawArg) -> Result<Source, String> {
    let sources = usize::from(raw.doc.is_some())
        + usize::from(raw.value.is_some())
        + usize::from(raw.global.is_some())
        + usize::from(raw.object.is_some());
    if sources != 1 {
        return Err(
            "an argument is one of a parameter (`doc`), `value`, `global`, and `object`".into(),
        );
    }
    let param_only = raw.sheet.is_some()
        || raw.r#enum.is_some()
        || raw.flags.is_some()
        || raw.unit.is_some()
        || raw.default.is_some()
        || raw.range.is_some();
    if raw.doc.is_none() && param_only {
        return Err("sheet, enum, flags, unit, default, and range belong to parameters".into());
    }
    if let Some(value) = raw.value {
        return Number::from_toml(&value)
            .map(Source::Value)
            .ok_or_else(|| "`value` is a number or a boolean".into());
    }
    if let Some(global) = raw.global {
        return Ok(Source::Global(global));
    }
    if let Some(path) = raw.object {
        not_blank("`object`", &path)?;
        return Ok(Source::Object(path));
    }
    let doc = raw.doc.unwrap_or_default();
    not_blank("the doc", &doc)?;
    let meanings = usize::from(raw.sheet.is_some())
        + usize::from(raw.r#enum.is_some())
        + usize::from(raw.flags.is_some());
    if meanings > 1 {
        return Err("a value means at most one of sheet, enum, and flags".into());
    }
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
    let number = |value: &toml::Value, what: &str| {
        Number::from_toml(value).ok_or_else(|| format!("`{what}` is a number or a boolean"))
    };
    let default = raw
        .default
        .map(|value| number(&value, "default"))
        .transpose()?;
    let range = match raw.range {
        Some([least, most]) => Some((number(&least, "range")?, number(&most, "range")?)),
        None => None,
    };
    Ok(Source::Param(Param {
        doc,
        kind: ValueKind::Void,
        meaning,
        unit: raw.unit,
        default,
        range,
    }))
}

/// Checks the actions against the entries and the rest of the object model,
/// and reads each argument's kind from the function's ABI.
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn check(
    objects: &mut Objects,
    entries: &BTreeMap<String, Entry>,
) -> Result<(), String> {
    let mut actions = std::mem::take(&mut objects.actions);
    for action in actions.values_mut() {
        check_action(objects, entries, action)
            .map_err(|message| format!("action `{}`: {message}", action.name))?;
    }
    objects.actions = actions;
    Ok(())
}

fn check_action(
    objects: &Objects,
    entries: &BTreeMap<String, Entry>,
    action: &mut Action,
) -> Result<(), String> {
    let Some(domain) = objects.domains.get(&action.domain) else {
        return Err(format!("domain `{}` is not in domains.toml", action.domain));
    };
    if domain.act.is_none() {
        return Err(format!(
            "domain `{}` has no `act` wording for the player",
            action.domain
        ));
    }
    let abi = match entries.get(&action.call).map(|entry| &entry.kind) {
        Some(EntryKind::Function {
            abi,
            thread: Thread::Main,
        }) => abi,
        Some(EntryKind::Function { .. }) => {
            return Err(format!(
                "`{}` is not a function of the main thread",
                action.call
            ));
        }
        Some(_) => return Err(format!("`{}` is not a function", action.call)),
        None => return Err(format!("`{}` is not an entry", action.call)),
    };
    if action.ok.is_some() && !is_integer(abi.ret) && abi.ret != ValueKind::Bool {
        return Err("`ok` needs a function that returns an integer or a bool".into());
    }
    let positions: Vec<u8> = action.args.iter().map(|arg| arg.position).collect();
    let expected: Vec<u8> = (0..abi.args.len())
        .map(|position| u8::try_from(position).unwrap_or(u8::MAX))
        .collect();
    if positions != expected {
        return Err(format!(
            "every argument of `{}` is given once: it has {}, positions 0 to {}",
            action.call,
            abi.args.len(),
            abi.args.len().saturating_sub(1)
        ));
    }
    for arg in &mut action.args {
        arg.kind = abi.args[usize::from(arg.position)];
        check_arg(objects, entries, arg)
            .map_err(|message| format!("argument `{}`: {message}", arg.name))?;
    }
    Ok(())
}

fn check_arg(
    objects: &Objects,
    entries: &BTreeMap<String, Entry>,
    arg: &mut ActArg,
) -> Result<(), String> {
    let kind = arg.kind;
    let float = matches!(kind, ValueKind::F32 | ValueKind::F64);
    let fits = |number: Number| match number {
        Number::Integer(_) => true,
        Number::Float(_) => float,
    };
    match &mut arg.source {
        Source::Value(value) => {
            if !fits(*value) {
                return Err("an integer argument has an integer `value`".into());
            }
        }
        Source::Global(global) => {
            if kind != ValueKind::Ptr {
                return Err("a global is passed as a pointer: the kind is ptr".into());
            }
            match entries.get(global.as_str()).map(|entry| &entry.kind) {
                Some(EntryKind::Global {
                    value: ValueKind::Struct | ValueKind::Ptr,
                }) => {}
                Some(_) => return Err(format!("`{global}` is not a global struct or pointer")),
                None => return Err(format!("`{global}` is not an entry")),
            }
        }
        Source::Object(_) => {
            if kind != ValueKind::Ptr {
                return Err("an object is passed as a pointer: the kind is ptr".into());
            }
        }
        Source::Param(param) => {
            if matches!(kind, ValueKind::Ptr | ValueKind::Void | ValueKind::Struct) {
                return Err("a plugin gives numbers and booleans, not pointers".into());
            }
            param.kind = kind;
            match &param.meaning {
                Meaning::Enum(name) | Meaning::Flags(name) => {
                    let flags = matches!(param.meaning, Meaning::Flags(_));
                    match objects.enums.get(name) {
                        Some(found) if found.flags == flags => {}
                        Some(_) => return Err(format!("`{name}` is not an enum of that kind")),
                        None => return Err(format!("`{name}` is not an enum")),
                    }
                }
                Meaning::Sheet(_) | Meaning::Plain => {}
            }
            if !matches!(param.meaning, Meaning::Plain) && !is_integer(kind) {
                return Err("a sheet row, enum, or flags is an integer".into());
            }
            for number in param
                .default
                .iter()
                .chain(param.range.iter().flat_map(|(least, most)| [least, most]))
            {
                if !fits(*number) {
                    return Err("an integer parameter has integer bounds and default".into());
                }
            }
            if let Some((least, most)) = param.range
                && least.float() > most.float()
            {
                return Err("a range's least value is above its most".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Number, Source};
    use crate::{Map, ValueKind};

    const ENTRIES: &str = r#"
[function."action.manager.use"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u32", "u32", "f32"], ret = "bool" }
thread = "main"

[function."action.manager.any"]
locator = { signature = "C3 90" }
abi = { args = ["u32"], ret = "void" }
thread = "any"

[global."action.manager.instance"]
locator = { signature = "C3 90 90" }
type = "struct"
"#;

    const DOMAINS: &str = r#"
[domains.actions]
doc = "Actions."
class = "you"
read = { en = "Sees the actions you use", ru = "Видит действия, которые вы используете" }
act = { en = "Uses actions for you", ru = "Использует действия за вас" }

[domains.quiet]
doc = "Read only."
class = "you"
read = { en = "Sees", ru = "Видит" }
"#;

    fn map(actions: &str) -> Result<Map, crate::MapError> {
        Map::from_sources(
            "format = 2",
            &[
                ("entries/action.toml".into(), ENTRIES.into()),
                ("domains.toml".into(), DOMAINS.into()),
                (
                    "enums/action.kind.toml".into(),
                    "doc = \"Kinds.\"\nvalues = { action = 1, item = 2 }\n".into(),
                ),
                ("actions.toml".into(), actions.into()),
            ],
        )
    }

    const USE: &str = r#"
[actions.use_action]
doc = "Uses an action."
call = "action.manager.use"
domain = "actions"
rate = 4
ok = true
when = [{ path = "player", says = "you are not in the world" }]

[actions.use_action.args.manager]
arg = 0
global = "action.manager.instance"

[actions.use_action.args.kind]
arg = 1
doc = "The kind."
enum = "action.kind"

[actions.use_action.args.action]
arg = 2
doc = "The action."
sheet = "Action"
default = 7

[actions.use_action.args.speed]
arg = 3
doc = "How fast."
range = [0.5, 2]
"#;

    #[test]
    fn arguments_take_their_kinds_from_the_function() {
        let map = map(USE).unwrap();
        let action = &map.objects().actions["use_action"];
        assert_eq!(action.rate, 4);
        assert_eq!(action.ok, Some(Number::Integer(1)));
        assert_eq!(action.when[0].says, "you are not in the world");
        let kinds: Vec<_> = action.args.iter().map(|arg| arg.kind).collect();
        assert_eq!(
            kinds,
            [
                ValueKind::Ptr,
                ValueKind::U32,
                ValueKind::U32,
                ValueKind::F32
            ]
        );
        assert!(
            matches!(&action.args[0].source, Source::Global(name) if name == "action.manager.instance")
        );
        let params: Vec<_> = action.params().map(|(name, _)| name).collect();
        assert_eq!(params, ["kind", "action", "speed"]);
    }

    #[test]
    fn mistakes_make_the_map_invalid() {
        for (from, to) in [
            ("domain = \"actions\"", "domain = \"quiet\""),
            ("domain = \"actions\"", "domain = \"party\""),
            (
                "call = \"action.manager.use\"",
                "call = \"action.manager.any\"",
            ),
            (
                "call = \"action.manager.use\"",
                "call = \"action.manager.instance\"",
            ),
            ("rate = 4", "rate = 21"),
            ("arg = 3\n", "arg = 4\n"),
            ("arg = 0\nglobal = \"action.manager.instance\"\n", ""),
            ("enum = \"action.kind\"", "enum = \"action.nothing\""),
            ("sheet = \"Action\"\ndefault = 7", "default = 7.5"),
            ("range = [0.5, 2]", "range = [2, 0.5]"),
            (
                "arg = 1\ndoc = \"The kind.\"",
                "arg = 1\nvalue = 1\ndoc = \"The kind.\"",
            ),
            (
                "global = \"action.manager.instance\"",
                "global = \"action.manager.use\"",
            ),
            (
                "global = \"action.manager.instance\"",
                "doc = \"A pointer.\"",
            ),
            ("[actions.use_action]", "[actions.Use]"),
        ] {
            let changed = USE.replace(from, to);
            assert_ne!(changed, USE, "{from}");
            assert!(map(&changed).is_err(), "{to}");
        }
    }
}
