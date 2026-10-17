//! The Angelica menu and plugin windows, drawn with egui. Player actions go
//! back to the host as queued actions and events; nothing here calls a
//! plugin. The menu itself is in [`crate::menu`].
#![forbid(unsafe_code)]
use crate::theme::{self, Word};
use crate::{
    host::{Event, Host},
    language::{Language, Strings},
    menu::{self, Look, Menu},
};
use angelica_view::{
    Action as EntryAction, ActionKind, Checkbox, Choice, Chosen, Edited, Entry, Fact, Icon,
    Mark as EntryMark, Number, NumberSet, Split, Table, Tabs, TextInput, Toggled, Widget,
    WidgetRef,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// The most icon textures the panel keeps.
const MAX_ICONS: usize = 512;
/// The time a frame may spend reading and decoding icons.
const ICON_BUDGET: Duration = Duration::from_millis(2);

/// What plugins draw over the game world, under every window.
fn draw_world(host: &Host, context: &egui::Context) {
    use crate::draw::Mark;
    let services = host.services();
    if services.draw.idle() {
        return;
    }
    let Some(matrix) = services.game.view_projection() else {
        return;
    };
    let screen = context.content_rect();
    let at = |(x, y): (f32, f32)| {
        egui::pos2(
            screen.left() + x * screen.width(),
            screen.top() + y * screen.height(),
        )
    };
    let color = |rgba: u32| {
        let [r, g, b, a] = rgba.to_be_bytes();
        egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    };
    let painter = context.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("angelica-world"),
    ));
    for mark in services.draw.marks(&matrix) {
        match mark {
            Mark::Line {
                from,
                to,
                color: rgba,
                thickness,
            } => {
                painter.line_segment(
                    [at(from), at(to)],
                    egui::Stroke::new(thickness, color(rgba)),
                );
            }
            Mark::Fill {
                points,
                color: rgba,
            } => {
                painter.add(egui::Shape::convex_polygon(
                    points.into_iter().map(at).collect(),
                    color(rgba),
                    egui::Stroke::NONE,
                ));
            }
            Mark::Text {
                at: place,
                text,
                color: rgba,
            } => {
                painter.text(
                    at(place),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(14.0),
                    color(rgba),
                );
            }
        }
    }
    context.request_repaint();
}

/// State the panel keeps between frames: the menu, the text the player is
/// editing in plugin windows, and the icons it has decoded.
pub struct Panel {
    menu: Menu,
    inputs: HashMap<(String, String, String), String>,
    pub(crate) places: crate::places::Places,
    icons: Icons,
    pub map_status: Vec<String>,
}

/// Icon textures by icon number, with the frame each was last drawn in;
/// `None` for an icon the game does not have or the host cannot decode.
#[derive(Default)]
pub(crate) struct Icons {
    textures: HashMap<u32, (Option<egui::TextureHandle>, u64)>,
    frame: u64,
    /// When this frame's time for decoding icons ends.
    until: Option<Instant>,
}

impl Icons {
    fn begin_frame(&mut self) {
        self.frame += 1;
        self.until = None;
    }

    /// The icon's texture, decoding it within the frame's budget; `None`
    /// draws an empty frame.
    fn texture(
        &mut self,
        host: &Host,
        context: &egui::Context,
        icon: u32,
    ) -> Option<egui::TextureId> {
        if let Some((texture, frame)) = self.textures.get_mut(&icon) {
            *frame = self.frame;
            return texture.as_ref().map(egui::TextureHandle::id);
        }
        let until = *self
            .until
            .get_or_insert_with(|| Instant::now() + ICON_BUDGET);
        if Instant::now() >= until {
            context.request_repaint();
            return None;
        }
        let texture = match crate::icons::load(&host.services().data, icon) {
            Ok(Some(image)) => Some(context.load_texture(
                format!("angelica-icon-{icon}"),
                egui::ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.rgba),
                egui::TextureOptions::LINEAR,
            )),
            Ok(None) => None,
            Err(error) => {
                host.diagnostic(format!("icon {icon}: {error}"));
                None
            }
        };
        if self.textures.len() >= MAX_ICONS
            && let Some(&oldest) = self
                .textures
                .iter()
                .min_by_key(|(_, (_, frame))| *frame)
                .map(|(icon, _)| icon)
        {
            self.textures.remove(&oldest);
        }
        let id = texture.as_ref().map(egui::TextureHandle::id);
        self.textures.insert(icon, (texture, self.frame));
        id
    }
}

impl Panel {
    /// The panel with the player's settings; `system` is the language of
    /// the system.
    #[must_use]
    pub fn new(host: &Host, map_status: Vec<String>, system: Language) -> Self {
        let places = crate::places::Places::new(host);
        Self {
            menu: Menu::placed(host, system, places.menu_place()),
            inputs: HashMap::new(),
            places,
            icons: Icons::default(),
            map_status,
        }
    }

    /// The Windows virtual-key code of the key that opens and closes the
    /// menu, and whether the menu waits for the player to press a new one.
    #[must_use]
    pub fn menu_key(&self) -> (u32, bool) {
        (self.menu.key().virtual_key(), self.menu.choosing_key())
    }

    /// The Windows virtual-key code of the key that stops plugins acting.
    #[must_use]
    pub fn stop_key(&self) -> u32 {
        self.menu.stop_key().virtual_key()
    }

    /// The key with the Windows virtual-key code `code` was pressed while
    /// the menu waited for its new key.
    pub fn key_pressed(&mut self, host: &Host, code: u32) {
        self.menu.key_pressed(host, code);
    }

