//! Collects a local diagnostic bundle without loading any code from the game.
use crate::{install, system};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};

const LOG_LIMIT: u64 = 2 * 1024 * 1024;
const DUMP_LIMIT: u64 = 32 * 1024 * 1024;

struct Sources {
    game: Option<PathBuf>,
    root: PathBuf,
    logs: PathBuf,
    user_logs: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailureStage {
    Loading,
    Game,
}

impl FailureStage {
    fn label(self, russian: bool) -> &'static str {
        match (self, russian) {
            (Self::Loading, true) => "Ошибка при загрузке Angelica",
            (Self::Loading, false) => "Angelica startup failed",
            (Self::Game, true) => "Сбой во время работы игры",
            (Self::Game, false) => "The game encountered an error",
        }
    }
}

fn read(path: &Path, limit: u64, tail: bool) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || (!tail && meta.len() > limit) {
        return None;
    }
    let mut file = File::open(path).ok()?;
    if tail {
        file.seek(SeekFrom::Start(meta.len().saturating_sub(limit)))
            .ok()?;
    }
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn json_file(path: &Path) -> Option<Value> {
    serde_json::from_slice(&read(path, LOG_LIMIT, false)?).ok()
}

impl Sources {
    fn find(game: Option<PathBuf>) -> Self {
        let default = install::installation_dir();
        let game = game
            .or_else(|| install::recorded_game(&default))
            .or_else(|| {
                angelica_games::find()
                    .into_iter()
                    .find(|game| game.join("angelica.json").is_file())
            });
        let root = game.as_ref().map_or_else(
            || default.clone(),
            |game| {
                #[cfg(windows)]
                if let Some(path) = json_file(&game.join("angelica.json"))
                    .and_then(|config| config["installation_dir"].as_str().map(PathBuf::from))
                    .filter(|path| path.is_absolute())
                {
                    return path;
                }
                install::installation_for(game).0
            },
        );
        let logs = json_file(&root.join("host.json"))
            .and_then(|config| config["log_directory"].as_str().map(PathBuf::from))
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| root.join("logs"));
        let user_logs = (root != default).then(|| default.join("logs"));
        Self {
            game,
            root,
            logs,
            user_logs,
        }
    }

    fn summary(&self) -> Value {
        let game_version = self
            .game
            .as_ref()
            .and_then(|game| read(&game.join("ffxivgame.ver"), 128, false))
            .and_then(|bytes| String::from_utf8(bytes).ok());
        json!({
            "format": 1,
            "reporter_version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "runtime_version": json_file(&self.root.join("current.json")).map(|current| current["version"].clone()),
            "game_version": game_version.map(|version| version.trim().to_owned()),
            "loader_failure": self.loader_failure().and_then(|path| json_file(&path)),
            "crash": latest(&self.logs, "crash-").and_then(|path| json_file(&path)),
            "last_host_error": read(&self.logs.join("host.log"), LOG_LIMIT, true).and_then(|bytes| {
                String::from_utf8_lossy(&bytes).lines().rev().find(|line| {
                    ["startup failed:", "overlay unavailable:", "overlay disabled:"].iter().any(|text| line.contains(text))
                }).map(str::to_owned)
            }),
        })
    }

    fn loader_failure(&self) -> Option<PathBuf> {
        self.game
            .as_ref()
            .and_then(|game| latest(&game.join("angelica-reports"), "loader-"))
            .or_else(|| latest(&self.root.join("reports"), "loader-"))
            .or_else(|| latest(&install::installation_dir().join("reports"), "loader-"))
    }

    fn last_failure(&self) -> Option<FailureStage> {
        [
            (self.loader_failure(), FailureStage::Loading),
            (latest(&self.logs, "crash-"), FailureStage::Game),
        ]
        .into_iter()
        .filter_map(|(path, stage)| {
            let path = path?;
            json_file(&path)?;
            Some((fs::metadata(path).ok()?.modified().ok()?, stage))
        })
        .max_by_key(|(time, _)| *time)
        .map(|(_, stage)| stage)
    }

    fn bundle(&self, dump: bool) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![("summary.json".into(), pretty(&self.summary()).into_bytes())];
        let mut add_log = |path: &Path, name: &str| {
            if let Some(bytes) = read(path, LOG_LIMIT, true) {
                files.push((name.into(), redact(&bytes).into_bytes()));
            }
        };
        if let Some(game) = &self.game {
            add_log(&game.join("angelica-loader.log"), "angelica-loader.log");
            add_log(
                &game.join("angelica-loader.old.log"),
                "angelica-loader.old.log",
            );
        }
        let fallback = self.root.join("logs");
        add_log(
            &fallback.join("angelica-loader.log"),
            "loader-installation.log",
        );
        add_log(
            &fallback.join("angelica-loader.old.log"),
            "loader-installation.old.log",
        );
        if let Some(local) = &self.user_logs {
            add_log(&local.join("angelica-loader.log"), "loader-user.log");
            add_log(
                &local.join("angelica-loader.old.log"),
                "loader-user.old.log",
            );
        }
        add_log(&fallback.join("host-bootstrap.log"), "host-bootstrap.log");
        add_log(
            &fallback.join("host-bootstrap.old.log"),
            "host-bootstrap.old.log",
        );
        if self.logs != fallback {
            add_log(
                &self.logs.join("host-bootstrap.log"),
                "host-bootstrap-configured.log",
            );
        }
        add_log(&self.logs.join("host.log"), "host.log");
        add_log(&self.logs.join("host.old.log"), "host.old.log");
        if dump && let Some(path) = latest(&self.logs, "crash-") {
            match read(&path.with_extension("dmp"), DUMP_LIMIT, false) {
                Some(bytes) if bytes.starts_with(b"MDMP") => {
                    files.push(("crash.dmp".into(), bytes));
                }
                _ => files.push((
                    "minidump-status.txt".into(),
                    b"No readable minidump up to 32 MiB for the latest crash report.\n".to_vec(),
                )),
            }
        }
        files
    }
}

