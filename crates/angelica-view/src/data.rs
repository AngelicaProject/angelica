//! Places and parts as the `ui` interface carries them: a `kind` and named
//! properties. [`encode_places`], [`encode`], and [`encode_popup`] make that data from typed
//! places; [`decode`] and [`decode_popup`] read it back, refusing anything
//! the plugin API does not list for the plugin's API version. Events go
//! the same way with [`encode_event`] and [`decode_event`].
use crate::{
    Action, ActionKind, Anchor, Backing, Button, Checkbox, Choice, Chosen, Countdown, Dialog,
    Edited, Entry, Event, Fact, Icon, Mark, Meter, Navigated, Number, NumberSet, Panel, Place,
    Popup, PopupAt, Readout, Ring, Role, Section, Shown, Sparkline, Status, Table, Tabs, Text,
    TextInput, Toggled, Tone, Widget, WidgetRef, Window,
};

/// The value of a property.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Integer(i64),
    Flag(bool),
    Texts(Vec<String>),
    Numbers(Vec<f64>),
}

/// A part as data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawPart {
    pub kind: String,
    pub id: String,
    pub properties: Vec<(String, Value)>,
}

/// A place as data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RawPlace {
    pub kind: String,
    pub id: String,
    pub properties: Vec<(String, Value)>,
    pub parts: Vec<RawPart>,
}

/// An event as data.
#[derive(Debug, Clone, PartialEq)]
pub struct RawEvent {
    pub kind: String,
    pub place: String,
    pub part: String,
    pub value: Option<Value>,
}

/// Every kind of place and part the plugin API lists, with the minor
/// version of API 1 that added it.
pub fn kinds() -> impl Iterator<Item = (&'static str, u16)> {
    KINDS.iter().map(|(name, since, _)| (*name, *since))
}

/// The properties of `kind`, each with the minor version of API 1 that
/// added it; empty for a kind the plugin API does not list.
#[must_use]
pub fn properties(kind: &str) -> Vec<(&'static str, u16)> {
    KINDS
        .iter()
        .find(|(name, ..)| *name == kind)
        .map(|(_, _, properties)| properties.to_vec())
        .unwrap_or_default()
}

/// The minor version of plugin API 1 that added each kind, with the
/// minor version that added each of its properties.
type Kind = (&'static str, u16, &'static [(&'static str, u16)]);

const KINDS: &[Kind] = &[
    (
        "window",
        0,
        &[("title", 0), ("path", 0), ("status", 0), ("status-tone", 0)],
    ),
    (
        "panel",
        0,
        &[("name", 0), ("backing", 0), ("anchor", 0), ("shown", 0)],
    ),
    (
        "popup",
        0,
        &[("at", 0), ("panel", 0), ("seconds", 0), ("attention", 0)],
    ),
    ("dialog", 0, &[("title", 0), ("window", 0)]),
    ("row", 0, &[]),
    ("column", 0, &[]),
    ("grid", 0, &[("columns", 0)]),
    ("section", 0, &[("title", 0), ("open", 0)]),
    ("table", 0, &[("columns", 0)]),
    (
        "entry",
        0,
        &[
            ("title", 0),
            ("note", 0),
            ("value", 0),
            ("mark", 0),
            ("chosen", 0),
            ("progress", 0),
        ],
    ),
    ("end", 0, &[]),
    ("spacer", 0, &[]),
    ("separator", 0, &[]),
    ("tabs", 0, &[("labels", 0), ("selected", 0)]),
    ("text", 0, &[("text", 0), ("role", 0), ("tone", 0)]),
    ("heading", 0, &[("text", 0)]),
    ("fact", 0, &[("name", 0), ("value", 0)]),
    ("tooltip", 0, &[("text", 0)]),
    ("tile", 0, &[("text", 0)]),
    ("icon", 0, &[("icon", 0), ("size", 0)]),
    (
        "meter",
        0,
        &[
            ("label", 0),
            ("text", 0),
            ("value", 0),
            ("max", 0),
            ("segments", 0),
            ("tone", 0),
        ],
    ),
    (
        "ring",
        0,
        &[
            ("value", 0),
            ("max", 0),
            ("size", 0),
            ("icon", 0),
            ("text", 0),
            ("tone", 0),
        ],
    ),
    (
        "readout",
        0,
        &[
            ("value", 0),
            ("decimals", 0),
            ("unit", 0),
            ("role", 0),
            ("tone", 0),
        ],
    ),
    ("countdown", 0, &[("until", 0), ("done", 0), ("role", 0)]),
    ("sparkline", 0, &[("values", 0), ("height", 0)]),
    (
        "button",
        0,
        &[("label", 0), ("style", 0), ("enabled", 0), ("in-row", 0)],
    ),
    ("checkbox", 0, &[("label", 0), ("checked", 0)]),
    ("text-field", 0, &[("label", 0), ("value", 0), ("hint", 0)]),
    (
        "choice",
        0,
        &[("label", 0), ("options", 0), ("selected", 0)],
    ),
    (
        "number",
        0,
        &[
            ("label", 0),
            ("value", 0),
            ("min", 0),
            ("max", 0),
            ("step", 0),
            ("decimals", 0),
            ("slider", 0),
        ],
    ),
];

/// The parts that take an id.
const ID_KINDS: &[&str] = &[
    "section",
    "table",
    "entry",
    "tabs",
    "readout",
    "button",
    "checkbox",
    "text-field",
    "choice",
    "number",
];

/// The kinds of events.
const EVENTS: &[&str] = &[
    "clicked",
    "toggled",
    "edited",
    "chosen",
    "set",
    "navigated",
    "closed",
    "open-requested",
];

// ---------------------------------------------------------------- encode

/// Collects the properties of one part or place, leaving out those equal
/// to their default.
#[derive(Default)]
struct Props(Vec<(String, Value)>);

impl Props {
    /// A text the kind requires, even when it is empty.
    fn required(mut self, name: &str, value: &str) -> Self {
        self.0.push((name.into(), Value::Text(value.into())));
        self
    }
    fn text(mut self, name: &str, value: &str) -> Self {
        if !value.is_empty() {
            self.0.push((name.into(), Value::Text(value.into())));
        }
        self
    }
    fn word(mut self, name: &str, value: &str, default: &str) -> Self {
        if value != default {
            self.0.push((name.into(), Value::Text(value.into())));
        }
        self
    }
    fn number(mut self, name: &str, value: f64) -> Self {
        self.0.push((name.into(), Value::Number(value)));
        self
    }
    fn integer(mut self, name: &str, value: i64, default: i64) -> Self {
        if value != default {
            self.0.push((name.into(), Value::Integer(value)));
        }
        self
    }
    fn flag(mut self, name: &str, value: bool, default: bool) -> Self {
        if value != default {
            self.0.push((name.into(), Value::Flag(value)));
        }
        self
    }
    fn texts(mut self, name: &str, value: &[String]) -> Self {
        self.0.push((name.into(), Value::Texts(value.to_vec())));
        self
    }
    fn part(self, kind: &str, id: &str) -> RawPart {
        RawPart {
            kind: kind.into(),
            id: id.into(),
            properties: self.0,
        }
    }
}

fn tone_word(tone: Tone) -> &'static str {
    match tone {
        Tone::Normal => "normal",
        Tone::Muted => "muted",
        Tone::Good => "good",
        Tone::Warning => "warning",
        Tone::Bad => "bad",
    }
}

fn role_word(role: Role) -> &'static str {
    match role {
        Role::Body => "body",
        Role::Title => "title",
        Role::Value => "value",
        Role::Big => "big",
        Role::Label => "label",
        Role::Small => "small",
    }
}

fn style_word(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Normal => "normal",
        ActionKind::Main => "main",
        ActionKind::Quiet => "quiet",
        ActionKind::Danger => "danger",
    }
}

