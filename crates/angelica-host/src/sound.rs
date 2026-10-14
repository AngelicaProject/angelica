//! Sounds plugins play: WAV files from their storage, checked and
//! re-encoded by the host before they reach Windows.
#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::error::{Error, Kind, Result};

/// The largest sound file.
pub const MOST_BYTES: u64 = 8 * 1024 * 1024;
/// The longest sound.
pub const MOST_TIME: Duration = Duration::from_secs(10);
/// The least time between two sounds of one plugin.
pub const SPACING: Duration = Duration::from_secs(1);

/// The sample formats a sound may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Samples {
    /// Unsigned 8-bit.
    U8,
    I16,
    I24,
    F32,
}

impl Samples {
    fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16 => 2,
            Self::I24 => 3,
            Self::F32 => 4,
        }
    }

    /// One sample as a value from -1 to 1.
    fn value(self, bytes: &[u8]) -> f32 {
        match self {
            Self::U8 => (f32::from(bytes[0]) - 128.0) / 128.0,
            Self::I16 => f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32768.0,
            Self::I24 => {
                let value = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8;
                #[allow(clippy::cast_precision_loss)] // 24 bits fit an f32's mantissa
                let value = value as f32 / 8_388_608.0;
                value
            }
            Self::F32 => {
                let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                if value.is_finite() {
                    value.clamp(-1.0, 1.0)
                } else {
                    0.0
                }
            }
        }
    }
}

fn invalid(problem: impl Into<String>) -> Error {
    Error::new(Kind::InvalidArgument, problem)
}

/// A checked sound: `wav` re-encoded as 16-bit PCM at `volume`, ready for
/// Windows.
///
/// # Errors
/// A file that is not a WAV the host plays: PCM of 8, 16, or 24 bits or
/// 32-bit float, 1 or 2 channels, 8 to 96 kHz, at most [`MOST_TIME`] long;
/// a volume outside 0 to 1.
pub fn prepare(wav: &[u8], volume: f32) -> Result<Vec<u8>> {
    if !(0.0..=1.0).contains(&volume) {
        return Err(invalid("the volume is from 0 to 1"));
    }
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(invalid("the sound is not a WAV file"));
    }
    let mut format = None;
    let mut data = None;
    let mut at = 12;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let size = u32::from_le_bytes([wav[at + 4], wav[at + 5], wav[at + 6], wav[at + 7]]);
        let size = usize::try_from(size).map_err(|_| invalid("a chunk is too large"))?;
        let start = at + 8;
        let end = start
            .checked_add(size)
            .filter(|end| *end <= wav.len())
            .ok_or_else(|| invalid("a chunk of the WAV file ends past the file"))?;
        match id {
            b"fmt " if format.is_none() => format = Some(&wav[start..end]),
            b"data" if data.is_none() => data = Some(&wav[start..end]),
            _ => {}
        }
        // Chunks are padded to an even size.
        at = end + (size & 1);
    }
    let format = format.ok_or_else(|| invalid("the WAV file has no format"))?;
    let data = data.ok_or_else(|| invalid("the WAV file has no samples"))?;
    if format.len() < 16 {
        return Err(invalid("the WAV file's format is cut short"));
    }
    let word = |at: usize| u16::from_le_bytes([format[at], format[at + 1]]);
    let mut tag = word(0);
    let channels = word(2);
    let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
    let bits = word(14);
    // WAVE_FORMAT_EXTENSIBLE names the format in its subformat's first two
    // bytes.
    if tag == 0xFFFE {
        if format.len() < 26 {
            return Err(invalid("the WAV file's format is cut short"));
        }
        tag = word(24);
    }
    let samples = match (tag, bits) {
        (1, 8) => Samples::U8,
        (1, 16) => Samples::I16,
        (1, 24) => Samples::I24,
        (3, 32) => Samples::F32,
        _ => {
            return Err(invalid(
                "the sound is PCM of 8, 16, or 24 bits or 32-bit float",
            ));
        }
    };
    if !(1..=2).contains(&channels) {
        return Err(invalid("the sound has 1 or 2 channels"));
    }
    if !(8_000..=96_000).contains(&rate) {
        return Err(invalid("the sound's rate is 8 to 96 kHz"));
    }
    let frame = samples.bytes() * usize::from(channels);
    let frames = data.len() / frame;
    let most = usize::try_from(u64::from(rate) * MOST_TIME.as_secs()).unwrap_or(usize::MAX);
    if frames > most {
        return Err(invalid(format!(
            "a sound is at most {} seconds long",
            MOST_TIME.as_secs()
        )));
    }
    let pcm = &data[..frames * frame];
    let mut out = Vec::with_capacity(44 + pcm.len() / samples.bytes() * 2);
    let length = u32::try_from(pcm.len() / samples.bytes() * 2)
        .map_err(|_| invalid("the sound is too large"))?;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + length).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&length.to_le_bytes());
    for sample in pcm.chunks_exact(samples.bytes()) {
        #[allow(clippy::cast_possible_truncation)] // within i16 after the clamp
        let value = (samples.value(sample) * volume * 32767.0)
            .round()
            .clamp(-32768.0, 32767.0) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    Ok(out)
}

