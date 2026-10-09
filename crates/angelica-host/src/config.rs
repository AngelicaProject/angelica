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
