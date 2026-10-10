//! Angelica's look over the game: two themes, light and dark, the type, and
//! the small drawn pieces the menu and plugin windows share. Color carries
//! meaning only; controls are words, not boxes
//! (`docs/architecture/ui.md#look`).
#![forbid(unsafe_code)]
use crate::fonts::{MEDIUM, STRONG, WORDMARK};
use egui::{
    Color32, CornerRadius, FontFamily, FontId, Pos2, Rect, Sense, Shadow, Stroke, TextStyle, Ui,
    Vec2, pos2, vec2,
};
use serde::{Deserialize, Serialize};

/// The player's choice of theme.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    #[serde(rename = "light")]
    Light,
    #[serde(rename = "dark")]
    Dark,
}

/// The player's choice of how large Angelica's windows are.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Size {
    #[serde(rename = "small")]
    Small,
    #[default]
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "large")]
    Large,
}

impl Size {
    pub const ALL: [Self; 3] = [Self::Small, Self::Normal, Self::Large];

    /// How many times its normal size the menu is drawn on a screen
    /// `height` points high. It grows slower than the screen, so a high
    /// resolution does not make it large.
    #[must_use]
    pub fn scale(self, height: f32) -> f32 {
        let factor = match self {
            Self::Small => 0.8,
            Self::Normal => 0.9,
            Self::Large => 1.05,
        };
        (factor * (height / 1080.0).max(0.0).sqrt()).clamp(0.7, 2.2)
    }
}

/// The colors of a theme, each with one role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub window: Color32,
    /// The bar with the word `angelica`.
    pub title: Color32,
    /// Menus and tooltips.
    pub popup: Color32,
    /// The column of sections in Settings.
    pub column: Color32,
    /// The chosen section and the active tab.
    pub chosen: Color32,
    /// The chosen row of a list, and the hovered one.
    pub selection: Color32,
    pub hover: Color32,
    /// Text fields.
    pub field: Color32,
    /// The tile behind a plugin's letter.
    pub tile: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub muted: Color32,
    pub line: Color32,
    pub border: Color32,
    /// What can be pressed or changed.
    pub action: Color32,
    pub attention: Color32,
    /// Text on the attention color.
    pub on_attention: Color32,
    pub on: Color32,
    pub danger: Color32,
    /// How dark the shadow under windows is, out of 255.
    pub shadow: u8,
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// Warm sand: the default.
pub const LIGHT: Palette = Palette {
    window: rgba(0xf6, 0xf3, 0xed, 245),
    title: rgba(0, 0, 0, 8),
    popup: rgba(0xf6, 0xf2, 0xeb, 250),
    column: rgba(0x7a, 0x5c, 0x2e, 20),
    chosen: rgba(255, 255, 255, 191),
    selection: rgba(0x78, 0x5a, 0x28, 31),
    hover: rgba(0x78, 0x5a, 0x28, 15),
    field: rgba(255, 255, 255, 255),
    tile: rgba(0x78, 0x5a, 0x28, 36),
    text: rgba(0x24, 0x21, 0x1c, 255),
    secondary: rgba(0x24, 0x21, 0x1c, 153),
    muted: rgba(0x24, 0x21, 0x1c, 107),
    line: rgba(0x3c, 0x2d, 0x14, 31),
    border: rgba(0x3c, 0x2d, 0x14, 56),
    action: rgba(0x7a, 0x5c, 0x2e, 255),
    attention: rgba(0xb7, 0x6e, 0x00, 255),
    on_attention: rgba(255, 255, 255, 255),
    on: rgba(0x2e, 0x7d, 0x46, 255),
    danger: rgba(0xb2, 0x3a, 0x3a, 255),
    shadow: 100,
};

/// Slate with muted lilac.
pub const DARK: Palette = Palette {
    window: rgba(0x2e, 0x31, 0x38, 247),
    title: rgba(0, 0, 0, 36),
    popup: rgba(0x28, 0x2a, 0x30, 250),
    column: rgba(0x28, 0x2a, 0x30, 255),
    chosen: rgba(0x3c, 0x3f, 0x47, 255),
    selection: rgba(255, 255, 255, 18),
    hover: rgba(255, 255, 255, 10),
    field: rgba(0, 0, 0, 51),
    tile: rgba(0xb4, 0xab, 0xd6, 41),
    text: rgba(0xdc, 0xdd, 0xe0, 255),
    secondary: rgba(0xa3, 0xa6, 0xad, 255),
    muted: rgba(0x80, 0x83, 0x8b, 255),
    line: rgba(255, 255, 255, 20),
    border: rgba(255, 255, 255, 33),
    action: rgba(0xb4, 0xab, 0xd6, 255),
    attention: rgba(0xe0, 0xa8, 0x4e, 255),
    on_attention: rgba(0x1e, 0x15, 0x05, 255),
    on: rgba(0x7c, 0xc4, 0x8e, 255),
    danger: rgba(0xe5, 0x8a, 0x8a, 255),
    shadow: 140,
};

