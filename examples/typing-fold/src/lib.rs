//! Keybound Brawler compares the typed text with an enemy's word after both
//! pass one normalizing function of the game. Its comparison folds Latin
//! letter case but compares Cyrillic byte by byte, so "титан" misses
//! "Титан". This plugin intercepts that function, which its own Anatomia
//! entries describe, and folds Cyrillic in the normalized text: every
//! letter becomes lowercase and "ё" becomes "е". Both are two bytes in
//! UTF-8, so the text keeps its length and changes in place.

use angelica_sdk::intercepts::{self, Call, Value};

const NORMALIZE: &str = "minigame.keybound.normalize";

struct TypingFold;

impl angelica_sdk::Plugin for TypingFold {
    fn start() -> Result<Self, String> {
        intercepts::after(NORMALIZE, 0)?;
        Ok(Self)
    }

    fn intercepted(call: Call) {
        // normalize(this, text): the text is the second argument.
        let Some(Value::Object(text)) = call.args.get(1) else {
            return;
        };
        let Ok(mut bytes) = intercepts::text(*text) else {
            return;
        };
        if fold(&mut bytes)
            && let Err(error) = intercepts::edit_text(*text, &bytes)
        {
            angelica_sdk::warning(&format!("the typed text is not folded: {}", error.message));
        }
    }
}

angelica_sdk::plugin!(TypingFold);

/// The game's text payloads, `0x02 … 0x03`, are left whole.
const PAYLOAD_START: u8 = 0x02;
const PAYLOAD_END: u8 = 0x03;

/// Folds Cyrillic letters of `text` as the comparison should: lowercase,
/// and "ё" as "е". Whether anything changed.
pub fn fold(text: &mut [u8]) -> bool {
    let mut changed = false;
    let mut at = 0;
    while at < text.len() {
        let lead = text[at];
        if lead == PAYLOAD_START {
            match payload_end(text, at) {
                Some(end) => {
                    at = end;
                    continue;
                }
                None => return changed,
            }
        }
        if at + 1 < text.len() {
            let pair = [text[at], text[at + 1]];
            let folded = fold_letter(pair);
            if folded != pair {
                text[at] = folded[0];
                text[at + 1] = folded[1];
                changed = true;
            }
        }
        at += utf8_length(lead);
    }
    changed
}

/// One two-byte letter of U+0400 to U+045F, lowercase, with "ё" as "е".
fn fold_letter([lead, tail]: [u8; 2]) -> [u8; 2] {
    let lower = match (lead, tail) {
        // Ѐ to Џ: ѐ to џ.
        (0xD0, 0x80..=0x8F) => [0xD1, tail + 0x10],
        // А to П: а to п.
        (0xD0, 0x90..=0x9F) => [0xD0, tail + 0x20],
        // Р to Я: р to я.
        (0xD0, 0xA0..=0xAF) => [0xD1, tail - 0x20],
        _ => [lead, tail],
    };
    if lower == [0xD1, 0x91] {
        // ё: е.
        [0xD0, 0xB5]
    } else {
        lower
    }
}

fn utf8_length(lead: u8) -> usize {
    match lead {
        0x00..=0xBF => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Where the payload that starts at `at` ends: `0x02`, a code byte, the
/// body's length as the game encodes integers, the body, and `0x03`.
fn payload_end(text: &[u8], at: usize) -> Option<usize> {
    let mut cursor = at + 2;
    let marker = *text.get(cursor)?;
    cursor += 1;
    let length = match marker {
        0x01..=0xCF => usize::from(marker - 1),
        0xF0..=0xFE => {
            let flags = (marker & 0x0F) + 1;
            let mut length = 0usize;
            for bit in (0..4).rev() {
                length <<= 8;
                if flags & (1 << bit) != 0 {
                    length |= usize::from(*text.get(cursor)?);
                    cursor += 1;
                }
            }
            length
        }
        _ => return None,
    };
    let end = cursor.checked_add(length)?;
    (text.get(end) == Some(&PAYLOAD_END)).then_some(end + 1)
}

#[cfg(test)]
mod tests {
    use super::fold;

    fn folded(text: &str) -> String {
        let mut bytes = text.as_bytes().to_vec();
        fold(&mut bytes);
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn cyrillic_letters_fold_to_lowercase_and_yo_to_ye() {
        assert_eq!(folded("Титан"), "титан");
        assert_eq!(folded("ЁЛКА ёлка"), "елка елка");
        assert_eq!(folded("ЀЏ АП РЯ"), "ѐџ ап ря");
        assert_eq!(folded("Iron Колосс"), "Iron колосс", "Latin is the game's");
    }

    #[test]
    fn the_length_never_changes_and_payloads_stay_whole() {
        let mut text = b"\x02\x10\x03\xD0\x90\x03\xD0\x90".to_vec();
        let before = text.len();
        assert!(fold(&mut text));
        assert_eq!(text.len(), before);
        assert_eq!(
            text, b"\x02\x10\x03\xD0\x90\x03\xD0\xB0",
            "the payload's А stays"
        );
        assert!(!fold(&mut b"already".to_vec()));
    }
}
