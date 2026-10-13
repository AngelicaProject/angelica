#![forbid(unsafe_code)]
use crate::sandbox::Limits;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub format: u32,
    pub map_directory: PathBuf,
    pub plugin_directory: PathBuf,
    pub storage_directory: PathBuf,
    pub grants_directory: PathBuf,
    pub log_directory: PathBuf,
    #[serde(default)]
    pub state_directory: Option<PathBuf>,
    #[serde(default)]
    pub cache_directory: Option<PathBuf>,
    #[serde(default)]
    pub keys_directory: Option<PathBuf>,
    #[serde(default = "memory_default")]
    pub memory_bytes: u64,
    #[serde(default = "fuel_default")]
    pub fuel: u64,
    #[serde(default = "time_default")]
    pub timeout_ms: u64,
}
fn memory_default() -> u64 {
    Limits::default().memory_bytes
}
fn fuel_default() -> u64 {
    Limits::default().fuel
}
fn time_default() -> u64 {
    u64::try_from(Limits::default().call_time.as_millis()).unwrap_or(u64::MAX)
}

impl Config {
    /// # Errors
    /// Refuses malformed configuration, relative paths and unbounded plugin limits.
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("host.json");
        let config = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<Self>(&bytes).map_err(|e| format!("host.json: {e}"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self {
                format: 1,
                map_directory: root.join("map"),
                plugin_directory: root.join("plugins"),
                storage_directory: root.join("storage"),
                grants_directory: root.join("grants"),
                log_directory: root.join("logs"),
                state_directory: None,
                cache_directory: None,
                keys_directory: None,
                memory_bytes: memory_default(),
                fuel: fuel_default(),
                timeout_ms: time_default(),
            },
            Err(e) => return Err(format!("host.json: {e}")),
        };
        if config.format != 1
            || [
                &config.state_directory,
                &config.cache_directory,
                &config.keys_directory,
            ]
            .iter()
            .any(|path| path.as_ref().is_some_and(|path| !path.is_absolute()))
            || [
                &config.map_directory,
                &config.plugin_directory,
                &config.storage_directory,
                &config.grants_directory,
                &config.log_directory,
            ]
            .iter()
            .any(|path| !path.is_absolute())
            || config.memory_bytes == 0
            || config.memory_bytes > crate::manifest::MAX_MEMORY_BYTES
            || config.fuel == 0
            || config.fuel > 10_000_000_000
            || config.timeout_ms == 0
            || config.timeout_ms > 1000
        {
            return Err(
                "host.json has an unsupported format, relative path or invalid resource limit"
                    .into(),
            );
        }
        Ok(config)
    }
    #[must_use]
    pub fn limits(&self) -> Limits {
        Limits {
            memory_bytes: self.memory_bytes,
            fuel: self.fuel,
            call_time: std::time::Duration::from_millis(self.timeout_ms),
            ..Limits::default()
        }
    }
}

/// How much of the overlay runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlayMode {
    /// The menu and plugin windows are drawn.
    #[default]
    On,
    /// `Present` is hooked and passed on; nothing is made or drawn.
    Hook,
    /// No Direct3D device, no hook.
    Off,
}

fn yes() -> bool {
    true
}

/// Parts of Angelica the player turns off to find which one a problem
/// comes from: `diagnostics.json` beside `host.json`. Without the file
/// everything runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostics {
    #[serde(default)]
    pub overlay: OverlayMode,
    /// Angelica, map, plugin, and catalog checks over the network.
    #[serde(default = "yes")]
    pub updates: bool,
    /// Loading installed plugins.
    #[serde(default = "yes")]
    pub plugins: bool,
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            overlay: OverlayMode::On,
            updates: true,
            plugins: true,
        }
    }
}

impl Diagnostics {
    /// `diagnostics.json` in `root`, with what is wrong with it; a file that
    /// cannot be read turns nothing off.
    #[must_use]
    pub fn load(root: &Path) -> (Self, Option<String>) {
        match std::fs::read(root.join("diagnostics.json")) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(diagnostics) => (diagnostics, None),
                Err(e) => (Self::default(), Some(format!("diagnostics.json: {e}"))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Self::default(), None),
            Err(e) => (Self::default(), Some(format!("diagnostics.json: {e}"))),
        }
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn parts_turn_off_one_by_one_and_a_bad_file_turns_nothing_off() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            Diagnostics::load(folder.path()),
            (Diagnostics::default(), None)
        );
        std::fs::write(
            folder.path().join("diagnostics.json"),
            r#"{"overlay": "hook"}"#,
        )
        .unwrap();
        let (diagnostics, problem) = Diagnostics::load(folder.path());
        assert_eq!(problem, None);
        assert_eq!(diagnostics.overlay, OverlayMode::Hook);
        assert!(diagnostics.updates && diagnostics.plugins);
        std::fs::write(
            folder.path().join("diagnostics.json"),
            r#"{"overlay": "off", "updates": false, "plugins": false}"#,
        )
        .unwrap();
        let (diagnostics, _) = Diagnostics::load(folder.path());
        assert_eq!(diagnostics.overlay, OverlayMode::Off);
        assert!(!diagnostics.updates && !diagnostics.plugins);
        std::fs::write(
            folder.path().join("diagnostics.json"),
            r#"{"overlay": "half"}"#,
        )
        .unwrap();
        let (diagnostics, problem) = Diagnostics::load(folder.path());
        assert_eq!(diagnostics, Diagnostics::default());
        assert!(problem.is_some());
    }
}
