//! Damaged inputs for tests of the parsers and checks that read what a
//! plugin, a package, or the player provides: they refuse a damaged input
//! and never panic.

/// A small deterministic generator, so a failing case can be replayed.
pub struct Mutator(u64);

impl Mutator {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound.max(1) as u64).unwrap_or(0)
    }

    /// `bytes` with one to four random changes: bits, bytes, fields set to
    /// edge values, truncation, or a repeated slice.
    pub fn mutate(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        for _ in 0..=self.below(4) {
            let at = self.below(bytes.len());
            match self.below(5) {
                0 if !bytes.is_empty() => bytes[at] ^= 1 << self.below(8),
                1 if !bytes.is_empty() => bytes[at] = self.next().to_le_bytes()[0],
                2 if bytes.len() >= at + 8 => {
                    let edges = [0, 1, 0xFF, 0xFFFF_FFFF, u64::MAX, bytes.len() as u64];
                    let value = edges[self.below(edges.len())];
                    let width = [2, 4, 8][self.below(3)];
                    bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
                }
                3 => bytes.truncate(at),
                _ => {
                    let end = (at + self.below(64)).min(bytes.len());
                    let slice = bytes[at..end].to_vec();
                    let to = self.below(bytes.len());
                    bytes.splice(to..to, slice);
                }
            }
        }
        bytes
    }

    /// `text` with random changes that keep it UTF-8: characters removed,
    /// repeated, or replaced with ones parsers treat specially.
    pub fn mutate_text(&mut self, text: &str) -> String {
        const SPECIAL: [&str; 12] = [
            "\"",
            " ",
            "\\",
            "/",
            "-",
            "\n",
            "\u{2}",
            "0",
            "99999999999999999999",
            "é",
            "\u{0}",
            "",
        ];
        let mut chars: Vec<String> = text.chars().map(String::from).collect();
        for _ in 0..=self.below(4) {
            let at = self.below(chars.len());
            match self.below(3) {
                0 if !chars.is_empty() => {
                    chars.remove(at);
                }
                1 if !chars.is_empty() => {
                    let repeated = chars[at].clone();
                    chars.insert(at, repeated);
                }
                _ => chars.insert(at, SPECIAL[self.below(SPECIAL.len())].to_owned()),
            }
        }
        chars.concat()
    }
}
