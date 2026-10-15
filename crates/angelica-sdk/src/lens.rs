//! Lens: the game's objects, read by path through the game map.
//!
//! A [`Query`] is prepared once, when the plugin starts or when it first
//! needs it, and run each time the plugin wants the values: every frame or
//! on an event. Rows turn into Rust structures by column name.
//!
//! ```ignore
//! use angelica_sdk::lens;
//!
//! #[derive(serde::Deserialize)]
//! struct Slot {
//!     item: u32,
//!     quantity: u32,
//! }
//!
//! let carried = lens::Query::prepare(
//!     "inventory.containers[kind in (bag_1, bag_2, bag_3, bag_4)].items[item != 0] { item, quantity }",
//! )?;
//! for slot in carried.rows::<Slot>()? {
//!     // `slot.item` is a row of the Item sheet.
//! }
//! ```
//!
//! A [`Watch`] is a query from an event of the map: its rows arrive in
//! [`crate::Plugin::game_event`] in the frame after the game reached the
//! event.
//!
//! ```ignore
//! #[derive(serde::Deserialize)]
//! struct Used {
//!     action: u32,
//! }
//!
//! // In `start`:
//! let used = lens::Watch::new("action_used[kind = action] { action }")?;
//! // In `Plugin::game_event`:
//! if let Some(rows) = used.decode::<Used>(&event) {
//!     for used in rows? { /* … */ }
//! }
//! ```
//!
//! [`act`] takes an action of the map for the player, with its parameters
//! by name:
//!
//! ```ignore
//! lens::act("use_action", &[("kind", lens::named("general_action")), ("action", 4.into())])?;
//! ```
//!
//! A [`Rule`] decides at a decision point of the map: while it is kept,
//! the host takes its outcome whenever the rule holds, without calling the
//! plugin:
//!
//! ```ignore
//! let quiet = lens::Rule::new("chat_message_added[kind = 57]", "hide")?;
//! ```
//!
//! At an event the host derives, such as a text cell of a sheet row, a rule
//! is equality conditions and an outcome that takes text, and a
//! [`RuleFile`] adds a whole file of rules, such as a text table written
//! with [`crate::text::table::Writer`]:
//!
//! ```ignore
//! let potion = lens::Rule::with(
//!     "sheet_text[sheet = \"Item\" and row = 4 and column = 9]",
//!     "replace",
//!     "Зелье",
//! )?;
//! let translation = lens::RuleFile::new("sheet_text", "translation.table")?;
//! ```
//!
//! [`set`] changes a value the map marks changeable, such as the camera's
//! zoom limit, until [`restore`] or the plugin stops:
//!
//! ```ignore
//! lens::set("camera.camera.max_distance", 40.0f32)?;
//! ```
//!
//! The manifest lists every domain a query reads, every domain the plugin
//! acts in, and every domain it changes:
//!
//! ```toml
//! [lens]
//! read = ["inventory"]
//! act = ["actions"]
//! change = ["camera", "chat"]
//! ```
use crate::api::lens as api;
pub use api::{Column, FlagSet, Row, SheetRow, Value};
use serde::de::{self, DeserializeOwned, IntoDeserializer};

/// A prepared query; dropping it releases it in the host.
#[derive(Debug)]
pub struct Query {
    handle: u32,
    columns: Vec<Column>,
}

impl Query {
    /// Checks `text` against the game map and plans its reads.
    ///
    /// # Errors
    /// See `lens.prepare`: the message names what is wrong.
    pub fn prepare(text: &str) -> Result<Self, crate::Error> {
        let prepared = api::prepare(text)?;
        Ok(Self {
            handle: prepared.query,
            columns: prepared.columns,
        })
    }

    /// The columns of each row, in the order of the query's braces.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The position of the column `name`.
    #[must_use]
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name == name)
    }

    /// The rows now.
    ///
    /// # Errors
    /// See `lens.run`.
    pub fn run(&self) -> Result<Vec<Row>, crate::Error> {
        api::run(self.handle)
    }

    /// The value of column `name` in `row`, `None` without a value or such a
    /// column.
    #[must_use]
    pub fn get<'a>(&self, row: &'a Row, name: &str) -> Option<&'a Value> {
        self.column(name)
            .and_then(|index| row.values.get(index))
            .and_then(Option::as_ref)
    }

    /// The rows now, each as a `T` whose fields are named like the
    /// columns. A column without a value is `None` for an `Option` field;
    /// text is the plain text the player reads; a sheet row is its number;
    /// a named value is its name; flags are their bits, or their names for a
    /// list.
    ///
    /// # Errors
    /// See `lens.run`; `invalid-argument` when a row does not fit `T`.
    pub fn rows<T: DeserializeOwned>(&self) -> Result<Vec<T>, crate::Error> {
        self.run()?
            .into_iter()
            .map(|row| from_row(&self.columns, row))
            .collect()
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        api::release(self.handle);
    }
}

