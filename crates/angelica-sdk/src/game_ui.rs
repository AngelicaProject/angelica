//! Rules for the game's own windows: elements kept hidden and text put in
//! text elements, named as the game map names them.
//!
//! ```ignore
//! use angelica_sdk::game_ui;
//!
//! // Returns the elements this game build does not have names for.
//! let missing = game_ui::hide(&[game_ui::element("_Exp", "window")])?;
//! game_ui::texts(&[game_ui::after("_Exp", "text", " (75%)")])?;
//! ```
use crate::api::game_ui as api;
pub use api::{Element, ElementInfo, Placement, TextRule};

/// The element `name` of the window the game names `window`.
#[must_use]
pub fn element(window: &str, name: &str) -> Element {
    Element {
        window: window.to_owned(),
        name: name.to_owned(),
    }
}

fn rule(window: &str, name: &str, text: impl Into<String>, placement: Placement) -> TextRule {
    TextRule {
        element: element(window, name),
        text: text.into(),
        placement,
    }
}

/// `text` after the game's text of an element.
#[must_use]
pub fn after(window: &str, name: &str, text: impl Into<String>) -> TextRule {
    rule(window, name, text, Placement::After)
}

/// `text` before the game's text of an element.
#[must_use]
pub fn before(window: &str, name: &str, text: impl Into<String>) -> TextRule {
    rule(window, name, text, Placement::Before)
}

/// `text` in place of the game's text of an element.
#[must_use]
pub fn instead(window: &str, name: &str, text: impl Into<String>) -> TextRule {
    rule(window, name, text, Placement::Instead)
}

/// The elements of a window the game map names in this game build.
///
/// # Errors
/// See `game-ui.elements`.
pub fn elements(window: &str) -> Result<Vec<ElementInfo>, crate::Error> {
    api::elements(window)
}

/// Replaces the elements the plugin keeps hidden; an empty list shows them
/// again. Returns the elements that are not available in this game build.
///
/// # Errors
/// See `game-ui.set-hidden`.
pub fn hide(elements: &[Element]) -> Result<Vec<Element>, crate::Error> {
    api::set_hidden(elements)
}

/// Replaces the texts the plugin puts in elements; an empty list removes
/// them. Returns the elements that are not available in this game build.
///
/// # Errors
/// See `game-ui.set-texts`.
pub fn texts(rules: &[TextRule]) -> Result<Vec<Element>, crate::Error> {
    api::set_texts(rules)
}
