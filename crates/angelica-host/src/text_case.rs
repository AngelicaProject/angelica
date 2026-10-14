//! Letters the game's case conversion gives as plugins' rules decide them,
//! at the map's derived decision points of the `case_letter` derivation,
//! and the conversion itself, applied after the game's own. Platform
//! independent: the Windows adapter passes the string the game just
//! converted.
#![forbid(unsafe_code)]
use std::{
    collections::HashMap,
    sync::{Arc, PoisonError, RwLock},
};

/// Letter rules one plugin can keep.
pub const MAX_LETTERS: usize = 4096;
/// The game's conversion to capital letters, whose event's rules are for
/// [`Direction::Upper`].
pub const UPPER_ENTRY: &str = "text.utf8_string.to_upper";
/// The game's conversion to small letters.
pub const LOWER_ENTRY: &str = "text.utf8_string.to_lower";

const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const SPACE: u8 = b' ';

/// Which conversion of the game a rule is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// To capital letters.
    Upper,
    /// To small letters.
    Lower,
}

/// The letters of every plugin's rules, as the conversion uses them.
#[derive(Debug, Default)]
pub struct Letters {
    /// UTF-8 of a letter to the one the conversion to capitals gives, and
    /// to small letters.
    upper: HashMap<Vec<u8>, Vec<u8>>,
    lower: HashMap<Vec<u8>, Vec<u8>>,
}

/// One plugin's rule: the letter a conversion gives for another.
struct Rule {
    plugin: String,
    handle: u32,
    direction: Direction,
    letter: char,
    becomes: char,
}

#[derive(Default)]
struct State {
    /// In the order the plugins added them.
    rules: Vec<Rule>,
    letters: Arc<Letters>,
}

#[derive(Default)]
pub struct TextCase {
    state: RwLock<State>,
}

impl TextCase {
    /// Adds the plugin's rule under `handle`: the conversion `direction`
    /// gives `becomes` for `letter`. Of rules for one letter and
    /// direction, the first added applies.
    ///
    /// # Errors
    /// Refuses ASCII letters, which the game converts itself, letters that
    /// are equal or differ in UTF-8 length, so text never changes length,
    /// and more than [`MAX_LETTERS`] rules of a plugin.
    pub fn add(
        &self,
        plugin: &str,
        handle: u32,
        direction: Direction,
        letter: char,
        becomes: char,
    ) -> Result<(), String> {
        if letter.is_ascii() || becomes.is_ascii() {
            return Err(format!(
                "{letter}/{becomes}: the game converts ASCII letters itself"
            ));
        }
        if letter == becomes || letter.len_utf8() != becomes.len_utf8() {
            return Err(format!(
                "{letter}/{becomes}: the letters must differ and have the same UTF-8 length"
            ));
        }
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        if state
            .rules
            .iter()
            .filter(|rule| rule.plugin == plugin)
            .count()
            >= MAX_LETTERS
        {
            return Err(format!("a plugin keeps at most {MAX_LETTERS} letter rules"));
        }
        state.rules.push(Rule {
            plugin: plugin.to_owned(),
            handle,
            direction,
            letter,
            becomes,
        });
        state.letters = Arc::new(combine(&state.rules));
        Ok(())
    }

    /// Removes the plugin's rule under `handle`; false when it has none.
    pub fn remove(&self, plugin: &str, handle: u32) -> bool {
        self.retain(|rule| rule.plugin != plugin || rule.handle != handle)
    }

    pub fn remove_plugin(&self, plugin: &str) {
        self.retain(|rule| rule.plugin != plugin);
    }

    fn retain(&self, keep: impl Fn(&Rule) -> bool) -> bool {
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let before = state.rules.len();
        state.rules.retain(keep);
        if state.rules.len() == before {
            return false;
        }
        state.letters = Arc::new(combine(&state.rules));
        true
    }

    #[must_use]
    pub fn letters(&self) -> Arc<Letters> {
        self.state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .letters
            .clone()
    }
}

/// The letters of the rules: for each letter and direction, the first
/// rule's.
fn combine(rules: &[Rule]) -> Letters {
    let mut letters = Letters::default();
    for rule in rules {
        let map = match rule.direction {
            Direction::Upper => &mut letters.upper,
            Direction::Lower => &mut letters.lower,
        };
        map.entry(rule.letter.to_string().into_bytes())
            .or_insert_with(|| rule.becomes.to_string().into_bytes());
    }
    letters
}

