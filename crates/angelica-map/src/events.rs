//! Events of the object model: points of the game's code, `function` or
//! `location` entries, whose arguments are fields a plugin watches. An
//! argument is a value, text behind a pointer, or a reference to an object of
//! a type, so a watch reads the objects the game's call works on, as a query
//! reads them from a root.
use std::collections::BTreeMap;

use serde::Deserialize;

use crate::objects::{Meaning, Objects, Shape, is_integer, is_word, not_blank};
use crate::{Entry, EntryKind, ValueKind};

/// The most firings of an event a watch keeps between two frames.
pub const MOST_RATE: u16 = 1024;

/// A register at a location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    Rax,
    Rbx,
    Rcx,
    Rdx,
    Rsi,
    Rdi,
    Rbp,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
    /// The low 64 bits of `xmm0` to `xmm15`.
    Xmm(u8),
}

impl Register {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "rax" => Self::Rax,
            "rbx" => Self::Rbx,
            "rcx" => Self::Rcx,
            "rdx" => Self::Rdx,
            "rsi" => Self::Rsi,
            "rdi" => Self::Rdi,
            "rbp" => Self::Rbp,
            "r8" => Self::R8,
            "r9" => Self::R9,
            "r10" => Self::R10,
            "r11" => Self::R11,
            "r12" => Self::R12,
            "r13" => Self::R13,
            "r14" => Self::R14,
            "r15" => Self::R15,
            _ => {
                let index = text.strip_prefix("xmm")?.parse::<u8>().ok()?;
                (index < 16 && text == format!("xmm{index}")).then_some(Self::Xmm(index))?
            }
        })
    }
}

/// Where an argument's value is when the event fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The function's argument at this position, from 0, by its ABI.
    Argument(u8),
    /// A register at the location.
    Register(Register),
}

/// One argument of an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    pub name: String,
    pub doc: String,
    pub source: Source,
    /// The value's kind, from the function's ABI or the argument's `kind`.
    pub kind: ValueKind,
    /// A value, text behind the pointer, or a reference to an object.
    pub shape: Shape,
    pub domain: String,
    pub forbidden: bool,
}

