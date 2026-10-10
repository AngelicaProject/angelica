//! The Angelica menu over the game: a small window with the player's plugins
//! in a dense list, a dialog for what a plugin asks, settings, and the
//! details for developers. While the menu is closed, short notes at the top
//! right tell what happened (`docs/architecture/ui.md`).
#![forbid(unsafe_code)]
use crate::{
    host::{Action, Event, Host, Plugin, Status},
    key::MenuKey,
    language::{Language, Settings, Strings, fill},
    manifest::{Class, Origin, Permission},
    plugin_installer::{Failure, PluginInstaller, Report},
    sandbox::bindings::angelica::plugin::runtime::Level,
    theme::{self, Icon, Palette, Size, Theme, Word, medium, mono, sans, strong, wordmark},
};
use egui::{
    Align2, Color32, CornerRadius, Id, Pos2, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2,
    vec2,
};
use std::{path::PathBuf, sync::Arc};

/// Where a plugin is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Needs the player: to approve it, or to retry it after a failure.
    Attention,
    On,
    Off,
}

impl Group {
    #[must_use]
    pub fn of(status: &Status) -> Self {
        match status {
            Status::AwaitingApproval | Status::Faulted(_) => Self::Attention,
            Status::Running => Self::On,
            Status::Disabled => Self::Off,
        }
    }

    fn title(self, s: &Strings) -> &'static str {
        match self {
            Self::Attention => s.group_attention,
            Self::On => s.group_on,
            Self::Off => s.group_off,
        }
    }
}

/// What the list knows of a plugin.
pub struct Entry<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub group: Group,
    /// The classes of the permissions it holds or asks for.
    pub classes: Vec<Class>,
}

/// A line of the list: a group's heading with how many it holds, or a
/// plugin by its index in the entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Heading(Group, usize),
    Plugin(usize),
}

/// The list's lines: the plugins whose name or description contains
/// `query` and, with a `class`, that hold or ask for a permission of it,
/// in groups, each by name.
#[must_use]
pub fn lines(entries: &[Entry<'_>], query: &str, class: Option<Class>) -> Vec<Line> {
    let query = query.trim().to_lowercase();
    let mut lines = Vec::new();
    for group in [Group::Attention, Group::On, Group::Off] {
        let mut members: Vec<(String, usize)> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.group == group
                    && class.is_none_or(|class| entry.classes.contains(&class))
                    && (query.is_empty()
                        || entry.name.to_lowercase().contains(&query)
                        || entry.description.to_lowercase().contains(&query))
            })
            .map(|(index, entry)| (entry.name.to_lowercase(), index))
            .collect();
        members.sort();
        if !members.is_empty() {
            lines.push(Line::Heading(group, members.len()));
            lines.extend(members.into_iter().map(|(_, index)| Line::Plugin(index)));
        }
    }
    lines
}

/// The classes of permissions, in the order they are shown.
const CLASSES: [Class; 4] = [
    Class::Network,
    Class::OtherPlayers,
    Class::WhatYouSee,
    Class::YoursOnly,
];

/// The icon of a class of permissions.
#[must_use]
pub fn class_icon(class: Class) -> Icon {
    match class {
        Class::Network => Icon::Network,
        Class::OtherPlayers => Icon::OtherPlayers,
        Class::WhatYouSee => Icon::WhatYouSee,
        Class::YoursOnly => Icon::YoursOnly,
    }
}

/// The classes of `permissions`, each once, in the order they are shown.
fn classes_of(permissions: impl IntoIterator<Item = Permission>) -> Vec<Class> {
    let held: Vec<Class> = permissions.into_iter().map(Permission::class).collect();
    CLASSES
        .into_iter()
        .filter(|class| held.contains(class))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Installed,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    General,
    Updates,
    Developers,
}

/// How long a note stays.
const NOTE_TIME: f64 = 7.0;
/// How long the note of how Angelica is stays after the game starts.
const START_TIME: f64 = 12.0;
/// The width of the menu's window, in points at the normal size.
const WIDTH: f32 = 640.0;
/// The height of a line of the list.
const ROW: f32 = 36.0;
/// How many lines the list shows before it scrolls; the list keeps this
/// height with fewer plugins, so the window does not change its size.
const VISIBLE_ROWS: f32 = 9.0;
/// The height of the columns' header over the list.
const COLUMNS: f32 = 18.0;
/// The height of Installed around the list: the search, the link field,
/// and the footer. Settings take the same height as Installed.
const AROUND_LIST: f32 = 44.0 + 44.0 + 28.0;

/// What the player can do with a plugin from its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowAction {
    Open,
    TurnOn,
    TurnOff,
    Review,
    Folder,
    Remove,
}

/// What a note means, which picks its mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Good,
    Attention,
    Failed,
}

struct Note {
    report: Report,
    at: f64,
}

/// The menu's state between frames.
pub struct Menu {
    language: Language,
    theme: Theme,
    /// The theme last applied to the context.
    applied: Option<Theme>,
    settings: Settings,
    settings_path: PathBuf,
    /// Whether the next key the player presses becomes the menu's key.
    choosing_key: bool,
    tab: Tab,
    section: Section,
    query: String,
    filter: Option<Class>,
    link: String,
    link_invalid: bool,
    selected: Option<String>,
    /// The plugin whose permissions the dialog shows.
    dialog: Option<String>,
    /// The plugin the removal dialog asks about.
    removing: Option<String>,
    notes: Vec<Note>,
    seen: u64,
}

/// What one frame of the menu works with.
#[derive(Clone, Copy)]
struct View<'a> {
    host: &'a Host,
    plugins: &'a [Arc<Plugin>],
    statuses: &'a [Status],
    map_status: &'a [String],
    screen: Rect,
    /// How many times its normal size the menu is drawn.
    k: f32,
    time: f64,
    p: &'static Palette,
}

impl View<'_> {
    fn at(&self, value: f32) -> f32 {
        value * self.k
    }

    fn look(&self) -> Look {
        Look {
            k: self.k,
            p: self.p,
        }
    }
}

/// The size and colors Angelica's windows are drawn with, also the windows
/// of plugins.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    /// How many times their normal size the windows are drawn.
    pub k: f32,
    pub p: &'static Palette,
}

impl Look {
    pub(crate) fn at(&self, value: f32) -> f32 {
        value * self.k
    }
}

impl Menu {
    /// The menu with the player's settings from the host's state folder;
    /// `system` is the language of the system.
    #[must_use]
    pub fn new(host: &Host, system: Language) -> Self {
        let settings_path = Settings::path(host.state_directory());
        let settings = Settings::load(&settings_path);
        Self {
            language: settings.language.unwrap_or(system),
            theme: settings.theme.unwrap_or_default(),
            applied: None,
            settings,
            settings_path,
            choosing_key: false,
            tab: Tab::Installed,
            section: Section::General,
            query: String::new(),
            filter: None,
            link: String::new(),
            link_invalid: false,
            selected: None,
            dialog: None,
            removing: None,
            notes: Vec::new(),
            seen: 0,
        }
    }

    /// The look of this frame's windows and the interface strings.
    pub(crate) fn look(&self, context: &egui::Context) -> (Look, &'static Strings) {
        let look = Look {
            k: self
                .settings
                .size
                .unwrap_or_default()
                .scale(context.content_rect().height()),
            p: self.theme.palette(),
        };
        (look, self.strings())
    }

