use std::fmt;
use std::str::FromStr;

use memchr::memmem;

/// Why text is not a signature.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    #[error("the signature is empty")]
    Empty,
    #[error("token {index} (`{token}`) is neither two hexadecimal digits nor `??`")]
    InvalidToken { index: usize, token: String },
    #[error("the signature has no fixed byte")]
    OnlyWildcards,
}

/// A byte pattern with wildcards, written as hexadecimal bytes separated by
/// spaces, with `??` for any byte: `48 8B ?? 24`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// `None` is a wildcard.
    pattern: Vec<Option<u8>>,
    /// The longest run of fixed bytes, searched for first.
    anchor_start: usize,
    anchor: Vec<u8>,
}

impl Signature {
    /// Parses the text form of a signature.
    ///
    /// # Errors
    ///
    /// Returns a [`SignatureError`] for empty text, a token that is not two
    /// hexadecimal digits or `??`, or a pattern with no fixed byte.
    pub fn parse(text: &str) -> Result<Self, SignatureError> {
        let pattern = text
            .split_whitespace()
            .enumerate()
            .map(|(index, token)| parse_token(index, token))
            .collect::<Result<Vec<_>, _>>()?;
        if pattern.is_empty() {
            return Err(SignatureError::Empty);
        }
        let (anchor_start, anchor_len) = longest_fixed_run(&pattern);
        if anchor_len == 0 {
            return Err(SignatureError::OnlyWildcards);
        }
        let anchor = pattern[anchor_start..anchor_start + anchor_len]
            .iter()
            .flatten()
            .copied()
            .collect();
        Ok(Self {
            pattern,
            anchor_start,
            anchor,
        })
    }

    /// The number of bytes the signature covers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pattern.len()
    }

    /// Always `false`: a signature has at least one byte.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pattern.is_empty()
    }

    /// Whether the signature matches `bytes` at their start.
    #[must_use]
    pub fn matches_at(&self, bytes: &[u8]) -> bool {
        bytes.len() >= self.pattern.len()
            && self
                .pattern
                .iter()
                .zip(bytes)
                .all(|(expected, &byte)| expected.is_none_or(|expected| expected == byte))
    }

    /// The offsets of every match in `haystack`, in ascending order,
    /// including overlapping matches.
    #[must_use]
    pub fn find_in(&self, haystack: &[u8]) -> Vec<usize> {
        // `memmem::find_iter` skips overlapping occurrences, so the search
        // restarts one byte after each occurrence of the anchor.
        let finder = memmem::Finder::new(&self.anchor);
        let mut found = Vec::new();
        let mut from = 0;
        while let Some(position) = finder.find(&haystack[from..]).map(|offset| from + offset) {
            if let Some(start) = position.checked_sub(self.anchor_start)
                && self.matches_at(&haystack[start..])
            {
                found.push(start);
            }
            from = position + 1;
        }
        found
    }
}

impl FromStr for Signature {
    type Err = SignatureError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.pattern.iter().enumerate() {
            if index > 0 {
                formatter.write_str(" ")?;
            }
            match byte {
                Some(byte) => write!(formatter, "{byte:02X}")?,
                None => formatter.write_str("??")?,
            }
        }
        Ok(())
    }
}

/// A byte, or `None` for a wildcard.
fn parse_token(index: usize, token: &str) -> Result<Option<u8>, SignatureError> {
    if token == "??" {
        return Ok(None);
    }
    let invalid = || SignatureError::InvalidToken {
        index,
        token: token.to_owned(),
    };
    if token.len() != 2 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    u8::from_str_radix(token, 16)
        .map(Some)
        .map_err(|_| invalid())
}

/// The start and length of the first longest run of fixed bytes.
fn longest_fixed_run(pattern: &[Option<u8>]) -> (usize, usize) {
    let (mut best_start, mut best_len) = (0, 0);
    let mut run_start = 0;
    for (index, byte) in pattern.iter().enumerate() {
        if byte.is_none() {
            run_start = index + 1;
        } else if index + 1 - run_start > best_len {
            best_start = run_start;
            best_len = index + 1 - run_start;
        }
    }
    (best_start, best_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(signature: &Signature, haystack: &[u8]) -> Vec<usize> {
        (0..haystack.len())
            .filter(|&start| signature.matches_at(&haystack[start..]))
            .collect()
    }

    #[test]
    fn parses_and_prints_canonical_text() {
        let signature = Signature::parse("48 8b ?? 24  10").unwrap();
        assert_eq!(signature.len(), 5);
        assert_eq!(signature.to_string(), "48 8B ?? 24 10");
        assert_eq!(
            signature.to_string().parse::<Signature>().unwrap(),
            signature
        );
    }

    #[test]
    fn refuses_malformed_text() {
        assert_eq!(Signature::parse("  "), Err(SignatureError::Empty));
        assert_eq!(
            Signature::parse("?? ??"),
            Err(SignatureError::OnlyWildcards)
        );
        assert_eq!(
            Signature::parse("48 8"),
            Err(SignatureError::InvalidToken {
                index: 1,
                token: "8".to_owned()
            })
        );
        assert_eq!(
            Signature::parse("48 ?"),
            Err(SignatureError::InvalidToken {
                index: 1,
                token: "?".to_owned()
            })
        );
        assert_eq!(
            Signature::parse("48 GG"),
            Err(SignatureError::InvalidToken {
                index: 1,
                token: "GG".to_owned()
            })
        );
        assert_eq!(
            Signature::parse("+1 00"),
            Err(SignatureError::InvalidToken {
                index: 0,
                token: "+1".to_owned()
            })
        );
    }

    #[test]
    fn anchors_on_the_longest_fixed_run() {
        let signature = Signature::parse("?? 01 ?? 02 03 04 ?? 05 06").unwrap();
        assert_eq!((signature.anchor_start, signature.anchor.len()), (3, 3));
    }

    #[test]
    fn finds_matches_with_wildcards_before_the_anchor() {
        let signature = Signature::parse("?? AA BB").unwrap();
        // A match of the anchor at offset 0 has no room for the wildcard.
        assert_eq!(signature.find_in(&[0xAA, 0xBB, 0x00, 0xAA, 0xBB]), vec![2]);
    }

    #[test]
    fn finds_overlapping_matches() {
        let signature = Signature::parse("AA AA").unwrap();
        assert_eq!(signature.find_in(&[0xAA, 0xAA, 0xAA]), vec![0, 1]);
    }

    #[test]
    fn ignores_a_match_cut_by_the_end() {
        let signature = Signature::parse("AA BB ??").unwrap();
        assert!(signature.find_in(&[0x00, 0xAA, 0xBB]).is_empty());
    }

    #[test]
    fn agrees_with_a_naive_search() {
        // Small alphabet so that partial and overlapping matches are common.
        let mut state = 0x2545_F491_u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        for _ in 0..500 {
            let haystack = (0..200).map(|_| (next() % 3) as u8).collect::<Vec<_>>();
            let len = 1 + (next() % 6) as usize;
            let text = (0..len)
                .map(|_| match next() % 4 {
                    0 => "??".to_owned(),
                    byte => format!("{:02X}", byte - 1),
                })
                .collect::<Vec<_>>()
                .join(" ");
            let Ok(signature) = Signature::parse(&text) else {
                continue;
            };
            assert_eq!(
                signature.find_in(&haystack),
                naive(&signature, &haystack),
                "signature {text}"
            );
        }
    }
}
