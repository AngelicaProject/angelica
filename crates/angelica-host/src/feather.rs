//! Angelica's mark: a feather drawn like an engine part. A curved shaft and
//! pairs of barbs at one pitch, the left side wider, in a 420 × 900 box.
//! The living feather is made of light and shows how Angelica is; the
//! blueprint feather is the quiet mechanism behind the menu.
#![forbid(unsafe_code)]
use crate::theme::{self, f32_of};
use egui::{Color32, Painter, Pos2, Stroke, Vec2};
use std::sync::LazyLock;

/// The shaft: a quadratic curve from the quill to the tip.
const SHAFT: [Pos2; 3] = [
    Pos2::new(205.0, 880.0),
    Pos2::new(140.0, 430.0),
    Pos2::new(250.0, 30.0),
];
/// The angle between a barb and the shaft, in radians (about 58°).
const BARB_ANGLE: f32 = 1.02;
/// The longest barbs on the left and the right.
const LEFT: f32 = 150.0;
const RIGHT: f32 = 118.0;

/// A point on the shaft at `t` from the quill (0) to the tip (1), and the
/// unit tangent there.
#[must_use]
pub fn rachis(t: f32) -> (Pos2, Vec2) {
    let [p0, p1, p2] = SHAFT;
    let u = 1.0 - t;
    let point = (u * u) * p0.to_vec2() + (2.0 * u * t) * p1.to_vec2() + (t * t) * p2.to_vec2();
    let tangent = 2.0 * u * (p1 - p0) + 2.0 * t * (p2 - p1);
    (point.to_pos2(), tangent.normalized())
}

/// One barb: a quadratic curve from the shaft outwards.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Barb {
    pub start: Pos2,
    pub control: Pos2,
    pub end: Pos2,
    /// −1 on the left, 1 on the right.
    pub side: f32,
}

impl Barb {
    /// The point at `u` from the shaft (0) to the barb's end (1).
    #[must_use]
    pub fn at(&self, u: f32) -> Pos2 {
        let v = 1.0 - u;
        ((v * v) * self.start.to_vec2()
            + (2.0 * v * u) * self.control.to_vec2()
            + (u * u) * self.end.to_vec2())
        .to_pos2()
    }
}

/// `pairs` pairs of barbs along the shaft, from `t` = 0.16 to 0.98, left
/// first in each pair.
#[must_use]
pub fn barbs(pairs: u32) -> Vec<Barb> {
    let mut barbs = Vec::new();
    for index in 0..pairs {
        let t = 0.16 + f32_of(index) / f32_of(pairs.saturating_sub(1).max(1)) * 0.82;
        let (point, tangent) = rachis(t);
        let envelope = (std::f32::consts::PI * ((t - 0.12) / 0.9).min(1.0))
            .sin()
            .max(0.0)
            .powf(0.62);
        for side in [-1.0_f32, 1.0] {
            let longest = if side < 0.0 { LEFT } else { RIGHT };
            let length = envelope * longest * (1.0 - 0.15 * t);
            let angle = side * BARB_ANGLE;
            let direction = Vec2::new(
                tangent.x * angle.cos() - tangent.y * angle.sin(),
                tangent.x * angle.sin() + tangent.y * angle.cos(),
            );
            barbs.push(Barb {
                start: point,
                control: point + direction * length * 0.55 + tangent * length * 0.12,
                end: point + direction * length,
                side,
            });
        }
    }
    barbs
}

/// The feather's shapes, computed once.
struct Geometry {
    /// The living feather's barbs: 23 pairs.
    small: Vec<Barb>,
    /// The blueprint feather's barbs as short polylines: 61 pairs.
    fine: Vec<Vec<Pos2>>,
    shaft: Vec<Pos2>,
}

