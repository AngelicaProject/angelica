//! The installer's one window: the same tall shape on every screen, with
//! Angelica's own title bar, in its light look (`docs/architecture/ui.md`).
use crate::{exit, install, neighbors::Neighbors, payload, system, words::Words};
use egui::{Color32, CornerRadius, FontFamily, FontId, Rect, RichText, Sense, Stroke, pos2, vec2};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError, mpsc},
};

const SIZE: [f32; 2] = [380.0, 500.0];
const BAR: f32 = 34.0;
const CORNER: u8 = 10;
const PADDING: f32 = 24.0;

const WINDOW: Color32 = Color32::from_rgb(0xf6, 0xf3, 0xed);
const BORDER: Color32 = Color32::from_rgb(0xdc, 0xd3, 0xc3);
const TEXT: Color32 = Color32::from_rgb(0x24, 0x21, 0x1c);
const SECONDARY: Color32 = Color32::from_rgb(0x6e, 0x67, 0x5c);
const MUTED: Color32 = Color32::from_rgb(0x9b, 0x94, 0x88);
const LINE: Color32 = Color32::from_rgb(0xec, 0xe6, 0xdb);
const TRACK: Color32 = Color32::from_rgb(0xe7, 0xdf, 0xd1);
const ACTION: Color32 = Color32::from_rgb(0x7a, 0x5c, 0x2e);
const GOOD: Color32 = Color32::from_rgb(0x2e, 0x7d, 0x46);
const ATTENTION: Color32 = Color32::from_rgb(0xb7, 0x6e, 0x00);
const DANGER: Color32 = Color32::from_rgb(0xb2, 0x3a, 0x3a);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Install,
    Uninstall,
}

/// What the work on its own thread reports.
#[derive(Debug, Clone, PartialEq)]
enum Work {
    Idle,
    Busy { step: &'static str, progress: f32 },
    Done,
    Failed(String),
}

struct App {
    mode: Mode,
    words: &'static Words,
    games: Vec<PathBuf>,
    chosen: usize,
    /// The folder the player chose has no game.
    missing: bool,
    /// While Windows' folder dialog is open: what it returns, once it does.
    picking: Option<mpsc::Receiver<Option<PathBuf>>>,
    keep: bool,
    work: Arc<Mutex<Work>>,
}

pub fn run(mode: Mode) {
    let words = if system::russian() {
        &crate::words::RU
    } else {
        &crate::words::EN
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(words.title)
            .with_inner_size(SIZE)
            .with_min_inner_size(SIZE)
            .with_max_inner_size(SIZE)
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_icon(egui::IconData::default()),
        centered: true,
        ..Default::default()
    };
    let games = if mode == Mode::Install {
        angelica_games::find()
    } else {
        Vec::new()
    };
    let _ = eframe::run_native(
        words.title,
        options,
        Box::new(move |creation| {
            creation.egui_ctx.set_fonts(fonts());
            Ok(Box::new(App {
                mode,
                words,
                games,
                chosen: 0,
                missing: false,
                picking: None,
                keep: true,
                work: Arc::new(Mutex::new(Work::Idle)),
            }))
        }),
    );
}

fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::empty();
    for (name, bytes) in [
        (
            "golos",
            &include_bytes!("../../angelica-look/fonts/GolosText-Regular.ttf")[..],
        ),
        (
            "golos-medium",
            &include_bytes!("../../angelica-look/fonts/GolosText-Medium.ttf")[..],
        ),
        (
            "quicksand",
            &include_bytes!("../../angelica-look/fonts/Quicksand-SemiBold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(egui::FontData::from_static(bytes)));
    }
    for (family, faces) in [
        (FontFamily::Proportional, vec!["golos"]),
        (FontFamily::Monospace, vec!["golos"]),
        (
            FontFamily::Name("medium".into()),
            vec!["golos-medium", "golos"],
        ),
        (
            FontFamily::Name("wordmark".into()),
            vec!["quicksand", "golos"],
        ),
    ] {
        fonts
            .families
            .insert(family, faces.into_iter().map(Into::into).collect());
    }
    fonts
}

fn sans(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

/// A word that acts when clicked, like the menu's actions.
fn word(ui: &mut egui::Ui, text: &str, color: Color32, strong: bool, enabled: bool) -> bool {
    let font = if strong { medium(14.0) } else { sans(13.0) };
    let color = if enabled { color } else { MUTED };
    let response = ui.add(
        egui::Label::new(RichText::new(text).font(font).color(color))
            .selectable(false)
            .sense(if enabled {
                Sense::click()
            } else {
                Sense::hover()
            }),
    );
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    enabled && response.clicked()
}

fn text(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(RichText::new(text).font(sans(13.0)).color(color))
            .selectable(false)
            .wrap(),
    );
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(medium(19.0)).color(TEXT)).selectable(false));
    ui.add_space(18.0);
}

