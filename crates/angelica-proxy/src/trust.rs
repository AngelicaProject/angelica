//! Configuration and signature checks without platform dependencies.
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Loader failures leave the game running without the host.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("angelica.json: {0}")]
    Config(#[from] serde_json::Error),
    #[error("current.json: {0}")]
    Current(String),
    #[error("installation_dir must be an absolute path")]
    RelativeInstallation,
    #[error("the proxy has no compiled release public key")]
    NoReleaseKey,
    #[error("invalid Ed25519 release public key")]
    InvalidKey,
    #[error("runtime.dll.sig must contain a raw 64-byte Ed25519 signature")]
    SignatureSize,
    #[error("runtime.dll Ed25519 signature verification failed")]
    Signature,
    #[error("{0}")]
    Platform(String),
}

/// Installation location. Its `current.json` names the version folder under
/// `versions/` that holds the runtime; without it, the runtime is in the
/// installation folder itself.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub installation_dir: PathBuf,
}

impl Config {
    /// Reads strict JSON configuration next to the proxy.
    ///
    /// # Errors
    /// Returns an error for missing or malformed configuration or a relative path.
    pub fn read(directory: &Path) -> Result<Self, LoadError> {
        let path = directory.join("angelica.json");
        let config: Self = serde_json::from_slice(&read(&path)?)?;
        if !config.installation_dir.is_absolute() {
            return Err(LoadError::RelativeInstallation);
        }
        Ok(config)
    }

    /// The runtime of the version `current.json` names. An update writes a
    /// new version folder and then replaces `current.json`, so the running
    /// version is never touched. A present but broken `current.json` is
    /// refused, never silently skipped for an older runtime.
    ///
    /// # Errors
    /// A malformed `current.json`, and filesystem errors other than its
    /// absence.
    pub fn runtime_path(&self) -> Result<PathBuf, LoadError> {
        let path = self.installation_dir.join("current.json");
        match fs::read(&path) {
            Ok(bytes) => {
                let current: Current = serde_json::from_slice(&bytes)
                    .map_err(|e| LoadError::Current(e.to_string()))?;
                if current.format != 1 || !version_name(&current.version) {
                    return Err(LoadError::Current(format!(
                        "format {} with version {:?}",
                        current.format, current.version
                    )));
                }
                Ok(self
                    .installation_dir
                    .join("versions")
                    .join(current.version)
                    .join("runtime.dll"))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(self.installation_dir.join("runtime.dll"))
            }
            Err(source) => Err(LoadError::Io { path, source }),
        }
    }
}

/// `current.json`: `{"format": 1, "version": "0.2.0"}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    format: u32,
    version: String,
}

/// A version folder's name: a `SemVer` version or another plain name, never a
/// path.
fn version_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>, LoadError> {
    fs::read(path).map_err(|source| LoadError::Io {
        path: path.to_owned(),
        source,
    })
}

/// Verifies exactly the runtime bytes that will be loaded.
///
/// # Errors
/// Returns an error for an absent/invalid key, malformed signature, or mismatch.
pub fn verify(runtime: &[u8], signature: &[u8], key: Option<&[u8; 32]>) -> Result<(), LoadError> {
    let key = VerifyingKey::from_bytes(key.ok_or(LoadError::NoReleaseKey)?)
        .map_err(|_| LoadError::InvalidKey)?;
    let signature = Signature::from_slice(signature).map_err(|_| LoadError::SignatureSize)?;
    key.verify_strict(runtime, &signature)
        .map_err(|_| LoadError::Signature)
}