    fn strings(&self) -> &'static Strings {
        self.language.strings()
    }

    fn save(&self, host: &Host) {
        if let Err(error) = self.settings.save(&self.settings_path) {
            host.diagnostic(format!("settings: {error}"));
        }
    }

    fn set_language(&mut self, host: &Host, language: Language) {
        self.language = language;
        self.settings.language = Some(language);
        self.save(host);
    }

    fn set_theme(&mut self, host: &Host, theme: Theme) {
        self.theme = theme;
        self.settings.theme = Some(theme);
        self.save(host);
    }

    /// The key that opens and closes the menu.
    #[must_use]
    pub fn key(&self) -> MenuKey {
        self.settings.key.unwrap_or_default()
    }

    /// Whether the menu waits for the player to press its new key; the
    /// next key pressed then comes to [`Self::key_pressed`] and nowhere else.
    #[must_use]
    pub fn choosing_key(&self) -> bool {
        self.choosing_key
    }

    /// The key with the Windows virtual-key code `code` was pressed while
    /// the menu waited for its new key: `Esc` keeps the key, a key the menu
    /// can be on becomes it, and any other key is waited out.
    pub fn key_pressed(&mut self, host: &Host, code: u32) {
        const ESCAPE: u32 = 0x1b;
        if !self.choosing_key {
            return;
        }
        if code == ESCAPE {
            self.choosing_key = false;
        } else if let Some(key) = MenuKey::from_virtual_key(code) {
            self.choosing_key = false;
            if key != self.key() {
                self.settings.key = Some(key);
                self.save(host);
            }
        }
    }

    /// Draws the menu when `open`, otherwise the notes. Returns whether
    /// the menu stays open.
    pub fn draw(
        &mut self,
        host: &Host,
        context: &egui::Context,
        map_status: &[String],
        open: bool,
    ) -> bool {
        if self.applied != Some(self.theme) {
            theme::apply(context, self.theme);
            self.applied = Some(self.theme);
        }
        let plugins = host.plugins();
        let statuses: Vec<Status> = plugins.iter().map(|plugin| plugin.status()).collect();
        let screen = context.content_rect();
        let view = View {
            host,
            plugins: &plugins,
            statuses: &statuses,
            map_status,
            screen,
            k: self
                .settings
                .size
                .unwrap_or_default()
                .scale(screen.height()),
            time: context.input(|input| input.time),
            p: self.theme.palette(),
        };
        if let Some(installer) = host.plugin_installer() {
            self.collect_notes(installer, view.time);
        }
        if open {
            self.draw_menu(context, &view)
        } else {
            self.dialog = None;
            self.removing = None;
            self.choosing_key = false;
            self.draw_notes(context, &view);
            false
        }
    }

    fn draw_menu(&mut self, context: &egui::Context, view: &View<'_>) -> bool {
        let typing = context.memory(|memory| memory.focused().is_some());
        let mut keep = typing || !context.input(|input| input.key_pressed(egui::Key::Escape));
        let width = view.at(WIDTH);
        egui::Window::new("angelica")
            .id(Id::new("angelica-menu"))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .frame(window_frame(context, view.p))
            .default_pos(pos2(
                view.screen.center().x - width / 2.0,
                view.screen.top() + view.screen.height() * 0.12,
            ))
            .show(context, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if self.menu_bar(ui, view) {
                    keep = false;
                }
                self.draw_tabs(ui, view);
                match self.tab {
                    Tab::Installed => {
                        if self.draw_installed(ui, view) {
                            keep = false;
                        }
                    }
                    Tab::Settings => self.draw_settings(ui, view),
                }
            });
        if self.tab != Tab::Settings || self.section != Section::General {
            self.choosing_key = false;
        }
        if self.dialog.is_some() {
            self.draw_dialog(context, view);
        }
        if self.removing.is_some() {
            self.draw_removal(context, view);
        }
        keep
    }

    /// The menu's title bar: the word `angelica`, how many plugins wait and
    /// run, and a menu of what applies to all of them. Returns whether the
    /// player closed the menu.
    fn menu_bar(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let s = self.strings();
        let waiting = view
            .statuses
            .iter()
            .any(|status| Group::of(status) == Group::Attention);
        let pulse = self.pulse(view.statuses);
        let mark = if waiting {
            Mark::Attention
        } else if view.statuses.contains(&Status::Running) {
            Mark::Running
        } else {
            Mark::None
        };
        let pressed = bar(
            ui,
            &view.look(),
            &Bar {
                wordmark: true,
                title: "",
                path: &[],
                status: Some((&pulse, mark)),
                collapsed: None,
                menu: true,
                strings: s,
            },
        );
        if let Some(button) = &pressed.menu {
            let installer = view.host.plugin_installer();
            egui::Popup::menu(button).show(|ui| {
                if let Some(installer) = installer
                    && !installer.checking()
                    && menu_item(ui, view, None, s.menu_check_updates, view.p.text)
                {
                    installer.check_updates();
                }
                if menu_item(ui, view, None, s.tab_settings, view.p.text) {
                    self.tab = Tab::Settings;
                    self.section = Section::General;
                }
                if menu_item(ui, view, None, s.section_developers, view.p.text) {
                    self.tab = Tab::Settings;
                    self.section = Section::Developers;
                }
            });
        }
        pressed.close
    }

    fn draw_tabs(&mut self, ui: &mut Ui, view: &View<'_>) {
        let s = self.strings();
        let p = view.p;
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), view.at(34.0)), Sense::hover());
        let painter = ui.painter().clone();
        painter.line_segment(
            [row.left_bottom(), row.right_bottom()],
            Stroke::new(1.0, p.line),
        );
        let mut x = row.left() + view.at(10.0);
        for (tab, title) in [
            (Tab::Installed, s.tab_installed),
            (Tab::Settings, s.tab_settings),
        ] {
            let active = self.tab == tab;
            let galley = painter.layout_no_wrap(title.to_owned(), sans(view.at(13.0)), p.text);
            let rect = Rect::from_min_size(
                pos2(x, row.bottom() - view.at(26.0)),
                vec2(galley.size().x + view.at(28.0), view.at(26.0)),
            );
            let response = ui
                .interact(rect, Id::new(("angelica-tab", title)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let color = if active || response.hovered() {
                p.text
            } else {
                p.secondary
            };
            if active {
                let tab = rect.with_max_y(rect.bottom() + 1.0);
                let corner = CornerRadius {
                    nw: 4,
                    ne: 4,
                    sw: 0,
                    se: 0,
                };
                painter.rect_filled(tab, corner, p.window);
                painter.rect_filled(tab, corner, p.chosen);
                painter.add(egui::Shape::line(
                    vec![
                        tab.left_bottom(),
                        tab.left_top(),
                        tab.right_top(),
                        tab.right_bottom(),
                    ],
                    Stroke::new(1.0, p.border),
                ));
            }
            painter.galley_with_override_text_color(
                rect.center() - galley.size() / 2.0,
                galley,
                color,
            );
            if response.clicked() {
                self.tab = tab;
            }
            x = rect.right() + view.at(4.0);
        }
    }

    /// Installed: the search, the list, and the link field. Returns true
    /// when the player opened a plugin, which closes the menu.
    fn draw_installed(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        self.toolbar(ui, view);
        let entries: Vec<Entry<'_>> = view
            .plugins
            .iter()
            .zip(view.statuses)
            .map(|(plugin, status)| Entry {
                name: &plugin.manifest.plugin.name,
                description: &plugin.manifest.plugin.description,
                group: Group::of(status),
                classes: plugin_classes(plugin, status),
            })
            .collect();
        let lines = lines(&entries, &self.query, self.filter);
        let opened = self.draw_list(ui, view, &lines);
        if let Some(installer) = view.host.plugin_installer() {
            self.draw_link(ui, view, installer);
        }
        self.footer(ui, view);
        opened
    }

    /// The search field and the filter by class.
    fn toolbar(&mut self, ui: &mut Ui, view: &View<'_>) {
        let s = self.strings();
        let p = view.p;
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), view.at(44.0)), Sense::hover());
        let filter_text = self
            .filter
            .map_or(s.filter_all, |class| self.language.class(class));
        let filter_width = ui
            .painter()
            .layout_no_wrap(filter_text.to_owned(), medium(view.at(13.5)), p.action)
            .size()
            .x
            + view.at(26.0);
        let field = Rect::from_min_max(
            pos2(row.left() + view.at(10.0), row.top() + view.at(9.0)),
            pos2(
                row.right() - view.at(22.0) - filter_width,
                row.bottom() - view.at(9.0),
            ),
        );
        search_field(ui, view, field, &mut self.query, s.find, "angelica-find");
        let filter = Rect::from_min_max(
            pos2(field.right() + view.at(12.0), row.top()),
            pos2(row.right() - view.at(10.0), row.bottom()),
        );
        let mut choose = None;
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(filter)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                let response = theme::value(ui, filter_text, view.k);
                egui::Popup::menu(&response).show(|ui| {
                    if menu_item(ui, view, None, s.filter_all, p.text) {
                        choose = Some(None);
                    }
                    for class in CLASSES {
                        if menu_item(
                            ui,
                            view,
                            Some(class_icon(class)),
                            self.language.class(class),
                            p.text,
                        ) {
                            choose = Some(Some(class));
                        }
                    }
                });
            },
        );
        if let Some(filter) = choose {
            self.filter = filter;
        }
    }

    /// The list. Returns true when the player opened a plugin.
    fn draw_list(&mut self, ui: &mut Ui, view: &View<'_>, lines: &[Line]) -> bool {
        let s = self.strings();
        let p = view.p;
        if self
            .selected
            .as_deref()
            .is_some_and(|id| !view.plugins.iter().any(|plugin| plugin.manifest.id() == id))
        {
            self.selected = None;
        }
        let list_height = view.at(COLUMNS + ROW * VISIBLE_ROWS);
        if lines.is_empty() {
            let top = ui.cursor().top();
            let text = if self.query.trim().is_empty() && self.filter.is_none() {
                s.empty
            } else {
                s.nothing_found
            };
            ui.add_space(view.at(18.0));
            ui.horizontal(|ui| {
                ui.add_space(view.at(16.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(text)
                            .font(sans(view.at(13.5)))
                            .color(p.secondary),
                    )
                    .wrap(),
                );
            });
            let used = ui.cursor().top() - top;
            ui.add_space((list_height - used).max(0.0));
            return false;
        }
        columns_header(ui, view, s);
        let height = view.at(ROW);
        let mut opened = false;
        egui::ScrollArea::vertical()
            .id_salt("angelica-installed")
            .auto_shrink([false, false])
            .max_height(height * VISIBLE_ROWS)
            .show(ui, |ui| {
                for line in lines {
                    match *line {
                        Line::Heading(group, count) => {
                            let color = match group {
                                Group::Attention => p.attention,
                                Group::On => p.secondary,
                                Group::Off => p.muted,
                            };
                            ui.add_space(height - view.at(26.0));
                            theme::header(ui, group.title(s), Some(count), color, view.k);
                        }
                        Line::Plugin(index) => {
                            if self.plugin_row(ui, view, index, height) {
                                opened = true;
                            }
                        }
                    }
                }
            });
        opened
    }

    /// A plugin's row: its tile, name, permissions, version, and state,
    /// with its actions under the pointer, a menu on the right click, and
    /// its details when it is chosen. Returns true when the
    /// player opened the plugin.
    fn plugin_row(&mut self, ui: &mut Ui, view: &View<'_>, index: usize, height: f32) -> bool {
        let p = view.p;
        let plugin = &view.plugins[index];
        let status = &view.statuses[index];
        let id = plugin.manifest.id();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter().clone();
        let chosen = self.selected.as_deref() == Some(id);
        let pointed = ui.rect_contains_pointer(rect);
        if chosen {
            painter.rect_filled(rect, CornerRadius::ZERO, p.selection);
        } else if pointed {
            painter.rect_filled(rect, CornerRadius::ZERO, p.hover);
        }
        let off = *status == Status::Disabled;
        let columns = Columns::of(rect, view);
        let tile = Rect::from_center_size(
            pos2(columns.tile, rect.center().y),
            vec2(view.at(24.0), view.at(24.0)),
        );
        theme::tile(&painter, tile, &plugin.manifest.plugin.name, off, p);
        let name = elided(
            &painter,
            &plugin.manifest.plugin.name,
            sans(view.at(13.5)),
            if off { p.muted } else { p.text },
            columns.permissions - view.at(12.0) - columns.name,
        );
        painter.galley(
            pos2(columns.name, rect.center().y - name.size().y / 2.0),
            name,
            p.text,
        );
        let (mut acted, mut opened) = (false, false);
        if pointed && !chosen {
            (acted, opened) = self.action_words(ui, &painter, view, plugin, status, rect, &columns);
        } else {
            row_facts(&painter, view, plugin, status, rect, &columns);
        }
        let mark = pos2(columns.state, rect.center().y);
        match status {
            Status::AwaitingApproval | Status::Faulted(_) => {
                theme::attention(&painter, mark, view.k, p);
            }
            Status::Running => theme::running(&painter, mark, view.k, p),
            Status::Disabled => theme::off(&painter, mark, view.k, p),
        }
        if !acted {
            if response.double_clicked() && *status == Status::Running {
                opened |= self.act(view, plugin, RowAction::Open);
            } else if response.clicked() {
                if *status == Status::AwaitingApproval {
                    self.selected = Some(id.to_owned());
                    self.dialog = Some(id.to_owned());
                } else if chosen {
                    self.selected = None;
                } else {
                    self.selected = Some(id.to_owned());
                }
            }
        }
        response.context_menu(|ui| {
            self.selected = Some(id.to_owned());
            opened |= self.row_menu(ui, view, plugin, status);
        });
        if chosen {
            opened |= self.details(ui, view, plugin, status);
        }
        opened
    }

    /// The actions of a pointed row, as words at its right. Returns whether
    /// the player used one, and whether it opened the plugin.
    #[allow(clippy::too_many_arguments)]
    fn action_words(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        view: &View<'_>,
        plugin: &Plugin,
        status: &Status,
        rect: Rect,
        columns: &Columns,
    ) -> (bool, bool) {
        let p = view.p;
        let id = plugin.manifest.id();
        let (mut acted, mut opened) = (false, false);
        let mut right = columns.version;
        for (index, (text, action)) in self.row_actions(status).into_iter().enumerate().rev() {
            let kind = if index == 0 {
                Word::Action
            } else {
                Word::Secondary
            };
            let galley = painter.layout_no_wrap(
                text.to_owned(),
                if kind == Word::Action {
                    medium(view.at(13.0))
                } else {
                    sans(view.at(13.0))
                },
                p.text,
            );
            let word = Rect::from_min_max(
                pos2(right - galley.size().x, rect.top()),
                pos2(right, rect.bottom()),
            );
            let hit = ui
                .interact(
                    word.expand2(vec2(view.at(4.0), 0.0)),
                    Id::new(("angelica-row-action", id, index)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let color = match kind {
                Word::Action => p.action,
                _ if hit.hovered() => p.text,
                _ => p.secondary,
            };
            let at = pos2(word.left(), rect.center().y - galley.size().y / 2.0);
            let width = galley.size().x;
            let bottom = at.y + galley.size().y;
            painter.galley_with_override_text_color(at, galley, color);
            if hit.hovered() {
                painter.line_segment(
                    [pos2(at.x, bottom), pos2(at.x + width, bottom)],
                    Stroke::new(1.0, theme::alpha(color, 0.6)),
                );
            }
            if hit.clicked() {
                acted = true;
                opened |= self.act(view, plugin, action);
            }
            right = word.left() - view.at(14.0);
        }
        (acted, opened)
    }

    /// What can be done with a plugin in its state, the main action first.
    fn row_actions(&self, status: &Status) -> Vec<(&'static str, RowAction)> {
        let s = self.strings();
        match status {
            Status::Running => vec![(s.open, RowAction::Open), (s.turn_off, RowAction::TurnOff)],
            Status::Disabled => vec![(s.turn_on, RowAction::TurnOn)],
            Status::AwaitingApproval => vec![(s.review, RowAction::Review)],
            Status::Faulted(_) => vec![
                (s.retry, RowAction::TurnOn),
                (s.keep_off, RowAction::TurnOff),
            ],
        }
    }

    /// Does `action` for `plugin`. Returns true when it opened the plugin,
    /// which closes the menu.
    fn act(&mut self, view: &View<'_>, plugin: &Plugin, action: RowAction) -> bool {
        let id = plugin.manifest.id();
        match action {
            RowAction::Open => {
                view.host.queue_event(id, Event::OpenRequested);
                return true;
            }
            RowAction::TurnOn => view.host.queue(Action::Enable(id.to_owned())),
            RowAction::TurnOff => view.host.queue(Action::Disable(id.to_owned())),
            RowAction::Review => {
                self.selected = Some(id.to_owned());
                self.dialog = Some(id.to_owned());
            }
            RowAction::Folder => view.host.queue(Action::OpenFolder(id.to_owned())),
            RowAction::Remove => self.removing = Some(id.to_owned()),
        }
        false
    }

    /// The chosen plugin's details under its row: what it does, what it may
    /// do, where it comes from, and every action. Returns true when the
    /// player opened the plugin.
    fn details(&mut self, ui: &mut Ui, view: &View<'_>, plugin: &Plugin, status: &Status) -> bool {
        let s = self.strings();
        let p = view.p;
        let manifest = &plugin.manifest;
        let mut action = None;
        egui::Frame::new()
            .fill(p.hover)
            .inner_margin(egui::Margin {
                left: num_u8(view.at(46.0)),
                right: num_u8(view.at(16.0)),
                top: num_u8(view.at(4.0)),
                bottom: num_u8(view.at(12.0)),
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = view.at(6.0);
                if !manifest.plugin.description.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&manifest.plugin.description)
                                .font(sans(view.at(13.0)))
                                .color(p.secondary),
                        )
                        .wrap(),
                    );
                }
                if let Status::Faulted(reason) = status {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{} {reason}", s.state_stopped))
                                .font(sans(view.at(12.5)))
                                .color(p.attention),
                        )
                        .wrap(),
                    );
                }
                let permissions = manifest.permissions();
                if permissions.is_empty() {
                    permission_line(ui, view, None, s.asks_nothing, p.secondary);
                }
                for &permission in permissions {
                    permission_line(
                        ui,
                        view,
                        Some(class_icon(permission.class())),
                        &self.permission_text(plugin, permission),
                        p.text,
                    );
                }
                ui.label(
                    RichText::new(format!(
                        "{}, {}",
                        fill(s.version, &[("version", &manifest.plugin.version)]),
                        self.source(plugin)
                    ))
                    .font(sans(view.at(12.0)))
                    .color(p.muted),
                );
                self.history(ui, view, plugin);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = view.at(14.0);
                    let mut actions = self.row_actions(status);
                    if *status != Status::AwaitingApproval {
                        actions.push((s.open_folder, RowAction::Folder));
                    }
                    actions.push((s.remove, RowAction::Remove));
                    for (index, (text, row_action)) in actions.into_iter().enumerate() {
                        let kind = if row_action == RowAction::Remove {
                            Word::Danger
                        } else if index == 0 {
                            Word::Main
                        } else {
                            Word::Secondary
                        };
                        if theme::word(ui, text, kind, true, view.k).clicked() {
                            action = Some(row_action);
                        }
                    }
                });
            });
        action.is_some_and(|action| self.act(view, plugin, action))
    }

    /// The menu of a row's right click. Returns true when the player opened
    /// the plugin.
    fn row_menu(&mut self, ui: &mut Ui, view: &View<'_>, plugin: &Plugin, status: &Status) -> bool {
        let s = self.strings();
        let p = view.p;
        let id = plugin.manifest.id();
        ui.set_min_width(view.at(250.0));
        ui.set_max_width(view.at(280.0));
        ui.horizontal(|ui| {
            ui.add_space(view.at(6.0));
            let (tile, _) =
                ui.allocate_exact_size(vec2(view.at(30.0), view.at(30.0)), Sense::hover());
            theme::tile(ui.painter(), tile, &plugin.manifest.plugin.name, false, p);
            ui.add_space(view.at(4.0));
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&plugin.manifest.plugin.name)
                        .font(strong(view.at(13.5)))
                        .color(p.text),
                );
                let (state, color) = self.state_line(status, p);
                ui.label(RichText::new(state).font(sans(view.at(12.0))).color(color));
            });
        });
        ui.add_space(view.at(4.0));
        let mut opened = false;
        let separate = |ui: &mut Ui| {
            ui.add_space(view.at(3.0));
            theme::rule(ui);
            ui.add_space(view.at(3.0));
        };
        separate(ui);
        match status {
            Status::AwaitingApproval => {
                if menu_item(ui, view, None, s.review, p.action) {
                    self.dialog = Some(id.to_owned());
                }
            }
            Status::Running => {
                if menu_item(ui, view, None, s.open, p.action) {
                    opened = true;
                }
                if menu_item(ui, view, None, s.turn_off, p.text) {
                    view.host.queue(Action::Disable(id.to_owned()));
                }
            }
            Status::Disabled => {
                if menu_item(ui, view, None, s.turn_on, p.action) {
                    view.host.queue(Action::Enable(id.to_owned()));
                }
            }
            Status::Faulted(_) => {
                if menu_item(ui, view, None, s.retry, p.action) {
                    view.host.queue(Action::Enable(id.to_owned()));
                }
                if menu_item(ui, view, None, s.keep_off, p.text) {
                    view.host.queue(Action::Disable(id.to_owned()));
                }
            }
        }
        if *status != Status::AwaitingApproval {
            separate(ui);
            if menu_item(ui, view, None, s.open_folder, p.text) {
                view.host.queue(Action::OpenFolder(id.to_owned()));
            }
        }
        separate(ui);
        if menu_item(ui, view, None, s.remove, p.danger) {
            self.removing = Some(id.to_owned());
        }
        if opened {
            view.host.queue_event(id, Event::OpenRequested);
        }
        opened
    }

    /// What a plugin's state is called, and its color.
    fn state_line(&self, status: &Status, p: &Palette) -> (&'static str, Color32) {
        let s = self.strings();
        match status {
            Status::Running => (s.state_running, p.on),
            Status::Disabled => (s.state_off, p.secondary),
            Status::AwaitingApproval => (s.state_waiting, p.attention),
            Status::Faulted(_) => (s.state_stopped, p.attention),
        }
    }

    /// Where a plugin comes from: the catalog, or its authors.
    fn source(&self, plugin: &Plugin) -> String {
        let s = self.strings();
        let manifest = &plugin.manifest;
        match manifest.origin {
            Origin::Catalog => s.source_catalog.to_owned(),
            Origin::Author if manifest.plugin.authors.is_empty() => s.source_author.to_owned(),
            Origin::Author => fill(
                s.source_authors,
                &[("authors", &manifest.plugin.authors.join(", "))],
            ),
        }
    }

    /// A permission in plain words, with the sites of a network one.
    fn permission_text(&self, plugin: &Plugin, permission: Permission) -> String {
        let text = self.language.permission(permission);
        if permission == Permission::HttpGet {
            let hosts: Vec<String> = plugin.manifest.http_hosts().into_iter().collect();
            format!("{text}: {}", hosts.join(", "))
        } else {
            text.to_owned()
        }
    }

    /// The field for a plugin's link, and what is wrong with the link.
    fn draw_link(&mut self, ui: &mut Ui, view: &View<'_>, installer: &PluginInstaller) {
        let s = self.strings();
        let p = view.p;
        let busy = installer.busy();
        let label = if busy { s.adding } else { s.add };
        let field_id = Id::new("angelica-link");
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), view.at(44.0)), Sense::hover());
        ui.painter()
            .line_segment([row.left_top(), row.right_top()], Stroke::new(1.0, p.line));
        let button_width = ui
            .painter()
            .layout_no_wrap(label.to_owned(), medium(view.at(13.5)), p.action)
            .size()
            .x
            + view.at(8.0);
        let field = Rect::from_min_max(
            pos2(row.left() + view.at(10.0), row.top() + view.at(9.0)),
            pos2(
                row.right() - view.at(22.0) - button_width,
                row.bottom() - view.at(9.0),
            ),
        );
        let response = text_field(
            ui,
            &view.look(),
            field,
            &mut self.link,
            s.link_hint,
            field_id,
            None,
        );
        if response.changed() {
            self.link_invalid = false;
        }
        let submit = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let enabled = !busy && !self.link.trim().is_empty();
        let button = Rect::from_min_max(
            pos2(field.right() + view.at(12.0), row.top()),
            pos2(row.right() - view.at(10.0), row.bottom()),
        );
        let clicked = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(button)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| theme::word(ui, label, Word::Action, enabled, view.k).clicked(),
            )
            .inner;
        if enabled && (clicked || submit) {
            match installer.install(&self.link) {
                Ok(()) => self.link.clear(),
                Err(_) => self.link_invalid = true,
            }
        }
        let focused = ui.ctx().memory(|memory| memory.has_focus(field_id));
        let note = if self.link_invalid {
            Some((s.link_invalid, p.danger))
        } else if focused || !self.link.is_empty() {
            Some((s.link_note, p.secondary))
        } else {
            None
        };
        if let Some((text, color)) = note {
            ui.horizontal(|ui| {
                ui.add_space(view.at(12.0));
                ui.add(
                    egui::Label::new(RichText::new(text).font(sans(view.at(12.0))).color(color))
                        .wrap(),
                );
            });
            ui.add_space(view.at(8.0));
        }
    }

    /// How to close the menu.
    fn footer(&self, ui: &mut Ui, view: &View<'_>) {
        let p = view.p;
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), view.at(28.0)), Sense::hover());
        let painter = ui.painter();
        painter.line_segment([row.left_top(), row.right_top()], Stroke::new(1.0, p.line));
        painter.text(
            pos2(row.right() - view.at(12.0), row.center().y),
            Align2::RIGHT_CENTER,
            self.strings().esc_closes,
            sans(view.at(12.0)),
            p.muted,
        );
    }

    /// "1 waiting for you, 14 running".
    fn pulse(&self, statuses: &[Status]) -> String {
        if statuses.is_empty() {
            return self.strings().nothing_yet.to_owned();
        }
        let waiting = statuses
            .iter()
            .filter(|status| Group::of(status) == Group::Attention)
            .count();
        let running = statuses
            .iter()
            .filter(|status| **status == Status::Running)
            .count();
        let running = self.language.running(running);
        if waiting > 0 {
            format!("{}, {running}", self.language.waiting(waiting))
        } else {
            running
        }
    }

    /// The dialog of what a plugin waiting for approval asks.
    fn draw_dialog(&mut self, context: &egui::Context, view: &View<'_>) {
        let s = self.strings();
        let p = view.p;
        let plugin = self.dialog.as_deref().and_then(|id| {
            view.plugins
                .iter()
                .find(|plugin| plugin.manifest.id() == id)
        });
        let Some(plugin) = plugin.filter(|plugin| plugin.status() == Status::AwaitingApproval)
        else {
            self.dialog = None;
            return;
        };
        let id = plugin.manifest.id().to_owned();
        let more = plugin.approved_before();
        let heading = if more { s.dialog_more } else { s.dialog_new };
        let width = view.at(420.0);
        let mut close = false;
        egui::Window::new("angelica-dialog")
            .id(Id::new(("angelica-dialog", &id)))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .order(egui::Order::Foreground)
            .frame(window_frame(context, p))
            .default_pos(pos2(
                view.screen.center().x - width / 2.0,
                view.screen.top() + view.screen.height() * 0.22,
            ))
            .show(context, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if title_bar(ui, &view.look(), heading, s) {
                    close = true;
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(
                        num_u8(view.at(16.0)),
                        num_u8(view.at(14.0)),
                    ))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = view.at(6.0);
                        ui.horizontal(|ui| {
                            let (tile, _) = ui.allocate_exact_size(
                                vec2(view.at(40.0), view.at(40.0)),
                                Sense::hover(),
                            );
                            theme::tile(ui.painter(), tile, &plugin.manifest.plugin.name, false, p);
                            ui.add_space(view.at(12.0));
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&plugin.manifest.plugin.name)
                                        .font(strong(view.at(14.0)))
                                        .color(p.text),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{}, {}",
                                        fill(
                                            s.version,
                                            &[("version", &plugin.manifest.plugin.version)]
                                        ),
                                        self.source(plugin)
                                    ))
                                    .font(sans(view.at(12.0)))
                                    .color(p.secondary),
                                );
                            });
                        });
                        ui.add_space(view.at(8.0));
                        self.dialog_requests(ui, view, plugin, more);
                    });
                let (row, _) = ui
                    .allocate_exact_size(vec2(ui.available_width(), view.at(40.0)), Sense::hover());
                ui.painter()
                    .line_segment([row.left_top(), row.right_top()], Stroke::new(1.0, p.line));
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(row.shrink2(vec2(view.at(10.0), 0.0)))
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = view.at(14.0);
                        if theme::word(ui, s.allow, Word::Main, true, view.k).clicked() {
                            view.host.queue(Action::Approve(id.clone()));
                            close = true;
                        }
                        if theme::word(ui, s.not_now, Word::Secondary, true, view.k).clicked() {
                            close = true;
                        }
                    },
                );
            });
        if close {
            self.dialog = None;
        }
    }

    /// What the plugin asks for, highlighted, and, for a new version, what
    /// it already has.
    fn dialog_requests(&self, ui: &mut Ui, view: &View<'_>, plugin: &Plugin, more: bool) {
        let s = self.strings();
        let p = view.p;
        let requests = plugin.requests();
        let asked: Vec<Permission> = requests
            .iter()
            .flat_map(|(_, requests)| requests.iter().map(|request| request.permission))
            .collect();
        let title = if more { s.asks_first } else { s.asks };
        theme::header(
            ui,
            title,
            None,
            if more { p.attention } else { p.secondary },
            view.k,
        );
        let tint = if more { p.attention } else { p.line };
        egui::Frame::new()
            .fill(theme::alpha(tint, if more { 0.08 } else { 0.0 }))
            .stroke(Stroke::new(
                1.0,
                theme::alpha(tint, if more { 0.35 } else { 0.0 }),
            ))
            .corner_radius(CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(
                num_u8(view.at(10.0)),
                num_u8(view.at(8.0)),
            ))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = view.at(6.0);
                if requests.is_empty() {
                    permission_line(ui, view, None, s.asks_nothing, p.secondary);
                }
                for (class, requests) in &requests {
                    for request in requests {
                        permission_line(
                            ui,
                            view,
                            Some(class_icon(*class)),
                            self.language.permission(request.permission),
                            p.text,
                        );
                        if !request.hosts.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add_space(view.at(24.0));
                                ui.label(
                                    RichText::new(request.hosts.join("  "))
                                        .font(mono(view.at(12.0)))
                                        .color(p.secondary),
                                );
                            });
                        }
                    }
                }
            });
        let already: Vec<Permission> = plugin
            .manifest
            .permissions()
            .iter()
            .copied()
            .filter(|permission| !asked.contains(permission))
            .collect();
        if more && !already.is_empty() {
            ui.add_space(view.at(6.0));
            theme::header(ui, s.already, None, p.secondary, view.k);
            for permission in already {
                ui.horizontal(|ui| {
                    ui.add_space(view.at(10.0));
                    permission_line(
                        ui,
                        view,
                        Some(class_icon(permission.class())),
                        &self.permission_text(plugin, permission),
                        p.secondary,
                    );
                });
            }
        }
    }

    /// Settings: the sections on the left, the chosen one on the right.
    fn draw_settings(&mut self, ui: &mut Ui, view: &View<'_>) {
        let p = view.p;
        let height = view.at(COLUMNS + ROW * VISIBLE_ROWS + AROUND_LIST);
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let column = Rect::from_min_size(area.min, vec2(view.at(180.0), height));
        let painter = ui.painter().clone();
        painter.rect_filled(
            column,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 6,
                se: 0,
            },
            p.column,
        );
        painter.line_segment(
            [column.right_top(), column.right_bottom()],
            Stroke::new(1.0, p.line),
        );
        let mut column_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(column.shrink2(vec2(0.0, view.at(8.0))))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        self.sections(&mut column_ui, view);
        let right = Rect::from_min_max(
            pos2(column.right() + view.at(16.0), area.top() + view.at(10.0)),
            pos2(area.right() - view.at(16.0), area.bottom() - view.at(10.0)),
        );
        let mut content = ui.new_child(
            UiBuilder::new()
                .max_rect(right)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        match self.section {
            Section::General => self.general(&mut content, view),
            Section::Updates => self.updates(&mut content, view),
            Section::Developers => {
                egui::ScrollArea::vertical()
                    .id_salt("angelica-developers")
                    .auto_shrink([false, false])
                    .show(&mut content, |ui| self.developers(ui, view));
            }
        }
    }

    fn sections(&mut self, ui: &mut Ui, view: &View<'_>) {
        let s = self.strings();
        let p = view.p;
        let sections = [
            (Section::General, Icon::General, s.section_general),
            (Section::Updates, Icon::Updates, s.section_updates),
            (Section::Developers, Icon::Developers, s.section_developers),
        ];
        for (section, icon, title) in sections {
            if section == Section::Developers {
                ui.add_space(view.at(8.0));
                let (line, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().line_segment(
                    [
                        pos2(line.left() + view.at(14.0), line.center().y),
                        pos2(line.right() - view.at(14.0), line.center().y),
                    ],
                    Stroke::new(1.0, p.line),
                );
                ui.add_space(view.at(8.0));
            }
            let (row, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), view.at(32.0)), Sense::click());
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            let chosen = self.section == section;
            let plate = row.shrink2(vec2(view.at(6.0), view.at(1.0)));
            let painter = ui.painter();
            if chosen {
                painter.rect_filled(plate, CornerRadius::same(4), p.chosen);
                painter.rect_stroke(
                    plate,
                    CornerRadius::same(4),
                    Stroke::new(1.0, p.line),
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                painter.rect_filled(plate, CornerRadius::same(4), p.hover);
            }
            let faint = section == Section::Developers && !chosen;
            theme::icon(
                painter,
                icon,
                Rect::from_center_size(
                    pos2(plate.left() + view.at(16.0), row.center().y),
                    vec2(view.at(14.0), view.at(14.0)),
                ),
                if chosen { p.action } else { p.secondary },
            );
            painter.text(
                pos2(plate.left() + view.at(32.0), row.center().y),
                Align2::LEFT_CENTER,
                title,
                if chosen {
                    medium(view.at(13.0))
                } else {
                    sans(view.at(13.0))
                },
                if faint { p.secondary } else { p.text },
            );
            if response.clicked() {
                self.section = section;
            }
            ui.add_space(view.at(2.0));
        }
    }

    fn general(&mut self, ui: &mut Ui, view: &View<'_>) {
        let s = self.strings();
        let p = view.p;
        let key = self.key().name();
        setting(ui, view, s.setting_key, |ui| {
            let text = if self.choosing_key { s.key_press } else { key };
            if theme::key(ui, text, view.k).clicked() {
                self.choosing_key = !self.choosing_key;
            }
        });
        if self.choosing_key {
            ui.add(
                egui::Label::new(
                    RichText::new(fill(s.key_note, &[("key", key)]))
                        .font(sans(view.at(12.0)))
                        .color(p.secondary),
                )
                .wrap(),
            );
            ui.add_space(view.at(10.0));
        }
        let mut language = None;
        setting(ui, view, s.setting_language, |ui| {
            let response = theme::value(ui, self.language.name(), view.k);
            egui::Popup::menu(&response).show(|ui| {
                for option in Language::ALL {
                    if menu_item(ui, view, None, option.name(), p.text) {
                        language = Some(option);
                    }
                }
            });
        });
        if let Some(language) = language.filter(|language| *language != self.language) {
            self.set_language(view.host, language);
        }
        let mut theme = None;
        setting(ui, view, s.setting_theme, |ui| {
            let chosen = Theme::ALL
                .iter()
                .position(|theme| *theme == self.theme)
                .unwrap_or(0);
            theme = theme::choice(ui, &[s.theme_light, s.theme_dark], chosen, view.k)
                .map(|index| Theme::ALL[index]);
        });
        if let Some(theme) = theme {
            self.set_theme(view.host, theme);
        }
        let mut size = None;
        setting(ui, view, s.setting_size, |ui| {
            let current = self.settings.size.unwrap_or_default();
            let chosen = Size::ALL
                .iter()
                .position(|size| *size == current)
                .unwrap_or(1);
            size = theme::choice(
                ui,
                &[s.size_small, s.size_normal, s.size_large],
                chosen,
                view.k,
            )
            .map(|index| Size::ALL[index]);
        });
        if let Some(size) = size {
            self.settings.size = Some(size);
            self.save(view.host);
        }
    }

    fn updates(&self, ui: &mut Ui, view: &View<'_>) {
        let s = self.strings();
        let p = view.p;
        if let Some(installer) = view.host.plugin_installer() {
            setting(ui, view, s.setting_check, |ui| {
                let checking = installer.checking();
                let label = if checking { s.checking } else { s.check_now };
                if theme::word(ui, label, Word::Action, !checking, view.k).clicked() {
                    installer.check_updates();
                }
            });
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{}. {}",
                        capitalized(&self.checked(installer)),
                        s.check_note
                    ))
                    .font(sans(view.at(12.0)))
                    .color(p.secondary),
                )
                .wrap(),
            );
            ui.add_space(view.at(10.0));
        }
        setting(ui, view, s.setting_updates, |ui| {
            ui.label(
                RichText::new(s.updates_auto)
                    .font(strong(view.at(13.5)))
                    .color(p.text),
            );
        });
        ui.add(
            egui::Label::new(
                RichText::new(s.updates_note)
                    .font(sans(view.at(12.0)))
                    .color(p.secondary),
            )
            .wrap(),
        );
    }

    /// When updates were last checked, in words.
    fn checked(&self, installer: &PluginInstaller) -> String {
        let s = self.strings();
        let Some(at) = installer.last_checked() else {
            return s.checked_never.to_owned();
        };
        let minutes = at.elapsed().as_secs() / 60;
        if minutes == 0 {
            s.checked_now.to_owned()
        } else if minutes < 60 {
            fill(s.checked_minutes, &[("n", &minutes.to_string())])
        } else {
            fill(s.checked_hours, &[("n", &(minutes / 60).to_string())])
        }
    }

    /// What the plugin installer did with `plugin` in this session.
    fn history(&self, ui: &mut Ui, view: &View<'_>, plugin: &Plugin) {
        let s = self.strings();
        let p = view.p;
        let id = plugin.manifest.id();
        let reports = view
            .host
            .plugin_installer()
            .map(|installer| installer.messages())
            .unwrap_or_default();
        let lines: Vec<(String, Color32)> = reports
            .iter()
            .filter_map(|(_, report)| match report {
                Report::Installed { plugin, version } if plugin == id => Some((
                    fill(s.history_installed, &[("version", version)]),
                    p.secondary,
                )),
                Report::Updated { plugin, from, to } if plugin == id => Some((
                    fill(s.history_updated, &[("from", from), ("to", to)]),
                    p.secondary,
                )),
                Report::Compiled(plugin) if plugin == id => {
                    Some((s.history_compiled.to_owned(), p.secondary))
                }
                Report::Failed(Failure::Update(plugin), reason) if plugin == id => Some((
                    fill(s.history_update_failed, &[("reason", reason)]),
                    p.danger,
                )),
                Report::Failed(Failure::Compile(plugin), reason) if plugin == id => Some((
                    fill(s.history_compile_failed, &[("reason", reason)]),
                    p.danger,
                )),
                _ => None,
            })
            .collect();
        if lines.is_empty() {
            return;
        }
        ui.label(
            RichText::new(s.history)
                .font(sans(view.at(12.0)))
                .color(p.muted),
        );
        for (line, color) in lines {
            ui.add(
                egui::Label::new(RichText::new(line).font(sans(view.at(12.5))).color(color)).wrap(),
            );
        }
    }

    /// The dialog that removes a plugin, keeping its data or not.
    fn draw_removal(&mut self, context: &egui::Context, view: &View<'_>) {
        let s = self.strings();
        let p = view.p;
        let Some(plugin) = self
            .removing
            .as_deref()
            .and_then(|id| {
                view.plugins
                    .iter()
                    .find(|plugin| plugin.manifest.id() == id)
            })
            .cloned()
        else {
            self.removing = None;
            return;
        };
        let id = plugin.manifest.id().to_owned();
        let width = view.at(520.0);
        let (mut close, mut removed) = (false, false);
        egui::Window::new("angelica-removal")
            .id(Id::new(("angelica-removal", &id)))
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .order(egui::Order::Foreground)
            .frame(window_frame(context, p))
            .default_pos(pos2(
                view.screen.center().x - width / 2.0,
                view.screen.top() + view.screen.height() * 0.25,
            ))
            .show(context, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if title_bar(ui, &view.look(), s.remove_title, s) {
                    close = true;
                }
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(
                        num_u8(view.at(16.0)),
                        num_u8(view.at(14.0)),
                    ))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = view.at(10.0);
                        plugin_heading(
                            ui,
                            view,
                            &plugin.manifest.plugin.name,
                            &fill(s.version, &[("version", &plugin.manifest.plugin.version)]),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(s.remove_note)
                                    .font(sans(view.at(12.5)))
                                    .color(p.secondary),
                            )
                            .wrap(),
                        );
                    });
                let (row, _) = ui
                    .allocate_exact_size(vec2(ui.available_width(), view.at(40.0)), Sense::hover());
                ui.painter()
                    .line_segment([row.left_top(), row.right_top()], Stroke::new(1.0, p.line));
                ui.scope_builder(
                    UiBuilder::new()
                        .max_rect(row.shrink2(vec2(view.at(10.0), 0.0)))
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = view.at(14.0);
                        for (text, data) in [(s.remove_all, true), (s.remove_keep, false)] {
                            if theme::word(ui, text, Word::Danger, true, view.k).clicked() {
                                view.host.queue(Action::Remove {
                                    id: id.clone(),
                                    data,
                                });
                                removed = true;
                            }
                        }
                        if theme::word(ui, s.cancel, Word::Secondary, true, view.k).clicked() {
                            close = true;
                        }
                    },
                );
            });
        if close || removed {
            self.removing = None;
        }
        if removed && self.selected.as_deref() == Some(id.as_str()) {
            self.selected = None;
        }
    }

    /// The details for developers: the game map, each plugin with its id,
    /// state, folder, and log, the installer's reports, and diagnostics.
    fn developers(&self, ui: &mut Ui, view: &View<'_>) {
        let host = view.host;
        let s = self.strings();
        let p = view.p;
        ui.spacing_mut().item_spacing.y = view.at(4.0);
        theme::header(ui, s.dev_map, None, p.secondary, view.k);
        for line in view.map_status {
            mono_line(ui, view, line, p.text);
        }
        theme::header(
            ui,
            s.dev_plugins,
            Some(view.plugins.len()),
            p.secondary,
            view.k,
        );
        if view.plugins.is_empty() {
            mono_line(ui, view, s.dev_nothing, p.muted);
        }
        for plugin in view.plugins {
            developer_plugin(ui, view, s, plugin);
        }
        theme::header(ui, s.dev_installer, None, p.secondary, view.k);
        let reports = host
            .plugin_installer()
            .map(|installer| installer.messages())
            .unwrap_or_default();
        if reports.is_empty() {
            mono_line(ui, view, s.dev_nothing, p.muted);
        }
        for (_, report) in reports.iter().rev() {
            let color = if matches!(report, Report::Failed(..)) {
                p.danger
            } else {
                p.text
            };
            mono_line(ui, view, &report.to_string(), color);
        }
        theme::header(ui, s.dev_diagnostics, None, p.secondary, view.k);
        let diagnostics = host.diagnostics();
        if diagnostics.is_empty() {
            mono_line(ui, view, s.dev_nothing, p.muted);
        }
        for line in diagnostics.iter().rev() {
            mono_line(ui, view, line, p.text);
        }
    }

    fn collect_notes(&mut self, installer: &PluginInstaller, time: f64) {
        for (number, report) in installer.messages() {
            if number <= self.seen {
                continue;
            }
            self.seen = number;
            if matches!(
                report,
                Report::Installed { .. }
                    | Report::Updated { .. }
                    | Report::Failed(Failure::Install(_), _)
            ) {
                self.notes.push(Note { report, at: time });
            }
        }
        self.notes.retain(|note| time - note.at < NOTE_TIME);
        let extra = self.notes.len().saturating_sub(3);
        self.notes.drain(..extra);
    }

    /// What a note says, and its tone.
    fn note_text(&self, host: &Host, report: &Report) -> (String, Tone) {
        let s = self.strings();
        let name = |id: &str| {
            host.plugin(id).map_or_else(
                || id.to_owned(),
                |plugin| plugin.manifest.plugin.name.clone(),
            )
        };
        match report {
            Report::Installed { plugin, .. } => (
                fill(s.note_installed, &[("name", &name(plugin))]),
                Tone::Attention,
            ),
            Report::Updated { plugin, .. } => {
                let asks = host
                    .plugin(plugin)
                    .is_some_and(|plugin| plugin.status() == Status::AwaitingApproval);
                let template = if asks {
                    s.note_updated_asks
                } else {
                    s.note_updated
                };
                (
                    fill(template, &[("name", &name(plugin))]),
                    if asks { Tone::Attention } else { Tone::Good },
                )
            }
            _ => (s.note_failed.to_owned(), Tone::Failed),
        }
    }

    /// The notes at the top right while the menu is closed: how Angelica
    /// is for the first seconds of the game and while a plugin needs the
    /// player, then what the plugin installer did. They take no input.
    fn draw_notes(&self, context: &egui::Context, view: &View<'_>) {
        let waiting = view
            .statuses
            .iter()
            .filter(|status| Group::of(status) == Group::Attention)
            .count();
        let mut notes: Vec<(String, Tone, f32)> = Vec::new();
        if !view.statuses.is_empty() && (waiting > 0 || view.time < START_TIME) {
            let fade = if waiting > 0 {
                1.0
            } else {
                fraction((START_TIME - view.time) / 0.5)
            };
            let tone = if waiting > 0 {
                Tone::Attention
            } else {
                Tone::Good
            };
            notes.push((self.pulse(view.statuses), tone, fade));
        }
        for note in self.notes.iter().rev() {
            let (text, tone) = self.note_text(view.host, &note.report);
            let age = view.time - note.at;
            notes.push((
                text,
                tone,
                fraction((age / 0.25).min((NOTE_TIME - age) / 0.5)),
            ));
        }
        let hint = fill(self.strings().hint, &[("key", self.key().name())]);
        let mut top = view.screen.top() + view.at(24.0);
        for (index, (text, tone, fade)) in notes.into_iter().enumerate() {
            let area = egui::Area::new(Id::new(("angelica-note", index)))
                .order(egui::Order::Foreground)
                .interactable(false)
                .pivot(Align2::RIGHT_TOP)
                .fixed_pos(pos2(view.screen.right() - view.at(24.0), top))
                .show(context, |ui| {
                    ui.multiply_opacity(fade);
                    note(ui, view, &text, tone, &hint);
                });
            top = area.response.rect.bottom() + view.at(8.0);
        }
    }
}

