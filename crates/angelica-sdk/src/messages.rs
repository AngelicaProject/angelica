//! Messages between plugins, delivered in the next frame as
//! [`GameEvent::Message`](crate::events::GameEvent::Message).
//!
//! ```ignore
//! use angelica_sdk::{events::GameEvent, messages};
//!
//! // Needs `messages.send` and the plugin in `[messages] to`.
//! messages::send("glamourer", "state", b"{}")?;
//!
//! // In `Plugin::game_event`:
//! if let GameEvent::Message(message) = &event {
//!     if message.name == "state" {
//!         messages::reply(message, b"{\"ok\":true}")?;
//!     }
//! }
//! ```
pub use crate::api::events::PluginMessage as Message;
use crate::api::messages as api;

/// Sends `payload` under `name` to the plugin `to`; returns the message's
/// id, which a reply names in `in_reply_to`.
///
/// # Errors
/// See `messages.send`.
pub fn send(to: &str, name: &str, payload: &[u8]) -> Result<u32, crate::Error> {
    api::send(to, name, payload, None)
}

/// Answers `message` with `payload` under the same name; needs no
/// permission.
///
/// # Errors
/// See `messages.send`.
pub fn reply(message: &Message, payload: &[u8]) -> Result<u32, crate::Error> {
    api::send(&message.sender, &message.name, payload, Some(message.id))
}
