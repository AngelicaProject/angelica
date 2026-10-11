//! The installer's one window: it keeps its size from the first screen to
//! the last, in Angelica's light look (`docs/architecture/ui.md`).
use crate::{exit, games, install, payload, system, words::Words};
use egui::{Color32, CornerRadius, FontFamily, FontId, RichText, Sense, Stroke, vec2};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

const SIZE: [f32; 2] = [480.0, 300.0];

const WINDOW: Color32 = Color32::from_rgb(0xf6, 0xf3, 0xed);
const TEXT: Color32 = Color32::from_rgb(0x24, 0x21, 0x1c);
const SECONDARY: Color32 = Color32::from_rgb(0x6e, 0x67, 0x5c);
const MUTED: Color32 = Color32::from_rgb(0x9b, 0x94, 0x88);
const LINE: Color32 = Color32::from_rgb(0xe6, 0xdf, 0xd2);
const TRACK: Color32 = Color32::from_rgb(0xe7, 0xdf, 0xd1);
const ACTION: Color32 = Color32::from_rgb(0x7a, 0x5c, 0x2e);
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
    typing: bool,
    typed: String,
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
            .with_maximize_button(false)
            .with_icon(egui::IconData::default()),
        ..Default::default()
    };
    let games = if mode == Mode::Install {
        games::find()
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
                typing: games.is_empty(),
                games,
                chosen: 0,
                typed: String::new(),
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
            &include_bytes!("../../angelica-host/fonts/GolosText-Regular.ttf")[..],
        ),
        (
            "golos-medium",
            &include_bytes!("../../angelica-host/fonts/GolosText-Medium.ttf")[..],
        ),
        (
            "quicksand",
            &include_bytes!("../../angelica-host/fonts/Quicksand-SemiBold.ttf")[..],
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
    let font = if strong { medium(14.0) } else { sans(13.5) };
    let color = if enabled { color } else { MUTED };
    let response = ui.add(
        egui::Label::new(RichText::new(text).font(font).color(color)).sense(if enabled {
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

fn paragraph(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).font(sans(13.0)).color(color)).wrap());
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(medium(18.0)).color(TEXT));
    ui.add_space(6.0);
}

/// A label and its value on one line, between thin rules.
fn row(ui: &mut egui::Ui, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rule, CornerRadius::ZERO, LINE);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).font(sans(13.0)).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), value);
    });
    ui.add_space(8.0);
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
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(10.0, 4.0), Sense::hover());
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, Color32::from_rgb(0xc9, 0xb4, 0x8f)),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ACTION);
}

/// The game folder in a few words: its launcher and the end of its path.
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
    format!("{launcher}…\\{parent}")
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let work = self
            .work
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if matches!(work, Work::Busy { .. }) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        egui::Frame::new()
            .fill(WINDOW)
            .inner_margin(egui::Margin::symmetric(22, 18))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.label(
                    RichText::new("angelica")
                        .font(FontId::new(13.0, FontFamily::Name("wordmark".into())))
                        .color(MUTED),
                );
                ui.add_space(12.0);
                match (self.mode, &work) {
                    (
                        _,
                        Work::Busy {
                            step,
                            progress: fraction,
                        },
                    ) => self.busy(ui, step, *fraction),
                    (_, Work::Failed(reason)) => {
                        let reason = reason.clone();
                        self.failed(ui, &reason);
                    }
                    (Mode::Install, Work::Done) => self.done(ui),
                    (Mode::Uninstall, Work::Done) => self.removed(ui),
                    (Mode::Install, Work::Idle) => self.start(ui),
                    (Mode::Uninstall, Work::Idle) => self.ask_remove(ui),
                }
            });
    }
}

impl App {
    fn set(&self, work: Work) {
        *self.work.lock().unwrap_or_else(PoisonError::into_inner) = work;
    }

