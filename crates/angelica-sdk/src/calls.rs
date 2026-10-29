//! Calls of the game's functions that the plugin describes in its own
//! Anatomia entries, and thunks: functions the game calls back, which run
//! [`crate::Plugin::thunk`]. Needs the `unreviewed` and `server`
//! permissions.
//!
//! ```ignore
//! use angelica_sdk::{calls, memory::Value};
//!
//! // In the main instance: a thunk of the plugin's own type, handed to a
//! // function that calls it back for every element.
//! let each = calls::thunk("fn(u32) -> u32", 0, None)?;
//! let sum = calls::call("game.each", &[each.value(), Value::Unsigned(3)])?;
//!
//! fn thunk(call: calls::ThunkCall) -> Value {
//!     match call.args.first() {
//!         Some(Value::Unsigned(n)) => Value::Unsigned(n * 10),
//!         _ => Value::None,
//!     }
//! }
//! ```

use crate::Error;
pub use crate::bindings::exports::angelica::plugin::thunk_handler::ThunkCall;
use crate::memory::{Handle, Value};

/// Calls `function` of the plugin's own entries with `args` by position,
/// and gives what it returns.
///
/// # Errors
/// Without the permissions, an unknown or absent function, arguments of
/// other types, and a function of the main thread called from a handler.
pub fn call(function: &str, args: &[Value]) -> Result<Value, Error> {
    crate::api::calls::call(function, args)
}

/// A thunk of the plugin's: a handle the plugin passes to a call or writes
/// into a pointer field of the same type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thunk(pub Handle);

impl Thunk {
    /// The thunk as an argument of a call.
    #[must_use]
    pub fn value(self) -> Value {
        Value::Pointer(self.0)
    }
}

impl crate::memory::IntoValue for &Thunk {
    fn into_value(self) -> Value {
        Value::Pointer(self.0)
    }
}

/// In the main instance: a thunk of the type `signature`, of the plugin's
/// own types, whose calls run [`crate::Plugin::thunk`] with `handler`;
/// `fallback` is a function of the plugin's entries of the same type that
/// runs instead when the handler cannot.
///
/// # Errors
/// Without the permissions, a type a thunk cannot take, a fallback of
/// another type, and more thunks than the game holds.
pub fn thunk(signature: &str, handler: u32, fallback: Option<&str>) -> Result<Thunk, Error> {
    crate::api::calls::thunk(signature, handler, fallback).map(Thunk)
}