fn mark_word(mark: Mark) -> &'static str {
    match mark {
        Mark::None => "none",
        Mark::On => "on",
        Mark::Off => "off",
        Mark::Attention => "attention",
    }
}

fn anchor_word(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::TopLeft => "top-left",
        Anchor::Top => "top",
        Anchor::TopRight => "top-right",
        Anchor::Left => "left",
        Anchor::Center => "center",
        Anchor::Right => "right",
        Anchor::BottomLeft => "bottom-left",
        Anchor::Bottom => "bottom",
        Anchor::BottomRight => "bottom-right",
    }
}

fn shown_word(shown: Shown) -> &'static str {
    match shown {
        Shown::Always => "always",
        Shown::InCombat => "in-combat",
        Shown::OutOfCombat => "out-of-combat",
    }
}

fn button(action: &Action, in_row: bool) -> RawPart {
    Props::default()
        .required("label", &action.label)
        .word("style", style_word(action.kind), "normal")
        .flag("enabled", action.enabled, true)
        .flag("in-row", in_row, false)
        .part("button", &action.id)
}

fn encode_entry(e: &Entry, parts: &mut Vec<RawPart>) {
    let p = Props::default;
    let mut props = p()
        .required("title", &e.title)
        .text("note", &e.note)
        .text("value", &e.value)
        .word("mark", mark_word(e.mark), "none")
        .flag("chosen", e.chosen, false);
    if let Some(progress) = e.progress {
        props = props.number("progress", f64::from(progress));
    }
    parts.push(props.part("entry", &e.id));
    parts.extend(e.actions.iter().map(|a| button(a, true)));
    parts.extend(e.facts.iter().map(|f| {
        p().required("name", &f.name)
            .required("value", &f.value)
            .part("fact", "")
    }));
    parts.extend(e.more.iter().map(|a| button(a, false)));
    parts.push(p().part("end", ""));
}

/// The parts that show values, as data.
fn encode_value(widget: &Widget) -> Option<RawPart> {
    let p = Props::default;
    Some(match widget {
        Widget::Meter(m) => p()
            .text("label", &m.label)
            .text("text", &m.text)
            .number("value", m.value)
            .number("max", m.max)
            .integer("segments", i64::from(m.segments), 0)
            .word("tone", tone_word(m.tone), "normal")
            .part("meter", ""),
        Widget::Ring(r) => {
            let mut props = p()
                .number("value", r.value)
                .number("max", r.max)
                .integer("size", i64::from(r.size), 40)
                .text("text", &r.text)
                .word("tone", tone_word(r.tone), "normal");
            if let Some(icon) = r.icon {
                props = props.integer("icon", i64::from(icon), -1);
            }
            props.part("ring", "")
        }
        Widget::Readout(r) => p()
            .number("value", r.value)
            .integer("decimals", i64::from(r.decimals), 0)
            .text("unit", &r.unit)
            .word("role", role_word(r.role), "value")
            .word("tone", tone_word(r.tone), "normal")
            .part("readout", &r.id),
        Widget::Countdown(c) => p()
            .integer("until", i64::try_from(c.until).unwrap_or(i64::MAX), -1)
            .word("done", &c.done, "0:00")
            .word("role", role_word(c.role), "value")
            .part("countdown", ""),
        Widget::Sparkline(s) => {
            let mut props = p();
            props.0.push((
                "values".into(),
                Value::Numbers(s.values.iter().map(|&v| f64::from(v)).collect()),
            ));
            props
                .integer("height", i64::from(s.height), 36)
                .part("sparkline", "")
        }
        Widget::Icon(i) => p()
            .integer("icon", i64::from(i.icon), -1)
            .integer("size", i64::from(i.size), 24)
            .part("icon", ""),
        _ => return None,
    })
}

