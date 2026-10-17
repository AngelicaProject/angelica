//! What the plugin shows: windows and panels from [`crate::Plugin::ui`] and
//! [`crate::Plugin::panels`], and popups from [`crate::notifications`],
//! built from the same parts.
//!
//! ```ignore
//! Window::new("main", "Translations")
//!     .heading("Packs")
//!     .tabs("view", &["Installed", "Available"], self.tab)
//!     .table("packs", &["Pack", "Size"], |table| {
//!         table.text("Russian").tooltip("Community translation").text("12 MB")
//!     })
//!     .slider("opacity", "Opacity", self.opacity, 0.0, 1.0)
//!     .row(|row| row.button("import", "Import").button("remove", "Remove"))
//!     .section("details", "Details", false, |section| section.muted("..."));
//!
//! Panel::new("health", "Health")
//!     .bare()
//!     .shown(Shown::InCombat)
//!     .meter(Meter::new(hp, max_hp).label("HP").text(format!("{hp} / {max_hp}")));
//! ```
pub use angelica_view::{
    Action, ActionKind, Anchor, Backing, Button, Checkbox, Choice, Chosen, Countdown, Dialog,
    Edited, Entry, Event, Fact, Icon, Mark, MenuChosen, MenuEntry, MenuFor, Meter, Navigated,
    Number, NumberSet, Panel, Popup, PopupAt, Readout, Ring, Role, Section, Shown, Sparkline,
    Split, Status, Table, Tabs, Text, TextInput, Toggled, Tone, Widget, WidgetRef, Window,
};

use crate::bindings::angelica::plugin::ui as wit;
use angelica_view::data::{RawEvent, RawPlace, Value};

fn value_to_wit(value: Value) -> wit::Value {
    match value {
        Value::Text(text) => wit::Value::Text(text),
        Value::Number(number) => wit::Value::Number(number),
        Value::Integer(integer) => wit::Value::Integer(integer),
        Value::Flag(flag) => wit::Value::Flag(flag),
        Value::Texts(texts) => wit::Value::Texts(texts),
        Value::Numbers(numbers) => wit::Value::Numbers(numbers),
    }
}

fn value_from_wit(value: wit::Value) -> Value {
    match value {
        wit::Value::Text(text) => Value::Text(text),
        wit::Value::Number(number) => Value::Number(number),
        wit::Value::Integer(integer) => Value::Integer(integer),
        wit::Value::Flag(flag) => Value::Flag(flag),
        wit::Value::Texts(texts) => Value::Texts(texts),
        wit::Value::Numbers(numbers) => Value::Numbers(numbers),
    }
}

fn properties(properties: Vec<(String, Value)>) -> Vec<wit::Property> {
    properties
        .into_iter()
        .map(|(name, value)| wit::Property {
            name,
            value: value_to_wit(value),
        })
        .collect()
}

#[doc(hidden)]
#[must_use]
pub fn place_to_wit(place: RawPlace) -> wit::Place {
    wit::Place {
        kind: place.kind,
        id: place.id,
        properties: properties(place.properties),
        parts: place
            .parts
            .into_iter()
            .map(|part| wit::Part {
                kind: part.kind,
                id: part.id,
                properties: properties(part.properties),
            })
            .collect(),
    }
}

/// The windows with their dialogs, the panels, and the menu entries, as
/// `ui` returns them.
#[doc(hidden)]
#[must_use]
pub fn view_to_wit(
    windows: Vec<Window>,
    panels: Vec<Panel>,
    entries: Vec<MenuEntry>,
) -> Vec<wit::Place> {
    let places: Vec<_> = windows
        .into_iter()
        .map(angelica_view::Place::Window)
        .chain(panels.into_iter().map(angelica_view::Place::Panel))
        .chain(entries.into_iter().map(angelica_view::Place::MenuEntry))
        .collect();
    angelica_view::data::encode_places(&places)
        .into_iter()
        .map(place_to_wit)
        .collect()
}

/// The event `ui-event` received; `None` for a kind of a later API
/// version, which the plugin does not know.
#[doc(hidden)]
#[must_use]
pub fn event_from_wit(event: wit::Event) -> Option<Event> {
    angelica_view::data::decode_event(&RawEvent {
        kind: event.kind,
        place: event.place,
        part: event.part,
        value: event.value.map(value_from_wit),
    })
}