    /// The bottom of the window, with actions at the right.
    fn bottom(ui: &mut egui::Ui, actions: impl FnOnce(&mut egui::Ui)) {
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    actions(ui);
                });
            });
        });
    }

    fn game(&self) -> Option<PathBuf> {
        if self.typing {
            games::resolve(&self.typed)
        } else {
            self.games.get(self.chosen).cloned()
        }
    }

    fn start(&mut self, ui: &mut egui::Ui) {
        let w = self.words;
        if payload::PAYLOAD.is_none() || payload::release_key().is_none() {
            heading(ui, w.development);
            paragraph(ui, w.development_text, SECONDARY);
            Self::bottom(ui, |ui| {
                if word(ui, w.close, ACTION, false, true) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            return;
        }
        heading(ui, w.install_heading);
        paragraph(ui, w.install_text, SECONDARY);
        ui.add_space(14.0);
        let (games, chosen, typing) = (self.games.clone(), self.chosen, self.typing);
        let mut pick = None;
        let mut switch = false;
        row(ui, w.game, |ui| {
            if typing {
                ui.add(
                    egui::TextEdit::singleline(&mut self.typed)
                        .hint_text(w.game_hint)
                        .desired_width(260.0)
                        .font(sans(12.5)),
                );
            } else if games.len() > 1 {
                egui::ComboBox::from_id_salt("game")
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
                ui.label(
                    RichText::new(short(&games[chosen]))
                        .font(sans(13.0))
                        .color(TEXT),
                );
            }
        });
        if !typing {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                switch = word(ui, w.game_other, SECONDARY, false, true);
            });
        }
        if let Some(index) = pick {
            self.chosen = index;
        }
        if switch {
            self.typing = true;
        }
        if self.typing && !self.typed.is_empty() && self.game().is_none() {
            ui.label(RichText::new(w.game_none).font(sans(12.0)).color(MUTED));
        }
        let game = self.game();
        Self::bottom(ui, |ui| {
            if word(ui, w.install, ACTION, true, game.is_some())
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
                w.installing
            } else {
                w.removing
            },
        );
        ui.add_space(6.0);
        progress(ui, fraction);
        ui.add_space(10.0);
        paragraph(ui, step, SECONDARY);
    }

    fn done(&self, ui: &mut egui::Ui) {
        let w = self.words;
        heading(ui, w.done);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.label(
                RichText::new(w.done_text_before)
                    .font(sans(13.0))
                    .color(SECONDARY),
            );
            key(ui, "Insert");
            ui.label(
                RichText::new(w.done_text_after)
                    .font(sans(13.0))
                    .color(SECONDARY),
            );
        });
        ui.add_space(14.0);
        row(ui, w.updates, |ui| {
            ui.label(RichText::new(w.updates_value).font(sans(13.0)).color(TEXT));
        });
        Self::bottom(ui, |ui| {
            if word(ui, w.close, ACTION, true, true) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn failed(&mut self, ui: &mut egui::Ui, reason: &str) {
        let w = self.words;
        heading(ui, w.failed);
        paragraph(ui, reason, TEXT);
        Self::bottom(ui, |ui| {
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
        heading(ui, w.remove_heading);
        paragraph(ui, w.remove_text, SECONDARY);
        ui.add_space(14.0);
        let keep = self.keep;
        let mut chosen = None;
        row(ui, w.player_data, |ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            // Right to left: the second word first.
            for (value, text) in [(false, w.delete), (true, w.keep)] {
                let font = if keep == value {
                    medium(13.5)
                } else {
                    sans(13.5)
                };
                let color = if keep == value { TEXT } else { MUTED };
                if ui
                    .add(
                        egui::Label::new(RichText::new(text).font(font).color(color))
                            .sense(Sense::click()),
                    )
                    .clicked()
                {
                    chosen = Some(value);
                }
            }
        });
        if let Some(value) = chosen {
            self.keep = value;
        }
        Self::bottom(ui, |ui| {
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
        paragraph(ui, w.removed_text, SECONDARY);
        Self::bottom(ui, |ui| {
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
            step: words.step_game,
            progress: 0.3,
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
    report(w.step_files, 0.35);
    let installation = install::installation_dir();
    install::install_files(
        &installation,
        payload.version,
        payload.channel,
        &payload.files(),
    )
    .map_err(|e| failure(w, &e))?;
    let uninstaller = installation.join("angelica-setup.exe");
    if let Ok(program) = std::env::current_exe()
        && program != uninstaller
    {
        let _ = std::fs::copy(&program, &uninstaller);
    }
    if install::writable(game) {
        report(w.step_game, 0.7);
        install::place(game, &installation, payload.dinput8_dll).map_err(|e| place_error(w, &e))?;
    } else {
        report(w.step_admin, 0.7);
        let args = format!(
            "--place \"{}\" \"{}\"",
            game.display(),
            installation.display()
        );
        match system::run_elevated(&args) {
            Ok(0) => {}
            Ok(code) => return Err(exit_error(w, code)),
            Err(_) => return Err(w.declined.into()),
        }
    }
    install::record_game(&installation, game).map_err(|e| failure(w, &e))?;
    system::register(&uninstaller, payload.version).map_err(|e| failure(w, &e))?;
    report(w.step_files, 1.0);
    Ok(())
}

fn uninstall_all(keep: bool, w: &Words) -> Result<(), String> {
    let installation = std::env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(std::path::Path::to_path_buf))
        .filter(|folder| folder.join("current.json").exists())
        .unwrap_or_else(install::installation_dir);
    if let Some(game) = install::recorded_game(&installation) {
        if install::writable(&game) {
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
