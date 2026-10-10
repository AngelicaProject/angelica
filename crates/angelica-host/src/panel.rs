//! The Angelica menu and plugin windows, drawn with egui. Player actions go
//! back to the host as queued actions and events; nothing here calls a
//! plugin. The menu itself is in [`crate::menu`].
#![forbid(unsafe_code)]
use crate::theme::{self, Word};
use crate::{
    host::{Event, Host, Plugin, Status},
    language::{Language, Strings},
    menu::{self, Look, Menu},
    sandbox::bindings::angelica::plugin::ui::{
        Choice, Chosen, Edited, Icon, Number, NumberSet, Progress, Table, Tabs, TextInput, Toggled,
        Tone, Widget, WidgetRef,
    },
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// The most icon textures the panel keeps.
const MAX_ICONS: usize = 512;
/// The time a frame may spend reading and decoding icons.
const ICON_BUDGET: Duration = Duration::from_millis(2);

/// State the panel keeps between frames: the menu, the text the player is
/// editing in plugin windows, and the icons it has decoded.
pub struct Panel {
    menu: Menu,
    inputs: HashMap<(String, String, String), String>,
    icons: Icons,
    pub map_status: Vec<String>,
}

/// Icon textures by icon number, with the frame each was last drawn in;
/// `None` for an icon the game does not have or the host cannot decode.
#[derive(Default)]
struct Icons {
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
        Self {
            menu: Menu::new(host, system),
            inputs: HashMap::new(),
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

    /// The key with the Windows virtual-key code `code` was pressed while
    /// the menu waited for its new key.
    pub fn key_pressed(&mut self, host: &Host, code: u32) {
        self.menu.key_pressed(host, code);
    }

    /// Draws the menu when `menu` is true, otherwise the plugins' windows
    /// and Angelica's sign. Returns whether the menu stays open.
    pub fn draw(&mut self, host: &Host, context: &egui::Context, menu: bool) -> bool {
        self.icons.begin_frame();
        let keep = self.menu.draw(host, context, &self.map_status, menu);
        if menu {
            return keep;
        }
        for plugin in &host.plugins() {
            if plugin.status() != Status::Running {
                continue;
            }
            let (look, strings) = self.menu.look(context);
            for window in plugin.windows() {
                self.draw_window(host, plugin, &window, context, look, strings);
            }
        }
        keep
    }

    fn draw_window(
        &mut self,
        host: &Host,
        plugin: &Plugin,
        window: &crate::host::Window,
        context: &egui::Context,
        look: Look,
        strings: &'static Strings,
    ) {
        let id = plugin.manifest.id();
        let name = &plugin.manifest.plugin.name;
        let title = if window.title == *name {
            name.clone()
        } else {
            format!("{} — {name}", window.title)
        };
        let mut open = true;
        egui::Window::new(&title)
            .id(egui::Id::new(("angelica-plugin", id, &window.id)))
            .title_bar(false)
            .collapsible(false)
            .frame(menu::window_frame(context, look.p))
            .default_width(look.at(480.0))
            .show(context, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                if menu::title_bar(ui, &look, Some(&title), strings.close) {
                    open = false;
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(
                        menu::num_u8(look.at(12.0)),
                        menu::num_u8(look.at(8.0)),
                    ))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing =
                                    egui::vec2(look.at(12.0), look.at(4.0));
                                let mut drawer = Drawer {
                                    host,
                                    plugin: id,
                                    window: &window.id,
                                    inputs: &mut self.inputs,
                                    icons: &mut self.icons,
                                    last: None,
                                    look,
                                    strings,
                                };
                                let mut position = 0;
                                drawer.widgets(ui, &window.widgets, &mut position);
                            });
                    });
            });
        if !open {
            host.queue_event(id, Event::Closed(window.id.clone()));
        }
    }
}

struct Drawer<'a> {
    host: &'a Host,
    plugin: &'a str,
    window: &'a str,
    inputs: &'a mut HashMap<(String, String, String), String>,
    icons: &'a mut Icons,
    /// The widget a following tooltip points at.
    last: Option<egui::Response>,
    look: Look,
    strings: &'static Strings,
}

