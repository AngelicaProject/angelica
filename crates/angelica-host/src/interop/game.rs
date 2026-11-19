//! The game process as the host's services see it.
use crate::{catalog::Catalog, excel_text::ExcelText, game::Game, text_case::TextCase};
use std::{path::PathBuf, sync::Arc};

pub struct WindowsGame {
    pub catalog: Arc<Catalog>,
    pub base: usize,
    pub excel: Arc<ExcelText>,
    pub data: Arc<crate::game_data::GameData>,
    pub sheets: Arc<crate::sheets::Sheets>,
    pub case: Arc<TextCase>,
    pub files: Arc<crate::file_replacements::FileReplacements>,
    pub commands: Arc<crate::commands::Commands>,
    pub menus: Arc<crate::menus::Menus>,
    pub lens: Arc<crate::lens::Lens>,
    pub version: String,
    pub combat_flag: Option<usize>,
}

impl WindowsGame {
    /// The contents of `ffxivgame.ver` next to the executable.
    pub fn read_version(executable: &std::path::Path) -> String {
        let path: PathBuf = executable.with_file_name("ffxivgame.ver");
        std::fs::read_to_string(path)
            .map(|text| text.trim().to_owned())
            .unwrap_or_default()
    }

    /// Locates the flag tested by `cmp byte ptr [rip + disp32], 0`.
    pub(super) fn combat_flag_address(
        catalog: &Catalog,
        base: usize,
        executable: &[u8],
    ) -> Option<usize> {
        let test = catalog.present.get("character.conditions.combat_test")?;
        let angelica_map::Value::Address(rva) = test.value else {
            return None;
        };
        base.checked_add(combat_flag_rva(executable, rva as usize)?)
    }
}

fn combat_flag_rva(executable: &[u8], test: usize) -> Option<usize> {
    let [0x80, 0x3d, a, b, c, d, 0, ..] = executable.get(test..)? else {
        return None;
    };
    let displacement = i32::from_le_bytes([*a, *b, *c, *d]);
    // RIP is after both the displacement and the comparison's immediate byte.
    let flag = test
        .checked_add(7)?
        .checked_add_signed(displacement as isize)?;
    (flag < executable.len()).then_some(flag)
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
        )?;
        super::outgoing_chat::install(self);
        super::action_names::install(self);
        super::quest_say::install(self);
        Ok(())
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

    fn enable_menus(&self) -> Result<(), String> {
        hooked(
            "item menus",
            super::menus::install(&self.catalog, self.base, self.menus.clone()),
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
        let present = &self.catalog.present;
        let rva = if let Some(angelica_map::Value::Address(rva)) = present
            .get("camera.projection.matrix")
            .map(|entry| entry.value)
        {
            rva as usize
        } else {
            let control = present.get("camera.control.instance")?;
            let matrix = present.get("camera.control.view_projection")?;
            let (angelica_map::Value::Address(rva), angelica_map::Value::Offset(offset)) =
                (control.value, matrix.value)
            else {
                return None;
            };
            (rva as usize).checked_add(offset as usize)?
        };
        let address = self.base.checked_add(rva)?;
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

    fn in_combat(&self) -> Option<bool> {
        let bytes = super::memory::read(self.combat_flag?, 1).ok()?;
        Some(bytes[0] != 0)
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
        preferred_image_base()
    }
}

/// `ImageBase` of a PE32+ file from its first bytes. It is read from the
/// file on disk: the loader writes the base it chose into the headers in
/// memory.
fn file_image_base(header: &[u8]) -> Option<u64> {
    let bytes = |at: usize, size: usize| header.get(at..at.checked_add(size)?);
    let offset = usize::try_from(u32::from_le_bytes(bytes(0x3C, 4)?.try_into().ok()?)).ok()?;
    // The optional header follows the 4-byte signature and the 20-byte file
    // header; a PE32+ header has the magic 0x20B and `ImageBase` at 24.
    let optional = offset.checked_add(4 + 20)?;
    let magic = u16::from_le_bytes(bytes(optional, 2)?.try_into().ok()?);
    (magic == 0x20B)
        .then(|| {
            bytes(optional + 24, 8)?
                .try_into()
                .ok()
                .map(u64::from_le_bytes)
        })
        .flatten()
}

/// `ImageBase` of the game's executable file.
fn preferred_image_base() -> Option<u64> {
    use std::io::Read as _;
    let mut header = vec![0u8; 4096];
    let read = std::fs::File::open(std::env::current_exe().ok()?)
        .and_then(|mut file| file.read(&mut header))
        .ok()?;
    header.truncate(read);
    file_image_base(&header)
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
    fn combat_flag_displacement_is_relative_to_the_end_of_the_comparison() {
        let code = [0x90, 0x80, 0x3d, 4, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        assert_eq!(super::combat_flag_rva(&code, 1), Some(12));
        assert_eq!(super::combat_flag_rva(&code, 0), None);
        assert_eq!(super::combat_flag_rva(&code[..7], 1), None);
        assert_eq!(super::combat_flag_rva(&code[..12], 1), None);
    }

    #[test]
    fn the_image_base_comes_from_the_file_headers() {
        let file = angelica_pe::testing::PeBuilder::new()
            .section(".text", angelica_pe::testing::CODE, &[0xC3])
            .build_file();
        let image = angelica_pe::Image::parse(&file, angelica_pe::Layout::File).unwrap();
        assert_eq!(super::file_image_base(&file), Some(image.image_base()));
        assert_eq!(super::file_image_base(&file[..0x40]), None, "cut short");
        assert!(
            super::preferred_image_base().is_some(),
            "this test's own executable"
        );
        // SAFETY: a null name asks for the module of this process.
        let base = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null())
        } as usize;
        assert!(super::image_size(base).is_some_and(|size| size > 0));
    }
}
