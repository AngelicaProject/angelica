//! Drawing over the game world: circles on the ground, lines, and text at
//! places in the world, projected through the game camera every frame.
//!
//! ```ignore
//! use angelica_sdk::draw;
//!
//! // Needs `draw.world`. The shapes stay until replaced.
//! draw::set(&[
//!     draw::circle([x, y, z], 5.0, 0xE8735AFF),
//!     draw::text([x, y + 2.5, z], "here", 0xFFFFFFFF),
//! ])?;
//! draw::clear()?;
//! ```
use crate::api::draw as api;
pub use api::{Circle, Label, Line, Point, Shape};

/// Replaces what the plugin draws.
///
/// # Errors
/// See `draw.set`.
pub fn set(shapes: &[Shape]) -> Result<(), crate::Error> {
    api::set(shapes)
}

/// Draws nothing until the next [`set`].
///
/// # Errors
/// See `draw.set`.
pub fn clear() -> Result<(), crate::Error> {
    api::set(&[])
}

fn point([x, y, z]: [f32; 3]) -> Point {
    Point { x, y, z }
}

/// A line from `start` to `end`, 2 points thick, in `0xRRGGBBAA`.
#[must_use]
pub fn line(start: [f32; 3], end: [f32; 3], color: u32) -> Shape {
    Shape::Line(Line {
        start: point(start),
        end: point(end),
        color,
        thickness: 2.0,
    })
}

/// The outline of a circle on the ground around `center`, `radius` yalms
/// wide, 2 points thick.
#[must_use]
pub fn circle(center: [f32; 3], radius: f32, color: u32) -> Shape {
    Shape::Circle(Circle {
        center: point(center),
        radius,
        color,
        thickness: 2.0,
        filled: false,
    })
}

/// A filled circle on the ground around `center`.
#[must_use]
pub fn disc(center: [f32; 3], radius: f32, color: u32) -> Shape {
    Shape::Circle(Circle {
        center: point(center),
        radius,
        color,
        thickness: 0.0,
        filled: true,
    })
}

/// Text centered on `at`.
#[must_use]
pub fn text(at: [f32; 3], text: &str, color: u32) -> Shape {
    Shape::Text(Label {
        at: point(at),
        text: text.to_owned(),
        color,
    })
}
