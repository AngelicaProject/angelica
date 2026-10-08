//! The text table format (`docs/formats/text-table-v1.md`): replacement
//! text for cells of the game's Excel sheets, each tied to the original text
//! by a [`guard`](crate::guard). [`Table::parse`] validates a whole file and
//! then reads it in place; [`Writer`] produces one.
use crate::sestring;
use std::collections::BTreeMap;

pub const MAGIC: [u8; 8] = *b"ANGTEXT1";
pub const HEADER_SIZE: usize = 128;
pub const SHEET_SIZE: usize = 32;
pub const COLUMN_SIZE: usize = 4;
pub const CELL_SIZE: usize = 24;
pub const MAX_DOMAIN: usize = 64;
pub const MAX_SHEET_NAME: usize = 256;
/// A sheet has at most this many text columns.
pub const MAX_COLUMNS: usize = 4096;
/// `language` value of a table that applies to sheets of any language.
pub const ANY_LANGUAGE: u8 = 0xFF;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("text table: {0}")]
pub struct Error(pub String);

fn fail<T>(message: impl Into<String>) -> Result<T, Error> {
    Err(Error(message.into()))
}

/// Whether a sheet's rows have subrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Rows,
    Subrows,
}

/// A text column of a sheet: its position among all of the sheet's columns
/// and the byte offset of its field in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Column {
    pub index: u16,
    pub offset: u16,
}

/// A cell of a parsed table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell<'a> {
    pub row: u32,
    pub subrow: u16,
    /// The column's index among all of the sheet's columns.
    pub column: u16,
    pub guard: u64,
    pub text: &'a [u8],
}

/// A validated table read in place from its bytes.
#[derive(Debug, Clone, Copy)]
pub struct Table<'a> {
    bytes: &'a [u8],
    language: u8,
    domain: &'a [u8],
    sheets: &'a [u8],
    columns: &'a [u8],
    cells: &'a [u8],
    names: &'a [u8],
    strings: &'a [u8],
}

/// A sheet of a parsed table.
#[derive(Debug, Clone, Copy)]
pub struct Sheet<'a> {
    pub name: &'a str,
    pub variant: Variant,
    columns: &'a [u8],
    cells: &'a [u8],
    strings: &'a [u8],
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}
fn size(value: u64, what: &str) -> Result<usize, Error> {
    usize::try_from(value).or_else(|_| fail(format!("{what} is too large")))
}