/// A small label above its value, as in the menu's details.
fn field(ui: &mut egui::Ui, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    ui.add(egui::Label::new(RichText::new(label).font(sans(11.5)).color(MUTED)).selectable(false));
    ui.add_space(3.0);
    value(ui);
    ui.add_space(14.0);
}

fn progress(ui: &mut egui::Ui, fraction: f32) {
    let (track, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
    ui.painter()
        .rect_filled(track, CornerRadius::same(2), TRACK);
    let mut filled = track;
    filled.set_width(track.width() * fraction.clamp(0.0, 1.0));
    ui.painter()
        .rect_filled(filled, CornerRadius::same(2), ACTION);
}

/// A key as a small token, like the menu's key.
fn key(ui: &mut egui::Ui, text: &str) {
    let galley = ui.painter().layout_no_wrap(text.into(), sans(12.0), ACTION);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(12.0, 6.0), Sense::hover());
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, Color32::from_rgb(0xc9, 0xb4, 0x8f)),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ACTION);
}

/// The game folder in a few words: its launcher and the installation's
/// folder name.
fn short(game: &std::path::Path) -> String {
    let text = game.display().to_string();
    let launcher = if text.to_ascii_lowercase().contains("steam") {
        "Steam · "
    } else {
        ""
    };
    let parent = game
        .parent()
        .and_then(|parent| parent.file_name())
        .map_or_else(|| text.clone(), |name| name.to_string_lossy().into_owned());
    format!("{launcher}{parent}")
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let work = self
            .work
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        self.picked();
        if matches!(work, Work::Busy { .. }) || self.picking.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let whole = ui.max_rect();
        ui.painter()
            .rect_filled(whole, CornerRadius::same(CORNER), WINDOW);
        ui.painter().rect_stroke(
            whole,
            CornerRadius::same(CORNER),
            Stroke::new(1.0, BORDER),
            egui::StrokeKind::Inside,
        );
        self.title_bar(ui, whole);
        let content = Rect::from_min_max(
            pos2(whole.left() + PADDING, whole.top() + BAR + 20.0),
            pos2(whole.right() - PADDING, whole.bottom() - PADDING + 4.0),
        );
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(content));
        match (self.mode, &work) {
            (
                _,
                Work::Busy {
                    step,
                    progress: fraction,
                },
            ) => self.busy(&mut ui, step, *fraction),
            (_, Work::Failed(reason)) => {
                let reason = reason.clone();
                self.failed(&mut ui, &reason);
            }
            (Mode::Install, Work::Done) => self.done(&mut ui),
            (Mode::Uninstall, Work::Done) => self.removed(&mut ui),
            (Mode::Install, Work::Idle) => self.start(&mut ui),
            (Mode::Uninstall, Work::Idle) => self.ask_remove(&mut ui),
        }
    }
}

impl App {
    fn set(&self, work: Work) {
        *self.work.lock().unwrap_or_else(PoisonError::into_inner) = work;
    }