/// The parts that take input, as data.
fn encode_input(widget: &Widget) -> Option<RawPart> {
    let p = Props::default;
    Some(match widget {
        Widget::Button(b) => button(
            &Action {
                id: b.id.clone(),
                label: b.label.clone(),
                kind: b.kind,
                enabled: b.enabled,
            },
            false,
        ),
        Widget::Checkbox(c) => p()
            .required("label", &c.label)
            .flag("checked", c.checked, false)
            .part("checkbox", &c.id),
        Widget::TextInput(t) => p()
            .text("label", &t.label)
            .text("value", &t.value)
            .text("hint", &t.hint)
            .part("text-field", &t.id),
        Widget::Choice(c) => p()
            .text("label", &c.label)
            .texts("options", &c.options)
            .integer("selected", c.selected.map_or(-1, i64::from), -1)
            .part("choice", &c.id),
        Widget::Number(n) => p()
            .text("label", &n.label)
            .number("value", n.value)
            .number("min", n.min)
            .number("max", n.max)
            .number("step", n.step)
            .integer("decimals", i64::from(n.decimals), 0)
            .flag("slider", n.slider, false)
            .part("number", &n.id),
        Widget::Tabs(t) => p()
            .texts("labels", &t.labels)
            .integer("selected", i64::from(t.selected), 0)
            .part("tabs", &t.id),
        _ => return None,
    })
}

fn encode_widget(widget: &Widget, parts: &mut Vec<RawPart>) {
    if let Widget::Entry(entry) = widget {
        encode_entry(entry, parts);
        return;
    }
    if let Some(part) = encode_value(widget).or_else(|| encode_input(widget)) {
        parts.push(part);
        return;
    }
    let p = Props::default;
    parts.push(match widget {
        Widget::Heading(text) => p().required("text", text).part("heading", ""),
        Widget::Text(text) => p()
            .required("text", &text.text)
            .word("role", role_word(text.role), "body")
            .word("tone", tone_word(text.tone), "normal")
            .part("text", ""),
        Widget::Separator => p().part("separator", ""),
        Widget::BeginRow => p().part("row", ""),
        Widget::BeginColumn => p().part("column", ""),
        Widget::BeginGrid(columns) => p()
            .integer("columns", i64::from(*columns), -1)
            .part("grid", ""),
        Widget::BeginSection(s) => p()
            .required("title", &s.title)
            .flag("open", s.open_by_default, true)
            .part("section", &s.id),
        Widget::BeginTable(t) => p().texts("columns", &t.columns).part("table", &t.id),
        Widget::EndRow
        | Widget::EndColumn
        | Widget::EndGrid
        | Widget::EndSection
        | Widget::EndTable => p().part("end", ""),
        Widget::Spacer => p().part("spacer", ""),
        Widget::Tooltip(text) => p().required("text", text).part("tooltip", ""),
        Widget::Tile(text) => p().required("text", text).part("tile", ""),
        Widget::Fact(f) => p()
            .required("name", &f.name)
            .required("value", &f.value)
            .part("fact", ""),
        // Handled above.
        Widget::Entry(_)
        | Widget::Meter(_)
        | Widget::Ring(_)
        | Widget::Readout(_)
        | Widget::Countdown(_)
        | Widget::Sparkline(_)
        | Widget::Icon(_)
        | Widget::Button(_)
        | Widget::Checkbox(_)
        | Widget::TextInput(_)
        | Widget::Choice(_)
        | Widget::Number(_)
        | Widget::Tabs(_) => return,
    });
}

fn encode_widgets(widgets: &[Widget]) -> Vec<RawPart> {
    let mut parts = Vec::with_capacity(widgets.len());
    for widget in widgets {
        encode_widget(widget, &mut parts);
    }
    parts
}

/// A window or a panel as data, without a window's dialog
/// ([`encode_places`] adds it).
#[must_use]
pub fn encode(place: &Place) -> RawPlace {
    match place {
        Place::Window(w) => {
            let mut props = Props::default().required("title", &w.title);
            if !w.path.is_empty() {
                props = props.texts("path", &w.path);
            }
            if let Some(status) = &w.status {
                props = props.text("status", &status.text).word(
                    "status-tone",
                    tone_word(status.tone),
                    "good",
                );
            }
            RawPlace {
                kind: "window".into(),
                id: w.id.clone(),
                properties: props.0,
                parts: encode_widgets(&w.widgets),
            }
        }
        Place::Panel(panel) => RawPlace {
            kind: "panel".into(),
            id: panel.id.clone(),
            properties: Props::default()
                .required("name", &panel.name)
                .word(
                    "backing",
                    match panel.backing {
                        Backing::Plate => "plate",
                        Backing::Bare => "bare",
                    },
                    "plate",
                )
                .word("anchor", anchor_word(panel.anchor), "bottom-left")
                .word("shown", shown_word(panel.shown), "always")
                .0,
            parts: encode_widgets(&panel.widgets),
        },
    }
}