impl<'a> Table<'a> {
    /// Validates every part of the file: header, sheets, columns, cells,
    /// names, and the text of every cell.
    ///
    /// # Errors
    /// Names the first rule the file breaks.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_SIZE || bytes[..8] != MAGIC {
            return fail("not a text table");
        }
        let version = u32_at(bytes, 8);
        if version != 1 {
            return fail(format!("version {version} is unsupported"));
        }
        let sheet_count = size(u64::from(u32_at(bytes, 12)), "sheet count")?;
        let column_count = size(u64_at(bytes, 16), "column count")?;
        let cell_count = size(u64_at(bytes, 24), "cell count")?;
        let names_length = size(u64_at(bytes, 32), "names length")?;
        let strings_length = size(u64_at(bytes, 40), "strings length")?;
        let language = bytes[48];
        let domain_length = usize::from(bytes[49]);
        if language > 8 && language != ANY_LANGUAGE {
            return fail(format!("language {language} is not a game language"));
        }
        if domain_length > MAX_DOMAIN {
            return fail("guard domain is longer than 64 bytes");
        }
        if bytes[50..64].iter().any(|&b| b != 0)
            || bytes[64 + domain_length..128].iter().any(|&b| b != 0)
        {
            return fail("reserved header bytes are not zero");
        }

        let mut at = HEADER_SIZE;
        let mut take = |count: usize, item: usize, what: &str| -> Result<&'a [u8], Error> {
            let length = count
                .checked_mul(item)
                .ok_or_else(|| Error(format!("{what} is too large")))?;
            let end = at
                .checked_add(length)
                .filter(|&end| end <= bytes.len())
                .ok_or_else(|| Error(format!("{what} extends past the end of the file")))?;
            let part = &bytes[at..end];
            at = end;
            Ok(part)
        };
        let sheets = take(sheet_count, SHEET_SIZE, "sheet records")?;
        let columns = take(column_count, COLUMN_SIZE, "column records")?;
        let cells = take(cell_count, CELL_SIZE, "cell records")?;
        let names = take(names_length, 1, "names")?;
        let strings = take(strings_length, 1, "strings")?;
        if at != bytes.len() {
            return fail("the file has bytes after the strings");
        }

        let table = Self {
            bytes,
            language,
            domain: &bytes[64..64 + domain_length],
            sheets,
            columns,
            cells,
            names,
            strings,
        };
        table.validate(column_count, cell_count)?;
        Ok(table)
    }

    // The section boundaries of bytes `parse` accepted, without validating
    // them again.
    fn sections(bytes: &'a [u8]) -> Self {
        // `parse` checked that every length fits in usize.
        let length = |at: usize| usize::try_from(u64_at(bytes, at)).unwrap_or(usize::MAX);
        let sheets_end = HEADER_SIZE + u32_at(bytes, 12) as usize * SHEET_SIZE;
        let columns_end = sheets_end + length(16) * COLUMN_SIZE;
        let cells_end = columns_end + length(24) * CELL_SIZE;
        let names_end = cells_end + length(32);
        Self {
            bytes,
            language: bytes[48],
            domain: &bytes[64..64 + usize::from(bytes[49])],
            sheets: &bytes[HEADER_SIZE..sheets_end],
            columns: &bytes[sheets_end..columns_end],
            cells: &bytes[columns_end..cells_end],
            names: &bytes[cells_end..names_end],
            strings: &bytes[names_end..],
        }
    }

    fn validate(&self, column_count: usize, cell_count: usize) -> Result<(), Error> {
        let mut next_column = 0usize;
        let mut next_cell = 0usize;
        let mut previous: Option<&str> = None;
        for index in 0..self.sheet_count() {
            let record = &self.sheets[index * SHEET_SIZE..][..SHEET_SIZE];
            let name_offset = size(u64::from(u32_at(record, 0)), "name offset")?;
            let name_length = size(u64::from(u32_at(record, 4)), "name length")?;
            let variant = record[8];
            let column_start = size(u64::from(u32_at(record, 12)), "column start")?;
            let columns = size(u64::from(u32_at(record, 16)), "column count")?;
            let cell_start = size(u64_at(record, 20), "cell start")?;
            let cells = size(u64::from(u32_at(record, 28)), "cell count")?;
            if record[9..12].iter().any(|&b| b != 0) {
                return fail(format!("sheet {index}: reserved bytes are not zero"));
            }
            let name = name_offset
                .checked_add(name_length)
                .and_then(|end| self.names.get(name_offset..end))
                .and_then(|name| std::str::from_utf8(name).ok())
                .ok_or_else(|| Error(format!("sheet {index}: invalid name")))?;
            if name.is_empty() || name.len() > MAX_SHEET_NAME {
                return fail(format!("sheet {index}: name length out of range"));
            }
            if previous.is_some_and(|previous| previous >= name) {
                return fail(format!(
                    "sheet {name}: sheets are not sorted by name or repeat"
                ));
            }
            previous = Some(name);
            if variant > 1 {
                return fail(format!("sheet {name}: unknown variant {variant}"));
            }
            if column_start != next_column || columns == 0 || columns > MAX_COLUMNS {
                return fail(format!(
                    "sheet {name}: column records are not contiguous or out of range"
                ));
            }
            if cell_start != next_cell || cells == 0 {
                return fail(format!(
                    "sheet {name}: cell records are not contiguous or empty"
                ));
            }
            next_column += columns;
            next_cell += cells;
            if next_column > column_count || next_cell > cell_count {
                return fail(format!("sheet {name}: records extend past their sections"));
            }

            let sheet = self.sheet_at(index);
            let mut last = None;
            for column in sheet.columns() {
                if last.is_some_and(|last: Column| last.index >= column.index) {
                    return fail(format!(
                        "sheet {name}: columns are not sorted by index or repeat"
                    ));
                }
                last = Some(column);
            }
            let mut last_key = None;
            for position in 0..cells {
                let cell = sheet.raw_cell(position)?;
                let key = (cell.row, cell.subrow, cell.column);
                if last_key.is_some_and(|last| last >= key) {
                    return fail(format!("sheet {name}: cells are not sorted or repeat"));
                }
                last_key = Some(key);
                if sheet.variant == Variant::Rows && cell.subrow != 0 {
                    return fail(format!(
                        "sheet {name}: row {} has a subrow without subrows",
                        cell.row
                    ));
                }
                if sheet.columns().all(|column| column.index != cell.column) {
                    return fail(format!(
                        "sheet {name}: row {} names column {}, which is not a text column",
                        cell.row, cell.column
                    ));
                }
                if !sestring::is_cell_text(cell.text) || cell.text.starts_with(b"_rsv_") {
                    return fail(format!(
                        "sheet {name}: row {} column {} has malformed text",
                        cell.row, cell.column
                    ));
                }
            }
        }
        if next_column != column_count || next_cell != cell_count {
            return fail("sheets do not cover every column and cell record");
        }
        Ok(())
    }

    /// The game language of the sheets the table was made for, or
    /// [`ANY_LANGUAGE`].
    #[must_use]
    pub fn language(&self) -> u8 {
        self.language
    }

    #[must_use]
    pub fn domain(&self) -> &'a [u8] {
        self.domain
    }

    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    #[must_use]
    pub fn sheet_count(&self) -> usize {
        self.sheets.len() / SHEET_SIZE
    }

    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.cells.len() / CELL_SIZE
    }

    pub fn sheets(&self) -> impl Iterator<Item = Sheet<'a>> + '_ {
        (0..self.sheet_count()).map(|index| self.sheet_at(index))
    }

    /// The sheet with this name.
    #[must_use]
    pub fn sheet(&self, name: &str) -> Option<Sheet<'a>> {
        self.sheet_index(name).map(|index| self.sheet_at(index))
    }

    /// The sheet at `index` in name order.
    #[must_use]
    pub fn sheet_by_index(&self, index: usize) -> Option<Sheet<'a>> {
        (index < self.sheet_count()).then(|| self.sheet_at(index))
    }

    /// The index of the sheet with this name.
    #[must_use]
    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        let (mut low, mut high) = (0, self.sheet_count());
        while low < high {
            let middle = usize::midpoint(low, high);
            match self.sheet_at(middle).name.cmp(name) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Some(middle),
            }
        }
        None
    }

    // Only called after `validate` checked the record's ranges, or by it
    // after checking them.
    fn sheet_at(&self, index: usize) -> Sheet<'a> {
        let record = &self.sheets[index * SHEET_SIZE..][..SHEET_SIZE];
        let name_offset = u32_at(record, 0) as usize;
        let name_length = u32_at(record, 4) as usize;
        let column_start = u32_at(record, 12) as usize;
        let columns = u32_at(record, 16) as usize;
        let cell_start = usize::try_from(u64_at(record, 20)).unwrap_or(usize::MAX);
        let cells = u32_at(record, 28) as usize;
        Sheet {
            name: std::str::from_utf8(&self.names[name_offset..name_offset + name_length])
                .unwrap_or(""),
            variant: if record[8] == 0 {
                Variant::Rows
            } else {
                Variant::Subrows
            },
            columns: &self.columns
                [column_start * COLUMN_SIZE..(column_start + columns) * COLUMN_SIZE],
            cells: &self.cells[cell_start * CELL_SIZE..(cell_start + cells) * CELL_SIZE],
            strings: self.strings,
        }
    }
}

