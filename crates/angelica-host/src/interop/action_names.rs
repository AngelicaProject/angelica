//! Retry native shell lookup with a translated name after an original name misses.
// Adapted from Harmonia. Based on Harmonia (https://github.com/AngelicaProject/Harmonia),
// Copyright (C) 2026 pokeda.
use super::{
    entries::Entries,
    game::WindowsGame,
    game_string::{Strings, TextLayout, pointer, read},
    memory,
};
use crate::{excel_text::ExcelText, game_data::GameData, sheets::Sheets};
use angelica_hook::Detour;
use angelica_map::{
    ColumnType,
    ValueKind::{I16, I32, Ptr, U8, U32},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

const SHEETS: [&str; 5] = ["Action", "CraftAction", "GeneralAction", "Pet", "PetMirage"];
const MAX_NAME: usize = 512;
type Match = unsafe extern "system" fn(usize, usize, usize, usize, i32, i32) -> u8;
type Names = BTreeMap<String, Vec<Alias>>;
type Cache = BTreeMap<(&'static str, u8, u16), Arc<Names>>;

struct Alias {
    shown: Vec<u8>,
    rows: Vec<u32>,
}

impl Alias {
    fn accepts(&self, row: i32) -> bool {
        u32::try_from(row).is_ok_and(|row| self.rows.contains(&row))
    }
}

struct Adapter {
    excel: Arc<ExcelText>,
    data: Arc<GameData>,
    sheets: Arc<Sheets>,
    layout: TextLayout,
    strings: Strings,
    sheet: usize,
    column: usize,
    language: usize,
    aliases: Mutex<(u64, Cache)>,
}

static ADAPTER: OnceLock<Adapter> = OnceLock::new();
static INSTALLED: OnceLock<()> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

pub(super) fn install(game: &WindowsGame) {
    INSTALLED.get_or_init(|| match install_detour(game) {
        Ok(()) => super::log("original command names hook: in place"),
        Err(error) => super::log(&format!(
            "original command names hook: not installed: {error}"
        )),
    });
}

fn install_detour(game: &WindowsGame) -> Result<(), String> {
    let entries = Entries {
        catalog: &game.catalog,
        base: game.base,
    };
    let function = entries.function(
        "shell.command_module.match_sheet_name",
        &[Ptr, Ptr, Ptr, Ptr, I32, I32],
        U8,
    )?;
    let adapter = Adapter {
        excel: game.excel.clone(),
        data: game.data.clone(),
        sheets: game.sheets.clone(),
        layout: TextLayout::resolve(&entries)?,
        strings: Strings::resolve(&entries)?,
        sheet: entries.field("shell.name_match_entry.sheet", Ptr)?,
        column: entries.field("shell.name_match_entry.column", I16)?,
        language: entries.field("excel.sheet.language", U32)?,
        aliases: Mutex::default(),
    };
    // SAFETY: the game map verified the entry and matcher ABI.
    let mut detour =
        unsafe { Detour::new(function.address, lookup as Match as usize, &function.code?) }
            .map_err(|error| error.to_string())?;
    ORIGINAL.store(detour.trampoline(), Ordering::Release);
    ADAPTER
        .set(adapter)
        .map_err(|_| "the command matcher is already set")?;
    // SAFETY: the trampoline and adapter were published before enabling the detour.
    unsafe { detour.enable() }.map_err(|error| error.to_string())?;
    std::mem::forget(detour);
    Ok(())
}

unsafe extern "system" fn lookup(
    module: usize,
    name: usize,
    out: usize,
    entry: usize,
    start: i32,
    end: i32,
) -> u8 {
    // SAFETY: the published trampoline is the game's matcher and has this ABI.
    let original = unsafe { std::mem::transmute::<usize, Match>(ORIGINAL.load(Ordering::Acquire)) };
    // SAFETY: forward the game's original arguments verbatim.
    let matched = unsafe { original(module, name, out, entry, start, end) };
    if matched != 0 || out == 0 || entry == 0 || name == 0 {
        return matched;
    }
    u8::from(super::guard(false, || {
        let Some(adapter) = ADAPTER.get() else {
            return false;
        };
        adapter.retry(name, entry, |alias| {
            let mut row = -1_i32;
            let accepted = adapter.strings.with(&alias.shown, |changed| {
                // SAFETY: the same native matcher sees the same descriptor and bounds,
                // a constructed temporary string and writable local storage for its result.
                unsafe { original(module, changed, (&raw mut row) as usize, entry, start, end) }
            });
            if accepted != 0 && alias.accepts(row) {
                return memory::write(out, &row.to_le_bytes()).is_ok();
            }
            false
        })
    }))
}

fn name(bytes: &[u8]) -> Option<&str> {
    (!bytes.is_empty() && bytes.len() <= MAX_NAME && !bytes.iter().any(|&b| b < 0x20))
        .then(|| std::str::from_utf8(bytes).ok())
        .flatten()
}

fn record(aliases: &mut Names, row: u32, original: &[u8], shown: &[u8]) {
    let (Some(original), Some(shown)) = (name(original), name(shown)) else {
        return;
    };
    let original = original.to_lowercase();
    if original == shown.to_lowercase() {
        return;
    }
    let matches = aliases.entry(original).or_default();
    if let Some(alias) = matches
        .iter_mut()
        .find(|alias| alias.shown == shown.as_bytes())
    {
        alias.rows.push(row);
    } else {
        matches.push(Alias {
            shown: shown.as_bytes().to_vec(),
            rows: vec![row],
        });
    }
}

impl Adapter {
    fn retry(&self, input: usize, entry: usize, mut retry: impl FnMut(&Alias) -> bool) -> bool {
        let Some(aliases) = self.aliases(entry) else {
            return false;
        };
        let Some(input) = self.layout.text(input, MAX_NAME) else {
            return false;
        };
        let Some(name) = name(&input) else {
            return false;
        };
        aliases
            .get(&name.to_lowercase())
            .is_some_and(|aliases| aliases.iter().take(32).any(&mut retry))
    }

    fn aliases(&self, entry: usize) -> Option<Arc<Names>> {
        let generation = self.excel.generation();
        if generation == 0 {
            return None;
        }
        let sheet = self.layout.text(entry, 32)?;
        let sheet = SHEETS.into_iter().find(|s| s.as_bytes() == sheet)?;
        let column =
            u16::try_from(i16::from_le_bytes(read(entry.checked_add(self.column)?)?)).ok()?;
        let native = pointer(entry.checked_add(self.sheet)?)?;
        let language = u8::try_from(u32::from_le_bytes(read(
            native.checked_add(self.language)?,
        )?))
        .ok()?;
        let key = (sheet, language, column);
        {
            let cache = self.aliases.lock().ok()?;
            if cache.0 == generation
                && let Some(aliases) = cache.1.get(&key)
            {
                return Some(aliases.clone());
            }
        }
        if !self
            .sheets
            .fields(sheet, &self.data)
            .ok()?
            .iter()
            .any(|field| {
                field.name == "name"
                    && field.column_type == ColumnType::Text
                    && field.index == column
            })
        {
            return None;
        }
        let binding = self.data.text_binding(sheet, language, &self.excel).ok()?;
        let mut aliases = Names::new();
        if !binding.is_empty() {
            self.data
                .visit_text(sheet, language, column, |row, original| {
                    if let Some(shown) = binding.shown(row, column, original) {
                        record(&mut aliases, row, original, shown);
                    }
                })
                .ok()?;
        }
        let aliases = Arc::new(aliases);
        let mut cache = self.aliases.lock().ok()?;
        if generation != self.excel.generation() {
            return None;
        }
        if cache.0 != generation {
            *cache = (generation, Cache::new());
        }
        cache.1.insert(key, aliases.clone());
        Some(aliases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identical_translation_cannot_redirect_an_original_name_to_another_action() {
        let mut aliases = Names::new();
        record(&mut aliases, 1, b"First action", b"Same name");
        record(&mut aliases, 2, b"Second action", b"Same name");
        record(&mut aliases, 3, b"SECOND ACTION", b"Same name");
        record(&mut aliases, 4, b"With macros", b"\x02\x10\x01\x03");
        record(&mut aliases, 5, b"Unchanged", b"unchanged");
        let alias = &aliases["second action"][0];
        assert!(!alias.accepts(1));
        assert!(alias.accepts(2));
        assert!(alias.accepts(3));
        assert!(!alias.accepts(-1));
        assert!(!aliases.contains_key("with macros"));
        assert!(!aliases.contains_key("unchanged"));
    }
}