/// A plugin at the top of a dialog: its tile, its name, and one line.
fn plugin_heading(ui: &mut Ui, view: &View<'_>, name: &str, line: &str) {
    let p = view.p;
    ui.horizontal(|ui| {
        let (tile, _) = ui.allocate_exact_size(vec2(view.at(40.0), view.at(40.0)), Sense::hover());
        theme::tile(ui.painter(), tile, name, false, p);
        ui.add_space(view.at(12.0));
        ui.vertical(|ui| {
            ui.label(
                RichText::new(name)
                    .font(strong(view.at(14.0)))
                    .color(p.text),
            );
            ui.label(
                RichText::new(line)
                    .font(sans(view.at(12.0)))
                    .color(p.secondary),
            );
        });
    });
}

/// The frame of Angelica's windows.
pub(crate) fn window_frame(context: &egui::Context, p: &Palette) -> egui::Frame {
    egui::Frame::new()
        .fill(p.window)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(6))
        .shadow(context.global_style().visuals.window_shadow)
}

/// The mark before a title bar's status.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    None,
    Running,
    Attention,
}

/// What a window's title bar shows. From the left: where the player is (the
/// word `angelica` in the menu, the title elsewhere, and the path with a way
/// back), what goes on (the status), and the window's controls.
pub(crate) struct Bar<'a> {
    /// The word `angelica` in place of a title: the menu only.
    pub wordmark: bool,
    pub title: &'a str,
    pub path: &'a [String],
    pub status: Option<(&'a str, Mark)>,
    /// Whether the window can be collapsed to its bar, and is.
    pub collapsed: Option<bool>,
    pub menu: bool,
    pub strings: &'a Strings,
}

