//! Text tables plugins activate (the `excel-text` interface) and the rows
//! they change. Platform independent: the Windows adapter and the scripted
//! game hand rows to [`SheetBinding::rewrite`] the same way.
//!
//! A row is changed only when the sheet matches a table exactly (name,
//! variant, language, text columns), its buffer has the version 3 layout
//! (`docs/formats/game-structures.md`), and the original text of each replaced
//! cell matches the cell's guard.
#![forbid(unsafe_code)]
use angelica_text::table::{ANY_LANGUAGE, Column, OwnedTable, Variant};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// Tables one plugin can keep active at once.
pub const MAX_TABLES_PER_PLUGIN: usize = 8;
/// Bytes of one table file; the host keeps active tables in memory.
pub const MAX_TABLE_SIZE: u64 = 512 * 1024 * 1024;
/// Bytes of one plugin's active tables together.
pub const MAX_TABLES_SIZE_PER_PLUGIN: usize = 1024 * 1024 * 1024;
/// Rebuilt rows stay below this size; text fields address at most 24 bits.
pub const MAX_ROW_SIZE: usize = 1 << 20;

/// What the game's sheet looks like, as the adapter reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetInfo {
    pub name: String,
    pub variant: Variant,
    /// The game language of the sheet's rows; 0 for a sheet without one.
    pub language: u8,
    /// The text columns, sorted by index.
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mismatch {
    Layout,
    Variant,
    Language,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub rows_changed: u64,
    pub cells_replaced: u64,
    pub guard_mismatches: u64,
    pub rows_refused: u64,
    pub sheets_mismatched: Vec<(String, Mismatch)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableInfo {
    pub sheets: u32,
    pub cells: u64,
}

/// An active table.
pub struct Entry {
    plugin: String,
    name: String,
    priority: i32,
    table: OwnedTable,
    /// Bytes of the table file.
    size: usize,
    rows_changed: AtomicU64,
    cells_replaced: AtomicU64,
    guard_mismatches: AtomicU64,
    rows_refused: AtomicU64,
    mismatched: Mutex<BTreeMap<String, Mismatch>>,
}

#[derive(Default)]
struct Snapshot {
    generation: u64,
    /// Sorted by descending priority, then plugin id and table name.
    entries: Vec<Arc<Entry>>,
}

#[derive(Default)]
pub struct ExcelText {
    snapshot: RwLock<Arc<Snapshot>>,
}

impl ExcelText {
    /// Activates `bytes` as the plugin's table `name`, replacing the one of
    /// that name. The previous table stays when `bytes` is invalid.
    ///
    /// # Errors
    /// Names why the table is invalid or the plugin has too many tables.
    pub fn activate(
        &self,
        plugin: &str,
        name: &str,
        priority: i32,
        bytes: Vec<u8>,
    ) -> Result<TableInfo, String> {
        let size = bytes.len();
        let table = OwnedTable::new(bytes).map_err(|e| e.to_string())?;
        let view = table.table();
        let info = TableInfo {
            sheets: u32::try_from(view.sheet_count()).unwrap_or(u32::MAX),
            cells: view.cell_count() as u64,
        };
        let entry = Arc::new(Entry {
            plugin: plugin.into(),
            name: name.into(),
            priority,
            table,
            size,
            rows_changed: AtomicU64::new(0),
            cells_replaced: AtomicU64::new(0),
            guard_mismatches: AtomicU64::new(0),
            rows_refused: AtomicU64::new(0),
            mismatched: Mutex::new(BTreeMap::new()),
        });
        let mut snapshot = self
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let mut entries: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|e| !(e.plugin == plugin && e.name == name))
            .cloned()
            .collect();
        let others = entries.iter().filter(|e| e.plugin == plugin);
        if others.clone().count() >= MAX_TABLES_PER_PLUGIN {
            return Err(format!(
                "a plugin can keep at most {MAX_TABLES_PER_PLUGIN} tables active"
            ));
        }
        if others.map(|e| e.size).sum::<usize>() + size > MAX_TABLES_SIZE_PER_PLUGIN {
            return Err(format!(
                "a plugin's active tables have at most {MAX_TABLES_SIZE_PER_PLUGIN} bytes together"
            ));
        }
        entries.push(entry);
        entries.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.plugin.cmp(&b.plugin))
                .then_with(|| a.name.cmp(&b.name))
        });
        *snapshot = Arc::new(Snapshot {
            generation: snapshot.generation + 1,
            entries,
        });
        Ok(info)
    }

    /// Removes the plugin's table; false when it had none of that name.
    pub fn deactivate(&self, plugin: &str, name: &str) -> bool {
        self.retain(|e| !(e.plugin == plugin && e.name == name))
    }

    /// Removes every table of the plugin.
    pub fn remove_plugin(&self, plugin: &str) {
        self.retain(|e| e.plugin != plugin);
    }

    fn retain(&self, keep: impl Fn(&Entry) -> bool) -> bool {
        let mut snapshot = self
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let entries: Vec<_> = snapshot
            .entries
            .iter()
            .filter(|e| keep(e))
            .cloned()
            .collect();
        if entries.len() == snapshot.entries.len() {
            return false;
        }
        *snapshot = Arc::new(Snapshot {
            generation: snapshot.generation + 1,
            entries,
        });
        true
    }

    #[must_use]
    pub fn status(&self, plugin: &str, name: &str) -> Option<Status> {
        let snapshot = self.current();
        let entry = snapshot
            .entries
            .iter()
            .find(|e| e.plugin == plugin && e.name == name)?;
        Some(Status {
            rows_changed: entry.rows_changed.load(Ordering::Relaxed),
            cells_replaced: entry.cells_replaced.load(Ordering::Relaxed),
            guard_mismatches: entry.guard_mismatches.load(Ordering::Relaxed),
            rows_refused: entry.rows_refused.load(Ordering::Relaxed),
            sheets_mismatched: entry
                .mismatched
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .map(|(sheet, reason)| (sheet.clone(), *reason))
                .collect(),
        })
    }

    /// Changes whenever a table is activated or removed; a cached
    /// [`SheetBinding`] of another generation must be bound again.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.current().generation
    }

    fn current(&self) -> Arc<Snapshot> {
        self.snapshot
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The text the active tables put in place of `original` in a cell: of
    /// the first table, by priority, that lists the sheet in this language,
    /// has the cell, and whose guard matches the game's text. `column` is
    /// the column's index among all of the sheet's columns.
    #[must_use]
    pub fn shown(
        &self,
        sheet: &str,
        language: u8,
        row: u32,
        subrow: u16,
        column: u16,
        original: &[u8],
    ) -> Option<Vec<u8>> {
        let snapshot = self.current();
        for entry in &snapshot.entries {
            let table = entry.table.table();
            if table.language() != ANY_LANGUAGE && language != 0 && table.language() != language {
                continue;
            }
            let Some(listed) = table
                .sheet_index(sheet)
                .and_then(|index| table.sheet_by_index(index))
            else {
                continue;
            };
            let cells = listed.row(row, subrow);
            if let Some(cell) = cells.iter().find(|cell| cell.column == column)
                && angelica_text::guard(table.domain(), original) == cell.guard
            {
                return Some(cell.text.to_vec());
            }
        }
        None
    }

    /// The tables that apply to a sheet. Tables that list the sheet but do
    /// not match it record why.
    #[must_use]
    pub fn bind(&self, sheet: &SheetInfo) -> SheetBinding {
        let snapshot = self.current();
        let mut parts = Vec::new();
        for entry in &snapshot.entries {
            let table = entry.table.table();
            let Some(index) = table.sheet_index(&sheet.name) else {
                continue;
            };
            let Some(listed) = table.sheet_by_index(index) else {
                continue;
            };
            let mismatch = if listed.variant != sheet.variant {
                Some(Mismatch::Variant)
            } else if table.language() != ANY_LANGUAGE
                && sheet.language != 0
                && table.language() != sheet.language
            {
                Some(Mismatch::Language)
            } else if !listed.columns().eq(sheet.columns.iter().copied()) {
                Some(Mismatch::Layout)
            } else {
                None
            };
            match mismatch {
                Some(reason) => {
                    entry
                        .mismatched
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(sheet.name.clone(), reason);
                }
                None => parts.push((entry.clone(), index)),
            }
        }
        SheetBinding {
            generation: snapshot.generation,
            columns: sheet.columns.clone(),
            parts,
        }
    }
}

