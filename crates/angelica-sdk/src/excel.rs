//! The game's sheets, read by column name.
//!
//! ```ignore
//! use angelica_sdk::excel::{self, ENGLISH};
//!
//! if let Some(values) = excel::row("Item", ENGLISH, &["name"], 4551)? {
//!     // The name as the player reads it, without the game's macros.
//!     let name = values[0].plain().unwrap_or_default();
//! }
//! // Row count, for progress while reading every row in a job.
//! let total = excel::sheet("Item")?.rows;
//! for row in excel::rows("Item", ENGLISH, &["name"]) {
//!     let row = row?;
//! }
//! ```
use crate::api::excel as api;
pub use api::{RowKey, SheetInfo, SheetRow, Value};

/// A sheet's layout, the columns the game map names, the languages it has
/// data for, and its row count.
///
/// # Errors
/// See `excel.sheet`.
pub fn sheet(name: &str) -> Result<SheetInfo, crate::Error> {
    api::sheet(name)
}

pub const JAPANESE: u8 = 1;
pub const ENGLISH: u8 = 2;
pub const GERMAN: u8 = 3;
pub const FRENCH: u8 = 4;

fn names(columns: &[&str]) -> Vec<String> {
    columns.iter().map(|name| (*name).to_owned()).collect()
}

/// The values of `columns` in a row (its first subrow), in the order given;
/// `None` when the sheet has no such row.
///
/// # Errors
/// See `excel.row`.
pub fn row(
    sheet: &str,
    language: u8,
    columns: &[&str],
    row: u32,
) -> Result<Option<Vec<Value>>, crate::Error> {
    api::row(sheet, language, &names(columns), RowKey { row, subrow: 0 })
        .map(|found| found.map(|row| row.values))
}

/// The text of a text column in a row as the game shows it now: with a
/// translation a plugin put there, or the game's own text. For showing to
/// the player only; `None` when the sheet has no such row.
///
/// # Errors
/// See `excel.shown`.
pub fn shown(
    sheet: &str,
    language: u8,
    column: &str,
    row: u32,
) -> Result<Option<String>, crate::Error> {
    api::shown(sheet, language, column, RowKey { row, subrow: 0 })
        .map(|found| found.map(|text| crate::text::sestring::plain(&text)))
}

/// Every row of a sheet with the values of `columns`, read page by page.
#[must_use]
pub fn rows(sheet: &str, language: u8, columns: &[&str]) -> Rows {
    Rows {
        sheet: sheet.to_owned(),
        language,
        columns: names(columns),
        after: None,
        page: Vec::new().into_iter(),
        done: false,
    }
}

/// The rows of a sheet; see [`rows`].
pub struct Rows {
    sheet: String,
    language: u8,
    columns: Vec<String>,
    after: Option<RowKey>,
    page: std::vec::IntoIter<SheetRow>,
    done: bool,
}

impl Iterator for Rows {
    type Item = Result<SheetRow, crate::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(row) = self.page.next() {
            return Some(Ok(row));
        }
        if self.done {
            return None;
        }
        match api::rows(&self.sheet, self.language, &self.columns, self.after, 1024) {
            Ok(page) => {
                self.done = page.len() < 1024;
                self.after = page.last().map(|row| row.key);
                self.page = page.into_iter();
                self.page.next().map(Ok)
            }
            Err(error) => {
                self.done = true;
                Some(Err(error))
            }
        }
    }
}

impl Value {
    /// A text value: the game's string, macros encoded.
    #[must_use]
    pub fn text(&self) -> Option<&[u8]> {
        match self {
            Self::Text(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// A text value as the player reads it, without macros (see
    /// [`crate::text::sestring::plain`]).
    #[must_use]
    pub fn plain(&self) -> Option<String> {
        self.text().map(crate::text::sestring::plain)
    }

    /// An unsigned, or a non-negative signed, value.
    #[must_use]
    pub fn unsigned(&self) -> Option<u64> {
        match *self {
            Self::Unsigned(value) => Some(value),
            Self::Signed(value) => u64::try_from(value).ok(),
            _ => None,
        }
    }

    /// A signed, or a small enough unsigned, value.
    #[must_use]
    pub fn signed(&self) -> Option<i64> {
        match *self {
            Self::Signed(value) => Some(value),
            Self::Unsigned(value) => i64::try_from(value).ok(),
            _ => None,
        }
    }

    #[must_use]
    pub fn boolean(&self) -> Option<bool> {
        match *self {
            Self::Boolean(value) => Some(value),
            _ => None,
        }
    }

    #[must_use]
    pub fn float(&self) -> Option<f32> {
        match *self {
            Self::Float(value) => Some(value),
            _ => None,
        }
    }
}