impl<'a> Bar<'a> {
    pub(crate) fn titled(title: &'a str, strings: &'a Strings) -> Self {
        Self {
            wordmark: false,
            title,
            path: &[],
            status: None,
            collapsed: None,
            menu: false,
            strings,
        }
    }
}

/// What the player pressed in a title bar.
#[derive(Default)]
pub(crate) struct Pressed {
    pub close: bool,
    /// Collapse or expand.
    pub toggle: bool,
    /// Go back to this many parts of the path.
    pub navigate: Option<u32>,
    /// The menu button, to open a menu under.
    pub menu: Option<egui::Response>,
}

/// A small square button of a title bar.
fn bar_button(ui: &mut Ui, view: &Look, center: Pos2, icon: Icon, hint: &str) -> egui::Response {
    let p = view.p;
    let button = Rect::from_center_size(center, vec2(view.at(24.0), view.at(24.0)));
    let response = ui
        .interact(button, ui.id().with(("angelica-bar", hint)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint);
    if response.hovered() {
        ui.painter()
            .rect_filled(button, CornerRadius::same(4), p.hover);
    }
    theme::icon(
        ui.painter(),
        icon,
        Rect::from_center_size(button.center(), vec2(view.at(11.0), view.at(11.0))),
        if response.hovered() {
            p.text
        } else {
            p.secondary
        },
    );
    response
}

/// The title and the path with a way back, from `x`. Returns where they end.
fn bar_path(
    ui: &mut Ui,
    view: &Look,
    bar: &Bar<'_>,
    mut x: f32,
    controls: f32,
    y: f32,
    pressed: &mut Pressed,
) -> f32 {
    let p = view.p;
    let s = bar.strings;
    let painter = ui.painter().clone();
    let collapsed = bar.collapsed == Some(true);
    let deep = !bar.path.is_empty() && !collapsed;
    if deep {
        let back = bar_button(ui, view, pos2(x + view.at(4.0), y), Icon::Back, s.bar_back);
        if back.clicked() {
            pressed.navigate = u32::try_from(bar.path.len() - 1).ok();
        }
        x += view.at(20.0);
    }
    let parts = std::iter::once(bar.title)
        .chain(bar.path.iter().map(String::as_str).filter(|_| !collapsed));
    let last = if collapsed { 0 } else { bar.path.len() };
    // The path gives way to the status: it keeps at most this much.
    let limit = (controls - x) * if bar.status.is_some() { 0.6 } else { 1.0 };
    let end = x + limit - view.at(8.0);
    for (index, part) in parts.enumerate() {
        if index > 0 {
            let mark = painter.text(
                pos2(x + view.at(6.0), y),
                Align2::LEFT_CENTER,
                "›",
                sans(view.at(13.0)),
                p.muted,
            );
            x = mark.right() + view.at(6.0);
        }
        let current = index == last;
        let font = if current {
            medium(view.at(13.0))
        } else {
            sans(view.at(13.0))
        };
        let galley = elided(&painter, part, font, p.text, (end - x).max(view.at(24.0)));
        let size = galley.size();
        let at = pos2(x, y - size.y / 2.0);
        let part_rect = Rect::from_min_size(at, size);
        let mut color = if current { p.text } else { p.secondary };
        if !current {
            let response = ui
                .interact(
                    part_rect.expand(view.at(3.0)),
                    ui.id().with(("angelica-bar-part", index)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.hovered() {
                color = p.text;
            }
            if response.clicked() {
                pressed.navigate = u32::try_from(index).ok();
            }
        }
        painter.galley_with_override_text_color(at, galley, color);
        x += size.x;
        if x >= end {
            break;
        }
    }
    x
}

/// The status and its mark, right before the controls.
fn bar_status(ui: &mut Ui, view: &Look, text: &str, mark: Mark, x: f32, controls: f32, y: f32) {
    let p = view.p;
    let painter = ui.painter().clone();
    let room = controls - view.at(10.0) - x - view.at(34.0);
    if room > view.at(24.0) {
        let color = if mark == Mark::Attention {
            p.attention
        } else {
            p.secondary
        };
        let galley = elided(&painter, text, sans(view.at(12.5)), color, room);
        let size = galley.size();
        let at = pos2(controls - view.at(10.0) - size.x, y - size.y / 2.0);
        let shown = Rect::from_min_size(at, size);
        painter.galley_with_override_text_color(at, galley, color);
        let center = pos2(at.x - view.at(12.0), y);
        match mark {
            Mark::Running => theme::running(&painter, center, view.k, p),
            Mark::Attention => theme::attention(&painter, center, view.k * 0.8, p),
            Mark::None => {}
        }
        ui.interact(shown, ui.id().with("angelica-bar-status"), Sense::hover())
            .on_hover_text(text);
    }
}

/// Draws a window's title bar and returns what the player pressed in it.
pub(crate) fn bar(ui: &mut Ui, view: &Look, bar: &Bar<'_>) -> Pressed {
    let p = view.p;
    let s = bar.strings;
    let mut pressed = Pressed::default();
    let collapsed = bar.collapsed == Some(true);
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), view.at(32.0)), Sense::hover());
    let painter = ui.painter().clone();
    let below = if collapsed { 6 } else { 0 };
    painter.rect_filled(
        rect,
        CornerRadius {
            nw: 6,
            ne: 6,
            sw: below,
            se: below,
        },
        p.title,
    );
    if !collapsed {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, p.line),
        );
    }
    let y = rect.center().y;

    // The controls, from the right edge.
    let step = view.at(26.0);
    let mut right = rect.right() - view.at(18.0);
    if collapsed {
        pressed.toggle = bar_button(ui, view, pos2(right, y), Icon::Expand, s.bar_expand).clicked();
        right -= step;
    } else {
        pressed.close = bar_button(ui, view, pos2(right, y), Icon::Close, s.close).clicked();
        right -= step;
        if bar.menu {
            pressed.menu = Some(bar_button(ui, view, pos2(right, y), Icon::More, s.bar_menu));
            right -= step;
        }
        if bar.collapsed.is_some() {
            pressed.toggle =
                bar_button(ui, view, pos2(right, y), Icon::Collapse, s.bar_collapse).clicked();
            right -= step;
        }
    }
    let controls = right + step / 2.0;

    // Where the player is, from the left edge.
    let mut x = rect.left() + view.at(12.0);
    if bar.wordmark {
        let word = painter.text(
            pos2(x, y),
            Align2::LEFT_CENTER,
            "angelica",
            wordmark(view.at(13.0)),
            p.secondary,
        );
        x = word.right();
    } else {
        x = bar_path(ui, view, bar, x, controls, y, &mut pressed);
    }

    // What goes on, before the controls.
    if let Some((text, mark)) = bar.status {
        bar_status(ui, view, text, mark, x, controls, y);
    }
    pressed
}

