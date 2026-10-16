//! Notifications at the top right of the screen, under the plugin's name.
//!
//! ```ignore
//! use angelica_sdk::notifications;
//!
//! // Needs `notifications`.
//! notifications::show("The duty is ready")?;
//! // Also flashes the game on the taskbar while the player is elsewhere.
//! notifications::alert("The duty is ready")?;
//! // A popup built from parts, above the middle of the screen.
//! notifications::pop_up(&Popup::center().seconds(3).styled("HP below a third", Role::Title, Tone::Warning))?;
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

/// Shows a popup built from parts (see [`crate::ui::Popup`]): in the
/// corner, above the middle of the screen, or beside one of the plugin's
/// panels.
///
/// # Errors
/// As [`show`]; `invalid-argument` for a popup that breaks the rules of
/// places and parts, such as one with a part that takes input.
pub fn pop_up(popup: &crate::ui::Popup) -> Result<(), crate::Error> {
    api::pop_up(&crate::ui::place_to_wit(angelica_view::data::encode_popup(
        popup,
    )))
}
