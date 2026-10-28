//! The game map: entries that describe where functions, data, and fields are
//! in a game build, and their resolution against a PE image.
//!
//! The format is specified in `docs/formats/game-map-v2.md`. The same
//! resolution runs offline over the executable file and in the game over the
//! mapped module, so both always agree on what an entry means.

mod actions;
mod code;
mod error;
mod events;
mod load;
mod model;
mod objects;
mod record;
mod resolve;
mod sheets;
mod whose;
mod windows;

pub use actions::{
    ActArg, Action, Condition as ActCondition, MOST_ACT_RATE, Number, Param, Source as ActSource,
};
pub use code::{FunctionCode, function_code, function_holding, leaf_function};
pub use error::MapError;
pub use events::{
    Arg, DERIVATIONS, DerivedValue, Event, MOST_OUTCOME_TEXT, MOST_RATE, Outcome, RULE_FORMATS,
    Register, Source as ArgSource, derivation,
};
pub use load::{FORMAT, Map, ROOT_FILES};
pub use model::{Abi, Check, Entry, EntryKind, Follow, Locator, Origin, Thread, ValueKind};
pub use objects::{
    Array, Bit, Domain, DomainClass, Enum, Field, Length, MOST_ELEMENTS, MOST_TEXT, Meaning,
    Objects, Partition, Root, Settable, Shape, Type, Variant, Wording,
};
pub use record::BuildRecord;
pub use resolve::{Absence, Resolution, Value};
pub use sheets::{
    ColumnType, KnownLayout, NamedColumn, SheetLayout, SheetSchema, parse as parse_sheet_schema,
};
pub use whose::{Entering, Renames, Whose};
pub use windows::{
    ElementKind, KnownWindowLayout, NamedElement, WindowSchema, parse as parse_window_schema,
    window_layout_fingerprint,
};
