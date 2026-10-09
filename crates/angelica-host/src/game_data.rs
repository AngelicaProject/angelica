//! The installed game's files and Excel sheets (the `game-files` and
//! `excel` interfaces), read from its archives with `angelica-sqpack` and
//! kept in a bounded cache. Never written to.
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
use angelica_sqpack::{
    GameData as Archives, SqPackError,
    excel::{self, ColumnKind, Language, Sheet, SheetError},
};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

/// The largest file `read` returns.
pub const MAX_FILE: usize = 64 * 1024 * 1024;
/// The most rows one `rows` call returns.
pub const MAX_ROWS: u32 = 1024;
/// Sheets kept decoded at once.
const CACHED_SHEETS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Boolean(bool),
    Signed(i64),
    Unsigned(u64),
    Float(f32),
}

/// A cell: text bytes or a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Text(Vec<u8>),
    Value(Value),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub row: u32,
    pub subrow: u16,
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetInfo {
    pub name: String,
    pub subrows: bool,
    /// Kind and offset of each column, in column order.
    pub columns: Vec<(ColumnKind, u16)>,
    /// The languages the installed game has data for; the header may list
    /// more, such as other regions' languages.
    pub languages: Vec<u8>,
    pub rows: u32,
}

impl SheetInfo {
    /// The sheet's layout as the game map describes layouts.
    #[must_use]
    pub fn layout(&self) -> angelica_map::SheetLayout {
        use angelica_map::ColumnType;
        angelica_map::SheetLayout {
            subrows: self.subrows,
            columns: self
                .columns
                .iter()
                .map(|(kind, offset)| {
                    let column_type = match kind {
                        ColumnKind::String => ColumnType::Text,
                        ColumnKind::Bool => ColumnType::Bool,
                        ColumnKind::Int8 => ColumnType::Int8,
                        ColumnKind::UInt8 => ColumnType::Uint8,
                        ColumnKind::Int16 => ColumnType::Int16,
                        ColumnKind::UInt16 => ColumnType::Uint16,
                        ColumnKind::Int32 => ColumnType::Int32,
                        ColumnKind::UInt32 => ColumnType::Uint32,
                        ColumnKind::Float32 => ColumnType::Float32,
                        ColumnKind::Int64 => ColumnType::Int64,
                        ColumnKind::UInt64 => ColumnType::Uint64,
                        ColumnKind::PackedBool(bit) => ColumnType::PackedBool(*bit),
                    };
                    (column_type, *offset)
                })
                .collect(),
        }
    }
}

/// `(row, subrow)` of each row, sorted, with its position in `Sheet::rows`.
type Order = Arc<Vec<((u32, u16), usize)>>;

struct Cached {
    name: String,
    language: Language,
    sheet: Arc<Sheet>,
    order: Order,
}

/// The game's archives, opened on first use.
pub struct GameData {
    root: Option<std::path::PathBuf>,
    archives: OnceLock<Archives>,
    /// Why the archives last failed to open, and when; opening is tried
    /// again after [`RETRY_OPEN`].
    failed: Mutex<Option<(std::time::Instant, String)>>,
    sheets: Mutex<VecDeque<Cached>>,
}

/// How long a failure to open the archives is reported before opening is
/// tried again.
const RETRY_OPEN: std::time::Duration = std::time::Duration::from_secs(5);

fn language(code: u8) -> Result<Language> {
    Ok(match code {
        0 => Language::None,
        1 => Language::Japanese,
        2 => Language::English,
        3 => Language::German,
        4 => Language::French,
        5 => Language::ChineseSimplified,
        6 => Language::ChineseTraditional,
        7 => Language::Korean,
        8 => Language::TraditionalChinese,
        _ => return Err(Error::invalid(format!("{code} is not a game language"))),
    })
}

fn archive_error(error: &SqPackError) -> Error {
    Error::new(Kind::Io, error.to_string())
}

fn sheet_error(error: SheetError) -> Error {
    match error {
        SheetError::NotFound(_) | SheetError::NotInstalled { .. } => {
            Error::new(Kind::NotFound, error.to_string())
        }
        SheetError::Game(error) => archive_error(&error),
        _ => Error::invalid(error.to_string()),
    }
}

impl GameData {
    /// The installation whose `game/sqpack` holds the archives; `None` when
    /// the host has no installation, and every call fails.
    #[must_use]
    pub fn new(root: Option<&Path>) -> Self {
        Self {
            root: root.map(Path::to_path_buf),
            archives: OnceLock::new(),
            failed: Mutex::new(None),
            sheets: Mutex::new(VecDeque::new()),
        }
    }

