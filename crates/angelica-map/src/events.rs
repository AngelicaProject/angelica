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
/// The most bytes of text an outcome takes.
pub const MOST_OUTCOME_TEXT: u32 = 65535;

/// A value the host's adapter gives a derived event: an integer of a kind,
/// or text of at most this many bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedValue {
    Integer(ValueKind),
    Text(u16),
}

/// The events the host derives at a function it hooks, instead of reading
/// the function's arguments: their names and the values each gives.
pub const DERIVATIONS: &[(&str, &[(&str, DerivedValue)])] = &[
    (
        "sheet_cell",
        &[
            ("sheet", DerivedValue::Text(256)),
            ("language", DerivedValue::Integer(ValueKind::U8)),
            ("row", DerivedValue::Integer(ValueKind::U32)),
            ("subrow", DerivedValue::Integer(ValueKind::U16)),
            ("column", DerivedValue::Integer(ValueKind::U16)),
            ("text", DerivedValue::Text(4096)),
        ],
    ),
    ("case_letter", &[("letter", DerivedValue::Text(4))]),
];

/// The formats of rule files each derivation takes.
pub const RULE_FORMATS: &[(&str, &str)] = &[("sheet_cell", "text-table-v1")];

/// The values a derivation gives, by name.
#[must_use]
pub fn derivation(name: &str) -> Option<&'static [(&'static str, DerivedValue)]> {
    DERIVATIONS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, values)| *values)
}

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The function's argument at this position, from 0, by its ABI.
    Argument(u8),
    /// A register at the location.
    Register(Register),
    /// A value the host's adapter derives, by its name in the derivation.
    Derived(String),
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
    /// The bits of an integer value that hold it; the others are cleared.
    pub mask: Option<u64>,
    /// For a reference: whose object it is, and the domains renamed below
    /// it.
    pub entering: crate::whose::Entering,
}

/// What a plugin's rule may make the game do at an event of a function:
/// return at once with `returns`, without running the function; or, at a
/// derived event, take the rule's text in place of what the host derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub name: String,
    pub doc: String,
    /// The value the function returns; `None` for a function without one.
    pub returns: Option<crate::Number>,
    /// For a derived event: the rule gives text of at most this many bytes.
    pub text: Option<u32>,
    /// For a derived event: an argument whose value in the rule's equality
    /// condition has as many bytes as the rule's text must.
    pub same_length: Option<String>,
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
    /// What rules may decide at the event; an event with outcomes is a
    /// decision point, at a function's start.
    pub outcomes: BTreeMap<String, Outcome>,
    /// For an event the host derives at `at`: the derivation's name.
    pub derived: Option<String>,
    /// The format of rule files the event takes, besides single rules.
    pub rules: Option<String>,
    /// Whose objects and values it gives, and the domains renamed in them.
    pub entering: crate::whose::Entering,
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
    derived: Option<String>,
    rules: Option<String>,
    #[serde(default)]
    args: BTreeMap<String, RawArg>,
    #[serde(default)]
    outcomes: BTreeMap<String, RawOutcome>,
    whose: Option<String>,
    r#as: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOutcome {
    doc: String,
    returns: Option<toml::Value>,
    text: Option<u32>,
    same_length: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArg {
    doc: String,
    arg: Option<u8>,
    register: Option<String>,
    from: Option<String>,
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
    mask: Option<u64>,
    whose: Option<String>,
    r#as: Option<BTreeMap<String, String>>,
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
        let entering = crate::whose::Entering::parse(event.whose, event.r#as)
            .map_err(|message| format!("event `{name}`: {message}"))?;
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
        let mut outcomes = BTreeMap::new();
        for (outcome_name, outcome) in event.outcomes {
            if !is_word(&outcome_name) {
                return Err(format!(
                    "event `{name}`: `{outcome_name}` is not a valid outcome name"
                ));
            }
            not_blank(
                &format!("the doc of outcome `{outcome_name}`"),
                &outcome.doc,
            )?;
            let returns = outcome
                .returns
                .map(|value| {
                    crate::actions::number(&value).ok_or_else(|| {
                        format!("event `{name}`, outcome `{outcome_name}`: `returns` is a number or a boolean")
                    })
                })
                .transpose()?;
            if outcome
                .text
                .is_some_and(|most| !(1..=MOST_OUTCOME_TEXT).contains(&most))
            {
                return Err(format!(
                    "event `{name}`, outcome `{outcome_name}`: `text` is 1 to {MOST_OUTCOME_TEXT} bytes"
                ));
            }
            outcomes.insert(
                outcome_name.clone(),
                Outcome {
                    name: outcome_name,
                    doc: outcome.doc,
                    returns,
                    text: outcome.text,
                    same_length: outcome.same_length,
                },
            );
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
                outcomes,
                derived: event.derived,
                rules: event.rules,
                entering,
            },
        );
    }
    Ok(events)
}

