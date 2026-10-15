//! Notifications at the top right of the screen, under the plugin's name.
//!
//! ```ignore
//! use angelica_sdk::notifications;
//!
//! // Needs `notifications`.
//! notifications::show("The duty is ready")?;
//! // Also flashes the game on the taskbar while the player is elsewhere.
//! notifications::alert("The duty is ready")?;
//! ```
use crate::api::notifications as api;

/// Shows one line for a few seconds.
///
/// # Errors
/// See `notifications.show`: text over 200 characters, or a second
/// notification within 2 seconds.
pub fn show(text: &str) -> Result<(), crate::Error> {
    api::show(text, false)
}

/// Shows one line and flashes the game's button on the taskbar while the
/// game is in the background.
///
/// # Errors
/// As [`show`].
pub fn alert(text: &str) -> Result<(), crate::Error> {
    api::show(text, true)
}