fn latest(folder: &Path, prefix: &str) -> Option<PathBuf> {
    fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if !name.starts_with(prefix)
                || !path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                || !entry.file_type().ok()?.is_file()
            {
                return None;
            }
            Some((entry.metadata().ok()?.modified().ok()?, path))
        })
        .max()
        .map(|(_, path)| path)
}

fn redact(bytes: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    for name in ["USERPROFILE", "HOME"] {
        if let Some(path) = std::env::var_os(name).filter(|path| !path.is_empty()) {
            let path = path.to_string_lossy();
            text = text.replace(path.as_ref(), "<profile>");
            text = text.replace(&path.replace('\\', "/"), "<profile>");
            text = text.replace(&path.replace('\\', "\\\\"), "<profile>");
        }
    }
    text
}

fn pretty(value: &Value) -> String {
    redact(
        serde_json::to_string_pretty(value)
            .unwrap_or_default()
            .as_bytes(),
    )
}

fn save(folder: &Path, sources: &Sources, dump: bool) -> io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = folder.join(format!(
        "angelica-report-{stamp}-{}.zip",
        std::process::id()
    ));
    let files = sources.bundle(dump);
    #[cfg(windows)]
    let files = {
        let mut files = files;
        if let Some(events) = system::game_events() {
            files.push((
                "windows-game-events.xml".into(),
                redact(&events).into_bytes(),
            ));
        }
        files
    };
    let mut out = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;
    let written = crate::report_zip::write(&mut out, &files).and_then(|()| out.sync_all());
    drop(out);
    if let Err(error) = written {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(path)
}

pub fn run(game: Option<PathBuf>) {
    let ru = system::russian();
    let title = if ru {
        "Отчёт Angelica"
    } else {
        "Angelica report"
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([540.0, 350.0])
            .with_min_inner_size([420.0, 310.0]),
        centered: true,
        ..Default::default()
    };
    let _ = eframe::run_native(
        title,
        options,
        Box::new(move |creation| {
            creation.egui_ctx.set_fonts(crate::app::fonts());
            let sources = Sources::find(game);
            let failure = sources.last_failure();
            Ok(Box::new(Report {
                sources,
                failure,
                dump: true,
                task: None,
                result: None,
                ru,
            }))
        }),
    );
}

struct Report {
    sources: Sources,
    failure: Option<FailureStage>,
    dump: bool,
    task: Option<mpsc::Receiver<io::Result<PathBuf>>>,
    result: Option<Result<String, String>>,
    ru: bool,
}

