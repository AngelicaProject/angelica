//! Values saved as JSON files in the plugin's folder, such as its settings.
//!
//! ```ignore
//! use angelica_sdk::storage;
//!
//! #[derive(Default, serde::Serialize, serde::Deserialize)]
//! struct Settings {
//!     greeting: String,
//! }
//!
//! // In `start`: the saved settings, or the defaults the first time.
//! let settings: Settings = storage::load("settings.json")?.unwrap_or_default();
//! // After the player changes them:
//! storage::save("settings.json", &settings)?;
//! ```
//!
//! [`crate::api::storage`] reads and writes bytes.
use serde::{Serialize, de::DeserializeOwned};

use crate::api::storage as api;

/// The value saved in `path`, or `None` when the file does not exist.
///
/// # Errors
/// See `storage.read`; `invalid-argument` when the file is not JSON of a
/// `T`.
pub fn load<T: DeserializeOwned>(path: &str) -> Result<Option<T>, crate::Error> {
    let bytes = match api::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind == crate::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| crate::Error {
            kind: crate::ErrorKind::InvalidArgument,
            message: format!("{path} is not the expected JSON: {e}"),
        })
}

/// Saves `value` in `path` as JSON, replacing the file atomically.
///
/// # Errors
/// See `storage.write`.
pub fn save(path: &str, value: &impl Serialize) -> Result<(), crate::Error> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| crate::Error {
        kind: crate::ErrorKind::InvalidArgument,
        message: format!("{path}: {e}"),
    })?;
    api::write(path, &bytes)
}