static GEOMETRY: LazyLock<Geometry> = LazyLock::new(|| Geometry {
    small: barbs(23),
    fine: barbs(61)
        .iter()
        .map(|barb| (0..=6).map(|step| barb.at(f32_of(step) / 6.0)).collect())
        .collect(),
    shaft: (0..=40).map(|step| rachis(f32_of(step) / 40.0).0).collect(),
});

/// How Angelica is, as the living feather shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// All is well: the feather breathes.
    Calm,
    /// The installer is working: a pulse runs along the barbs.
    Working,
    /// Something waits for the player: one warm node.
    NeedsYou,
    /// Something went wrong: the light dims.
    Wrong,
}

/// Where a feather is drawn: the box's top left corner on screen and the
/// size of one feather unit.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub origin: Pos2,
    pub scale: f32,
}

impl Place {
    /// A feather `height` tall whose shaft's middle is at `x` and whose
    /// top is at `top`.
    #[must_use]
    pub fn with_height(x: f32, top: f32, height: f32) -> Self {
        let scale = height / 900.0;
        Self {
            origin: Pos2::new(x - 190.0 * scale, top),
            scale,
        }
    }

    fn point(self, point: Pos2) -> Pos2 {
        self.origin + point.to_vec2() * self.scale
    }
}

/// The living feather of light: dots along the barbs that breathe, a
/// glowing shaft, and the sign of `mood`. `dot` is the dots' size in
/// points; `time` is in seconds.
pub fn living(painter: &Painter, place: Place, time: f64, mood: Mood, dot: f32) {
    const DOTS: u32 = 4;
    let geometry = &*GEOMETRY;
    let time = num_traits::cast::<f64, f32>(time % 3600.0).unwrap_or(0.0);
    let (strength, warm) = match mood {
        Mood::Wrong => (0.38, Color32::from_rgb(196, 200, 212)),
        _ => (1.0, Color32::from_rgb(244, 226, 180)),
    };
    let breath = if mood == Mood::Wrong { 0.45 } else { 1.0 };
    for (index, barb) in geometry.small.iter().enumerate() {
        let n = f32_of(u32::try_from(index).unwrap_or(0));
        for step in 1..=DOTS {
            let k = f32_of(step);
            let drift = (time / 0.9 + n * 0.4 + k).sin() * 2.0;
            let at = barb.at(k / f32_of(DOTS)) + Vec2::new(drift, -drift * 0.6);
            let g = 0.5 + 0.5 * (time * breath / 0.7 + n * 0.3 + k * 1.7).sin();
            painter.circle_filled(
                place.point(at),
                (0.8 + g * 0.9) * dot,
                theme::alpha(warm, (0.2 + 0.55 * g) * strength),
            );
        }
    }
    let shaft: Vec<Pos2> = geometry.shaft.iter().map(|&p| place.point(p)).collect();
    painter.add(egui::Shape::line(
        shaft.clone(),
        Stroke::new(4.0 * dot, theme::alpha(theme::LIGHT, 0.16 * strength)),
    ));
    painter.add(egui::Shape::line(
        shaft,
        Stroke::new(
            1.2 * dot.max(0.8),
            Color32::from_rgba_unmultiplied(255, 240, 205, unit(0.85 * strength)),
        ),
    ));
    match mood {
        Mood::NeedsYou => {
            let barb = geometry.small[geometry.small.len() * 58 / 100];
            let at = place.point(barb.end);
            let pulse = 0.6 + 0.4 * (time / 0.38).sin();
            theme::glow(
                painter,
                at,
                7.0 * dot + 4.0,
                Color32::from_rgba_unmultiplied(240, 168, 70, unit(pulse)),
            );
            painter.circle_filled(at, 1.4 * dot + 0.6, Color32::from_rgb(0xff, 0xf1, 0xd6));
        }
        Mood::Working => {
            // A pulse runs out along one barb after another.
            let run = time * 2.5;
            let count = geometry.small.len();
            let index = usize::try_from(
                num_traits::cast::<f32, i64>(run.floor())
                    .unwrap_or(0)
                    .rem_euclid(i64::try_from(count).unwrap_or(1)),
            )
            .unwrap_or(0);
            let at = place.point(geometry.small[(index * 7) % count].at(run.fract()));
            theme::glow(
                painter,
                at,
                5.0 * dot + 3.0,
                theme::alpha(theme::UPDATING, 0.75),
            );
            painter.circle_filled(at, 1.1 * dot + 0.4, Color32::WHITE);
        }
        Mood::Calm | Mood::Wrong => {}
    }
}

