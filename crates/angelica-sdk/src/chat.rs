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
//!
//! // An item link the player clicks for the item's tooltip: Iron Ingot,
//! // high quality, named in English.
//! Message::new().text("Found ").item(5057, true, 2).print()?;
//! ```
use crate::api::chat as api;
pub use api::Tone;

/// A message of spans in tones and item links.
#[derive(Debug, Clone, Default)]
pub struct Message {
    pieces: Vec<api::Piece>,
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
        self.pieces.push(api::Piece::Span(api::Span {
            text: text.into(),
            tone,
            italic: false,
        }));
        self
    }

    /// Makes the last span italic.
    #[must_use]
    pub fn italic(mut self) -> Self {
        if let Some(api::Piece::Span(span)) = self.pieces.last_mut() {
            span.italic = true;
        }
        self
    }

    /// A link to the item of the `Item` sheet's row `item`, its high-quality
    /// version with `hq`: the host writes the item's name in `language` (the
    /// game's number, as `excel` takes it) in the color of its rarity, and
    /// the player clicks it for the item's tooltip.
    #[must_use]
    pub fn item(mut self, item: u32, hq: bool, language: u8) -> Self {
        self.pieces
            .push(api::Piece::Item(api::ItemLink { item, hq, language }));
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
    /// See `chat.print` and, with item links, `chat.print-pieces`.
    pub fn print(&self) -> Result<(), crate::Error> {
        let spans: Option<Vec<api::Span>> = self
            .pieces
            .iter()
            .map(|piece| match piece {
                api::Piece::Span(span) => Some(span.clone()),
                api::Piece::Item(_) => None,
            })
            .collect();
        match spans {
            Some(spans) => api::print(&spans),
            None => api::print_pieces(&self.pieces),
        }
    }
}

/// Prints one line of plain text.
///
/// # Errors
/// See `chat.print`.
pub fn print(text: impl Into<String>) -> Result<(), crate::Error> {
    Message::new().text(text).print()
}
