//! Plays plugins' checked sounds through Windows.
use std::sync::{Mutex, PoisonError};

use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

/// The sound playing now. Windows reads it while it plays, so it lives
/// until the next sound has replaced it.
static PLAYING: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Plays `wav`, a sound [`crate::sound::prepare`] made, stopping the one
/// before.
pub fn play(wav: Vec<u8>) -> bool {
    let mut playing = PLAYING.lock().unwrap_or_else(PoisonError::into_inner);
    // SAFETY: a null sound stops the sound playing, so Windows no longer
    // reads the buffer about to be replaced.
    unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
    *playing = wav;
    // SAFETY: with SND_MEMORY the first argument points to a whole WAV
    // file in memory, which stays in PLAYING until the next call stops it.
    unsafe {
        PlaySoundW(
            playing.as_ptr().cast(),
            std::ptr::null_mut(),
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        ) != 0
    }
}
