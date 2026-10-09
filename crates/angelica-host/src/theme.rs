//! Angelica's look over the game: colors, type, and the small drawn pieces
//! its menu and plugin windows share. A face of soft night colors and one
//! warm light for the main action; the mechanism in pale blueprint lines.
#![forbid(unsafe_code)]
use crate::fonts::{SOFT, SOFT_LIGHT, STRONG};
use egui::{Color32, CornerRadius, FontFamily, FontId, Pos2, Stroke, TextStyle, Vec2};

/// The night behind the menu, top and bottom.
pub const NIGHT: Color32 = Color32::from_rgb(0x0b, 0x12, 0x20);
pub const NIGHT_LOW: Color32 = Color32::from_rgb(0x1d, 0x2b, 0x45);
/// The face's own color: names and titles.
pub const PEARL: Color32 = Color32::from_rgb(0xf4, 0xf2, 0xee);
/// Body text.
pub const TEXT: Color32 = Color32::from_rgb(0xdf, 0xe6, 0xef);
/// Text the player reads second.
pub const SOFT_TEXT: Color32 = Color32::from_rgb(0xc9, 0xd1, 0xdd);
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa6, 0xb8);
pub const FAINT: Color32 = Color32::from_rgb(0x8d, 0x98, 0xa8);
/// The mechanism: blueprint lines and their labels.
pub const BLUEPRINT: Color32 = Color32::from_rgb(0x3f, 0x7f, 0xa8);
pub const BLUEPRINT_PALE: Color32 = Color32::from_rgb(0x8f, 0xb7, 0xd6);
/// The one warm light: the main action and what waits for the player.
pub const LIGHT: Color32 = Color32::from_rgb(0xe3, 0xb7, 0x68);
pub const LIGHT_TEXT: Color32 = Color32::from_rgb(0xf0, 0xc2, 0x7a);
/// Text on the light.
pub const ON_LIGHT: Color32 = Color32::from_rgb(0x22, 0x18, 0x0a);
/// A plugin's state: running, updating, and off.
pub const ON: Color32 = Color32::from_rgb(0x9f, 0xd5, 0x9a);
pub const UPDATING: Color32 = Color32::from_rgb(0xbf, 0xe0, 0xff);
pub const OFF: Color32 = Color32::from_rgb(0x7d, 0x87, 0x97);
/// The most serious class of permissions.
pub const EMBER: Color32 = Color32::from_rgb(0xee, 0x9a, 0x6a);

/// `color` with `alpha` from 0 to 1.
#[must_use]
pub fn alpha(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, unit_byte(alpha))
}

/// A fraction from 0 to 1 as a byte.
fn unit_byte(value: f32) -> u8 {
    num_traits::cast(value.clamp(0.0, 1.0) * 255.0).unwrap_or(0)
}

/// The soft italic at `size` points.
#[must_use]
pub fn soft(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SOFT.into()))
}

/// The light soft italic at `size` points.
#[must_use]
pub fn soft_light(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SOFT_LIGHT.into()))
}

/// Manrope semibold at `size` points.
#[must_use]
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(STRONG.into()))
}

#[must_use]
pub fn sans(size: f32) -> FontId {
    FontId::proportional(size)
}