    fn archives(&self) -> Result<&Archives> {
        if let Some(archives) = self.archives.get() {
            return Ok(archives);
        }
        let Some(root) = &self.root else {
            return Err(Error::new(
                Kind::Unavailable,
                "the game's files are not available",
            ));
        };
        let mut failed = self.failed.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((at, reason)) = failed.as_ref()
            && at.elapsed() < RETRY_OPEN
        {
            return Err(Error::new(Kind::Unavailable, reason.clone()));
        }
        match Archives::open(root) {
            Ok(archives) => {
                *failed = None;
                Ok(self.archives.get_or_init(|| archives))
            }
            Err(error) => {
                let reason = error.to_string();
                *failed = Some((std::time::Instant::now(), reason.clone()));
                Err(Error::new(Kind::Unavailable, reason))
            }
        }
    }

    /// # Errors
    /// Fails for an unreadable archive.
    pub fn exists(&self, path: &str) -> Result<bool> {
        // Reading is the only way to find a file in the index and check it.
        Ok(self
            .archives()?
            .file(path)
            .map_err(|e| archive_error(&e))?
            .is_some())
    }

    /// # Errors
    /// Fails for a missing file, an unreadable archive, an unsupported file
    /// kind, and files over 64 MiB.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let bytes = self
            .archives()?
            .file(path)
            .map_err(|e| match e {
                SqPackError::Invalid(message) => Error::invalid(format!("{path}: {message}")),
                other => archive_error(&other),
            })?
            .ok_or_else(|| {
                Error::new(
                    Kind::NotFound,
                    format!("{path} is not in the game's archives"),
                )
            })?;
        if bytes.len() > MAX_FILE {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("{path} is larger than 64 MiB"),
            ));
        }
        Ok(bytes)
    }

    /// # Errors
    /// Fails for a missing or malformed sheet list.
    pub fn sheets(&self) -> Result<Vec<String>> {
        excel::sheet_names(self.archives()?).map_err(|e| archive_error(&e))
    }

    /// # Errors
    /// Fails for a sheet without a header and an unsupported header.
    pub fn sheet(&self, name: &str) -> Result<SheetInfo> {
        let header = excel::sheet_header(self.archives()?, name)
            .map_err(sheet_error)?
            .ok_or_else(|| Error::new(Kind::NotFound, format!("the game has no sheet {name}")))?;
        let subrows = match header.variant {
            1 => false,
            2 => true,
            other => {
                return Err(Error::invalid(format!(
                    "sheet {name} has the unsupported variant {other}"
                )));
            }
        };
        let columns = header
            .columns
            .iter()
            .map(|&(code, offset)| {
                excel::ColumnKind::from_code(code)
                    .map(|kind| (kind, offset))
                    .ok_or_else(|| {
                        Error::invalid(format!(
                            "sheet {name} has the unsupported column type {code:#x}"
                        ))
                    })
            })
            .collect::<Result<_>>()?;
        Ok(SheetInfo {
            name: name.to_owned(),
            subrows,
            columns,
            languages: excel::installed_languages(self.archives()?, name, &header)
                .map_err(|e| archive_error(&e))?,
            rows: header.row_count,
        })
    }

    /// The decoded sheet, from the cache or the archives.
    fn cached(&self, name: &str, code: u8) -> Result<(Arc<Sheet>, Order)> {
        let requested = language(code)?;
        let mut sheets = self.sheets.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(position) = sheets
            .iter()
            .position(|c| c.name == name && c.language == requested)
        {
            let cached = sheets.remove(position).expect("found");
            let result = (cached.sheet.clone(), cached.order.clone());
            sheets.push_front(cached);
            return Ok(result);
        }
        drop(sheets);
        let header = excel::sheet_header(self.archives()?, name)
            .map_err(sheet_error)?
            .ok_or_else(|| Error::new(Kind::NotFound, format!("the game has no sheet {name}")))?;
        if header.effective_language(requested).is_none() {
            return Err(Error::new(
                Kind::NotFound,
                format!("sheet {name} has no data in language {code} and no language-neutral data"),
            ));
        }
        let sheet =
            Arc::new(excel::read_sheet(self.archives()?, name, requested).map_err(sheet_error)?);
        let mut order: Vec<_> = sheet
            .rows()
            .iter()
            .enumerate()
            .map(|(index, row)| ((row.row_id, row.subrow_id), index))
            .collect();
        order.sort_unstable();
        let order = Arc::new(order);
        let mut sheets = self.sheets.lock().unwrap_or_else(PoisonError::into_inner);
        sheets.push_front(Cached {
            name: name.to_owned(),
            language: requested,
            sheet: sheet.clone(),
            order: order.clone(),
        });
        sheets.truncate(CACHED_SHEETS);
        Ok((sheet, order))
    }

    /// Up to `limit` rows after `after`, sorted by row and subrow.
    ///
    /// # Errors
    /// Fails for a missing sheet, a language the sheet has neither data nor
    /// language-neutral data for, a limit outside 1 to 1024, and malformed
    /// data.
    pub fn rows(
        &self,
        name: &str,
        code: u8,
        after: Option<(u32, u16)>,
        limit: u32,
    ) -> Result<Vec<Row>> {
        if !(1..=MAX_ROWS).contains(&limit) {
            return Err(Error::invalid(format!("the limit is 1 to {MAX_ROWS}")));
        }
        let (sheet, order) = self.cached(name, code)?;
        let start = after.map_or(0, |after| order.partition_point(|(key, _)| *key <= after));
        let rows = sheet.rows();
        order[start..]
            .iter()
            .take(limit as usize)
            .map(|&((row, subrow), index)| decode(&sheet, &rows[index], row, subrow))
            .collect()
    }

    /// # Errors
    /// As [`GameData::rows`].
    pub fn row(&self, name: &str, code: u8, key: (u32, u16)) -> Result<Option<Row>> {
        let (sheet, order) = self.cached(name, code)?;
        let Ok(position) = order.binary_search_by(|(candidate, _)| candidate.cmp(&key)) else {
            return Ok(None);
        };
        let rows = sheet.rows();
        decode(&sheet, &rows[order[position].1], key.0, key.1).map(Some)
    }
}