/// The bar of a dialog: its title and the close button. Returns whether
/// the player closed the window.
pub(crate) fn title_bar(ui: &mut Ui, view: &Look, title: &str, strings: &Strings) -> bool {
    bar(ui, view, &Bar::titled(title, strings)).close
}

/// Where the columns of a row are: the tile's center, where the name and
/// the permissions begin, where the version ends, and the state's center.
struct Columns {
    tile: f32,
    name: f32,
    permissions: f32,
    version: f32,
    state: f32,
}

impl Columns {
    fn of(row: Rect, view: &View<'_>) -> Self {
        let state = row.right() - view.at(24.0);
        let version = state - view.at(20.0);
        Self {
            tile: row.left() + view.at(24.0),
            name: row.left() + view.at(46.0),
            permissions: version - view.at(56.0) - view.at(80.0),
            version,
            state,
        }
    }
}

fn columns_header(ui: &mut Ui, view: &View<'_>, s: &Strings) {
    let p = view.p;
    let (row, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), view.at(18.0)), Sense::hover());
    let columns = Columns::of(row, view);
    let painter = ui.painter();
    let font = sans(view.at(11.0));
    painter.text(
        pos2(columns.name, row.center().y),
        Align2::LEFT_CENTER,
        s.column_plugin,
        font.clone(),
        p.muted,
    );
    painter.text(
        pos2(columns.permissions, row.center().y),
        Align2::LEFT_CENTER,
        s.column_permissions,
        font.clone(),
        p.muted,
    );
    painter.text(
        pos2(columns.version, row.center().y),
        Align2::RIGHT_CENTER,
        s.column_version,
        font,
        p.muted,
    );
}