#[must_use]
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Applies Angelica's style to everything egui draws: the menu and the
/// windows of plugins, whose authors do nothing for it.
pub fn apply(context: &egui::Context) {
    context.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, sans(12.5)),
            (TextStyle::Body, sans(15.0)),
            (TextStyle::Button, strong(14.5)),
            (TextStyle::Heading, soft(26.0)),
            (TextStyle::Monospace, mono(13.0)),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(8.0, 7.0);
        spacing.button_padding = Vec2::new(14.0, 5.0);
        spacing.window_margin = egui::Margin::symmetric(20, 16);
        spacing.interact_size.y = 26.0;
        spacing.slider_rail_height = 4.0;
        style.visuals = visuals();
    });
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    let quiet = Color32::from_rgba_unmultiplied(255, 255, 255, 18);
    let round = CornerRadius::same(4);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = alpha(NIGHT, 0.0);
    widgets.noninteractive.weak_bg_fill = alpha(NIGHT, 0.0);
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, quiet);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = round;
    widgets.inactive.weak_bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 16);
    widgets.inactive.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 30);
    widgets.inactive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 40));
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.corner_radius = round;
    widgets.hovered.weak_bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 30);
    widgets.hovered.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 40);
    widgets.hovered.bg_stroke = Stroke::new(1.0, alpha(LIGHT, 0.55));
    widgets.hovered.fg_stroke = Stroke::new(1.5, PEARL);
    widgets.hovered.corner_radius = round;
    widgets.active.weak_bg_fill = alpha(LIGHT, 0.32);
    widgets.active.bg_fill = alpha(LIGHT, 0.45);
    widgets.active.bg_stroke = Stroke::new(1.0, LIGHT);
    widgets.active.fg_stroke = Stroke::new(1.5, PEARL);
    widgets.active.corner_radius = round;
    widgets.open = widgets.hovered;
    // The title bar of the window the player last used.
    widgets.open.weak_bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 8);
    visuals.selection.bg_fill = alpha(LIGHT, 0.42);
    visuals.selection.stroke = Stroke::new(1.0, LIGHT);
    visuals.hyperlink_color = BLUEPRINT_PALE;
    visuals.faint_bg_color = Color32::from_rgba_unmultiplied(255, 255, 255, 7);
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(5, 9, 16, 170);
    visuals.code_bg_color = Color32::from_rgba_unmultiplied(63, 127, 168, 40);
    visuals.warn_fg_color = LIGHT_TEXT;
    visuals.error_fg_color = EMBER;
    visuals.weak_text_color = Some(MUTED);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.window_fill = Color32::from_rgba_unmultiplied(14, 20, 32, 236);
    visuals.panel_fill = visuals.window_fill;
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36));
    visuals.window_shadow = egui::Shadow {
        offset: [0, 14],
        blur: 40,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    visuals.window_highlight_topmost = false;
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.slider_trailing_fill = true;
    visuals.collapsing_header_frame = false;
    visuals.striped = false;
    visuals
}

/// A soft round glow: `color` at the center fading out at `radius`.
pub fn glow(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    const SEGMENTS: u32 = 20;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    for index in 0..SEGMENTS {
        let angle = std::f32::consts::TAU * f32_of(index) / f32_of(SEGMENTS);
        mesh.colored_vertex(center + radius * Vec2::angled(angle), Color32::TRANSPARENT);
    }
    for index in 0..SEGMENTS {
        mesh.add_triangle(0, 1 + index, 1 + (index + 1) % SEGMENTS);
    }
    painter.add(mesh);
}

/// A small number as `f32`.
#[must_use]
pub fn f32_of(value: u32) -> f32 {
    num_traits::cast(value).unwrap_or(0.0)
}

