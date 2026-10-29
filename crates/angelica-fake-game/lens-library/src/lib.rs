//! A test plugin that reads, watches, and decides through the Lens library.
//! The host's tests click a button and the log tells what came of it:
//!
//! - `query:<query>`: the rows of a query;
//! - `watch:<query>`: watches an event; `take`: the rows of the firings
//!   since;
//! - `rule:<outcome>:<query>`: a rule at an event; `decided`: how many
//!   calls it decided.
use angelica_lens::{Query, Row, Rule, Value, Watch};
use angelica_sdk::{
    Error, Plugin,
    intercepts::Call,
    ui::{Event, Window},
};

#[derive(Default)]
struct LensLibrary {
    watches: Vec<Watch>,
    rules: Vec<Rule>,
}

fn kind(error: &Error) -> String {
    angelica_sdk::warning(&format!("why: {}", error.message));
    let kind = format!("{:?}", error.kind);
    kind.trim_start_matches("ErrorKind::").to_owned()
}

/// A value as the tests compare it.
fn value(value: Option<&Value>) -> String {
    match value {
        None => "-".into(),
        Some(Value::Text(text)) => String::from_utf8_lossy(text).into_owned(),
        Some(Value::Unsigned(number)) => number.to_string(),
        Some(Value::Signed(number)) => number.to_string(),
        Some(Value::Boolean(flag)) => flag.to_string(),
        Some(Value::Float(number)) => number.to_string(),
        Some(Value::Named(name)) => name.clone(),
        Some(Value::Sheet(row)) => format!("{}#{}", row.sheet, row.row),
        Some(Value::Flags(flags)) => flags.names.join("|"),
    }
}

fn rows(rows: &[Row]) -> String {
    rows.iter()
        .map(|row| {
            row.values
                .iter()
                .map(|v| value(v.as_ref()))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

impl LensLibrary {
    fn clicked(&mut self, id: &str) -> String {
        if let Some(text) = id.strip_prefix("query:") {
            return format!(
                "query: {}",
                Query::prepare(text)
                    .and_then(|query| query.run())
                    .map_or_else(|error| kind(&error), |found| rows(&found))
            );
        }
        if let Some(text) = id.strip_prefix("watch:") {
            return match Watch::new(text) {
                Ok(watch) => {
                    self.watches.push(watch);
                    "watch: accepted".into()
                }
                Err(error) => format!("watch: {}", kind(&error)),
            };
        }
        if let Some(rest) = id.strip_prefix("rule:") {
            let (outcome, text) = rest.split_once(':').unwrap_or((rest, ""));
            return match Rule::new(text, outcome) {
                Ok(rule) => {
                    self.rules.push(rule);
                    "rule: accepted".into()
                }
                Err(error) => format!("rule: {}", kind(&error)),
            };
        }
        match id {
            "take" => {
                let taken: Vec<String> = self
                    .watches
                    .iter()
                    .map(|watch| {
                        watch
                            .take()
                            .map_or_else(|error| kind(&error), |found| rows(&found))
                    })
                    .collect();
                format!("take: {}", taken.join(" | "))
            }
            "decided" => format!(
                "decided: {}",
                self.rules
                    .iter()
                    .map(|rule| rule.decided().to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            _ => format!("{id}: unknown"),
        }
    }
}

impl Plugin for LensLibrary {
    fn start() -> Result<Self, String> {
        Ok(Self::default())
    }

    fn ui(&mut self) -> Vec<Window> {
        vec![Window::new("main", "Lens library")]
    }

    fn ui_event(&mut self, event: Event) {
        if let Event::Clicked(target) = event {
            angelica_sdk::info(&self.clicked(&target.id));
        }
    }

    fn prepare() -> Result<(), String> {
        angelica_lens::prepare()
    }

    fn intercepted(call: Call) {
        angelica_lens::intercepted(&call);
    }
}

angelica_sdk::plugin!(LensLibrary);
