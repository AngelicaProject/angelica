//! What the game did: subscribe to kinds of events and receive them in
//! [`crate::Plugin::game_event`].
//!
//! ```ignore
//! use angelica_sdk::events::{self, EventKind, GameEvent};
//!
//! // In `start`; needs `chat.read`.
//! events::subscribe(&[EventKind::ChatMessage])?;
//!
//! // In `Plugin::game_event`:
//! if let GameEvent::ChatMessage(message) = event {
//!     let text = angelica_sdk::text::sestring::plain(&message.message);
//! }
//! ```
use crate::api::events as api;
pub use api::{ChatMessage, EventKind, GameEvent, PluginMessage, WatchDropped, Watched};

/// Replaces the plugin's subscriptions; an empty slice ends them.
///
/// # Errors
/// See `events.subscribe`.
pub fn subscribe(kinds: &[EventKind]) -> Result<(), crate::Error> {
    api::subscribe(kinds)
}
