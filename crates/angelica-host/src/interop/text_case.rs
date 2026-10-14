//! Plugins' letters in the game's case conversion. `Utf8String::ToUpper`
//! and `ToLower` change only the letters the game knows; the detours run
//! the game's function, then apply the plugins' letters to the same string
//! in place with the same rule. Letters keep their UTF-8 length, so the
//! string never changes length.
use super::entries::{Entries, Function};
use crate::{catalog::Catalog, text_case::TextCase};
use angelica_hook::Detour;
use angelica_map::ValueKind::{I32, I64, Ptr, U8};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

const TO_UPPER: &str = crate::text_case::UPPER_ENTRY;
const TO_LOWER: &str = crate::text_case::LOWER_ENTRY;
/// Bytes of an exclusion list record (`docs/formats/game-structures.md`
/// describes the text structures the host reads).
const EXCLUSION_RECORD: usize = 16;
const MAX_EXCLUSIONS: usize = 256;
const MAX_LENGTH: usize = 1 << 20;

struct Layout {
    string: usize,
    used: usize,
    word: usize,
    word_length: usize,
}

pub struct CaseHooks {
    case: Arc<TextCase>,
    layout: Layout,
    pub errors: AtomicU64,
}

static ADAPTER: OnceLock<CaseHooks> = OnceLock::new();
static UPPER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static LOWER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

type ToUpper = unsafe extern "system" fn(usize, u8, u8, u8, usize) -> usize;
type ToLower = unsafe extern "system" fn(usize) -> usize;

/// Installs both detours once.
pub fn install(catalog: &Catalog, base: usize, case: Arc<TextCase>) -> Result<(), String> {
    if ADAPTER.get().is_some() {
        return Ok(());
    }
    let entries = Entries { catalog, base };
    let layout = Layout {
        string: entries.field("text.utf8_string.string_ptr", Ptr)?,
        used: entries.field("text.utf8_string.used_size", I64)?,
        word: entries.field("text.case_exclusion.word", Ptr)?,
        word_length: entries.field("text.case_exclusion.length", I32)?,
    };
    if layout.word + 8 > EXCLUSION_RECORD || layout.word_length + 4 > EXCLUSION_RECORD {
        return Err("exclusion fields lie outside the 16-byte record".into());
    }
    let upper = entries.function(TO_UPPER, &[Ptr, U8, U8, U8, Ptr], Ptr)?;
    let lower = entries.function(TO_LOWER, &[Ptr], Ptr)?;
    install_resolved(layout, &upper, &lower, case)
}

fn install_resolved(
    layout: Layout,
    upper: &Function,
    lower: &Function,
    case: Arc<TextCase>,
) -> Result<(), String> {
    let mut detours = Vec::new();
    for (function, replacement, original) in [
        (upper, to_upper as ToUpper as usize, &UPPER_ORIGINAL),
        (lower, to_lower as ToLower as usize, &LOWER_ORIGINAL),
    ] {
        // SAFETY: the map verified this address as the start of a function
        // with this ABI; the replacement has the same ABI.
        let detour = unsafe { Detour::new(function.address, replacement, function.length) }
            .map_err(|e| format!("case detour: {e}"))?;
        original.store(detour.trampoline(), Ordering::Release);
        detours.push(detour);
    }
    if ADAPTER
        .set(CaseHooks {
            case,
            layout,
            errors: AtomicU64::new(0),
        })
        .is_err()
    {
        return Ok(());
    }
    for detour in &mut detours {
        // SAFETY: the adapter and trampolines are published; nothing else in
        // this process changes these bytes meanwhile.
        unsafe { detour.enable() }.map_err(|e| format!("case detour: {e}"))?;
    }
    std::mem::forget(detours);
    Ok(())
}

unsafe extern "system" fn to_upper(
    text: usize,
    first_only: u8,
    every_word: u8,
    normalize: u8,
    exclusions: usize,
) -> usize {
    // SAFETY: the trampoline runs the game's function with its own ABI.
    let result = unsafe {
        std::mem::transmute::<usize, ToUpper>(UPPER_ORIGINAL.load(Ordering::Acquire))(
            text, first_only, every_word, normalize, exclusions,
        )
    };
    if let Some(adapter) = ADAPTER.get() {
        let failed = super::guard(true, || {
            adapter.apply(text, |letters, bytes| {
                // The words are compared with the text as the game left it.
                let original = if exclusions == 0 {
                    Vec::new()
                } else {
                    bytes.to_vec()
                };
                letters.to_upper(bytes, first_only != 0, every_word != 0, |at| {
                    adapter.excluded(exclusions, original.get(at..).unwrap_or_default())
                });
            })
        });
        if failed {
            adapter.errors.fetch_add(1, Ordering::Relaxed);
        }
    }
    result
}

unsafe extern "system" fn to_lower(text: usize) -> usize {
    // SAFETY: as above.
    let result = unsafe {
        std::mem::transmute::<usize, ToLower>(LOWER_ORIGINAL.load(Ordering::Acquire))(text)
    };
    if let Some(adapter) = ADAPTER.get() {
        let failed = super::guard(true, || {
            adapter.apply(text, crate::text_case::Letters::to_lower)
        });
        if failed {
            adapter.errors.fetch_add(1, Ordering::Relaxed);
        }
    }
    result
}