impl Theme {
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];

    #[must_use]
    pub fn palette(self) -> &'static Palette {
        match self {
            Self::Light => &LIGHT,
            Self::Dark => &DARK,
        }
    }
}

/// `color` with `alpha` from 0 to 1 of its own opacity.
#[must_use]
pub fn alpha(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, unit_byte(f32::from(a) / 255.0 * alpha))
}

/// A fraction from 0 to 1 as a byte.
fn unit_byte(value: f32) -> u8 {
    num_traits::cast(value.clamp(0.0, 1.0) * 255.0).unwrap_or(0)
}

/// Golos Text at `size` points.
#[must_use]
pub fn sans(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Golos Text medium at `size` points.
#[must_use]
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

/// Golos Text semibold at `size` points.
#[must_use]
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(STRONG.into()))
}

/// The face of the word `angelica`.
#[must_use]
pub fn wordmark(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(WORDMARK.into()))
}

#[must_use]
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Where the context keeps the theme in use.
fn theme_id() -> egui::Id {
    egui::Id::new("angelica-theme")
}

/// The theme everything is drawn with.
#[must_use]
pub fn current(context: &egui::Context) -> Theme {
    context.data(|data| data.get_temp(theme_id()).unwrap_or_default())
}

/// Applies `theme` to everything egui draws: the menu and the windows of
/// plugins, whose authors do nothing for it.
pub fn apply(context: &egui::Context, theme: Theme) {
    context.data_mut(|data| data.insert_temp(theme_id(), theme));
    let palette = theme.palette();
    context.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, sans(12.0)),
            (TextStyle::Body, sans(13.5)),
            (TextStyle::Button, medium(13.5)),
            (TextStyle::Heading, strong(16.0)),
            (TextStyle::Monospace, mono(12.5)),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = Vec2::new(8.0, 6.0);
        spacing.button_padding = Vec2::new(10.0, 4.0);
        spacing.window_margin = egui::Margin::symmetric(14, 12);
        spacing.menu_margin = egui::Margin::same(5);
        spacing.interact_size.y = 24.0;
        spacing.slider_rail_height = 2.0;
        style.visuals = visuals(theme, palette);
    });
}

fn visuals(theme: Theme, p: &Palette) -> egui::Visuals {
    let mut visuals = match theme {
        Theme::Light => egui::Visuals::light(),
        Theme::Dark => egui::Visuals::dark(),
    };
    let round = CornerRadius::same(4);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    widgets.noninteractive.corner_radius = round;
    widgets.inactive.weak_bg_fill = p.hover;
    widgets.inactive.bg_fill = p.selection;
    widgets.inactive.bg_stroke = Stroke::new(1.0, p.line);
    widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    widgets.inactive.corner_radius = round;
    widgets.hovered.weak_bg_fill = p.selection;
    widgets.hovered.bg_fill = p.selection;
    widgets.hovered.bg_stroke = Stroke::new(1.0, alpha(p.action, 0.6));
    widgets.hovered.fg_stroke = Stroke::new(1.5, p.text);
    widgets.hovered.corner_radius = round;
    widgets.active.weak_bg_fill = alpha(p.action, 0.25);
    widgets.active.bg_fill = alpha(p.action, 0.35);
    widgets.active.bg_stroke = Stroke::new(1.0, p.action);
    widgets.active.fg_stroke = Stroke::new(1.5, p.text);
    widgets.active.corner_radius = round;
    widgets.open = widgets.hovered;
    visuals.selection.bg_fill = alpha(p.action, 0.3);
    visuals.selection.stroke = Stroke::new(1.0, p.action);
    visuals.hyperlink_color = p.action;
    visuals.faint_bg_color = p.hover;
    visuals.extreme_bg_color = p.field;
    visuals.code_bg_color = p.tile;
    visuals.warn_fg_color = p.attention;
    visuals.error_fg_color = p.danger;
    visuals.weak_text_color = Some(p.secondary);
    visuals.text_cursor.stroke = Stroke::new(1.5, p.text);
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.window_fill = p.window;
    visuals.panel_fill = p.window;
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(p.shadow),
    };
    visuals.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(p.shadow),
    };
    visuals.window_highlight_topmost = false;
    visuals.menu_corner_radius = CornerRadius::same(4);
    visuals.slider_trailing_fill = true;
    visuals.collapsing_header_frame = false;
    visuals.striped = false;
    visuals
}

