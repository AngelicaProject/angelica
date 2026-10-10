use std::path::{Path, PathBuf};

/// Why a map cannot be read. A map with any error is refused as a whole.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MapError {
    #[error("{path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("{path}: {message}")]
    Toml { path: PathBuf, message: String },
    #[error("map.toml: format {found:?} is not supported; this reader supports format {supported}")]
    Format { found: Option<i64>, supported: i64 },
    #[error("{path}: only `.toml` files belong in `entries/`, `sheets/`, `state/`, and `windows/`")]
    UnexpectedFile { path: PathBuf },
    #[error("{path}: `{name}` is not a valid entry name")]
    InvalidName { path: PathBuf, name: String },
    #[error("{path}: `{name}` does not belong to the area of this file")]
    WrongArea { path: PathBuf, name: String },
    #[error("`{name}` is defined more than once")]
    Duplicate { name: String },
    #[error("`{name}`: {message}")]
    InvalidEntry { name: String, message: String },
    #[error("`{name}` refers to `{reference}`, which is {problem}")]
    BadReference {
        name: String,
        reference: String,
        problem: String,
    },
    #[error("entries depend on each other in a cycle: {}", names.join(" -> "))]
    Cycle { names: Vec<String> },
    #[error("{path}: {message}")]
    InvalidSheet { path: PathBuf, message: String },
    #[error("{path}: {message}")]
    InvalidState { path: PathBuf, message: String },
    #[error("{path}: {message}")]
    InvalidWindow { path: PathBuf, message: String },
}

impl MapError {
    /// A stable identifier of the kind of error, for machine-readable output.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::Toml { .. } => "toml",
            Self::Format { .. } => "format",
            Self::UnexpectedFile { .. } => "unexpected-file",
            Self::InvalidName { .. } => "invalid-name",
            Self::WrongArea { .. } => "wrong-area",
            Self::Duplicate { .. } => "duplicate",
            Self::InvalidEntry { .. } => "invalid-entry",
            Self::BadReference { .. } => "bad-reference",
            Self::Cycle { .. } => "cycle",
            Self::InvalidSheet { .. } => "invalid-sheet",
            Self::InvalidState { .. } => "invalid-state",
            Self::InvalidWindow { .. } => "invalid-window",
        }
    }

    /// The file the error is about, when there is one.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Io { path, .. }
            | Self::Toml { path, .. }
            | Self::UnexpectedFile { path }
            | Self::InvalidName { path, .. }
            | Self::WrongArea { path, .. }
            | Self::InvalidSheet { path, .. }
            | Self::InvalidState { path, .. }
            | Self::InvalidWindow { path, .. } => Some(path),
            _ => None,
        }
    }

    /// The entry the error is about, when there is one.
    #[must_use]
    pub fn entry(&self) -> Option<&str> {
        match self {
            Self::InvalidName { name, .. }
            | Self::WrongArea { name, .. }
            | Self::Duplicate { name }
            | Self::InvalidEntry { name, .. }
            | Self::BadReference { name, .. } => Some(name),
            _ => None,
        }
    }
}
