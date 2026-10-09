//! The Angelica menu over the game: the player's plugins as a calm list in
//! groups, the chosen plugin's card with its face and how it works,
//! settings, and the details for developers. While the menu is closed, a
//! small living feather in the corner shows how Angelica is, and short
//! notes tell what the plugin installer did.
#![forbid(unsafe_code)]
use crate::{
    feather::{self, Mood, Place},
    host::{Action, Event, Host, Plugin, Status},
    language::{Language, Settings, Strings, fill},
    manifest::{Class, Origin, Permission},
    plugin_installer::{Failure, PluginInstaller, Report},
    sandbox::bindings::angelica::plugin::runtime::Level,
    theme::{self, Button, alpha, mono, sans, soft, soft_light, strong},
};
use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2,
    text::LayoutJob, vec2,
};
use std::{path::PathBuf, sync::Arc};

/// Where a plugin is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Waiting for the player: to approve it, or to retry it after a
    /// failure.
    Waiting,
    On,
    Off,
}

impl Group {
    #[must_use]
    pub fn of(status: &Status) -> Self {
        match status {
            Status::AwaitingApproval | Status::Faulted(_) => Self::Waiting,
            Status::Running => Self::On,
            Status::Disabled => Self::Off,
        }
    }

    fn title(self, s: &Strings) -> &'static str {
        match self {
            Self::Waiting => s.group_waiting,
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
}

/// A line of the list: a group's heading with how many it holds, or a
/// plugin by its index in the entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Heading(Group, usize),
    Plugin(usize),
}