/// How an action word looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Word {
    /// The main action of a window: bold, in the action color, with an
    /// arrow.
    Main,
    /// An action in a list row or a value to change: the action color.
    Action,
    /// Any other action: the secondary text color.
    Secondary,
    /// What cannot be undone: the danger color.
    Danger,
}

/// An action word, `k` times its normal size. It is underlined under the
/// pointer.
pub fn word(ui: &mut Ui, text: &str, kind: Word, enabled: bool, k: f32) -> egui::Response {
    let p = current(ui.ctx()).palette();
    let (text, font, color) = match kind {
        Word::Main => (format!("{text}  →"), strong(13.5 * k), p.action),
        Word::Action => (text.to_owned(), medium(13.5 * k), p.action),
        Word::Secondary => (text.to_owned(), sans(13.5 * k), p.secondary),
        Word::Danger => (text.to_owned(), sans(13.5 * k), p.danger),
    };
    let galley = ui.painter().layout_no_wrap(text, font, color);
    let padding = vec2(4.0, 5.0) * k;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(galley.size() + 2.0 * padding, sense);
    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        let color = if !enabled {
            alpha(color, 0.45)
        } else if hovered && kind == Word::Secondary {
            p.text
        } else {
            color
        };
        let at = rect.center() - galley.size() / 2.0;
        let width = galley.size().x;
        let bottom = at.y + galley.size().y;
        ui.painter()
            .galley_with_override_text_color(at, galley, color);
        if hovered {
            ui.painter().line_segment(
                [pos2(at.x, bottom), pos2(at.x + width, bottom)],
                Stroke::new(1.0, alpha(color, 0.6)),
            );
        }
    }
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A choice of short words: the chosen one bold in the text color, the
/// others muted. Returns the index the player picked.
pub fn choice(ui: &mut Ui, options: &[&str], chosen: usize, k: f32) -> Option<usize> {
    let p = current(ui.ctx()).palette();
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0 * k;
        for (index, option) in options.iter().enumerate() {
            let is_chosen = index == chosen;
            let font = if is_chosen {
                strong(13.5 * k)
            } else {
                sans(13.5 * k)
            };
            let galley = ui
                .painter()
                .layout_no_wrap((*option).to_owned(), font, p.text);
            let (rect, response) = ui.allocate_exact_size(
                galley.size() + vec2(0.0, 10.0 * k),
                if is_chosen {
                    Sense::hover()
                } else {
                    Sense::click()
                },
            );
            let color = if is_chosen {
                p.text
            } else if response.hovered() {
                p.secondary
            } else {
                p.muted
            };
            ui.painter().galley_with_override_text_color(
                rect.center() - galley.size() / 2.0,
                galley,
                color,
            );
            if !is_chosen
                && response
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
            {
                picked = Some(index);
            }
        }
    });
    picked
}

/// A value the player changes from a menu: the value in the action color
/// with a small chevron.
pub fn value(ui: &mut Ui, text: &str, k: f32) -> egui::Response {
    let p = current(ui.ctx()).palette();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), medium(13.5 * k), p.action);
    let chevron = 12.0 * k;
    let size = galley.size() + vec2(chevron + 4.0 * k, 10.0 * k);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let at = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    let right = at.x + galley.size().x;
    ui.painter()
        .galley_with_override_text_color(at, galley, p.action);
    icon(
        ui.painter(),
        Icon::Chevron,
        Rect::from_center_size(
            pos2(right + 4.0 * k + chevron / 2.0, rect.center().y),
            Vec2::splat(chevron),
        ),
        p.action,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A key as a small token in the action color.
pub fn key(ui: &mut Ui, text: &str, k: f32) -> egui::Response {
    let p = current(ui.ctx()).palette();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), strong(12.0 * k), p.action);
    let size = galley.size() + vec2(16.0 * k, 6.0 * k);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), alpha(p.action, 0.1));
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, alpha(p.action, 0.4)),
        egui::StrokeKind::Inside,
    );
    painter.galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, p.action);
    response
}