/// Windows and panels as `ui` returns them: each window's dialog follows
/// it as a place of its own that names the window.
#[must_use]
pub fn encode_places(places: &[Place]) -> Vec<RawPlace> {
    let mut raw = Vec::with_capacity(places.len());
    for place in places {
        raw.push(encode(place));
        if let Place::Window(window) = place
            && let Some(dialog) = &window.dialog
        {
            raw.push(encode_dialog(&window.id, dialog));
        }
    }
    raw
}

/// A window's dialog as data.
#[must_use]
pub fn encode_dialog(window: &str, dialog: &Dialog) -> RawPlace {
    RawPlace {
        kind: "dialog".into(),
        id: dialog.id.clone(),
        properties: Props::default()
            .required("title", &dialog.title)
            .required("window", window)
            .0,
        parts: encode_widgets(&dialog.widgets),
    }
}

/// A popup as data.
#[must_use]
pub fn encode_popup(popup: &Popup) -> RawPlace {
    let mut props = Props::default();
    match &popup.at {
        PopupAt::Corner => {}
        PopupAt::Center => props = props.text("at", "center"),
        PopupAt::Panel(panel) => props = props.text("at", "panel").text("panel", panel),
    }
    RawPlace {
        kind: "popup".into(),
        id: String::new(),
        properties: props
            .integer("seconds", i64::from(popup.seconds), 5)
            .flag("attention", popup.attention, false)
            .0,
        parts: encode_widgets(&popup.widgets),
    }
}

/// An event as data.
#[must_use]
pub fn encode_event(event: &Event) -> RawEvent {
    let raw = |kind: &str, place: &str, part: &str, value: Option<Value>| RawEvent {
        kind: kind.into(),
        place: place.into(),
        part: part.into(),
        value,
    };
    match event {
        Event::Clicked(r) => raw("clicked", &r.window, &r.id, None),
        Event::Toggled(t) => raw("toggled", &t.window, &t.id, Some(Value::Flag(t.checked))),
        Event::Edited(e) => raw(
            "edited",
            &e.window,
            &e.id,
            Some(Value::Text(e.value.clone())),
        ),
        Event::Chosen(c) => raw(
            "chosen",
            &c.window,
            &c.id,
            Some(Value::Integer(i64::from(c.index))),
        ),
        Event::Set(s) => raw("set", &s.window, &s.id, Some(Value::Number(s.value))),
        Event::Closed(window) => raw("closed", window, "", None),
        Event::Navigated(n) => raw(
            "navigated",
            &n.window,
            "",
            Some(Value::Integer(i64::from(n.depth))),
        ),
        Event::OpenRequested => raw("open-requested", "", "", None),
    }
}

/// An event read back; `None` for a kind this version does not know, which
/// a plugin ignores.
#[must_use]
pub fn decode_event(raw: &RawEvent) -> Option<Event> {
    let window = raw.place.clone();
    let id = raw.part.clone();
    Some(match (raw.kind.as_str(), &raw.value) {
        ("clicked", _) => Event::Clicked(WidgetRef { window, id }),
        ("toggled", Some(Value::Flag(checked))) => Event::Toggled(Toggled {
            window,
            id,
            checked: *checked,
        }),
        ("edited", Some(Value::Text(value))) => Event::Edited(Edited {
            window,
            id,
            value: value.clone(),
        }),
        ("chosen", Some(Value::Integer(index))) => Event::Chosen(Chosen {
            window,
            id,
            index: u32::try_from(*index).ok()?,
        }),
        ("set", Some(Value::Number(value))) => Event::Set(NumberSet {
            window,
            id,
            value: *value,
        }),
        ("closed", _) => Event::Closed(window),
        ("navigated", Some(Value::Integer(depth))) => Event::Navigated(Navigated {
            window,
            depth: u32::try_from(*depth).ok()?,
        }),
        ("open-requested", _) => Event::OpenRequested,
        _ => return None,
    })
}

/// Whether `kind` is an event this version sends.
#[must_use]
pub fn is_event(kind: &str) -> bool {
    EVENTS.contains(&kind)
}

// ---------------------------------------------------------------- decode

/// The properties of one part or place being read: each is taken once, and
/// what is left over is refused.
struct Reader<'a> {
    what: String,
    kind: &'a str,
    properties: Vec<(&'a str, &'a Value)>,
}

