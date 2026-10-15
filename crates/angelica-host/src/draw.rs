//! Drawing over the game world: plugins' marks, lines, and text at places in
//! the world, which the host projects onto the screen every frame with the
//! game camera's view-projection matrix from the game map. A plugin replaces
//! its shapes whenever it wants; they stay until then or until it stops.
//! Platform independent: the overlay paints what [`project`] gives.
#![forbid(unsafe_code)]
use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// The most shapes one plugin draws.
pub const MOST_SHAPES: usize = 1024;
/// The most bytes of one text.
pub const MOST_TEXT: usize = 256;
/// The largest radius of a circle and length of a line, in yalms.
pub const MOST_SIZE: f32 = 1000.0;
/// How many segments a circle is drawn with.
const CIRCLE_SEGMENTS: usize = 48;
/// How many pieces a line is cut into, so a line that passes behind the
/// camera is drawn where it is in front.
const LINE_PIECES: usize = 16;

/// A place in the game world: `y` is up, in yalms.
pub type Point = [f32; 3];

/// A color as `0xRRGGBBAA`.
pub type Color = u32;

/// One shape a plugin draws.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line {
        from: Point,
        to: Point,
        color: Color,
        thickness: f32,
    },
    /// A circle on the ground: around `center`, level with it.
    Circle {
        center: Point,
        radius: f32,
        color: Color,
        thickness: f32,
        filled: bool,
    },
    Text {
        at: Point,
        text: String,
        color: Color,
    },
}

/// Something to paint, in the screen's fractions: `(0, 0)` is the top left
/// corner and `(1, 1)` the bottom right.
#[derive(Debug, Clone, PartialEq)]
pub enum Mark {
    Line {
        from: (f32, f32),
        to: (f32, f32),
        color: Color,
        thickness: f32,
    },
    /// A filled polygon, convex as a circle seen at an angle.
    Fill {
        points: Vec<(f32, f32)>,
        color: Color,
    },
    Text {
        at: (f32, f32),
        text: String,
        color: Color,
    },
}

/// Every plugin's shapes.
#[derive(Default)]
pub struct Drawings {
    shapes: Mutex<BTreeMap<String, Vec<Shape>>>,
}

impl Drawings {
    /// Replaces a plugin's shapes; none clears them.
    ///
    /// # Errors
    /// `limit-exceeded` past [`MOST_SHAPES`]; `invalid-argument` for a
    /// shape that is not finite or too large, or text too long or with
    /// control characters.
    pub fn set(&self, plugin: &str, shapes: Vec<Shape>) -> Result<()> {
        if shapes.len() > MOST_SHAPES {
            return Err(Error::new(
                crate::error::Kind::LimitExceeded,
                format!("a plugin draws at most {MOST_SHAPES} shapes"),
            ));
        }
        for shape in &shapes {
            check(shape)?;
        }
        let mut all = self.shapes.lock().unwrap_or_else(PoisonError::into_inner);
        if shapes.is_empty() {
            all.remove(plugin);
        } else {
            all.insert(plugin.to_owned(), shapes);
        }
        Ok(())
    }

    /// Forgets a stopped plugin's shapes.
    pub fn remove_plugin(&self, plugin: &str) {
        self.shapes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin);
    }

    /// Whether any plugin draws.
    #[must_use]
    pub fn idle(&self) -> bool {
        self.shapes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }

    /// What every plugin's shapes look like through the camera of `matrix`,
    /// in the order of plugin ids.
    #[must_use]
    pub fn marks(&self, matrix: &[f32; 16]) -> Vec<Mark> {
        let all = self.shapes.lock().unwrap_or_else(PoisonError::into_inner);
        let mut marks = Vec::new();
        for shape in all.values().flatten() {
            marks.extend(project_shape(matrix, shape));
        }
        marks
    }
}

fn check(shape: &Shape) -> Result<()> {
    let finite = |point: &Point| point.iter().all(|value| value.is_finite());
    let fits = |size: f32| size.is_finite() && (0.0..=MOST_SIZE).contains(&size);
    let ok = match shape {
        Shape::Line {
            from,
            to,
            thickness,
            ..
        } => {
            finite(from)
                && finite(to)
                && fits(*thickness)
                && from
                    .iter()
                    .zip(to)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<f32>()
                    .sqrt()
                    <= MOST_SIZE
        }
        Shape::Circle {
            center,
            radius,
            thickness,
            ..
        } => finite(center) && fits(*radius) && fits(*thickness),
        Shape::Text { at, text, .. } => {
            if text.len() > MOST_TEXT || text.chars().any(char::is_control) {
                return Err(Error::invalid(format!(
                    "a text has at most {MOST_TEXT} bytes without control characters"
                )));
            }
            finite(at)
        }
    };
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "a shape has finite places and sizes of at most {MOST_SIZE} yalms"
        )))
    }
}

