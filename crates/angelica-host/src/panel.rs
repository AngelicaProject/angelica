//! The Angelica menu and plugin windows, drawn with egui. Player actions go
//! back to the host as queued actions and events; nothing here calls a
//! plugin.
#![forbid(unsafe_code)]
use crate::{
    host::{Action, Event, Host, Plugin, Status},
    sandbox::bindings::angelica::plugin::{
        runtime::Level,
        ui::{
            Choice, Chosen, Edited, Number, NumberSet, Table, Tabs, TextInput, Toggled, Tone,
            Widget, WidgetRef,
        },
    },
};
use std::collections::HashMap;

/// State the panel keeps between frames: the text the player is editing.
#[derive(Default)]
pub struct Panel {
    inputs: HashMap<(String, String, String), String>,
    pub map_status: Vec<String>,
}

impl Panel {
    #[must_use]
    pub fn new(map_status: Vec<String>) -> Self {
        Self {
            inputs: HashMap::new(),
            map_status,
        }
    }

    pub fn draw(&mut self, host: &Host, context: &egui::Context) {
        egui::Window::new("Angelica")
            .default_width(420.0)
            .vscroll(true)
            .show(context, |ui| {
                for line in &self.map_status {
                    ui.label(egui::RichText::new(line).weak());
                }
                ui.separator();
                for plugin in &host.plugins() {
                    draw_plugin(host, plugin, ui);
                }
                let diagnostics = host.diagnostics();
                if !diagnostics.is_empty() {
                    egui::CollapsingHeader::new("Diagnostics").show(ui, |ui| {
                        for line in diagnostics.iter().rev() {
                            ui.label(line);
                        }
                    });
                }
            });
        for plugin in &host.plugins() {
            if plugin.status() != Status::Running {
                continue;
            }
            for window in plugin.windows() {
                self.draw_window(host, plugin, &window, context);
            }
        }
    }

    fn draw_window(
        &mut self,
        host: &Host,
        plugin: &Plugin,
        window: &crate::host::Window,
        context: &egui::Context,
    ) {
        let id = plugin.manifest.id();
        let mut open = true;
        egui::Window::new(format!(
            "{} — {}",
            window.title, plugin.manifest.plugin.name
        ))
        .id(egui::Id::new(("angelica-plugin", id, &window.id)))
        .open(&mut open)
        .vscroll(true)
        .default_width(480.0)
        .show(context, |ui| {
            let mut drawer = Drawer {
                host,
                plugin: id,
                window: &window.id,
                inputs: &mut self.inputs,
                last: None,
            };
            let mut position = 0;
            drawer.widgets(ui, &window.widgets, &mut position);
        });
        if !open {
            host.queue_event(id, Event::Closed(window.id.clone()));
        }
    }
}

fn draw_plugin(host: &Host, plugin: &Plugin, ui: &mut egui::Ui) {
    let manifest = &plugin.manifest;
    let id = manifest.id().to_owned();
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.strong(&manifest.plugin.name);
            ui.label(egui::RichText::new(format!("{} {}", id, manifest.plugin.version)).weak());
        });
        ui.label(&manifest.plugin.description);
        let status = plugin.status();
        match &status {
            Status::AwaitingApproval => {
                ui.label("Not approved yet. It may:");
                if manifest.permissions().is_empty() {
                    ui.label("  • show windows and keep files in its own folder");
                }
                for permission in manifest.permissions() {
                    ui.label(format!(
                        "  • {} ({})",
                        permission.description(),
                        permission.name()
                    ));
                    if *permission == crate::manifest::Permission::HttpGet {
                        for host in manifest.http_hosts() {
                            ui.label(format!("      {host}"));
                        }
                    }
                }
                if ui.button("Approve and enable").clicked() {
                    host.queue(Action::Approve(id.clone()));
                }
            }
            Status::Disabled => {
                if ui.button("Enable").clicked() {
                    host.queue(Action::Enable(id.clone()));
                }
            }
            Status::Running => {
                ui.horizontal(|ui| {
                    if ui.button("Open").clicked() {
                        host.queue_event(&id, Event::OpenRequested);
                    }
                    if ui.button("Disable").clicked() {
                        host.queue(Action::Disable(id.clone()));
                    }
                });
            }
            Status::Faulted(reason) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Stopped after a failure: {reason}"),
                );
                ui.horizontal(|ui| {
                    if ui.button("Retry").clicked() {
                        host.queue(Action::Enable(id.clone()));
                    }
                    if ui.button("Keep disabled").clicked() {
                        host.queue(Action::Disable(id.clone()));
                    }
                });
            }
        }
        if status != Status::AwaitingApproval && ui.button("Open plugin folder").clicked() {
            host.queue(Action::OpenFolder(id.clone()));
        }
        let lines = plugin.log.lines();
        if !lines.is_empty() {
            egui::CollapsingHeader::new("Log")
                .id_salt(("angelica-log", &id))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in lines {
                                let text = egui::RichText::new(line.text);
                                match line.level {
                                    Level::Error => {
                                        ui.colored_label(ui.visuals().error_fg_color, text)
                                    }
                                    Level::Warning => {
                                        ui.colored_label(ui.visuals().warn_fg_color, text)
                                    }
                                    Level::Debug => ui.label(text.weak()),
                                    Level::Info => ui.label(text),
                                };
                            }
                        });
                });
        }
    });
}