/// A watch of an event; dropping it ends it.
#[derive(Debug)]
pub struct Watch {
    handle: u32,
    columns: Vec<Column>,
}

impl Watch {
    /// Checks a query from an event against the game map and starts
    /// watching it.
    ///
    /// # Errors
    /// See `lens.watch`.
    pub fn new(text: &str) -> Result<Self, crate::Error> {
        let prepared = api::watch(text)?;
        Ok(Self {
            handle: prepared.query,
            columns: prepared.columns,
        })
    }

    /// The columns of each row, in the order of the query's braces.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The rows of one firing, when `event` is one of this watch.
    #[must_use]
    pub fn rows<'e>(&self, event: &'e crate::events::GameEvent) -> Option<&'e [Row]> {
        match event {
            crate::events::GameEvent::Watched(watched) if watched.query == self.handle => {
                Some(&watched.rows)
            }
            _ => None,
        }
    }

    /// The rows of one firing as `T`s, when `event` is one of this watch;
    /// see [`Query::rows`].
    #[must_use]
    pub fn decode<T: DeserializeOwned>(
        &self,
        event: &crate::events::GameEvent,
    ) -> Option<Result<Vec<T>, crate::Error>> {
        self.rows(event).map(|rows| {
            rows.iter()
                .map(|row| from_row(&self.columns, row.clone()))
                .collect()
        })
    }

    /// How many firings were left out, when `event` says so for this watch.
    #[must_use]
    pub fn dropped(&self, event: &crate::events::GameEvent) -> Option<u32> {
        match event {
            crate::events::GameEvent::WatchDropped(dropped) if dropped.query == self.handle => {
                Some(dropped.count)
            }
            _ => None,
        }
    }

    /// The value of column `name` in `row`.
    #[must_use]
    pub fn get<'a>(&self, row: &'a Row, name: &str) -> Option<&'a Value> {
        self.columns
            .iter()
            .position(|column| column.name == name)
            .and_then(|index| row.values.get(index))
            .and_then(Option::as_ref)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        api::release(self.handle);
    }
}

/// A row as a `T` whose fields are named like `columns`.
///
/// # Errors
/// `invalid-argument` when the row does not fit `T`.
pub fn from_row<T: DeserializeOwned>(columns: &[Column], row: Row) -> Result<T, crate::Error> {
    T::deserialize(RowDeserializer {
        entries: columns
            .iter()
            .map(|column| column.name.clone())
            .zip(row.values)
            .collect::<Vec<_>>()
            .into_iter(),
        value: None,
    })
    .map_err(|error: DeError| crate::Error {
        kind: crate::ErrorKind::InvalidArgument,
        message: error.0,
    })
}

#[derive(Debug)]
struct DeError(String);

impl std::fmt::Display for DeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DeError {}

impl de::Error for DeError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

struct RowDeserializer {
    entries: std::vec::IntoIter<(String, Option<Value>)>,
    value: Option<Value>,
}