impl<'a> Reader<'a> {
    fn new(
        what: String,
        kind: &'a str,
        minor: u16,
        properties: &'a [(String, Value)],
    ) -> Result<Self, String> {
        let Some((_, since, known)) = KINDS.iter().find(|(name, ..)| *name == kind) else {
            return Err(format!("{what}: no kind {kind:?}"));
        };
        if *since > minor {
            return Err(format!(
                "{what}: {kind} is in plugin API 1.{since}; the manifest says 1.{minor}"
            ));
        }
        let mut seen = Vec::with_capacity(properties.len());
        for (name, value) in properties {
            match known.iter().find(|(known, _)| known == name) {
                None => return Err(format!("{what}: {kind} has no property {name:?}")),
                Some((_, since)) if *since > minor => {
                    return Err(format!(
                        "{what}: {kind}'s {name} is in plugin API 1.{since}; the manifest says 1.{minor}"
                    ));
                }
                Some(_) => {}
            }
            if seen.iter().any(|(seen, _)| *seen == name.as_str()) {
                return Err(format!("{what}: {name} is given twice"));
            }
            seen.push((name.as_str(), value));
        }
        Ok(Self {
            what,
            kind,
            properties: seen,
        })
    }

    fn take(&mut self, name: &str) -> Option<&'a Value> {
        let at = self.properties.iter().position(|(n, _)| *n == name)?;
        Some(self.properties.remove(at).1)
    }

    fn wrong(&self, name: &str, shape: &str) -> String {
        format!("{}: {}'s {name} is not {shape}", self.what, self.kind)
    }

    fn missing(&self, name: &str) -> String {
        format!("{}: {} needs {name}", self.what, self.kind)
    }

    fn text(&mut self, name: &str) -> Result<Option<String>, String> {
        match self.take(name) {
            None => Ok(None),
            Some(Value::Text(text)) => Ok(Some(text.clone())),
            Some(_) => Err(self.wrong(name, "a text")),
        }
    }

    fn text_or(&mut self, name: &str, default: &str) -> Result<String, String> {
        Ok(self.text(name)?.unwrap_or_else(|| default.into()))
    }

    fn required_text(&mut self, name: &str) -> Result<String, String> {
        self.text(name)?.ok_or_else(|| self.missing(name))
    }

    fn number(&mut self, name: &str) -> Result<Option<f64>, String> {
        match self.take(name) {
            None => Ok(None),
            // An integer is also a number.
            #[allow(clippy::cast_precision_loss)]
            Some(Value::Integer(value)) => Ok(Some(*value as f64)),
            Some(Value::Number(value)) if value.is_finite() => Ok(Some(*value)),
            Some(_) => Err(self.wrong(name, "a finite number")),
        }
    }

    fn required_number(&mut self, name: &str) -> Result<f64, String> {
        self.number(name)?.ok_or_else(|| self.missing(name))
    }

    fn integer(&mut self, name: &str) -> Result<Option<i64>, String> {
        match self.take(name) {
            None => Ok(None),
            Some(Value::Integer(value)) => Ok(Some(*value)),
            Some(_) => Err(self.wrong(name, "an integer")),
        }
    }

    /// An integer within `range`, or `default`.
    fn ranged<T: TryFrom<i64>>(
        &mut self,
        name: &str,
        range: std::ops::RangeInclusive<i64>,
        default: T,
    ) -> Result<T, String> {
        match self.integer(name)? {
            None => Ok(default),
            Some(value) if range.contains(&value) => {
                T::try_from(value).map_err(|_| self.wrong(name, "an integer in range"))
            }
            Some(value) => Err(format!(
                "{}: {}'s {name} is {value}, not {} to {}",
                self.what,
                self.kind,
                range.start(),
                range.end()
            )),
        }
    }

    fn flag(&mut self, name: &str, default: bool) -> Result<bool, String> {
        match self.take(name) {
            None => Ok(default),
            Some(Value::Flag(value)) => Ok(*value),
            Some(_) => Err(self.wrong(name, "a flag")),
        }
    }

    fn texts(&mut self, name: &str) -> Result<Option<Vec<String>>, String> {
        match self.take(name) {
            None => Ok(None),
            Some(Value::Texts(texts)) => Ok(Some(texts.clone())),
            Some(_) => Err(self.wrong(name, "a list of texts")),
        }
    }

    fn numbers(&mut self, name: &str) -> Result<Option<Vec<f64>>, String> {
        match self.take(name) {
            None => Ok(None),
            Some(Value::Numbers(numbers)) if numbers.iter().all(|n| n.is_finite()) => {
                Ok(Some(numbers.clone()))
            }
            Some(_) => Err(self.wrong(name, "a list of finite numbers")),
        }
    }

    /// One of `words`, or the first.
    fn word(&mut self, name: &str, words: &[&str]) -> Result<usize, String> {
        match self.text(name)? {
            None => Ok(0),
            Some(word) => words.iter().position(|w| *w == word).ok_or_else(|| {
                format!(
                    "{}: {}'s {name} is {word:?}, not one of {}",
                    self.what,
                    self.kind,
                    words.join(", ")
                )
            }),
        }
    }

    fn tone(&mut self, name: &str) -> Result<Tone, String> {
        Ok([
            Tone::Normal,
            Tone::Muted,
            Tone::Good,
            Tone::Warning,
            Tone::Bad,
        ][self.word(name, &["normal", "muted", "good", "warning", "bad"])?])
    }

    fn role(&mut self, default: Role) -> Result<Role, String> {
        const ROLES: [Role; 6] = [
            Role::Body,
            Role::Title,
            Role::Value,
            Role::Big,
            Role::Label,
            Role::Small,
        ];
        match self.text("role")? {
            None => Ok(default),
            Some(word) => ["body", "title", "value", "big", "label", "small"]
                .iter()
                .position(|w| *w == word)
                .map(|at| ROLES[at])
                .ok_or_else(|| self.wrong("role", "a role")),
        }
    }

    /// Ends the reading; a property the kind lists but this use of it
    /// does not take is refused.
    fn done(self) -> Result<(), String> {
        match self.properties.first() {
            None => Ok(()),
            Some((name, _)) => Err(format!("{}: {} takes no {name} here", self.what, self.kind)),
        }
    }
}