fn unit(value: f32) -> u8 {
    num_traits::cast(value.clamp(0.0, 1.0) * 255.0).unwrap_or(0)
}

/// The blueprint feather: thin pale lines and its construction circle,
/// barely there. It does not move.
pub fn blueprint(painter: &Painter, place: Place, strength: f32) {
    let geometry = &*GEOMETRY;
    let stroke = Stroke::new(0.8, theme::alpha(theme::BLUEPRINT_PALE, strength));
    for barb in &geometry.fine {
        painter.add(egui::Shape::line(
            barb.iter().map(|&p| place.point(p)).collect(),
            stroke,
        ));
    }
    painter.add(egui::Shape::line(
        geometry.shaft.iter().map(|&p| place.point(p)).collect(),
        stroke,
    ));
    let center = place.point(Pos2::new(190.0, 450.0));
    let radius = 300.0 * place.scale;
    let circle: Vec<Pos2> = (0..=96)
        .map(|step| center + radius * Vec2::angled(std::f32::consts::TAU * f32_of(step) / 96.0))
        .collect();
    painter.extend(egui::Shape::dashed_line(&circle, stroke, 3.0, 7.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shaft_runs_from_the_quill_to_the_tip() {
        let (quill, up) = rachis(0.0);
        let (tip, _) = rachis(1.0);
        assert!((quill - SHAFT[0]).length() < 1e-3);
        assert!((tip - SHAFT[2]).length() < 1e-3);
        assert!(up.y < 0.0, "the shaft rises from the quill");
        assert!((up.length() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn barbs_come_in_pairs_with_the_left_side_wider() {
        let barbs = barbs(61);
        assert_eq!(barbs.len(), 122);
        for pair in barbs.chunks(2) {
            let [left, right] = [pair[0], pair[1]];
            assert_eq!(left.start, right.start);
            assert!(left.side < 0.0 && right.side > 0.0);
            assert!((left.end - left.start).length() > (right.end - right.start).length());
            assert!(left.end.x < left.start.x, "the left barb goes left");
            assert!(right.end.x > right.start.x, "the right barb goes right");
        }
        let (first, _) = rachis(0.16);
        let (last, _) = rachis(0.98);
        assert!((barbs[0].start - first).length() < 1e-3);
        assert!((barbs[121].start - last).length() < 1e-3);
    }

    #[test]
    fn barbs_leave_the_shaft_at_the_feather_angle() {
        let pairs = barbs(3);
        let (_, tangent) = rachis(0.57);
        for barb in &pairs[2..4] {
            let chord = barb.end - barb.start;
            let angle = (tangent.x * chord.y - tangent.y * chord.x).atan2(tangent.dot(chord));
            assert!((angle.abs() - BARB_ANGLE).abs() < 1e-3, "{angle}");
            assert!(angle * barb.side > 0.0, "the barb turns to its side");
        }
    }

    #[test]
    fn the_feather_stays_in_its_box_and_is_widest_in_the_middle() {
        let barbs = barbs(61);
        for barb in &barbs {
            for point in [barb.start, barb.control, barb.end] {
                assert!(
                    (0.0..=420.0).contains(&point.x) && (0.0..=900.0).contains(&point.y),
                    "{point:?}"
                );
            }
        }
        let length = |barb: &Barb| (barb.end - barb.start).length();
        let middle = length(&barbs[60]);
        assert!(middle > length(&barbs[0]) && middle > length(&barbs[120]));
    }
}
