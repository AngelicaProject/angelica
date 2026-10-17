//! Plugins' places on the screen ([interface](../../../docs/architecture/ui.md#places-and-parts)):
//! windows that fit their content, resize from every edge, and snap;
//! panels the player arranges in the layout mode; popups. Where each window
//! and panel is, and the player's choices, are kept in
//! `state/_layout.json` across game starts.
#![forbid(unsafe_code)]
use crate::{
    host::{Action, Event, Host, Plugin, Status},
    language::Strings,
    menu::{self, Bar, Look, Mark},
    panel::{Drawer, Icons},
    theme,
};
use angelica_view::{
    Anchor, Backing, Dialog, Navigated, Panel, PopupAt, Role, Shown, Tone, Widget, Window,
};
use egui::{Align2, CornerRadius, Id, LayerId, Order, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// How long a window or a panel takes to open or close.
const OPEN: f64 = 0.14;
/// How long a window's height takes to follow its content.
const HEIGHT: f32 = 0.18;
/// How close an edge snaps, in points.
const SNAP: f32 = 8.0;

type Key = (String, String);
/// The key of Angelica's menu in `state/_layout.json`.
const MENU: &str = "angelica/menu";

/// What one frame of drawing works with.
pub(crate) struct Frame<'a> {
    pub(crate) host: &'a Host,
    pub(crate) context: &'a egui::Context,
    pub(crate) look: Look,
    pub(crate) strings: &'static Strings,
    pub(crate) inputs: &'a mut HashMap<(String, String, String), String>,
    pub(crate) icons: &'a mut Icons,
    pub(crate) now: f64,
    pub(crate) screen: Rect,
}

impl Frame<'_> {
    /// Draws a place's parts with `look`.
    fn parts(
        &mut self,
        ui: &mut egui::Ui,
        plugin: &str,
        place: &str,
        look: Look,
        widgets: &[Widget],
    ) {
        self.parts_filling(ui, plugin, place, look, widgets, false);
    }

    /// Draws a place's parts with `look`; with `fill`, a split takes the
    /// rest of the room and each side scrolls in it. Returns where a split
    /// starts and the height its taller side needs.
    fn parts_filling(
        &mut self,
        ui: &mut egui::Ui,
        plugin: &str,
        place: &str,
        look: Look,
        widgets: &[Widget],
        fill: bool,
    ) -> Option<(f32, f32)> {
        let mut drawer = Drawer {
            host: self.host,
            plugin,
            window: place,
            inputs: &mut *self.inputs,
            icons: &mut *self.icons,
            last: None,
            look,
            strings: self.strings,
            index: 0,
            faded: None,
            fill,
            needed: None,
        };
        let mut position = 0;
        drawer.widgets(ui, widgets, &mut position);
        drawer.needed
    }

    /// How far an opening that began at `opened` is, from 0 to 1.
    fn opening(&self, opened: f64) -> f32 {
        #[allow(clippy::cast_possible_truncation)]
        let opening = ((self.now - opened) / OPEN).clamp(0.0, 1.0) as f32;
        if opening < 1.0 {
            self.context.request_repaint();
        }
        opening
    }
}

/// Where a window is and how big, in screen points.
#[derive(Clone)]
struct WindowState {
    pos: Pos2,
    width: f32,
    /// The height the player gave it; `None` while it follows its content.
    height: Option<f32>,
    /// The height its content took last frame.
    content: f32,
    collapsed: bool,
    /// When it last opened.
    opened: f64,
    /// The edge the player is resizing it by: its height follows the
    /// pointer at once.
    resizing: Option<&'static str>,
    /// While the player drags it: where the pointer alone would put it,
    /// before it snaps.
    free: Option<Pos2>,
    /// The lines it snapped to, across and down, and when it last took one.
    snapped: [Option<f32>; 2],
    snapped_at: f64,
    /// The guides of its snap, and when the player let it go: they fade.
    guides: Vec<[Pos2; 2]>,
    released_at: f64,
    /// The dialog it asks and when it began asking.
    asked: Option<(String, f64)>,
}

/// What the player did to a window this frame.
#[derive(Default)]
struct Handled {
    pressed: menu::Pressed,
    moved: Vec2,
    resized: Option<(&'static str, Vec2)>,
    stopped: bool,
    measured: Option<f32>,
}

/// The player's choice of when a panel is shown.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Choice {
    Always,
    InCombat,
    OutOfCombat,
    Hidden,
}

#[derive(Clone, Copy, Default)]
struct PanelState {
    pos: Option<Pos2>,
    choice: Option<Choice>,
    opened: f64,
}

