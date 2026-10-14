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

    fn with_screen(
        &self,
        work: &mut dyn FnMut(&mut dyn crate::game_ui::Screen),
    ) -> Result<(), String> {
        super::game_ui::with_screen(&self.catalog, self.base, work)
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
}
