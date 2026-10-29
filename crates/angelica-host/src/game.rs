//! What the host needs from the game process. The Windows adapter installs
//! hooks through verified map entries; the scripted game in
//! `angelica-fake-game` drives the same services from test data.
#![forbid(unsafe_code)]
use std::sync::Arc;

/// Where writes to a watched address came from: the address where the
/// writing instruction ended, its offset in the game's executable when it
/// is there, and how many writes ended there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub after: u64,
    pub offset: Option<u64>,
    pub count: u64,
}

pub trait Game: Send + Sync {
    /// The contents of `game/ffxivgame.ver`.
    fn version(&self) -> String;

    /// Makes rows the game builds from now on go through the active text
    /// tables. Called on every activation; installs its hooks once.
    ///
    /// # Errors
    /// Names the map entries the game build lacks.
    fn enable_excel_text(&self) -> Result<(), String>;

    /// Makes the game's case conversion apply the plugins' letters.
    ///
    /// # Errors
    /// Names the map entries the game build lacks.
    fn enable_text_case(&self) -> Result<(), String>;

    /// Makes the game read the plugins' replacement files when it loads
    /// them.
    ///
    /// # Errors
    /// Names the map entries the game build lacks.
    fn enable_file_replacements(&self) -> Result<(), String>;

    /// Makes the game hand the plugins' command lines to
    /// [`crate::commands::Commands::submit`] before it runs a line.
    ///
    /// # Errors
    /// Names the map entries the game build lacks.
    fn enable_commands(&self) -> Result<(), String>;

    /// Makes the game's item menus show the entries
    /// [`crate::menus::Menus::items`] gives, and note what the player
    /// chooses with [`crate::menus::Menus::choose`]. Installs its hooks
    /// once.
    ///
    /// # Errors
    /// Names the map entries the game build lacks.
    fn enable_menus(&self) -> Result<(), String>;

    /// Prints the game's text `message` in the chat log's Echo channel. On
    /// the main thread only.
    ///
    /// # Errors
    /// Names the map entries the game build lacks, or says the chat log
    /// does not exist yet.
    fn print(&self, message: &[u8]) -> Result<(), String>;

    /// The rows of a prepared Lens query, read from the game's memory. On
    /// the main thread only.
    ///
    /// # Errors
    /// The limits of [`crate::lens::run`].
    fn query(
        &self,
        prepared: &crate::lens::Prepared,
    ) -> crate::error::Result<Vec<crate::lens::Row>>;

    /// The game's memory, read so that an unreadable address fails instead
    /// of faulting; for `angelica look`.
    fn memory(&self) -> &dyn crate::lens::Memory;

    /// Makes the game call [`crate::lens::Watches::fire`] with `point` at
    /// the code of the map entry `entry`, at `address`. Called once per
    /// watched event; the hook stays until the process exits.
    ///
    /// # Errors
    /// Why the point cannot be hooked in this build.
    fn enable_watch(&self, point: usize, entry: &str, address: u64) -> Result<(), String>;

    /// Starts counting the instructions that write `size` bytes at
    /// `address`, for `angelica look` in dev mode; one watch at a time.
    ///
    /// # Errors
    /// Why writes cannot be watched there.
    fn watch_writes(&self, address: u64, size: u8) -> Result<(), String> {
        let _ = (address, size);
        Err("this game cannot watch writes".into())
    }

    /// Ends the watch of writes: where they came from, and how many came
    /// from places past the ones it tells apart.
    fn take_writes(&self) -> (Vec<Written>, u64) {
        (Vec::new(), 0)
    }

    /// The image base the game's executable asks for in its headers, where
    /// a disassembler places it; none when it cannot be read.
    fn preferred_image_base(&self) -> Option<u64> {
        None
    }

    /// Calls the game's function at `address` with `args`, each as its 64
    /// bits, on the game's main thread, for a Lens action.
    ///
    /// # Errors
    /// Why the call cannot be made.
    fn call(&self, address: u64, args: &[u64]) -> Result<crate::lens::act::Returned, String> {
        let _ = (address, args);
        Err("this game cannot be acted in".into())
    }

