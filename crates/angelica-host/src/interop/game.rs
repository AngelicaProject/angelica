//! The game process as the host's services see it.
use crate::{catalog::Catalog, excel_text::ExcelText, game::Game, text_case::TextCase};
use std::{path::PathBuf, sync::Arc};

pub struct WindowsGame {
    pub catalog: Arc<Catalog>,
    pub base: usize,
    pub excel: Arc<ExcelText>,
    pub case: Arc<TextCase>,
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
}