/// What the parts after a container belong to, while reading.
enum Open {
    Row,
    Column,
    Grid,
    Section,
    Table,
    Entry,
}

fn read_action(reader: &mut Reader<'_>, id: &str) -> Result<(Action, bool), String> {
    let label = reader.required_text("label")?;
    let kind = [
        ActionKind::Normal,
        ActionKind::Main,
        ActionKind::Quiet,
        ActionKind::Danger,
    ][reader.word("style", &["normal", "main", "quiet", "danger"])?];
    let enabled = reader.flag("enabled", true)?;
    let in_row = reader.flag("in-row", false)?;
    Ok((
        Action {
            id: id.into(),
            label,
            kind,
            enabled,
        },
        in_row,
    ))
}

/// Reads the parts of a place into widgets.
/// A container, or the `end` of one.
fn read_container(
    kind: &str,
    r: &mut Reader<'_>,
    id: &str,
    open: &mut Vec<Open>,
) -> Result<Option<Widget>, String> {
    Ok(Some(match kind {
        "row" => {
            open.push(Open::Row);
            Widget::BeginRow
        }
        "column" => {
            open.push(Open::Column);
            Widget::BeginColumn
        }
        "grid" => {
            open.push(Open::Grid);
            let columns: u8 = r.ranged("columns", 2..=8, 0)?;
            if columns == 0 {
                return Err(r.missing("columns"));
            }
            Widget::BeginGrid(columns)
        }
        "section" => {
            open.push(Open::Section);
            Widget::BeginSection(Section {
                id: id.into(),
                title: r.required_text("title")?,
                open_by_default: r.flag("open", true)?,
            })
        }
        "table" => {
            open.push(Open::Table);
            Widget::BeginTable(Table {
                id: id.into(),
                columns: r.texts("columns")?.ok_or_else(|| r.missing("columns"))?,
            })
        }
        "end" => match open.pop() {
            Some(Open::Row) => Widget::EndRow,
            Some(Open::Column) => Widget::EndColumn,
            Some(Open::Grid) => Widget::EndGrid,
            Some(Open::Section) => Widget::EndSection,
            Some(Open::Table) => Widget::EndTable,
            Some(Open::Entry) | None => return Err(format!("{}: an end closes nothing", r.what)),
        },
        _ => return Ok(None),
    }))
}

/// Text and the parts around it.
fn read_text(kind: &str, r: &mut Reader<'_>) -> Result<Option<Widget>, String> {
    Ok(Some(match kind {
        "spacer" => Widget::Spacer,
        "separator" => Widget::Separator,
        "text" => Widget::Text(Text {
            text: r.required_text("text")?,
            role: r.role(Role::Body)?,
            tone: r.tone("tone")?,
        }),
        "heading" => Widget::Heading(r.required_text("text")?),
        "fact" => Widget::Fact(Fact {
            name: r.required_text("name")?,
            value: r.required_text("value")?,
        }),
        "tooltip" => Widget::Tooltip(r.required_text("text")?),
        "tile" => Widget::Tile(r.required_text("text")?),
        _ => return Ok(None),
    }))
}

/// The parts that show values.
fn read_value(kind: &str, r: &mut Reader<'_>, id: &str) -> Result<Option<Widget>, String> {
    Ok(Some(match kind {
        "icon" => Widget::Icon(Icon {
            icon: r.ranged("icon", 0..=i64::from(u32::MAX), u32::MAX)?,
            size: r.ranged("size", 1..=256, 24)?,
        }),
        "meter" => Widget::Meter(Meter {
            label: r.text_or("label", "")?,
            text: r.text_or("text", "")?,
            value: r.required_number("value")?,
            max: r.required_number("max")?,
            segments: r.ranged("segments", 0..=32, 0)?,
            tone: r.tone("tone")?,
        }),
        "ring" => Widget::Ring(Ring {
            value: r.required_number("value")?,
            max: r.required_number("max")?,
            size: r.ranged("size", 16..=128, 40)?,
            icon: r
                .integer("icon")?
                .map(|icon| u32::try_from(icon).map_err(|_| r.wrong("icon", "an icon number")))
                .transpose()?,
            text: r.text_or("text", "")?,
            tone: r.tone("tone")?,
        }),
        "readout" => Widget::Readout(Readout {
            id: id.into(),
            value: r.required_number("value")?,
            decimals: r.ranged("decimals", 0..=6, 0)?,
            unit: r.text_or("unit", "")?,
            role: r.role(Role::Value)?,
            tone: r.tone("tone")?,
        }),
        "countdown" => Widget::Countdown(Countdown {
            until: r.ranged("until", 0..=i64::MAX, u64::MAX)?,
            done: r.text_or("done", "0:00")?,
            role: r.role(Role::Value)?,
        }),
        "sparkline" => Widget::Sparkline(Sparkline {
            #[allow(clippy::cast_possible_truncation)]
            values: r
                .numbers("values")?
                .ok_or_else(|| r.missing("values"))?
                .into_iter()
                .map(|v| v as f32)
                .collect(),
            height: r.ranged("height", 16..=128, 36)?,
        }),
        _ => return Ok(None),
    }))
}