impl Drawer<'_> {
    /// Draws widgets from `position` up to the `end` that closes the
    /// enclosing row, section, or table, or the end of the list.
    fn widgets(&mut self, ui: &mut egui::Ui, widgets: &[Widget], position: &mut usize) {
        while let Some(widget) = widgets.get(*position) {
            *position += 1;
            if matches!(
                widget,
                Widget::EndRow | Widget::EndSection | Widget::EndTable
            ) {
                return;
            }
            self.item(ui, widgets, widget, position);
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
        match widget {
            Widget::BeginRow => {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    self.widgets(ui, widgets, position);
                });
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
            Widget::Tooltip(text) => {
                if let Some(response) = self.last.take() {
                    response.on_hover_text(text);
                }
            }
            other => self.last = self.widget(ui, other),
        }
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
                let p = self.look.p;
                let color = match text.tone {
                    Tone::Normal => p.text,
                    Tone::Muted => p.secondary,
                    Tone::Good => p.on,
                    Tone::Warning => p.attention,
                    Tone::Bad => p.danger,
                };
                ui.label(self.rich(&text.text, color))
            }
            Widget::Button(button) => {
                let response =
                    theme::word(ui, &button.label, Word::Action, button.enabled, self.look.k);
                if response.clicked() {
                    self.host
                        .queue_event(self.plugin, Event::Clicked(self.reference(&button.id)));
                }
                response
            }
            Widget::Checkbox(checkbox) => {
                let words = [self.strings.word_on, self.strings.word_off];
                let chosen = usize::from(!checkbox.checked);
                let row = ui.horizontal(|ui| {
                    ui.label(self.rich(&checkbox.label, self.look.p.text));
                    theme::choice(ui, &words, chosen, self.look.k)
                });
                if let Some(picked) = row.inner {
                    self.host.queue_event(
                        self.plugin,
                        Event::Toggled(Toggled {
                            window: self.window.into(),
                            id: checkbox.id.clone(),
                            checked: picked == 0,
                        }),
                    );
                }
                row.response
            }
            Widget::TextInput(input) => self.text_input(ui, input),
            Widget::Choice(choice) => self.choice(ui, choice),
            Widget::Number(number) => self.number(ui, number),
            Widget::Tabs(tabs) => self.tabs(ui, tabs),
            Widget::Progress(progress) => self.progress(ui, progress),
            Widget::Icon(icon) => self.icon(ui, *icon),
            Widget::Separator => {
                ui.add_space(self.look.at(4.0));
                theme::rule(ui);
                ui.add_space(self.look.at(4.0));
                return None;
            }
            Widget::BeginRow
            | Widget::EndRow
            | Widget::BeginSection(_)
            | Widget::EndSection
            | Widget::BeginTable(_)
            | Widget::EndTable
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
            egui::vec2(ui.available_width(), look.at(26.0)),
            egui::Sense::hover(),
        );
        let painter = ui.painter();
        let title = painter.text(
            egui::pos2(rect.left(), rect.bottom() - look.at(5.0)),
            egui::Align2::LEFT_BOTTOM,
            text,
            theme::sans(look.at(12.0)),
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

    /// A thin line that fills with the action color, and its label.
    fn progress(&self, ui: &mut egui::Ui, progress: &Progress) -> egui::Response {
        let look = self.look;
        let fraction = progress.fraction.clamp(0.0, 1.0);
        ui.horizontal(|ui| {
            let width = if progress.label.is_empty() {
                ui.available_width()
            } else {
                (ui.available_width() * 0.5).max(look.at(120.0))
            };
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(width, look.at(20.0)), egui::Sense::hover());
            let line = egui::Rect::from_center_size(
                rect.center(),
                egui::vec2(rect.width(), look.at(2.0).max(2.0)),
            );
            ui.painter().rect_filled(line, 1.0, look.p.line);
            ui.painter().rect_filled(
                line.with_max_x(line.left() + line.width() * fraction),
                1.0,
                look.p.action,
            );
            if !progress.label.is_empty() {
                ui.label(self.rich(&progress.label, look.p.secondary));
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

    /// Tabs like the menu's: the chosen one is a light plate on the line
    /// under the row.
    fn tabs(&mut self, ui: &mut egui::Ui, tabs: &Tabs) -> egui::Response {
        let look = self.look;
        let p = look.p;
        let (row, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), look.at(32.0)),
            egui::Sense::hover(),
        );
        let painter = ui.painter().clone();
        painter.line_segment(
            [row.left_bottom(), row.right_bottom()],
            egui::Stroke::new(1.0, p.line),
        );
        let mut x = row.left();
        for (index, label) in tabs.labels.iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            let active = tabs.selected == index;
            let galley = painter.layout_no_wrap(label.clone(), theme::sans(look.at(13.0)), p.text);
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, row.bottom() - look.at(26.0)),
                egui::vec2(galley.size().x + look.at(28.0), look.at(26.0)),
            );
            let tab = ui
                .interact(
                    rect,
                    egui::Id::new(("angelica-tabs", self.plugin, self.window, &tabs.id, index)),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if active {
                let plate = rect.with_max_y(rect.bottom() + 1.0);
                let corner = egui::CornerRadius {
                    nw: 4,
                    ne: 4,
                    sw: 0,
                    se: 0,
                };
                painter.rect_filled(plate, corner, p.window);
                painter.rect_filled(plate, corner, p.chosen);
                painter.add(egui::Shape::line(
                    vec![
                        plate.left_bottom(),
                        plate.left_top(),
                        plate.right_top(),
                        plate.right_bottom(),
                    ],
                    egui::Stroke::new(1.0, p.border),
                ));
            }
            let color = if active || tab.hovered() {
                p.text
            } else {
                p.secondary
            };
            painter.galley_with_override_text_color(
                rect.center() - galley.size() / 2.0,
                galley,
                color,
            );
            if tab.clicked() && !active {
                self.host.queue_event(
                    self.plugin,
                    Event::Chosen(Chosen {
                        window: self.window.into(),
                        id: tabs.id.clone(),
                        index,
                    }),
                );
            }
            x = rect.right() + look.at(4.0);
        }
        response
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
        let selected = choice
            .selected
            .and_then(|index| choice.options.get(index as usize));
        let look = self.look;
        let mut picked = None;
        let response = ui
            .horizontal(|ui| {
                if !choice.label.is_empty() {
                    ui.label(self.rich(&choice.label, look.p.text));
                }
                let response = theme::value(ui, selected.map_or("—", String::as_str), look.k);
                egui::Popup::menu(&response).show(|ui| {
                    for (index, option) in choice.options.iter().enumerate() {
                        if menu::popup_item(ui, &look, None, option, look.p.text) {
                            picked = u32::try_from(index).ok();
                        }
                    }
                });
                response
            })
            .inner;
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

/// The position after the `end` matching the `begin` before `position`.
fn skip(widgets: &[Widget], mut position: usize) -> usize {
    let mut depth = 1;
    while let Some(widget) = widgets.get(position) {
        position += 1;
        match widget {
            Widget::BeginRow | Widget::BeginSection(_) | Widget::BeginTable(_) => depth += 1,
            Widget::EndRow | Widget::EndSection | Widget::EndTable => {
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
