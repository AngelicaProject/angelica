//! The player's clipboard, written for the player's own action.
//!
//! ```ignore
//! use angelica_sdk::clipboard;
//!
//! // In `Plugin::ui_event` or `Plugin::command`; needs `clipboard.write`.
//! clipboard::copy("<pos>")?;
//! ```
use crate::api::clipboard as api;

/// Puts `text` on the clipboard at the end of the frame.
///
/// # Errors
/// See `clipboard.copy`: outside `ui-event` and `command`, or text over
/// 64 KiB.
pub fn copy(text: &str) -> Result<(), crate::Error> {
    api::copy(text)
}
