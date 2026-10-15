//! Live connections to the plugin's hosts. What happens on them arrives in
//! [`crate::Plugin::game_event`] as [`GameEvent::SocketOpened`],
//! [`GameEvent::SocketMessage`], and [`GameEvent::SocketClosed`].
//!
//! ```ignore
//! use angelica_sdk::{events::{Data, GameEvent}, websocket::Socket};
//!
//! // In `start`; needs `http.websocket` and the host in `[http] hosts`.
//! let socket = Socket::open("wss://sync.example.org/live")?;
//!
//! // In `Plugin::game_event`:
//! match event {
//!     GameEvent::SocketOpened(opened) if opened.socket == socket.id() => {
//!         socket.send_text("hello")?;
//!     }
//!     GameEvent::SocketMessage(message) if message.socket == socket.id() => {
//!         if let Data::Text(text) = &message.data { /* ... */ }
//!     }
//!     _ => {}
//! }
//! ```
//!
//! [`GameEvent::SocketOpened`]: crate::events::GameEvent::SocketOpened
//! [`GameEvent::SocketMessage`]: crate::events::GameEvent::SocketMessage
//! [`GameEvent::SocketClosed`]: crate::events::GameEvent::SocketClosed
use crate::api::websocket as api;
pub use crate::api::websocket::Data;

/// A connection the plugin opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Socket(u32);

impl Socket {
    /// Opens a connection to `url`; the outcome arrives as an event.
    ///
    /// # Errors
    /// See `websocket.open`.
    pub fn open(url: &str) -> Result<Self, crate::Error> {
        api::open(url, &[], None).map(Self)
    }

    /// Opens a connection offering `protocols`, with `authorization` as its
    /// header when given.
    ///
    /// # Errors
    /// See `websocket.open`.
    pub fn open_with(
        url: &str,
        protocols: &[String],
        authorization: Option<&str>,
    ) -> Result<Self, crate::Error> {
        api::open(url, protocols, authorization).map(Self)
    }

    /// The id events name the connection by.
    #[must_use]
    pub fn id(self) -> u32 {
        self.0
    }

    /// Queues a text message.
    ///
    /// # Errors
    /// See `websocket.send`.
    pub fn send_text(self, text: &str) -> Result<(), crate::Error> {
        api::send(self.0, &Data::Text(text.to_owned()))
    }

    /// Queues a binary message.
    ///
    /// # Errors
    /// See `websocket.send`.
    pub fn send_binary(self, bytes: &[u8]) -> Result<(), crate::Error> {
        api::send(self.0, &Data::Binary(bytes.to_vec()))
    }

    /// Closes the connection once what was queued is sent.
    ///
    /// # Errors
    /// See `websocket.close`.
    pub fn close(self) -> Result<(), crate::Error> {
        api::close(self.0)
    }
}