impl eframe::App for Report {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        use mpsc::TryRecvError;
        let words = |en, ru| if self.ru { ru } else { en };
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(result) => {
                    self.result = Some(
                        result
                            .map(|path| path.display().to_string())
                            .map_err(|e| e.to_string()),
                    );
                    self.task = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.result = Some(Err(words(
                        "The report could not be saved.",
                        "Не удалось сохранить отчёт.",
                    )
                    .into()));
                    self.task = None;
                }
                Err(TryRecvError::Empty) => ui
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(words("Game and Angelica diagnostics", "Диагностика игры и Angelica"));
            ui.label(words(
                "Save diagnostic files as a ZIP or copy a short summary. Everything stays on this computer.",
                "Сохраните диагностические файлы в ZIP или скопируйте короткую сводку. Данные остаются на этом компьютере.",
            ));
            if let Some(failure) = self.failure {
                ui.add_space(8.0);
                ui.strong(words("Last recorded failure", "Последняя сохранённая ошибка"));
                ui.label(failure.label(self.ru));
            }
            ui.add_space(12.0);
            let game = self.sources.game.as_ref().map_or_else(|| words("Game folder not found", "Папка игры не найдена").to_owned(), |game| game.display().to_string());
            ui.horizontal_wrapped(|ui| {
                ui.label(game);
                if ui.button(words("Choose game folder", "Выбрать папку игры")).clicked()
                    && let Some(folder) = system::pick_folder(system::foreground_window(), self.sources.game.as_deref()) {
                    if let Some(game) = angelica_games::resolve(&folder) {
                        self.sources = Sources::find(Some(game));
                        self.failure = self.sources.last_failure();
                    } else {
                        self.result = Some(Err(words("This folder does not contain the game.", "В этой папке нет игры.").into()));
                    }
                }
            });
            ui.add_space(8.0);
            ui.checkbox(&mut self.dump, words("Include the latest minidump", "Добавить последний минидамп"));
            ui.label(words("A minidump may contain fragments of game memory.", "В минидампе могут остаться фрагменты памяти игры."));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(self.task.is_none(), egui::Button::new(words("Save ZIP", "Сохранить ZIP"))).clicked()
                    && let Some(folder) = system::pick_folder(system::foreground_window(), None) {
                    let sources = Sources::find(self.sources.game.clone());
                    let dump = self.dump;
                    let (send, receive) = mpsc::channel();
                    self.task = Some(receive);
                    self.result = None;
                    let _ = std::thread::Builder::new().name("angelica-report".into()).spawn(move || {
                        let _ = send.send(save(&folder, &sources, dump));
                    });
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                }
                if ui.button(words("Copy summary", "Скопировать сводку")).clicked() {
                    ui.ctx().copy_text(pretty(&self.sources.summary()));
                    self.result = Some(Ok(words("Summary copied.", "Сводка скопирована.").into()));
                }
            });
            if let Some(result) = &self.result {
                ui.add_space(12.0);
                match result {
                    Ok(message) => { ui.label(message); }
                    Err(error) => { ui.colored_label(ui.visuals().error_fg_color, error); }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_combines_loader_and_host_data_and_only_includes_a_dump_when_asked() {
        let folder = tempfile::tempdir().unwrap();
        let root = folder.path().to_path_buf();
        let game = root.join("game");
        let logs = root.join("logs");
        for dir in [&game, &logs, &root.join("reports"), &root.join("storage")] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(root.join("current.json"), r#"{"version":"0.1.0"}"#).unwrap();
        fs::write(
            root.join("reports/loader-1.json"),
            r#"{"windows_error":127}"#,
        )
        .unwrap();
        fs::write(game.join("angelica-loader.log"), "LoadLibraryExW failed").unwrap();
        fs::write(logs.join("host.log"), "host started").unwrap();
        fs::write(logs.join("crash-1.json"), r#"{"dump_written":true}"#).unwrap();
        fs::write(logs.join("crash-1.dmp"), b"MDMPdata").unwrap();
        fs::write(root.join("storage/private.txt"), "never include this").unwrap();
        let sources = Sources {
            game: Some(game),
            root,
            logs,
            user_logs: None,
        };
        let summary = sources.summary();
        assert_eq!(summary["loader_failure"]["windows_error"], 127);
        assert_eq!(summary["crash"]["dump_written"], true);
        assert_eq!(summary["runtime_version"], "0.1.0");
        assert_eq!(
            sources
                .bundle(false)
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["summary.json", "angelica-loader.log", "host.log"]
        );
        assert_eq!(sources.bundle(true).last().unwrap().1, b"MDMPdata");
        OpenOptions::new()
            .write(true)
            .open(sources.logs.join("crash-1.json"))
            .unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(1))
            .unwrap();
        assert_eq!(sources.last_failure(), Some(FailureStage::Loading));
    }
}
