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
}