/// The permissions and the version in a row.
fn row_facts(
    painter: &egui::Painter,
    view: &View<'_>,
    plugin: &Plugin,
    status: &Status,
    rect: Rect,
    columns: &Columns,
) {
    let p = view.p;
    let off = *status == Status::Disabled;
    let asked: Vec<Class> = if *status == Status::AwaitingApproval {
        plugin
            .requests()
            .into_iter()
            .map(|(class, _)| class)
            .collect()
    } else {
        Vec::new()
    };
    for (at, class) in plugin_classes(plugin, status).into_iter().enumerate() {
        let color = if asked.contains(&class) {
            p.attention
        } else if off {
            p.muted
        } else {
            p.secondary
        };
        let x = columns.permissions + view.at(20.0) * f32_of(at) + view.at(7.0);
        theme::icon(
            painter,
            class_icon(class),
            Rect::from_center_size(pos2(x, rect.center().y), vec2(view.at(14.0), view.at(14.0))),
            color,
        );
    }
    painter.text(
        pos2(columns.version, rect.center().y),
        Align2::RIGHT_CENTER,
        &plugin.manifest.plugin.version,
        sans(view.at(12.5)),
        p.muted,
    );
}

/// The classes a plugin holds, and for one waiting for approval, also
/// those it asks for.
fn plugin_classes(plugin: &Plugin, status: &Status) -> Vec<Class> {
    let mut permissions: Vec<Permission> = plugin.manifest.permissions().iter().copied().collect();
    if *status == Status::AwaitingApproval {
        permissions.extend(
            plugin
                .requests()
                .into_iter()
                .flat_map(|(_, requests)| requests.into_iter().map(|request| request.permission)),
        );
    }
    classes_of(permissions)
}