    /// Angelica's title bar: the word `angelica`, and minimize and close at
    /// the right; dragging it moves the window.
    fn title_bar(&self, ui: &mut egui::Ui, whole: Rect) {
        let bar = Rect::from_min_size(whole.min, vec2(whole.width(), BAR));
        let drag = ui.interact(bar, egui::Id::new("title-bar"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        ui.painter().text(
            pos2(bar.left() + 14.0, bar.center().y),
            egui::Align2::LEFT_CENTER,
            "angelica",
            FontId::new(13.0, FontFamily::Name("wordmark".into())),
            MUTED,
        );
        ui.painter().line_segment(
            [
                pos2(bar.left() + 1.0, bar.bottom()),
                pos2(bar.right() - 1.0, bar.bottom()),
            ],
            Stroke::new(1.0, LINE),
        );
        let close =
            Rect::from_center_size(pos2(bar.right() - 20.0, bar.center().y), vec2(28.0, 24.0));
        let minimize = close.translate(vec2(-30.0, 0.0));
        let minimize_hit = ui
            .interact(minimize, egui::Id::new("minimize"), Sense::click())
            .on_hover_text(self.words.minimize);
        let close_hit = ui
            .interact(close, egui::Id::new("close"), Sense::click())
            .on_hover_text(self.words.close_window);
        let pen = |hovered: bool| Stroke::new(1.3, if hovered { TEXT } else { MUTED });
        let c = minimize.center();
        ui.painter().line_segment(
            [pos2(c.x - 5.0, c.y), pos2(c.x + 5.0, c.y)],
            pen(minimize_hit.hovered()),
        );
        let c = close.center();
        for (from, to) in [((-4.5, -4.5), (4.5, 4.5)), ((-4.5, 4.5), (4.5, -4.5))] {
            ui.painter().line_segment(
                [
                    pos2(c.x + from.0, c.y + from.1),
                    pos2(c.x + to.0, c.y + to.1),
                ],
                pen(close_hit.hovered()),
            );
        }
        if minimize_hit.clicked() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if close_hit.clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// The bottom line: a muted note at the left and actions at the right.
    fn bottom(ui: &mut egui::Ui, note: &str, actions: impl FnOnce(&mut egui::Ui)) {
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(RichText::new(note).font(sans(11.5)).color(MUTED))
                        .selectable(false),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    actions(ui);
                });
            });
        });
    }

    fn version() -> String {
        payload::PAYLOAD.map_or_else(String::new, |payload| {
            format!("Angelica {}", payload.version)
        })
    }

    fn game(&self) -> Option<PathBuf> {
        self.games.get(self.chosen).cloned()
    }

    /// Opens Windows' folder dialog over the installer on its own thread,
    /// starting at the chosen game's installation.
    fn pick(&mut self) {
        let (sender, receiver) = mpsc::channel();
        let start = self
            .game()
            .and_then(|game| game.parent().map(std::path::Path::to_path_buf));
        let owner = system::foreground_window();
        std::thread::spawn(move || {
            let _ = sender.send(system::pick_folder(owner, start.as_deref()));
        });
        self.picking = Some(receiver);
    }

    /// Takes the folder the dialog returned: its game is chosen, added to
    /// the list when it was not there.
    fn picked(&mut self) {
        let Some(receiver) = &self.picking else {
            return;
        };
        let picked = match receiver.try_recv() {
            Ok(picked) => picked,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        self.picking = None;
        let Some(folder) = picked else {
            return;
        };
        match angelica_games::resolve(&folder) {
            Some(game) => {
                self.missing = false;
                self.chosen = self
                    .games
                    .iter()
                    .position(|known| *known == game)
                    .unwrap_or_else(|| {
                        self.games.push(game);
                        self.games.len() - 1
                    });
            }
            None => self.missing = true,
        }
    }

    fn start(&mut self, ui: &mut egui::Ui) {
        let w = self.words;
        if payload::PAYLOAD.is_none() || payload::release_key().is_none() {
            heading(ui, w.development);
            text(ui, w.development_text, SECONDARY);
            Self::bottom(ui, "", |ui| {
                if word(ui, w.close, ACTION, true, true) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            return;
        }
        heading(ui, w.installation);
        let (games, chosen, picking) = (self.games.clone(), self.chosen, self.picking.is_some());
        let mut pick = None;
        let mut browse = false;
        field(ui, w.game, |ui| {
            if games.is_empty() {
                browse = word(ui, w.game_choose, ACTION, false, !picking);
                return;
            }
            if games.len() > 1 {
                egui::ComboBox::from_id_salt("game")
                    .width(ui.available_width())
                    .selected_text(RichText::new(short(&games[chosen])).color(ACTION))
                    .show_ui(ui, |ui| {
                        for (index, game) in games.iter().enumerate() {
                            if ui
                                .selectable_label(index == chosen, game.display().to_string())
                                .clicked()
                            {
                                pick = Some(index);
                            }
                        }
                    });
            } else {
                ui.add(
                    egui::Label::new(
                        RichText::new(short(&games[chosen]))
                            .font(sans(13.5))
                            .color(TEXT),
                    )
                    .selectable(false),
                )
                .on_hover_text(games[chosen].display().to_string());
            }
            ui.add_space(2.0);
            browse = word(ui, w.game_other, ACTION, false, !picking);
        });
        if let Some(index) = pick {
            self.chosen = index;
            self.missing = false;
        }
        if browse {
            self.pick();
        }
        if self.missing {
            text(ui, w.game_none, ATTENTION);
            ui.add_space(10.0);
        }
        let game = self.game();
        let neighbors = game
            .as_deref()
            .map(|game| Neighbors::find(game, system::dalamud_installed()));
        let compatible = neighbors
            .as_ref()
            .map(Neighbors::compatible)
            .unwrap_or_default();
        if !compatible.is_empty() {
            field(ui, w.beside, |ui| {
                text(
                    ui,
                    &w.beside_value.replace("{names}", &compatible.join(", ")),
                    GOOD,
                );
            });
        }
        let conflict = neighbors.as_ref().is_some_and(|found| found.other_proxy);
        if conflict {
            text(ui, w.conflict, DANGER);
        } else if cfg!(windows) && game.as_deref().is_some_and(|game| !install::writable(game)) {
            text(ui, w.admin_note, SECONDARY);
        }
        let ready = game.is_some() && !conflict && !picking;
        Self::bottom(ui, &Self::version(), |ui| {
            if word(ui, w.install, ACTION, true, ready)
                && let Some(game) = game
            {
                self.begin_install(game);
            }
        });
    }

    fn busy(&self, ui: &mut egui::Ui, step: &str, fraction: f32) {
        let w = self.words;
        heading(
            ui,
            if self.mode == Mode::Install {
                w.installation
            } else {
                w.removal
            },
        );
        progress(ui, fraction);
        ui.add_space(12.0);
        text(ui, step, SECONDARY);
        Self::bottom(ui, &Self::version(), |_| {});
    }

    fn done(&self, ui: &mut egui::Ui) {
        let w = self.words;
        heading(ui, w.done);
        field(ui, w.menu_key, |ui| key(ui, "Insert"));
        field(ui, w.updates, |ui| text(ui, w.updates_value, TEXT));
        Self::bottom(ui, &Self::version(), |ui| {
            if word(ui, w.close, ACTION, true, true) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn failed(&mut self, ui: &mut egui::Ui, reason: &str) {
        let w = self.words;
        heading(
            ui,
            if self.mode == Mode::Install {
                w.failed
            } else {
                w.removal
            },
        );
        text(ui, reason, TEXT);
        Self::bottom(ui, &Self::version(), |ui| {
            if word(ui, w.retry, ACTION, true, true) {
                self.set(Work::Idle);
            }
            if word(ui, w.close, SECONDARY, false, true) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn ask_remove(&mut self, ui: &mut egui::Ui) {
        let w = self.words;
        heading(ui, w.removal);
        field(ui, w.game_file, |ui| text(ui, w.game_file_value, TEXT));
        let keep = self.keep;
        let mut chosen = None;
        field(ui, w.player_data, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                for (value, label) in [(true, w.keep), (false, w.delete)] {
                    let font = if keep == value {
                        medium(13.5)
                    } else {
                        sans(13.5)
                    };
                    let color = if keep == value { TEXT } else { MUTED };
                    if ui
                        .add(
                            egui::Label::new(RichText::new(label).font(font).color(color))
                                .selectable(false)
                                .sense(Sense::click()),
                        )
                        .clicked()
                    {
                        chosen = Some(value);
                    }
                }
            });
        });
        if let Some(value) = chosen {
            self.keep = value;
        }
        Self::bottom(ui, "", |ui| {
            if word(ui, w.remove, DANGER, true, true) {
                self.begin_uninstall();
            }
            if word(ui, w.cancel, SECONDARY, false, true) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn removed(&self, ui: &mut egui::Ui) {
        let w = self.words;
        heading(ui, w.removed);
        field(ui, w.game, |ui| text(ui, w.removed_value, TEXT));
        Self::bottom(ui, "", |ui| {
            if word(ui, w.close, ACTION, true, true) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn begin_install(&self, game: PathBuf) {
        let work = self.work.clone();
        let words = self.words;
        self.set(Work::Busy {
            step: words.step_check,
            progress: 0.1,
        });
        std::thread::spawn(move || {
            let report = |step, progress| {
                *work.lock().unwrap_or_else(PoisonError::into_inner) =
                    Work::Busy { step, progress };
            };
            let result = install_all(&game, words, &report);
            *work.lock().unwrap_or_else(PoisonError::into_inner) = match result {
                Ok(()) => Work::Done,
                Err(reason) => Work::Failed(reason),
            };
        });
    }

    fn begin_uninstall(&self) {
        let work = self.work.clone();
        let words = self.words;
        let keep = self.keep;
        self.set(Work::Busy {
            step: words.step_remove,
            progress: 0.4,
        });
        std::thread::spawn(move || {
            let result = uninstall_all(keep, words);
            *work.lock().unwrap_or_else(PoisonError::into_inner) = match result {
                Ok(()) => Work::Done,
                Err(reason) => Work::Failed(reason),
            };
        });
    }
}

fn failure(words: &Words, reason: &str) -> String {
    words.failure.replace("{reason}", reason)
}

fn install_all(
    game: &std::path::Path,
    w: &Words,
    report: &dyn Fn(&'static str, f32),
) -> Result<(), String> {
    let (Some(payload), Some(release)) = (payload::PAYLOAD, payload::release_key()) else {
        return Err(w.development_text.into());
    };
    payload.verify(&release).map_err(|e| failure(w, &e))?;
    closed_game(w)?;
    let prefixes = wine_prefixes(game, w)?;
    report(w.step_files, 0.35);
    let (installation, seen) = install::installation_for(game);
    install::install_files(
        &installation,
        payload.version,
        payload.channel,
        &payload.files(),
    )
    .map_err(|e| failure(w, &e))?;
    let uninstaller = installation.join(install::UNINSTALLER);
    if let Ok(program) = std::env::current_exe()
        && program != uninstaller
    {
        let _ = std::fs::copy(&program, &uninstaller);
    }
    if cfg!(not(windows)) || install::writable(game) {
        report(w.step_game, 0.7);
        install::place(game, &seen, payload.dinput8_dll).map_err(|e| place_error(w, &e))?;
    } else {
        report(w.step_admin, 0.7);
        let args = format!("--place \"{}\" \"{}\"", game.display(), seen.display());
        match system::run_elevated(&args) {
            Ok(0) => {}
            Ok(code) => return Err(exit_error(w, code)),
            Err(_) => return Err(w.declined.into()),
        }
    }
    set_overrides(&prefixes, w)?;
    install::record_game(&installation, game, &prefixes).map_err(|e| failure(w, &e))?;
    system::register(&uninstaller, payload.version).map_err(|e| failure(w, &e))?;
    report(w.step_files, 1.0);
    Ok(())
}

fn uninstall_all(keep: bool, w: &Words) -> Result<(), String> {
    closed_game(w)?;
    let installation = std::env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(std::path::Path::to_path_buf))
        .filter(|folder| folder.join("current.json").exists())
        .unwrap_or_else(install::installation_dir);
    remove_overrides(&install::recorded_prefixes(&installation), w)?;
    if let Some(game) = install::recorded_game(&installation) {
        if cfg!(not(windows)) || install::writable(&game) {
            install::remove_from_game(&game).map_err(|e| place_error(w, &e))?;
        } else {
            match system::run_elevated(&format!("--remove \"{}\"", game.display())) {
                Ok(0) => {}
                Ok(code) => return Err(exit_error(w, code)),
                Err(_) => return Err(w.declined.into()),
            }
        }
    }
    system::unregister();
    install::remove_installation(&installation, keep);
    Ok(())
}

/// Angelica is installed and removed only while the game is closed: the
/// game holds the proxy and the runtime open while it runs.
fn closed_game(w: &Words) -> Result<(), String> {
    match system::game_running() {
        Ok(false) => Ok(()),
        Ok(true) => Err(w.in_use.into()),
        Err(reason) => Err(failure(w, &reason)),
    }
}

/// The Wine prefixes the game runs in, none on Windows. Their registry is
/// changed only while Wine is stopped in each.
#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)]
fn wine_prefixes(_game: &std::path::Path, _w: &Words) -> Result<Vec<PathBuf>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
fn wine_prefixes(game: &std::path::Path, w: &Words) -> Result<Vec<PathBuf>, String> {
    let prefixes = angelica_games::wine_prefixes(game);
    if prefixes.is_empty() {
        return Err(w.no_prefix.into());
    }
    if prefixes.iter().any(|prefix| system::prefix_running(prefix)) {
        return Err(w.wine_running.into());
    }
    Ok(prefixes)
}

#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)]
fn set_overrides(_prefixes: &[PathBuf], _w: &Words) -> Result<(), String> {
    Ok(())
}

/// Makes Wine load the proxy in each prefix.
#[cfg(not(windows))]
fn set_overrides(prefixes: &[PathBuf], w: &Words) -> Result<(), String> {
    for prefix in prefixes {
        crate::wine::set_override(prefix).map_err(|e| registry_error(w, e))?;
    }
    Ok(())
}

#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)]
fn remove_overrides(_prefixes: &[PathBuf], _w: &Words) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn remove_overrides(prefixes: &[PathBuf], w: &Words) -> Result<(), String> {
    for prefix in prefixes
        .iter()
        .filter(|prefix| prefix.join("user.reg").is_file())
    {
        crate::wine::remove_override(prefix).map_err(|e| registry_error(w, e))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn registry_error(w: &Words, error: crate::wine::RegistryError) -> String {
    match error {
        crate::wine::RegistryError::Running => w.wine_running.into(),
        crate::wine::RegistryError::Other(reason) => failure(w, &reason),
    }
}

fn place_error(w: &Words, error: &install::PlaceError) -> String {
    match error {
        install::PlaceError::OtherTool => w.other_tool.into(),
        install::PlaceError::InUse => w.in_use.into(),
        install::PlaceError::Other(reason) => failure(w, reason),
    }
}

fn exit_error(w: &Words, code: u32) -> String {
    match i32::try_from(code).unwrap_or(exit::FAILED) {
        exit::OTHER_TOOL => w.other_tool.into(),
        exit::IN_USE => w.in_use.into(),
        _ => failure(w, &format!("exit code {code}")),
    }
}