/// A vertical gradient over `rect` from `top` to `bottom`.
pub fn gradient(painter: &egui::Painter, rect: egui::Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// A check mark in a box of `size` centered at `center`.
pub fn check(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let h = size / 2.0;
    painter.add(egui::Shape::line(
        vec![
            center + Vec2::new(-h * 0.8, 0.0),
            center + Vec2::new(-h * 0.2, h * 0.6),
            center + Vec2::new(h * 0.85, -h * 0.65),
        ],
        Stroke::new(1.6, color),
    ));
}

/// A turning arrow: the card turns over.
pub fn turn(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    let start = -0.4_f32;
    let points: Vec<Pos2> = (0..=16)
        .map(|index| {
            let angle = start + 5.0 * f32_of(index) / 16.0;
            center + radius * Vec2::angled(angle)
        })
        .collect();
    let tip = points[0];
    painter.add(egui::Shape::line(points, stroke));
    let along = Vec2::angled(start - std::f32::consts::FRAC_PI_2);
    let out = Vec2::angled(start);
    painter.line_segment([tip, tip + (along + out) * radius * 0.55], stroke);
    painter.line_segment([tip, tip + (along - out) * radius * 0.55], stroke);
}

/// A small diamond, the mark of a class of permissions.
pub fn diamond(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + Vec2::new(0.0, -radius),
            center + Vec2::new(radius, 0.0),
            center + Vec2::new(0.0, radius),
            center + Vec2::new(-radius, 0.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// How a button of the menu looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// The one main action: the light.
    Main,
    /// A quiet outlined action.
    Ghost,
    /// Words only.
    Quiet,
}

/// A pill button of the menu, `scale` times its normal size.
pub fn button(
    ui: &mut egui::Ui,
    text: &str,
    kind: Button,
    enabled: bool,
    scale: f32,
) -> egui::Response {
    let font = strong(14.5 * scale);
    let color = match kind {
        Button::Main => ON_LIGHT,
        Button::Ghost => PEARL,
        Button::Quiet => SOFT_TEXT,
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let padding = Vec2::new(if kind == Button::Quiet { 6.0 } else { 20.0 }, 9.0) * scale;
    let size = galley.size() + 2.0 * padding;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let hovered = enabled && response.hovered();
        let opacity = if enabled { 1.0 } else { 0.45 };
        let radius = CornerRadius::same(u8::MAX);
        match kind {
            Button::Main => {
                glow(
                    painter,
                    rect.center() + Vec2::new(0.0, 5.0 * scale),
                    rect.width() * 0.42,
                    alpha(LIGHT, 0.13 * opacity),
                );
                let fill = if hovered {
                    Color32::from_rgb(0xec, 0xc4, 0x7c)
                } else {
                    LIGHT
                };
                painter.rect_filled(rect, radius, alpha(fill, opacity));
            }
            Button::Ghost => {
                let line = if hovered {
                    alpha(LIGHT, 0.7)
                } else {
                    Color32::from_rgba_unmultiplied(255, 255, 255, 70)
                };
                if hovered {
                    painter.rect_filled(
                        rect,
                        radius,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                    );
                }
                painter.rect_stroke(
                    rect,
                    radius,
                    Stroke::new(1.0, alpha(line, opacity)),
                    egui::StrokeKind::Inside,
                );
            }
            Button::Quiet => {}
        }
        let text_color = match kind {
            Button::Quiet if hovered => PEARL,
            _ => color,
        };
        painter.galley_with_override_text_color(
            rect.center() - galley.size() / 2.0,
            galley,
            alpha(text_color, opacity),
        );
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A switch: on is the light, off a quiet track.
pub fn switch(ui: &mut egui::Ui, on: bool, scale: f32) -> egui::Response {
    let size = Vec2::new(40.0, 23.0) * scale;
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let progress = ui.ctx().animate_bool(response.id, on);
        paint_switch(ui.painter(), rect, progress, response.hovered());
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Paints a switch in `rect`, `progress` of the way from off to on.
pub fn paint_switch(painter: &egui::Painter, rect: egui::Rect, progress: f32, hovered: bool) {
    let pill = CornerRadius::same(u8::MAX);
    let track = lerp_color(
        Color32::from_rgba_unmultiplied(255, 255, 255, 38),
        LIGHT,
        progress,
    );
    painter.rect_filled(rect, pill, track);
    let inset = rect.height() * 0.14;
    let radius = rect.height() / 2.0 - inset;
    let x = egui::lerp(
        rect.left() + inset + radius..=rect.right() - inset - radius,
        progress,
    );
    let knob = lerp_color(SOFT_TEXT, ON_LIGHT, progress);
    painter.circle_filled(Pos2::new(x, rect.center().y), radius, knob);
    if hovered {
        painter.rect_stroke(
            rect,
            pill,
            Stroke::new(1.0, alpha(PEARL, 0.35)),
            egui::StrokeKind::Outside,
        );
    }
}

fn lerp_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let a = from.to_array();
    let b = to.to_array();
    let mix = |index: usize| -> u8 {
        num_traits::cast(egui::lerp(
            f32::from(a[index])..=f32::from(b[index]),
            t.clamp(0.0, 1.0),
        ))
        .unwrap_or(b[index])
    };
    Color32::from_rgba_premultiplied(mix(0), mix(1), mix(2), mix(3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_keeps_the_color_and_clamps() {
        assert_eq!(alpha(LIGHT, 1.0), LIGHT);
        assert_eq!(alpha(LIGHT, 2.0), LIGHT);
        assert_eq!(alpha(LIGHT, -1.0).a(), 0);
    }
}