/// When each plugin last played a sound.
#[derive(Default)]
pub struct Sounds {
    last: Mutex<BTreeMap<String, Instant>>,
}

impl Sounds {
    /// Lets `plugin` play a sound at `now`.
    ///
    /// # Errors
    /// A second sound of the plugin within [`SPACING`].
    pub fn allow(&self, plugin: &str, now: Instant) -> Result<()> {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if last
            .get(plugin)
            .is_some_and(|last| now.duration_since(*last) < SPACING)
        {
            return Err(Error::new(
                Kind::LimitExceeded,
                "a plugin plays at most one sound a second",
            ));
        }
        last.insert(plugin.to_owned(), now);
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A WAV file of `samples` with the format's tag and bits.
    pub(crate) fn wav(tag: u16, channels: u16, rate: u32, bits: u16, samples: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + u32::try_from(samples.len()).unwrap()).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels * bits / 8)).to_le_bytes());
        out.extend_from_slice(&(channels * bits / 8).to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&u32::try_from(samples.len()).unwrap().to_le_bytes());
        out.extend_from_slice(samples);
        out
    }

    fn samples_of(prepared: &[u8]) -> Vec<i16> {
        prepared[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect()
    }

    #[test]
    fn sounds_are_re_encoded_at_their_volume() {
        let half = prepare(&wav(1, 1, 44_100, 16, &[0x00, 0x40, 0x00, 0xC0]), 0.5).unwrap();
        assert_eq!(&half[..4], b"RIFF");
        assert_eq!(samples_of(&half), [8192, -8192]);
        let bytes = prepare(&wav(1, 2, 8_000, 8, &[255, 0]), 1.0).unwrap();
        assert_eq!(samples_of(&bytes), [32511, -32767]);
        let float = prepare(
            &wav(
                3,
                1,
                48_000,
                32,
                &[0.5f32.to_le_bytes(), f32::NAN.to_le_bytes()].concat(),
            ),
            1.0,
        )
        .unwrap();
        assert_eq!(samples_of(&float), [16384, 0]);
        let deep = prepare(&wav(1, 1, 96_000, 24, &[0, 0, 0x80]), 1.0).unwrap();
        assert_eq!(samples_of(&deep), [-32767]);
    }

    #[test]
    fn only_checked_sounds_are_played() {
        let refused = |wav: &[u8], volume: f32, problem: &str| {
            let error = prepare(wav, volume).unwrap_err();
            assert_eq!(error.kind, Kind::InvalidArgument);
            assert!(error.message.contains(problem), "{}", error.message);
        };
        refused(b"ID3\x04", 1.0, "not a WAV");
        refused(&wav(1, 1, 44_100, 16, &[0, 0]), 1.5, "volume");
        refused(&wav(2, 1, 44_100, 4, &[0, 0]), 1.0, "PCM");
        refused(&wav(1, 3, 44_100, 16, &[0; 6]), 1.0, "1 or 2 channels");
        refused(&wav(1, 1, 200_000, 16, &[0, 0]), 1.0, "rate");
        refused(
            &wav(1, 1, 8_000, 8, &vec![128; 8_000 * 11]),
            1.0,
            "at most 10 seconds",
        );
        let mut cut = wav(1, 1, 44_100, 16, &[0; 8]);
        cut.truncate(cut.len() - 4);
        refused(&cut, 1.0, "ends past the file");
        let mut no_data = wav(1, 1, 44_100, 16, &[]);
        no_data.truncate(36);
        refused(&no_data, 1.0, "no samples");
    }

    #[test]
    fn a_plugin_plays_at_most_one_sound_a_second() {
        let sounds = Sounds::default();
        let now = Instant::now();
        sounds.allow("a", now).unwrap();
        assert_eq!(
            sounds
                .allow("a", now + Duration::from_millis(500))
                .unwrap_err()
                .kind,
            Kind::LimitExceeded
        );
        sounds.allow("b", now).unwrap();
        sounds.allow("a", now + SPACING).unwrap();
    }
}
