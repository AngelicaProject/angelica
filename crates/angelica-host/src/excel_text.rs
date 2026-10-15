//! The text of cells of the game's sheets as plugins' rules decide it, at
//! the map's derived decision point of the `sheet_cell` derivation: single
//! rules of equality conditions (`lens.decide`) and rule files in the text
//! table format (`lens.decide-file`). Platform independent: the Windows
//! adapter and the scripted game hand rows to [`SheetBinding::rewrite`] the
//! same way.
//!
//! A row is changed only when its buffer has the version 3 layout
//! (`docs/formats/game-structures.md`); a rule file applies to a sheet only
//! when the sheet matches it exactly (name, variant, language, text
//! columns) and to a cell only when the cell's original text matches the
//! cell's guard. The first rule, in the order plugins added them, that
//! holds for a cell decides its text.
#![forbid(unsafe_code)]
use angelica_text::table::{ANY_LANGUAGE, Column, OwnedTable, Variant};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// Rule files one plugin can keep at once.
pub const MAX_TABLES_PER_PLUGIN: usize = 8;
/// Bytes of one rule file; the host keeps them in memory.
pub const MAX_TABLE_SIZE: u64 = 512 * 1024 * 1024;
/// Bytes of one plugin's rule files together.
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

impl Mismatch {
    /// What the player and the plugin are told.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::Layout => "its text columns differ from the file's",
            Self::Variant => "it has subrows and the file says it has none, or the reverse",
            Self::Language => "it is in another language than the file's",
        }
    }
}

/// What a rule or a rule file did since it was added.
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

/// A single rule: every condition it has, and the text it gives.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CellRule {
    pub sheet: Option<String>,
    pub language: Option<u8>,
    pub row: Option<u32>,
    pub subrow: Option<u16>,
    pub column: Option<u16>,
    /// The cell's original text, exactly.
    pub original: Option<Vec<u8>>,
    pub text: Vec<u8>,
}

impl CellRule {
    fn fits(&self, sheet: &SheetInfo) -> bool {
        self.sheet.as_ref().is_none_or(|name| *name == sheet.name)
            && self
                .language
                .is_none_or(|language| language == sheet.language)
            && self
                .column
                .is_none_or(|column| sheet.columns.iter().any(|known| known.index == column))
    }

    fn lists(&self, row: u32, subrow: u16) -> bool {
        self.row.is_none_or(|known| known == row) && self.subrow.is_none_or(|known| known == subrow)
    }

    fn holds(&self, column: u16, original: &[u8]) -> bool {
        self.column.is_none_or(|known| known == column)
            && self
                .original
                .as_ref()
                .is_none_or(|known| known.as_slice() == original)
    }
}

/// Where a source's rules come from.
enum Rules {
    Table { table: OwnedTable, size: usize },
    Single(CellRule),
}

/// A plugin's rule or rule file, under the handle Lens gave it.
pub struct Entry {
    plugin: String,
    handle: u32,
    rules: Rules,
    rows_changed: AtomicU64,
    cells_replaced: AtomicU64,
    guard_mismatches: AtomicU64,
    rows_refused: AtomicU64,
    mismatched: Mutex<BTreeMap<String, Mismatch>>,
}

impl Entry {
    fn new(plugin: &str, handle: u32, rules: Rules) -> Self {
        Self {
            plugin: plugin.into(),
            handle,
            rules,
            rows_changed: AtomicU64::new(0),
            cells_replaced: AtomicU64::new(0),
            guard_mismatches: AtomicU64::new(0),
            rows_refused: AtomicU64::new(0),
            mismatched: Mutex::new(BTreeMap::new()),
        }
    }

    fn size(&self) -> usize {
        match &self.rules {
            Rules::Table { size, .. } => *size,
            Rules::Single(_) => 0,
        }
    }
}

#[derive(Default)]
struct Snapshot {
    generation: u64,
    /// In the order the plugins added them.
    entries: Vec<Arc<Entry>>,
}

#[derive(Default)]
pub struct ExcelText {
    snapshot: RwLock<Arc<Snapshot>>,
}

