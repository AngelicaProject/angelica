//! Sounds from WAV files in the plugin's storage.
//!
//! ```ignore
//! use angelica_sdk::sound;
//!
//! // Needs `sound.play`; `ready.wav` ships in the plugin's storage.
//! sound::play("ready.wav", 0.8)?;
//! ```
use crate::api::sound as api;

/// Plays the WAV file at `path` in the plugin's storage at `volume`, from
/// 0 to 1.
///
/// # Errors
/// See `sound.play`: a file the host does not play, or a second sound
/// within a second.
pub fn play(path: &str, volume: f32) -> Result<(), crate::Error> {
    api::play(path, volume)
}