/// What `state/_layout.json` holds.
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    format: u32,
    #[serde(default)]
    windows: BTreeMap<String, SavedWindow>,
    #[serde(default)]
    panels: BTreeMap<String, SavedPanel>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct SavedWindow {
    x: f32,
    y: f32,
    width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    height: Option<f32>,
    #[serde(default)]
    collapsed: bool,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct SavedPanel {
    x: f32,
    y: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shown: Option<Choice>,
}

fn name(key: &Key) -> String {
    format!("{}/{}", key.0, key.1)
}

fn window_id(key: &Key) -> Id {
    Id::new(("angelica-plugin-window", &key.0, &key.1))
}

/// The places of every plugin on the screen.
pub(crate) struct Places {
    windows: HashMap<Key, WindowState>,
    panels: HashMap<Key, PanelState>,
    /// Windows drawn last frame with their content, to fade out the ones
    /// that went away.
    drawn: HashMap<Key, Window>,
    closing: Vec<(Key, Window, WindowState, f64)>,
    /// How many windows opened in this game session, for the step each
    /// next one opens at.
    opened: usize,
    saved: Saved,
    path: PathBuf,
    /// Whether the player is arranging panels.
    pub(crate) layout: bool,
    /// The panels drawn this frame, for popups beside them.
    panel_rects: HashMap<Key, Rect>,
    in_combat: InCombat,
}

impl Places {
    pub(crate) fn new(host: &Host) -> Self {
        let path = host.state_directory().join("_layout.json");
        let saved = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Saved>(&bytes).ok())
            .filter(|saved| saved.format == 1)
            .unwrap_or_default();
        Self {
            windows: HashMap::new(),
            panels: HashMap::new(),
            drawn: HashMap::new(),
            closing: Vec::new(),
            opened: 0,
            saved,
            path,
            layout: false,
            panel_rects: HashMap::new(),
            in_combat: InCombat::default(),
        }
    }

    /// Where the player left Angelica's menu and how large.
    pub(crate) fn menu_place(&self) -> Option<menu::MenuPlace> {
        let saved = self.saved.windows.get(MENU)?;
        Some(menu::MenuPlace {
            x: saved.x,
            y: saved.y,
            width: saved.width,
            height: saved.height?,
        })
    }

    /// Keeps where the player put Angelica's menu and how large.
    pub(crate) fn keep_menu(&mut self, f: &Frame<'_>, place: menu::MenuPlace) {
        self.saved.windows.insert(
            MENU.to_owned(),
            SavedWindow {
                x: place.x,
                y: place.y,
                width: place.width,
                height: Some(place.height),
                collapsed: false,
            },
        );
        self.save(f);
    }

    /// Writes the places through a temporary file and a rename, keeping
    /// those of installed plugins only.
    fn save(&mut self, f: &Frame<'_>) {
        let screen = f.screen;
        let at = |pos: Pos2| {
            (
                (pos.x - screen.left()) / screen.width().max(1.0),
                (pos.y - screen.top()) / screen.height().max(1.0),
            )
        };
        for (key, state) in &self.windows {
            let (x, y) = at(state.pos);
            self.saved.windows.insert(
                name(key),
                SavedWindow {
                    x,
                    y,
                    width: state.width / f.look.k,
                    height: state.height.map(|height| height / f.look.k),
                    collapsed: state.collapsed,
                },
            );
        }
        for (key, state) in &self.panels {
            if let Some(pos) = state.pos {
                let (x, y) = at(pos);
                let shown = state.choice;
                self.saved
                    .panels
                    .insert(name(key), SavedPanel { x, y, shown });
            }
        }
        let installed: HashSet<String> = f
            .host
            .plugins()
            .iter()
            .map(|plugin| plugin.manifest.id().to_owned())
            .collect();
        let keep = |key: &String| {
            let plugin = key.split('/').next().unwrap_or_default();
            plugin == "angelica" || installed.contains(plugin)
        };
        self.saved.windows.retain(|key, _| keep(key));
        self.saved.panels.retain(|key, _| keep(key));
        self.saved.format = 1;
        if let Ok(text) = serde_json::to_vec(&self.saved) {
            let temporary = self.path.with_extension("json.tmp");
            if std::fs::write(&temporary, text).is_ok() {
                let _ = std::fs::rename(&temporary, &self.path);
            }
        }
    }

    /// Draws every running plugin's panels, then its windows, then the
    /// popups. `raised` is the plugin whose windows come to the front;
    /// `notes_bottom` where Angelica's own notes end.
    pub(crate) fn draw(&mut self, f: &mut Frame<'_>, raised: Option<&str>, notes_bottom: f32) {
        let plugins: Vec<_> = f
            .host
            .plugins()
            .into_iter()
            .filter(|plugin| plugin.status() == Status::Running)
            .collect();
        let in_combat = self.in_combat.now(f.host);
        self.panel_rects.clear();
        for plugin in &plugins {
            for panel in plugin.panels() {
                self.panel(f, plugin, &panel, in_combat);
            }
        }
        if self.layout {
            self.layout_bar(f);
        }
        let mut shown = HashSet::new();
        for plugin in &plugins {
            for window in plugin.windows() {
                let key = (plugin.manifest.id().to_owned(), window.id.clone());
                shown.insert(key.clone());
                self.window(f, plugin, &window);
                if raised == Some(plugin.manifest.id()) {
                    f.context
                        .move_to_top(LayerId::new(Order::Middle, window_id(&key)));
                }
                self.drawn.insert(key, window);
            }
        }
        let gone: Vec<Key> = self
            .drawn
            .keys()
            .filter(|key| !shown.contains(*key))
            .cloned()
            .collect();
        for key in gone {
            if let (Some(window), Some(state)) = (self.drawn.remove(&key), self.windows.get(&key)) {
                self.closing.push((key, window, state.clone(), f.now));
            }
        }
        self.fade_closing(f);
        self.popups(f, notes_bottom);
    }

    // ------------------------------------------------------------ windows

    /// A window's place the first time it opens: where the player left
    /// it, or in the middle of the screen, each next one a step lower and
    /// to the right.
    fn first_state(&mut self, key: &Key, f: &Frame<'_>) -> WindowState {
        let (look, screen) = (f.look, f.screen);
        if let Some(saved) = self.saved.windows.get(&name(key)) {
            return WindowState {
                pos: pos2(
                    screen.left() + saved.x * screen.width(),
                    screen.top() + saved.y * screen.height(),
                ),
                width: (saved.width * look.k)
                    .clamp(look.at(240.0), screen.width().max(look.at(240.0))),
                height: saved.height.map(|height| {
                    (height * look.k).clamp(look.at(80.0), screen.height().max(look.at(80.0)))
                }),
                content: look.at(120.0),
                collapsed: saved.collapsed,
                opened: f.now,
                resizing: None,
                free: None,
                snapped: [None; 2],
                snapped_at: 0.0,
                guides: Vec::new(),
                released_at: 0.0,
                asked: None,
            };
        }
        let order = self.opened;
        self.opened += 1;
        let width = look.at(360.0);
        #[allow(clippy::cast_precision_loss)]
        let step = look.at(28.0) * (order % 8) as f32;
        WindowState {
            pos: pos2(
                screen.center().x - width / 2.0 + step,
                screen.top() + screen.height() * 0.16 + step,
            ),
            width,
            height: None,
            content: look.at(120.0),
            collapsed: false,
            opened: f.now,
            resizing: None,
            free: None,
            snapped: [None; 2],
            snapped_at: 0.0,
            guides: Vec::new(),
            released_at: 0.0,
            asked: None,
        }
    }

    fn window(&mut self, f: &mut Frame<'_>, plugin: &Plugin, window: &Window) {
        let id = plugin.manifest.id();
        let key = (id.to_owned(), window.id.clone());
        if !self.windows.contains_key(&key) {
            let state = self.first_state(&key, f);
            self.windows.insert(key.clone(), state);
        }
        if !self.drawn.contains_key(&key) {
            // Opening again after it went away.
            if let Some(state) = self.windows.get_mut(&key) {
                state.opened = f.now;
            }
            self.closing.retain(|(closing, ..)| closing != &key);
        }
        let look = f.look;
        let others: Vec<Rect> = self
            .windows
            .iter()
            .filter(|(other, _)| **other != key && self.drawn.contains_key(*other))
            .map(|(_, state)| {
                let height = if state.collapsed {
                    look.at(32.0) + 2.0
                } else {
                    state
                        .height
                        .unwrap_or(state.content)
                        .clamp(look.at(80.0), f.screen.height() * 0.7)
                };
                Rect::from_min_size(state.pos, vec2(state.width, height))
            })
            .collect();
        let Some(state) = self.windows.get(&key).cloned() else {
            return;
        };
        let target = if state.collapsed {
            look.at(32.0) + 2.0
        } else {
            state
                .height
                .unwrap_or(state.content)
                .clamp(look.at(80.0), f.screen.height() * 0.7)
        };
        let ease = if state.resizing.is_some() {
            0.0
        } else {
            HEIGHT
        };
        let height =
            f.context
                .animate_value_with_time(window_id(&key).with("height"), target, ease);
        let handled = window_area(f, &key, window, &state, height);
        if let Some(state) = self.windows.get_mut(&key) {
            let stopped = apply(state, &handled, &others, f, height);
            match &window.dialog {
                Some(dialog) => {
                    if state
                        .asked
                        .as_ref()
                        .is_none_or(|(asked, _)| *asked != dialog.id)
                    {
                        state.asked = Some((dialog.id.clone(), f.now));
                    }
                    let at = state.asked.as_ref().map_or(f.now, |(_, at)| *at);
                    let over = Rect::from_min_size(state.pos, vec2(state.width, height));
                    if dialog_area(f, &key, dialog, over, at).0 {
                        f.host.queue_event(id, Event::Closed(dialog.id.clone()));
                    }
                }
                None => state.asked = None,
            }
            if let Some(depth) = handled.pressed.navigate {
                let window = window.id.clone();
                f.host
                    .queue_event(id, Event::Navigated(Navigated { window, depth }));
            }
            if handled.pressed.close {
                f.host.queue_event(id, Event::Closed(window.id.clone()));
            }
            if stopped {
                self.save(f);
            }
        }
    }

    fn fade_closing(&mut self, f: &mut Frame<'_>) {
        let now = f.now;
        self.closing.retain(|(.., at)| now - at < OPEN);
        let look = f.look;
        for (key, window, state, at) in &self.closing {
            #[allow(clippy::cast_possible_truncation)]
            let left = 1.0 - ((now - at) / OPEN) as f32;
            f.context.request_repaint();
            let context = f.context;
            egui::Area::new(window_id(key).with("closing"))
                .order(Order::Middle)
                .interactable(false)
                .fixed_pos(state.pos + vec2(0.0, look.at(6.0) * (1.0 - left)))
                .show(context, |ui| {
                    ui.multiply_opacity(left);
                    menu::window_frame(context, look.p).show(ui, |ui| {
                        ui.set_width(state.width - 2.0);
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        let _ = menu::bar(ui, &look, &Bar::titled(&window.title, f.strings));
                        if !state.collapsed {
                            body_frame(look).show(ui, |ui| {
                                ui.set_max_height(state.height.unwrap_or(state.content));
                                f.parts(ui, &key.0, &key.1, look, &window.widgets);
                            });
                        }
                    });
                });
        }
    }

    // ------------------------------------------------------------- panels

    /// Where a panel first appears: at its anchor, 24 points from the
    /// edges, aside from panels already there.
    fn anchored(&self, anchor: Anchor, size: Vec2, f: &Frame<'_>) -> Pos2 {
        let (look, screen) = (f.look, f.screen);
        let margin = look.at(24.0);
        let (left, center_x, right) = (
            screen.left() + margin,
            screen.center().x - size.x / 2.0,
            screen.right() - margin - size.x,
        );
        let (top, center_y, bottom) = (
            screen.top() + margin,
            screen.center().y - size.y / 2.0,
            screen.bottom() - margin - size.y,
        );
        let (x, y) = match anchor {
            Anchor::TopLeft => (left, top),
            Anchor::Top => (center_x, top),
            Anchor::TopRight => (right, top),
            Anchor::Left => (left, center_y),
            Anchor::Center => (center_x, center_y),
            Anchor::Right => (right, center_y),
            Anchor::BottomLeft => (left, bottom),
            Anchor::Bottom => (center_x, bottom),
            Anchor::BottomRight => (right, bottom),
        };
        let mut at = pos2(x, y);
        let upward = matches!(
            anchor,
            Anchor::BottomLeft | Anchor::Bottom | Anchor::BottomRight
        );
        for _ in 0..8 {
            let rect = Rect::from_min_size(at, size);
            let Some(taken) = self
                .panel_rects
                .values()
                .find(|other| other.intersects(rect))
            else {
                break;
            };
            at.y = if upward {
                taken.top() - look.at(8.0) - size.y
            } else {
                taken.bottom() + look.at(8.0)
            };
        }
        at
    }

    fn panel(
        &mut self,
        f: &mut Frame<'_>,
        plugin: &Plugin,
        panel: &Panel,
        in_combat: Option<bool>,
    ) {
        let screen = f.screen;
        let key = (plugin.manifest.id().to_owned(), panel.id.clone());
        let saved = self.saved.panels.get(&name(&key)).copied();
        let now = f.now;
        let state = self
            .panels
            .entry(key.clone())
            .or_insert_with(|| PanelState {
                pos: saved.map(|saved| {
                    pos2(
                        screen.left() + saved.x * screen.width(),
                        screen.top() + saved.y * screen.height(),
                    )
                }),
                choice: saved.and_then(|saved| saved.shown),
                opened: now,
            });
        let choice = state.choice.unwrap_or(match panel.shown {
            Shown::Always => Choice::Always,
            Shown::InCombat => Choice::InCombat,
            Shown::OutOfCombat => Choice::OutOfCombat,
        });
        let visible = match choice {
            Choice::Always => true,
            Choice::Hidden => false,
            Choice::InCombat => in_combat.unwrap_or(true),
            Choice::OutOfCombat => !in_combat.unwrap_or(false),
        };
        if !visible && !self.layout {
            state.opened = now;
            return;
        }
        let (stored, opened) = (state.pos, state.opened);
        let area = Id::new(("angelica-panel", &key.0, &key.1));
        let size = f
            .context
            .memory(|memory| memory.area_rect(area))
            .map_or(vec2(f.look.at(200.0), f.look.at(60.0)), |rect| rect.size());
        let pos = stored.unwrap_or_else(|| self.anchored(panel.anchor, size, f));
        let opacity = f.opening(opened) * if visible { 1.0 } else { 0.45 };
        let response = panel_area(f, &key, panel, pos, opacity, self.layout);
        self.panel_rects.insert(key.clone(), response.rect);
        if !self.layout {
            return;
        }
        let picked = layout_outline(f, &response, panel, choice);
        let state = self.panels.get_mut(&key).expect("inserted above");
        state.pos = Some(if response.dragged() {
            pos + response.drag_delta()
        } else {
            pos
        });
        if let Some(option) = picked {
            state.choice = Some(option);
        }
        if response.drag_stopped() || picked.is_some() {
            self.save(f);
        }
    }

    /// The note at the top of the screen in the layout mode, with **Done**.
    fn layout_bar(&mut self, f: &mut Frame<'_>) {
        let (look, context, strings) = (f.look, f.context, f.strings);
        let mut done = context.input(|input| input.key_pressed(egui::Key::Escape));
        egui::Area::new(Id::new("angelica-layout"))
            .order(Order::Foreground)
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(pos2(f.screen.center().x, f.screen.top() + look.at(24.0)))
            .show(context, |ui| {
                menu::window_frame(context, look.p)
                    .inner_margin(margin(look, 14.0, 9.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = look.at(16.0);
                            ui.label(
                                egui::RichText::new(strings.arrange_hint)
                                    .font(theme::sans(look.at(13.5)))
                                    .color(look.p.text),
                            );
                            let word = theme::word(
                                ui,
                                strings.arrange_done,
                                theme::Word::Main,
                                true,
                                look.k,
                            );
                            done |= word.clicked();
                        });
                    });
            });
        if done {
            self.layout = false;
            self.save(f);
        }
    }

    // ------------------------------------------------------------- popups

    fn popups(&mut self, f: &mut Frame<'_>, notes_bottom: f32) {
        let now = std::time::Instant::now();
        let mut corner = notes_bottom;
        let shown: Vec<_> = f
            .host
            .services()
            .notifications
            .current(now)
            .into_iter()
            .filter(|shown| shown.popup.is_some())
            .collect();
        for (index, shown) in shown.into_iter().enumerate() {
            let Some(popup) = &shown.popup else { continue };
            let age = now.duration_since(shown.at).as_secs_f32();
            let fade = (age / 0.25)
                .min((shown.shown.as_secs_f32() - age) / 0.5)
                .clamp(0.0, 1.0);
            f.context.request_repaint();
            let beside = match &popup.at {
                PopupAt::Panel(panel) => self
                    .panel_rects
                    .get(&(shown.plugin.clone(), panel.clone()))
                    .copied(),
                _ => None,
            };
            let place = popup_place(f, &popup.at, beside, corner);
            let rect = popup_area(f, index, &shown.plugin, popup, place, fade);
            if popup.at != PopupAt::Center && beside.is_none() {
                corner = rect.bottom() + f.look.at(8.0);
            }
        }
    }
}