/// The active tables that match one sheet, in the order they are tried.
pub struct SheetBinding {
    generation: u64,
    columns: Vec<Column>,
    parts: Vec<(Arc<Entry>, usize)>,
}

/// What [`SheetBinding::rewrite`] did with a row.
#[derive(Debug, PartialEq, Eq)]
pub enum Rewrite {
    /// No table has a cell for the row whose guard matched.
    Unchanged,
    /// The buffer does not have the expected layout; the row keeps the
    /// game's text.
    Refused,
    /// The new row buffer.
    Changed(Vec<u8>),
}

/// One text field of a row buffer.
struct Field {
    offset: usize,
    start: usize,
    length: usize,
}

impl SheetBinding {
    /// A binding without tables, for a sheet that cannot be read.
    #[must_use]
    pub fn none(generation: u64) -> Self {
        Self {
            generation,
            columns: Vec::new(),
            parts: Vec::new(),
        }
    }

    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// How many active tables apply to the sheet.
    #[must_use]
    pub fn table_count(&self) -> usize {
        self.parts.len()
    }

    /// The sheet's text columns.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Whether any bound table lists cells of the row; the adapter reads
    /// the row's text only then.
    #[must_use]
    pub fn lists(&self, row: u32, subrow: u16) -> bool {
        self.parts.iter().any(|(entry, index)| {
            entry
                .table
                .table()
                .sheet_by_index(*index)
                .is_some_and(|sheet| !sheet.row(row, subrow).is_empty())
        })
    }

