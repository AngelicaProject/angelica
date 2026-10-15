//! Messages in the chat log, built span by span.
//!
//! ```ignore
//! use angelica_sdk::chat::Message;
//!
//! Message::new()
//!     .text("Installed ")
//!     .accent("Russian 1.4")
//!     .muted(" (12 MB)")
//!     .print()?;
//! ```
use crate::api::chat as api;
pub use api::Tone;

/// A message of spans in tones.
#[derive(Debug, Clone, Default)]
pub struct Message {
    spans: Vec<api::Span>,
}

impl Message {
    /// An empty message; add spans to it.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A span in a tone.
    #[must_use]
    pub fn span(mut self, text: impl Into<String>, tone: Tone) -> Self {
        self.spans.push(api::Span {
            text: text.into(),
            tone,
            italic: false,
        });
        self
    }

    /// Makes the last span italic.
    #[must_use]
    pub fn italic(mut self) -> Self {
        if let Some(span) = self.spans.last_mut() {
            span.italic = true;
        }
        self
    }

    /// A span in the normal tone.
    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Normal)
    }

    /// A span in the accent tone, for names and what the player typed.
    #[must_use]
    pub fn accent(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Accent)
    }

    /// A span in the muted tone, for what matters less.
    #[must_use]
    pub fn muted(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Muted)
    }

    /// A span in the tone of success.
    #[must_use]
    pub fn good(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Good)
    }

    /// A span in the tone of a warning.
    #[must_use]
    pub fn warning(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Warning)
    }

    /// A span in the tone of a failure.
    #[must_use]
    pub fn bad(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Bad)
    }

    /// Starts a new line.
    #[must_use]
    pub fn line(self) -> Self {
        self.text("\n")
    }

    /// Prints the message in the chat log.
    ///
    /// # Errors
    /// See `chat.print`.
    pub fn print(&self) -> Result<(), crate::Error> {
        api::print(&self.spans)
    }
}

/// Prints one line of plain text.
///
/// # Errors
/// See `chat.print`.
pub fn print(text: impl Into<String>) -> Result<(), crate::Error> {
    Message::new().text(text).print()
}