impl<'a> Sheet<'a> {
    /// The sheet's text columns, sorted by index.
    pub fn columns(&self) -> impl Iterator<Item = Column> + '_ {
        self.columns
            .as_chunks::<COLUMN_SIZE>()
            .0
            .iter()
            .map(|record| Column {
                index: u16_at(record, 0),
                offset: u16_at(record, 2),
            })
    }

    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.cells.len() / CELL_SIZE
    }

    fn raw_cell(&self, position: usize) -> Result<Cell<'a>, Error> {
        let record = &self.cells[position * CELL_SIZE..][..CELL_SIZE];
        let offset = u32_at(record, 16) as usize;
        let length = u32_at(record, 20) as usize;
        let text = offset
            .checked_add(length)
            .and_then(|end| self.strings.get(offset..end))
            .ok_or_else(|| {
                Error(format!(
                    "sheet {}: cell text lies outside the strings",
                    self.name
                ))
            })?;
        Ok(Cell {
            row: u32_at(record, 0),
            subrow: u16_at(record, 4),
            column: u16_at(record, 6),
            guard: u64_at(record, 8),
            text,
        })
    }

    /// Every cell, sorted by row, subrow, and column.
    pub fn cells(&self) -> impl Iterator<Item = Cell<'a>> + '_ {
        (0..self.cell_count()).filter_map(|position| self.raw_cell(position).ok())
    }

    /// The cells of one row, sorted by column.
    #[must_use]
    pub fn row(&self, row: u32, subrow: u16) -> Vec<Cell<'a>> {
        let key = |position: usize| {
            let record = &self.cells[position * CELL_SIZE..][..CELL_SIZE];
            (u32_at(record, 0), u16_at(record, 4))
        };
        let count = self.cell_count();
        let (mut low, mut high) = (0, count);
        while low < high {
            let middle = usize::midpoint(low, high);
            if key(middle) < (row, subrow) {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let mut cells = Vec::new();
        while low < count && key(low) == (row, subrow) {
            if let Ok(cell) = self.raw_cell(low) {
                cells.push(cell);
            }
            low += 1;
        }
        cells
    }
}