fn parse_arg(name: &str, raw: RawArg, domain: &str) -> Result<Arg, String> {
    not_blank("doc", &raw.doc)?;
    let source = match (raw.arg, &raw.register, &raw.from) {
        (Some(position), None, None) => Source::Argument(position),
        (None, Some(register), None) => Source::Register(
            Register::parse(register).ok_or_else(|| format!("`{register}` is not a register"))?,
        ),
        (None, None, Some(from)) => Source::Derived(from.clone()),
        _ => return Err("an argument has one of `arg`, `register`, and `from`".into()),
    };
    let kind = match (&source, &raw.kind) {
        (Source::Derived(_) | Source::Argument(_), None) => ValueKind::Void,
        (Source::Derived(_), Some(_)) => {
            return Err("a derived value takes its kind from the derivation".into());
        }
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
    if raw.mask == Some(0) {
        return Err("a mask keeps at least one bit".into());
    }
    if raw.mask.is_some() && !matches!(shape, Shape::Value { .. }) {
        return Err("a mask belongs to values".into());
    }
    let entering = crate::whose::Entering::parse(raw.whose, raw.r#as)?;
    if !entering.is_empty() && !matches!(shape, Shape::Ref(_)) {
        return Err("`whose` and `as` belong to references".into());
    }
    Ok(Arg {
        name: name.to_owned(),
        doc: raw.doc,
        source,
        kind,
        shape,
        domain: raw.domain.unwrap_or_else(|| domain.to_owned()),
        forbidden: raw.forbidden,
        mask: raw.mask,
        entering,
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
    if let Some(derivation) = event.derived.clone() {
        return check_derived(objects, event, &derivation, abi.is_some());
    }
    if event.rules.is_some() {
        return Err("only a derived event takes rule files".into());
    }
    check_outcomes(objects, event, abi)?;
    for arg in event.args.values_mut() {
        let wrong = |message: String| format!("argument `{}`: {message}", arg.name);
        match (arg.source.clone(), abi) {
            (Source::Derived(_), _) => {
                return Err(wrong("only a derived event has `from`".into()));
            }
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
        if let Source::Register(register) = &arg.source
            && float != matches!(*register, Register::Xmm(_))
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
                    Meaning::Sheet(_) | Meaning::Plain | Meaning::Element(_) => {}
                }
                if !matches!(meaning, Meaning::Plain) && !is_integer(kind) {
                    return Err(wrong("a sheet row, enum, or flags is an integer".into()));
                }
                *value = kind;
                if arg.mask.is_some() && !is_integer(kind) {
                    return Err(wrong("a mask belongs to integers".into()));
                }
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

/// Checks an event the host derives at a function: every argument a value
/// of the derivation, of its kind; outcomes that take text, at a domain
/// with `change` wording; rule files of a format the derivation takes.
fn check_derived(
    objects: &Objects,
    event: &mut Event,
    derivation: &str,
    function: bool,
) -> Result<(), String> {
    let Some(values) = crate::events::derivation(derivation) else {
        let known = DERIVATIONS
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "`{derivation}` is not a derivation of the host: {known}"
        ));
    };
    if !function {
        return Err(format!(
            "`{}` is a location: a derived event is at a function",
            event.at
        ));
    }
    if let Some(format) = &event.rules
        && !RULE_FORMATS.contains(&(derivation, format.as_str()))
    {
        return Err(format!("`{derivation}` takes no rule files of `{format}`"));
    }
    for arg in event.args.values_mut() {
        let wrong = |message: String| format!("argument `{}`: {message}", arg.name);
        let Source::Derived(from) = &arg.source else {
            return Err(wrong("an argument of a derived event has `from`".into()));
        };
        let Some((_, given)) = values.iter().find(|(name, _)| name == from) else {
            return Err(wrong(format!("`{derivation}` gives no `{from}`")));
        };
        if !objects.domains.contains_key(&arg.domain) {
            return Err(wrong(format!(
                "domain `{}` is not in domains.toml",
                arg.domain
            )));
        }
        match (given, &mut arg.shape) {
            (
                DerivedValue::Integer(kind),
                Shape::Value {
                    kind: value,
                    meaning,
                    ..
                },
            ) => {
                if let Meaning::Enum(name) | Meaning::Flags(name) = &*meaning
                    && !objects.enums.contains_key(name)
                {
                    return Err(wrong(format!("`{name}` is not an enum")));
                }
                if matches!(meaning, Meaning::Element(_)) {
                    return Err(wrong("a derived value names no window element".into()));
                }
                *value = *kind;
                arg.kind = *kind;
            }
            (DerivedValue::Text(most), Shape::Text(length)) if *length <= *most => {
                arg.kind = ValueKind::Ptr;
            }
            (DerivedValue::Text(most), _) => {
                return Err(wrong(format!("`{from}` is text of at most {most} bytes")));
            }
            (DerivedValue::Integer(_), _) => {
                return Err(wrong(format!("`{from}` is an integer")));
            }
        }
        if arg.mask.is_some() {
            return Err(wrong("a derived value has no mask".into()));
        }
    }
    if objects
        .domains
        .get(&event.domain)
        .is_none_or(|domain| domain.change.is_none())
        && !event.outcomes.is_empty()
    {
        return Err(format!(
            "domain `{}` has no `change` wording for the player",
            event.domain
        ));
    }
    for outcome in event.outcomes.values() {
        if outcome.returns.is_some() || outcome.text.is_none() {
            return Err(format!(
                "outcome `{}`: at a derived event an outcome takes `text`, and returns nothing",
                outcome.name
            ));
        }
        if let Some(arg) = &outcome.same_length
            && !matches!(
                event.args.get(arg).map(|arg| &arg.shape),
                Some(Shape::Text(_))
            )
        {
            return Err(format!(
                "outcome `{}`: `same_length` names `{arg}`, which is not a text argument",
                outcome.name
            ));
        }
    }
    Ok(())
}

/// Checks a decision point: a function's start, a domain with `change`
/// wording, and outcomes that return a value of the function's kind in
/// `rax`, or nothing for a function without one.
fn check_outcomes(
    objects: &Objects,
    event: &Event,
    abi: Option<&crate::Abi>,
) -> Result<(), String> {
    if event.outcomes.is_empty() {
        return Ok(());
    }
    let Some(abi) = abi else {
        return Err(format!(
            "`{}` is a location: a decision point is a function's start",
            event.at
        ));
    };
    if objects
        .domains
        .get(&event.domain)
        .is_none_or(|domain| domain.change.is_none())
    {
        return Err(format!(
            "domain `{}` has no `change` wording for the player",
            event.domain
        ));
    }
    for outcome in event.outcomes.values() {
        if outcome.text.is_some() || outcome.same_length.is_some() {
            return Err(format!(
                "outcome `{}`: only a derived event's outcome takes text",
                outcome.name
            ));
        }
        // A decision returns in `rax`, never in `xmm0`.
        let fits = match (abi.ret, outcome.returns) {
            (ValueKind::Void, None) => true,
            (kind, Some(crate::Number::Integer(_))) => is_integer(kind) || kind == ValueKind::Bool,
            _ => false,
        };
        if !fits {
            return Err(format!(
                "outcome `{}`: `returns` gives an integer or a boolean of the function's return kind, and only when it has one",
                outcome.name
            ));
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
    fn decision_points_have_outcomes_of_their_return_kind() {
        let changing = DOMAINS.replace(
            "read = { en = \"Sees the actions you use\", ru = \"Видит действия, которые вы используете\" }",
            "read = { en = \"Sees the actions you use\", ru = \"Видит действия, которые вы используете\" }
change = { en = \"Stops actions\", ru = \"Отменяет действия\" }",
        );
        let decided = |domains: &str, extra: &str| {
            Map::from_sources(
                "format = 2",
                &[
                    ("entries/action.toml".into(), ENTRIES.into()),
                    ("domains.toml".into(), domains.into()),
                    (
                        "enums/action.kind.toml".into(),
                        "doc = \"Kinds.\"
values = { action = 1, item = 2 }
"
                        .into(),
                    ),
                    (
                        "events.toml".into(),
                        format!(
                            "[events.used]
doc = \"Used.\"
at = \"action.manager.use\"
domain = \"actions\"

[events.used.args.kind]
doc = \"The kind.\"
arg = 1
mask = 0x7F

{extra}"
                        ),
                    ),
                ],
            )
        };
        let refuse = "[events.used.outcomes.refuse]
doc = \"Not used.\"
returns = false
";
        let map = decided(&changing, refuse).unwrap();
        let used = &map.objects().events["used"];
        assert_eq!(used.args["kind"].mask, Some(0x7F));
        assert_eq!(
            used.outcomes["refuse"].returns,
            Some(crate::Number::Integer(0))
        );
        assert!(decided(DOMAINS, refuse).is_err(), "no change wording");
        for wrong in [
            "[events.used.outcomes.refuse]
doc = \"Not used.\"
",
            "[events.used.outcomes.refuse]
doc = \"Not used.\"
returns = 1.5
",
            "[events.used.outcomes.Refuse]
doc = \"Not used.\"
returns = 0
",
            "[events.used.args.speed]
doc = \"A float.\"
arg = 5
mask = 1
",
        ] {
            assert!(decided(&changing, wrong).is_err(), "{wrong}");
        }
        let at_a_location = "[events.seen]
doc = \"Seen.\"
at = \"action.manager.used\"
domain = \"actions\"

[events.seen.args.value]
doc = \"A value.\"
register = \"rcx\"
kind = \"u32\"

[events.seen.outcomes.skip]
doc = \"Skipped.\"
";
        assert!(
            decided(&changing, at_a_location).is_err(),
            "a location decides nothing"
        );
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

    #[test]
    fn derived_events_take_the_hosts_values_and_outcomes_with_text() {
        let derived = |events: &str| {
            Map::from_sources(
                "format = 2",
                &[
                    (
                        "entries/text.toml".into(),
                        r#"
[function."text.store_row"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "ptr" }
thread = "any"

[location."text.stored"]
locator = { signature = "C3 90" }
thread = "any"
"#
                        .into(),
                    ),
                    (
                        "domains.toml".into(),
                        r#"
[domains.text]
doc = "Text."
class = "display"
read = { en = "Sees text", ru = "Видит тексты" }
change = { en = "Changes text", ru = "Меняет тексты" }
"#
                        .into(),
                    ),
                    ("events.toml".into(), events.into()),
                ],
            )
        };
        let valid = r#"
[events.sheet_text]
doc = "A cell."
at = "text.store_row"
derived = "sheet_cell"
domain = "text"
rules = "text-table-v1"

[events.sheet_text.args.row]
doc = "The row."
from = "row"

[events.sheet_text.args.text]
doc = "The game's text."
from = "text"
text = 4096

[events.sheet_text.outcomes.replace]
doc = "The rule's text."
text = 65535
same_length = "text"
"#;
        let map = derived(valid).unwrap();
        let event = &map.objects().events["sheet_text"];
        assert_eq!(event.derived.as_deref(), Some("sheet_cell"));
        assert_eq!(event.rules.as_deref(), Some("text-table-v1"));
        assert_eq!(event.args["row"].kind, ValueKind::U32);
        assert_eq!(event.args["row"].source, ArgSource::Derived("row".into()));
        assert_eq!(event.outcomes["replace"].text, Some(65535));
        assert_eq!(
            event.outcomes["replace"].same_length.as_deref(),
            Some("text")
        );

        for (old, new) in [
            ("derived = \"sheet_cell\"", "derived = \"sheet_rows\""),
            ("from = \"row\"", "from = \"page\""),
            ("from = \"row\"", "arg = 1"),
            (
                "from = \"text\"\ntext = 4096",
                "from = \"text\"\ntext = 8192",
            ),
            ("from = \"row\"", "from = \"text\""),
            ("from = \"text\"\ntext = 4096", "from = \"row\"\ntext = 16"),
            ("rules = \"text-table-v1\"", "rules = \"csv\""),
            ("text = 65535\n", "returns = 0\n"),
            ("text = 65535\n", "text = 70000\n"),
            ("same_length = \"text\"", "same_length = \"row\""),
            ("at = \"text.store_row\"", "at = \"text.stored\""),
        ] {
            let events = valid.replace(old, new);
            assert!(derived(&events).is_err(), "{old} -> {new}");
        }
        // An event the game calls has no `from` and takes no rule files.
        let called = valid.replace("derived = \"sheet_cell\"\n", "");
        assert!(derived(&called).is_err());
        let called = called
            .replace("rules = \"text-table-v1\"\n", "")
            .replace("from = \"row\"", "arg = 1");
        assert!(
            derived(&called).is_err(),
            "an outcome with text at a called event"
        );
    }
}
