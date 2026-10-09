//! Plugin messages in the chat log (the `chat` interface): checked, limited
//! per plugin, and encoded as the game's text with the plugin's name in
//! front. The colors are rows of the game's `UIColor` sheet, which hold a
//! color for each interface theme (`docs/formats/game-structures.md`).
#![forbid(unsafe_code)]
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// Spans of one message.
pub const MAX_SPANS: usize = 64;
/// Bytes of text of one message.
pub const MAX_TEXT: usize = 2000;
/// Messages a plugin prints in any [`WINDOW`].
pub const MAX_PER_WINDOW: usize = 20;
pub const WINDOW: Duration = Duration::from_secs(1);

/// The `UIColor` rows of the tones, and of the name in front of every
/// message, which no tone uses.
const NAME_COLOR: u32 = 500;
const ACCENT_COLOR: u32 = 502;
const GOOD_COLOR: u32 = 504;
const WARNING_COLOR: u32 = 506;
const BAD_COLOR: u32 = 508;
const MUTED_COLOR: u32 = 3;

const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const NEW_LINE: u8 = 0x10;
const ITALIC: u8 = 0x1A;
/// Text color by `UIColor` row; row 0 restores the previous color.
const FOREGROUND: u8 = 0x48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Accent,
    Muted,
    Good,
    Warning,
    Bad,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub tone: Tone,
    pub italic: bool,
}

impl Span {
    #[must_use]
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
            italic: false,
        }
    }
}

/// How often each plugin printed recently.
#[derive(Default)]
pub struct Chat {
    recent: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Chat {
    /// Counts a message of `plugin` at `now`; false when the plugin already
    /// printed [`MAX_PER_WINDOW`] messages in the [`WINDOW`] before.
    pub fn admit(&self, plugin: &str, now: Instant) -> bool {
        let mut recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        let times = recent.entry(plugin.to_owned()).or_default();
        while times
            .front()
            .is_some_and(|&time| now.duration_since(time) >= WINDOW)
        {
            times.pop_front();
        }
        if times.len() >= MAX_PER_WINDOW {
            return false;
        }
        times.push_back(now);
        true
    }

    pub fn remove_plugin(&self, plugin: &str) {
        self.recent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin);
    }
}

/// Checks a message's spans.
///
/// # Errors
/// Names the rule a message breaks: too many spans, too much text, or a
/// control character other than a line break.
pub fn check(spans: &[Span]) -> Result<(), String> {
    if spans.len() > MAX_SPANS {
        return Err(format!("a message has at most {MAX_SPANS} spans"));
    }
    let text: usize = spans.iter().map(|span| span.text.len()).sum();
    if text > MAX_TEXT {
        return Err(format!("a message has at most {MAX_TEXT} bytes of text"));
    }
    if spans.iter().all(|span| span.text.trim().is_empty()) {
        return Err("the message is empty".into());
    }
    if let Some(c) = spans
        .iter()
        .flat_map(|span| span.text.chars())
        .find(|&c| c.is_control() && c != '\n')
    {
        return Err(format!("a message cannot contain {c:?}"));
    }
    Ok(())
}

/// The game's text of a checked message: `[name]` in the name color, a
/// space, then each span.
#[must_use]
pub fn encode(name: &str, spans: &[Span]) -> Vec<u8> {
    let mut out = Vec::new();
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    color(&mut out, NAME_COLOR);
    out.extend_from_slice(format!("[{}]", name.trim()).as_bytes());
    color(&mut out, 0);
    out.push(b' ');
    for span in spans {
        let row = match span.tone {
            Tone::Normal => None,
            Tone::Accent => Some(ACCENT_COLOR),
            Tone::Muted => Some(MUTED_COLOR),
            Tone::Good => Some(GOOD_COLOR),
            Tone::Warning => Some(WARNING_COLOR),
            Tone::Bad => Some(BAD_COLOR),
        };
        if let Some(row) = row {
            color(&mut out, row);
        }
        if span.italic {
            macro_with(&mut out, ITALIC, 1);
        }
        for (index, line) in span.text.split('\n').enumerate() {
            if index > 0 {
                out.extend_from_slice(&[STX, NEW_LINE, 1, ETX]);
            }
            out.extend_from_slice(line.as_bytes());
        }
        if span.italic {
            macro_with(&mut out, ITALIC, 0);
        }
        if row.is_some() {
            color(&mut out, 0);
        }
    }
    out
}

fn color(out: &mut Vec<u8>, row: u32) {
    macro_with(out, FOREGROUND, row);
}