struct Drawer<'a> {
    host: &'a Host,
    plugin: &'a str,
    window: &'a str,
    inputs: &'a mut HashMap<(String, String, String), String>,
    /// The widget a following tooltip points at.
    last: Option<egui::Response>,
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
                ui.horizontal_wrapped(|ui| self.widgets(ui, widgets, position));
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
            Widget::Heading(text) => ui.heading(text),
            Widget::Text(text) => {
                let rich = egui::RichText::new(&text.text);
                match text.tone {
                    Tone::Normal => ui.label(rich),
                    Tone::Muted => ui.label(rich.weak()),
                    Tone::Good => ui.colored_label(egui::Color32::from_rgb(0x5c, 0xb8, 0x5c), rich),
                    Tone::Warning => ui.colored_label(ui.visuals().warn_fg_color, rich),
                    Tone::Bad => ui.colored_label(ui.visuals().error_fg_color, rich),
                }
            }
            Widget::Button(button) => {
                let response = ui.add_enabled(button.enabled, egui::Button::new(&button.label));
                if response.clicked() {
                    self.host
                        .queue_event(self.plugin, Event::Clicked(self.reference(&button.id)));
                }
                response
            }
            Widget::Checkbox(checkbox) => {
                let mut checked = checkbox.checked;
                let response = ui.checkbox(&mut checked, &checkbox.label);
                if response.changed() {
                    self.host.queue_event(
                        self.plugin,
                        Event::Toggled(Toggled {
                            window: self.window.into(),
                            id: checkbox.id.clone(),
                            checked,
                        }),
                    );
                }
                response
            }
            Widget::TextInput(input) => self.text_input(ui, input),
            Widget::Choice(choice) => self.choice(ui, choice),
            Widget::Number(number) => self.number(ui, number),
            Widget::Tabs(tabs) => self.tabs(ui, tabs),
            Widget::Progress(progress) => {
                let bar = egui::ProgressBar::new(progress.fraction.clamp(0.0, 1.0));
                ui.add(if progress.label.is_empty() {
                    bar
                } else {
                    bar.text(&progress.label)
                })
            }
            Widget::Separator => {
                ui.separator();
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

    fn tabs(&mut self, ui: &mut egui::Ui, tabs: &Tabs) -> egui::Response {
        let response = ui
            .horizontal(|ui| {
                for (index, label) in tabs.labels.iter().enumerate() {
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    if ui.selectable_label(tabs.selected == index, label).clicked()
                        && tabs.selected != index
                    {
                        self.host.queue_event(
                            self.plugin,
                            Event::Chosen(Chosen {
                                window: self.window.into(),
                                id: tabs.id.clone(),
                                index,
                            }),
                        );
                    }
                }
            })
            .response;
        ui.separator();
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
        ui.horizontal(|ui| {
            if !input.label.is_empty() {
                ui.label(&input.label);
            }
            let response = ui.add(
                egui::TextEdit::singleline(text)
                    .id(editor_id)
                    .hint_text(&input.hint),
            );
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
        ui.horizontal(|ui| {
            if !choice.label.is_empty() {
                ui.label(&choice.label);
            }
            egui::ComboBox::from_id_salt(("angelica-choice", self.plugin, self.window, &choice.id))
                .selected_text(selected.map_or("", String::as_str))
                .show_ui(ui, |ui| {
                    for (index, option) in choice.options.iter().enumerate() {
                        let index = u32::try_from(index).unwrap_or(u32::MAX);
                        if ui
                            .selectable_label(choice.selected == Some(index), option)
                            .clicked()
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
                    }
                })
                .response
        })
        .inner
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