// ---------------------------------------------------------- window parts

fn margin(look: Look, x: f32, y: f32) -> egui::Margin {
    egui::Margin::symmetric(menu::num_u8(look.at(x)), menu::num_u8(look.at(y)))
}

fn body_frame(look: Look) -> egui::Frame {
    egui::Frame::new().inner_margin(margin(look, 16.0, 12.0))
}

/// Draws a window and returns what the player did to it.
fn window_area(
    f: &mut Frame<'_>,
    key: &Key,
    window: &Window,
    state: &WindowState,
    height: f32,
) -> Handled {
    let look = f.look;
    let opening = f.opening(state.opened);
    let status = window.status.as_ref().map(|status| {
        let mark = match status.tone {
            Tone::Good => Mark::Running,
            Tone::Warning | Tone::Bad => Mark::Attention,
            Tone::Normal | Tone::Muted => Mark::None,
        };
        (status.text.as_str(), mark)
    });
    let bar = Bar {
        wordmark: false,
        title: &window.title,
        path: &window.path,
        status,
        collapsed: Some(state.collapsed),
        menu: true,
        strings: f.strings,
    };
    let mut handled = Handled::default();
    let context = f.context;
    let width = state.width;
    let grab = grab_margin(look);
    egui::Area::new(window_id(key))
        .order(Order::Middle)
        .fixed_pos(state.pos - Vec2::splat(grab) + vec2(0.0, look.at(6.0) * (1.0 - opening)))
        .constrain(false)
        .show(context, |ui| {
            ui.multiply_opacity(opening);
            // Lifted while the player drags it: its shadow grows and deepens.
            let lift = context.animate_bool_with_time(
                window_id(key).with("lift"),
                state.free.is_some(),
                0.12,
            );
            let mut shadow = context.global_style().visuals.window_shadow;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                shadow.blur = shadow.blur.saturating_add(look.at(14.0 * lift) as u8);
                shadow.offset[1] = shadow.offset[1].saturating_add(look.at(6.0 * lift) as i8);
                shadow.color = shadow.color.gamma_multiply(1.0 + 0.8 * lift);
            }
            let frame = menu::window_frame(context, look.p)
                .shadow(shadow)
                .outer_margin(grab_outer(look));
            let shown = frame.show(ui, |ui| {
                ui.set_width(width - 2.0);
                ui.set_height(height - 2.0);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                // Registered before the bar's buttons, which come out on
                // top of it.
                let bar_rect = Rect::from_min_size(ui.cursor().min, vec2(width, look.at(32.0)));
                let drag = ui.interact(
                    bar_rect,
                    Id::new(("angelica-window-move", &key.0, &key.1)),
                    Sense::drag(),
                );
                if drag.dragged() {
                    handled.moved = drag.drag_delta();
                }
                handled.stopped |= drag.drag_stopped();
                handled.pressed = menu::bar(ui, &look, &bar);
                window_menu(f, &handled.pressed, &key.0);
                if !state.collapsed {
                    handled.measured = Some(window_body(ui, f, key, window));
                }
            });
            // The frame as drawn, so the edges the player takes are its own.
            let frame_rect = shown.response.rect.shrink(grab);
            if !state.collapsed {
                let (resized, stopped) = resize_edges(ui, window_id(key), frame_rect, look);
                handled.resized = resized;
                handled.stopped |= stopped;
            }
        });
    handled
}