/// The list's lines: the plugins whose name or description contains
/// `query`, in groups, each by name.
#[must_use]
pub fn lines(entries: &[Entry<'_>], query: &str) -> Vec<Line> {
    let query = query.trim().to_lowercase();
    let mut lines = Vec::new();
    for group in [Group::Waiting, Group::On, Group::Off] {
        let mut members: Vec<(String, usize)> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.group == group
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

/// How Angelica is: something went wrong, something waits for the player,
/// the installer works, or all is calm, in that order.
#[must_use]
pub fn mood<'a>(statuses: impl IntoIterator<Item = &'a Status>, working: bool) -> Mood {
    let mut waiting = false;
    for status in statuses {
        match status {
            Status::Faulted(_) => return Mood::Wrong,
            Status::AwaitingApproval => waiting = true,
            Status::Running | Status::Disabled => {}
        }
    }
    if waiting {
        Mood::NeedsYou
    } else if working {
        Mood::Working
    } else {
        Mood::Calm
    }
}

/// The color a class of permissions is marked with, the most serious
/// warmest.
#[must_use]
pub fn class_color(class: Class) -> Color32 {
    match class {
        Class::Network => theme::EMBER,
        Class::OtherPlayers => theme::LIGHT,
        Class::WhatYouSee => theme::BLUEPRINT_PALE,
        Class::YoursOnly => theme::ON,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Mine,
    Settings,
}

/// The card's side and its turn: the side it turns to and when the turn
/// began.
#[derive(Debug, Default, Clone, Copy)]
struct Flip {
    back: bool,
    since: Option<f64>,
}

/// Half a turn of the card, in seconds: squash, then open.
const HALF_TURN: f64 = 0.16;
/// How long a note stays.
const NOTE_TIME: f64 = 7.0;
/// How long the corner shows how to open Angelica after the game starts.
const HINT_TIME: f64 = 20.0;

struct Note {
    report: Report,
    at: f64,
}

/// The menu's state between frames.
pub struct Menu {
    language: Language,
    settings: Settings,
    settings_path: PathBuf,
    tab: Tab,
    query: String,
    link: String,
    link_invalid: bool,
    selected: Option<String>,
    flip: Flip,
    developers: bool,
    notes: Vec<Note>,
    seen: u64,
}

/// Sizes of the menu, in points, for a screen of this height.
#[derive(Clone, Copy)]
struct Scale(f32);

impl Scale {
    fn of(screen: Rect) -> Self {
        Self((screen.height() / 900.0).clamp(0.8, 1.6))
    }

    fn at(self, value: f32) -> f32 {
        value * self.0
    }
}

/// What one frame of the menu works with.
#[derive(Clone, Copy)]
struct View<'a> {
    host: &'a Host,
    plugins: &'a [Arc<Plugin>],
    statuses: &'a [Status],
    map_status: &'a [String],
    screen: Rect,
    k: Scale,
    time: f64,
    mood: Mood,
}

/// The parts of My plugins.
struct Layout {
    detail: Rect,
    find: Rect,
    list: Rect,
    link: Rect,
}

impl Layout {
    fn of(screen: Rect, k: Scale) -> Self {
        let left = screen.left() + k.at(64.0);
        let width = k.at(600.0).min(screen.width() * 0.42);
        let find = Rect::from_min_size(
            pos2(left, screen.top() + k.at(124.0)),
            vec2(width, k.at(44.0)),
        );
        let link = Rect::from_min_max(
            pos2(left, screen.bottom() - k.at(84.0)),
            pos2(left + width, screen.bottom() - k.at(40.0)),
        );
        Self {
            detail: Rect::from_min_max(
                pos2(left + width + k.at(116.0), screen.top() + k.at(124.0)),
                pos2(screen.right() - k.at(64.0), screen.bottom() - k.at(40.0)),
            ),
            find,
            list: Rect::from_min_max(
                pos2(left, find.bottom() + k.at(10.0)),
                pos2(left + width, link.top() - k.at(36.0)),
            ),
            link,
        }
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
            settings,
            settings_path,
            tab: Tab::Mine,
            query: String::new(),
            link: String::new(),
            link_invalid: false,
            selected: None,
            flip: Flip::default(),
            developers: false,
            notes: Vec::new(),
            seen: 0,
        }
    }

    fn strings(&self) -> &'static Strings {
        self.language.strings()
    }

    fn set_language(&mut self, host: &Host, language: Language) {
        self.language = language;
        self.settings.language = Some(language);
        if let Err(error) = self.settings.save(&self.settings_path) {
            host.diagnostic(format!("settings: {error}"));
        }
    }

    /// Draws the menu when `open`, or the sign in the corner, and the
    /// notes. Returns whether the menu stays open.
    pub fn draw(
        &mut self,
        host: &Host,
        context: &egui::Context,
        map_status: &[String],
        open: bool,
    ) -> bool {
        let plugins = host.plugins();
        let statuses: Vec<Status> = plugins.iter().map(|plugin| plugin.status()).collect();
        let installer = host.plugin_installer();
        let screen = context.content_rect();
        let view = View {
            host,
            plugins: &plugins,
            statuses: &statuses,
            map_status,
            screen,
            k: Scale::of(screen),
            time: context.input(|input| input.time),
            mood: mood(
                &statuses,
                installer.is_some_and(|installer| installer.busy()),
            ),
        };
        if let Some(installer) = installer {
            self.collect_notes(installer, view.time);
        }
        let keep = if open {
            self.draw_menu(context, &view)
        } else {
            self.draw_sign(context, &view);
            false
        };
        self.draw_notes(context, &view, open);
        keep
    }

    fn draw_menu(&mut self, context: &egui::Context, view: &View<'_>) -> bool {
        let screen = view.screen;
        let typing = context.memory(|memory| memory.focused().is_some());
        let mut keep = typing || !context.input(|input| input.key_pressed(egui::Key::Escape));
        egui::Area::new(Id::new("angelica-menu"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .constrain(false)
            .show(context, |ui| {
                ui.set_clip_rect(screen);
                let _backdrop = ui.allocate_rect(screen, Sense::click_and_drag());
                theme::gradient(
                    ui.painter(),
                    screen,
                    Color32::from_rgba_unmultiplied(11, 18, 32, 196),
                    Color32::from_rgba_unmultiplied(13, 19, 31, 224),
                );
                if !self.draw_header(ui, view) {
                    keep = false;
                }
                match self.tab {
                    Tab::Mine => {
                        if self.draw_mine(ui, view) {
                            keep = false;
                        }
                    }
                    Tab::Settings => self.draw_settings(ui, view),
                }
            });
        keep
    }

    /// The small living feather, the word, the tabs, the pulse, and the way
    /// back to the game. Returns false when the player goes back.
    fn draw_header(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let (screen, k) = (view.screen, view.k);
        let painter = ui.painter().clone();
        let margin = k.at(64.0);
        feather::living(
            &painter,
            Place::with_height(
                screen.left() + margin,
                screen.top() + k.at(18.0),
                k.at(74.0),
            ),
            view.time,
            view.mood,
            k.at(0.7),
        );
        let word = painter.text(
            pos2(
                screen.left() + margin + k.at(40.0),
                screen.top() + k.at(30.0),
            ),
            Align2::LEFT_TOP,
            "angelica",
            soft_light(k.at(42.0)),
            theme::PEARL,
        );
        let baseline = word.bottom() - k.at(9.0);
        let x = self.draw_tabs(ui, &painter, word.right() + k.at(44.0), baseline, k);
        let pulse = painter.layout_job(self.pulse(view.statuses, k));
        painter.galley(
            pos2(x + k.at(14.0), baseline - pulse.size().y),
            pulse,
            theme::SOFT_TEXT,
        );
        !self.back_to_game(ui, &painter, view)
    }

    /// The tabs from `x` on `baseline`; returns where they end.
    fn draw_tabs(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        mut x: f32,
        baseline: f32,
        k: Scale,
    ) -> f32 {
        let s = self.strings();
        for (tab, title) in [(Tab::Mine, s.tab_mine), (Tab::Settings, s.tab_settings)] {
            let active = self.tab == tab;
            let font = if active {
                strong(k.at(16.0))
            } else {
                sans(k.at(16.0))
            };
            let galley = painter.layout_no_wrap(title.to_owned(), font, theme::PEARL);
            let rect = Rect::from_min_size(pos2(x, baseline - galley.size().y), galley.size());
            let response = ui
                .interact(
                    rect.expand(k.at(6.0)),
                    Id::new(("angelica-tab", title)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let color = if active {
                theme::PEARL
            } else if response.hovered() {
                theme::SOFT_TEXT
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 120)
            };
            painter.galley_with_override_text_color(rect.min, galley, color);
            if active {
                let y = rect.bottom() + k.at(9.0);
                painter.line_segment(
                    [pos2(rect.left(), y), pos2(rect.right(), y)],
                    Stroke::new(k.at(1.5), theme::LIGHT),
                );
            }
            if response.clicked() && !active {
                self.tab = tab;
            }
            x = rect.right() + k.at(30.0);
        }
        x
    }

    /// "1 waiting for you · 14 running".
    fn pulse(&self, statuses: &[Status], k: Scale) -> LayoutJob {
        let waiting = statuses
            .iter()
            .filter(|status| Group::of(status) == Group::Waiting)
            .count();
        let running = statuses
            .iter()
            .filter(|status| **status == Status::Running)
            .count();
        let mut job = LayoutJob::default();
        let font = sans(k.at(13.5));
        if statuses.is_empty() {
            append(&mut job, self.strings().nothing_yet, font, theme::SOFT_TEXT);
            return job;
        }
        if waiting > 0 {
            append(
                &mut job,
                &self.language.waiting(waiting),
                strong(k.at(13.5)),
                theme::LIGHT_TEXT,
            );
            append(&mut job, " · ", font.clone(), theme::MUTED);
        }
        append(
            &mut job,
            &self.language.running(running),
            font,
            theme::SOFT_TEXT,
        );
        job
    }

    /// "Esc — back to the game" at the top right; returns whether the
    /// player clicked it.
    fn back_to_game(&self, ui: &mut Ui, painter: &egui::Painter, view: &View<'_>) -> bool {
        let k = view.k;
        let back = painter.layout_no_wrap(
            self.strings().back_to_game.to_owned(),
            sans(k.at(14.0)),
            theme::MUTED,
        );
        let key = painter.layout_no_wrap("Esc".to_owned(), mono(k.at(12.0)), theme::PEARL);
        let top = view.screen.top() + k.at(44.0);
        let back_rect = Rect::from_min_size(
            pos2(view.screen.right() - k.at(64.0) - back.size().x, top),
            back.size(),
        );
        let key_rect = Rect::from_center_size(
            pos2(
                back_rect.left() - k.at(16.0) - key.size().x / 2.0,
                back_rect.center().y,
            ),
            key.size() + vec2(k.at(12.0), k.at(4.0)),
        );
        let response = ui
            .interact(
                back_rect.union(key_rect).expand(k.at(6.0)),
                Id::new("angelica-back-to-game"),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let color = if response.hovered() {
            theme::PEARL
        } else {
            theme::MUTED
        };
        painter.rect_stroke(
            key_rect,
            CornerRadius::same(4),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 80)),
            egui::StrokeKind::Inside,
        );
        painter.galley(key_rect.center() - key.size() / 2.0, key, theme::PEARL);
        painter.galley_with_override_text_color(back_rect.min, back, color);
        response.clicked()
    }

    /// My plugins: the list on the left, the chosen plugin's card on the
    /// right, and the link field. Returns true when the player opened a
    /// plugin, which closes the menu.
    fn draw_mine(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let k = view.k;
        let layout = Layout::of(view.screen, k);
        let painter = ui.painter().clone();
        feather::blueprint(
            &painter,
            Place::with_height(
                layout.detail.right() - k.at(250.0),
                view.screen.top() + k.at(50.0),
                k.at(810.0),
            ),
            0.07,
        );
        painter.text(
            pos2(layout.detail.left(), view.screen.bottom() - k.at(16.0)),
            Align2::LEFT_BOTTOM,
            "wasm sandbox · signed packages · your permission",
            mono(k.at(11.5)),
            alpha(theme::BLUEPRINT_PALE, 0.5),
        );
        self.find_field(ui, view, layout.find);
        if let Some(installer) = view.host.plugin_installer() {
            self.draw_link(ui, installer, layout.link, k);
        }
        let entries: Vec<Entry<'_>> = view
            .plugins
            .iter()
            .zip(view.statuses)
            .map(|(plugin, status)| Entry {
                name: &plugin.manifest.plugin.name,
                description: &plugin.manifest.plugin.description,
                group: Group::of(status),
            })
            .collect();
        let lines = lines(&entries, &self.query);
        self.keep_a_choice(&lines, view.plugins);
        let row = self.draw_list(ui, view, layout.list, &lines);
        let chosen = self.selected.as_deref().and_then(|id| {
            view.plugins
                .iter()
                .position(|plugin| plugin.manifest.id() == id)
        });
        let Some(index) = chosen else {
            return false;
        };
        if let Some(row) = row {
            leader(&painter, row, &layout, k);
        }
        self.draw_card(ui, view, index, layout.detail, &lines)
    }

    /// Chooses the first plugin of the list when the chosen one is not in
    /// it, unless a search found nothing.
    fn keep_a_choice(&mut self, lines: &[Line], plugins: &[Arc<Plugin>]) {
        let ids = plugin_ids(lines, plugins);
        if self.selected.as_deref().is_none_or(|id| !ids.contains(&id)) {
            let first = ids.first().map(|id| (*id).to_owned());
            if first.is_some() || self.query.trim().is_empty() {
                self.select(first);
            }
        }
    }

    fn find_field(&mut self, ui: &mut Ui, view: &View<'_>, find: Rect) {
        let k = view.k;
        let painter = ui.painter().clone();
        let count = painter.layout_no_wrap(
            self.language.plugins(view.plugins.len()),
            sans(k.at(13.0)),
            theme::MUTED,
        );
        let count_width = count.size().x;
        painter.galley(
            pos2(
                find.right() - count_width,
                find.bottom() - k.at(12.0) - count.size().y,
            ),
            count,
            theme::MUTED,
        );
        let hint = self.strings().find;
        ui.scope_builder(UiBuilder::new().max_rect(find), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(Id::new("angelica-find"))
                    .font(soft_light(k.at(27.0)))
                    .text_color(theme::PEARL)
                    .hint_text(
                        RichText::new(hint)
                            .font(soft_light(k.at(27.0)))
                            .color(Color32::from_rgba_unmultiplied(255, 255, 255, 112)),
                    )
                    .frame(egui::Frame::NONE)
                    .desired_width(find.width() - count_width - k.at(20.0)),
            );
        });
        painter.line_segment(
            [find.left_bottom(), find.right_bottom()],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 56)),
        );
    }

    /// The list of plugins in `list`. Returns the chosen plugin's row when
    /// it is in view.
    fn draw_list(
        &mut self,
        ui: &mut Ui,
        view: &View<'_>,
        list: Rect,
        lines: &[Line],
    ) -> Option<Rect> {
        let k = view.k;
        let mut chosen = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            if lines.is_empty() {
                let s = self.strings();
                let text = if self.query.trim().is_empty() {
                    s.empty
                } else {
                    s.nothing_found
                };
                ui.add_space(k.at(16.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(text)
                            .font(soft(k.at(22.0)))
                            .color(theme::MUTED),
                    )
                    .wrap(),
                );
                return;
            }
            ui.spacing_mut().item_spacing.y = 0.0;
            let height = k.at(46.0);
            let output = egui::ScrollArea::vertical()
                .id_salt("angelica-mine")
                .auto_shrink([false, false])
                .max_height(list.height())
                .show_rows(ui, height, lines.len(), |ui, range| {
                    for line in &lines[range] {
                        match *line {
                            Line::Heading(group, count) => {
                                self.heading_row(ui, group, count, height, k);
                            }
                            Line::Plugin(index) => {
                                let rect = self.plugin_row(ui, view, index, height);
                                if self.selected.as_deref()
                                    == Some(view.plugins[index].manifest.id())
                                {
                                    chosen = Some(rect);
                                }
                            }
                        }
                    }
                });
            let visible = output.inner_rect;
            chosen =
                chosen.filter(|row| (visible.top()..=visible.bottom()).contains(&row.center().y));
        });
        chosen
    }

    fn select(&mut self, id: Option<String>) {
        if self.selected != id {
            self.selected = id;
            self.flip = Flip::default();
        }
    }

    fn heading_row(&self, ui: &mut Ui, group: Group, count: usize, height: f32, k: Scale) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        let (color, number) = if group == Group::Waiting {
            (theme::LIGHT_TEXT, theme::LIGHT_TEXT)
        } else {
            (theme::FAINT, theme::SOFT_TEXT)
        };
        let mut job = LayoutJob::default();
        spaced(
            &mut job,
            &group.title(self.strings()).to_uppercase(),
            mono(k.at(11.5)),
            color,
            k.at(1.6),
        );
        spaced(
            &mut job,
            &format!("   {count}"),
            mono(k.at(11.5)),
            number,
            k.at(1.6),
        );
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            pos2(
                rect.left() + k.at(4.0),
                rect.bottom() - k.at(9.0) - galley.size().y,
            ),
            galley,
            color,
        );
    }

    /// A plugin's row: its light, its name and phrase, and its switch or
    /// what it waits for. Returns the row.
    fn plugin_row(&mut self, ui: &mut Ui, view: &View<'_>, index: usize, height: f32) -> Rect {
        let k = view.k;
        let plugin = &view.plugins[index];
        let status = &view.statuses[index];
        let id = plugin.manifest.id();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter().clone();
        if self.selected.as_deref() == Some(id) {
            let mut mesh = egui::Mesh::default();
            let warm = alpha(theme::LIGHT, 0.15);
            let fade = alpha(theme::LIGHT, 0.02);
            mesh.colored_vertex(rect.left_top(), warm);
            mesh.colored_vertex(rect.right_top(), fade);
            mesh.colored_vertex(rect.right_bottom(), fade);
            mesh.colored_vertex(rect.left_bottom(), warm);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            painter.add(mesh);
        } else if response.hovered() {
            painter.rect_filled(
                rect,
                CornerRadius::same(8),
                Color32::from_rgba_unmultiplied(255, 255, 255, 8),
            );
        }
        state_dot(
            &painter,
            status,
            pos2(rect.left() + k.at(12.0), rect.center().y),
            k,
        );
        let side = self.row_side(ui, &painter, view, index, rect);
        let name_left = rect.left() + k.at(34.0);
        let name = elided(
            &painter,
            &plugin.manifest.plugin.name,
            soft(k.at(25.0)),
            theme::PEARL,
            side - k.at(14.0) - name_left,
        );
        let name_rect = Rect::from_min_size(
            pos2(name_left, rect.center().y - name.size().y / 2.0),
            name.size(),
        );
        painter.galley(name_rect.min, name, theme::PEARL);
        let room = side - k.at(14.0) - (name_rect.right() + k.at(12.0));
        if room > k.at(30.0) {
            let phrase = elided(
                &painter,
                &plugin.manifest.plugin.description,
                sans(k.at(14.0)),
                theme::MUTED,
                room,
            );
            painter.galley(
                pos2(
                    name_rect.right() + k.at(12.0),
                    rect.center().y - phrase.size().y / 2.0 + k.at(2.0),
                ),
                phrase,
                theme::MUTED,
            );
        }
        if response.clicked() {
            self.select(Some(id.to_owned()));
        }
        rect
    }

    /// The row's right side: what waits for the player, or the switch.
    /// Returns where it begins.
    fn row_side(
        &self,
        ui: &mut Ui,
        painter: &egui::Painter,
        view: &View<'_>,
        index: usize,
        rect: Rect,
    ) -> f32 {
        let s = self.strings();
        let (plugin, status, k) = (&view.plugins[index], &view.statuses[index], view.k);
        let right = rect.right() - k.at(12.0);
        let center = pos2(right, rect.center().y);
        match status {
            Status::AwaitingApproval => painter
                .text(
                    center,
                    Align2::RIGHT_CENTER,
                    s.row_review,
                    strong(k.at(13.0)),
                    theme::LIGHT_TEXT,
                )
                .left(),
            Status::Faulted(_) => painter
                .text(
                    center,
                    Align2::RIGHT_CENTER,
                    s.row_stopped,
                    sans(k.at(13.0)),
                    theme::EMBER,
                )
                .left(),
            Status::Running | Status::Disabled => {
                let id = plugin.manifest.id();
                let on = *status == Status::Running;
                let switch = Rect::from_center_size(
                    pos2(right - k.at(17.0), rect.center().y),
                    vec2(k.at(34.0), k.at(20.0)),
                );
                let toggle = ui
                    .interact(switch, Id::new(("angelica-row-switch", id)), Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let progress = ui.ctx().animate_bool(toggle.id, on);
                theme::paint_switch(painter, switch, progress, toggle.hovered());
                if toggle.clicked() {
                    view.host.queue(switched(id, on));
                }
                switch.left()
            }
        }
    }

    /// The field for a plugin's link, its button, and what is wrong with
    /// the link.
    fn draw_link(&mut self, ui: &mut Ui, installer: &PluginInstaller, rect: Rect, k: Scale) {
        let s = self.strings();
        let painter = ui.painter().clone();
        let busy = installer.busy();
        let label = if busy { s.adding } else { s.add };
        let field_id = Id::new("angelica-link");
        let button_width = painter
            .layout_no_wrap(label.to_owned(), strong(k.at(14.5)), theme::PEARL)
            .size()
            .x
            + k.at(40.0);
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = k.at(14.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.link)
                        .id(field_id)
                        .font(sans(k.at(15.0)))
                        .text_color(theme::PEARL)
                        .hint_text(
                            RichText::new(s.link_hint)
                                .font(soft(k.at(21.0)))
                                .color(Color32::from_rgba_unmultiplied(255, 255, 255, 112)),
                        )
                        .frame(egui::Frame::NONE)
                        .desired_width(rect.width() - button_width - k.at(14.0)),
                );
                let y = field.rect.bottom() + k.at(4.0);
                painter.line_segment(
                    [pos2(field.rect.left(), y), pos2(field.rect.right(), y)],
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 56)),
                );
                if field.changed() {
                    self.link_invalid = false;
                }
                let submit =
                    field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let enabled = !busy && !self.link.trim().is_empty();
                let clicked = theme::button(ui, label, Button::Ghost, enabled, k.0).clicked();
                if enabled && (clicked || submit) {
                    match installer.install(&self.link) {
                        Ok(()) => self.link.clear(),
                        Err(_) => self.link_invalid = true,
                    }
                }
            });
        });
        let focused = ui.ctx().memory(|memory| memory.has_focus(field_id));
        let note = if self.link_invalid {
            Some((s.link_invalid, theme::EMBER))
        } else if focused || !self.link.is_empty() {
            Some((s.link_note, theme::MUTED))
        } else {
            None
        };
        if let Some((text, color)) = note {
            painter.text(
                pos2(rect.left(), rect.top() - k.at(6.0)),
                Align2::LEFT_BOTTOM,
                text,
                sans(k.at(12.5)),
                color,
            );
        }
    }

    /// The side the card shows and how open it is, from 0 to 1, while it
    /// turns: squash, change the side, open.
    fn turn_state(&mut self, time: f64) -> (bool, f32) {
        let (back, open) = match self.flip.since {
            Some(since) => {
                let phase = (time - since) / HALF_TURN;
                if phase < 1.0 {
                    (!self.flip.back, 1.0 - phase)
                } else if phase < 2.0 {
                    (self.flip.back, phase - 1.0)
                } else {
                    self.flip.since = None;
                    (self.flip.back, 1.0)
                }
            }
            None => (self.flip.back, 1.0),
        };
        let open = num_traits::cast::<f64, f32>(open).unwrap_or(1.0);
        (back, open * open * (3.0 - 2.0 * open))
    }

    /// The chosen plugin's card: its face, or how it works on the back.
    /// Returns true when the player opened the plugin.
    fn draw_card(
        &mut self,
        ui: &mut Ui,
        view: &View<'_>,
        index: usize,
        area: Rect,
        lines: &[Line],
    ) -> bool {
        let k = view.k;
        let plugin = &view.plugins[index];
        let (back, open) = self.turn_state(view.time);
        let turning = self.flip.since.is_some();
        let padding = vec2(k.at(44.0), k.at(36.0));
        let background = ui.painter().add(egui::Shape::Noop);
        let squashed = Rect::from_center_size(
            area.center(),
            vec2(area.width() * open.max(0.001), area.height()),
        );
        let mut content = ui.new_child(
            UiBuilder::new()
                .max_rect(area.shrink2(padding))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        content.set_clip_rect(squashed.intersect(ui.clip_rect()));
        content.multiply_opacity(open);
        content.spacing_mut().item_spacing = vec2(k.at(12.0), k.at(16.0));
        let (opened, turn) = if back {
            (false, self.card_back(&mut content, view.host, plugin, k))
        } else {
            let face = self.card_face(&mut content, view.host, plugin, &view.statuses[index], k);
            if face.not_now {
                self.select_next(lines, view.plugins);
            }
            (face.opened, face.turn)
        };
        let bottom = (content.min_rect().bottom() + padding.y).min(area.bottom());
        let card = Rect::from_min_max(area.min, pos2(area.right(), bottom));
        let shown = Rect::from_center_size(
            card.center(),
            vec2(card.width() * open.max(0.001), card.height()),
        );
        ui.painter()
            .set(background, card_background(shown, back, k));
        if turn && !turning {
            self.flip = Flip {
                back: !self.flip.back,
                since: Some(view.time),
            };
        }
        if opened {
            view.host
                .queue_event(plugin.manifest.id(), Event::OpenRequested);
        }
        opened
    }

    /// Moves the choice to the next plugin in the list.
    fn select_next(&mut self, lines: &[Line], plugins: &[Arc<Plugin>]) {
        let ids = plugin_ids(lines, plugins);
        let at = ids
            .iter()
            .position(|id| Some(*id) == self.selected.as_deref());
        if let Some(at) = at
            && ids.len() > 1
        {
            self.select(Some(ids[(at + 1) % ids.len()].to_owned()));
        }
    }

    /// The face: where the plugin comes from, its name and phrase, and what
    /// the player can do.
    fn card_face(
        &self,
        ui: &mut Ui,
        host: &Host,
        plugin: &Plugin,
        status: &Status,
        k: Scale,
    ) -> Face {
        let s = self.strings();
        let manifest = &plugin.manifest;
        let over = match manifest.origin {
            Origin::Catalog => s.from_catalog.to_owned(),
            Origin::Author if manifest.plugin.authors.is_empty() => s.from_author.to_owned(),
            Origin::Author => format!(
                "{} · {}",
                s.from_author,
                manifest.plugin.authors.join(", ").to_uppercase()
            ),
        };
        let mut job = LayoutJob::default();
        spaced(&mut job, &over, mono(k.at(11.5)), theme::LIGHT, k.at(1.4));
        ui.label(job);
        ui.add(
            egui::Label::new(
                RichText::new(&manifest.plugin.name)
                    .font(soft(k.at(64.0)))
                    .color(theme::PEARL),
            )
            .wrap(),
        );
        ui.scope(|ui| {
            ui.set_max_width(k.at(560.0));
            ui.add(
                egui::Label::new(
                    RichText::new(&manifest.plugin.description)
                        .font(sans(k.at(17.0)))
                        .color(theme::SOFT_TEXT),
                )
                .wrap(),
            );
        });
        let mut face = Face::default();
        match status {
            Status::AwaitingApproval => face.not_now = self.approval(ui, host, plugin, k),
            Status::Running | Status::Disabled => {
                face.opened = self.controls(ui, host, plugin, status, k);
            }
            Status::Faulted(_) => {
                let id = manifest.id();
                rule(ui, k);
                ui.label(
                    RichText::new(s.stopped)
                        .font(sans(k.at(15.0)))
                        .color(theme::SOFT_TEXT),
                );
                ui.horizontal(|ui| {
                    if theme::button(ui, s.retry, Button::Main, true, k.0).clicked() {
                        host.queue(Action::Enable(id.to_owned()));
                    }
                    if theme::button(ui, s.keep_off, Button::Ghost, true, k.0).clicked() {
                        host.queue(Action::Disable(id.to_owned()));
                    }
                });
            }
        }
        face.turn = flip_link(ui, s.how, true, k);
        face
    }

    /// What a plugin waiting for approval asks, by class, and the approval.
    /// Returns whether the player put it off.
    fn approval(&self, ui: &mut Ui, host: &Host, plugin: &Plugin, k: Scale) -> bool {
        let s = self.strings();
        rule(ui, k);
        let asks = if plugin.approved_before() {
            s.asks_more
        } else {
            s.asks
        };
        ui.label(
            RichText::new(asks)
                .font(sans(k.at(13.5)))
                .color(theme::MUTED),
        );
        let requests = plugin.requests();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = k.at(6.0);
            if requests.is_empty() {
                ask(ui, s.asks_nothing, theme::ON, k);
            }
            for (class, requests) in requests {
                ui.add_space(k.at(4.0));
                ui.label(
                    RichText::new(self.language.class(class))
                        .font(strong(k.at(12.5)))
                        .color(class_color(class)),
                );
                for request in requests {
                    ask(
                        ui,
                        self.language.permission(request.permission),
                        class_color(class),
                        k,
                    );
                    if !request.hosts.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(k.at(24.0));
                            ui.label(
                                RichText::new(request.hosts.join("  "))
                                    .font(mono(k.at(12.5)))
                                    .color(theme::BLUEPRINT_PALE),
                            );
                        });
                    }
                }
            }
        });
        let mut not_now = false;
        ui.horizontal(|ui| {
            if theme::button(ui, s.allow, Button::Main, true, k.0).clicked() {
                host.queue(Action::Approve(plugin.manifest.id().to_owned()));
            }
            not_now = theme::button(ui, s.not_now, Button::Quiet, true, k.0).clicked();
        });
        not_now
    }

    /// Open, the switch, and the version of an approved plugin. Returns
    /// whether the player opened it.
    fn controls(
        &self,
        ui: &mut Ui,
        host: &Host,
        plugin: &Plugin,
        status: &Status,
        k: Scale,
    ) -> bool {
        let s = self.strings();
        let manifest = &plugin.manifest;
        let on = *status == Status::Running;
        let mut opened = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = k.at(16.0);
            opened = theme::button(ui, s.open, Button::Main, on, k.0).clicked();
            if theme::switch(ui, on, k.0 * 1.2).clicked() {
                host.queue(switched(manifest.id(), on));
            }
        });
        let mut facts = LayoutJob::default();
        append(
            &mut facts,
            &format!("{} ", s.version),
            sans(k.at(14.0)),
            theme::MUTED,
        );
        append(
            &mut facts,
            &manifest.plugin.version,
            strong(k.at(14.0)),
            theme::TEXT,
        );
        let source = match manifest.origin {
            Origin::Catalog => s.source_catalog,
            Origin::Author => s.source_author,
        };
        append(
            &mut facts,
            &format!("   ·   {source}"),
            sans(k.at(14.0)),
            theme::MUTED,
        );
        ui.label(facts);
        opened
    }

    /// The back: the mechanism in the blueprint's mono. Returns whether the
    /// player turned the card back.
    fn card_back(&self, ui: &mut Ui, host: &Host, plugin: &Plugin, k: Scale) -> bool {
        let s = self.strings();
        let manifest = &plugin.manifest;
        let signed = match manifest.origin {
            Origin::Catalog => "Angelica · ed25519".to_owned(),
            Origin::Author if manifest.plugin.authors.is_empty() => {
                format!("{} · ed25519", s.source_author)
            }
            Origin::Author => format!("{} · ed25519", manifest.plugin.authors.join(", ")),
        };
        let rows = [
            (s.back_does, self.does(plugin), theme::TEXT),
            (s.back_cannot, self.cannot(plugin), theme::TEXT),
            (
                s.back_sandbox,
                format!("wasm · {} MiB", host.memory_bytes(plugin) / (1024 * 1024)),
                theme::TEXT,
            ),
            (
                s.back_frame,
                format!("≤ {} ms", host.frame_time().as_millis()),
                theme::TEXT,
            ),
            (s.back_signed, signed, theme::ON),
            (s.version, manifest.plugin.version.clone(), theme::TEXT),
            (s.back_id, manifest.id().to_owned(), theme::MUTED),
        ];
        ui.label(
            RichText::new(&manifest.plugin.name)
                .font(soft(k.at(40.0)))
                .color(theme::PEARL),
        );
        egui::Grid::new(("angelica-card-back", manifest.id()))
            .striped(false)
            .num_columns(2)
            .spacing(vec2(k.at(24.0), k.at(12.0)))
            .show(ui, |ui| {
                for (term, value, color) in rows {
                    ui.label(
                        RichText::new(term)
                            .font(mono(k.at(14.0)))
                            .color(theme::BLUEPRINT_PALE),
                    );
                    ui.add(
                        egui::Label::new(RichText::new(value).font(mono(k.at(14.0))).color(color))
                            .wrap(),
                    );
                    ui.end_row();
                }
            });
        flip_link(ui, s.front, false, k)
    }

    /// What the plugin may do, one permission a line.
    fn does(&self, plugin: &Plugin) -> String {
        let manifest = &plugin.manifest;
        let lines: Vec<String> = manifest
            .permissions()
            .iter()
            .map(|&permission| {
                let text = self.language.permission(permission);
                if permission == Permission::HttpGet {
                    let hosts: Vec<String> = manifest.http_hosts().into_iter().collect();
                    format!("{text}: {}", hosts.join(", "))
                } else {
                    text.to_owned()
                }
            })
            .collect();
        if lines.is_empty() {
            self.strings().back_own.to_owned()
        } else {
            lines.join("\n")
        }
    }

    /// What the plugin cannot do, one thing a line.
    fn cannot(&self, plugin: &Plugin) -> String {
        let s = self.strings();
        let permissions = plugin.manifest.permissions();
        let mut cannot = Vec::new();
        if !permissions.contains(&Permission::HttpGet) {
            cannot.push(s.cannot_online);
        }
        if !permissions.contains(&Permission::ChatRead) {
            cannot.push(s.cannot_chat);
        }
        if !permissions
            .iter()
            .any(|permission| permission.class() == Class::WhatYouSee)
        {
            cannot.push(s.cannot_look);
        }
        cannot.push(s.cannot_always);
        cannot.join("\n")
    }

    /// Settings: language, the key, updates, and the details for
    /// developers.
    fn draw_settings(&mut self, ui: &mut Ui, view: &View<'_>) {
        let (screen, k) = (view.screen, view.k);
        let s = self.strings();
        let painter = ui.painter().clone();
        let width = k.at(760.0).min(screen.width() - k.at(128.0));
        let left =
            (screen.center().x - width / 2.0 + k.at(80.0)).min(screen.right() - k.at(64.0) - width);
        feather::blueprint(
            &painter,
            Place::with_height(
                screen.left() + k.at(230.0),
                screen.top() + k.at(110.0),
                k.at(720.0),
            ),
            0.06,
        );
        let mut top = screen.top() + k.at(140.0);
        let rows = [
            (s.setting_language, s.setting_language_note),
            (s.setting_key, s.setting_key_note),
            (s.setting_updates, s.setting_updates_note),
            (s.setting_developers, s.setting_developers_note),
        ];
        for (index, (title, note)) in rows.into_iter().enumerate() {
            let row = Rect::from_min_size(pos2(left, top), vec2(width, k.at(80.0)));
            painter.text(
                pos2(row.left(), row.center().y - k.at(2.0)),
                Align2::LEFT_BOTTOM,
                title,
                strong(k.at(17.0)),
                theme::PEARL,
            );
            painter.text(
                pos2(row.left(), row.center().y + k.at(4.0)),
                Align2::LEFT_TOP,
                note,
                sans(k.at(14.5)),
                theme::MUTED,
            );
            painter.line_segment(
                [row.left_bottom(), row.right_bottom()],
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 22)),
            );
            let value = Rect::from_min_max(pos2(row.center().x, row.top()), row.max);
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(value)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| self.setting_value(ui, view.host, index, k),
            );
            top = row.bottom();
        }
        if self.developers {
            let area = Rect::from_min_max(
                pos2(left, top + k.at(20.0)),
                pos2(left + width, screen.bottom() - k.at(40.0)),
            );
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("angelica-developers")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.developers(ui, view));
            });
        }
    }

    /// The value of the settings row `index`.
    fn setting_value(&mut self, ui: &mut Ui, host: &Host, index: usize, k: Scale) {
        let s = self.strings();
        match index {
            0 => self.language_switch(ui, host, k),
            1 => {
                let key = ui.painter().layout_no_wrap(
                    "Insert".to_owned(),
                    mono(k.at(13.0)),
                    theme::PEARL,
                );
                let (rect, _) = ui
                    .allocate_exact_size(key.size() + vec2(k.at(24.0), k.at(10.0)), Sense::hover());
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(6),
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 80)),
                    egui::StrokeKind::Inside,
                );
                ui.painter()
                    .galley(rect.center() - key.size() / 2.0, key, theme::PEARL);
            }
            2 => {
                ui.label(
                    RichText::new(s.setting_updates_value)
                        .font(sans(k.at(14.5)))
                        .color(theme::SOFT_TEXT),
                );
            }
            _ => {
                let label = if self.developers { s.hide } else { s.show };
                if theme::button(ui, label, Button::Ghost, true, k.0 * 0.92).clicked() {
                    self.developers = !self.developers;
                }
            }
        }
    }

    fn language_switch(&mut self, ui: &mut Ui, host: &Host, k: Scale) {
        let painter = ui.painter().clone();
        let galleys: Vec<_> = Language::ALL
            .iter()
            .map(|language| {
                painter.layout_no_wrap(language.name().to_owned(), strong(k.at(14.0)), theme::PEARL)
            })
            .collect();
        let pad = vec2(k.at(16.0), k.at(7.0));
        let width: f32 = galleys
            .iter()
            .map(|galley| galley.size().x + 2.0 * pad.x)
            .sum();
        let height = galleys[0].size().y + 2.0 * pad.y;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let pill = CornerRadius::same(u8::MAX);
        painter.rect_stroke(
            rect,
            pill,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 48)),
            egui::StrokeKind::Inside,
        );
        let mut x = rect.left();
        for (language, galley) in Language::ALL.into_iter().zip(galleys) {
            let segment = Rect::from_min_size(
                pos2(x, rect.top()),
                vec2(galley.size().x + 2.0 * pad.x, height),
            );
            let response = ui
                .interact(
                    segment,
                    Id::new(("angelica-language", language.name())),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let chosen = language == self.language;
            if chosen {
                painter.rect_filled(segment, pill, theme::PEARL);
            }
            let color = if chosen {
                Color32::from_rgb(0x16, 0x20, 0x2f)
            } else if response.hovered() {
                theme::PEARL
            } else {
                theme::MUTED
            };
            painter.galley_with_override_text_color(
                segment.center() - galley.size() / 2.0,
                galley,
                color,
            );
            if response.clicked() && !chosen {
                self.set_language(host, language);
            }
            x = segment.right();
        }
    }

    /// The details for developers: the game map, each plugin with its id,
    /// state, folder, and log, the installer's reports, and diagnostics.
    fn developers(&self, ui: &mut Ui, view: &View<'_>) {
        let (host, k) = (view.host, view.k);
        let s = self.strings();
        ui.spacing_mut().item_spacing.y = k.at(6.0);
        section(ui, s.dev_map, k);
        for line in view.map_status {
            mono_line(ui, line, theme::SOFT_TEXT, k);
        }
        section(ui, s.dev_plugins, k);
        if view.plugins.is_empty() {
            mono_line(ui, s.dev_nothing, theme::FAINT, k);
        }
        for plugin in view.plugins {
            developer_plugin(ui, s, host, plugin, k);
        }
        section(ui, s.dev_installer, k);
        let reports = host
            .plugin_installer()
            .map(|installer| installer.messages())
            .unwrap_or_default();
        if reports.is_empty() {
            mono_line(ui, s.dev_nothing, theme::FAINT, k);
        }
        for (_, report) in reports.iter().rev() {
            let color = if matches!(report, Report::Failed(..)) {
                theme::EMBER
            } else {
                theme::SOFT_TEXT
            };
            mono_line(ui, &report.to_string(), color, k);
        }
        section(ui, s.dev_diagnostics, k);
        let diagnostics = host.diagnostics();
        if diagnostics.is_empty() {
            mono_line(ui, s.dev_nothing, theme::FAINT, k);
        }
        for line in diagnostics.iter().rev() {
            mono_line(ui, line, theme::SOFT_TEXT, k);
        }
    }

    /// The sign in the corner while the menu is closed: a tiny living
    /// feather, and for a while or when something needs the player, how to
    /// open Angelica. It takes no input.
    fn draw_sign(&self, context: &egui::Context, view: &View<'_>) {
        let painter = context.layer_painter(egui::LayerId::background());
        let center = pos2(view.screen.right() - 52.0, view.screen.bottom() - 54.0);
        theme::glow(
            &painter,
            center,
            44.0,
            Color32::from_rgba_unmultiplied(14, 20, 32, 150),
        );
        feather::living(
            &painter,
            Place::with_height(center.x, center.y - 33.0, 66.0),
            view.time,
            view.mood,
            0.6,
        );
        let fade = if matches!(view.mood, Mood::NeedsYou | Mood::Wrong) {
            1.0
        } else {
            num_traits::cast::<f64, f32>(((HINT_TIME - view.time) / 2.0).clamp(0.0, 1.0))
                .unwrap_or(0.0)
        };
        if fade > 0.0 {
            let galley = painter.layout_no_wrap(
                self.strings().hint.to_owned(),
                sans(13.0),
                theme::SOFT_TEXT,
            );
            let rect = Rect::from_min_size(
                pos2(center.x - 48.0 - galley.size().x - 24.0, center.y + 6.0),
                galley.size() + vec2(24.0, 12.0),
            );
            pill(&painter, rect, fade);
            painter.galley_with_override_text_color(
                rect.center() - galley.size() / 2.0,
                galley,
                alpha(theme::SOFT_TEXT, fade),
            );
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

    /// What a note says, and the color of its light.
    fn note_text(&self, host: &Host, report: &Report) -> (String, Color32) {
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
                theme::LIGHT,
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
                    if asks { theme::LIGHT } else { theme::ON },
                )
            }
            _ => (s.note_failed.to_owned(), theme::EMBER),
        }
    }

    /// The installer's news as short pills at the top right.
    fn draw_notes(&self, context: &egui::Context, view: &View<'_>, open: bool) {
        let k = view.k;
        let painter = context.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            Id::new("angelica-notes"),
        ));
        let right = view.screen.right() - if open { k.at(64.0) } else { 24.0 };
        let mut top = view.screen.top() + if open { k.at(92.0) } else { 24.0 };
        for note in self.notes.iter().rev() {
            let (text, color) = self.note_text(view.host, &note.report);
            let age = view.time - note.at;
            let fade =
                num_traits::cast::<f64, f32>((age / 0.25).min(NOTE_TIME - age).clamp(0.0, 1.0))
                    .unwrap_or(0.0);
            let galley = painter.layout_no_wrap(text, sans(13.5), theme::SOFT_TEXT);
            let size = galley.size() + vec2(44.0, 16.0);
            let rect = Rect::from_min_size(pos2(right - size.x, top), size);
            pill(&painter, rect, fade * 1.4);
            let dot = pos2(rect.left() + 18.0, rect.center().y);
            theme::glow(&painter, dot, 9.0, alpha(color, 0.5 * fade));
            painter.circle_filled(dot, 3.5, alpha(color, fade));
            painter.galley_with_override_text_color(
                pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0),
                galley,
                alpha(theme::TEXT, fade),
            );
            top = rect.bottom() + 8.0;
        }
    }
}