/// A validated table that owns its bytes.
#[derive(Debug, Clone)]
pub struct OwnedTable {
    bytes: std::sync::Arc<[u8]>,
}

impl OwnedTable {
    /// Validates `bytes` like [`Table::parse`].
    ///
    /// # Errors
    /// Names the first rule the file breaks.
    pub fn new(bytes: Vec<u8>) -> Result<Self, Error> {
        Table::parse(&bytes)?;
        Ok(Self {
            bytes: bytes.into(),
        })
    }

    #[must_use]
    pub fn table(&self) -> Table<'_> {
        Table::sections(&self.bytes)
    }
}

/// Builds a table in memory. Cells may be added in any order; text is
/// stored once per distinct string. Memory is about 24 bytes per cell plus
/// the distinct text.
#[derive(Debug, Default)]
pub struct Writer {
    language: u8,
    domain: Vec<u8>,
    sheets: BTreeMap<String, SheetDraft>,
    strings: Vec<u8>,
    /// Hash of a string's bytes to the places it is stored.
    interned: std::collections::HashMap<u64, Vec<(u32, u32)>>,
}

#[derive(Debug)]
struct SheetDraft {
    variant: Variant,
    columns: Vec<Column>,
    cells: Vec<DraftCell>,
}

#[derive(Debug, Clone, Copy)]
struct DraftCell {
    row: u32,
    subrow: u16,
    column: u16,
    guard: u64,
    offset: u32,
    length: u32,
}

