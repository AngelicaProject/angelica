//! The game map: entries that describe where functions, data, and fields are
//! in a game build, and their resolution against a PE image.
//!
//! The format is specified in `docs/formats/game-map-v1.md`. The same
//! resolution runs offline over the executable file and in the game over the
//! mapped module, so both always agree on what an entry means.

mod error;
mod load;
mod model;
mod record;
mod resolve;
mod sheets;
mod state;
mod windows;

pub use error::MapError;
pub use load::{FORMAT, Map};
pub use model::{Abi, Check, Entry, EntryKind, Follow, Locator, Origin, Thread, ValueKind};
pub use record::BuildRecord;
pub use resolve::{Absence, Resolution, Value};
pub use sheets::{ColumnType, KnownLayout, NamedColumn, SheetLayout, SheetSchema};
pub use state::{INDEX, Representation, StateField, StateList, StateTable};
pub use windows::{
    ElementKind, KnownWindowLayout, NamedElement, WindowSchema, window_layout_fingerprint,
};