    /// Rewrites a row of the sheet. `buffer` holds the row as the game built
    /// it: `fixed_size` bytes of fields, then each text column's string with
    /// its terminator, in column order. It may extend past the last string.
    #[must_use]
    pub fn rewrite(&self, row: u32, subrow: u16, buffer: &[u8], fixed_size: usize) -> Rewrite {
        let candidates: Vec<_> = self
            .parts
            .iter()
            .filter_map(|(entry, index)| {
                let sheet = entry.table.table().sheet_by_index(*index)?;
                Some((entry, sheet.row(row, subrow)))
            })
            .filter(|(_, cells)| !cells.is_empty())
            .collect();
        if candidates.is_empty() {
            return Rewrite::Unchanged;
        }
        let Some(fields) = read_fields(buffer, fixed_size, &self.columns) else {
            for (entry, _) in &candidates {
                entry.rows_refused.fetch_add(1, Ordering::Relaxed);
            }
            return Rewrite::Refused;
        };

        let mut replacements: Vec<Option<&[u8]>> = vec![None; self.columns.len()];
        let mut applied: Vec<u64> = vec![0; candidates.len()];
        for (ordinal, column) in self.columns.iter().enumerate() {
            let field = &fields[ordinal];
            let original = &buffer[field.start..field.start + field.length];
            let mut guards: Vec<(&[u8], u64)> = Vec::new();
            for (position, (entry, cells)) in candidates.iter().enumerate() {
                let Some(cell) = cells.iter().find(|cell| cell.column == column.index) else {
                    continue;
                };
                let domain = entry.table.table().domain();
                let guard = if let Some(&(_, guard)) = guards.iter().find(|(d, _)| *d == domain) {
                    guard
                } else {
                    let guard = angelica_text::guard(domain, original);
                    guards.push((domain, guard));
                    guard
                };
                if guard == cell.guard {
                    replacements[ordinal] = Some(cell.text);
                    applied[position] += 1;
                    break;
                }
                entry.guard_mismatches.fetch_add(1, Ordering::Relaxed);
            }
        }
        if replacements.iter().all(Option::is_none) {
            return Rewrite::Unchanged;
        }
        let Some(rebuilt) = build(buffer, fixed_size, &fields, &replacements) else {
            for (entry, _) in &candidates {
                entry.rows_refused.fetch_add(1, Ordering::Relaxed);
            }
            return Rewrite::Refused;
        };
        for ((entry, _), count) in candidates.iter().zip(applied) {
            if count > 0 {
                entry.rows_changed.fetch_add(1, Ordering::Relaxed);
                entry.cells_replaced.fetch_add(count, Ordering::Relaxed);
            }
        }
        Rewrite::Changed(rebuilt)
    }
}

/// Reads the text fields of a version 3 row: each field holds the distance
/// from itself to its string in the low 24 bits, and the strings follow the
/// fixed part contiguously in column order.
fn read_fields(buffer: &[u8], fixed_size: usize, columns: &[Column]) -> Option<Vec<Field>> {
    if fixed_size == 0 || fixed_size > buffer.len() || fixed_size > MAX_ROW_SIZE {
        return None;
    }
    let mut cursor = fixed_size;
    let mut fields = Vec::with_capacity(columns.len());
    for column in columns {
        let offset = usize::from(column.offset);
        let header = buffer
            .get(offset..offset + 4)
            .filter(|_| offset + 4 <= fixed_size)?;
        let distance = u32::from_le_bytes(header.try_into().ok()?) & 0x00FF_FFFF;
        if offset + distance as usize != cursor {
            return None;
        }
        let length = buffer.get(cursor..)?.iter().position(|&b| b == 0)?;
        fields.push(Field {
            offset,
            start: cursor,
            length,
        });
        cursor += length + 1;
        if cursor > MAX_ROW_SIZE {
            return None;
        }
    }
    Some(fields)
}