/// Draws an example of the documentation's gallery at `at` with the code
/// that draws plugins' places, and returns the rectangle it took; `height`
/// carries a window's content height from one frame to the next.
#[cfg(test)]
pub(crate) fn draw_example(
    f: &mut Frame<'_>,
    name: &str,
    shows: &angelica_view::examples::Shows,
    at: Pos2,
    height: &mut f32,
) -> Rect {
    use angelica_view::examples::Shows;
    let key: Key = ("gallery".into(), name.into());
    let look = f.look;
    match shows {
        Shows::Parts(widgets) => {
            let context = f.context;
            egui::Area::new(Id::new(("gallery", name)))
                .order(Order::Middle)
                .fixed_pos(at)
                .show(context, |ui| {
                    menu::window_frame(context, look.p).show(ui, |ui| {
                        ui.set_width(look.at(340.0));
                        body_frame(look).show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(look.at(12.0), look.at(8.0));
                            f.parts(ui, &key.0, &key.1, look, widgets);
                        });
                    });
                })
                .response
                .rect
        }
        Shows::Window(window) => {
            let state = WindowState {
                pos: at,
                width: look.at(360.0),
                height: None,
                content: *height,
                collapsed: false,
                opened: 0.0,
                resizing: None,
                free: None,
                snapped: [None; 2],
                snapped_at: 0.0,
                guides: Vec::new(),
                released_at: 0.0,
                asked: None,
            };
            let tall = height.clamp(look.at(80.0), f.screen.height() * 0.7);
            let handled = window_area(f, &key, window, &state, tall);
            if let Some(measured) = handled.measured {
                *height = measured;
            }
            let rect = Rect::from_min_size(at, vec2(state.width, tall));
            match &window.dialog {
                Some(dialog) => rect.union(dialog_area(f, &key, dialog, rect, 0.0).1),
                None => rect,
            }
        }
        Shows::Panel(panel) => panel_area(f, &key, panel, at, 1.0, false).rect,
        Shows::Popup(popup) => popup_area(f, 0, "Timers", popup, (Align2::LEFT_TOP, at), 1.0),
    }
}