/// The parts that take input.
fn read_input(kind: &str, r: &mut Reader<'_>, id: &str) -> Result<Option<Widget>, String> {
    let id = id.to_owned();
    Ok(Some(match kind {
        "button" => {
            let (action, in_row) = read_action(r, &id)?;
            if in_row {
                return Err(format!(
                    "{}: in-row belongs to a button in an entry",
                    r.what
                ));
            }
            Widget::Button(Button {
                id,
                label: action.label,
                enabled: action.enabled,
                kind: action.kind,
            })
        }
        "checkbox" => Widget::Checkbox(Checkbox {
            id,
            label: r.required_text("label")?,
            checked: r.flag("checked", false)?,
        }),
        "text-field" => Widget::TextInput(TextInput {
            id,
            label: r.text_or("label", "")?,
            value: r.text_or("value", "")?,
            hint: r.text_or("hint", "")?,
        }),
        "choice" => {
            let label = r.text_or("label", "")?;
            let options = r.texts("options")?.ok_or_else(|| r.missing("options"))?;
            let selected: i64 = r.ranged("selected", -1..=i64::from(u32::MAX), -1)?;
            Widget::Choice(Choice {
                id,
                label,
                options,
                selected: u32::try_from(selected).ok(),
            })
        }
        "number" => Widget::Number(Number {
            id,
            label: r.text_or("label", "")?,
            value: r.required_number("value")?,
            min: r.required_number("min")?,
            max: r.required_number("max")?,
            step: r.number("step")?.unwrap_or(0.0),
            decimals: r.ranged("decimals", 0..=6, 0)?,
            slider: r.flag("slider", false)?,
        }),
        "tabs" => Widget::Tabs(Tabs {
            id,
            labels: r.texts("labels")?.ok_or_else(|| r.missing("labels"))?,
            selected: r.ranged("selected", 0..=i64::from(u32::MAX), 0)?,
        }),
        _ => return Ok(None),
    }))
}

/// The row of an entry; its buttons and facts follow up to its `end`.
fn read_entry(r: &mut Reader<'_>, id: &str) -> Result<Entry, String> {
    let progress = r.number("progress")?;
    if progress.is_some_and(|p| !(0.0..=1.0).contains(&p)) {
        return Err(format!("{}: an entry's progress is 0 to 1", r.what));
    }
    Ok(Entry {
        title: r.required_text("title")?,
        note: r.text_or("note", "")?,
        value: r.text_or("value", "")?,
        mark: [Mark::None, Mark::On, Mark::Off, Mark::Attention]
            [r.word("mark", &["none", "on", "off", "attention"])?],
        chosen: r.flag("chosen", false)?,
        #[allow(clippy::cast_possible_truncation)]
        progress: progress.map(|p| p as f32),
        id: id.into(),
        actions: Vec::new(),
        facts: Vec::new(),
        more: Vec::new(),
    })
}

/// A part inside an entry: a button, a fact, or the entry's `end`.
/// Returns whether the entry ended.
fn entry_part(kind: &str, r: &mut Reader<'_>, id: &str, entry: &mut Entry) -> Result<bool, String> {
    match kind {
        "button" => {
            let (action, in_row) = read_action(r, id)?;
            if in_row {
                entry.actions.push(action);
            } else {
                entry.more.push(action);
            }
        }
        "fact" => entry.facts.push(Fact {
            name: r.required_text("name")?,
            value: r.required_text("value")?,
        }),
        "end" => return Ok(true),
        other => {
            return Err(format!(
                "{}: an entry holds buttons and facts, not {other}",
                r.what
            ));
        }
    }
    Ok(false)
}

/// Reads the parts of a place into widgets.
fn decode_parts(place: &str, parts: &[RawPart], minor: u16) -> Result<Vec<Widget>, String> {
    let mut widgets = Vec::with_capacity(parts.len());
    let mut open: Vec<Open> = Vec::new();
    // The entry being filled by the parts inside it.
    let mut entry: Option<Entry> = None;
    for (index, part) in parts.iter().enumerate() {
        let what = format!("{place}: part {index}");
        let kind = part.kind.as_str();
        let mut r = Reader::new(what.clone(), kind, minor, &part.properties)?;
        let id = part.id.as_str();
        let takes_id = ID_KINDS.contains(&kind);
        if takes_id == id.is_empty() {
            return Err(if takes_id {
                format!("{what}: a {kind} needs an id")
            } else {
                format!("{what}: a {kind} has no id")
            });
        }
        if let Some(current) = entry.as_mut() {
            if entry_part(kind, &mut r, id, current)? {
                open.pop();
                widgets.extend(entry.take().map(Widget::Entry));
            }
            r.done()?;
            continue;
        }
        if kind == "entry" {
            open.push(Open::Entry);
            entry = Some(read_entry(&mut r, id)?);
            r.done()?;
            continue;
        }
        let widget = match read_container(kind, &mut r, id, &mut open)? {
            Some(widget) => widget,
            None => match read_text(kind, &mut r)? {
                Some(widget) => widget,
                None => match read_value(kind, &mut r, id)? {
                    Some(widget) => widget,
                    None => read_input(kind, &mut r, id)?
                        .ok_or_else(|| format!("{what}: {kind} is a place, not a part"))?,
                },
            },
        };
        r.done()?;
        widgets.push(widget);
    }
    if !open.is_empty() {
        return Err(format!("{place}: a container is not ended"));
    }
    Ok(widgets)
}

