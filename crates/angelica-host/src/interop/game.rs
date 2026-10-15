//! The game process as the host's services see it.
use crate::{catalog::Catalog, excel_text::ExcelText, game::Game, text_case::TextCase};
use std::{path::PathBuf, sync::Arc};

pub struct WindowsGame {
    pub catalog: Arc<Catalog>,
    pub base: usize,
    pub excel: Arc<ExcelText>,
    pub case: Arc<TextCase>,
    pub files: Arc<crate::file_replacements::FileReplacements>,
    pub commands: Arc<crate::commands::Commands>,
    pub lens: Arc<crate::lens::Lens>,
    pub version: String,
}

impl WindowsGame {
    /// The contents of `ffxivgame.ver` next to the executable.
    pub fn read_version(executable: &std::path::Path) -> String {
        let path: PathBuf = executable.with_file_name("ffxivgame.ver");
        std::fs::read_to_string(path)
            .map(|text| text.trim().to_owned())
            .unwrap_or_default()
    }
}

/// Logs whether the hooks of `what` are in place, and passes `result` on.
fn hooked(what: &str, result: Result<(), String>) -> Result<(), String> {
    match &result {
        Ok(()) => super::log(&format!("{what} hooks: in place")),
        Err(error) => super::log(&format!("{what} hooks: not installed: {error}")),
    }
    result
}

/// The game's memory, read through Windows so that an unreadable address
/// fails instead of faulting.
pub(super) struct ProcessMemory;

impl crate::lens::Memory for ProcessMemory {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        let Ok(address) = usize::try_from(address) else {
            return false;
        };
        match super::memory::read(address, buffer.len()) {
            Ok(bytes) => {
                buffer.copy_from_slice(&bytes);
                true
            }
            Err(_) => false,
        }
    }
}

impl Game for WindowsGame {
    fn version(&self) -> String {
        self.version.clone()
    }

    fn enable_excel_text(&self) -> Result<(), String> {
        hooked(
            "game text",
            super::excel_rows::install(&self.catalog, self.base, self.excel.clone()),
        )
    }

    fn enable_text_case(&self) -> Result<(), String> {
        hooked(
            "letter case",
            super::text_case::install(&self.catalog, self.base, self.case.clone()),
        )
    }

    fn enable_file_replacements(&self) -> Result<(), String> {
        hooked(
            "file replacements",
            super::file_reads::install(&self.catalog, self.base, self.files.clone()),
        )
    }

    fn enable_commands(&self) -> Result<(), String> {
        hooked(
            "chat commands",
            super::chat::install_commands(&self.catalog, self.base, self.commands.clone()),
        )
    }

    fn print(&self, message: &[u8]) -> Result<(), String> {
        super::chat::print(&self.catalog, self.base, message)
    }

    fn query(
        &self,
        prepared: &crate::lens::Prepared,
    ) -> crate::error::Result<Vec<crate::lens::Row>> {
        crate::lens::run(&ProcessMemory, prepared)
    }

    fn memory(&self) -> &dyn crate::lens::Memory {
        &ProcessMemory
    }

    fn call(&self, address: u64, args: &[u64]) -> Result<crate::lens::act::Returned, String> {
        super::act::call(address, args)
    }

    fn view_projection(&self) -> Option<[f32; 16]> {
        let control = self.catalog.present.get("camera.control.instance")?;
        let matrix = self.catalog.present.get("camera.control.view_projection")?;
        let (angelica_map::Value::Address(rva), angelica_map::Value::Offset(offset)) =
            (control.value, matrix.value)
        else {
            return None;
        };
        let address = self.base + rva as usize + offset as usize;
        let bytes = super::memory::read(address, 64).ok()?;
        let mut values = [0f32; 16];
        for (value, chunk) in values.iter_mut().zip(bytes.as_chunks::<4>().0) {
            *value = f32::from_le_bytes(*chunk);
        }
        values
            .iter()
            .all(|value| value.is_finite())
            .then_some(values)
    }

    fn play_sound(&self, wav: Vec<u8>) -> bool {
        super::sound::play(wav)
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        usize::try_from(address).is_ok_and(|address| super::memory::write(address, bytes).is_ok())
    }

    fn enable_decision(&self, point: usize, entry: &str, address: u64) -> Result<(), String> {
        hooked(
            "decision point",
            super::decide::install(&self.catalog, &self.lens, point, entry, address),
        )
    }

    fn enable_watch(&self, point: usize, entry: &str, address: u64) -> Result<(), String> {
        hooked(
            "watched event",
            super::watch::install(&self.catalog, &self.lens, point, entry, address),
        )
    }

    fn watch_writes(&self, address: u64, size: u8) -> Result<(), String> {
        super::writes::start(address, size)
    }

    fn take_writes(&self) -> (Vec<crate::game::Written>, u64) {
        let (mut written, missed) = super::writes::finish();
        let size = image_size(self.base);
        for place in &mut written {
            place.offset = usize::try_from(place.after)
                .ok()
                .and_then(|after| after.checked_sub(self.base))
                .filter(|offset| size.is_some_and(|size| *offset < size))
                .map(|offset| offset as u64);
        }
        (written, missed)
    }

    fn preferred_image_base(&self) -> Option<u64> {
        preferred_image_base(self.base)
    }
}

/// `ImageBase` of the PE32+ module at `base`, from its headers in memory.
fn preferred_image_base(base: usize) -> Option<u64> {
    let read = |at: usize, size: usize| super::memory::read(at, size).ok();
    let offset = u32::from_le_bytes(read(base.checked_add(0x3C)?, 4)?.try_into().ok()?);
    // The optional header follows the 4-byte signature and the 20-byte file
    // header; a PE32+ header has the magic 0x20B and `ImageBase` at 24.
    let optional = base
        .checked_add(usize::try_from(offset).ok()?)?
        .checked_add(4 + 20)?;
    let magic = u16::from_le_bytes(read(optional, 2)?.try_into().ok()?);
    (magic == 0x20B)
        .then(|| {
            read(optional.checked_add(24)?, 8)?
                .try_into()
                .ok()
                .map(u64::from_le_bytes)
        })
        .flatten()
}

/// `SizeOfImage` of the module at `base`, from its PE headers in memory.
fn image_size(base: usize) -> Option<usize> {
    let read_u32 = |at: usize| {
        super::memory::read(at, 4)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .map(u32::from_le_bytes)
    };
    let headers = base.checked_add(usize::try_from(read_u32(base.checked_add(0x3C)?)?).ok()?)?;
    // The optional header follows the 4-byte signature and the 20-byte
    // file header; `SizeOfImage` is at offset 56 in it.
    usize::try_from(read_u32(headers.checked_add(4 + 20 + 56)?)?).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_image_base_comes_from_the_module_headers() {
        // SAFETY: a null name asks for the module of this process.
        let base = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null())
        } as usize;
        let preferred = super::preferred_image_base(base).unwrap();
        assert_ne!(preferred, 0);
        assert_eq!(preferred % 0x1_0000, 0, "an image base is 64 KiB aligned");
        assert!(super::image_size(base).is_some_and(|size| size > 0));
    }
}