/// What the player did on the card's face.
#[derive(Default)]
struct Face {
    opened: bool,
    turn: bool,
    not_now: bool,
}

/// The action that turns a plugin off when `on`, or on.
fn switched(id: &str, on: bool) -> Action {
    if on {
        Action::Disable(id.to_owned())
    } else {
        Action::Enable(id.to_owned())
    }
}

/// The ids of the plugins in the list, in its order.
fn plugin_ids<'a>(lines: &[Line], plugins: &'a [Arc<Plugin>]) -> Vec<&'a str> {
    lines
        .iter()
        .filter_map(|line| match line {
            Line::Plugin(index) => Some(plugins[*index].manifest.id()),
            Line::Heading(..) => None,
        })
        .collect()
}

/// A plugin's light in its row.
fn state_dot(painter: &egui::Painter, status: &Status, dot: egui::Pos2, k: Scale) {
    let radius = k.at(4.5);
    match status {
        Status::Running => {
            painter.circle_filled(dot, radius, theme::ON);
        }
        Status::AwaitingApproval => {
            theme::glow(painter, dot, k.at(13.0), alpha(theme::LIGHT, 0.75));
            painter.circle_filled(dot, radius, theme::LIGHT);
        }
        Status::Faulted(_) => {
            painter.circle_stroke(dot, radius, Stroke::new(1.2, alpha(theme::EMBER, 0.8)));
        }
        Status::Disabled => {
            painter.circle_stroke(dot, radius, Stroke::new(1.0, theme::OFF));
        }
    }
}