/// Reads the windows and panels `ui` returned, for a plugin of API 1.`minor`,
/// and checks them ([`crate::check`]).
///
/// # Errors
/// Names the first place or part that breaks a rule.
pub fn decode(places: &[RawPlace], minor: u16) -> Result<Vec<Place>, String> {
    let mut decoded = Vec::with_capacity(places.len());
    let mut dialogs = Vec::new();
    for place in places {
        let what = format!("{} {:?}", place.kind, place.id);
        let mut r = Reader::new(what.clone(), &place.kind, minor, &place.properties)?;
        let widgets = decode_parts(&what, &place.parts, minor)?;
        decoded.push(match place.kind.as_str() {
            "window" => {
                let title = r.required_text("title")?;
                let path = r.texts("path")?.unwrap_or_default();
                let status_text = r.text("status")?;
                let tone = match r.text("status-tone")? {
                    None => Tone::Good,
                    Some(word) => [
                        Tone::Normal,
                        Tone::Muted,
                        Tone::Good,
                        Tone::Warning,
                        Tone::Bad,
                    ][["normal", "muted", "good", "warning", "bad"]
                        .iter()
                        .position(|w| *w == word)
                        .ok_or_else(|| r.wrong("status-tone", "a tone"))?],
                };
                Place::Window(Window {
                    id: place.id.clone(),
                    title,
                    path,
                    status: status_text.map(|text| Status { text, tone }),
                    widgets,
                    dialog: None,
                })
            }
            "dialog" => {
                let title = r.required_text("title")?;
                let window = r.required_text("window")?;
                r.done()?;
                dialogs.push((
                    window,
                    Dialog {
                        id: place.id.clone(),
                        title,
                        widgets,
                    },
                ));
                continue;
            }
            "panel" => Place::Panel(Panel {
                id: place.id.clone(),
                name: r.required_text("name")?,
                backing: [Backing::Plate, Backing::Bare][r.word("backing", &["plate", "bare"])?],
                anchor: {
                    const ANCHORS: [Anchor; 9] = [
                        Anchor::BottomLeft,
                        Anchor::TopLeft,
                        Anchor::Top,
                        Anchor::TopRight,
                        Anchor::Left,
                        Anchor::Center,
                        Anchor::Right,
                        Anchor::Bottom,
                        Anchor::BottomRight,
                    ];
                    ANCHORS[r.word(
                        "anchor",
                        &[
                            "bottom-left",
                            "top-left",
                            "top",
                            "top-right",
                            "left",
                            "center",
                            "right",
                            "bottom",
                            "bottom-right",
                        ],
                    )?]
                },
                shown: [Shown::Always, Shown::InCombat, Shown::OutOfCombat]
                    [r.word("shown", &["always", "in-combat", "out-of-combat"])?],
                widgets,
            }),
            "popup" => {
                return Err(format!(
                    "{what}: a popup is shown with notifications.pop-up"
                ));
            }
            other => return Err(format!("{what}: {other} is a part, not a place")),
        });
        r.done()?;
    }
    attach_dialogs(&mut decoded, dialogs)?;
    crate::check::places(&decoded)?;
    Ok(decoded)
}

/// Gives each dialog to the window it names, which `ui` returned with it.
fn attach_dialogs(decoded: &mut [Place], dialogs: Vec<(String, Dialog)>) -> Result<(), String> {
    for (owner, dialog) in dialogs {
        let window = decoded.iter_mut().find_map(|place| match place {
            Place::Window(window) if window.id == owner => Some(window),
            _ => None,
        });
        let Some(window) = window else {
            return Err(format!(
                "dialog {:?}: no window {owner:?} in the same ui",
                dialog.id
            ));
        };
        if window.dialog.is_some() {
            return Err(format!("window {owner:?}: more than one dialog"));
        }
        window.dialog = Some(dialog);
    }
    Ok(())
}

/// Reads a popup for `notifications.pop-up` and checks it.
///
/// # Errors
/// Names the first rule it breaks.
pub fn decode_popup(place: &RawPlace, minor: u16) -> Result<Popup, String> {
    let what = "the popup".to_owned();
    if place.kind != "popup" {
        return Err(format!("{what} is a {}, not a popup", place.kind));
    }
    let mut r = Reader::new(what.clone(), &place.kind, minor, &place.properties)?;
    let widgets = decode_parts(&what, &place.parts, minor)?;
    let at = match r.word("at", &["corner", "center", "panel"])? {
        0 => {
            if r.text("panel")?.is_some() {
                return Err(format!("{what}: panel is given only with at = panel"));
            }
            PopupAt::Corner
        }
        1 => {
            if r.text("panel")?.is_some() {
                return Err(format!("{what}: panel is given only with at = panel"));
            }
            PopupAt::Center
        }
        _ => PopupAt::Panel(r.required_text("panel")?),
    };
    let popup = Popup {
        at,
        seconds: r.ranged("seconds", 2..=10, 5)?,
        attention: r.flag("attention", false)?,
        widgets,
    };
    r.done()?;
    crate::check::popup(&popup)?;
    Ok(popup)
}
