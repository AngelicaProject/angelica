//! Text tables applied to the rows the game builds
//! (`docs/formats/game-structures.md`). The game hands every new row to its
//! sheet's row resolver through `StoreRow`; nothing else sees the row
//! before. The detour rebuilds the row buffer with the replaced text,
//! allocated and freed by the game's own Excel environment, and passes the
//! row on.
use super::{
    entries::{Entries, Function},
    memory::read,
};
use crate::{
    catalog::Catalog,
    excel_text::{ExcelText, Rewrite, SheetBinding, SheetInfo},
};
use angelica_hook::Detour;
use angelica_map::ValueKind::{Ptr, U8, U16, U32};
use angelica_text::table::{Column, Variant};
use std::{
    collections::HashMap,
    sync::{
        Arc, OnceLock, PoisonError, RwLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

pub const HASH_TABLE_STORE_ROW: &str = "excel.hash_table_page_row_resolver.store_row";
pub const RING_BUFFER_STORE_ROW: &str = "excel.ring_buffer_page_row_resolver.store_row";

/// Bytes of a column definition record: kind, index, and offset, each a
/// `u16` (`docs/formats/game-structures.md`).
const COLUMN_RECORD: usize = 6;
const MAX_COLUMNS: usize = 4096;
const MAX_NAME: usize = 256;
/// The game's variant values: the row parser adds `2 * variant` bytes of
/// subrow header to the fixed size.
const GAME_ROWS: u32 = 0;
const GAME_SUBROWS: u32 = 1;

/// Where the adapter finds what it reads, from the map.
struct Layout {
    environment: usize,
    allocate: usize,
    free: usize,
    row_data: usize,
    row_sheet: usize,
    sheet_version: usize,
    sheet_column_count: usize,
    sheet_columns: usize,
    sheet_data_offset: usize,
    sheet_variant: usize,
    sheet_language: usize,
    sheet_name: usize,
    column_kind: usize,
    column_offset: usize,
    row_id: usize,
    subrow_count: usize,
    subrow_ids: usize,
}

struct Cached {
    name: usize,
    column_count: u16,
    binding: Arc<SheetBinding>,
}

pub struct ExcelRows {
    text: Arc<ExcelText>,
    layout: Layout,
    sheets: RwLock<HashMap<usize, Cached>>,
    pub errors: AtomicU64,
    _detours: Vec<Detour>,
}

static ADAPTER: OnceLock<ExcelRows> = OnceLock::new();
// Counters that bound the notes the adapter logs.
static SEEN: AtomicU64 = AtomicU64::new(0);
static BOUND: AtomicU64 = AtomicU64::new(0);
static OTHER_VERSIONS: AtomicU64 = AtomicU64::new(0);
static HASH_TABLE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static RING_BUFFER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

type StoreRow = unsafe extern "system" fn(usize, usize, usize) -> u8;
type Allocate = unsafe extern "system" fn(usize, u64, u64) -> usize;
type Free = unsafe extern "system" fn(usize, usize) -> usize;

/// Installs the `StoreRow` detours once.
pub fn install(catalog: &Catalog, base: usize, text: Arc<ExcelText>) -> Result<(), String> {
    if ADAPTER.get().is_some() {
        return Ok(());
    }
    let (layout, hash_table, ring_buffer) = resolve(catalog, base)?;
    install_resolved(layout, &hash_table, &ring_buffer, text)
}

/// The adapter's layout and the two `StoreRow` functions, from the map.
fn resolve(catalog: &Catalog, base: usize) -> Result<(Layout, Function, Function), String> {
    let entries = Entries { catalog, base };
    let layout = Layout {
        environment: entries.global("excel.exd_environment.instance", Ptr)?,
        allocate: entries.field("excel.exd_environment.allocate", Ptr)?,
        free: entries.field("excel.exd_environment.free", Ptr)?,
        row_data: entries.field("excel.row.data", Ptr)?,
        row_sheet: entries.field("excel.row.sheet", Ptr)?,
        sheet_version: entries.field("excel.sheet.version", U16)?,
        sheet_column_count: entries.field("excel.sheet.column_count", U16)?,
        sheet_columns: entries.field("excel.sheet.column_definitions", Ptr)?,
        sheet_data_offset: entries.field("excel.sheet.data_offset", U32)?,
        sheet_variant: entries.field("excel.sheet.variant", U32)?,
        sheet_language: entries.field("excel.sheet.language", U32)?,
        sheet_name: entries.field("excel.sheet.name_ptr", Ptr)?,
        column_kind: entries.field("excel.column_definition.kind", U16)?,
        column_offset: entries.field("excel.column_definition.offset", U16)?,
        row_id: entries.field("excel.row_descriptor.row_id", U32)?,
        subrow_count: entries.field("excel.row_descriptor.subrow_count", U16)?,
        subrow_ids: entries.field("excel.row_descriptor.subrow_ids", U16)?,
    };
    if layout.column_kind + 2 > COLUMN_RECORD || layout.column_offset + 2 > COLUMN_RECORD {
        return Err("column definition fields lie outside the 6-byte record".into());
    }
    let hash_table = entries.function(HASH_TABLE_STORE_ROW, &[Ptr, Ptr, Ptr], U8)?;
    let ring_buffer = entries.function(RING_BUFFER_STORE_ROW, &[Ptr, Ptr, Ptr], U8)?;
    Ok((layout, hash_table, ring_buffer))
}

fn install_resolved(
    layout: Layout,
    hash_table: &Function,
    ring_buffer: &Function,
    text: Arc<ExcelText>,
) -> Result<(), String> {
    let mut detours = Vec::new();
    for (function, replacement, original) in [
        (
            hash_table,
            hash_table_store_row as StoreRow as usize,
            &HASH_TABLE_ORIGINAL,
        ),
        (
            ring_buffer,
            ring_buffer_store_row as StoreRow as usize,
            &RING_BUFFER_ORIGINAL,
        ),
    ] {
        // SAFETY: the map verified this address as the start of a function
        // with this ABI in the executable's code, which stays mapped; the
        // replacement has the same ABI.
        let detour = unsafe { Detour::new(function.address, replacement, function.length) }
            .map_err(|e| format!("StoreRow detour: {e}"))?;
        original.store(detour.trampoline(), Ordering::Release);
        detours.push(detour);
    }
    let adapter = ExcelRows {
        text,
        layout,
        sheets: RwLock::new(HashMap::new()),
        errors: AtomicU64::new(0),
        _detours: Vec::new(),
    };
    if ADAPTER.set(adapter).is_err() {
        return Ok(());
    }
    for detour in &mut detours {
        // SAFETY: the adapter and the trampolines the replacements call are
        // published above; nothing else in this process changes these bytes
        // while the detour is written.
        unsafe { detour.enable() }.map_err(|e| format!("StoreRow detour: {e}"))?;
    }
    // Detours stay installed until the process exits; without tables they
    // only pass rows on. Their memory is never freed.
    std::mem::forget(detours);
    Ok(())
}

unsafe extern "system" fn hash_table_store_row(
    resolver: usize,
    descriptor: usize,
    row: usize,
) -> u8 {
    translate(descriptor, row);
    // SAFETY: the trampoline runs the original function with its own ABI.
    unsafe { original(&HASH_TABLE_ORIGINAL)(resolver, descriptor, row) }
}

unsafe extern "system" fn ring_buffer_store_row(
    resolver: usize,
    descriptor: usize,
    row: usize,
) -> u8 {
    translate(descriptor, row);
    // SAFETY: as above.
    unsafe { original(&RING_BUFFER_ORIGINAL)(resolver, descriptor, row) }
}

fn original(slot: &AtomicUsize) -> StoreRow {
    let address = slot.load(Ordering::Acquire);
    // SAFETY: `install` stores the trampoline before the detour can run;
    // it has the StoreRow ABI.
    unsafe { std::mem::transmute::<usize, StoreRow>(address) }
}

fn translate(descriptor: usize, row: usize) {
    let Some(adapter) = ADAPTER.get() else { return };
    let result = super::guard(Err("the adapter panicked".into()), || {
        adapter.translate(descriptor, row)
    });
    if let Err(error) = result {
        adapter.errors.fetch_add(1, Ordering::Relaxed);
        note(&format!("row left as the game built it: {error}"));
    }
}

/// Writes one of the first lines about what the adapter does to the host
/// log, so a game build that differs from the contract shows why.
fn note(message: &str) {
    static NOTES: AtomicU64 = AtomicU64::new(0);
    if NOTES.fetch_add(1, Ordering::Relaxed) < 32 {
        super::log(&format!("excel rows: {message}"));
    }
}

// Fields of the row, sheet, and descriptor StoreRow receives are read
// directly, as the game's own code reads them during the same call: the
// objects are live, and the offsets come from verified map entries.
fn read_u16(address: usize) -> u16 {
    // SAFETY: see above.
    unsafe { std::ptr::read_unaligned(address as *const u16) }
}
fn read_u32(address: usize) -> u32 {
    // SAFETY: see above.
    unsafe { std::ptr::read_unaligned(address as *const u32) }
}
fn read_usize(address: usize) -> usize {
    // SAFETY: see above.
    unsafe { std::ptr::read_unaligned(address as *const usize) }
}

impl ExcelRows {
    fn translate(&self, descriptor: usize, row: usize) -> Result<(), String> {
        if descriptor == 0 || row == 0 {
            return Ok(());
        }
        let layout = &self.layout;
        let sheet = read_usize(row + layout.row_sheet);
        let data = read_usize(row + layout.row_data);
        // Small values tag an empty row.
        if sheet == 0 || data < 0x1_0000 {
            return Ok(());
        }
        let version = read_u16(sheet + layout.sheet_version);
        // Only version 3 is the layout below: the game parses later
        // versions with the same parser today, but a new version may bring
        // a new layout. The buffer layout is checked for every row.
        if version != 3 {
            if OTHER_VERSIONS.fetch_add(1, Ordering::Relaxed) < 4 {
                note(&format!("a sheet of version {version} is left untouched"));
            }
            return Ok(());
        }
        let Some(binding) = self.binding(sheet) else {
            return Ok(());
        };
        let row_id = read_u32(descriptor + layout.row_id);
        let subrow = if read_u32(sheet + layout.sheet_variant) == GAME_SUBROWS {
            // A subrow descriptor carries the subrow id as its single key.
            if read_u16(descriptor + layout.subrow_count) != 1 {
                return Ok(());
            }
            read_u16(descriptor + layout.subrow_ids)
        } else {
            0
        };
        if !binding.lists(row_id, subrow) {
            return Ok(());
        }
        let fixed = read_u32(sheet + layout.sheet_data_offset) as usize;
        let buffer = read_row(data, fixed, binding.column_count())?;
        let Rewrite::Changed(rebuilt) = binding.rewrite(row_id, subrow, &buffer, fixed) else {
            return Ok(());
        };
        self.replace(row, data, &rebuilt)
    }

    /// The sheet's binding, cached per sheet object while the sheet keeps
    /// its name and columns and no table changed.
    fn binding(&self, sheet: usize) -> Option<Arc<SheetBinding>> {
        let layout = &self.layout;
        let name = read_usize(sheet + layout.sheet_name);
        let column_count = read_u16(sheet + layout.sheet_column_count);
        let generation = self.text.generation();
        if let Some(cached) = self
            .sheets
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&sheet)
            && cached.name == name
            && cached.column_count == column_count
            && cached.binding.generation() == generation
        {
            return (!cached.binding.is_empty()).then(|| cached.binding.clone());
        }
        // A sheet the adapter cannot read is remembered as having no tables,
        // so its rows are not read again.
        let (binding, info) = match self.sheet_info(sheet, name, column_count) {
            Ok(info) => (Arc::new(self.text.bind(&info)), Some(info)),
            Err(error) => {
                note(&format!("a sheet is left untouched: {error}"));
                (Arc::new(SheetBinding::none(generation)), None)
            }
        };
        let seen = SEEN.fetch_add(1, Ordering::Relaxed);
        if let Some(info) = info
            && (seen < 4 || (!binding.is_empty() && BOUND.fetch_add(1, Ordering::Relaxed) < 4))
        {
            note(&format!(
                "sheet {} ({:?}, language {}, {} text columns): {} tables apply",
                info.name,
                info.variant,
                info.language,
                info.columns.len(),
                binding.table_count()
            ));
        }
        self.sheets
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                sheet,
                Cached {
                    name,
                    column_count,
                    binding: binding.clone(),
                },
            );
        (!binding.is_empty()).then_some(binding)
    }

    fn sheet_info(
        &self,
        sheet: usize,
        name: usize,
        column_count: u16,
    ) -> Result<SheetInfo, String> {
        let layout = &self.layout;
        if name == 0 || usize::from(column_count) > MAX_COLUMNS {
            return Err("sheet without a name or with too many columns".into());
        }
        let name = read_c_string(name, MAX_NAME)?;
        let variant = match read_u32(sheet + layout.sheet_variant) {
            GAME_ROWS => Variant::Rows,
            GAME_SUBROWS => Variant::Subrows,
            other => return Err(format!("sheet {name}: unknown variant {other}")),
        };
        let language = u8::try_from(read_u32(sheet + layout.sheet_language))
            .map_err(|_| "language out of range")?;
        let definitions = read_usize(sheet + layout.sheet_columns);
        let records = read(definitions, usize::from(column_count) * COLUMN_RECORD)?;
        let mut columns = Vec::new();
        for (index, record) in records.as_chunks::<COLUMN_RECORD>().0.iter().enumerate() {
            let field = |at: usize| u16::from_le_bytes([record[at], record[at + 1]]);
            if field(layout.column_kind) == 0 {
                columns.push(Column {
                    index: u16::try_from(index).map_err(|_| "too many columns")?,
                    offset: field(layout.column_offset),
                });
            }
        }
        Ok(SheetInfo {
            name,
            variant,
            language,
            columns,
        })
    }

    /// Swaps in the rebuilt buffer, allocated and freed exactly as the game
    /// allocates and frees row buffers.
    fn replace(&self, row: usize, old: usize, rebuilt: &[u8]) -> Result<(), String> {
        let layout = &self.layout;
        let environment = read_usize(layout.environment);
        if environment == 0 {
            return Err("the Excel environment does not exist".into());
        }
        let vtable = read_usize(environment);
        // SAFETY: the map verified these vtable slots as the environment's
        // allocate and free functions with these ABIs; `environment` is the
        // live instance the game allocates row buffers from.
        let (allocate, free) = unsafe {
            (
                std::mem::transmute::<usize, Allocate>(read_usize(vtable + layout.allocate)),
                std::mem::transmute::<usize, Free>(read_usize(vtable + layout.free)),
            )
        };
        // SAFETY: called as the game's row parser calls it.
        let target = unsafe { allocate(environment, rebuilt.len() as u64, 0) };
        if target == 0 {
            return Err("the Excel environment could not allocate a row".into());
        }
        // SAFETY: `target` is a fresh allocation of `rebuilt.len()` bytes;
        // `row` is the live row the game passed to StoreRow, whose data
        // field holds `old`, and no other code has seen the row yet.
        unsafe {
            std::ptr::copy_nonoverlapping(rebuilt.as_ptr(), target as *mut u8, rebuilt.len());
            std::ptr::write_unaligned((row + layout.row_data) as *mut usize, target);
            free(environment, old);
        }
        Ok(())
    }
}