impl ExcelText {
    /// Adds `bytes`, a rule file in the text table format, as the plugin's
    /// rules under `handle`.
    ///
    /// # Errors
    /// Names why the file is invalid or the plugin has too many files.
    pub fn add_table(
        &self,
        plugin: &str,
        handle: u32,
        bytes: Vec<u8>,
    ) -> Result<TableInfo, String> {
        let size = bytes.len();
        let table = OwnedTable::new(bytes).map_err(|e| e.to_string())?;
        let view = table.table();
        let info = TableInfo {
            sheets: u32::try_from(view.sheet_count()).unwrap_or(u32::MAX),
            cells: view.cell_count() as u64,
        };
        let mut snapshot = self
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let files = snapshot
            .entries
            .iter()
            .filter(|e| e.plugin == plugin && matches!(e.rules, Rules::Table { .. }));
        if files.clone().count() >= MAX_TABLES_PER_PLUGIN {
            return Err(format!(
                "a plugin keeps at most {MAX_TABLES_PER_PLUGIN} rule files"
            ));
        }
        if files.map(|e| e.size()).sum::<usize>() + size > MAX_TABLES_SIZE_PER_PLUGIN {
            return Err(format!(
                "a plugin's rule files have at most {MAX_TABLES_SIZE_PER_PLUGIN} bytes together"
            ));
        }
        let entry = Arc::new(Entry::new(plugin, handle, Rules::Table { table, size }));
        Self::push(&mut snapshot, entry);
        Ok(info)
    }

    /// Adds a single rule of the plugin under `handle`.
    pub fn add_rule(&self, plugin: &str, handle: u32, rule: CellRule) {
        let mut snapshot = self
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        Self::push(
            &mut snapshot,
            Arc::new(Entry::new(plugin, handle, Rules::Single(rule))),
        );
    }

    fn push(snapshot: &mut Arc<Snapshot>, entry: Arc<Entry>) {
        let mut entries = snapshot.entries.clone();
        entries.push(entry);
        *snapshot = Arc::new(Snapshot {
            generation: snapshot.generation + 1,
            entries,
        });
    }

    /// Removes the plugin's rule or rule file under `handle`; false when it
    /// has none.
    pub fn remove(&self, plugin: &str, handle: u32) -> bool {
        self.retain(|e| !(e.plugin == plugin && e.handle == handle))
    }

    /// Removes every rule of the plugin.
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

    /// What the plugin's rule or rule file under `handle` did.
    #[must_use]
    pub fn status(&self, plugin: &str, handle: u32) -> Option<Status> {
        let snapshot = self.current();
        let entry = snapshot
            .entries
            .iter()
            .find(|e| e.plugin == plugin && e.handle == handle)?;
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

    /// Changes whenever a rule is added or removed; a cached
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

    /// The text the rules put in place of `original` in a cell: of the
    /// first, in the order they were added, that holds. `column` is the
    /// column's index among all of the sheet's columns.
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
            match &entry.rules {
                Rules::Single(rule) => {
                    let place = rule.sheet.as_ref().is_none_or(|name| name == sheet)
                        && rule.language.is_none_or(|known| known == language);
                    if place && rule.lists(row, subrow) && rule.holds(column, original) {
                        return Some(rule.text.clone());
                    }
                }
                Rules::Table { table, .. } => {
                    let table = table.table();
                    if table.language() != ANY_LANGUAGE
                        && language != 0
                        && table.language() != language
                    {
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
            }
        }
        None
    }

    /// The rules that apply to a sheet. Rule files that list the sheet but
    /// do not match it record why.
    #[must_use]
    pub fn bind(&self, sheet: &SheetInfo) -> SheetBinding {
        let snapshot = self.current();
        let mut parts = Vec::new();
        for entry in &snapshot.entries {
            let table = match &entry.rules {
                Rules::Single(rule) => {
                    if rule.fits(sheet) {
                        parts.push(Part::Rule(entry.clone()));
                    }
                    continue;
                }
                Rules::Table { table, .. } => table.table(),
            };
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
                None => parts.push(Part::Table(entry.clone(), index)),
            }
        }
        SheetBinding {
            generation: snapshot.generation,
            columns: sheet.columns.clone(),
            parts,
        }
    }
}

/// A source of rules bound to one sheet.
enum Part {
    /// A rule file and the index of the sheet in it.
    Table(Arc<Entry>, usize),
    Rule(Arc<Entry>),
}

impl Part {
    fn entry(&self) -> &Arc<Entry> {
        match self {
            Self::Table(entry, _) | Self::Rule(entry) => entry,
        }
    }
}

/// The rules that apply to one sheet, in the order they are tried.
pub struct SheetBinding {
    generation: u64,
    columns: Vec<Column>,
    parts: Vec<Part>,
}