/// The dashed leader from the chosen row's light to its card.
fn leader(painter: &egui::Painter, row: Rect, layout: &Layout, k: Scale) {
    let y = row.center().y;
    let start = layout.list.right();
    let anchor = pos2(layout.detail.left(), layout.detail.top() + k.at(66.0));
    let gap = anchor.x - start;
    painter.extend(egui::Shape::dashed_line(
        &[
            pos2(start + k.at(8.0), y),
            pos2(start + gap * 0.55, y),
            pos2(start + gap * 0.78, anchor.y),
            anchor,
        ],
        Stroke::new(1.0, alpha(theme::LIGHT_TEXT, 0.55)),
        k.at(2.0),
        k.at(5.0),
    ));
    painter.circle_filled(anchor, k.at(3.0), alpha(theme::LIGHT_TEXT, 0.9));
}

/// The card's shadow and side: the face in night glass, the back on the
/// blueprint grid.
fn card_background(shown: Rect, back: bool, k: Scale) -> egui::Shape {
    let corner = CornerRadius::same(12);
    let shadow = egui::Shadow {
        offset: [0, 30],
        blur: 80,
        spread: 0,
        color: Color32::from_black_alpha(130),
    };
    let mut shapes = vec![shadow.as_shape(shown, corner).into()];
    if back {
        shapes.push(egui::Shape::rect_filled(
            shown,
            corner,
            Color32::from_rgba_unmultiplied(13, 21, 37, 246),
        ));
        let grid = Stroke::new(0.6, alpha(theme::BLUEPRINT, 0.22));
        let step = k.at(22.0);
        let mut x = shown.left() + step;
        while x < shown.right() {
            shapes.push(egui::Shape::line_segment(
                [pos2(x, shown.top()), pos2(x, shown.bottom())],
                grid,
            ));
            x += step;
        }
        let mut y = shown.top() + step;
        while y < shown.bottom() {
            shapes.push(egui::Shape::line_segment(
                [pos2(shown.left(), y), pos2(shown.right(), y)],
                grid,
            ));
            y += step;
        }
        shapes.push(egui::Shape::rect_stroke(
            shown,
            corner,
            Stroke::new(1.0, alpha(theme::BLUEPRINT, 0.4)),
            egui::StrokeKind::Inside,
        ));
    } else {
        shapes.push(egui::Shape::rect_filled(
            shown,
            corner,
            Color32::from_rgba_unmultiplied(20, 27, 42, 238),
        ));
        shapes.push(egui::Shape::rect_stroke(
            shown,
            corner,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 31)),
            egui::StrokeKind::Inside,
        ));
    }
    egui::Shape::Vec(shapes)
}