/// Where `point` is on the screen through the camera of `matrix`, a
/// view-projection matrix of row vectors as the game keeps it; `None`
/// behind the camera.
#[must_use]
pub fn project(matrix: &[f32; 16], point: Point) -> Option<(f32, f32)> {
    let [x, y, z] = point;
    let column = |index: usize| {
        x * matrix[index] + y * matrix[4 + index] + z * matrix[8 + index] + matrix[12 + index]
    };
    let w = column(3);
    if w < 0.1 {
        return None;
    }
    Some((
        f32::midpoint(column(0) / w, 1.0),
        (1.0 - column(1) / w) / 2.0,
    ))
}

fn project_shape(matrix: &[f32; 16], shape: &Shape) -> Vec<Mark> {
    match shape {
        Shape::Line {
            from,
            to,
            color,
            thickness,
        } => {
            #[allow(clippy::cast_precision_loss)] // a few pieces
            let along = |step: usize| {
                let fraction = step as f32 / LINE_PIECES as f32;
                [0, 1, 2].map(|axis| from[axis] + (to[axis] - from[axis]) * fraction)
            };
            polyline(matrix, (0..=LINE_PIECES).map(along), *color, *thickness)
        }
        Shape::Circle {
            center,
            radius,
            color,
            thickness,
            filled,
        } => {
            #[allow(clippy::cast_precision_loss)] // a few segments
            let around = |step: usize| {
                let angle = std::f32::consts::TAU * step as f32 / CIRCLE_SEGMENTS as f32;
                [
                    center[0] + radius * angle.cos(),
                    center[1],
                    center[2] + radius * angle.sin(),
                ]
            };
            if *filled {
                let points: Option<Vec<_>> = (0..CIRCLE_SEGMENTS)
                    .map(|step| project(matrix, around(step)))
                    .collect();
                // A filled circle partly behind the camera is drawn as its
                // outline only.
                if let Some(points) = points {
                    return vec![Mark::Fill {
                        points,
                        color: *color,
                    }];
                }
            }
            polyline(
                matrix,
                (0..=CIRCLE_SEGMENTS).map(around),
                *color,
                *thickness,
            )
        }
        Shape::Text { at, text, color } => project(matrix, *at)
            .map(|at| Mark::Text {
                at,
                text: text.clone(),
                color: *color,
            })
            .into_iter()
            .collect(),
    }
}

/// The visible pieces of a line through `points`.
fn polyline(
    matrix: &[f32; 16],
    points: impl Iterator<Item = Point>,
    color: Color,
    thickness: f32,
) -> Vec<Mark> {
    let projected: Vec<Option<(f32, f32)>> = points.map(|point| project(matrix, point)).collect();
    projected
        .windows(2)
        .filter_map(|pair| match (pair[0], pair[1]) {
            (Some(from), Some(to)) => Some(Mark::Line {
                from,
                to,
                color,
                thickness,
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Drawings, Mark, Shape, project};

    /// A camera at the origin looking down +z, as a view-projection of row
    /// vectors: x and y pass through, w is the depth.
    const LOOKING_AHEAD: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 0.0, 1.0, //
        0.0, 0.0, 0.0, 0.0,
    ];

    #[test]
    fn places_ahead_are_projected_and_behind_are_not() {
        assert_eq!(project(&LOOKING_AHEAD, [0.0, 0.0, 5.0]), Some((0.5, 0.5)));
        assert_eq!(project(&LOOKING_AHEAD, [5.0, 5.0, 5.0]), Some((1.0, 0.0)));
        assert_eq!(project(&LOOKING_AHEAD, [0.0, 0.0, -5.0]), None);
    }

    #[test]
    fn shapes_are_checked_and_drawn_until_replaced() {
        let drawings = Drawings::default();
        let line = Shape::Line {
            from: [0.0, 0.0, -5.0],
            to: [0.0, 0.0, 5.0],
            color: 0xFF00_00FF,
            thickness: 2.0,
        };
        drawings.set("p", vec![line]).unwrap();
        let marks = drawings.marks(&LOOKING_AHEAD);
        assert!(!marks.is_empty(), "the part ahead of the camera");
        assert!(marks.len() < 16, "not the part behind it");
        assert!(
            drawings
                .set(
                    "p",
                    vec![Shape::Text {
                        at: [0.0, 0.0, 1.0],
                        text: "a\nb".into(),
                        color: 0,
                    }]
                )
                .is_err()
        );
        assert!(
            drawings
                .set(
                    "p",
                    vec![Shape::Circle {
                        center: [f32::NAN, 0.0, 0.0],
                        radius: 1.0,
                        color: 0,
                        thickness: 1.0,
                        filled: false,
                    }]
                )
                .is_err()
        );
        drawings
            .set(
                "p",
                vec![Shape::Circle {
                    center: [0.0, 0.0, 10.0],
                    radius: 1.0,
                    color: 0x00FF_0080,
                    thickness: 1.0,
                    filled: true,
                }],
            )
            .unwrap();
        assert!(matches!(
            drawings.marks(&LOOKING_AHEAD)[..],
            [Mark::Fill { .. }]
        ));
        drawings.remove_plugin("p");
        assert!(drawings.idle());
    }
}
