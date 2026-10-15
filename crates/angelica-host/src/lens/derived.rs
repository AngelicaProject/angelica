//! Rules at derived decision points: events of the map whose arguments the
//! host's adapter derives at the function it hooks, such as each text cell
//! of a sheet row the game builds, instead of reading registers. A rule
//! there is equality conditions on the event's arguments and an outcome
//! with the text it gives; the adapter looks rules up by those values, so
//! many rules cost a lookup, not a scan. Platform independent: the
//! adapters of each derivation apply the rules.
#![forbid(unsafe_code)]
use super::query::{self, Filter, Literal, Op, Test};
use super::read::Value;
use super::schema::Schema;
use crate::error::{Error, Kind, Result};
use angelica_map::{Event, Meaning, Shape};
use std::collections::BTreeMap;

/// The value of an equality condition, as the adapter compares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Integer(i128),
    Text(Vec<u8>),
}

/// A rule at a derived decision point, checked against the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub event: String,
    pub derivation: String,
    /// The function entry the derivation hooks.
    pub at: String,
    /// Each condition, by the derived value it compares: the argument's
    /// `from`.
    pub conditions: BTreeMap<String, Key>,
    pub outcome: String,
    /// The text the outcome takes.
    pub text: Vec<u8>,
}

/// Whether `text` names a derived event of the map: then the rule is
/// planned by [`prepare`], not as a query.
#[must_use]
pub fn event<'a>(schema: &'a Schema, text: &str) -> Option<&'a Event> {
    let parsed = query::parse(text).ok()?;
    schema
        .objects
        .events
        .get(&parsed.root)
        .filter(|event| event.derived.is_some())
}

/// Checks a rule at a derived decision point: `text` is the event with
/// equality conditions on its arguments joined by `and`, such as
/// `sheet_text[sheet = "Item" and row = 4 and column = 9]`; `outcome` one
/// of the event's outcomes and `value` the text it takes. `may_change`
/// says whether the plugin may change a domain, `may_read` whether it may
/// read one.
///
/// # Errors
/// `invalid-argument` for anything but equality conditions, a value of the
/// wrong shape, or text that is not the game's text; `not-found` for an
/// unknown event, argument, or outcome; `permission-denied` for a domain
/// the plugin may not change or read; `unavailable` for an event the build
/// lacks.
pub fn prepare(
    schema: &Schema,
    text: &str,
    outcome: &str,
    value: Option<&Value>,
    may_read: &dyn Fn(&str) -> bool,
    may_change: &dyn Fn(&str) -> bool,
) -> Result<Rule> {
    let parsed = query::parse(text).map_err(Error::invalid)?;
    let Some(event) = event(schema, text) else {
        return Err(Error::new(
            Kind::NotFound,
            format!("the game map has no derived event {}", parsed.root),
        ));
    };
    if !parsed.path.is_empty() || parsed.select.is_some() {
        return Err(Error::invalid(format!(
            "a rule at {} is the event with conditions in brackets, such as {}[a = 1 and b = \"x\"]",
            event.name, event.name
        )));
    }
    if !may_change(&event.domain) {
        return Err(Error::new(
            Kind::PermissionDenied,
            format!(
                "deciding at {} changes domain {}, which the plugin may not change: list it in the manifest's [lens] change",
                event.name, event.domain
            ),
        ));
    }
    if let Err(reason) = schema
        .events
        .get(&event.name)
        .cloned()
        .unwrap_or_else(|| Err("the event is not in the schema".into()))
    {
        return Err(Error::new(
            Kind::Unavailable,
            format!(
                "event {} is not available in this game build: {reason}",
                event.name
            ),
        ));
    }
    let conditions = conditions(schema, event, parsed.filter.as_ref(), may_read, may_change)?;
    let Some(found) = event.outcomes.get(outcome) else {
        let known = event
            .outcomes
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::new(
            Kind::NotFound,
            format!(
                "event {} has no outcome {outcome}; its outcomes: {known}",
                event.name
            ),
        ));
    };
    let text = outcome_text(event, found, value, &conditions)?;
    Ok(Rule {
        event: event.name.clone(),
        derivation: event.derived.clone().unwrap_or_default(),
        at: event.at.clone(),
        conditions,
        outcome: outcome.to_owned(),
        text,
    })
}

