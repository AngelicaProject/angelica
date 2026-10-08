//! The Angelica menu and plugin windows, drawn with egui. Player actions go
//! back to the host as queued actions and events; nothing here calls a
//! plugin.
#![forbid(unsafe_code)]
use crate::{
    host::{Action, Event, Host, Plugin, Status},
    sandbox::bindings::angelica::plugin::{
        runtime::Level,
        ui::{Choice, Chosen, Edited, TextInput, Toggled, Tone, Widget, WidgetRef},
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
                for plugin in host.plugins() {
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
        for plugin in host.plugins() {
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
}

impl Drawer<'_> {
    /// Draws widgets from `position` up to the `end` that closes the
    /// enclosing row or section, or the end of the list.
    fn widgets(&mut self, ui: &mut egui::Ui, widgets: &[Widget], position: &mut usize) {
        while let Some(widget) = widgets.get(*position) {
            *position += 1;
            match widget {
                Widget::EndRow | Widget::EndSection => return,
                Widget::BeginRow => {
                    ui.horizontal_wrapped(|ui| self.widgets(ui, widgets, position));
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
                }
                other => self.widget(ui, other),
            }
        }
    }

    fn reference(&self, id: &str) -> WidgetRef {
        WidgetRef {
            window: self.window.into(),
            id: id.into(),
        }
    }

    fn widget(&mut self, ui: &mut egui::Ui, widget: &Widget) {
        match widget {
            Widget::Heading(text) => {
                ui.heading(text);
            }
            Widget::Text(text) => {
                let rich = egui::RichText::new(&text.text);
                match text.tone {
                    Tone::Normal => ui.label(rich),
                    Tone::Muted => ui.label(rich.weak()),
                    Tone::Good => ui.colored_label(egui::Color32::from_rgb(0x5c, 0xb8, 0x5c), rich),
                    Tone::Warning => ui.colored_label(ui.visuals().warn_fg_color, rich),
                    Tone::Bad => ui.colored_label(ui.visuals().error_fg_color, rich),
                };
            }
            Widget::Button(button) => {
                if ui
                    .add_enabled(button.enabled, egui::Button::new(&button.label))
                    .clicked()
                {
                    self.host
                        .queue_event(self.plugin, Event::Clicked(self.reference(&button.id)));
                }
            }
            Widget::Checkbox(checkbox) => {
                let mut checked = checkbox.checked;
                if ui.checkbox(&mut checked, &checkbox.label).changed() {
                    self.host.queue_event(
                        self.plugin,
                        Event::Toggled(Toggled {
                            window: self.window.into(),
                            id: checkbox.id.clone(),
                            checked,
                        }),
                    );
                }
            }
            Widget::TextInput(input) => self.text_input(ui, input),
            Widget::Choice(choice) => self.choice(ui, choice),
            Widget::Progress(progress) => {
                let bar = egui::ProgressBar::new(progress.fraction.clamp(0.0, 1.0));
                ui.add(if progress.label.is_empty() {
                    bar
                } else {
                    bar.text(&progress.label)
                });
            }
            Widget::Separator => {
                ui.separator();
            }
            Widget::BeginRow | Widget::EndRow | Widget::BeginSection(_) | Widget::EndSection => {}
        }
    }
    fn text_input(&mut self, ui: &mut egui::Ui, input: &TextInput) {
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
        });
    }

    fn choice(&mut self, ui: &mut egui::Ui, choice: &Choice) {
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
                });
        });
    }
}

/// The position after the `end` matching the `begin` before `position`.
fn skip(widgets: &[Widget], mut position: usize) -> usize {
    let mut depth = 1;
    while let Some(widget) = widgets.get(position) {
        position += 1;
        match widget {
            Widget::BeginRow | Widget::BeginSection(_) => depth += 1,
            Widget::EndRow | Widget::EndSection => {
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