/// What [`SheetBinding::rewrite`] did with a row.
#[derive(Debug, PartialEq, Eq)]
pub enum Rewrite {
    /// No rule holds for a cell of the row.
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

/// A part's rules for one row.
enum Listed<'a> {
    Cells(Vec<angelica_text::table::Cell<'a>>),
    Rule(&'a CellRule),
}

impl SheetBinding {
    /// A binding without rules, for a sheet that cannot be read.
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

    /// How many rules and rule files apply to the sheet.
    #[must_use]
    pub fn table_count(&self) -> usize {
        self.parts.len()
    }

    /// The sheet's text columns.
    #[must_use]
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Whether any bound rule lists cells of the row; the adapter reads the
    /// row's text only then.
    #[must_use]
    pub fn lists(&self, row: u32, subrow: u16) -> bool {
        self.parts.iter().any(|part| match part {
            Part::Table(entry, index) => match &entry.rules {
                Rules::Table { table, .. } => table
                    .table()
                    .sheet_by_index(*index)
                    .is_some_and(|sheet| !sheet.row(row, subrow).is_empty()),
                Rules::Single(_) => false,
            },
            Part::Rule(entry) => match &entry.rules {
                Rules::Single(rule) => rule.lists(row, subrow),
                Rules::Table { .. } => false,
            },
        })
    }

    /// The parts with rules for the row, and those rules.
    fn candidates(&self, row: u32, subrow: u16) -> Vec<(&Arc<Entry>, Listed<'_>)> {
        self.parts
            .iter()
            .filter_map(|part| match (part, &part.entry().rules) {
                (Part::Table(entry, index), Rules::Table { table, .. }) => {
                    let cells = table.table().sheet_by_index(*index)?.row(row, subrow);
                    (!cells.is_empty()).then_some((entry, Listed::Cells(cells)))
                }
                (Part::Rule(entry), Rules::Single(rule)) => rule
                    .lists(row, subrow)
                    .then_some((entry, Listed::Rule(rule))),
                _ => None,
            })
            .collect()
    }

    /// Rewrites a row of the sheet. `buffer` holds the row as the game built
    /// it: `fixed_size` bytes of fields, then each text column's string with
    /// its terminator, in column order. It may extend past the last string.
    #[must_use]
    pub fn rewrite(&self, row: u32, subrow: u16, buffer: &[u8], fixed_size: usize) -> Rewrite {
        let candidates = self.candidates(row, subrow);
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
            for (position, (entry, listed)) in candidates.iter().enumerate() {
                let replacement = match listed {
                    Listed::Rule(rule) => {
                        if !rule.holds(column.index, original) {
                            continue;
                        }
                        rule.text.as_slice()
                    }
                    Listed::Cells(cells) => {
                        let Some(cell) = cells.iter().find(|cell| cell.column == column.index)
                        else {
                            continue;
                        };
                        let Rules::Table { table, .. } = &entry.rules else {
                            continue;
                        };
                        let domain = table.table().domain();
                        let guard = if let Some(&(_, guard)) =
                            guards.iter().find(|(known, _)| *known == domain)
                        {
                            guard
                        } else {
                            let guard = angelica_text::guard(domain, original);
                            guards.push((domain, guard));
                            guard
                        };
                        if guard != cell.guard {
                            entry.guard_mismatches.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        cell.text
                    }
                };
                replacements[ordinal] = Some(replacement);
                applied[position] += 1;
                break;
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
    fn the_shown_text_of_a_cell_is_the_first_rule_that_holds() {
        let text = ExcelText::default();
        text.add_table("a", 1, table("A", &[(7, 4, b"Other")]))
            .unwrap();
        text.add_table("b", 1, table("Зелье", &[(7, 4, b"Potion")]))
            .unwrap();
        // The first file guards another text: the next one has it.
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
        text.add_table("a", 1, table("Зелье", &[(7, 4, b"Potion")]))
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
        let status = text.status("a", 1).unwrap();
        assert_eq!((status.rows_changed, status.cells_replaced), (1, 1));
    }

    #[test]
    fn single_rules_hold_by_their_conditions_in_the_order_they_were_added() {
        let text = ExcelText::default();
        let rule = |row: Option<u32>, original: Option<&[u8]>, shown: &str| CellRule {
            sheet: Some("Item".into()),
            row,
            column: Some(4),
            original: original.map(<[u8]>::to_vec),
            text: shown.as_bytes().to_vec(),
            ..CellRule::default()
        };
        text.add_rule("a", 1, rule(Some(7), Some(b"Other"), "A"));
        text.add_rule("a", 2, rule(Some(7), None, "B"));
        text.add_rule("b", 1, rule(None, None, "C"));
        text.add_rule(
            "b",
            2,
            CellRule {
                sheet: Some("Action".into()),
                ..rule(None, None, "D")
            },
        );
        let binding = text.bind(&sheet());
        assert_eq!(
            binding.table_count(),
            3,
            "the rule of another sheet is left out"
        );
        assert!(binding.lists(8, 0), "a rule without a row lists every row");
        let Rewrite::Changed(seven) = binding.rewrite(7, 0, &row(b"Name", b"Potion"), 12) else {
            panic!("row not changed");
        };
        assert_eq!(
            &seven[12..],
            b"Name\0B\0",
            "the first rule's original differs"
        );
        let Rewrite::Changed(eight) = binding.rewrite(8, 0, &row(b"Name", b"Potion"), 12) else {
            panic!("row not changed");
        };
        assert_eq!(&eight[12..], b"Name\0C\0");
        assert_eq!(text.status("a", 2).unwrap().cells_replaced, 1);
        assert_eq!(
            text.shown("Item", 2, 7, 0, 4, b"Other"),
            Some(b"A".to_vec())
        );
        assert!(text.remove("a", 2));
        assert!(!text.remove("a", 2));
        let binding = text.bind(&sheet());
        assert!(matches!(
            binding.rewrite(7, 0, &row(b"Name", b"Potion"), 12),
            Rewrite::Changed(rebuilt) if &rebuilt[12..] == b"Name\0C\0"
        ));
        // A rule of a column the sheet does not have never holds.
        let other = ExcelText::default();
        other.add_rule(
            "a",
            1,
            CellRule {
                column: Some(9),
                ..rule(None, None, "X")
            },
        );
        assert!(other.bind(&sheet()).is_empty());
    }

    #[test]
    fn guard_mismatch_keeps_text_and_tries_the_next_table() {
        let text = ExcelText::default();
        text.add_table("a", 1, table("A", &[(1, 1, b"Other")]))
            .unwrap();
        text.add_table("b", 1, table("B", &[(1, 1, b"Name")]))
            .unwrap();
        let binding = text.bind(&sheet());
        let Rewrite::Changed(rebuilt) = binding.rewrite(1, 0, &row(b"Name", b"x"), 12) else {
            panic!("row not changed");
        };
        assert_eq!(&rebuilt[12..14], b"B\0");
        assert_eq!(text.status("a", 1).unwrap().guard_mismatches, 1);
        assert_eq!(text.status("b", 1).unwrap().cells_replaced, 1);

        text.remove("b", 1);
        let binding = text.bind(&sheet());
        assert_eq!(
            binding.rewrite(1, 0, &row(b"Name", b"x"), 12),
            Rewrite::Unchanged
        );
    }

    #[test]
    fn sheets_that_differ_from_the_table_are_not_bound() {
        let text = ExcelText::default();
        text.add_table("a", 1, table("X", &[(1, 1, b"Name")]))
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
            text.status("a", 1).unwrap().sheets_mismatched,
            [("Item".to_owned(), Mismatch::Language)],
            "the last reason recorded for the sheet"
        );
    }

    #[test]
    fn unexpected_buffers_are_refused() {
        let text = ExcelText::default();
        text.add_table("a", 1, table("X", &[(1, 1, b"Name")]))
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
        assert_eq!(text.status("a", 1).unwrap().rows_refused, 3);
    }

    #[test]
    fn invalid_tables_and_too_many_tables_are_refused() {
        let text = ExcelText::default();
        assert!(text.add_table("a", 1, vec![1, 2, 3]).is_err());
        for handle in 0..MAX_TABLES_PER_PLUGIN {
            text.add_table(
                "a",
                u32::try_from(handle).unwrap(),
                table("X", &[(1, 1, b"Name")]),
            )
            .unwrap();
        }
        assert!(
            text.add_table("a", 99, table("X", &[(1, 1, b"Name")]))
                .is_err()
        );
        text.add_rule("a", 100, CellRule::default());
        let generation = text.generation();
        text.remove_plugin("a");
        assert!(text.generation() > generation);
        assert!(text.bind(&sheet()).is_empty());
    }
}