fn too_large() -> Error {
    Error("table sections exceed 4 GiB".into())
}

fn u32_of(value: usize) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| too_large())
}

impl Writer {
    /// A table for sheets in `language` (a game language, 0 to 8, or
    /// [`ANY_LANGUAGE`]) with guards computed in `domain`.
    ///
    /// # Errors
    /// Refuses an invalid language or a domain longer than [`MAX_DOMAIN`].
    pub fn new(language: u8, domain: &[u8]) -> Result<Self, Error> {
        if language > 8 && language != ANY_LANGUAGE {
            return fail(format!("language {language} is not a game language"));
        }
        if domain.len() > MAX_DOMAIN {
            return fail("guard domain is longer than 64 bytes");
        }
        Ok(Self {
            language,
            domain: domain.to_vec(),
            ..Self::default()
        })
    }

    /// Declares a sheet with its text columns. Must come before its cells.
    ///
    /// # Errors
    /// Refuses a repeated sheet, an invalid name, or invalid columns.
    pub fn sheet(&mut self, name: &str, variant: Variant, columns: &[Column]) -> Result<(), Error> {
        if name.is_empty() || name.len() > MAX_SHEET_NAME {
            return fail(format!("sheet name {name:?} length out of range"));
        }
        if columns.is_empty() || columns.len() > MAX_COLUMNS {
            return fail(format!(
                "sheet {name}: needs 1 to {MAX_COLUMNS} text columns"
            ));
        }
        let mut sorted = columns.to_vec();
        sorted.sort_unstable();
        if sorted.windows(2).any(|pair| pair[0].index == pair[1].index) {
            return fail(format!("sheet {name}: a column repeats"));
        }
        if self.sheets.contains_key(name) {
            return fail(format!("sheet {name} is declared twice"));
        }
        self.sheets.insert(
            name.to_owned(),
            SheetDraft {
                variant,
                columns: sorted,
                cells: Vec::new(),
            },
        );
        Ok(())
    }

