//! Dev mode: work on the game map on the player's own Angelica
//! (`docs/formats/host-installation-v1.md#dev-mode`). While `dev/mode.json`
//! says it is on, `angelica look` reads the running game, and a local map in
//! `dev/map/` takes the signed map's place. It never changes what a plugin
//! may do, and no plugin reaches the `dev` folder.
#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// The folder of dev mode in the installation.
pub const FOLDER: &str = "dev";
/// Whether dev mode is on.
pub const MODE: &str = "mode.json";
/// The local map.
pub const MAP: &str = "map";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModeFile {
    format: u32,
    on: bool,
}

/// Whether `folder` says dev mode is on: `mode.json` is `{"format": 1,
/// "on": true}`. A missing or unreadable file, or anything else in it, is
/// off.
#[must_use]
pub fn read(folder: &Path) -> bool {
    std::fs::read(folder.join(MODE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ModeFile>(&bytes).ok())
        .is_some_and(|mode| mode.format == 1 && mode.on)
}

/// Writes `mode.json` in `folder` through a temporary file and a rename.
///
/// # Errors
/// The operating system's message.
pub fn write(folder: &Path, on: bool) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let bytes = serde_json::to_vec(&ModeFile { format: 1, on }).map_err(|e| e.to_string())?;
    crate::look::write_atomically(&folder.join(MODE), &bytes)
}

/// The host's view of dev mode, read again every second.
pub struct DevMode {
    folder: PathBuf,
    on: AtomicBool,
    checked: Mutex<Instant>,
}

impl DevMode {
    const INTERVAL: Duration = Duration::from_secs(1);

    #[must_use]
    pub fn new(installation: &Path) -> Self {
        let folder = installation.join(FOLDER);
        Self {
            on: AtomicBool::new(read(&folder)),
            folder,
            checked: Mutex::new(Instant::now()),
        }
    }

    #[must_use]
    pub fn on(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    /// Reads `mode.json` again when a second has passed; a line for the
    /// host's log when dev mode turned on or off.
    pub fn poll(&self) -> Option<String> {
        {
            let mut checked = self.checked.lock().unwrap_or_else(PoisonError::into_inner);
            if checked.elapsed() < Self::INTERVAL {
                return None;
            }
            *checked = Instant::now();
        }
        let on = read(&self.folder);
        (self.on.swap(on, Ordering::AcqRel) != on).then(|| Self::line(on))
    }

    /// Turns dev mode on or off, as the switch under For developers does.
    ///
    /// # Errors
    /// The operating system's message; dev mode stays as it was.
    pub fn set(&self, on: bool) -> Result<String, String> {
        write(&self.folder, on)?;
        self.on.store(on, Ordering::Release);
        Ok(Self::line(on))
    }

    /// The local map's folder, while dev mode is on and a local map is
    /// there.
    #[must_use]
    pub fn local_map(&self) -> Option<PathBuf> {
        let map = self.folder.join(MAP);
        (self.on() && map.join("files.json").is_file()).then_some(map)
    }

    fn line(on: bool) -> String {
        if on {
            "dev mode turned on".into()
        } else {
            "dev mode turned off".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_format_1_with_on_true_is_on() {
        let folder = tempfile::tempdir().unwrap();
        assert!(!read(folder.path()), "no file is off");
        for (text, on) in [
            (r#"{"format": 1, "on": true}"#, true),
            (r#"{"format": 1, "on": false}"#, false),
            (r#"{"format": 2, "on": true}"#, false),
            (r#"{"format": 1, "on": true, "more": 1}"#, false),
            ("on", false),
        ] {
            std::fs::write(folder.path().join(MODE), text).unwrap();
            assert_eq!(read(folder.path()), on, "{text}");
        }
    }

    #[test]
    fn the_switch_writes_what_the_poll_reads() {
        let installation = tempfile::tempdir().unwrap();
        let dev = DevMode::new(installation.path());
        assert!(!dev.on());
        assert_eq!(dev.set(true).unwrap(), "dev mode turned on");
        assert!(read(&installation.path().join(FOLDER)));
        assert!(dev.local_map().is_none(), "no local map yet");

        let map = installation.path().join(FOLDER).join(MAP);
        std::fs::create_dir_all(&map).unwrap();
        std::fs::write(map.join("files.json"), "{}").unwrap();
        assert_eq!(dev.local_map(), Some(map));

        write(&installation.path().join(FOLDER), false).unwrap();
        *dev.checked.lock().unwrap() = Instant::now().checked_sub(DevMode::INTERVAL).unwrap();
        assert_eq!(dev.poll().as_deref(), Some("dev mode turned off"));
        assert!(
            dev.local_map().is_none(),
            "the local map is used only in dev mode"
        );
    }
}