/// A quiet pill behind a short text over the game.
fn pill(painter: &egui::Painter, rect: Rect, strength: f32) {
    let round = CornerRadius::same(u8::MAX);
    painter.rect_filled(
        rect,
        round,
        Color32::from_rgba_unmultiplied(14, 20, 32, unit(0.62 * strength)),
    );
    painter.rect_stroke(
        rect,
        round,
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, unit(0.1 * strength)),
        ),
        egui::StrokeKind::Inside,
    );
}

/// A plugin in the developers' details: name, id, version, state, folder,
/// and log.
fn developer_plugin(ui: &mut Ui, s: &Strings, host: &Host, plugin: &Plugin, k: Scale) {
    let manifest = &plugin.manifest;
    let id = manifest.id();
    let status = plugin.status();
    let state = match &status {
        Status::Running => s.status_running.to_owned(),
        Status::Disabled => s.status_off.to_owned(),
        Status::AwaitingApproval => s.status_waiting.to_owned(),
        Status::Faulted(reason) => fill(s.status_stopped, &[("reason", reason)]),
    };
    ui.add_space(k.at(4.0));
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(&manifest.plugin.name)
                .font(soft(k.at(20.0)))
                .color(theme::PEARL),
        );
        ui.label(
            RichText::new(format!("{id} {}", manifest.plugin.version))
                .font(mono(k.at(12.5)))
                .color(theme::FAINT),
        );
        let color = if matches!(status, Status::Faulted(_)) {
            theme::EMBER
        } else {
            theme::SOFT_TEXT
        };
        ui.label(RichText::new(state).font(mono(k.at(12.5))).color(color));
    });
    if status != Status::AwaitingApproval
        && theme::button(ui, s.dev_open_folder, Button::Ghost, true, k.0 * 0.8).clicked()
    {
        host.queue(Action::OpenFolder(id.to_owned()));
    }
    let lines = plugin.log.lines();
    if lines.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(
        RichText::new(format!("{} · {}", s.dev_log, lines.len()))
            .font(sans(k.at(13.5)))
            .color(theme::BLUEPRINT_PALE),
    )
    .id_salt(("angelica-log", id))
    .show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("angelica-log-lines", id))
            .max_height(k.at(200.0))
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in lines {
                    let color = match line.level {
                        Level::Error => theme::EMBER,
                        Level::Warning => theme::LIGHT_TEXT,
                        Level::Debug => theme::FAINT,
                        Level::Info => theme::TEXT,
                    };
                    mono_line(ui, &line.text, color, k);
                }
            });
    });
}