impl Letters {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.upper.is_empty() && self.lower.is_empty()
    }

    /// The game's upper-case rule: with `first_only` and not `every_word`
    /// the first character; with both the first character of every word (a
    /// word starts after a space) unless `excluded` says a listed word
    /// starts there (never at 0); without `first_only` every letter.
    /// Macros are skipped whole.
    pub fn to_upper(
        &self,
        text: &mut [u8],
        first_only: bool,
        every_word: bool,
        excluded: impl Fn(usize) -> bool,
    ) {
        if self.upper.is_empty() {
            return;
        }
        if first_only && !every_word {
            Self::replace(&self.upper, text, 0);
            return;
        }
        let mut word_start = true;
        let mut at = 0;
        while at < text.len() {
            match text[at] {
                0 => return,
                STX => {
                    let Some(end) = skip_macro(text, at) else {
                        return;
                    };
                    at = end;
                    word_start = false;
                }
                SPACE => {
                    word_start = true;
                    at += 1;
                }
                lead => {
                    if (!first_only || word_start) && (at == 0 || !excluded(at)) {
                        Self::replace(&self.upper, text, at);
                    }
                    word_start = false;
                    at += utf8_length(lead);
                }
            }
        }
    }

    /// The game's lower-case rule: every letter. Macros are skipped whole.
    pub fn to_lower(&self, text: &mut [u8]) {
        if self.lower.is_empty() {
            return;
        }
        let mut at = 0;
        while at < text.len() {
            match text[at] {
                0 => return,
                STX => {
                    let Some(end) = skip_macro(text, at) else {
                        return;
                    };
                    at = end;
                }
                lead => {
                    Self::replace(&self.lower, text, at);
                    at += utf8_length(lead);
                }
            }
        }
    }

    fn replace(map: &HashMap<Vec<u8>, Vec<u8>>, text: &mut [u8], at: usize) {
        let Some(&lead) = text.get(at) else { return };
        let length = utf8_length(lead);
        if length < 2 {
            return;
        }
        if let Some(letter) = text.get(at..at + length)
            && let Some(other) = map.get(letter)
        {
            text[at..at + length].copy_from_slice(other);
        }
    }
}

fn utf8_length(lead: u8) -> usize {
    match lead {
        0..0xC0 => 1,
        0xC0..0xE0 => 2,
        0xE0..0xF0 => 3,
        _ => 4,
    }
}

/// The end of the macro at `at`: STX, a code byte, the body length as an
/// encoded integer, the body, ETX. `None` when it is cut off or malformed.
fn skip_macro(text: &[u8], at: usize) -> Option<usize> {
    let mut cursor = at + 2;
    let marker = *text.get(cursor)?;
    cursor += 1;
    let length = match marker {
        1..0xD0 => usize::from(marker) - 1,
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
    (*text.get(end)? == ETX).then_some(end + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cyrillic() -> Vec<(char, char)> {
        ('а'..='я')
            .zip('А'..='Я')
            .chain(('ѐ'..='џ').zip('Ѐ'..='Џ'))
            .collect()
    }

    /// The rules of the Cyrillic pairs, both ways, under handles from 0.
    fn with_cyrillic(case: &TextCase, plugin: &str) {
        let mut handle = 0;
        for (small, capital) in cyrillic() {
            case.add(plugin, handle, Direction::Upper, small, capital)
                .unwrap();
            case.add(plugin, handle + 1, Direction::Lower, capital, small)
                .unwrap();
            handle += 2;
        }
    }

    fn upper(letters: &Letters, text: &str, first_only: bool, every_word: bool) -> String {
        let mut bytes = text.as_bytes().to_vec();
        letters.to_upper(&mut bytes, first_only, every_word, |_| false);
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn follows_the_game_rules_for_added_letters() {
        let case = TextCase::default();
        with_cyrillic(&case, "p");
        let letters = case.letters();
        assert_eq!(upper(&letters, "паладин тьмы", true, false), "Паладин тьмы");
        assert_eq!(upper(&letters, "паладин тьмы", true, true), "Паладин Тьмы");
        assert_eq!(upper(&letters, "ёлка и ель", false, false), "ЁЛКА И ЕЛЬ");
        assert_eq!(
            upper(&letters, "latin ок", false, false),
            "latin ОК",
            "ASCII is the game's"
        );
        let mut lower = "ПАЛАДИН Ё".as_bytes().to_vec();
        letters.to_lower(&mut lower);
        assert_eq!(String::from_utf8(lower).unwrap(), "паладин ё");
    }

    #[test]
    fn skips_macros_and_excluded_words() {
        let case = TextCase::default();
        with_cyrillic(&case, "p");
        let letters = case.letters();
        // A macro whose body holds Cyrillic bytes stays as it is.
        let mut text = vec![0x02, 0x10, 0x05];
        text.extend_from_slice("ая".as_bytes());
        text.extend_from_slice(&[0x03]);
        text.extend_from_slice("я".as_bytes());
        let original = text.clone();
        letters.to_upper(&mut text, false, false, |_| false);
        assert_eq!(text[..8], original[..8]);
        assert_eq!(&text[8..], "Я".as_bytes());

        let mut words = "меч и щит".as_bytes().to_vec();
        let excluded = "меч ".len();
        letters.to_upper(&mut words, true, true, |at| at == excluded);
        assert_eq!(String::from_utf8(words).unwrap(), "Меч и Щит");
    }

    #[test]
    fn the_first_rule_of_a_letter_applies_and_invalid_ones_are_refused() {
        let case = TextCase::default();
        assert!(case.add("p", 0, Direction::Upper, 'a', 'A').is_err());
        assert!(case.add("p", 0, Direction::Upper, 'я', 'я').is_err());
        assert!(
            case.add("p", 0, Direction::Upper, 'я', 'ꙗ').is_err(),
            "different UTF-8 length"
        );
        case.add("p", 1, Direction::Upper, 'я', 'Я').unwrap();
        case.add("q", 1, Direction::Upper, 'я', 'Ѣ').unwrap();
        assert_eq!(upper(&case.letters(), "я", false, false), "Я");
        assert!(case.remove("p", 1));
        assert!(!case.remove("p", 1));
        assert_eq!(upper(&case.letters(), "я", false, false), "Ѣ");
        case.remove_plugin("q");
        assert!(case.letters().is_empty());
    }
}