/// The rebuilt row: the fixed part, then every string with its terminator.
/// Replaced strings get the game's hash of their text in the field's high
/// byte; untouched strings keep theirs, because the game hashes the resolved
/// text of `_rsv_` strings, which cannot be recomputed here.
fn build(
    buffer: &[u8],
    fixed_size: usize,
    fields: &[Field],
    replacements: &[Option<&[u8]>],
) -> Option<Vec<u8>> {
    let size = fixed_size
        + fields
            .iter()
            .zip(replacements)
            .map(|(field, replacement)| replacement.map_or(field.length, <[u8]>::len) + 1)
            .sum::<usize>();
    if size > MAX_ROW_SIZE {
        return None;
    }
    let mut output = Vec::with_capacity(size);
    output.extend_from_slice(&buffer[..fixed_size]);
    for (field, replacement) in fields.iter().zip(replacements) {
        let text = replacement.unwrap_or(&buffer[field.start..field.start + field.length]);
        let distance = u32::try_from(output.len() - field.offset).ok()?;
        let hash = match replacement {
            Some(text) => string_hash(text),
            None => buffer[field.offset + 3],
        };
        let header = (distance & 0x00FF_FFFF) | (u32::from(hash) << 24);
        output[field.offset..field.offset + 4].copy_from_slice(&header.to_le_bytes());
        output.extend_from_slice(text);
        output.push(0);
    }
    Some(output)
}