    /// Draws the menu when `menu` is true, otherwise the plugins' windows
    /// and Angelica's sign. Returns whether the menu stays open.
    pub fn draw(&mut self, host: &Host, context: &egui::Context, menu: bool) -> bool {
        self.icons.begin_frame();
        draw_world(host, context);
        if let Some(text) = host.services().clipboard.take() {
            context.copy_text(text);
        }
        let mut keep = self.menu.draw(host, context, &self.map_status, menu);
        if self.menu.take_arrange() {
            self.places.layout = true;
            keep = false;
        }
        let raised = self.menu.take_raised();
        let (look, strings) = self.menu.look(context);
        // Plugin windows are drawn with the menu open too; one opened from
        // the menu comes in front of it.
        let mut frame = crate::places::Frame {
            host,
            context,
            look,
            strings,
            inputs: &mut self.inputs,
            icons: &mut self.icons,
            now: context.input(|input| input.time),
            screen: context.content_rect(),
        };
        if let Some(place) = self.menu.take_place(frame.screen) {
            self.places.keep_menu(&frame, place);
        }
        self.places
            .draw(&mut frame, raised.as_deref(), self.menu.notes_bottom());
        keep
    }
}

pub(crate) struct Drawer<'a> {
    pub(crate) host: &'a Host,
    pub(crate) plugin: &'a str,
    pub(crate) window: &'a str,
    pub(crate) inputs: &'a mut HashMap<(String, String, String), String>,
    pub(crate) icons: &'a mut Icons,
    /// The widget a following tooltip points at.
    pub(crate) last: Option<egui::Response>,
    pub(crate) look: Look,
    pub(crate) strings: &'static Strings,
    /// The position of the part being drawn, which names the parts
    /// without an id for their motion.
    pub(crate) index: usize,
    /// When tabs changed, so the parts after them fade in.
    pub(crate) faded: Option<f64>,
    /// Whether a split fills the rest of its window, each side scrolling
    /// in it; otherwise it is as tall as its sides.
    pub(crate) fill: bool,
    /// Where a split starts and the height its taller side needs,
    /// measured as it is drawn.
    pub(crate) needed: Option<(f32, f32)>,
}

/// How long the parts after tabs take to fade in after a change.
const TAB_FADE: f64 = 0.12;
/// How long a new row takes to appear.
const ROW_IN: f64 = 0.22;
/// How long a row stays tinted after it starts needing the player.
const ROW_TINT: f64 = 1.4;

/// What an entry remembers between frames: when it first appeared, its
/// last mark, and when it started needing the player.
#[derive(Clone, Copy)]
struct Seen {
    since: f64,
    mark: EntryMark,
    tinted: f64,
}

