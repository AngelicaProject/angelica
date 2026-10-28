//! Handlers in the game's calls of functions the plugin describes in its
//! own Anatomia entries (`anatomia/` beside `angelica.toml`). Needs the
//! `unreviewed` permission.
//!
//! [`before`] and [`after`] in the main instance make
//! [`crate::Plugin::intercepted`] run in an intercept instance on every
//! call of the function, on the thread that called it: before the game's
//! code runs, or after it returned. A `before` handler may change the
//! arguments with [`set_argument`] or make the function return at once with
//! [`skip`]; an `after` handler may change the result with [`set_result`].
//! A handler reads the call's values and the text of objects whose
//! structure reads as text; it runs for at most 50 ms. Handlers run in one
//! intercept instance of the plugin, which keeps its memory between calls:
//! data the main instance publishes with [`publish_snapshot`] reaches it
//! through [`crate::Plugin::prepare`], which reads it once, in the
//! background.
//!
//! ```ignore
//! fn start() -> Result<Self, String> {
//!     angelica_sdk::intercepts::after("minigame.keybound.normalize", 0)?;
//!     Ok(Self)
//! }
//!
//! fn intercepted(call: angelica_sdk::intercepts::Call) {
//!     if let Some(Value::Object(text)) = call.args.get(1) {
//!         if let Ok(mut bytes) = angelica_sdk::intercepts::text(*text) {
//!             bytes.make_ascii_lowercase();
//!             let _ = angelica_sdk::intercepts::edit_text(*text, &bytes);
//!         }
//!     }
//! }
//! ```

use crate::Error;
pub use crate::api::intercepts::{Call, Handle, Phase, Value};

/// Calls [`crate::Plugin::intercepted`] with `handler` before every call of
/// `function` of the plugin's own entries.
///
/// # Errors
/// Without the permission or the entry, for an absent function, a function
/// a handler cannot take, and one intercepted before already.
pub fn before(function: &str, handler: u32) -> Result<(), Error> {
    crate::api::intercepts::before(function, handler)
}

/// Calls [`crate::Plugin::intercepted`] with `handler` after every call of
/// `function` of the plugin's own entries.
///
/// # Errors
/// As for [`before`].
pub fn after(function: &str, handler: u32) -> Result<(), Error> {
    crate::api::intercepts::after(function, handler)
}

/// Bytes for a new intercept instance to prepare in the background
/// ([`crate::Plugin::prepare`]); handlers run in it once it is ready.
///
/// # Errors
/// More than 64 MiB.
pub fn publish_snapshot(bytes: &[u8]) -> Result<(), Error> {
    crate::api::intercepts::publish_snapshot(bytes)
}

/// In [`crate::Plugin::prepare`] or a handler: the snapshot the intercept
/// instance prepares or prepared.
#[must_use]
pub fn snapshot() -> Vec<u8> {
    crate::api::intercepts::snapshot()
}

/// In a `before` handler: the argument at `index` is `value` for the
/// handlers after this one and the game's code.
///
/// # Errors
/// An `after` handler, no such argument, and a value of another type: a
/// number out of its range, or a handle of another call or type.
pub fn set_argument(index: u32, value: &Value) -> Result<(), Error> {
    crate::api::intercepts::set_argument(index, *value)
}

/// In a `before` handler: the game's code does not run and the function
/// returns `value` (`Value::None` for one that returns nothing).
///
/// # Errors
/// An `after` handler, and a value of another type than the function's.
pub fn skip(value: &Value) -> Result<(), Error> {
    crate::api::intercepts::skip(*value)
}

/// In an `after` handler: the function returns `value`.
///
/// # Errors
/// A `before` handler, and a value of another type than the function's.
pub fn set_result(value: &Value) -> Result<(), Error> {
    crate::api::intercepts::set_result(*value)
}

/// In a handler: the text of a [`Value::Text`], or of a [`Value::Object`]
/// whose structure reads as text.
///
/// # Errors
/// A handle of another call or of something else, and text that cannot be
/// read.
pub fn text(of: Handle) -> Result<Vec<u8>, Error> {
    crate::api::intercepts::text(of)
}

/// In a handler: replaces the text in place with as many bytes.
///
/// # Errors
/// Another length, a NUL, and text that cannot be written.
pub fn edit_text(of: Handle, bytes: &[u8]) -> Result<(), Error> {
    crate::api::intercepts::edit_text(of, bytes)
}
