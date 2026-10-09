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

    #[must_use]
    pub fn text(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Normal)
    }

    #[must_use]
    pub fn accent(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Accent)
    }

    #[must_use]
    pub fn muted(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Muted)
    }

    #[must_use]
    pub fn good(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Good)
    }

    #[must_use]
    pub fn warning(self, text: impl Into<String>) -> Self {
        self.span(text, Tone::Warning)
    }

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