impl<'de> de::MapAccess<'de> for RowDeserializer {
    type Error = DeError;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, DeError> {
        match self.entries.next() {
            Some((name, value)) => {
                self.value = value;
                seed.deserialize(name.into_deserializer()).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, DeError> {
        seed.deserialize(ValueDeserializer(self.value.take()))
    }
}

impl<'de> de::Deserializer<'de> for RowDeserializer {
    type Error = DeError;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        visitor.visit_map(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map struct enum identifier ignored_any
    }
}

struct ValueDeserializer(Option<Value>);

impl<'de> de::Deserializer<'de> for ValueDeserializer {
    type Error = DeError;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            None => visitor.visit_none(),
            Some(Value::Boolean(value)) => visitor.visit_bool(value),
            Some(Value::Signed(value)) => visitor.visit_i64(value),
            Some(Value::Unsigned(value)) => visitor.visit_u64(value),
            Some(Value::Float(value)) => visitor.visit_f64(value),
            Some(Value::Text(bytes)) => visitor.visit_string(crate::text::sestring::plain(&bytes)),
            Some(Value::Sheet(sheet)) => visitor.visit_u32(sheet.row),
            Some(Value::Named(name)) => visitor.visit_string(name),
            Some(Value::Flags(flags)) => visitor.visit_u64(flags.bits),
        }
    }

    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            None => visitor.visit_none(),
            Some(value) => visitor.visit_some(Self(Some(value))),
        }
    }

    fn deserialize_bytes<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            Some(Value::Text(bytes)) => visitor.visit_byte_buf(bytes),
            other => Self(other).deserialize_any(visitor),
        }
    }

    fn deserialize_byte_buf<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, DeError> {
        match self.0 {
            Some(Value::Flags(flags)) => {
                visitor.visit_seq(de::value::SeqDeserializer::new(flags.names.into_iter()))
            }
            Some(Value::Text(bytes)) => {
                visitor.visit_seq(de::value::SeqDeserializer::new(bytes.into_iter()))
            }
            other => Self(other).deserialize_any(visitor),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        unit unit_struct newtype_struct tuple tuple_struct map struct enum
        identifier ignored_any
    }
}

/// Takes the map's action `action` for the player now, with each parameter
/// by name; a parameter left out takes the map's default.
///
/// # Errors
/// See `lens.act`: `unavailable` with the reason when the game cannot take
/// it now, `permission-denied` while the player stopped plugins from
/// acting.
pub fn act(action: &str, params: &[(&str, Value)]) -> Result<(), crate::Error> {
    let params: Vec<(String, Value)> = params
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect();
    api::act(action, &params)
}

/// A rule at a decision point; dropping it ends it.
#[derive(Debug)]
pub struct Rule {
    handle: u32,
}

impl Rule {
    /// Adds the rule `text`, a query from an event with outcomes without
    /// braces, to take `outcome` whenever it holds.
    ///
    /// # Errors
    /// See `lens.decide`.
    pub fn new(text: &str, outcome: &str) -> Result<Self, crate::Error> {
        Ok(Self {
            handle: api::decide(text, outcome, None)?,
        })
    }

    /// Adds the rule `text` to take `outcome` with `value`, such as the
    /// text that replaces a sheet's cell at a derived event.
    ///
    /// # Errors
    /// See `lens.decide`.
    pub fn with(text: &str, outcome: &str, value: impl Into<Value>) -> Result<Self, crate::Error> {
        Ok(Self {
            handle: api::decide(text, outcome, Some(&value.into()))?,
        })
    }

    /// What the rule did, at a derived event.
    ///
    /// # Errors
    /// See `lens.decisions`.
    pub fn decisions(&self) -> Result<DecisionStatus, crate::Error> {
        api::decisions(self.handle)
    }
}

impl Drop for Rule {
    fn drop(&mut self) {
        api::release(self.handle);
    }
}

pub use api::{DecisionStatus, RuleFile as RuleFileInfo};

/// The rules of a file in the plugin's storage at a derived event; dropping
/// it ends them.
#[derive(Debug)]
pub struct RuleFile {
    info: RuleFileInfo,
}

impl RuleFile {
    /// Adds the rules of the file at `path` at `event`.
    ///
    /// # Errors
    /// See `lens.decide-file`.
    pub fn new(event: &str, path: &str) -> Result<Self, crate::Error> {
        Ok(Self {
            info: api::decide_file(event, path)?,
        })
    }

    /// How many parts of the game, such as sheets, and rules the file has.
    #[must_use]
    pub fn info(&self) -> &RuleFileInfo {
        &self.info
    }

    /// What the file's rules did.
    ///
    /// # Errors
    /// See `lens.decisions`.
    pub fn decisions(&self) -> Result<DecisionStatus, crate::Error> {
        api::decisions(self.info.handle)
    }
}

impl Drop for RuleFile {
    fn drop(&mut self) {
        api::release(self.info.handle);
    }
}

/// Changes the value at `path`, a field the map marks changeable such as
/// `camera.camera.max_distance`, in every object the path reaches; returns
/// how many. The host puts the original back on [`restore`] and when the
/// plugin stops.
///
/// # Errors
/// See `lens.set`.
pub fn set(path: &str, value: impl Into<Value>) -> Result<u32, crate::Error> {
    api::set(path, &value.into())
}