/// The menu under a window's menu button.
fn window_menu(f: &Frame<'_>, pressed: &menu::Pressed, plugin: &str) {
    let Some(button) = &pressed.menu else { return };
    let (look, strings, host) = (f.look, f.strings, f.host);
    egui::Popup::menu(button).show(|ui| {
        if menu::popup_item(ui, &look, None, strings.open_folder, look.p.text) {
            host.queue(Action::OpenFolder(plugin.to_owned()));
        }
        if menu::popup_item(ui, &look, None, strings.turn_off, look.p.text) {
            host.queue(Action::Disable(plugin.to_owned()));
        }
    });
}

/// A window's parts, scrolling when they do not fit; returns the height the
/// whole window needs for them.
fn window_body(ui: &mut egui::Ui, f: &mut Frame<'_>, key: &Key, window: &Window) -> f32 {
    let look = f.look;
    let asking = window.dialog.is_some();
    let mut measured = 0.0;
    let body = body_frame(look).show(ui, |ui| {
        // While the window asks, it waits for the answer: its parts take no
        // input, and the shade below shows it waits.
        if asking {
            ui.disable();
            ui.style_mut().visuals.disabled_alpha = 1.0;
        }
        // A split takes the rest of the window, and each of its sides
        // scrolls on its own.
        if window
            .widgets
            .iter()
            .any(|widget| matches!(widget, Widget::BeginSplit(_)))
        {
            let top = ui.cursor().min.y;
            ui.spacing_mut().item_spacing = vec2(look.at(12.0), look.at(8.0));
            let needed = f
                .parts_filling(ui, &key.0, &key.1, look, &window.widgets, true)
                .map_or(0.0, |(at, tall)| at - top + tall);
            measured = look.at(32.0) + look.at(12.0) * 2.0 + needed + 4.0;
            return;
        }
        // Content wider than the window scrolls sideways; the window keeps
        // the width the player gave it.
        let body = egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(look.at(12.0), look.at(8.0));
                f.parts(ui, &key.0, &key.1, look, &window.widgets);
            });
        measured = look.at(32.0) + look.at(12.0) * 2.0 + body.content_size.y + 4.0;
    });
    if asking {
        ui.painter().rect_filled(
            body.response.rect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 6,
                se: 6,
            },
            egui::Color32::from_black_alpha(64),
        );
    }
    measured
}

/// Draws the question a window asks over it, `over` being the window's
/// frame; returns whether the player closed it without an answer.
fn dialog_area(
    f: &mut Frame<'_>,
    key: &Key,
    dialog: &Dialog,
    over: Rect,
    asked: f64,
) -> (bool, Rect) {
    let (look, strings, context, screen) = (f.look, f.strings, f.context, f.screen);
    let opening = f.opening(asked);
    // Inset in its window, so it reads as a question over the window.
    let width = (over.width() - look.at(56.0))
        .clamp(look.at(240.0), look.at(360.0))
        .min(screen.width() - look.at(32.0));
    let x = (over.center().x - width / 2.0).clamp(
        screen.left() + look.at(16.0),
        (screen.right() - width - look.at(16.0)).max(screen.left()),
    );
    let y = (over.top() + look.at(32.0 + 28.0)).min(screen.bottom() - look.at(120.0));
    let mut closed = false;
    let area = egui::Area::new(window_id(key).with(("dialog", &dialog.id)))
        .order(Order::Foreground)
        .fixed_pos(pos2(x, y + look.at(6.0) * (1.0 - opening)))
        .constrain(false)
        .show(context, |ui| {
            ui.multiply_opacity(opening);
            let mut shadow = context.global_style().visuals.window_shadow;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                shadow.blur = shadow.blur.saturating_add(look.at(12.0) as u8);
                shadow.offset[1] = shadow.offset[1].saturating_add(look.at(6.0) as i8);
                shadow.color = shadow.color.gamma_multiply(1.6);
            }
            menu::window_frame(context, look.p)
                .shadow(shadow)
                .show(ui, |ui| {
                    ui.set_width(width - 2.0);
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    closed = menu::bar(ui, &look, &Bar::titled(&dialog.title, strings)).close;
                    egui::Frame::new()
                        .inner_margin(margin(look, 16.0, 14.0))
                        .show(ui, |ui| {
                            ui.set_width(width - 2.0 - look.at(32.0));
                            ui.spacing_mut().item_spacing = vec2(look.at(12.0), look.at(8.0));
                            f.parts(ui, &key.0, &dialog.id, look, &dialog.widgets);
                        });
                });
        });
    (closed, area.response.rect)
}

/// How far outside a window's frame the player can grab its edges.
pub(crate) fn grab_margin(look: Look) -> f32 {
    look.at(6.0).round()
}

/// The outer margin that keeps the grabbing band inside a window's area,
/// so the game does not receive the press that starts a resize.
pub(crate) fn grab_outer(look: Look) -> egui::Margin {
    egui::Margin::same(menu::num_u8(grab_margin(look)))
}