/// The row buffer: the fixed part and `strings` terminated strings after
/// it, read in chunks that stop at the end of readable memory.
fn read_row(data: usize, fixed: usize, strings: usize) -> Result<Vec<u8>, String> {
    const CHUNK: usize = 1024;
    if fixed == 0 || fixed > crate::excel_text::MAX_ROW_SIZE {
        return Err("row fixed size out of range".into());
    }
    let mut buffer = read(data, fixed)?;
    let mut terminators = 0;
    while terminators < strings {
        if buffer.len() >= crate::excel_text::MAX_ROW_SIZE {
            return Err("row strings exceed the row size limit".into());
        }
        let at = data + buffer.len();
        let chunk = read(at, CHUNK).or_else(|_| read(at, 1))?;
        for &byte in &chunk {
            buffer.push(byte);
            if byte == 0 {
                terminators += 1;
                if terminators == strings {
                    break;
                }
            }
        }
    }
    Ok(buffer)
}

fn read_c_string(address: usize, limit: usize) -> Result<String, String> {
    let mut bytes = Vec::new();
    while bytes.len() < limit {
        let byte = read(address + bytes.len(), 1)?[0];
        if byte == 0 {
            return String::from_utf8(bytes).map_err(|_| "sheet name is not UTF-8".into());
        }
        bytes.push(byte);
    }
    Err("sheet name is too long".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_text::{guard, table::Writer};
    use std::sync::Mutex;

    static ALLOCATED: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    static FREED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

    unsafe extern "system" fn allocate(_: usize, size: u64, zero: u64) -> usize {
        assert_eq!(zero, 0);
        let buffer = vec![0xAAu8; usize::try_from(size).unwrap()].leak();
        let address = buffer.as_mut_ptr() as usize;
        ALLOCATED.lock().unwrap().push(address);
        address
    }

    unsafe extern "system" fn free(_: usize, buffer: usize) -> usize {
        FREED.lock().unwrap().push(buffer);
        0
    }

    /// A row with a 12-byte fixed part: text columns 0 and 2 at offsets 0
    /// and 8, a number column 1 at offset 4.
    fn row_buffer(first: &[u8], second: &[u8]) -> Vec<u8> {
        let mut buffer = vec![0; 12];
        buffer[4..8].copy_from_slice(&0x1234_5678u32.to_le_bytes());
        let second_distance = u32::try_from(12 + first.len() + 1 - 8).unwrap();
        buffer[0..4].copy_from_slice(&(0x0Cu32 | (0x11 << 24)).to_le_bytes());
        buffer[8..12].copy_from_slice(&(second_distance | (0x22 << 24)).to_le_bytes());
        buffer.extend_from_slice(first);
        buffer.push(0);
        buffer.extend_from_slice(second);
        buffer.push(0);
        buffer
    }

    /// The layout of the synthetic structures, with an environment whose
    /// vtable holds `allocate` and `free`.
    fn synthetic_layout() -> Layout {
        let vtable: &'static [usize; 3] = Box::leak(Box::new([
            0,
            allocate as *const () as usize,
            free as *const () as usize,
        ]));
        let environment: &'static usize = Box::leak(Box::new(std::ptr::from_ref(vtable) as usize));
        let global: &'static usize = Box::leak(Box::new(std::ptr::from_ref(environment) as usize));
        Layout {
            environment: std::ptr::from_ref(global) as usize,
            allocate: 8,
            free: 16,
            row_data: 0,
            row_sheet: 8,
            sheet_version: 0,
            sheet_column_count: 2,
            sheet_columns: 8,
            sheet_data_offset: 16,
            sheet_variant: 20,
            sheet_language: 24,
            sheet_name: 32,
            column_kind: 0,
            column_offset: 4,
            row_id: 0,
            subrow_count: 6,
            subrow_ids: 8,
        }
    }

    /// A version 3 sheet "Item" of rows in English with columns text,
    /// number (kind 7), text.
    fn synthetic_sheet() -> &'static [u8] {
        // Column definitions: text, number (kind 7), text.
        let definitions: &'static mut [u8] = vec![0u8; 18].leak();
        definitions[6..8].copy_from_slice(&7u16.to_le_bytes());
        definitions[4..6].copy_from_slice(&0u16.to_le_bytes());
        definitions[10..12].copy_from_slice(&4u16.to_le_bytes());
        definitions[16..18].copy_from_slice(&8u16.to_le_bytes());
        let name: &'static [u8] = b"Item\0";
        let sheet: &'static mut [u8] = vec![0u8; 64].leak();
        sheet[0..2].copy_from_slice(&3u16.to_le_bytes());
        sheet[2..4].copy_from_slice(&3u16.to_le_bytes());
        sheet[8..16].copy_from_slice(&(definitions.as_ptr() as usize).to_le_bytes());
        sheet[16..20].copy_from_slice(&12u32.to_le_bytes());
        sheet[20..24].copy_from_slice(&GAME_ROWS.to_le_bytes());
        sheet[24..28].copy_from_slice(&2u32.to_le_bytes());
        sheet[32..40].copy_from_slice(&(name.as_ptr() as usize).to_le_bytes());
        sheet
    }

    #[test]
    fn store_row_detour_rebuilds_listed_rows_with_the_game_allocator() {
        // `mov eax, 7; ret`, long enough to detour.
        let code = [0xB8, 7, 0, 0, 0, 0xC3, 0x90, 0x90, 0x90, 0x90];
        let hash_table = Function {
            address: angelica_hook::testing::executable(&code),
            length: Some(code.len()),
        };
        let ring_buffer = Function {
            address: angelica_hook::testing::executable(&code),
            length: Some(code.len()),
        };
        let layout = synthetic_layout();

        let text = Arc::new(ExcelText::default());
        let mut writer = Writer::new(2, b"T").unwrap();
        let columns = [
            Column {
                index: 0,
                offset: 0,
            },
            Column {
                index: 2,
                offset: 8,
            },
        ];
        writer.sheet("Item", Variant::Rows, &columns).unwrap();
        writer
            .cell("Item", 5, 0, 2, guard(b"T", b"Potion"), "Зелье".as_bytes())
            .unwrap();
        text.add_table("test", 1, writer.finish().unwrap()).unwrap();
        install_resolved(layout, &hash_table, &ring_buffer, text.clone()).unwrap();

        let sheet = synthetic_sheet();

        let store = |row_id: u32, original: Vec<u8>| -> (u8, Vec<u8>, usize) {
            let original = original.leak();
            let mut row = [0u8; 16];
            row[0..8].copy_from_slice(&(original.as_ptr() as usize).to_le_bytes());
            row[8..16].copy_from_slice(&(sheet.as_ptr() as usize).to_le_bytes());
            let mut descriptor = [0u8; 16];
            descriptor[0..4].copy_from_slice(&row_id.to_le_bytes());
            // SAFETY: the detoured synthetic function takes three pointers
            // and returns 7.
            let call: StoreRow =
                unsafe { std::mem::transmute::<usize, StoreRow>(hash_table.address) };
            // SAFETY: the row and descriptor live across the call.
            let result = unsafe { call(0, descriptor.as_ptr() as usize, row.as_ptr() as usize) };
            let data = usize::from_le_bytes(row[0..8].try_into().unwrap());
            (result, read_row(data, 12, 2).unwrap(), data)
        };

        let (result, rebuilt, data) = store(5, row_buffer(b"Name", b"Potion"));
        assert_eq!(result, 7, "the original runs");
        let mut expected = row_buffer(b"Name", "Зелье".as_bytes());
        expected[11] = crate::excel_text::string_hash("Зелье".as_bytes());
        assert_eq!(rebuilt, expected);
        assert_eq!(ALLOCATED.lock().unwrap().as_slice(), [data]);
        assert_eq!(FREED.lock().unwrap().len(), 1, "the old buffer is freed");

        let (_, unchanged, _) = store(5, row_buffer(b"Name", b"Ether"));
        assert_eq!(unchanged, row_buffer(b"Name", b"Ether"), "guard mismatch");
        let (_, other, _) = store(6, row_buffer(b"Name", b"Potion"));
        assert_eq!(
            other,
            row_buffer(b"Name", b"Potion"),
            "a row the table does not list"
        );
        assert_eq!(ALLOCATED.lock().unwrap().len(), 1);
        assert_eq!(text.status("test", 1).unwrap().guard_mismatches, 1);

        text.remove("test", 1);
        let (_, removed, _) = store(5, row_buffer(b"Name", b"Potion"));
        assert_eq!(
            removed,
            row_buffer(b"Name", b"Potion"),
            "no table, no change"
        );
    }
}
