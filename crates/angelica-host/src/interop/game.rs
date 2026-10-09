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
    pub events: Arc<crate::events::Events>,
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

/// The game's memory, read through Windows so that an unreadable address
/// fails instead of faulting.
struct ProcessMemory;

impl crate::state::Memory for ProcessMemory {
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
        super::excel_rows::install(&self.catalog, self.base, self.excel.clone())
    }

    fn enable_text_case(&self) -> Result<(), String> {
        super::text_case::install(&self.catalog, self.base, self.case.clone())
    }

    fn enable_file_replacements(&self) -> Result<(), String> {
        super::file_reads::install(&self.catalog, self.base, self.files.clone())
    }

    fn enable_commands(&self) -> Result<(), String> {
        super::chat::install_commands(&self.catalog, self.base, self.commands.clone())
    }

    fn print(&self, message: &[u8]) -> Result<(), String> {
        super::chat::print(&self.catalog, self.base, message)
    }

    fn enable_chat_messages(&self) -> Result<(), String> {
        super::chat::install_messages(&self.catalog, self.base, self.events.clone())
    }

    fn read_state(
        &self,
        table: &crate::state::TablePlan,
        _names: &[String],
        fields: &[crate::state::FieldPlan],
    ) -> Vec<Vec<Option<crate::state::StateValue>>> {
        crate::state::read(&ProcessMemory, table, fields)
    }
}