/// A macro with one integer argument.
fn macro_with(out: &mut Vec<u8>, code: u8, value: u32) {
    let argument = integer(value);
    out.extend_from_slice(&[STX, code, u8::try_from(argument.len() + 1).unwrap_or(0)]);
    out.extend_from_slice(&argument);
    out.push(ETX);
}

/// An integer in the game's canonical form: one byte (value + 1) below
/// 0xCF, otherwise a marker whose low bits, plus one, flag which of four
/// big-endian bytes follow; zero bytes are left out.
fn integer(value: u32) -> Vec<u8> {
    if value < 0xCF {
        return vec![u8::try_from(value + 1).unwrap_or(0)];
    }
    let bytes = value.to_be_bytes();
    let mut flags = 0u8;
    let mut present = Vec::new();
    for (index, &byte) in bytes.iter().enumerate() {
        if byte != 0 {
            flags |= 8 >> index;
            present.push(byte);
        }
    }
    let mut out = vec![0xF0 + flags - 1];
    out.extend(present);
    out
}

/// The text of an encoded message without its macros, line breaks as
/// `\n`; for tests and logs.
#[must_use]
pub fn plain(text: &[u8]) -> String {
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        if text[at] == STX && at + 2 < text.len() {
            let length = usize::from(text[at + 2]);
            if text[at + 1] == NEW_LINE {
                out.push(b'\n');
            }
            at += 3 + length;
        } else {
            out.push(text[at]);
            at += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_start_with_the_name_and_color_their_spans() {
        let spans = [
            Span::new("Installed ", Tone::Normal),
            Span::new("pack", Tone::Accent),
            Span {
                text: "\nlater".into(),
                tone: Tone::Muted,
                italic: true,
            },
        ];
        check(&spans).unwrap();
        let text = encode("Hello", &spans);
        assert!(angelica_text::sestring::is_well_formed(&text));
        assert_eq!(plain(&text), "[Hello] Installed pack\nlater");
        // UIColor 502 is 0x01F6: marker 0xF2, then its two bytes.
        let accent = [STX, FOREGROUND, 4, 0xF2, 0x01, 0xF6, ETX];
        assert!(text.windows(accent.len()).any(|w| w == accent));
        let reset = [STX, FOREGROUND, 2, 1, ETX];
        assert!(text.windows(reset.len()).any(|w| w == reset));
        assert!(text.windows(5).any(|w| w == [STX, ITALIC, 2, 2, ETX]));
    }

    #[test]
    fn messages_are_checked() {
        assert!(check(&[Span::new("\u{2}x", Tone::Normal)]).is_err());
        assert!(check(&[Span::new(" ", Tone::Normal)]).is_err());
        assert!(check(&[Span::new("x".repeat(MAX_TEXT + 1), Tone::Normal)]).is_err());
        assert!(check(&vec![Span::new("x", Tone::Normal); MAX_SPANS + 1]).is_err());
        assert!(check(&[Span::new("a\nb", Tone::Bad)]).is_ok());
    }

    #[test]
    fn plugins_print_at_most_twenty_messages_a_second() {
        let chat = Chat::default();
        let now = Instant::now();
        for _ in 0..MAX_PER_WINDOW {
            assert!(chat.admit("a", now));
        }
        assert!(!chat.admit("a", now));
        assert!(chat.admit("b", now), "per plugin");
        assert!(chat.admit("a", now + WINDOW));
    }

    #[test]
    fn integers_take_the_canonical_form() {
        assert_eq!(integer(0), [1]);
        assert_eq!(integer(0xCE), [0xCF]);
        assert_eq!(integer(0xCF), [0xF0, 0xCF]);
        assert_eq!(integer(500), [0xF2, 0x01, 0xF4]);
        assert_eq!(integer(0x0100), [0xF1, 0x01]);
    }

    #[test]
    fn damaged_messages_are_refused_without_panicking() {
        let mut mutator = crate::mutations::Mutator::new(17);
        for _ in 0..20_000 {
            let spans: Vec<Span> = (0..=mutator.below(3))
                .map(|_| Span::new(mutator.mutate_text("Hello, [world]!\nAgain"), Tone::Accent))
                .collect();
            if check(&spans).is_ok() {
                let encoded = encode(&mutator.mutate_text("Hello"), &spans);
                let _ = plain(&encoded);
            }
            let _ = plain(&mutator.mutate(b"\x02\x48\x04\xF2\x02\x25\x03text\x02\x48\x02\x01\x03"));
        }
    }
}