/// The edges and corners the player resizes a window by, a band across
/// the frame's line on every side: which one was dragged and by how much,
/// and whether a drag ended. The edge under the pointer is drawn in the
/// action color, so the player sees what they are about to take.
pub(crate) fn resize_edges(
    ui: &mut egui::Ui,
    id: Id,
    r: Rect,
    look: Look,
) -> (Option<(&'static str, Vec2)>, bool) {
    use egui::CursorIcon::{ResizeHorizontal, ResizeNeSw, ResizeNwSe, ResizeVertical};
    let out = grab_margin(look);
    let inside = look.at(4.0);
    let corner = look.at(14.0);
    let (left, right, top, bottom) = (r.left(), r.right(), r.top(), r.bottom());
    let band = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
    // Corners come last, so they are on top of the edges they touch.
    let edges = [
        (
            "e",
            band(right - inside, top + corner, right + out, bottom - corner),
            ResizeHorizontal,
        ),
        (
            "w",
            band(left - out, top + corner, left + inside, bottom - corner),
            ResizeHorizontal,
        ),
        (
            "s",
            band(left + corner, bottom - inside, right - corner, bottom + out),
            ResizeVertical,
        ),
        (
            "n",
            band(left + corner, top - out, right - corner, top + 2.0),
            ResizeVertical,
        ),
        (
            "se",
            band(right - corner, bottom - corner, right + out, bottom + out),
            ResizeNwSe,
        ),
        (
            "sw",
            band(left - out, bottom - corner, left + corner, bottom + out),
            ResizeNeSw,
        ),
        (
            "ne",
            band(right - corner, top - out, right + out, top + 2.0),
            ResizeNeSw,
        ),
        (
            "nw",
            band(left - out, top - out, left + corner, top + 2.0),
            ResizeNwSe,
        ),
    ];
    let mut resized = None;
    let mut stopped = false;
    let mut lit = None;
    let clip = ui.clip_rect();
    ui.set_clip_rect(clip.union(r.expand(out)));
    for (name, rect, cursor) in edges {
        let response = ui
            .interact(rect, id.with(("resize", name)), Sense::drag())
            .on_hover_cursor(cursor);
        if let Some(delta) = follow(ui, id, name, r, &response) {
            resized = Some((name, delta));
            lit = Some((name, 1.0));
        } else if response.hovered() && lit.is_none() {
            lit = Some((name, 0.6));
        }
        stopped |= response.drag_stopped();
    }
    ui.set_clip_rect(clip);
    let shown =
        ui.ctx()
            .animate_value_with_time(id.with("resize-lit"), lit.map_or(0.0, |(_, a)| a), 0.12);
    if let Some((name, _)) = lit {
        ui.ctx()
            .data_mut(|data| data.insert_temp(id.with("resize-edge"), name));
    }
    let name = ui
        .ctx()
        .data(|data| data.get_temp::<&'static str>(id.with("resize-edge")));
    if let Some(name) = name.filter(|_| shown > 0.01) {
        light_edge(ui.ctx(), id, r, look, name, shown);
    }
    (resized, stopped)
}

/// How far the edge `name` of the frame `r` has to move to stay under the
/// pointer that drags it: it follows the pointer from where the player
/// took it, so it never drifts from it, also after a size limit.
fn follow(ui: &egui::Ui, id: Id, name: &str, r: Rect, response: &egui::Response) -> Option<Vec2> {
    let edge = pos2(
        if name.contains('e') {
            r.right()
        } else {
            r.left()
        },
        if name.contains('s') {
            r.bottom()
        } else {
            r.top()
        },
    );
    let taken = id.with(("resize-taken", name));
    let pointer = response.interact_pointer_pos()?;
    if response.drag_started() {
        ui.ctx()
            .data_mut(|data| data.insert_temp(taken, pointer - edge));
    }
    if !response.dragged() {
        return None;
    }
    let offset = ui
        .ctx()
        .data(|data| data.get_temp::<Vec2>(taken))
        .unwrap_or_default();
    let mut delta = pointer - offset - edge;
    if !name.contains('e') && !name.contains('w') {
        delta.x = 0.0;
    }
    if !name.contains('n') && !name.contains('s') {
        delta.y = 0.0;
    }
    Some(delta)
}

/// Draws the edge or the corner `name` of the frame `r` in the action color,
/// `shown` of the way in.
fn light_edge(context: &egui::Context, id: Id, r: Rect, look: Look, name: &str, shown: f32) {
    let (left, right, top, bottom) = (r.left(), r.right(), r.top(), r.bottom());
    let painter = context.layer_painter(LayerId::new(Order::Foreground, id.with("resize-line")));
    let stroke = Stroke::new(look.at(2.0), theme::alpha(look.p.action, 0.7 * shown));
    let round = look.at(6.0);
    if name.contains('e') {
        painter.line_segment(
            [pos2(right, top + round), pos2(right, bottom - round)],
            stroke,
        );
    }
    if name.contains('w') {
        painter.line_segment(
            [pos2(left, top + round), pos2(left, bottom - round)],
            stroke,
        );
    }
    if name.contains('s') {
        painter.line_segment(
            [pos2(left + round, bottom), pos2(right - round, bottom)],
            stroke,
        );
    }
    if name.contains('n') {
        painter.line_segment([pos2(left + round, top), pos2(right - round, top)], stroke);
    }
}

/// Applies what the player did to a window's state; returns whether to
/// save the places.
fn apply(
    state: &mut WindowState,
    handled: &Handled,
    others: &[Rect],
    f: &Frame<'_>,
    height: f32,
) -> bool {
    let (look, screen) = (f.look, f.screen);
    if handled.moved != Vec2::ZERO {
        let free = state.free.unwrap_or(state.pos) + handled.moved;
        state.free = Some(free);
        let size = vec2(state.width, height);
        let (pos, snapped) = snap(free, size, others, screen, look, state.snapped);
        if snapped != state.snapped && snapped.iter().any(Option::is_some) {
            state.snapped_at = f.now;
        }
        state.pos = pos;
        state.snapped = snapped;
        state.guides = guides(Rect::from_min_size(pos, size), snapped, others);
    }
    if handled.stopped && state.free.take().is_some() {
        state.snapped = [None; 2];
        state.released_at = f.now;
    }
    state.resizing = handled.resized.map(|(edge, _)| edge);
    if let Some((edge, delta)) = handled.resized {
        let current = state.height.unwrap_or(height);
        if edge.contains('e') {
            state.width = (state.width + delta.x).max(look.at(240.0));
        }
        if edge.contains('w') {
            let width = (state.width - delta.x).max(look.at(240.0));
            state.pos.x += state.width - width;
            state.width = width;
        }
        if edge.contains('s') {
            state.height = Some((current + delta.y).max(look.at(80.0)));
        }
        if edge.contains('n') {
            let new = (current - delta.y).max(look.at(80.0));
            state.pos.y += current - new;
            state.height = Some(new);
        }
    }
    keep_on_screen(state, screen, look, height);
    if let Some(measured) = handled.measured {
        state.content = measured;
    }
    draw_guides(f, state);
    let mut stopped = handled.stopped;
    if handled.pressed.toggle {
        state.collapsed = !state.collapsed;
        stopped = true;
    }
    stopped
}

/// Keeps a window whole on the screen, as Angelica's menu: no wider or
/// taller than the screen, and inside it.
fn keep_on_screen(state: &mut WindowState, screen: Rect, look: Look, height: f32) {
    state.width = state
        .width
        .clamp(look.at(240.0), screen.width().max(look.at(240.0)));
    if let Some(given) = state.height {
        state.height = Some(given.clamp(look.at(80.0), screen.height().max(look.at(80.0))));
    }
    let height = height.min(screen.height());
    state.pos.x = state.pos.x.clamp(
        screen.left(),
        (screen.right() - state.width).max(screen.left()),
    );
    state.pos.y = state
        .pos
        .y
        .clamp(screen.top(), (screen.bottom() - height).max(screen.top()));
}

/// Where a window of `size` that the pointer alone would put at `free`
/// goes: to an edge of the screen or of a window beside it within [`SNAP`]
/// points, across and down, and the lines it snapped to. A line it already
/// holds lets go only a little further away, so it neither jitters on the
/// line nor sticks to it.
fn snap(
    free: Pos2,
    size: Vec2,
    others: &[Rect],
    screen: Rect,
    look: Look,
    held: [Option<f32>; 2],
) -> (Pos2, [Option<f32>; 2]) {
    let near = look.at(SNAP);
    let rect = Rect::from_min_size(free, size);
    let mut xs = vec![screen.left(), screen.right()];
    let mut ys = vec![screen.top(), screen.bottom()];
    for other in others {
        // Only the edges of a window beside this one along the other axis.
        if other.top() - near < rect.bottom() && rect.top() < other.bottom() + near {
            xs.extend([other.left(), other.right()]);
        }
        if other.left() - near < rect.right() && rect.left() < other.right() + near {
            ys.extend([other.top(), other.bottom()]);
        }
    }
    let axis = |start: f32, len: f32, lines: &[f32], held: Option<f32>| {
        let mut best: Option<(f32, f32, f32)> = None;
        for &line in lines {
            let reach = if held == Some(line) {
                near * 1.75
            } else {
                near
            };
            for (edge, at) in [(start, line), (start + len, line - len)] {
                let distance = (edge - line).abs();
                if distance < reach && best.is_none_or(|(d, ..)| distance < d) {
                    best = Some((distance, line, at));
                }
            }
        }
        best.map(|(_, line, at)| (line, at))
    };
    let x = axis(free.x, size.x, &xs, held[0]);
    let y = axis(free.y, size.y, &ys, held[1]);
    (
        pos2(
            x.map_or(free.x, |(_, at)| at),
            y.map_or(free.y, |(_, at)| at),
        ),
        [x.map(|(line, _)| line), y.map(|(line, _)| line)],
    )
}

/// The guides of a snap: each line the window `rect` snapped to, along the
/// window and the windows that share it.
fn guides(rect: Rect, snapped: [Option<f32>; 2], others: &[Rect]) -> Vec<[Pos2; 2]> {
    let mut lines = Vec::new();
    if let Some(x) = snapped[0] {
        let (mut top, mut bottom) = (rect.top(), rect.bottom());
        for other in others
            .iter()
            .filter(|o| (o.left() - x).abs() < 0.5 || (o.right() - x).abs() < 0.5)
        {
            top = top.min(other.top());
            bottom = bottom.max(other.bottom());
        }
        lines.push([pos2(x, top), pos2(x, bottom)]);
    }
    if let Some(y) = snapped[1] {
        let (mut left, mut right) = (rect.left(), rect.right());
        for other in others
            .iter()
            .filter(|o| (o.top() - y).abs() < 0.5 || (o.bottom() - y).abs() < 0.5)
        {
            left = left.min(other.left());
            right = right.max(other.right());
        }
        lines.push([pos2(left, y), pos2(right, y)]);
    }
    lines
}

/// Draws a window's snap guides: bright for a moment when it takes a line,
/// softer while it holds it, fading once the player lets it go.
fn draw_guides(f: &Frame<'_>, state: &WindowState) {
    if state.guides.is_empty() {
        return;
    }
    #[allow(clippy::cast_possible_truncation)]
    let strength = if state.free.is_some() {
        let flash = (1.0 - (f.now - state.snapped_at) / 0.3).clamp(0.0, 1.0) as f32;
        if state.snapped.iter().all(Option::is_none) {
            return;
        }
        0.45 + 0.55 * flash
    } else {
        0.45 * (1.0 - (f.now - state.released_at) / 0.25).clamp(0.0, 1.0) as f32
    };
    if strength <= 0.0 {
        return;
    }
    f.context.request_repaint();
    let look = f.look;
    let painter = f
        .context
        .layer_painter(LayerId::new(Order::Foreground, Id::new("angelica-snap")));
    for guide in &state.guides {
        painter.line_segment(
            *guide,
            Stroke::new(look.at(2.0), theme::alpha(look.p.action, strength)),
        );
    }
}

// ----------------------------------------------------------- panel parts

/// Whether a part takes the player's input, so its panel takes clicks.
fn takes_input(widget: &Widget) -> bool {
    matches!(
        widget,
        Widget::Button(_)
            | Widget::Checkbox(_)
            | Widget::TextInput(_)
            | Widget::Choice(_)
            | Widget::Number(_)
            | Widget::Tabs(_)
            | Widget::Entry(_)
            | Widget::BeginSection(_)
    )
}

/// The frame of a panel or a popup: a plate, or a soft dark backdrop for
/// parts straight over the game.
fn backing_frame(f: &Frame<'_>, bare: bool) -> egui::Frame {
    let look = f.look;
    let frame = if bare {
        egui::Frame::new()
            .fill(theme::BARE.window)
            .corner_radius(CornerRadius::same(8))
            .shadow(egui::Shadow {
                offset: [0, 0],
                blur: 18,
                spread: 2,
                color: egui::Color32::from_black_alpha(48),
            })
    } else {
        egui::Frame::new()
            .fill(look.p.window)
            .stroke(Stroke::new(1.0, look.p.border))
            .corner_radius(CornerRadius::same(6))
            .shadow(f.context.global_style().visuals.window_shadow)
    };
    frame.inner_margin(margin(look, 11.0, 9.0))
}

/// Draws a panel and returns its response: dragged in the layout mode.
fn panel_area(
    f: &mut Frame<'_>,
    key: &Key,
    panel: &Panel,
    pos: Pos2,
    opacity: f32,
    layout: bool,
) -> egui::Response {
    let bare = panel.backing == Backing::Bare;
    let look = f.look;
    let parts_look = if bare {
        Look {
            k: look.k,
            p: &theme::BARE,
        }
    } else {
        look
    };
    let interactable = layout || panel.widgets.iter().any(takes_input);
    let screen = f.screen;
    let frame = backing_frame(f, bare);
    let context = f.context;
    egui::Area::new(Id::new(("angelica-panel", &key.0, &key.1)))
        .order(Order::Background)
        .fixed_pos(pos)
        .interactable(interactable)
        .sense(if layout {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        })
        .show(context, |ui| {
            ui.multiply_opacity(opacity);
            frame.show(ui, |ui| {
                ui.set_max_width(look.at(260.0).min(screen.width() / 3.0));
                ui.set_max_height(screen.height() / 3.0);
                ui.spacing_mut().item_spacing = vec2(look.at(10.0), look.at(7.0));
                f.parts(ui, &key.0, &key.1, parts_look, &panel.widgets);
            });
        })
        .response
}

/// A panel's dashed outline and name in the layout mode, and its menu of
/// when it is shown; returns the player's choice.
fn layout_outline(
    f: &Frame<'_>,
    response: &egui::Response,
    panel: &Panel,
    choice: Choice,
) -> Option<Choice> {
    let (look, strings) = (f.look, f.strings);
    let words = [
        (Choice::Always, strings.shown_always),
        (Choice::InCombat, strings.shown_in_combat),
        (Choice::OutOfCombat, strings.shown_out_of_combat),
        (Choice::Hidden, strings.shown_hidden),
    ];
    let when = words
        .iter()
        .find(|(c, _)| *c == choice)
        .map_or("", |(_, word)| word);
    let painter = f
        .context
        .layer_painter(LayerId::new(Order::Foreground, response.id.with("outline")));
    let outline = response.rect.expand(look.at(5.0));
    let stroke = Stroke::new(1.0, egui::Color32::WHITE);
    for (from, to) in [
        (outline.left_top(), outline.right_top()),
        (outline.right_top(), outline.right_bottom()),
        (outline.right_bottom(), outline.left_bottom()),
        (outline.left_bottom(), outline.left_top()),
    ] {
        painter.extend(egui::Shape::dashed_line(
            &[from, to],
            stroke,
            look.at(5.0),
            look.at(4.0),
        ));
    }
    let label = format!("{} · {when}", panel.name);
    let galley = painter.layout_no_wrap(label, theme::sans(look.at(11.5)), egui::Color32::WHITE);
    let at = outline.left_top() - vec2(0.0, galley.size().y + look.at(6.0));
    painter.rect_filled(
        Rect::from_min_size(
            at - vec2(look.at(6.0), look.at(2.0)),
            galley.size() + vec2(look.at(12.0), look.at(4.0)),
        ),
        CornerRadius::same(3),
        egui::Color32::from_black_alpha(150),
    );
    painter.galley(at, galley, egui::Color32::WHITE);
    let mut picked = None;
    response.context_menu(|ui| {
        for (option, word) in words {
            let color = if option == choice {
                look.p.action
            } else {
                look.p.text
            };
            if menu::popup_item(ui, &look, None, word, color) {
                picked = Some(option);
            }
        }
    });
    picked
}

// ----------------------------------------------------------- popup parts

/// Where a popup goes: its pivot and the point it is pinned to.
fn popup_place(f: &Frame<'_>, at: &PopupAt, beside: Option<Rect>, corner: f32) -> (Align2, Pos2) {
    let (look, screen) = (f.look, f.screen);
    if *at == PopupAt::Center {
        return (
            Align2::CENTER_TOP,
            pos2(screen.center().x, screen.top() + screen.height() * 0.18),
        );
    }
    match beside {
        Some(rect) if rect.right() + look.at(260.0) < screen.right() => (
            Align2::LEFT_TOP,
            rect.right_top() + vec2(look.at(10.0), 0.0),
        ),
        Some(rect) => (
            Align2::RIGHT_TOP,
            rect.left_top() - vec2(look.at(10.0), 0.0),
        ),
        None => (
            Align2::RIGHT_TOP,
            pos2(screen.right() - look.at(24.0), corner),
        ),
    }
}

/// Draws a popup and returns where it went.
fn popup_area(
    f: &mut Frame<'_>,
    index: usize,
    plugin: &str,
    popup: &angelica_view::Popup,
    (pivot, at): (Align2, Pos2),
    fade: f32,
) -> Rect {
    let look = f.look;
    let center = popup.at == PopupAt::Center;
    let mut widgets = popup.widgets.clone();
    // Above the middle of the screen, the first text is large.
    if center
        && let Some(Widget::Text(first)) = widgets.iter_mut().find(|w| matches!(w, Widget::Text(_)))
        && first.role == Role::Body
    {
        first.role = Role::Big;
    }
    let parts_look = if center {
        Look {
            k: look.k,
            p: &theme::BARE,
        }
    } else {
        look
    };
    let name = f.host.plugin(plugin).map_or(plugin.to_owned(), |plugin| {
        plugin.manifest.plugin.name.clone()
    });
    let frame = if center {
        egui::Frame::new()
    } else {
        menu::window_frame(f.context, look.p).inner_margin(margin(look, 11.0, 8.0))
    };
    let context = f.context;
    egui::Area::new(Id::new(("angelica-popup", index, plugin)))
        .order(Order::Foreground)
        .interactable(false)
        .pivot(pivot)
        .fixed_pos(at)
        .show(context, |ui| {
            ui.multiply_opacity(fade);
            frame.show(ui, |ui| {
                ui.set_max_width(look.at(320.0));
                ui.spacing_mut().item_spacing = vec2(look.at(8.0), look.at(4.0));
                if !center {
                    ui.label(
                        egui::RichText::new(&name)
                            .font(theme::wordmark(look.at(12.0)))
                            .color(look.p.secondary),
                    );
                }
                f.parts(ui, plugin, "popup", parts_look, &widgets);
            });
        })
        .response
        .rect
}

/// Whether the player is in combat, read through Lens with the query
/// prepared once for each game map; `None` when the map does not say.
#[derive(Default)]
struct InCombat {
    prepared: Option<(u64, Option<crate::lens::Prepared>)>,
}

impl InCombat {
    fn now(&mut self, host: &Host) -> Option<bool> {
        let services = host.services();
        let (schema, generation) = services.lens.schema();
        if self
            .prepared
            .as_ref()
            .is_none_or(|(seen, _)| *seen != generation)
        {
            let prepared = crate::lens::prepare(&schema, "activity { in_combat }", &|_| true).ok();
            self.prepared = Some((generation, prepared));
        }
        let prepared = self.prepared.as_ref()?.1.as_ref()?;
        let rows = services.game.query(prepared).ok()?;
        match rows.first()?.values.first()? {
            Some(crate::lens::Value::Boolean(value)) => Some(*value),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look {
        Look {
            k: 1.0,
            p: &theme::BARE,
        }
    }

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0))
    }

    #[test]
    fn a_window_snaps_to_the_screens_edges_and_lets_go_a_little_further() {
        let size = vec2(300.0, 200.0);
        let snap_at =
            |x: f32, held| snap(pos2(x, 300.0), size, &[], screen(), look(), [held, None]);
        assert_eq!(snap_at(5.0, None), (pos2(0.0, 300.0), [Some(0.0), None]));
        assert_eq!(
            snap_at(1296.0, None),
            (pos2(1300.0, 300.0), [Some(1600.0), None])
        );
        // Beyond reach it moves freely; held, the edge keeps it a little longer.
        assert_eq!(snap_at(12.0, None), (pos2(12.0, 300.0), [None, None]));
        assert_eq!(
            snap_at(12.0, Some(0.0)),
            (pos2(0.0, 300.0), [Some(0.0), None])
        );
        assert_eq!(snap_at(16.0, Some(0.0)), (pos2(16.0, 300.0), [None, None]));
    }

    #[test]
    fn a_window_snaps_only_to_windows_beside_it() {
        let size = vec2(300.0, 200.0);
        let beside = Rect::from_min_size(pos2(500.0, 250.0), vec2(200.0, 300.0));
        let far_below = Rect::from_min_size(pos2(500.0, 700.0), vec2(200.0, 150.0));
        // Its left edge 4 points right of the other's right edge.
        let (pos, lines) = snap(
            pos2(704.0, 300.0),
            size,
            &[beside],
            screen(),
            look(),
            [None; 2],
        );
        assert_eq!((pos.x, lines[0]), (700.0, Some(700.0)));
        let (pos, lines) = snap(
            pos2(704.0, 300.0),
            size,
            &[far_below],
            screen(),
            look(),
            [None; 2],
        );
        assert_eq!((pos.x, lines[0]), (704.0, None));
    }

    #[test]
    fn a_window_stays_whole_on_the_screen() {
        let mut state = WindowState {
            pos: pos2(600.0, 800.0),
            width: 593_725.6,
            height: Some(2000.0),
            content: 120.0,
            collapsed: false,
            opened: 0.0,
            resizing: None,
            free: None,
            snapped: [None; 2],
            snapped_at: 0.0,
            guides: Vec::new(),
            released_at: 0.0,
            asked: None,
        };
        keep_on_screen(&mut state, screen(), look(), 400.0);
        assert!((state.width - 1600.0).abs() < 0.01, "{}", state.width);
        assert_eq!(state.height, Some(900.0));
        assert_eq!(state.pos, pos2(0.0, 500.0));
    }
}