/// A wrapped line in the mechanism's mono.
fn mono_line(ui: &mut Ui, text: &str, color: Color32, k: Scale) {
    ui.add(egui::Label::new(RichText::new(text).font(mono(k.at(12.5))).color(color)).wrap());
}

/// A line of the face: the class's diamond and what the plugin asks.
fn ask(ui: &mut Ui, text: &str, color: Color32, k: Scale) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(k.at(14.0), k.at(20.0)), Sense::hover());
        theme::diamond(ui.painter(), rect.center(), k.at(4.5), color);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .font(sans(k.at(16.0)))
                    .color(theme::TEXT),
            )
            .wrap(),
        );
    });
}

/// A thin rule across the card.
fn rule(ui: &mut Ui, k: Scale) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), k.at(2.0)), Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 24)),
    );
}

/// The link that turns the card: "How it works ↻" on the face, "← Front"
/// on the back. Returns whether it was clicked.
fn flip_link(ui: &mut Ui, text: &str, face: bool, k: Scale) -> bool {
    let font = sans(k.at(14.5));
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, theme::BLUEPRINT_PALE);
    let icon = k.at(18.0);
    let size = galley.size() + vec2(icon, 0.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if response.hovered() {
        theme::PEARL
    } else {
        theme::BLUEPRINT_PALE
    };
    let painter = ui.painter();
    if face {
        painter.galley_with_override_text_color(rect.min, galley, color);
        theme::turn(
            painter,
            pos2(rect.right() - icon / 2.0 + k.at(2.0), rect.center().y),
            k.at(5.0),
            color,
        );
    } else {
        painter.text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            "←",
            sans(k.at(14.5)),
            color,
        );
        painter.galley_with_override_text_color(
            pos2(rect.left() + icon, rect.top()),
            galley,
            color,
        );
    }
    response.clicked()
}

