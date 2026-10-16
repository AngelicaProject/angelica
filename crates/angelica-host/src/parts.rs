//! Drawing the value parts of plugins' places — meters, rings, readouts,
//! countdowns, sparklines, tiles — and text in its role, with the motion
//! `docs/architecture/ui.md` gives each change: meters and rings move to a
//! new value, a readout shows its change for a moment.
#![forbid(unsafe_code)]
use crate::menu::Look;
use crate::theme::{self, Palette};
use angelica_view::{Countdown, Meter, Readout, Ring, Role, Sparkline, Tone};
use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

/// How long a meter or a ring takes to reach a new value.
const GLIDE: f32 = 0.35;
/// How long a readout's change stays beside it.
const CHANGE: f64 = 1.4;

/// The color of a tone, with `normal` as `normal`.
#[must_use]
pub fn tone_color(p: &Palette, tone: Tone, normal: Color32) -> Color32 {
    match tone {
        Tone::Normal => normal,
        Tone::Muted => p.secondary,
        Tone::Good => p.on,
        Tone::Warning => p.attention,
        Tone::Bad => p.danger,
    }
}

/// The font and color of a role.
#[must_use]
pub fn role_style(look: Look, role: Role, tone: Tone) -> (egui::FontId, Color32) {
    let p = look.p;
    match role {
        Role::Body => (theme::sans(look.at(13.5)), tone_color(p, tone, p.text)),
        Role::Title => (theme::medium(look.at(13.5)), tone_color(p, tone, p.text)),
        Role::Value => (theme::medium(look.at(14.0)), tone_color(p, tone, p.text)),
        Role::Big => (theme::strong(look.at(22.0)), tone_color(p, tone, p.text)),
        Role::Label => (theme::sans(look.at(12.0)), tone_color(p, tone, p.secondary)),
        Role::Small => (theme::sans(look.at(11.5)), tone_color(p, tone, p.secondary)),
    }
}

/// Text in its role.
pub fn text(ui: &mut Ui, look: Look, text: &str, role: Role, tone: Tone) -> egui::Response {
    let (font, color) = role_style(look, role, tone);
    let label = egui::Label::new(egui::RichText::new(text).font(font).color(color));
    // Figures and labels never break inside: a number cut over two lines
    // reads as two numbers.
    match role {
        Role::Value | Role::Big | Role::Label => ui.add(label.extend()),
        Role::Body | Role::Title | Role::Small => ui.add(label.wrap()),
    }
}

fn fill_color(p: &Palette, tone: Tone) -> Color32 {
    tone_color(p, tone, p.action)
}

/// A bar or segments, with the label and the value in words above.
pub fn meter(ui: &mut Ui, look: Look, id: Id, meter: &Meter) -> egui::Response {
    let p = look.p;
    let fraction = {
        #[allow(clippy::cast_possible_truncation)]
        let target = (meter.value / meter.max).clamp(0.0, 1.0) as f32;
        ui.ctx().animate_value_with_time(id, target, GLIDE)
    };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = look.at(3.0);
        if !meter.label.is_empty() || !meter.text.is_empty() {
            ui.horizontal(|ui| {
                if !meter.label.is_empty() {
                    text(ui, look, &meter.label, Role::Label, Tone::Normal);
                }
                if !meter.text.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        text(ui, look, &meter.text, Role::Small, Tone::Normal);
                    });
                }
            });
        }
        let width = ui.available_width().max(look.at(60.0));
        if meter.segments >= 2 {
            let (rect, response) =
                ui.allocate_exact_size(vec2(width, look.at(6.0)), Sense::hover());
            let gap = look.at(3.0);
            let count = f32::from(meter.segments);
            let each = (rect.width() - gap * (count - 1.0)) / count;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let filled = (fraction * count).round() as u8;
            for index in 0..meter.segments {
                let left = rect.left() + f32::from(index) * (each + gap);
                let segment =
                    Rect::from_min_size(pos2(left, rect.top()), vec2(each, rect.height()));
                let color = if index < filled {
                    fill_color(p, meter.tone)
                } else {
                    p.line
                };
                ui.painter()
                    .rect_filled(segment, CornerRadius::same(2), color);
            }
            response
        } else {
            let (rect, response) =
                ui.allocate_exact_size(vec2(width, look.at(4.0)), Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(2), p.line);
            ui.painter().rect_filled(
                rect.with_max_x(rect.left() + rect.width() * fraction),
                CornerRadius::same(2),
                fill_color(p, meter.tone),
            );
            response
        }
    })
    .inner
}

/// A ring with an icon or a short text inside; the icon is drawn by the
/// caller into the returned inner rectangle.
pub fn ring(ui: &mut Ui, look: Look, id: Id, ring: &Ring) -> (egui::Response, Rect) {
    let p = look.p;
    #[allow(clippy::cast_possible_truncation)]
    let target = (ring.value / ring.max).clamp(0.0, 1.0) as f32;
    let fraction = ui.ctx().animate_value_with_time(id, target, GLIDE);
    let size = look.at(f32::from(ring.size));
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter();
    let width = look.at(3.0);
    let radius = size / 2.0 - width;
    painter.circle_stroke(rect.center(), radius, Stroke::new(width, p.line));
    if fraction > 0.0 {
        let full = fraction >= 1.0;
        let color = if full && ring.tone == Tone::Normal {
            p.on
        } else {
            fill_color(p, ring.tone)
        };
        let steps = (64.0 * fraction).ceil().max(2.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let points: Vec<Pos2> = (0..=steps as usize)
            .map(|step| {
                #[allow(clippy::cast_precision_loss)]
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * fraction * (step as f32 / steps);
                rect.center() + radius * vec2(angle.cos(), angle.sin())
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(width, color)));
    }
    if ring.icon.is_none() && !ring.text.is_empty() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            &ring.text,
            theme::medium(look.at(11.0)),
            p.text,
        );
    }
    let inner = Rect::from_center_size(rect.center(), vec2(size - width * 4.5, size - width * 4.5));
    (response, inner)
}