/// A search field: a plain field with a thin border and a magnifier.
fn search_field(ui: &mut Ui, view: &View<'_>, rect: Rect, text: &mut String, hint: &str, id: &str) {
    text_field(
        ui,
        &view.look(),
        rect,
        text,
        hint,
        Id::new(id),
        Some(Icon::Search),
    );
}

/// A plain text field with a thin border.
pub(crate) fn text_field(
    ui: &mut Ui,
    view: &Look,
    rect: Rect,
    text: &mut String,
    hint: &str,
    id: Id,
    icon: Option<Icon>,
) -> egui::Response {
    let p = view.p;
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(4), p.field);
    let focused = ui.ctx().memory(|memory| memory.has_focus(id));
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(
            1.0,
            if focused {
                theme::alpha(p.action, 0.6)
            } else {
                p.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    let mut inner = rect.shrink2(vec2(view.at(8.0), 0.0));
    if let Some(icon) = icon {
        theme::icon(
            &painter,
            icon,
            Rect::from_center_size(
                pos2(inner.left() + view.at(6.0), rect.center().y),
                vec2(view.at(12.0), view.at(12.0)),
            ),
            p.muted,
        );
        inner.min.x += view.at(18.0);
    }
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .id(id)
                    .font(sans(view.at(13.0)))
                    .text_color(p.text)
                    .hint_text(RichText::new(hint).font(sans(view.at(13.0))).color(p.muted))
                    .frame(egui::Frame::NONE)
                    .desired_width(inner.width()),
            )
        },
    )
    .inner
}