/// A small title of the developers' details.
fn section(ui: &mut Ui, title: &str, k: Scale) {
    ui.add_space(k.at(14.0));
    let mut job = LayoutJob::default();
    spaced(
        &mut job,
        &title.to_uppercase(),
        mono(k.at(11.5)),
        theme::FAINT,
        k.at(1.6),
    );
    ui.label(job);
}

fn append(job: &mut LayoutJob, text: &str, font: FontId, color: Color32) {
    job.append(text, 0.0, egui::TextFormat::simple(font, color));
}

/// Text with space between its letters, as the mechanism's labels have.
fn spaced(job: &mut LayoutJob, text: &str, font: FontId, color: Color32, spacing: f32) {
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
}

/// One line of text cut with an ellipsis to `width`.
fn elided(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

fn unit(value: f32) -> u8 {
    num_traits::cast(value.clamp(0.0, 1.0) * 255.0).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(name: &'a str, description: &'a str, group: Group) -> Entry<'a> {
        Entry {
            name,
            description,
            group,
        }
    }

    #[test]
    fn plugins_are_grouped_waiting_first_and_sorted_by_name() {
        let entries = [
            entry("Timers", "Hunts, retainers, resets", Group::On),
            entry("tasks", "Dailies", Group::Off),
            entry("Items", "Where things are", Group::Waiting),
            entry("Harmonia", "The game in another language", Group::On),
        ];
        assert_eq!(
            lines(&entries, ""),
            vec![
                Line::Heading(Group::Waiting, 1),
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
            entry("Items", "Where things are", Group::Waiting),
            entry("Market", "Prices and your sales", Group::Off),
        ];
        assert_eq!(
            lines(&entries, "  ОХОТА "),
            vec![Line::Heading(Group::On, 1), Line::Plugin(0)]
        );
        assert_eq!(
            lines(&entries, "items"),
            vec![Line::Heading(Group::Waiting, 1), Line::Plugin(1)]
        );
        assert!(lines(&entries, "nothing like it").is_empty());
    }

    #[test]
    fn faulted_plugins_wait_for_the_player() {
        assert_eq!(Group::of(&Status::Faulted("trap".into())), Group::Waiting);
        assert_eq!(Group::of(&Status::AwaitingApproval), Group::Waiting);
        assert_eq!(Group::of(&Status::Running), Group::On);
        assert_eq!(Group::of(&Status::Disabled), Group::Off);
    }

    #[test]
    fn the_mood_puts_trouble_first() {
        let running = Status::Running;
        let waiting = Status::AwaitingApproval;
        let faulted = Status::Faulted("trap".into());
        assert_eq!(mood([&running], false), Mood::Calm);
        assert_eq!(mood([&running], true), Mood::Working);
        assert_eq!(mood([&running, &waiting], true), Mood::NeedsYou);
        assert_eq!(mood([&waiting, &faulted], true), Mood::Wrong);
        assert_eq!(mood([], false), Mood::Calm);
    }

    #[test]
    fn each_class_has_its_own_color() {
        let classes = [
            Class::Network,
            Class::OtherPlayers,
            Class::WhatYouSee,
            Class::YoursOnly,
        ];
        for (index, class) in classes.iter().enumerate() {
            for other in &classes[index + 1..] {
                assert_ne!(class_color(*class), class_color(*other));
            }
        }
        assert_eq!(class_color(Class::Network), theme::EMBER);
    }
}
