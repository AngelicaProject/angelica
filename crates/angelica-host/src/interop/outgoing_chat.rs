//! Restore game-supplied labels in outgoing chat links after native encoding.
use super::{entries::Entries, game::WindowsGame, memory};
use crate::{excel_text::ExcelText, game_data::GameData, sheets::Sheets};
use angelica_hook::Detour;
use angelica_map::{
    ColumnType,
    ValueKind::{I64, Ptr, U8, U32, Void},
};
use angelica_text::sestring::{
    self,
    chat_links::{self, Name},
};
use std::{
    cell::OnceCell,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

type Encode = unsafe extern "system" fn(usize, usize, u32, u8) -> usize;
type Getter = unsafe extern "system" fn(usize) -> usize;
type SetString = unsafe extern "system" fn(usize, *const u8);

static ADAPTER: OnceLock<Adapter> = OnceLock::new();
static INSTALLED: OnceLock<()> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static WARNINGS: AtomicUsize = AtomicUsize::new(0);

struct Adapter {
    excel: Arc<ExcelText>,
    data: Arc<GameData>,
    sheets: Arc<Sheets>,
    thread: u32,
    framework: usize,
    ui_module: usize,
    excel_module: usize,
    get_language: usize,
    string_ptr: usize,
    used_size: usize,
    set_string: usize,
}

pub(super) fn install(game: &WindowsGame) {
    INSTALLED.get_or_init(|| match install_detour(game) {
        Ok(()) => super::log("outgoing chat links hook: in place"),
        Err(error) => super::log(&format!("outgoing chat links hook: not installed: {error}")),
    });
}

fn install_detour(game: &WindowsGame) -> Result<(), String> {
    let entries = Entries {
        catalog: &game.catalog,
        base: game.base,
    };
    let function = entries.function(
        "chat.pronoun_module.encode_links",
        &[Ptr, Ptr, U32, U8],
        Ptr,
    )?;
    let adapter = Adapter {
        excel: game.excel.clone(),
        data: game.data.clone(),
        sheets: game.sheets.clone(),
        thread: thread_id(),
        framework: entries.global("framework.instance", Ptr)?,
        ui_module: entries
            .function("framework.get_ui_module", &[Ptr], Ptr)?
            .address,
        excel_module: entries.field("ui.ui_module.excel_module", Ptr)?,
        get_language: entries
            .function("excel.module_interface.get_language", &[Ptr], Ptr)?
            .address,
        string_ptr: entries.field("text.utf8_string.string_ptr", Ptr)?,
        used_size: entries.field("text.utf8_string.used_size", I64)?,
        set_string: entries
            .function("text.utf8_string.set_string", &[Ptr, Ptr], Void)?
            .address,
    };
    // SAFETY: the map verified the function's entry and ABI. The detour uses that ABI.
    let mut detour =
        unsafe { Detour::new(function.address, encode as Encode as usize, &function.code?) }
            .map_err(|error| error.to_string())?;
    ORIGINAL.store(detour.trampoline(), Ordering::Release);
    ADAPTER
        .set(adapter)
        .map_err(|_| "the chat adapter is already set")?;
    // SAFETY: the adapter and trampoline are available before the hook can run.
    unsafe { detour.enable() }.map_err(|error| error.to_string())?;
    std::mem::forget(detour);
    Ok(())
}

fn thread_id() -> u32 {
    // SAFETY: this Windows query has no arguments or memory effects.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

unsafe extern "system" fn encode(module: usize, input: usize, limit: u32, flags: u8) -> usize {
    // SAFETY: the trampoline has the game's ABI and gets the game's own arguments.
    let result = unsafe {
        std::mem::transmute::<usize, Encode>(ORIGINAL.load(Ordering::Acquire))(
            module, input, limit, flags,
        )
    };
    if let Some(adapter) = ADAPTER.get()
        && result != 0
        && result != input
        && thread_id() == adapter.thread
        && adapter.excel.generation() != 0
    {
        let error = match super::guard(None, || Some(adapter.restore(result, limit as usize))) {
            Some(Ok(())) => None,
            Some(Err(error)) => Some(format!("{error:?}")),
            None => Some("the adapter panicked".to_owned()),
        };
        if let Some(error) = error
            && WARNINGS.fetch_add(1, Ordering::Relaxed) < 5
        {
            super::log(&format!("outgoing chat link restoration skipped: {error}"));
        }
    }
    result
}

fn read<const N: usize>(address: usize) -> Option<[u8; N]> {
    memory::read(address, N).ok()?.try_into().ok()
}

fn pointer(address: usize) -> Option<usize> {
    let value = usize::from_le_bytes(read(address)?);
    (value != 0).then_some(value)
}

impl Adapter {
    fn language(&self) -> Option<u8> {
        let framework = pointer(self.framework)?;
        // SAFETY: this runs on the game's main thread. The framework global points to
        // the live framework and the getter was verified from the map.
        let ui = unsafe { std::mem::transmute::<usize, Getter>(self.ui_module)(framework) };
        if ui == 0 {
            return None;
        }
        let excel = pointer(ui.checked_add(self.excel_module)?)?;
        // SAFETY: the map identifies this field as the receiver of this getter.
        let language = unsafe { std::mem::transmute::<usize, Getter>(self.get_language)(excel) };
        let code = u32::from_le_bytes(read(language)?);
        u8::try_from(code).ok()?.checked_add(1)
    }

    fn original_name(&self, name: Name, language: u8) -> Option<String> {
        let (sheet, row) = match name {
            Name::Item(row) => ("Item", row),
            Name::EventItem(row) => ("EventItem", row),
            Name::Quest(row) => ("Quest", row),
        };
        let column = self
            .sheets
            .fields(sheet, &self.data)
            .ok()?
            .into_iter()
            .find(|field| field.name == "name" && field.column_type == ColumnType::Text)?;
        let bytes = self
            .data
            .text_cell(sheet, language, row, column.index)
            .ok()??;
        Some(sestring::plain(&bytes))
    }

    fn restore(&self, result: usize, limit: usize) -> Result<(), chat_links::Error> {
        let limit = limit.min(sestring::MAX_LENGTH);
        let Some(used) = result
            .checked_add(self.used_size)
            .and_then(read)
            .map(i64::from_le_bytes)
            .and_then(|used| usize::try_from(used).ok())
            .filter(|&used| used > 1 && used - 1 <= limit)
        else {
            return Ok(());
        };
        let Some(bytes) = result
            .checked_add(self.string_ptr)
            .and_then(pointer)
            .and_then(|address| memory::read(address, used).ok())
        else {
            return Ok(());
        };
        let Some((&0, line)) = bytes.split_last() else {
            return Ok(());
        };
        let language = OnceCell::new();
        let Some(mut replacement) = chat_links::restore(line, limit, |name| {
            self.original_name(name, (*language.get_or_init(|| self.language()))?)
        })?
        else {
            return Ok(());
        };
        replacement.push(0);
        // SAFETY: the encoder returned a live initialized game string. The verified
        // setter copies the validated sequence, including its NUL, into game-owned storage.
        unsafe {
            std::mem::transmute::<usize, SetString>(self.set_string)(result, replacement.as_ptr());
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct TestString {
        bytes: *const u8,
        used: i64,
        written: Vec<u8>,
    }

    unsafe extern "system" fn set_string(result: usize, bytes: *const u8) {
        // SAFETY: the test passes its live TestString and the adapter passes a NUL-terminated buffer.
        unsafe {
            (*(result as *mut TestString)).written =
                std::ffi::CStr::from_ptr(bytes.cast()).to_bytes().to_vec();
        }
    }

    #[test]
    fn calls_the_game_setter_with_a_complete_nul_terminated_replacement() {
        let adapter = Adapter {
            excel: Arc::default(),
            data: Arc::new(GameData::new(None)),
            sheets: Arc::default(),
            thread: thread_id(),
            framework: 0,
            ui_module: 0,
            excel_module: 0,
            get_language: 0,
            string_ptr: std::mem::offset_of!(TestString, bytes),
            used_size: std::mem::offset_of!(TestString, used),
            set_string: set_string as SetString as usize,
        };
        let encoded = b"See \x02\x2e\x0b\xc9\x0b\x64\x02\xff\x02X\xff\x02Y\x03 now.\0";
        let mut result = TestString {
            bytes: encoded.as_ptr(),
            used: i64::try_from(encoded.len()).unwrap(),
            written: Vec::new(),
        };
        let address = std::ptr::from_mut(&mut result) as usize;
        assert_eq!(adapter.restore(address, encoded.len() - 2), Ok(()));
        assert!(result.written.is_empty());

        assert_eq!(adapter.restore(address, encoded.len() - 1), Ok(()));
        assert_eq!(
            result.written,
            b"See \x02\x2e\x06\xc9\x0b\x64\x01\x01\x03 now."
        );
    }
}
