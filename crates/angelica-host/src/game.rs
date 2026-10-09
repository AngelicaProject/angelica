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

    /// The rows of a state table with the values of `fields` (named
    /// `names`), read from the game's memory. On the main thread only.
    fn read_state(
        &self,
        table: &crate::state::TablePlan,
        names: &[String],
        fields: &[crate::state::FieldPlan],
    ) -> Vec<Vec<Option<crate::state::StateValue>>>;
}