/// An event: a point of the game's code and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub doc: String,
    /// A `function` entry, fired when the function is called, or a
    /// `location` entry, fired when the instruction runs.
    pub at: String,
    pub domain: String,
    /// The most firings a watch keeps between two frames.
    pub rate: u16,
    pub args: BTreeMap<String, Arg>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvents {
    events: BTreeMap<String, RawEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvent {
    doc: String,
    at: String,
    domain: String,
    rate: Option<u16>,
    #[serde(default)]
    args: BTreeMap<String, RawArg>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArg {
    doc: String,
    arg: Option<u8>,
    register: Option<String>,
    kind: Option<String>,
    text: Option<u16>,
    r#ref: Option<String>,
    sheet: Option<String>,
    r#enum: Option<String>,
    flags: Option<String>,
    unit: Option<String>,
    domain: Option<String>,
    #[serde(default)]
    forbidden: bool,
}

/// Parses `events.toml`; the kinds of function arguments come later, from
/// their entries ([`check`]).
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn parse(text: &str) -> Result<BTreeMap<String, Event>, String> {
    let raw: RawEvents = toml::from_str(text).map_err(|error| error.message().to_owned())?;
    let mut events = BTreeMap::new();
    for (name, event) in raw.events {
        if !is_word(&name) {
            return Err(format!("`{name}` is not a valid event name"));
        }
        not_blank(&format!("the doc of event `{name}`"), &event.doc)?;
        let rate = event.rate.unwrap_or(64);
        if !(1..=MOST_RATE).contains(&rate) {
            return Err(format!("event `{name}`: `rate` is 1 to {MOST_RATE}"));
        }
        let mut args = BTreeMap::new();
        for (arg_name, arg) in event.args {
            if !is_word(&arg_name) {
                return Err(format!(
                    "event `{name}`: `{arg_name}` is not a valid argument name"
                ));
            }
            let parsed = parse_arg(&arg_name, arg, &event.domain)
                .map_err(|message| format!("event `{name}`, argument `{arg_name}`: {message}"))?;
            args.insert(arg_name, parsed);
        }
        if args.is_empty() {
            return Err(format!("event `{name}` has no arguments"));
        }
        events.insert(
            name.clone(),
            Event {
                name,
                doc: event.doc,
                at: event.at,
                domain: event.domain,
                rate,
                args,
            },
        );
    }
    Ok(events)
}

fn parse_arg(name: &str, raw: RawArg, domain: &str) -> Result<Arg, String> {
    not_blank("doc", &raw.doc)?;
    let source = match (raw.arg, &raw.register) {
        (Some(position), None) => Source::Argument(position),
        (None, Some(register)) => Source::Register(
            Register::parse(register).ok_or_else(|| format!("`{register}` is not a register"))?,
        ),
        _ => return Err("an argument has either `arg` or `register`".into()),
    };
    let kind = match (&source, &raw.kind) {
        (Source::Argument(_), None) => ValueKind::Void,
        (Source::Argument(_), Some(_)) => {
            return Err("a function's argument takes its kind from the function's ABI".into());
        }
        (Source::Register(_), Some(kind)) => match crate::model::parse_value_kind(kind) {
            Some(ValueKind::Void | ValueKind::Struct) | None => {
                return Err(format!("`{kind}` is not a value kind"));
            }
            Some(kind) => kind,
        },
        (Source::Register(_), None) => return Err("a register gives its `kind`".into()),
    };
    let meanings = usize::from(raw.sheet.is_some())
        + usize::from(raw.r#enum.is_some())
        + usize::from(raw.flags.is_some());
    if meanings > 1 {
        return Err("a value means at most one of sheet, enum, and flags".into());
    }
    let shape = match (raw.text, raw.r#ref) {
        (Some(_), Some(_)) => return Err("an argument is text or a reference, not both".into()),
        (Some(length), None) => {
            if !(1..=crate::objects::MOST_TEXT).contains(&length) {
                return Err(format!("text is 1 to {} bytes", crate::objects::MOST_TEXT));
            }
            Shape::Text(length)
        }
        (None, Some(to)) => Shape::Ref(to),
        (None, None) => Shape::Value {
            kind,
            meaning: if let Some(sheet) = raw.sheet {
                not_blank("sheet", &sheet)?;
                Meaning::Sheet(sheet)
            } else if let Some(name) = raw.r#enum {
                Meaning::Enum(name)
            } else if let Some(name) = raw.flags {
                Meaning::Flags(name)
            } else {
                Meaning::Plain
            },
            unit: raw.unit,
        },
    };
    if !matches!(shape, Shape::Value { .. }) && meanings > 0 {
        return Err("sheet, enum, and flags belong to values".into());
    }
    Ok(Arg {
        name: name.to_owned(),
        doc: raw.doc,
        source,
        kind,
        shape,
        domain: raw.domain.unwrap_or_else(|| domain.to_owned()),
        forbidden: raw.forbidden,
    })
}

/// Checks the events against the entries and the rest of the object model,
/// and reads each function argument's kind from the function's ABI.
///
/// # Errors
/// A message naming what is wrong.
pub(crate) fn check(
    objects: &mut Objects,
    entries: &BTreeMap<String, Entry>,
) -> Result<(), String> {
    let mut events = std::mem::take(&mut objects.events);
    for event in events.values_mut() {
        check_event(objects, entries, event)
            .map_err(|message| format!("event `{}`: {message}", event.name))?;
    }
    objects.events = events;
    Ok(())
}

fn check_event(
    objects: &Objects,
    entries: &BTreeMap<String, Entry>,
    event: &mut Event,
) -> Result<(), String> {
    if objects.roots.contains_key(&event.name) {
        return Err("a root has the same name".into());
    }
    if !objects.domains.contains_key(&event.domain) {
        return Err(format!("domain `{}` is not in domains.toml", event.domain));
    }
    let abi = match entries.get(&event.at).map(|entry| &entry.kind) {
        Some(EntryKind::Function { abi, .. }) => Some(abi),
        Some(EntryKind::Location { .. }) => None,
        Some(_) => return Err(format!("`{}` is not a function or a location", event.at)),
        None => return Err(format!("`{}` is not an entry", event.at)),
    };
    for arg in event.args.values_mut() {
        let wrong = |message: String| format!("argument `{}`: {message}", arg.name);
        match (arg.source, abi) {
            (Source::Argument(position), Some(abi)) => {
                arg.kind = *abi.args.get(usize::from(position)).ok_or_else(|| {
                    wrong(format!("`{}` has {} arguments", event.at, abi.args.len()))
                })?;
            }
            (Source::Argument(_), None) => {
                return Err(wrong(format!(
                    "`{}` is a location: name a `register`",
                    event.at
                )));
            }
            (Source::Register(_), Some(_)) => {
                return Err(wrong(format!(
                    "`{}` is a function: name its `arg`",
                    event.at
                )));
            }
            (Source::Register(_), None) => {}
        }
        if !objects.domains.contains_key(&arg.domain) {
            return Err(wrong(format!(
                "domain `{}` is not in domains.toml",
                arg.domain
            )));
        }
        let float = matches!(arg.kind, ValueKind::F32 | ValueKind::F64);
        if let Source::Register(register) = arg.source
            && float != matches!(register, Register::Xmm(_))
        {
            return Err(wrong(
                "a float is in an xmm register, and only a float".into(),
            ));
        }
        let kind = arg.kind;
        match &mut arg.shape {
            Shape::Value {
                kind: value,
                meaning,
                ..
            } => {
                if matches!(kind, ValueKind::Ptr) && !matches!(meaning, Meaning::Plain) {
                    return Err(wrong("a pointer has no sheet, enum, or flags".into()));
                }
                let flags = matches!(meaning, Meaning::Flags(_));
                match &*meaning {
                    Meaning::Enum(name) | Meaning::Flags(name) => match objects.enums.get(name) {
                        Some(found) if found.flags == flags => {}
                        Some(_) => {
                            return Err(wrong(format!("`{name}` is not an enum of that kind")));
                        }
                        None => return Err(wrong(format!("`{name}` is not an enum"))),
                    },
                    Meaning::Sheet(_) | Meaning::Plain => {}
                }
                if !matches!(meaning, Meaning::Plain) && !is_integer(kind) {
                    return Err(wrong("a sheet row, enum, or flags is an integer".into()));
                }
                *value = kind;
            }
            Shape::Text(_) | Shape::Ref(_) if kind != ValueKind::Ptr => {
                return Err(wrong(
                    "text and references are behind a pointer: the kind is ptr".into(),
                ));
            }
            Shape::Ref(to) if !objects.types.contains_key(to.as_str()) => {
                return Err(wrong(format!("`{to}` is not a type")));
            }
            Shape::Text(_) | Shape::Ref(_) => {}
            Shape::Embed(_) | Shape::Array { .. } => unreachable!("arguments are not parsed so"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{ArgSource, Map, Register, Shape, ValueKind};

    const ENTRIES: &str = r#"
[function."action.manager.use"]
locator = { signature = "C3" }
abi = { args = ["ptr", "u32", "u32", "u64", "u32", "f32"], ret = "bool" }
thread = "main"

[location."action.manager.used"]
locator = { signature = "C3 90" }
thread = "main"
"#;

    const DOMAINS: &str = r#"
[domains.actions]
doc = "Actions."
class = "you"
read = { en = "Sees the actions you use", ru = "Видит действия, которые вы используете" }
"#;

    fn map(events: &str) -> Result<Map, crate::MapError> {
        Map::from_sources(
            "format = 2",
            &[
                ("entries/action.toml".into(), ENTRIES.into()),
                ("domains.toml".into(), DOMAINS.into()),
                (
                    "enums/action.kind.toml".into(),
                    "doc = \"Kinds.\"\nvalues = { action = 1, item = 2 }\n".into(),
                ),
                ("events.toml".into(), events.into()),
            ],
        )
    }

    const USED: &str = r#"
[events.action_used]
doc = "An action was used."
at = "action.manager.use"
domain = "actions"

[events.action_used.args.kind]
doc = "The kind."
arg = 1
enum = "action.kind"

[events.action_used.args.speed]
doc = "A float argument."
arg = 5

[events.action_used.args.checked]
doc = "At a location."
at = "x"
"#;

    #[test]
    fn arguments_take_their_kinds_from_the_function() {
        let valid = USED.replace(
            "[events.action_used.args.checked]\ndoc = \"At a location.\"\nat = \"x\"\n",
            "[events.action_seen]\ndoc = \"Seen.\"\nat = \"action.manager.used\"\ndomain = \"actions\"\nrate = 8\n\n[events.action_seen.args.id]\ndoc = \"An id.\"\nregister = \"r8\"\nkind = \"u32\"\nsheet = \"Action\"\n",
        );
        let map = map(&valid).unwrap();
        let events = &map.objects().events;
        let used = &events["action_used"];
        assert_eq!(used.rate, 64);
        assert_eq!(used.args["kind"].kind, ValueKind::U32);
        assert_eq!(used.args["speed"].kind, ValueKind::F32);
        assert!(matches!(
            used.args["kind"].shape,
            Shape::Value {
                kind: ValueKind::U32,
                ..
            }
        ));
        let seen = &events["action_seen"];
        assert_eq!(seen.rate, 8);
        assert_eq!(seen.args["id"].source, ArgSource::Register(Register::R8));
    }

    #[test]
    fn mistakes_make_the_map_invalid() {
        let base = USED.replace(
            "[events.action_used.args.checked]\ndoc = \"At a location.\"\nat = \"x\"\n",
            "",
        );
        assert!(map(&base).is_ok());
        for (from, to) in [
            ("arg = 1\n", "arg = 9\n"),
            (
                "at = \"action.manager.use\"",
                "at = \"action.manager.used\"",
            ),
            ("at = \"action.manager.use\"", "at = \"action.nothing\""),
            ("domain = \"actions\"", "domain = \"party\""),
            ("enum = \"action.kind\"", "enum = \"action.nothing\""),
            ("arg = 1\n", "arg = 1\nkind = \"u8\"\n"),
            ("arg = 5\n", "arg = 5\nsheet = \"Action\"\n"),
            (
                "arg = 1\nenum = \"action.kind\"\n",
                "arg = 1\nref = \"action.manager\"\n",
            ),
            ("[events.action_used]", "[events.Action]"),
        ] {
            assert!(map(&base.replace(from, to)).is_err(), "{to}");
        }
        let register = |register: &str, kind: &str| {
            format!(
                "[events.seen]\ndoc = \"Seen.\"\nat = \"action.manager.used\"\ndomain = \"actions\"\n\n[events.seen.args.value]\ndoc = \"A value.\"\nregister = \"{register}\"\nkind = \"{kind}\"\n"
            )
        };
        assert!(map(&register("xmm3", "f32")).is_ok());
        for (name, kind) in [
            ("xmm3", "u32"),
            ("rcx", "f64"),
            ("xmm16", "f32"),
            ("rip", "u64"),
        ] {
            assert!(map(&register(name, kind)).is_err(), "{name} {kind}");
        }
    }
}