/// The rule's conditions by derived value: each an argument `=` a value.
fn conditions(
    schema: &Schema,
    event: &Event,
    filter: Option<&Filter>,
    may_read: &dyn Fn(&str) -> bool,
    may_change: &dyn Fn(&str) -> bool,
) -> Result<BTreeMap<String, Key>> {
    let conditions = match filter {
        None => return Ok(BTreeMap::new()),
        Some(Filter::Index(_)) => {
            return Err(Error::invalid(format!(
                "{} is an event, not an array: it has no index",
                event.name
            )));
        }
        Some(Filter::Conditions(conditions)) => conditions,
    };
    let mut keys = BTreeMap::new();
    for condition in conditions {
        let [name] = condition.field.as_slice() else {
            return Err(Error::invalid(format!(
                "a rule at {} compares the event's own arguments",
                event.name
            )));
        };
        let Some(arg) = event.args.get(name) else {
            let known = event.args.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(Error::new(
                Kind::NotFound,
                format!(
                    "{} has no argument {name}; its arguments: {known}",
                    event.name
                ),
            ));
        };
        if !may_read(&arg.domain) && !may_change(&arg.domain) {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!(
                    "the rule reads {}, which the plugin may neither read nor change: list it in the manifest's [lens] read",
                    arg.domain
                ),
            ));
        }
        let Test::Compare(Op::Equal, literal) = &condition.test else {
            return Err(Error::invalid(format!(
                "a rule at {} compares with = only: the host looks its rules up by their values",
                event.name
            )));
        };
        let key = key(
            schema,
            &format!("{}.{name}", event.name),
            &arg.shape,
            literal,
        )?;
        let angelica_map::ArgSource::Derived(from) = &arg.source else {
            unreachable!("the map checks that a derived event's arguments are derived");
        };
        if keys.insert(from.clone(), key).is_some() {
            return Err(Error::invalid(format!(
                "{}.{name} is compared twice",
                event.name
            )));
        }
    }
    Ok(keys)
}

/// A literal as the value of an argument of `shape`.
fn key(schema: &Schema, name: &str, shape: &Shape, literal: &Literal) -> Result<Key> {
    let wrong = |what: &str| Err(Error::invalid(format!("{name} {what}")));
    match (shape, literal) {
        (Shape::Text(_), Literal::Text(text)) => Ok(Key::Text(text.as_bytes().to_vec())),
        (Shape::Text(_), _) => wrong("is text: compare it with \"text\""),
        (Shape::Value { meaning, .. }, Literal::Name(value)) => match meaning {
            Meaning::Enum(enum_name) => {
                let values = &schema.objects.enums[enum_name].values;
                values
                    .get(value)
                    .map(|number| Key::Integer(i128::from(*number)))
                    .ok_or_else(|| {
                        let known = values.keys().cloned().collect::<Vec<_>>().join(", ");
                        Error::invalid(format!("{name} has no value {value}; its values: {known}"))
                    })
            }
            _ => wrong(&format!("has no named values, so {value} means nothing")),
        },
        (Shape::Value { .. }, Literal::Integer(value)) => Ok(Key::Integer(*value)),
        (Shape::Value { .. }, _) => wrong("is a whole number"),
        _ => wrong("is not compared"),
    }
}