/// A small header over a part of a list: its title, a count, and a thin
/// rule to the right edge.
pub fn header(ui: &mut Ui, title: &str, count: Option<usize>, color: Color32, k: f32) {
    let p = current(ui.ctx()).palette();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0 * k), Sense::hover());
    let painter = ui.painter();
    let left = rect.left() + 12.0 * k;
    let y = rect.bottom() - 6.0 * k;
    let title = painter.text(
        pos2(left, y),
        egui::Align2::LEFT_BOTTOM,
        title,
        sans(12.0 * k),
        color,
    );
    let mut end = title.right();
    if let Some(count) = count {
        end = painter
            .text(
                pos2(end + 6.0 * k, y),
                egui::Align2::LEFT_BOTTOM,
                count.to_string(),
                sans(12.0 * k),
                p.muted,
            )
            .right();
    }
    let line_y = title.center().y;
    if end + 12.0 * k < rect.right() - 12.0 * k {
        painter.line_segment(
            [
                pos2(end + 12.0 * k, line_y),
                pos2(rect.right() - 12.0 * k, line_y),
            ],
            Stroke::new(1.0, p.line),
        );
    }
}

/// A thin rule across `ui`.
pub fn rule(ui: &mut Ui) {
    let p = current(ui.ctx()).palette();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0, p.line),
    );
}

/// The small drawn icons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Search,
    Close,
    Chevron,
    /// The classes of permissions.
    Network,
    OtherPlayers,
    WhatYouSee,
    YoursOnly,
    /// The sections of Settings.
    General,
    Updates,
    Developers,
}