/// What a readout remembers between frames: its last value, the change,
/// and when it changed.
#[derive(Clone, Copy)]
struct Seen {
    value: f64,
    change: f64,
    at: f64,
}

/// A number with its change beside it for a moment after it changes.
pub fn readout(ui: &mut Ui, look: Look, id: Id, readout: &Readout) -> egui::Response {
    let p = look.p;
    let now = ui.input(|input| input.time);
    let seen = ui.data_mut(|data| {
        let seen = data.get_temp::<Seen>(id).unwrap_or(Seen {
            value: readout.value,
            change: 0.0,
            at: f64::NEG_INFINITY,
        });
        let next = if seen.value.to_bits() == readout.value.to_bits() {
            seen
        } else {
            Seen {
                value: readout.value,
                change: readout.value - seen.value,
                at: now,
            }
        };
        data.insert_temp(id, next);
        next
    });
    let decimals = usize::from(readout.decimals);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = look.at(6.0);
        let number = format!("{}{}", grouped(readout.value, decimals), readout.unit);
        let response = text(ui, look, &number, readout.role, readout.tone);
        let age = now - seen.at;
        if age < CHANGE && seen.change != 0.0 {
            #[allow(clippy::cast_possible_truncation)]
            let fade = (1.0 - (age / CHANGE) as f32).clamp(0.0, 1.0);
            let color = if seen.change > 0.0 { p.on } else { p.danger };
            let sign = if seen.change > 0.0 { "+" } else { "−" };
            ui.label(
                egui::RichText::new(format!("{sign}{}", grouped(seen.change.abs(), decimals)))
                    .font(theme::sans(look.at(11.5)))
                    .color(theme::alpha(color, fade)),
            );
            ui.ctx().request_repaint();
        }
        response
    })
    .inner
}

/// A number with thin spaces between groups of three digits.
#[must_use]
pub fn grouped(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value.abs());
    let (whole, fraction) = text
        .split_once('.')
        .map_or((text.as_str(), None), |(w, f)| (w, Some(f)));
    let mut out = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(digit);
    }
    if let Some(fraction) = fraction {
        out.push('.');
        out.push_str(fraction);
    }
    if value < 0.0 {
        out.insert(0, '−');
    }
    out
}

/// The time left until a countdown's moment, `h:mm:ss` or `m:ss`; its
/// `done` text once it runs out.
#[must_use]
pub fn time_left(countdown: &Countdown, now_ms: u64) -> Option<String> {
    let left = countdown.until.saturating_sub(now_ms);
    if left == 0 {
        return None;
    }
    let seconds = left.div_ceil(1000);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    Some(if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    })
}

/// A countdown the host counts down itself.
pub fn countdown(ui: &mut Ui, look: Look, countdown: &Countdown) -> egui::Response {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(250));
    match time_left(countdown, now) {
        Some(left) => text(ui, look, &left, countdown.role, Tone::Normal),
        None => text(ui, look, &countdown.done, countdown.role, Tone::Warning),
    }
}

/// Recent values as a line over a faint fill, scaled between the smallest
/// and the largest.
pub fn sparkline(ui: &mut Ui, look: Look, sparkline: &Sparkline) -> egui::Response {
    let p = look.p;
    let width = ui.available_width().max(look.at(80.0));
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, look.at(f32::from(sparkline.height))),
        Sense::hover(),
    );
    let values = &sparkline.values;
    let low = values.iter().copied().fold(f32::INFINITY, f32::min);
    let high = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let span = (high - low).max(f32::EPSILON);
    #[allow(clippy::cast_precision_loss)]
    let step = rect.width() / (values.len().saturating_sub(1).max(1)) as f32;
    let points: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            #[allow(clippy::cast_precision_loss)]
            let x = rect.left() + step * index as f32;
            let y = rect.bottom() - 1.0 - (rect.height() - 2.0) * (value - low) / span;
            pos2(x, y)
        })
        .collect();
    let mut area = points.clone();
    area.push(rect.right_bottom());
    area.push(rect.left_bottom());
    let fill = theme::alpha(p.action, 0.10);
    // The fill under the line, as columns between neighbouring points.
    for pair in points.windows(2) {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                pair[0],
                pair[1],
                pos2(pair[1].x, rect.bottom()),
                pos2(pair[0].x, rect.bottom()),
            ],
            fill,
            Stroke::NONE,
        ));
    }
    ui.painter().add(egui::Shape::line(
        points,
        Stroke::new(look.at(1.5), p.action),
    ));
    response
}

/// A tile with the first letter of `text`.
pub fn tile(ui: &mut Ui, look: Look, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(look.at(22.0)), Sense::hover());
    theme::tile(ui.painter(), rect, text, false, false, look.p);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdowns_read_like_clocks_and_numbers_are_grouped() {
        let countdown = Countdown::new(10_000_000);
        assert_eq!(time_left(&countdown, 10_000_000), None);
        assert_eq!(time_left(&countdown, 9_999_500).as_deref(), Some("0:01"));
        assert_eq!(
            time_left(&countdown, 10_000_000 - 754_000).as_deref(),
            Some("12:34")
        );
        assert_eq!(
            time_left(&countdown, 10_000_000 - 5_320_000).as_deref(),
            Some("1:28:40")
        );
        assert_eq!(grouped(48_210.0, 0), "48\u{202f}210");
        assert_eq!(grouped(-1_234_567.25, 2), "−1\u{202f}234\u{202f}567.25");
        assert_eq!(grouped(999.0, 0), "999");
    }
}