fn decode(sheet: &Sheet, row: &excel::Row<'_>, id: u32, subrow: u16) -> Result<Row> {
    let malformed =
        |e: excel::CellError| Error::invalid(format!("{} row {id}/{subrow}: {e}", sheet.name));
    let mut cells = Vec::with_capacity(sheet.columns.len());
    for (column, info) in sheet.columns.iter().enumerate() {
        if info.kind == ColumnKind::String {
            cells.push(Cell::Text(row.string(column).map_err(malformed)?.to_vec()));
            continue;
        }
        cells.push(Cell::Value(value(
            info.kind,
            &row.value(column).map_err(malformed)?,
        )));
    }
    Ok(Row {
        row: id,
        subrow,
        cells,
    })
}

/// A value from its canonical little-endian bytes (`excel::Row::value`).
fn value(kind: ColumnKind, bytes: &[u8]) -> Value {
    let mut wide = [0u8; 8];
    let length = bytes.len().min(8);
    wide[..length].copy_from_slice(&bytes[..length]);
    let unsigned = u64::from_le_bytes(wide);
    let signed = |bits: u32| {
        let shift = 64 - bits;
        (unsigned << shift).cast_signed() >> shift
    };
    match kind {
        ColumnKind::Bool | ColumnKind::PackedBool(_) | ColumnKind::String => {
            Value::Boolean(unsigned != 0)
        }
        ColumnKind::Int8 => Value::Signed(signed(8)),
        ColumnKind::Int16 => Value::Signed(signed(16)),
        ColumnKind::Int32 => Value::Signed(signed(32)),
        ColumnKind::Int64 => Value::Signed(unsigned.cast_signed()),
        ColumnKind::UInt8 | ColumnKind::UInt16 | ColumnKind::UInt32 | ColumnKind::UInt64 => {
            Value::Unsigned(unsigned)
        }
        ColumnKind::Float32 => Value::Float(f32::from_bits(u32::try_from(unsigned).unwrap_or(0))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_sqpack::testing::{FakeGame, FakeRow, FakeSheet, FakeValue};

    fn installation() -> (tempfile::TempDir, GameData) {
        let folder = tempfile::tempdir().unwrap();
        let sheet = FakeSheet::new(vec![ColumnKind::String, ColumnKind::UInt32]).with_rows(
            Language::English,
            (1..=5)
                .rev()
                .map(|id| {
                    FakeRow::new(
                        id,
                        vec![
                            FakeValue::String(format!("Item {id}").into_bytes()),
                            FakeValue::UInt32(id * 10),
                        ],
                    )
                })
                .collect(),
        );
        FakeGame::new("2026.09.15.0000.0000")
            .with_sheet("Item", sheet.with_uninstalled(Language::Korean))
            .with_file("common/font/AXIS_12.fdt", b"fcsv0100".to_vec())
            .write(folder.path())
            .unwrap();
        let data = GameData::new(Some(folder.path()));
        (folder, data)
    }

    #[test]
    fn reads_files_and_sheets_from_the_archives() {
        let (_folder, data) = installation();
        assert_eq!(data.read("common/font/AXIS_12.fdt").unwrap(), b"fcsv0100");
        assert_eq!(
            data.read("COMMON/font/axis_12.fdt").unwrap(),
            b"fcsv0100",
            "case-insensitive"
        );
        assert!(data.exists("common/font/AXIS_12.fdt").unwrap());
        assert_eq!(
            data.read("common/font/none.fdt").unwrap_err().kind,
            Kind::NotFound
        );
        assert_eq!(data.sheets().unwrap(), ["Item"]);
        let info = data.sheet("Item").unwrap();
        assert_eq!(info.rows, 5);
        assert!(!info.subrows);
        assert_eq!(info.languages, [2], "Korean is listed but not installed");
        assert_eq!(data.sheet("Missing").unwrap_err().kind, Kind::NotFound);

        let page = data.rows("Item", 2, None, 2).unwrap();
        assert_eq!(
            page.iter().map(|r| r.row).collect::<Vec<_>>(),
            [1, 2],
            "sorted"
        );
        assert_eq!(page[1].cells[0], Cell::Text(b"Item 2".to_vec()));
        assert_eq!(page[1].cells[1], Cell::Value(Value::Unsigned(20)));
        let rest = data.rows("Item", 2, Some((2, 0)), 1024).unwrap();
        assert_eq!(rest.iter().map(|r| r.row).collect::<Vec<_>>(), [3, 4, 5]);
        assert_eq!(data.row("Item", 2, (4, 0)).unwrap().unwrap().row, 4);
        assert!(data.row("Item", 2, (9, 0)).unwrap().is_none());
        assert_eq!(
            data.rows("Item", 3, None, 1).unwrap_err().kind,
            Kind::NotFound,
            "no German or neutral data"
        );
        assert_eq!(
            data.rows("Item", 7, None, 1).unwrap_err().kind,
            Kind::NotFound,
            "no Korean data files"
        );
        assert!(data.rows("Item", 2, None, 0).is_err());
        assert!(data.rows("Item", 9, None, 1).is_err());
    }

    #[test]
    fn values_follow_their_column_kind() {
        assert_eq!(
            value(ColumnKind::Int16, &(-2i16).to_le_bytes()),
            Value::Signed(-2)
        );
        assert_eq!(value(ColumnKind::Int8, &[0xFF]), Value::Signed(-1));
        assert_eq!(
            value(ColumnKind::UInt16, &[0xFF, 0xFF]),
            Value::Unsigned(65535)
        );
        assert_eq!(value(ColumnKind::PackedBool(3), &[1]), Value::Boolean(true));
        assert_eq!(
            value(ColumnKind::Float32, &1.5f32.to_le_bytes()),
            Value::Float(1.5)
        );
    }

    #[test]
    fn without_an_installation_every_call_is_unavailable() {
        let data = GameData::new(None);
        assert_eq!(data.read("a/b").unwrap_err().kind, Kind::Unavailable);
        assert_eq!(data.sheets().unwrap_err().kind, Kind::Unavailable);
    }

    #[test]
    fn archives_that_failed_to_open_are_tried_again() {
        let folder = tempfile::tempdir().unwrap();
        let data = GameData::new(Some(folder.path()));
        assert_eq!(data.sheets().unwrap_err().kind, Kind::Unavailable);
        FakeGame::new("2026.09.15.0000.0000")
            .with_file("common/font/AXIS_12.fdt", b"fcsv0100".to_vec())
            .write(folder.path())
            .unwrap();
        assert_eq!(
            data.read("common/font/AXIS_12.fdt").unwrap_err().kind,
            Kind::Unavailable,
            "the failure is kept for a while"
        );
        let earlier = std::time::Instant::now().checked_sub(RETRY_OPEN).unwrap();
        if let Some((at, _)) = data.failed.lock().unwrap().as_mut() {
            *at = earlier;
        }
        assert_eq!(data.read("common/font/AXIS_12.fdt").unwrap(), b"fcsv0100");
    }

    /// The installed game: `ANGELICA_GAME_PATH` is its `game` folder.
    #[test]
    #[ignore = "needs ANGELICA_GAME_PATH"]
    fn reads_the_installed_game() {
        let game = std::path::PathBuf::from(
            std::env::var("ANGELICA_GAME_PATH").expect("ANGELICA_GAME_PATH"),
        );
        let data = GameData::new(game.parent());
        let sheets = data.sheets().unwrap();
        assert!(sheets.len() > 1000, "{} sheets", sheets.len());
        let item = data.sheet("Item").unwrap();
        assert!(item.languages.contains(&2));
        let rows = data.rows("Item", 2, None, 1024).unwrap();
        assert_eq!(rows.len(), 1024);
        let potion = data.row("Item", 2, (4551, 0)).unwrap().unwrap();
        assert!(
            potion
                .cells
                .iter()
                .any(|cell| matches!(cell, Cell::Text(text) if !text.is_empty()))
        );
        let quest = data.sheet("Quest").unwrap();
        assert!(quest.rows > 1000);
        let commands = data.sheet("TextCommand").unwrap();
        for &language in &commands.languages {
            data.rows("TextCommand", language, None, 1).unwrap();
        }
        assert!(!data.read("common/font/AXIS_12.fdt").unwrap().is_empty());
        assert!(data.read("common/font/font1.tex").unwrap().len() > 1024 * 1024);
    }
}
