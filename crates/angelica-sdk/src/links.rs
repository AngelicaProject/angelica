//! Pages opened in the player's browser for the player's own action.
//!
//! ```ignore
//! use angelica_sdk::links;
//!
//! // In `Plugin::ui_event` or `Plugin::command`; needs `links.open` and the
//! // site in `[links] hosts`.
//! links::open("https://www.example.com/item/5057")?;
//! ```
use crate::api::links as api;

/// Opens `url` in the player's browser at the end of the frame.
///
/// # Errors
/// See `links.open`: outside `ui-event` and `command`, a URL that is not
/// `https` to a site the manifest lists, or more than one link a second.
pub fn open(url: &str) -> Result<(), crate::Error> {
    api::open(url)
}