    fn intern(&mut self, text: &[u8]) -> Result<(u32, u32), Error> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        let places = self.interned.entry(hasher.finish()).or_default();
        for &(offset, length) in places.iter() {
            if &self.strings[offset as usize..(offset + length) as usize] == text {
                return Ok((offset, length));
            }
        }
        let offset = u32_of(self.strings.len())?;
        let length = u32_of(text.len())?;
        u32_of(self.strings.len() + text.len())?;
        self.strings.extend_from_slice(text);
        places.push((offset, length));
        Ok((offset, length))
    }

    /// Adds a replacement for a cell whose original text has `guard`.
    ///
    /// # Errors
    /// Refuses an undeclared sheet, a column that is not a text column, a
    /// subrow in a sheet without subrows, and malformed text. A repeated
    /// cell is refused by [`Writer::finish`].
    pub fn cell(
        &mut self,
        sheet: &str,
        row: u32,
        subrow: u16,
        column: u16,
        guard: u64,
        text: &[u8],
    ) -> Result<(), Error> {
        let Some(draft) = self.sheets.get(sheet) else {
            return fail(format!("sheet {sheet} is not declared"));
        };
        if draft.variant == Variant::Rows && subrow != 0 {
            return fail(format!(
                "sheet {sheet}: row {row} has a subrow without subrows"
            ));
        }
        if draft.columns.iter().all(|c| c.index != column) {
            return fail(format!(
                "sheet {sheet}: column {column} is not a text column"
            ));
        }
        if !sestring::is_cell_text(text) || text.starts_with(b"_rsv_") {
            return fail(format!(
                "sheet {sheet}: row {row} column {column} has malformed text"
            ));
        }
        let (offset, length) = self.intern(text)?;
        if let Some(draft) = self.sheets.get_mut(sheet) {
            draft.cells.push(DraftCell {
                row,
                subrow,
                column,
                guard,
                offset,
                length,
            });
        }
        Ok(())
    }

    /// The finished file. Sheets without cells are left out.
    ///
    /// # Errors
    /// Refuses a repeated cell and a table whose sections exceed the
    /// format's sizes.
    pub fn finish(self) -> Result<Vec<u8>, Error> {
        let mut sheets: Vec<_> = self
            .sheets
            .into_iter()
            .filter(|(_, s)| !s.cells.is_empty())
            .collect();
        let mut names = Vec::new();
        let mut sheet_records = Vec::new();
        let mut column_records = Vec::new();
        let mut cell_records = Vec::new();
        let mut column_count = 0u32;
        let mut cell_count = 0u64;
        for (name, sheet) in &mut sheets {
            sheet
                .cells
                .sort_unstable_by_key(|c| (c.row, c.subrow, c.column));
            let key = |c: &DraftCell| (c.row, c.subrow, c.column);
            if let Some(pair) = sheet
                .cells
                .windows(2)
                .find(|pair| key(&pair[0]) == key(&pair[1]))
            {
                return fail(format!(
                    "sheet {name}: row {} subrow {} column {} repeats",
                    pair[0].row, pair[0].subrow, pair[0].column
                ));
            }
            sheet_records.extend_from_slice(&u32_of(names.len())?.to_le_bytes());
            names.extend_from_slice(name.as_bytes());
            sheet_records.extend_from_slice(&u32_of(name.len())?.to_le_bytes());
            sheet_records.push(u8::from(sheet.variant == Variant::Subrows));
            sheet_records.extend_from_slice(&[0; 3]);
            sheet_records.extend_from_slice(&column_count.to_le_bytes());
            let columns = u32_of(sheet.columns.len())?;
            sheet_records.extend_from_slice(&columns.to_le_bytes());
            sheet_records.extend_from_slice(&cell_count.to_le_bytes());
            let cells = u32_of(sheet.cells.len())?;
            sheet_records.extend_from_slice(&cells.to_le_bytes());
            column_count = column_count.checked_add(columns).ok_or_else(too_large)?;
            cell_count += u64::from(cells);
            for column in &sheet.columns {
                column_records.extend_from_slice(&column.index.to_le_bytes());
                column_records.extend_from_slice(&column.offset.to_le_bytes());
            }
            for cell in &sheet.cells {
                cell_records.extend_from_slice(&cell.row.to_le_bytes());
                cell_records.extend_from_slice(&cell.subrow.to_le_bytes());
                cell_records.extend_from_slice(&cell.column.to_le_bytes());
                cell_records.extend_from_slice(&cell.guard.to_le_bytes());
                cell_records.extend_from_slice(&cell.offset.to_le_bytes());
                cell_records.extend_from_slice(&cell.length.to_le_bytes());
            }
        }

        let mut file = Vec::with_capacity(
            HEADER_SIZE
                + sheet_records.len()
                + column_records.len()
                + cell_records.len()
                + names.len()
                + self.strings.len(),
        );
        file.extend_from_slice(&MAGIC);
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&u32_of(sheets.len())?.to_le_bytes());
        file.extend_from_slice(&u64::from(column_count).to_le_bytes());
        file.extend_from_slice(&cell_count.to_le_bytes());
        file.extend_from_slice(&(names.len() as u64).to_le_bytes());
        file.extend_from_slice(&(self.strings.len() as u64).to_le_bytes());
        file.push(self.language);
        file.push(u8::try_from(self.domain.len()).map_err(|_| too_large())?);
        file.extend_from_slice(&[0; 14]);
        file.extend_from_slice(&self.domain);
        file.resize(HEADER_SIZE, 0);
        file.extend_from_slice(&sheet_records);
        file.extend_from_slice(&column_records);
        file.extend_from_slice(&cell_records);
        file.extend_from_slice(&names);
        file.extend_from_slice(&self.strings);
        Ok(file)
    }
}