impl CaseHooks {
    /// Converts the string the game just converted. Returns whether it
    /// failed; the game's own result stays then.
    fn apply(
        &self,
        text: usize,
        convert: impl FnOnce(&crate::text_case::Letters, &mut [u8]),
    ) -> bool {
        let letters = self.case.letters();
        if letters.is_empty() || text == 0 {
            return false;
        }
        // SAFETY: `text` is the Utf8String the game's function just
        // converted in place; its fields are live, at verified offsets.
        let (pointer, used) = unsafe {
            (
                std::ptr::read_unaligned((text + self.layout.string) as *const usize),
                std::ptr::read_unaligned((text + self.layout.used) as *const i64),
            )
        };
        let Ok(used) = usize::try_from(used) else {
            return true;
        };
        if pointer == 0 || used <= 1 {
            return false;
        }
        if used > MAX_LENGTH {
            return true;
        }
        // SAFETY: the string's buffer holds `used` bytes including the
        // terminator; the game's function just wrote them.
        let bytes = unsafe { std::slice::from_raw_parts_mut(pointer as *mut u8, used - 1) };
        convert(&letters, bytes);
        false
    }

    /// Whether a word of the game's exclusion list starts `rest`, compared
    /// as the game compares it.
    fn excluded(&self, list: usize, rest: &[u8]) -> bool {
        if list == 0 {
            return false;
        }
        for index in 0..MAX_EXCLUSIONS {
            let record = list + index * EXCLUSION_RECORD;
            // SAFETY: the game passed this list to ToUpper; each record holds
            // a word pointer and length at verified offsets, and a record
            // with a zero length ends it.
            let (word, length) = unsafe {
                (
                    std::ptr::read_unaligned((record + self.layout.word) as *const usize),
                    std::ptr::read_unaligned((record + self.layout.word_length) as *const i32),
                )
            };
            let Ok(length) = usize::try_from(length) else {
                return false;
            };
            if length == 0 {
                return false;
            }
            if word != 0 && length <= rest.len() {
                // SAFETY: the word holds `length` bytes.
                let word = unsafe { std::slice::from_raw_parts(word as *const u8, length) };
                if rest.starts_with(word) {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Utf8String`: the bytes at 0, the used size at 16.
    struct Text {
        object: [usize; 3],
        _bytes: Vec<u8>,
    }

    fn text(value: &str) -> Text {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        Text {
            object: [bytes.as_mut_ptr() as usize, 0, bytes.len()],
            _bytes: bytes,
        }
    }

    fn read_back(text: &Text) -> String {
        // SAFETY: the string's bytes live in `_bytes`.
        let bytes =
            unsafe { std::slice::from_raw_parts(text.object[0] as *const u8, text.object[2] - 1) };
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[test]
    fn detours_apply_added_letters_after_the_game() {
        // `mov rax, rcx; nop; nop; ret`: returns the string unchanged, as the
        // game's functions do for text without letters they know.
        let code = [0x48, 0x89, 0xC8, 0x90, 0x90, 0xC3, 0x90, 0x90];
        let upper = Function {
            address: angelica_hook::testing::executable(&code),
            length: Some(code.len()),
        };
        let lower = Function {
            address: angelica_hook::testing::executable(&code),
            length: Some(code.len()),
        };
        let case = Arc::new(TextCase::default());
        let layout = Layout {
            string: 0,
            used: 16,
            word: 0,
            word_length: 8,
        };
        install_resolved(layout, &upper, &lower, case.clone()).unwrap();
        // SAFETY: the detoured synthetic functions have the hooked ABIs.
        let (to_upper, to_lower) = unsafe {
            (
                std::mem::transmute::<usize, ToUpper>(upper.address),
                std::mem::transmute::<usize, ToLower>(lower.address),
            )
        };

        let unchanged = text("меч и щит");
        // SAFETY: the string object lives across the call; no exclusions.
        unsafe { to_upper(std::ptr::from_ref(&unchanged.object) as usize, 1, 1, 0, 0) };
        assert_eq!(read_back(&unchanged), "меч и щит", "no letters added yet");

        for (handle, (small, capital)) in
            [('м', 'М'), ('и', 'И'), ('щ', 'Щ')].into_iter().enumerate()
        {
            let handle = u32::try_from(handle).unwrap();
            case.add(
                "plugin",
                handle,
                crate::text_case::Direction::Upper,
                small,
                capital,
            )
            .unwrap();
            case.add(
                "plugin",
                handle + 10,
                crate::text_case::Direction::Lower,
                capital,
                small,
            )
            .unwrap();
        }
        let word = "и ".as_bytes();
        let exclusions: [usize; 4] = [word.as_ptr() as usize, 3, 0, 0];
        let words = text("меч и щит");
        // SAFETY: as above, with a list of one excluded word ended by a zero length.
        unsafe {
            to_upper(
                std::ptr::from_ref(&words.object) as usize,
                1,
                1,
                0,
                exclusions.as_ptr() as usize,
            )
        };
        assert_eq!(
            read_back(&words),
            "Меч и Щит",
            "every word but the excluded one"
        );

        let lowered = text("МИЩ");
        // SAFETY: as above.
        unsafe { to_lower(std::ptr::from_ref(&lowered.object) as usize) };
        assert_eq!(read_back(&lowered), "мищ");
    }
}