impl Drawer<'_> {
    fn id(&self, what: &str) -> egui::Id {
        egui::Id::new(("angelica-part", self.plugin, self.window, what, self.index))
    }

    /// Draws widgets from `position` up to the `end` that closes the
    /// enclosing container, or the end of the list. A spacer pushes what
    /// follows it to the end of its row, by the width it took last frame.
    pub(crate) fn widgets(&mut self, ui: &mut egui::Ui, widgets: &[Widget], position: &mut usize) {
        let mut spacer: Option<(egui::Id, f32)> = None;
        while let Some(widget) = widgets.get(*position) {
            *position += 1;
            if is_end(widget) {
                break;
            }
            if matches!(widget, Widget::Spacer) {
                self.index = *position - 1;
                let id = self.id("spacer");
                let rest = ui.data(|data| data.get_temp::<f32>(id)).unwrap_or(0.0);
                ui.add_space((ui.available_width() - rest - 1.0).max(0.0));
                spacer = Some((id, ui.cursor().left()));
                self.last = None;
                continue;
            }
            let now = ui.input(|input| input.time);
            match self.faded {
                Some(at) if now - at < TAB_FADE => {
                    #[allow(clippy::cast_possible_truncation)]
                    let opacity = ((now - at) / TAB_FADE) as f32;
                    ui.scope(|ui| {
                        ui.multiply_opacity(opacity);
                        self.item(ui, widgets, widget, position);
                    });
                    ui.ctx().request_repaint();
                }
                _ => self.item(ui, widgets, widget, position),
            }
        }
        if let Some((id, left)) = spacer {
            let rest = ui.min_rect().right() - left;
            ui.data_mut(|data| data.insert_temp(id, rest.max(0.0)));
        }
    }

    /// Draws one widget, or the group a `begin` opens.
    fn item(
        &mut self,
        ui: &mut egui::Ui,
        widgets: &[Widget],
        widget: &Widget,
        position: &mut usize,
    ) {
        self.index = *position - 1;
        match widget {
            Widget::BeginRow => {
                // A row with a spacer is one line, its end at the right;
                // other rows wrap what does not fit.
                let one_line = row_has_spacer(widgets, *position);
                let draw = |ui: &mut egui::Ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    self.widgets(ui, widgets, position);
                };
                if one_line {
                    ui.horizontal(draw);
                } else {
                    ui.horizontal_wrapped(draw);
                }
                self.last = None;
            }
            Widget::BeginColumn => {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = self.look.at(6.0);
                    self.widgets(ui, widgets, position);
                });
                self.last = None;
            }
            Widget::BeginGrid(columns) => {
                self.grid(ui, widgets, *columns, position);
                self.last = None;
            }
            Widget::BeginSection(section) => {
                let start = *position;
                let shown = egui::CollapsingHeader::new(&section.title)
                    .id_salt(("angelica-section", self.plugin, self.window, &section.id))
                    .default_open(section.open_by_default)
                    .show(ui, |ui| self.widgets(ui, widgets, position));
                if shown.body_returned.is_none() {
                    *position = skip(widgets, start);
                }
                self.last = None;
            }
            Widget::BeginTable(table) => {
                self.table(ui, widgets, table, position);
                self.last = None;
            }
            Widget::BeginSplit(split) => {
                self.split(ui, widgets, split, position);
                self.last = None;
            }
            Widget::Checkbox(_) | Widget::Choice(_) if !ui.layout().is_horizontal() => {
                let above = widgets[..*position - 1]
                    .iter()
                    .rev()
                    .find(|w| !matches!(w, Widget::Tooltip(_)));
                let line = matches!(above, Some(Widget::Checkbox(_) | Widget::Choice(_)));
                self.last = Some(self.setting(ui, widget, line));
            }
            Widget::Tooltip(text) => {
                if let Some(response) = self.last.take() {
                    response.on_hover_text(text);
                }
            }
            other => self.last = self.widget(ui, other),
        }
    }

    /// Fills a grid of equal columns up to its end, left to right, row by
    /// row; a tooltip belongs to the cell before it.
    fn grid(&mut self, ui: &mut egui::Ui, widgets: &[Widget], columns: u8, position: &mut usize) {
        let columns = usize::from(columns.max(1));
        let look = self.look;
        #[allow(clippy::cast_precision_loss)]
        let width =
            (ui.available_width() - look.at(12.0) * (columns as f32 - 1.0)) / columns as f32;
        egui::Grid::new(self.id("grid"))
            .num_columns(columns)
            .min_col_width(width.max(look.at(24.0)))
            .max_col_width(width.max(look.at(24.0)))
            .spacing(egui::vec2(look.at(12.0), look.at(8.0)))
            .show(ui, |ui| {
                let mut cells = 0;
                while let Some(widget) = widgets.get(*position) {
                    *position += 1;
                    if matches!(widget, Widget::EndGrid) {
                        break;
                    }
                    let cell = !matches!(widget, Widget::Tooltip(_));
                    self.item(ui, widgets, widget, position);
                    if cell {
                        cells += 1;
                        if cells % columns == 0 {
                            ui.end_row();
                        }
                    }
                }
            });
    }

    /// Draws a table's headings and fills its cells up to `end-table`; a
    /// tooltip belongs to the cell before it.
    fn table(
        &mut self,
        ui: &mut egui::Ui,
        widgets: &[Widget],
        table: &Table,
        position: &mut usize,
    ) {
        let columns = table.columns.len().max(1);
        egui::Grid::new(("angelica-table", self.plugin, self.window, &table.id))
            .striped(true)
            .num_columns(columns)
            .show(ui, |ui| {
                for column in &table.columns {
                    ui.strong(column);
                }
                ui.end_row();
                let mut cells = 0;
                while let Some(widget) = widgets.get(*position) {
                    *position += 1;
                    if matches!(widget, Widget::EndTable) {
                        break;
                    }
                    let cell = !matches!(widget, Widget::Tooltip(_));
                    self.item(ui, widgets, widget, position);
                    if cell {
                        cells += 1;
                        if cells % columns == 0 {
                            ui.end_row();
                        }
                    }
                }
            });
    }

    /// Draws a split: its sections in a column at the left, on the color of
    /// Angelica's own section column with a line at its right, and the
    /// chosen section's parts up to `end` beside it. Filling a window, each
    /// side scrolls on its own.
    fn split(
        &mut self,
        ui: &mut egui::Ui,
        widgets: &[Widget],
        split: &Split,
        position: &mut usize,
    ) {
        let look = self.look;
        let p = look.p;
        let fill = self.fill;
        let side = look.at(f32::from(split.width));
        let height = if fill {
            ui.available_height().max(look.at(48.0))
        } else {
            f32::INFINITY
        };
        // The column is painted under both sides once their height is known.
        let backdrop = ui.painter().add(egui::Shape::Noop);
        let top = ui.cursor().min;
        let mut tall: f32 = 0.0;
        let row = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let shrink = [false, !fill];
            let sections = ui.allocate_ui_with_layout(
                egui::vec2(side, height.min(1.0e6)),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(side);
                    egui::ScrollArea::vertical()
                        .id_salt(("angelica-split-side", self.plugin, self.window, &split.id))
                        .max_height(height)
                        .auto_shrink(shrink)
                        .show(ui, |ui| self.split_sections(ui, split))
                        .content_size
                        .y
                },
            );
            tall = tall.max(sections.inner);
            ui.add_space(look.at(16.0));
            let rest = ui.available_width().max(look.at(48.0));
            let main = ui.allocate_ui_with_layout(
                egui::vec2(rest, height.min(1.0e6)),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("angelica-split-main", self.plugin, self.window, &split.id))
                        .max_height(height)
                        .auto_shrink(shrink)
                        .show(ui, |ui| {
                            ui.set_width(rest);
                            ui.add_space(look.at(6.0));
                            ui.spacing_mut().item_spacing = egui::vec2(look.at(12.0), look.at(8.0));
                            self.widgets(ui, widgets, position);
                            ui.add_space(look.at(6.0));
                        })
                        .content_size
                        .y
                },
            );
            tall = tall.max(main.inner);
        });
        self.needed = Some((top.y, tall));
        // Filling a window, the column reaches the window's left and bottom
        // edges, as Angelica's own section column does.
        let (left, bottom) = if fill {
            (
                top.x - look.at(16.0),
                row.response.rect.bottom() + look.at(12.0),
            )
        } else {
            (top.x, top.y + tall)
        };
        let column =
            egui::Rect::from_min_max(egui::pos2(left, top.y), egui::pos2(top.x + side, bottom));
        let corner = egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: if fill { 6 } else { 0 },
            se: 0,
        };
        ui.painter().set(
            backdrop,
            egui::Shape::Vec(vec![
                egui::Shape::rect_filled(column, corner, p.column),
                egui::Shape::line_segment(
                    [column.right_top(), column.right_bottom()],
                    egui::Stroke::new(1.0, p.line),
                ),
            ]),
        );
    }

    /// The sections of a split, with their group headings and its action.
    fn split_sections(&mut self, ui: &mut egui::Ui, split: &Split) {
        let look = self.look;
        let p = look.p;
        ui.add_space(look.at(6.0));
        for (index, label) in split.labels.iter().enumerate() {
            if let Some(group) = split.groups.get(index).filter(|group| !group.is_empty()) {
                ui.add_space(look.at(6.0));
                let (line, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), look.at(20.0)),
                    egui::Sense::hover(),
                );
                ui.painter().text(
                    egui::pos2(line.left() + look.at(14.0), line.center().y),
                    egui::Align2::LEFT_CENTER,
                    group,
                    theme::sans(look.at(11.5)),
                    p.secondary,
                );
            }
            let at = u32::try_from(index).unwrap_or(u32::MAX);
            let (row, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), look.at(30.0)),
                egui::Sense::click(),
            );
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            let chosen = split.selected == at;
            let plate = row.shrink2(egui::vec2(look.at(6.0), look.at(1.0)));
            let painter = ui.painter();
            if chosen {
                painter.rect_filled(plate, egui::CornerRadius::same(4), p.chosen);
                painter.rect_stroke(
                    plate,
                    egui::CornerRadius::same(4),
                    egui::Stroke::new(1.0, p.line),
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                painter.rect_filled(plate, egui::CornerRadius::same(4), p.hover);
            }
            let value = split.values.get(index).map_or("", String::as_str);
            let value_width = if value.is_empty() {
                0.0
            } else {
                let galley = painter.layout_no_wrap(
                    value.to_owned(),
                    theme::sans(look.at(12.5)),
                    p.secondary,
                );
                let width = galley.size().x;
                painter.galley(
                    egui::pos2(
                        plate.right() - look.at(8.0) - width,
                        row.center().y - galley.size().y / 2.0,
                    ),
                    galley,
                    p.secondary,
                );
                width + look.at(8.0)
            };
            let font = if chosen {
                theme::medium(look.at(13.0))
            } else {
                theme::sans(look.at(13.0))
            };
            let room = (plate.width() - look.at(16.0) - value_width).max(0.0);
            let mut job = egui::text::LayoutJob::simple_singleline(label.clone(), font, p.text);
            job.wrap = egui::text::TextWrapping::truncate_at_width(room);
            let galley = painter.layout_job(job);
            painter.galley(
                egui::pos2(
                    plate.left() + look.at(8.0),
                    row.center().y - galley.size().y / 2.0,
                ),
                galley,
                p.text,
            );
            if response.clicked() && !chosen {
                self.host.queue_event(
                    self.plugin,
                    Event::Chosen(Chosen {
                        window: self.window.into(),
                        id: split.id.clone(),
                        index: at,
                    }),
                );
            }
            ui.add_space(look.at(2.0));
        }
        if !split.action.is_empty() {
            ui.add_space(look.at(4.0));
            ui.horizontal(|ui| {
                ui.add_space(look.at(10.0));
                if theme::word(ui, &split.action, Word::Action, true, look.k).clicked() {
                    self.host
                        .queue_event(self.plugin, Event::Clicked(self.reference(&split.id)));
                }
            });
        }
        ui.add_space(look.at(6.0));
    }

    fn reference(&self, id: &str) -> WidgetRef {
        WidgetRef {
            window: self.window.into(),
            id: id.into(),
        }
    }

    /// Draws a widget and returns what the player sees of it, for a tooltip.
    fn widget(&mut self, ui: &mut egui::Ui, widget: &Widget) -> Option<egui::Response> {
        Some(match widget {
            Widget::Heading(text) => self.heading(ui, text),
            Widget::Text(text) => {
                crate::parts::text(ui, self.look, &text.text, text.role, text.tone)
            }
            Widget::Button(button) => {
                let response = theme::word(
                    ui,
                    &button.label,
                    word_of(button.kind),
                    button.enabled,
                    self.look.k,
                );
                if response.clicked() {
                    self.host
                        .queue_event(self.plugin, Event::Clicked(self.reference(&button.id)));
                }
                response
            }
            Widget::Checkbox(checkbox) => {
                ui.horizontal(|ui| {
                    ui.label(self.rich(&checkbox.label, self.look.p.text));
                    self.checkbox_value(ui, checkbox);
                })
                .response
            }
            Widget::TextInput(input) => self.text_input(ui, input),
            Widget::Choice(choice) => self.choice(ui, choice),
            Widget::Number(number) => self.number(ui, number),
            Widget::Tabs(tabs) => self.tabs(ui, tabs),
            Widget::Meter(meter) => crate::parts::meter(ui, self.look, self.id("meter"), meter),
            Widget::Ring(ring) => {
                let (response, inner) = crate::parts::ring(ui, self.look, self.id("ring"), ring);
                if let Some(icon) = ring.icon
                    && let Some(texture) = self.icons.texture(self.host, ui.ctx(), icon)
                {
                    ui.painter().image(
                        texture,
                        inner,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                response
            }
            Widget::Readout(readout) => {
                let id = egui::Id::new(("angelica-readout", self.plugin, self.window, &readout.id));
                crate::parts::readout(ui, self.look, id, readout)
            }
            Widget::Countdown(countdown) => crate::parts::countdown(ui, self.look, countdown),
            Widget::Sparkline(sparkline) => crate::parts::sparkline(ui, self.look, sparkline),
            Widget::Tile(text) => crate::parts::tile(ui, self.look, text),
            Widget::Fact(fact) => self.fact(ui, fact),
            Widget::Entry(entry) => self.entry(ui, entry),
            Widget::Icon(icon) => self.icon(ui, *icon),
            Widget::Separator => {
                ui.add_space(self.look.at(6.0));
                theme::rule(ui);
                ui.add_space(self.look.at(6.0));
                return None;
            }
            Widget::BeginRow
            | Widget::EndRow
            | Widget::BeginColumn
            | Widget::EndColumn
            | Widget::BeginGrid(_)
            | Widget::EndGrid
            | Widget::BeginSection(_)
            | Widget::EndSection
            | Widget::BeginTable(_)
            | Widget::EndTable
            | Widget::BeginSplit(_)
            | Widget::EndSplit
            | Widget::Spacer
            | Widget::Tooltip(_) => return None,
        })
    }

    fn rich(&self, text: &str, color: egui::Color32) -> egui::RichText {
        egui::RichText::new(text)
            .font(theme::sans(self.look.at(13.5)))
            .color(color)
    }

    /// A small heading over a part of the window, with a thin rule to the
    /// right edge.
    fn heading(&self, ui: &mut egui::Ui, text: &str) -> egui::Response {
        let look = self.look;
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), look.at(34.0)),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        let title = painter.text(
            egui::pos2(rect.left(), rect.bottom() - look.at(7.0)),
            egui::Align2::LEFT_BOTTOM,
            text,
            theme::sans(look.at(12.5)),
            look.p.secondary,
        );
        let y = title.center().y + 1.0;
        painter.line_segment(
            [
                egui::pos2(title.right() + look.at(8.0), y),
                egui::pos2(rect.right(), y),
            ],
            egui::Stroke::new(1.0, look.p.line),
        );
        response
    }

    /// A name in the secondary color and its value.
    fn fact(&self, ui: &mut egui::Ui, fact: &Fact) -> egui::Response {
        let look = self.look;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = look.at(8.0);
            let (name, _) = ui.allocate_exact_size(
                egui::vec2(look.at(130.0), look.at(20.0)),
                egui::Sense::hover(),
            );
            let galley = ui.painter().layout(
                fact.name.clone(),
                theme::sans(look.at(13.0)),
                look.p.secondary,
                name.width(),
            );
            ui.painter().galley(
                egui::pos2(name.left(), name.center().y - look.at(8.5)),
                galley,
                look.p.secondary,
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&fact.value)
                        .font(theme::sans(look.at(13.0)))
                        .color(look.p.text),
                )
                .wrap(),
            )
        })
        .inner
    }

    fn act(&self, action: &EntryAction) {
        self.host
            .queue_event(self.plugin, Event::Clicked(self.reference(&action.id)));
    }

    /// What follows an entry's title: its actions at the right, its value
    /// before them, and its progress or note, which starts at `note` (its
    /// left edge and middle). Returns whether an action was pressed.
    fn entry_side(
        &mut self,
        ui: &mut egui::Ui,
        entry: &Entry,
        row: egui::Rect,
        left: f32,
        note: egui::Pos2,
        mut right: f32,
    ) -> bool {
        let look = self.look;
        let p = look.p;
        let painter = ui.painter().clone();
        let y = row.center().y;
        let mut acted = false;
        if !entry.actions.is_empty() {
            let room = egui::Rect::from_min_max(
                egui::pos2(left, row.top()),
                egui::pos2(right, row.bottom()),
            );
            let used = ui
                .scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(room)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = look.at(12.0);
                        for action in entry.actions.iter().rev() {
                            let word = theme::word(
                                ui,
                                &action.label,
                                word_of(action.kind),
                                action.enabled,
                                look.k,
                            );
                            if word.clicked() {
                                self.act(action);
                                acted = true;
                            }
                        }
                    },
                )
                .response
                .rect;
            right = used.left() - look.at(12.0);
        }
        if !entry.value.is_empty() && right > left {
            let value = painter.text(
                egui::pos2(right, y),
                egui::Align2::RIGHT_CENTER,
                &entry.value,
                theme::sans(look.at(12.5)),
                p.muted,
            );
            right = value.left() - look.at(10.0);
        }
        let (left, y) = (note.x, note.y);
        if let Some(fraction) = entry.progress {
            let line = egui::Rect::from_min_max(
                egui::pos2(left, y - 1.0),
                egui::pos2(right.max(left), y + 1.0),
            );
            painter.rect_filled(line, 1.0, p.line);
            painter.rect_filled(
                line.with_max_x(line.left() + line.width() * fraction.clamp(0.0, 1.0)),
                1.0,
                p.action,
            );
        } else if !entry.note.is_empty() && right - left > look.at(24.0) {
            let mut job = egui::text::LayoutJob::simple_singleline(
                entry.note.clone(),
                theme::sans(look.at(13.0)),
                p.secondary,
            );
            job.wrap = egui::text::TextWrapping::truncate_at_width(right - left);
            let galley = painter.layout_job(job);
            painter.galley(
                egui::pos2(left, y - galley.size().y / 2.0),
                galley,
                p.secondary,
            );
        }
        acted
    }

    /// The chosen entry's facts and actions under its row.
    fn entry_details(&mut self, ui: &mut egui::Ui, entry: &Entry) {
        let look = self.look;
        let p = look.p;
        egui::Frame::new()
            .fill(p.selection)
            .corner_radius(egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: 4,
                se: 4,
            })
            .inner_margin(egui::Margin {
                left: menu::num_u8(look.at(42.0)),
                right: menu::num_u8(look.at(10.0)),
                top: menu::num_u8(look.at(2.0)),
                bottom: menu::num_u8(look.at(12.0)),
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = egui::vec2(look.at(12.0), look.at(6.0));
                for fact in &entry.facts {
                    self.fact(ui, fact);
                }
                if !entry.more.is_empty() {
                    ui.add_space(look.at(6.0));
                    ui.horizontal_wrapped(|ui| {
                        for action in &entry.more {
                            let word = theme::word(
                                ui,
                                &action.label,
                                word_of(action.kind),
                                action.enabled,
                                look.k,
                            );
                            if word.clicked() {
                                self.act(action);
                            }
                        }
                    });
                }
            });
    }

    /// A row of a list and, when it is chosen, its facts and actions under
    /// it.
    fn entry(&mut self, ui: &mut egui::Ui, entry: &Entry) -> egui::Response {
        let look = self.look;
        let p = look.p;
        // A game icon in place of the letter tile; an empty frame until it
        // is read, or for an icon the game does not have.
        let icon = entry
            .icon
            .map(|icon| self.icons.texture(self.host, ui.ctx(), icon));
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let (row, response) = ui.allocate_exact_size(
                egui::vec2(
                    ui.available_width(),
                    look.at(if two_lines(entry) { 48.0 } else { 40.0 }),
                ),
                egui::Sense::click(),
            );
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            let hovered = ui.rect_contains_pointer(row);
            let details = entry.chosen && !(entry.facts.is_empty() && entry.more.is_empty());
            let painter = ui.painter().clone();
            let key = egui::Id::new(("angelica-entry", self.plugin, self.window, &entry.id));
            let now = ui.input(|input| input.time);
            let seen = ui.data_mut(|data| {
                let mut seen = data.get_temp::<Seen>(key).unwrap_or(Seen {
                    since: now,
                    mark: entry.mark,
                    tinted: f64::NEG_INFINITY,
                });
                if entry.mark == EntryMark::Attention && seen.mark != EntryMark::Attention {
                    seen.tinted = now;
                }
                seen.mark = entry.mark;
                data.insert_temp(key, seen);
                seen
            });
            if now - seen.since < ROW_IN {
                #[allow(clippy::cast_possible_truncation)]
                ui.multiply_opacity(((now - seen.since) / ROW_IN) as f32);
                ui.ctx().request_repaint();
            }
            let below = if details { 0 } else { 4 };
            let corners = egui::CornerRadius {
                nw: 4,
                ne: 4,
                sw: below,
                se: below,
            };
            if now - seen.tinted < ROW_TINT {
                #[allow(clippy::cast_possible_truncation)]
                let left = 1.0 - ((now - seen.tinted) / ROW_TINT) as f32;
                painter.rect_filled(row, corners, crate::theme::alpha(p.attention, 0.18 * left));
                ui.ctx().request_repaint();
            }
            let hover = ui
                .ctx()
                .animate_bool_with_time(key.with("hover"), hovered, 0.12);
            if entry.chosen {
                painter.rect_filled(row, corners, p.selection);
            } else if hover > 0.0 {
                painter.rect_filled(row, corners, crate::theme::alpha(p.hover, hover));
            }
            let y = row.center().y;
            let (left, note) = entry_head(&painter, look, row, entry, icon.as_ref());
            let mut right = row.right() - look.at(10.0);
            let center = egui::pos2(right - look.at(4.0), y);
            match entry.mark {
                EntryMark::None => {}
                EntryMark::On => theme::running(&painter, center, look.k, p),
                EntryMark::Off => theme::off(&painter, center, look.k, p),
                EntryMark::Attention => theme::attention(&painter, center, look.k * 0.8, p),
            }
            if entry.mark != EntryMark::None {
                right -= look.at(22.0);
            }
            let acted = self.entry_side(ui, entry, row, left, note, right);
            if response.clicked() && !acted {
                self.host
                    .queue_event(self.plugin, Event::Clicked(self.reference(&entry.id)));
            }
            if details {
                self.entry_details(ui, entry);
            }
            response
        })
        .inner
    }

    /// Draws an icon, or an empty frame while the game has none for it.
    fn icon(&mut self, ui: &mut egui::Ui, icon: Icon) -> egui::Response {
        let size = egui::Vec2::splat(f32::from(icon.size));
        if let Some(texture) = self.icons.texture(self.host, ui.ctx(), icon.icon) {
            return ui.add(egui::Image::from_texture((texture, size)));
        }
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().rect_stroke(
            rect,
            2.0,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
        response
    }

    fn number(&mut self, ui: &mut egui::Ui, number: &Number) -> egui::Response {
        let mut value = number.value;
        let decimals = usize::from(number.decimals);
        let response = if number.slider {
            let mut slider = egui::Slider::new(&mut value, number.min..=number.max)
                .fixed_decimals(decimals)
                .text(&number.label);
            if number.step > 0.0 {
                slider = slider.step_by(number.step);
            }
            ui.add(slider)
        } else {
            ui.horizontal(|ui| {
                if !number.label.is_empty() {
                    ui.label(&number.label);
                }
                let speed = if number.step > 0.0 {
                    number.step
                } else {
                    ((number.max - number.min) / 200.0).max(f64::EPSILON)
                };
                ui.add(
                    egui::DragValue::new(&mut value)
                        .range(number.min..=number.max)
                        .speed(speed)
                        .fixed_decimals(decimals),
                )
            })
            .inner
        };
        if number.step > 0.0 {
            value = number.min + ((value - number.min) / number.step).round() * number.step;
            value = value.clamp(number.min, number.max);
        }
        // Only a different value is an event, bit for bit.
        if response.changed() && value.to_bits() != number.value.to_bits() {
            self.host.queue_event(
                self.plugin,
                Event::Set(NumberSet {
                    window: self.window.into(),
                    id: number.id.clone(),
                    value,
                }),
            );
        }
        response
    }

    /// Tabs like the menu's (`theme::tab_strip`).
    fn tabs(&mut self, ui: &mut egui::Ui, tabs: &Tabs) -> egui::Response {
        let look = self.look;
        let p = look.p;
        let key = egui::Id::new(("angelica-tabs-seen", self.plugin, self.window, &tabs.id));
        let now = ui.input(|input| input.time);
        let changed = ui.data_mut(|data| {
            let (selected, at) = data
                .get_temp::<(u32, f64)>(key)
                .unwrap_or((tabs.selected, f64::NEG_INFINITY));
            let at = if selected == tabs.selected { at } else { now };
            data.insert_temp(key, (tabs.selected, at));
            at
        });
        self.faded = Some(changed);
        let labels: Vec<&str> = tabs.labels.iter().map(String::as_str).collect();
        let selected = usize::try_from(tabs.selected).unwrap_or(usize::MAX);
        let id = egui::Id::new(("angelica-tabs", self.plugin, self.window, &tabs.id));
        let strip = ui.horizontal(|ui| {
            theme::tab_strip(ui, p, &|value| look.at(value), id, &labels, selected)
        });
        if let Some(index) = strip.inner.and_then(|index| u32::try_from(index).ok()) {
            self.host.queue_event(
                self.plugin,
                Event::Chosen(Chosen {
                    window: self.window.into(),
                    id: tabs.id.clone(),
                    index,
                }),
            );
        }
        strip.response
    }

    fn text_input(&mut self, ui: &mut egui::Ui, input: &TextInput) -> egui::Response {
        let key = (
            self.plugin.to_owned(),
            self.window.to_owned(),
            input.id.clone(),
        );
        let editor_id = egui::Id::new(("angelica-input", &key));
        let focused = ui.memory(|memory| memory.has_focus(editor_id));
        let text = self
            .inputs
            .entry(key)
            .or_insert_with(|| input.value.clone());
        // The plugin's value wins unless the player is typing.
        if !focused && *text != input.value {
            text.clone_from(&input.value);
        }
        let look = self.look;
        let label = (!input.label.is_empty()).then(|| {
            egui::RichText::new(&input.label)
                .font(theme::sans(look.at(13.5)))
                .color(look.p.text)
        });
        ui.horizontal(|ui| {
            if let Some(label) = label {
                ui.label(label);
            }
            let (field, _) = ui.allocate_exact_size(
                egui::vec2(
                    ui.available_width().clamp(look.at(120.0), look.at(320.0)),
                    look.at(26.0),
                ),
                egui::Sense::hover(),
            );
            let response = menu::text_field(ui, &look, field, text, &input.hint, editor_id, None);
            if response.changed() {
                self.host.queue_event(
                    self.plugin,
                    Event::Edited(Edited {
                        window: self.window.into(),
                        id: input.id.clone(),
                        value: text.clone(),
                    }),
                );
            }
            response
        })
        .inner
    }

    fn choice(&mut self, ui: &mut egui::Ui, choice: &Choice) -> egui::Response {
        ui.horizontal(|ui| {
            if !choice.label.is_empty() {
                ui.label(self.rich(&choice.label, self.look.p.text));
            }
            self.choice_value(ui, choice)
        })
        .inner
    }

    /// A checkbox's two words.
    fn checkbox_value(&mut self, ui: &mut egui::Ui, checkbox: &Checkbox) {
        let words = match &checkbox.words {
            Some([on, off]) => [on.as_str(), off.as_str()],
            None => [self.strings.word_on, self.strings.word_off],
        };
        let chosen = usize::from(!checkbox.checked);
        if let Some(picked) = theme::choice(ui, &words, chosen, self.look.k) {
            self.host.queue_event(
                self.plugin,
                Event::Toggled(Toggled {
                    window: self.window.into(),
                    id: checkbox.id.clone(),
                    checked: picked == 0,
                }),
            );
        }
    }

    /// A checkbox or choice among parts under each other, drawn as a
    /// setting: its label on the left, its value at the right edge, its hint
    /// under them, and a thin line above when a setting is right above.
    fn setting(&mut self, ui: &mut egui::Ui, widget: &Widget, line: bool) -> egui::Response {
        let look = self.look;
        let p = look.p;
        let (label, hint) = match widget {
            Widget::Checkbox(c) => (c.label.as_str(), c.hint.as_str()),
            Widget::Choice(c) => (c.label.as_str(), c.hint.as_str()),
            _ => ("", ""),
        };
        // The value's width last frame puts it at the right edge.
        let id = self.id("setting");
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if line {
                let (rule, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().hline(
                    rule.x_range(),
                    rule.center().y,
                    egui::Stroke::new(1.0, p.line),
                );
            }
            ui.horizontal(|ui| {
                ui.set_min_height(look.at(34.0));
                ui.label(self.rich(label, p.text));
                let width = ui.data(|data| data.get_temp::<f32>(id)).unwrap_or(0.0);
                ui.add_space((ui.available_width() - width).max(look.at(12.0)));
                let start = ui.cursor().left();
                match widget {
                    Widget::Checkbox(checkbox) => self.checkbox_value(ui, checkbox),
                    Widget::Choice(choice) => {
                        self.choice_value(ui, choice);
                    }
                    _ => {}
                }
                let used = ui.min_rect().right() - start;
                ui.data_mut(|data| data.insert_temp(id, used));
            });
            if !hint.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(hint)
                            .font(theme::sans(look.at(12.5)))
                            .color(p.muted),
                    )
                    .wrap(),
                );
                ui.add_space(look.at(8.0));
            }
        })
        .response
    }

    /// A choice's current value, which opens the menu of its options.
    fn choice_value(&mut self, ui: &mut egui::Ui, choice: &Choice) -> egui::Response {
        let selected = choice
            .selected
            .and_then(|index| choice.options.get(index as usize));
        let look = self.look;
        let mut picked = None;
        let response = theme::value(ui, selected.map_or("—", String::as_str), look.k);
        egui::Popup::menu(&response).show(|ui| {
            for (index, option) in choice.options.iter().enumerate() {
                if menu::popup_item(ui, &look, None, option, look.p.text) {
                    picked = u32::try_from(index).ok();
                }
            }
        });
        if let Some(index) = picked
            && choice.selected != Some(index)
        {
            self.host.queue_event(
                self.plugin,
                Event::Chosen(Chosen {
                    window: self.window.into(),
                    id: choice.id.clone(),
                    index,
                }),
            );
        }
        response
    }
}

