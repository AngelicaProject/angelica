//! Failures of host operations as plugins receive them (`types.error`).
#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    InvalidArgument,
    NotFound,
    PermissionDenied,
    WrongContext,
    Unavailable,
    LimitExceeded,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub kind: Kind,
    pub message: String,
}

impl Error {
    pub fn new(kind: Kind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(Kind::InvalidArgument, message)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
