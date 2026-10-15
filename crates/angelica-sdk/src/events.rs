//! What reaches [`crate::Plugin::game_event`]: firings of the plugin's
//! [Lens watches](crate::lens::Watch), messages from other plugins, and what
//! happened on its connections. None needs a subscription.
//!
//! ```ignore
//! use angelica_sdk::{events::GameEvent, lens};
//!
//! // In `start`; needs `[lens] read = ["chat"]`.
//! let chat = lens::Watch::new("chat_message_added { kind, message.text }")?;
//!
//! // In `Plugin::game_event`:
//! for row in chat.rows(&event).unwrap_or_default() {
//!     let text = chat.get(row, "message_text").and_then(lens::Value::plain);
//! }
//! ```
pub use crate::api::events::{GameEvent, PluginMessage, WatchDropped, Watched};
pub use crate::api::websocket::{Data, SocketClosed, SocketMessage, SocketOpened};