/// Draws `icon` in `rect` with thin lines of `color`.
pub fn icon(painter: &egui::Painter, icon: Icon, rect: Rect, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let at = |x: f32, y: f32| c + vec2(x, y) * (s / 24.0);
    let stroke = Stroke::new((s / 12.0).clamp(1.0, 2.0), color);
    let r = s / 24.0;
    match icon {
        Icon::Search => {
            painter.circle_stroke(at(-2.0, -2.0), 7.0 * r, stroke);
            painter.line_segment([at(3.0, 3.0), at(8.5, 8.5)], stroke);
        }
        Icon::Close => {
            painter.line_segment([at(-6.0, -6.0), at(6.0, 6.0)], stroke);
            painter.line_segment([at(6.0, -6.0), at(-6.0, 6.0)], stroke);
        }
        Icon::Chevron => {
            painter.add(egui::Shape::line(
                vec![at(-5.0, -2.5), at(0.0, 2.5), at(5.0, -2.5)],
                stroke,
            ));
        }
        Icon::Network => {
            painter.circle_stroke(c, 9.0 * r, stroke);
            painter.line_segment([at(-9.0, 0.0), at(9.0, 0.0)], stroke);
            painter.add(egui::Shape::ellipse_stroke(
                c,
                vec2(4.0 * r, 9.0 * r),
                stroke,
            ));
        }
        Icon::OtherPlayers => {
            painter.circle_stroke(at(-3.0, -4.0), 3.5 * r, stroke);
            painter.add(egui::Shape::line(arc(at(-3.0, 9.0), 7.0 * r), stroke));
            painter.circle_stroke(at(5.5, -2.5), 2.8 * r, stroke);
            painter.add(egui::Shape::line(
                vec![at(5.0, 3.5), at(8.5, 4.5), at(10.0, 9.0)],
                stroke,
            ));
        }
        Icon::WhatYouSee => {
            painter.rect_stroke(
                Rect::from_min_max(at(-9.0, -7.0), at(9.0, 5.0)),
                CornerRadius::same(2),
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([at(-4.0, 9.0), at(4.0, 9.0)], stroke);
            painter.line_segment([at(0.0, 5.0), at(0.0, 9.0)], stroke);
        }
        Icon::YoursOnly => {
            painter.circle_stroke(at(0.0, -4.0), 4.0 * r, stroke);
            painter.add(egui::Shape::line(arc(at(0.0, 10.0), 8.0 * r), stroke));
        }
        Icon::General => {
            for (y, x) in [(-6.0, -3.0), (0.0, 4.0), (6.0, -1.0)] {
                painter.line_segment([at(-9.0, y), at(9.0, y)], stroke);
                painter.circle_filled(at(x, y), 2.2 * r, color);
            }
        }
        Icon::Updates => {
            let points: Vec<Pos2> = (0..=20)
                .map(|step| {
                    let angle = -0.9 + 5.0 * step_f32(step) / 20.0;
                    c + 8.0 * r * Vec2::angled(angle)
                })
                .collect();
            let tip = points[0];
            painter.add(egui::Shape::line(points, stroke));
            painter.line_segment([tip, tip + vec2(0.0, -4.5 * r)], stroke);
            painter.line_segment([tip, tip + vec2(-4.5 * r, 0.5 * r)], stroke);
        }
        Icon::Developers => {
            painter.add(egui::Shape::line(
                vec![at(-4.0, -6.0), at(-9.0, 0.0), at(-4.0, 6.0)],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![at(4.0, -6.0), at(9.0, 0.0), at(4.0, 6.0)],
                stroke,
            ));
        }
    }
}

/// The top half of a circle of `radius` around `center`: shoulders.
fn arc(center: Pos2, radius: f32) -> Vec<Pos2> {
    (0..=12)
        .map(|step| {
            let angle = std::f32::consts::PI * (1.0 + step_f32(step) / 12.0);
            center + radius * Vec2::angled(angle)
        })
        .collect()
}

fn step_f32(step: u32) -> f32 {
    num_traits::cast(step).unwrap_or(0.0)
}

/// The mark of a plugin that needs the player: `!` in the attention color.
pub fn attention(painter: &egui::Painter, center: Pos2, k: f32, p: &Palette) {
    painter.circle_filled(center, 7.5 * k, p.attention);
    painter.text(
        center + vec2(0.0, 0.5 * k),
        egui::Align2::CENTER_CENTER,
        "!",
        strong(10.5 * k),
        p.on_attention,
    );
}

/// The mark of a running plugin: a small dot in the "on" color.
pub fn running(painter: &egui::Painter, center: Pos2, k: f32, p: &Palette) {
    painter.circle_filled(center, 3.0 * k, p.on);
}

/// The mark of a plugin that is off: a hollow ring.
pub fn off(painter: &egui::Painter, center: Pos2, k: f32, p: &Palette) {
    painter.circle_stroke(center, 3.0 * k, Stroke::new(1.0, p.muted));
}

/// A plugin's tile: a rounded square with the first letter of its name.
pub fn tile(painter: &egui::Painter, rect: Rect, name: &str, faded: bool, p: &Palette) {
    let opacity = if faded { 0.45 } else { 1.0 };
    painter.rect_filled(rect, CornerRadius::same(5), alpha(p.tile, opacity));
    painter.rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0, alpha(p.line, opacity)),
        egui::StrokeKind::Inside,
    );
    let letter: String = name
        .chars()
        .next()
        .into_iter()
        .flat_map(char::to_uppercase)
        .collect();
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        strong(rect.height() * 0.5),
        alpha(p.action, opacity),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_scales_the_opacity_and_clamps() {
        assert_eq!(alpha(LIGHT.text, 1.0), LIGHT.text);
        assert_eq!(alpha(LIGHT.text, 2.0), LIGHT.text);
        assert_eq!(alpha(LIGHT.text, -1.0).a(), 0);
        assert_eq!(alpha(rgba(1, 2, 3, 200), 0.5).a(), 100);
    }

    #[test]
    fn both_themes_tell_states_apart_from_text_and_each_other() {
        for theme in Theme::ALL {
            let p = theme.palette();
            let marks = [p.action, p.attention, p.on, p.danger, p.text];
            for (index, color) in marks.iter().enumerate() {
                for other in &marks[index + 1..] {
                    assert_ne!(color, other, "{theme:?}");
                }
            }
        }
    }

    #[test]
    fn sizes_grow_in_order_and_slower_than_the_screen() {
        for height in [720.0, 1080.0, 1440.0, 2160.0] {
            let [small, normal, large] = Size::ALL.map(|size| size.scale(height));
            assert!(small < normal && normal < large, "{height}");
        }
        let normal = |height| Size::Normal.scale(height);
        assert!(normal(1440.0) / normal(1080.0) < 1440.0 / 1080.0);
        assert!((normal(1080.0) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn the_theme_is_kept_in_the_context() {
        let context = egui::Context::default();
        assert_eq!(current(&context), Theme::Light);
        apply(&context, Theme::Dark);
        assert_eq!(current(&context), Theme::Dark);
        assert_eq!(context.global_style().visuals.window_fill, DARK.window);
    }
}