/// The look of an action of `kind`.
fn word_of(kind: ActionKind) -> Word {
    match kind {
        ActionKind::Normal => Word::Action,
        ActionKind::Main => Word::Main,
        ActionKind::Quiet => Word::Secondary,
        ActionKind::Danger => Word::Danger,
    }
}

/// Whether the entry is a plain one with its note or progress under the
/// title.
fn two_lines(entry: &Entry) -> bool {
    entry.plain && (!entry.note.is_empty() || entry.progress.is_some())
}

/// Draws an entry's tile and title. Returns where what follows the title
/// starts, and where the note starts (its left edge and middle): after the
/// title, or under it in a plain row, which has no tile and is one line
/// without a note.
fn entry_head(
    painter: &egui::Painter,
    look: Look,
    row: egui::Rect,
    entry: &Entry,
    icon: Option<&Option<egui::TextureId>>,
) -> (f32, egui::Pos2) {
    let p = look.p;
    let faded = entry.mark == EntryMark::Off;
    let y = row.center().y;
    let (start, title_y) = if entry.plain {
        let shift = if two_lines(entry) { look.at(8.0) } else { 0.0 };
        (row.left() + look.at(10.0), y - shift)
    } else {
        let tile = egui::Rect::from_center_size(
            egui::pos2(row.left() + look.at(20.0), y),
            egui::Vec2::splat(look.at(24.0)),
        );
        entry_tile(painter, tile, entry, icon, faded, p);
        (row.left() + look.at(42.0), y)
    };
    let name = painter.text(
        egui::pos2(start, title_y),
        egui::Align2::LEFT_CENTER,
        &entry.title,
        if entry.chosen {
            theme::medium(look.at(13.5))
        } else {
            theme::sans(look.at(13.5))
        },
        if faded { p.secondary } else { p.text },
    );
    let left = name.right() + look.at(10.0);
    let note = if two_lines(entry) {
        egui::pos2(start, y + look.at(9.0))
    } else {
        egui::pos2(left, y)
    };
    (left, note)
}

