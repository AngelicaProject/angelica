//! What plugins show, between the `ui` interface's data and the typed
//! places of [`angelica_view`], which hold the kinds, their properties, and
//! the checks.
#![forbid(unsafe_code)]
use crate::sandbox::bindings::angelica::plugin::ui as wit;
use angelica_view::data::{RawEvent, RawPart, RawPlace, Value};

fn value(value: wit::Value) -> Value {
    match value {
        wit::Value::Text(text) => Value::Text(text),
        wit::Value::Number(number) => Value::Number(number),
        wit::Value::Integer(integer) => Value::Integer(integer),
        wit::Value::Flag(flag) => Value::Flag(flag),
        wit::Value::Texts(texts) => Value::Texts(texts),
        wit::Value::Numbers(numbers) => Value::Numbers(numbers),
    }
}

fn properties(properties: Vec<wit::Property>) -> Vec<(String, Value)> {
    properties
        .into_iter()
        .map(|property| (property.name, value(property.value)))
        .collect()
}

/// A place as the plugin returned it.
#[must_use]
pub fn place_from_wit(place: wit::Place) -> RawPlace {
    RawPlace {
        kind: place.kind,
        id: place.id,
        properties: properties(place.properties),
        parts: place
            .parts
            .into_iter()
            .map(|part| RawPart {
                kind: part.kind,
                id: part.id,
                properties: properties(part.properties),
            })
            .collect(),
    }
}

/// An event as `ui-event` takes it.
#[must_use]
pub fn event_to_wit(event: &angelica_view::Event) -> wit::Event {
    let RawEvent {
        kind,
        place,
        part,
        value,
    } = angelica_view::data::encode_event(event);
    wit::Event {
        kind,
        place,
        part,
        value: value.map(|value| match value {
            Value::Text(text) => wit::Value::Text(text),
            Value::Number(number) => wit::Value::Number(number),
            Value::Integer(integer) => wit::Value::Integer(integer),
            Value::Flag(flag) => wit::Value::Flag(flag),
            Value::Texts(texts) => wit::Value::Texts(texts),
            Value::Numbers(numbers) => wit::Value::Numbers(numbers),
        }),
    }
}

/// The minor version of plugin API 1 a manifest's `plugin.api` names.
#[must_use]
pub fn api_minor(api: &str) -> u16 {
    api.split_once('.')
        .and_then(|(_, minor)| minor.parse().ok())
        .unwrap_or(0)
}
