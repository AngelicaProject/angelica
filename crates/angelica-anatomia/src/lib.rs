//! Anatomia: the description of the game, what it contains and how to find
//! each part in the running build, and the resolution of its entries
//! against a PE image.
//!
//! The format is specified in `docs/formats/anatomia-1.md`. An entry that
//! cannot be read, refers to what does not exist, or does not resolve is
//! absent with its reason; it never makes the rest of the description
//! invalid. The Lens catalog beside it says what Lens offers over it.

mod catalog;
mod convert;
mod load;
mod meaning;
mod model;
mod record;
mod resolve;
mod table;
mod ty;

use std::path::PathBuf;

pub use catalog::{Catalog, CatalogItem};
pub use convert::{Converted, convert_map, convert_sources};
pub use load::Anatomia;
pub use meaning::{Domain, FieldMeaning, Meaning, Partition, Wording};
pub use model::{
    Check, Entry, Enum, Kind, OperandKind, Place, Slot, Step, Struct, StructField, Text, Thread,
    TypeDef,
};
pub use record::BuildRecord;
pub use resolve::{Absence, Resolution, Value};
pub use ty::{FnType, Param, Primitive, Ty};

/// The major version of the format this reader reads.
pub const FORMAT_MAJOR: u32 = 1;
/// The newest minor version this reader knows every key of.
pub const FORMAT_MINOR: u32 = 0;

/// Why a description cannot be read at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnatomiaError {
    #[error("{path}: {message}")]
    Io { path: PathBuf, message: String },
    #[error("{path}: {message}")]
    Toml { path: PathBuf, message: String },
    #[error("anatomia.toml: {message}")]
    Manifest { message: String },
    #[error("anatomia.toml: format {found} is not supported; this reader reads {supported}")]
    Format { found: String, supported: String },
    #[error("{path}: no file belongs here")]
    UnexpectedFile { path: PathBuf },
}

impl AnatomiaError {
    /// A stable identifier of the kind of error, for machine-readable output.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::Toml { .. } => "toml",
            Self::Manifest { .. } => "manifest",
            Self::Format { .. } => "format",
            Self::UnexpectedFile { .. } => "unexpected-file",
        }
    }
}
