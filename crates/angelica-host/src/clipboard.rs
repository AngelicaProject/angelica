//! Text plugins put on the player's clipboard, taken by the overlay at the
//! end of the frame.
#![forbid(unsafe_code)]

use std::sync::{Mutex, PoisonError};

use crate::error::{Error, Kind, Result};

/// The most text one copy carries, in bytes.
pub const MOST_TEXT: usize = 64 * 1024;

/// The text to put on the clipboard at the end of the frame.
#[derive(Default)]
pub struct Clipboard {
    copied: Mutex<Option<String>>,
}

impl Clipboard {
    /// Keeps `text` for the end of the frame, replacing an earlier copy of
    /// the frame.
    ///
    /// # Errors
    /// Text over [`MOST_TEXT`] or with a NUL character.
    pub fn copy(&self, text: &str) -> Result<()> {
        if text.len() > MOST_TEXT {
            return Err(Error::new(
                Kind::InvalidArgument,
                format!("copied text has at most {MOST_TEXT} bytes"),
            ));
        }
        if text.contains('\0') {
            return Err(Error::new(
                Kind::InvalidArgument,
                "copied text has no NUL characters",
            ));
        }
        *self.copied.lock().unwrap_or_else(PoisonError::into_inner) = Some(text.to_owned());
        Ok(())
    }

    /// The text copied since the last call.
    pub fn take(&self) -> Option<String> {
        self.copied
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_copy_of_a_frame_is_taken_once() {
        let clipboard = Clipboard::default();
        assert_eq!(clipboard.take(), None);
        clipboard.copy("first").unwrap();
        clipboard.copy("second").unwrap();
        assert_eq!(clipboard.take().as_deref(), Some("second"));
        assert_eq!(clipboard.take(), None);
        assert_eq!(
            clipboard.copy("a\0b").unwrap_err().kind,
            Kind::InvalidArgument
        );
        assert_eq!(
            clipboard.copy(&"x".repeat(MOST_TEXT + 1)).unwrap_err().kind,
            Kind::InvalidArgument
        );
        assert!(clipboard.copy(&"x".repeat(MOST_TEXT)).is_ok());
    }
}