/// An entry's tile: the first letter of its title, or the game's icon it
/// names; an empty frame for an icon not read yet or that the game does not
/// have.
fn entry_tile(
    painter: &egui::Painter,
    tile: egui::Rect,
    entry: &Entry,
    icon: Option<&Option<egui::TextureId>>,
    faded: bool,
    p: &theme::Palette,
) {
    match icon {
        None => theme::tile(painter, tile, &entry.title, faded, false, p),
        Some(Some(texture)) => {
            let tint = if faded {
                egui::Color32::from_white_alpha(140)
            } else {
                egui::Color32::WHITE
            };
            painter.image(
                *texture,
                tile,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                tint,
            );
        }
        Some(None) => {
            painter.rect_stroke(
                tile,
                2.0,
                egui::Stroke::new(1.0, p.line),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// Whether the widget ends a container.
fn is_end(widget: &Widget) -> bool {
    matches!(
        widget,
        Widget::EndRow
            | Widget::EndColumn
            | Widget::EndGrid
            | Widget::EndSection
            | Widget::EndTable
            | Widget::EndSplit
    )
}

/// The position after the `end` matching the `begin` before `position`.
fn skip(widgets: &[Widget], mut position: usize) -> usize {
    let mut depth = 1;
    while let Some(widget) = widgets.get(position) {
        position += 1;
        match widget {
            Widget::BeginRow
            | Widget::BeginColumn
            | Widget::BeginGrid(_)
            | Widget::BeginSection(_)
            | Widget::BeginTable(_)
            | Widget::BeginSplit(_) => depth += 1,
            end if is_end(end) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
    }
    position
}

/// Whether the row whose parts start at `position` holds a spacer of its
/// own, outside the containers inside it.
fn row_has_spacer(widgets: &[Widget], position: usize) -> bool {
    let mut depth = 0usize;
    for widget in widgets.iter().skip(position) {
        match widget {
            Widget::BeginRow
            | Widget::BeginColumn
            | Widget::BeginGrid(_)
            | Widget::BeginSection(_)
            | Widget::BeginTable(_)
            | Widget::BeginSplit(_) => depth += 1,
            Widget::EndRow
            | Widget::EndColumn
            | Widget::EndGrid
            | Widget::EndSection
            | Widget::EndTable
            | Widget::EndSplit => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            Widget::Spacer if depth == 0 => return true,
            _ => {}
        }
    }
    false
}
