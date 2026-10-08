//! Letter pairs plugins add to the game's case conversion (the `text-case`
//! interface) and the conversion itself, applied after the game's own.
//! Platform independent: the Windows adapter passes the string the game just
//! converted.
#![forbid(unsafe_code)]
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, PoisonError, RwLock},
};

/// Letters one plugin can add.
pub const MAX_LETTERS: usize = 4096;

const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const SPACE: u8 = b' ';

/// The letters of every plugin, as the conversion uses them.
#[derive(Debug, Default)]
pub struct Letters {
    /// UTF-8 of a lower-case letter to its upper case, and the reverse.
    upper: HashMap<Vec<u8>, Vec<u8>>,
    lower: HashMap<Vec<u8>, Vec<u8>>,
}

#[derive(Default)]
struct State {
    owners: BTreeMap<String, Vec<(char, char)>>,
    letters: Arc<Letters>,
}

#[derive(Default)]
pub struct TextCase {
    state: RwLock<State>,
}

impl TextCase {
    /// Replaces the plugin's letters.
    ///
    /// # Errors
    /// Refuses ASCII letters, a pair whose letters differ in UTF-8 length or
    /// are equal, a repeated letter, and a letter another plugin paired
    /// differently.
    pub fn set(&self, plugin: &str, pairs: &[(char, char)]) -> Result<(), String> {
        if pairs.len() > MAX_LETTERS {
            return Err(format!("at most {MAX_LETTERS} letters"));
        }
        for &(lower, upper) in pairs {
            if lower.is_ascii() || upper.is_ascii() {
                return Err(format!(
                    "{lower}/{upper}: the game converts ASCII letters itself"
                ));
            }
            if lower == upper || lower.len_utf8() != upper.len_utf8() {
                return Err(format!(
                    "{lower}/{upper}: the letters must differ and have the same UTF-8 length"
                ));
            }
        }
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let mut owners = state.owners.clone();
        owners.remove(plugin);
        if !pairs.is_empty() {
            owners.insert(plugin.into(), pairs.to_vec());
        }
        let letters = combine(&owners)?;
        state.owners = owners;
        state.letters = Arc::new(letters);
        Ok(())
    }

    pub fn remove_plugin(&self, plugin: &str) {
        let _ = self.set(plugin, &[]);
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

fn combine(owners: &BTreeMap<String, Vec<(char, char)>>) -> Result<Letters, String> {
    let mut upper: HashMap<char, (char, &str)> = HashMap::new();
    let mut lower: HashMap<char, (char, &str)> = HashMap::new();
    for (plugin, pairs) in owners {
        for &(small, capital) in pairs {
            for (map, from, to) in [(&mut upper, small, capital), (&mut lower, capital, small)] {
                if let Some(&(known, owner)) = map.get(&from) {
                    if known != to {
                        return Err(format!("{from} is already paired with {known} by {owner}"));
                    }
                    if owner == plugin.as_str() {
                        return Err(format!("{from} is listed twice"));
                    }
                }
                map.insert(from, (to, plugin));
            }
            if upper.contains_key(&capital) || lower.contains_key(&small) {
                return Err(format!("{small}/{capital}: a letter cannot be both cases"));
            }
        }
    }
    let encode = |map: HashMap<char, (char, &str)>| {
        map.into_iter()
            .map(|(from, (to, _))| (from.to_string().into_bytes(), to.to_string().into_bytes()))
            .collect()
    };
    Ok(Letters {
        upper: encode(upper),
        lower: encode(lower),
    })
}

impl Letters {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.upper.is_empty()
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

    fn upper(letters: &Letters, text: &str, first_only: bool, every_word: bool) -> String {
        let mut bytes = text.as_bytes().to_vec();
        letters.to_upper(&mut bytes, first_only, every_word, |_| false);
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn follows_the_game_rules_for_added_letters() {
        let case = TextCase::default();
        case.set("p", &cyrillic()).unwrap();
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
        case.set("p", &cyrillic()).unwrap();
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
    fn refuses_conflicts_and_invalid_pairs() {
        let case = TextCase::default();
        assert!(case.set("p", &[('a', 'A')]).is_err());
        assert!(case.set("p", &[('я', 'я')]).is_err());
        assert!(
            case.set("p", &[('я', 'ꙗ')]).is_err(),
            "different UTF-8 length"
        );
        case.set("p", &[('я', 'Я')]).unwrap();
        assert!(case.set("q", &[('я', 'Ѣ')]).is_err());
        case.set("q", &[('я', 'Я')]).unwrap();
        assert!(case.set("r", &[('ё', 'Ё'), ('ё', 'Ё')]).is_err());
        case.remove_plugin("p");
        case.remove_plugin("q");
        assert!(case.letters().is_empty());
    }
}