/// The game's hash of a row string's text.
#[must_use]
pub fn string_hash(text: &[u8]) -> u8 {
    text.iter().fold(0u8, |hash, &byte| byte ^ (hash << 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_text::{guard, table::Writer};

    const DOMAIN: &[u8] = b"TEST";

    fn columns() -> Vec<Column> {
        vec![
            Column {
                index: 1,
                offset: 0,
            },
            Column {
                index: 4,
                offset: 8,
            },
        ]
    }

    fn sheet() -> SheetInfo {
        SheetInfo {
            name: "Item".into(),
            variant: Variant::Rows,
            language: 2,
            columns: columns(),
        }
    }

    /// A row with a 12-byte fixed part: text fields at 0 and 8, a number at 4.
    fn row(first: &[u8], second: &[u8]) -> Vec<u8> {
        let mut buffer = vec![0; 12];
        buffer[4..8].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let first_header = 0x0Cu32 | (0x11 << 24);
        let second_header = u32::try_from(12 + first.len() + 1 - 8).unwrap() | (0x22 << 24);
        buffer[0..4].copy_from_slice(&first_header.to_le_bytes());
        buffer[8..12].copy_from_slice(&second_header.to_le_bytes());
        buffer.extend_from_slice(first);
        buffer.push(0);
        buffer.extend_from_slice(second);
        buffer.push(0);
        buffer
    }

    fn table(priority_text: &str, cells: &[(u32, u16, &[u8])]) -> Vec<u8> {
        let mut writer = Writer::new(2, DOMAIN).unwrap();
        writer.sheet("Item", Variant::Rows, &columns()).unwrap();
        for (row, column, original) in cells {
            writer
                .cell(
                    "Item",
                    *row,
                    0,
                    *column,
                    guard(DOMAIN, original),
                    priority_text.as_bytes(),
                )
                .unwrap();
        }
        writer.finish().unwrap()
    }

    #[test]
    fn the_shown_text_of_a_cell_is_the_first_matching_tables() {
        let text = ExcelText::default();
        text.activate("a", "high", 5, table("A", &[(7, 4, b"Other")]))
            .unwrap();
        text.activate("b", "low", 0, table("Зелье", &[(7, 4, b"Potion")]))
            .unwrap();
        // The table of higher priority guards another text: the next one has it.
        assert_eq!(
            text.shown("Item", 2, 7, 0, 4, b"Potion"),
            Some("Зелье".as_bytes().to_vec())
        );
        assert_eq!(text.shown("Item", 2, 7, 0, 4, b"Changed"), None);
        assert_eq!(text.shown("Item", 2, 7, 0, 1, b"Potion"), None);
        assert_eq!(text.shown("Item", 2, 8, 0, 4, b"Potion"), None);
        assert_eq!(
            text.shown("Item", 3, 7, 0, 4, b"Potion"),
            None,
            "another language"
        );
        assert_eq!(text.shown("Action", 2, 7, 0, 4, b"Potion"), None);
    }

    #[test]
    fn rewrites_guarded_cells_and_keeps_everything_else() {
        let text = ExcelText::default();
        text.activate("a", "main", 0, table("Зелье", &[(7, 4, b"Potion")]))
            .unwrap();
        let binding = text.bind(&sheet());
        let original = row(b"Name", b"Potion");
        let Rewrite::Changed(rebuilt) = binding.rewrite(7, 0, &original, 12) else {
            panic!("row not changed");
        };
        assert_eq!(rebuilt, {
            let mut expected = row(b"Name", "Зелье".as_bytes());
            expected[11] = string_hash("Зелье".as_bytes());
            expected
        });
        assert_eq!(rebuilt[3], 0x11, "untouched string keeps its hash byte");
        assert_eq!(binding.rewrite(8, 0, &original, 12), Rewrite::Unchanged);
        let status = text.status("a", "main").unwrap();
        assert_eq!((status.rows_changed, status.cells_replaced), (1, 1));
    }

    #[test]
    fn guard_mismatch_keeps_text_and_tries_the_next_table() {
        let text = ExcelText::default();
        text.activate("a", "high", 5, table("A", &[(1, 1, b"Other")]))
            .unwrap();
        text.activate("b", "low", 0, table("B", &[(1, 1, b"Name")]))
            .unwrap();
        let binding = text.bind(&sheet());
        let Rewrite::Changed(rebuilt) = binding.rewrite(1, 0, &row(b"Name", b"x"), 12) else {
            panic!("row not changed");
        };
        assert_eq!(&rebuilt[12..14], b"B\0");
        assert_eq!(text.status("a", "high").unwrap().guard_mismatches, 1);
        assert_eq!(text.status("b", "low").unwrap().cells_replaced, 1);

        text.deactivate("b", "low");
        let binding = text.bind(&sheet());
        assert_eq!(
            binding.rewrite(1, 0, &row(b"Name", b"x"), 12),
            Rewrite::Unchanged
        );
    }

    #[test]
    fn sheets_that_differ_from_the_table_are_not_bound() {
        let text = ExcelText::default();
        text.activate("a", "main", 0, table("X", &[(1, 1, b"Name")]))
            .unwrap();
        let mut moved = sheet();
        moved.columns[1].offset = 12;
        assert!(text.bind(&moved).is_empty());
        let mut subrows = sheet();
        subrows.variant = Variant::Subrows;
        assert!(text.bind(&subrows).is_empty());
        let mut german = sheet();
        german.language = 3;
        assert!(text.bind(&german).is_empty());
        let mut neutral = sheet();
        neutral.language = 0;
        assert!(!text.bind(&neutral).is_empty());
        assert_eq!(
            text.status("a", "main").unwrap().sheets_mismatched,
            [("Item".to_owned(), Mismatch::Language)],
            "the last reason recorded for the sheet"
        );
    }

    #[test]
    fn unexpected_buffers_are_refused() {
        let text = ExcelText::default();
        text.activate("a", "main", 0, table("X", &[(1, 1, b"Name")]))
            .unwrap();
        let binding = text.bind(&sheet());
        let original = row(b"Name", b"x");
        let mut gap = original.clone();
        gap[0] = 13;
        assert_eq!(binding.rewrite(1, 0, &gap, 12), Rewrite::Refused);
        assert_eq!(
            binding.rewrite(1, 0, &original[..15], 12),
            Rewrite::Refused,
            "missing terminator"
        );
        assert_eq!(binding.rewrite(1, 0, &original, 0), Rewrite::Refused);
        assert_eq!(text.status("a", "main").unwrap().rows_refused, 3);
    }

    #[test]
    fn invalid_tables_and_too_many_tables_are_refused() {
        let text = ExcelText::default();
        assert!(text.activate("a", "bad", 0, vec![1, 2, 3]).is_err());
        for index in 0..MAX_TABLES_PER_PLUGIN {
            text.activate("a", &index.to_string(), 0, table("X", &[(1, 1, b"Name")]))
                .unwrap();
        }
        assert!(
            text.activate("a", "one more", 0, table("X", &[(1, 1, b"Name")]))
                .is_err()
        );
        text.activate("a", "0", 0, table("Y", &[(1, 1, b"Name")]))
            .unwrap();
        let generation = text.generation();
        text.remove_plugin("a");
        assert!(text.generation() > generation);
        assert!(text.bind(&sheet()).is_empty());
    }
}