/// The text an outcome takes: the game's text, UTF-8 and the game's macros
/// by their structure, without NUL, within the outcome's length and, when
/// the outcome says so, as long as an argument's compared value.
fn outcome_text(
    event: &Event,
    outcome: &angelica_map::Outcome,
    value: Option<&Value>,
    conditions: &BTreeMap<String, Key>,
) -> Result<Vec<u8>> {
    let name = format!("{}.{}", event.name, outcome.name);
    let Some(Value::Text(text)) = value else {
        return Err(Error::invalid(format!("{name} takes text")));
    };
    let most = outcome.text.unwrap_or_default();
    if text.len() > usize::try_from(most).unwrap_or(usize::MAX) {
        return Err(Error::new(
            Kind::LimitExceeded,
            format!("{name} takes text of at most {most} bytes"),
        ));
    }
    if text.contains(&0) || !angelica_text::sestring::is_cell_text(text) {
        return Err(Error::invalid(format!(
            "the text for {name} is not the game's text: UTF-8 and well-formed macros, without NUL"
        )));
    }
    if text.starts_with(b"_rsv_") {
        return Err(Error::invalid(format!(
            "the text for {name} begins with _rsv_, which the game resolves itself"
        )));
    }
    if let Some(arg) = &outcome.same_length {
        let Some(angelica_map::ArgSource::Derived(from)) =
            event.args.get(arg).map(|arg| &arg.source)
        else {
            unreachable!("the map checks `same_length`");
        };
        match conditions.get(from) {
            Some(Key::Text(compared)) if compared.len() == text.len() => {}
            Some(_) => {
                return Err(Error::invalid(format!(
                    "the text for {name} has as many bytes as {}.{arg}",
                    event.name
                )));
            }
            None => {
                return Err(Error::invalid(format!(
                    "a rule for {name} names {}.{arg} with =",
                    event.name
                )));
            }
        }
    }
    Ok(text.clone())
}

/// The rule's value compared to `from`: an integer.
#[must_use]
pub fn integer(rule: &Rule, from: &str) -> Option<i128> {
    match rule.conditions.get(from) {
        Some(Key::Integer(value)) => Some(*value),
        _ => None,
    }
}