    /// Calls the game's function at `function` that sets a value of the
    /// object at `object` to `value`, on the main thread, for a Lens
    /// change; text is handed over NUL-terminated. Whether it was called.
    fn call_setter(&self, function: u64, object: u64, value: &crate::lens::set::Setting) -> bool {
        use crate::lens::set::Setting;
        match value {
            Setting::Number(bits) => self.call(function, &[object, *bits]).is_ok(),
            Setting::Text(text) => {
                let mut terminated = text.clone();
                terminated.push(0);
                // The buffer outlives the call, which copies the text.
                let pointer = terminated.as_ptr() as u64;
                self.call(function, &[object, pointer]).is_ok()
            }
        }
    }

    /// Makes the game ask [`crate::lens::decide::Decisions::decide`] with
    /// `point` at the start of the function of the map entry `entry`, at
    /// `address`, and return at once when a rule holds. Called once per
    /// decision point; the hook stays until the process exits.
    ///
    /// # Errors
    /// Why the function cannot be hooked in this build.
    fn enable_decision(&self, point: usize, entry: &str, address: u64) -> Result<(), String> {
        let _ = (point, address);
        Err(format!(
            "map entry {entry} cannot be decided at in this game"
        ))
    }

    /// The game camera's view-projection matrix this frame, of row vectors
    /// as the game keeps it; `None` while the game has none or the build
    /// lacks its map entries.
    fn view_projection(&self) -> Option<[f32; 16]> {
        None
    }

    /// Plays `wav`, a sound [`crate::sound::prepare`] checked, stopping the
    /// one playing; whether Windows took it.
    fn play_sound(&self, wav: Vec<u8>) -> bool {
        let _ = wav;
        false
    }

    /// The game's executable as it is mapped: where it is loaded and its
    /// bytes, for resolving plugins' own Anatomia entries; none when it
    /// cannot be read.
    fn executable(&self) -> Option<(u64, Arc<Vec<u8>>)> {
        None
    }

    /// The code of the game's function at `address`, as a hook checks it;
    /// none where the executable does not tell it exactly.
    fn code_of(&self, address: u64) -> Option<crate::intercepts::Code> {
        code_in(&self.executable()?, address)
    }

    /// Makes the game call [`crate::intercepts::Intercepts::after_call`]
    /// when the function at `address`, with `code`, returns. Called once
    /// per function; the hook stays until the process exits.
    ///
    /// # Errors
    /// Why the function cannot be hooked.
    fn enable_intercept(
        &self,
        address: u64,
        code: &crate::intercepts::Code,
        intercepts: &Arc<crate::intercepts::Intercepts>,
    ) -> Result<(), String> {
        let _ = (address, code, intercepts);
        Err("this game cannot be intercepted".into())
    }

    /// Where the game calls the thunk in `slot`, which calls
    /// [`crate::intercepts::Intercepts::called`] with `slot`. A slot is
    /// given once and keeps its address until the process exits.
    ///
    /// # Errors
    /// Why the game cannot call plugins back.
    fn thunk(
        &self,
        slot: usize,
        intercepts: &Arc<crate::intercepts::Intercepts>,
    ) -> Result<u64, String> {
        let _ = (slot, intercepts);
        Err("this game cannot call plugins back".into())
    }

    /// Compiles the entry point `entry` of HLSL for shader model 5.0 into
    /// the GPU's code, a vertex shader or a pixel shader. The scripted
    /// game, which draws nothing, accepts any.
    ///
    /// # Errors
    /// The compiler's message.
    fn compile_shader(&self, hlsl: &str, entry: &str, vertex: bool) -> Result<Vec<u8>, String> {
        let _ = (hlsl, vertex);
        Ok(entry.as_bytes().to_vec())
    }

    /// Writes `bytes` at `address` of the game's data, for a Lens change;
    /// false when the range is not writable data.
    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        let _ = (address, bytes);
        false
    }
}

/// The code of the function at `address` in `executable`, loaded where it
/// says and mapped as its bytes, as a hook checks it.
#[must_use]
pub fn code_in(executable: &(u64, Arc<Vec<u8>>), address: u64) -> Option<crate::intercepts::Code> {
    let (base, bytes) = executable;
    let image = angelica_pe::Image::parse(bytes, angelica_pe::Layout::Mapped).ok()?;
    let rva = u32::try_from(address.checked_sub(*base)?).ok()?;
    let found = angelica_map::function_code(&image, rva)?;
    let at = |rva: u32| base + u64::from(rva);
    Some(crate::intercepts::Code {
        start: at(found.start),
        ranges: found
            .ranges
            .iter()
            .map(|range| at(range.start)..at(range.end))
            .collect(),
        prologue: u64::from(found.prologue.unwrap_or(0)),
    })
}
