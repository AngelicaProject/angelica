//! What the host needs from the game process. The Windows adapter installs
//! hooks through verified map entries; the scripted game in
//! `angelica-fake-game` drives the same services from test data.
#![forbid(unsafe_code)]

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

    /// Prints the game's text `message` in the chat log's Echo channel. On
    /// the main thread only.
    ///
    /// # Errors
    /// Names the map entries the game build lacks, or says the chat log
    /// does not exist yet.
    fn print(&self, message: &[u8]) -> Result<(), String>;

    /// Makes the game's chat log report the messages added to it to
    /// [`crate::events::Events`]. Called on every subscription; installs its
    /// hook once.
    ///
    /// # Errors
    /// Names the map entries the game build lacks.
    fn enable_chat_messages(&self) -> Result<(), String>;

    /// Runs `work` with the game's own windows, when the game has them
    /// yet. On the main thread only.
    ///
    /// # Errors
    /// Names the map entries the game build lacks.
    fn with_screen(
        &self,
        work: &mut dyn FnMut(&mut dyn crate::game_ui::Screen),
    ) -> Result<(), String>;

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

    /// Calls the game's function at `address` with `args`, each as its 64
    /// bits, on the game's main thread, for a Lens action.
    ///
    /// # Errors
    /// Why the call cannot be made.
    fn call(&self, address: u64, args: &[u64]) -> Result<crate::lens::act::Returned, String> {
        let _ = (address, args);
        Err("this game cannot be acted in".into())
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

    /// Writes `bytes` at `address` of the game's data, for a Lens change;
    /// false when the range is not writable data.
    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        let _ = (address, bytes);
        false
    }
}