/// The rule's value compared to `from`: text.
#[must_use]
pub fn text<'a>(rule: &'a Rule, from: &str) -> Option<&'a [u8]> {
    match rule.conditions.get(from) {
        Some(Key::Text(value)) => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_map::{EntryKind, Map, Value as Resolved};

    const SOURCES: [(&str, &str); 4] = [
        (
            "entries/text.toml",
            r#"
[function."text.store_row"]
locator = { signature = "C3" }
abi = { args = ["ptr", "ptr"], ret = "ptr" }
thread = "any"

[function."text.to_upper"]
locator = { signature = "C3" }
abi = { args = ["ptr"], ret = "ptr" }
thread = "any"
"#,
        ),
        (
            "domains.toml",
            r#"
[domains.text]
doc = "Text."
class = "display"
read = { en = "Sees text", ru = "Видит тексты" }
change = { en = "Changes text", ru = "Меняет тексты" }
"#,
        ),
        (
            "enums/text.language.toml",
            "doc = \"Languages.\"\nvalues = { english = 1, german = 3 }\n",
        ),
        (
            "events.toml",
            r#"
[events.sheet_text]
doc = "A cell."
at = "text.store_row"
derived = "sheet_cell"
domain = "text"
rules = "text-table-v1"

[events.sheet_text.args.sheet]
doc = "The sheet."
from = "sheet"
text = 256

[events.sheet_text.args.language]
doc = "The language."
from = "language"
enum = "text.language"

[events.sheet_text.args.row]
doc = "The row."
from = "row"

[events.sheet_text.args.text]
doc = "The game's text."
from = "text"
text = 4096

[events.sheet_text.outcomes.replace]
doc = "The rule's text."
text = 16

[events.letter_upper]
doc = "A letter."
at = "text.to_upper"
derived = "case_letter"
domain = "text"

[events.letter_upper.args.letter]
doc = "The letter."
from = "letter"
text = 4

[events.letter_upper.outcomes.becomes]
doc = "The rule's letter."
text = 4
same_length = "letter"
"#,
        ),
    ];

    fn schema() -> Schema {
        let files: Vec<(String, String)> = SOURCES
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect();
        let map = Map::from_sources("format = 2", &files).unwrap();
        let entries = map.clone();
        Schema::new(&map, 0, move |name| {
            Ok(match &entries.get(name).unwrap().kind {
                EntryKind::Field { offset, .. } => Resolved::Offset(offset.unwrap()),
                _ => Resolved::Address(0x10),
            })
        })
    }

    fn all(_: &str) -> bool {
        true
    }

    fn text(value: &str) -> Value {
        Value::Text(value.as_bytes().to_vec())
    }

    fn rule(rule: &str, outcome: &str, value: &Value) -> Result<Rule> {
        prepare(&schema(), rule, outcome, Some(value), &all, &all)
    }

    #[test]
    fn a_rule_is_equalities_by_derived_value_and_text_to_take() {
        let found = rule(
            "sheet_text[sheet = \"Item\" and language = german and row = 4 and text = \"Potion\"]",
            "replace",
            &text("Trank"),
        )
        .unwrap();
        assert_eq!(found.derivation, "sheet_cell");
        assert_eq!(found.at, "text.store_row");
        assert_eq!(super::text(&found, "sheet"), Some(b"Item".as_slice()));
        assert_eq!(integer(&found, "language"), Some(3));
        assert_eq!(integer(&found, "row"), Some(4));
        assert_eq!(found.text, b"Trank");
        assert!(event(&schema(), "sheet_text[row = 1]").is_some());
        assert!(event(&schema(), "nothing").is_none());
        let letter = rule("letter_upper[letter = \"я\"]", "becomes", &text("Я")).unwrap();
        assert_eq!(super::text(&letter, "letter"), Some("я".as_bytes()));
    }

    #[test]
    fn rules_that_are_not_equalities_or_do_not_fit_are_refused() {
        let kind = |rule_text: &str, outcome: &str, value: Value| {
            rule(rule_text, outcome, &value).unwrap_err().kind
        };
        let potion = || text("Trank");
        for wrong in [
            "sheet_text[row > 4]",
            "sheet_text[row in (1, 2)]",
            "sheet_text[row = 1 and row = 2]",
            "sheet_text[row = \"one\"]",
            "sheet_text[sheet = 4]",
            "sheet_text[language = french]",
            "sheet_text[0]",
            "sheet_text.sheet",
        ] {
            assert_eq!(
                kind(wrong, "replace", potion()),
                Kind::InvalidArgument,
                "{wrong}"
            );
        }
        assert_eq!(
            kind("sheet_text[column = 1]", "replace", potion()),
            Kind::NotFound
        );
        assert_eq!(
            kind("sheet_text[row = 1]", "hide", potion()),
            Kind::NotFound
        );
        assert_eq!(
            kind("letter_lower[letter = \"я\"]", "becomes", text("Я")),
            Kind::NotFound
        );
        assert_eq!(
            kind("sheet_text[row = 1]", "replace", Value::Unsigned(1)),
            Kind::InvalidArgument
        );
        assert_eq!(
            kind("sheet_text[row = 1]", "replace", text("seventeen bytes!!")),
            Kind::LimitExceeded
        );
        assert_eq!(
            kind("sheet_text[row = 1]", "replace", Value::Text(vec![0xFF])),
            Kind::InvalidArgument,
            "not UTF-8"
        );
        assert_eq!(
            kind("sheet_text[row = 1]", "replace", text("_rsv_x")),
            Kind::InvalidArgument
        );
        assert_eq!(
            kind("letter_upper[letter = \"я\"]", "becomes", text("Z")),
            Kind::InvalidArgument,
            "another length"
        );
        assert_eq!(
            kind("letter_upper", "becomes", text("Я")),
            Kind::InvalidArgument,
            "the letter it gives one for"
        );
        let unchanging = |domain: &str| domain != "text";
        assert_eq!(
            prepare(
                &schema(),
                "sheet_text[row = 1]",
                "replace",
                Some(&potion()),
                &all,
                &unchanging
            )
            .unwrap_err()
            .kind,
            Kind::PermissionDenied
        );
    }

    #[test]
    fn a_derived_event_is_never_watched() {
        let error = super::super::prepare_watch(&schema(), "sheet_text { row }", &all).unwrap_err();
        assert_eq!(error.kind, Kind::InvalidArgument);
        assert!(error.message.contains("derived by the host"));
    }
}