/// Puts back the originals of what [`set`] changed through `path`.
///
/// # Errors
/// See `lens.restore`.
pub fn restore(path: &str) -> Result<(), crate::Error> {
    api::restore(path)
}

/// The value of an enum by its name, for a parameter of [`act`].
#[must_use]
pub fn named(name: &str) -> Value {
    Value::Named(name.to_owned())
}

impl From<&str> for Value {
    /// Text, for a text field.
    fn from(value: &str) -> Self {
        Self::Text(value.as_bytes().to_vec())
    }
}

impl From<String> for Value {
    /// Text, for a text field.
    fn from(value: String) -> Self {
        Self::Text(value.into_bytes())
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Self::Unsigned(value.into())
    }
}

impl From<u64> for Value {
    fn from(value: u64) -> Self {
        Self::Unsigned(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Signed(value)
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Self::Float(value.into())
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl Value {
    /// Text as the game stores it.
    #[must_use]
    pub fn text(&self) -> Option<&[u8]> {
        match self {
            Self::Text(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Text as the player reads it, without macros (see
    /// [`crate::text::sestring::plain`]).
    #[must_use]
    pub fn plain(&self) -> Option<String> {
        self.text().map(crate::text::sestring::plain)
    }

    /// An unsigned, or a non-negative signed, value; the number of a sheet
    /// row; the bits of flags.
    #[must_use]
    pub fn unsigned(&self) -> Option<u64> {
        match self {
            Self::Unsigned(value) => Some(*value),
            Self::Signed(value) => u64::try_from(*value).ok(),
            Self::Sheet(sheet) => Some(u64::from(sheet.row)),
            Self::Flags(flags) => Some(flags.bits),
            _ => None,
        }
    }

    /// A signed, or a small enough unsigned, value.
    #[must_use]
    pub fn signed(&self) -> Option<i64> {
        match self {
            Self::Signed(value) => Some(*value),
            Self::Unsigned(value) => i64::try_from(*value).ok(),
            _ => None,
        }
    }

    #[must_use]
    pub fn float(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    /// The row of a sheet a number refers to.
    #[must_use]
    pub fn sheet_row(&self) -> Option<&SheetRow> {
        match self {
            Self::Sheet(sheet) => Some(sheet),
            _ => None,
        }
    }

    /// The name of an enum's value.
    #[must_use]
    pub fn named(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(names: &[&str]) -> Vec<Column> {
        names
            .iter()
            .map(|name| Column {
                name: (*name).into(),
                doc: String::new(),
            })
            .collect()
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Slot {
        item: u32,
        quantity: u32,
        high_quality: bool,
        kind: String,
        name: Option<String>,
        marks: Vec<String>,
        bits: u64,
    }

    #[test]
    fn rows_turn_into_structures_by_column_name() {
        let row = Row {
            indexes: vec![0, 3],
            values: vec![
                Some(Value::Sheet(SheetRow {
                    sheet: "Item".into(),
                    row: 5057,
                })),
                Some(Value::Unsigned(12)),
                Some(Value::Boolean(true)),
                Some(Value::Named("bag_1".into())),
                None,
                Some(Value::Flags(FlagSet {
                    names: vec!["new".into()],
                    bits: 1,
                })),
                Some(Value::Flags(FlagSet {
                    names: vec!["new".into()],
                    bits: 5,
                })),
            ],
        };
        let slot: Slot = from_row(
            &columns(&[
                "item",
                "quantity",
                "high_quality",
                "kind",
                "name",
                "marks",
                "bits",
            ]),
            row,
        )
        .unwrap();
        assert_eq!(
            slot,
            Slot {
                item: 5057,
                quantity: 12,
                high_quality: true,
                kind: "bag_1".into(),
                name: None,
                marks: vec!["new".into()],
                bits: 5,
            }
        );
    }

    #[test]
    fn a_row_that_does_not_fit_names_the_problem() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Named {
            name: String,
        }
        let error = from_row::<Named>(
            &columns(&["name"]),
            Row {
                indexes: Vec::new(),
                values: vec![None],
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, crate::ErrorKind::InvalidArgument);
        let text = from_row::<Named>(
            &columns(&["name"]),
            Row {
                indexes: Vec::new(),
                values: vec![Some(Value::Text(b"Alisae".to_vec()))],
            },
        )
        .unwrap();
        assert_eq!(text.name, "Alisae");
    }
}