/// A line of a menu: full width, highlighted under the pointer. Returns
/// whether it was clicked; a click closes the menu.
fn menu_item(ui: &mut Ui, view: &View<'_>, icon: Option<Icon>, text: &str, color: Color32) -> bool {
    popup_item(ui, &view.look(), icon, text, color)
}

/// A line of a popup menu, also in a plugin's window.
pub(crate) fn popup_item(
    ui: &mut Ui,
    view: &Look,
    icon: Option<Icon>,
    text: &str,
    color: Color32,
) -> bool {
    let p = view.p;
    let (rect, response) = ui.allocate_exact_size(
        vec2(
            ui.available_width().clamp(view.at(200.0), view.at(280.0)),
            view.at(26.0),
        ),
        Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(3), p.selection);
    }
    let mut x = rect.left() + view.at(8.0);
    if let Some(icon) = icon {
        theme::icon(
            painter,
            icon,
            Rect::from_center_size(
                pos2(x + view.at(6.0), rect.center().y),
                vec2(view.at(13.0), view.at(13.0)),
            ),
            p.secondary,
        );
        x += view.at(20.0);
    }
    painter.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        sans(view.at(13.0)),
        color,
    );
    if response.clicked() {
        ui.close();
        true
    } else {
        false
    }
}

/// A permission in the details or the dialog: its class's icon and its words.
fn permission_line(ui: &mut Ui, view: &View<'_>, icon: Option<Icon>, text: &str, color: Color32) {
    let p = view.p;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(view.at(16.0), view.at(16.0)), Sense::hover());
        if let Some(icon) = icon {
            theme::icon(ui.painter(), icon, rect.shrink(view.at(1.0)), p.secondary);
        }
        ui.add_space(view.at(4.0));
        ui.add(egui::Label::new(RichText::new(text).font(sans(view.at(12.5))).color(color)).wrap());
    });
}

/// A setting: its name on the left, its value on the right.
fn setting(ui: &mut Ui, view: &View<'_>, name: &str, value: impl FnOnce(&mut Ui)) {
    let p = view.p;
    ui.horizontal(|ui| {
        ui.set_min_height(view.at(34.0));
        let (rect, _) = ui.allocate_exact_size(vec2(view.at(180.0), view.at(34.0)), Sense::hover());
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            name,
            sans(view.at(13.5)),
            p.text,
        );
        value(ui);
    });
}

/// A note over the game: the word `angelica`, a mark, one line, and how to
/// open Angelica.
fn note(ui: &mut Ui, view: &View<'_>, text: &str, tone: Tone, hint: &str) {
    let p = view.p;
    egui::Frame::new()
        .fill(p.window)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(6))
        .shadow(ui.style().visuals.popup_shadow)
        .inner_margin(egui::Margin::symmetric(
            num_u8(view.at(12.0)),
            num_u8(view.at(10.0)),
        ))
        .show(ui, |ui| {
            ui.set_max_width(view.at(320.0));
            ui.spacing_mut().item_spacing.y = view.at(5.0);
            ui.label(
                RichText::new("angelica")
                    .font(wordmark(view.at(12.0)))
                    .color(p.secondary),
            );
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(view.at(14.0), view.at(16.0)), Sense::hover());
                let mark = rect.center();
                match tone {
                    Tone::Good => theme::running(ui.painter(), mark, view.k, p),
                    Tone::Attention => theme::attention(ui.painter(), mark, view.k * 0.85, p),
                    Tone::Failed => {
                        ui.painter().circle_filled(mark, view.at(3.0), p.danger);
                    }
                }
                ui.add(
                    egui::Label::new(RichText::new(text).font(sans(view.at(13.0))).color(p.text))
                        .wrap(),
                );
            });
            ui.label(RichText::new(hint).font(sans(view.at(12.0))).color(p.muted));
        });
}

/// A plugin in the developers' details: name, id, version, state, folder,
/// and log.
fn developer_plugin(ui: &mut Ui, view: &View<'_>, s: &Strings, plugin: &Plugin) {
    let p = view.p;
    let manifest = &plugin.manifest;
    let id = manifest.id();
    let status = plugin.status();
    let state = match &status {
        Status::Running => s.status_running.to_owned(),
        Status::Disabled => s.status_off.to_owned(),
        Status::AwaitingApproval => s.status_waiting.to_owned(),
        Status::Faulted(reason) => fill(s.status_stopped, &[("reason", reason)]),
    };
    ui.add_space(view.at(4.0));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = view.at(8.0);
        ui.label(
            RichText::new(&manifest.plugin.name)
                .font(strong(view.at(13.0)))
                .color(p.text),
        );
        ui.label(
            RichText::new(format!("{id} {}", manifest.plugin.version))
                .font(mono(view.at(12.0)))
                .color(p.muted),
        );
        let color = if matches!(status, Status::Faulted(_)) {
            p.danger
        } else {
            p.secondary
        };
        ui.label(RichText::new(state).font(mono(view.at(12.0))).color(color));
        if status != Status::AwaitingApproval
            && theme::word(ui, s.open_folder, Word::Action, true, view.k * 0.92).clicked()
        {
            view.host.queue(Action::OpenFolder(id.to_owned()));
        }
    });
    let lines = plugin.log.lines();
    if lines.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(
        RichText::new(format!("{} {}", s.dev_log, lines.len()))
            .font(sans(view.at(12.5)))
            .color(p.action),
    )
    .id_salt(("angelica-log", id))
    .show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("angelica-log-lines", id))
            .max_height(view.at(180.0))
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in lines {
                    let color = match line.level {
                        Level::Error => p.danger,
                        Level::Warning => p.attention,
                        Level::Debug => p.muted,
                        Level::Info => p.text,
                    };
                    mono_line(ui, view, &line.text, color);
                }
            });
    });
}

/// A wrapped line of ids, paths, or logs.
fn mono_line(ui: &mut Ui, view: &View<'_>, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).font(mono(view.at(12.0))).color(color)).wrap());
}

/// One line of text cut with an ellipsis to `width`.
fn elided(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

/// `text` with its first letter capital.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// A fraction clamped to 0..=1.
fn fraction(value: f64) -> f32 {
    num_traits::cast::<f64, f32>(value.clamp(0.0, 1.0)).unwrap_or(0.0)
}

fn f32_of(value: usize) -> f32 {
    num_traits::cast(value).unwrap_or(0.0)
}

/// A margin in whole points.
pub(crate) fn num_u8(value: f32) -> i8 {
    num_traits::cast(value.round()).unwrap_or(i8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(name: &'a str, description: &'a str, group: Group) -> Entry<'a> {
        Entry {
            name,
            description,
            group,
            classes: Vec::new(),
        }
    }

    #[test]
    fn plugins_are_grouped_attention_first_and_sorted_by_name() {
        let entries = [
            entry("Timers", "Hunts, retainers, resets", Group::On),
            entry("tasks", "Dailies", Group::Off),
            entry("Items", "Where things are", Group::Attention),
            entry("Harmonia", "The game in another language", Group::On),
        ];
        assert_eq!(
            lines(&entries, "", None),
            vec![
                Line::Heading(Group::Attention, 1),
                Line::Plugin(2),
                Line::Heading(Group::On, 2),
                Line::Plugin(3),
                Line::Plugin(0),
                Line::Heading(Group::Off, 1),
                Line::Plugin(1),
            ]
        );
    }

    #[test]
    fn search_finds_names_and_descriptions_in_any_case() {
        let entries = [
            entry("Таймеры", "Охота, ретейнеры, сбросы", Group::On),
            entry("Items", "Where things are", Group::Attention),
            entry("Market", "Prices and your sales", Group::Off),
        ];
        assert_eq!(
            lines(&entries, "  ОХОТА ", None),
            vec![Line::Heading(Group::On, 1), Line::Plugin(0)]
        );
        assert_eq!(
            lines(&entries, "items", None),
            vec![Line::Heading(Group::Attention, 1), Line::Plugin(1)]
        );
        assert!(lines(&entries, "nothing like it", None).is_empty());
    }

    #[test]
    fn the_filter_keeps_plugins_with_a_permission_of_the_class() {
        let mut online = entry("Market", "Prices", Group::On);
        online.classes = vec![Class::Network, Class::WhatYouSee];
        let mut local = entry("Timers", "Resets", Group::On);
        local.classes = vec![Class::YoursOnly];
        let entries = [online, local];
        assert_eq!(
            lines(&entries, "", Some(Class::Network)),
            vec![Line::Heading(Group::On, 1), Line::Plugin(0)]
        );
        assert!(lines(&entries, "", Some(Class::OtherPlayers)).is_empty());
        assert_eq!(lines(&entries, "", None).len(), 3);
    }

    #[test]
    fn faulted_plugins_need_attention() {
        assert_eq!(Group::of(&Status::Faulted("trap".into())), Group::Attention);
        assert_eq!(Group::of(&Status::AwaitingApproval), Group::Attention);
        assert_eq!(Group::of(&Status::Running), Group::On);
        assert_eq!(Group::of(&Status::Disabled), Group::Off);
    }

    #[test]
    fn classes_come_once_in_their_order() {
        assert_eq!(
            classes_of([
                Permission::StateSelf,
                Permission::ExcelText,
                Permission::HttpGet,
                Permission::FilesReplace,
            ]),
            vec![Class::Network, Class::WhatYouSee, Class::YoursOnly]
        );
        for class in CLASSES {
            for other in CLASSES.iter().filter(|other| **other != class) {
                assert_ne!(class_icon(class), class_icon(*other));
            }
        }
    }
}
