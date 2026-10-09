//! The game's state, read by table and field name.
//!
//! ```ignore
//! use angelica_sdk::state;
//!
//! // `None` before the player logs in.
//! if let Some(values) = state::read_one("self", &["level", "class_job"])? {
//!     let level = values[0].as_ref().and_then(state::Value::unsigned);
//! }
//! ```
use crate::api::state as api;
pub use api::{Field, Table, Value};

fn names(fields: &[&str]) -> Vec<String> {
    fields.iter().map(|name| (*name).to_owned()).collect()
}

/// Every table of the game map, with its fields.
#[must_use]
pub fn tables() -> Vec<Table> {
    api::tables()
}

/// The rows of a table with the values of `fields`, in the order given.
///
/// # Errors
/// See `state.read`.
pub fn read(table: &str, fields: &[&str]) -> Result<Vec<Vec<Option<Value>>>, crate::Error> {
    api::read(table, &names(fields))
}

/// The values of `fields` in a table of one object, such as `self`, or
/// `None` while the object does not exist.
///
/// # Errors
/// See `state.read`.
pub fn read_one(table: &str, fields: &[&str]) -> Result<Option<Vec<Option<Value>>>, crate::Error> {
    read(table, fields).map(|rows| rows.into_iter().next())
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

    /// An unsigned, or a non-negative signed, value.
    #[must_use]
    pub fn unsigned(&self) -> Option<u64> {
        match *self {
            Self::Unsigned(value) => Some(value),
            Self::Signed(value) => u64::try_from(value).ok(),
            _ => None,
        }
    }

    /// A signed, or a small enough unsigned, value.
    #[must_use]
    pub fn signed(&self) -> Option<i64> {
        match *self {
            Self::Signed(value) => Some(value),
            Self::Unsigned(value) => i64::try_from(value).ok(),
            _ => None,
        }
    }

    #[must_use]
    pub fn float(&self) -> Option<f64> {
        match *self {
            Self::Float(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn boolean(&self) -> Option<bool> {
        match *self {
            Self::Boolean(value) => Some(value),
            _ => None,
        }
    }
}
