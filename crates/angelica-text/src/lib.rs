//! Game text as the host and plugins exchange it: the structure of the
//! game's encoded strings ([`sestring`]), the guard that ties a replacement
//! to the text it was made for ([`guard`]), and the text table format
//! ([`table`]).
pub mod sestring;
pub mod table;

use sha2::{Digest, Sha256};

/// The guard of an original string: the first 8 bytes, read as a
/// little-endian integer, of SHA-256 over `domain`, the string's length as
/// a little-endian `u32`, and its bytes without the terminator.
///
/// The domain lets a format keep the guards it already has; it is at most
/// [`table::MAX_DOMAIN`] bytes.
#[must_use]
pub fn guard(domain: &[u8], original: &[u8]) -> u64 {
    let length = u32::try_from(original.len()).unwrap_or(u32::MAX);
    let digest = Sha256::new()
        .chain_update(domain)
        .chain_update(length.to_le_bytes())
        .chain_update(original)
        .finalize();
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    u64::from_le_bytes(first)
}

#[cfg(test)]
mod tests {
    #[test]
    fn guard_hashes_domain_length_and_bytes() {
        // SHA-256("D" || 03 00 00 00 || "abc"), first 8 bytes little-endian.
        let mut input = b"D".to_vec();
        input.extend_from_slice(&3u32.to_le_bytes());
        input.extend_from_slice(b"abc");
        let digest = <sha2::Sha256 as sha2::Digest>::digest(&input);
        let expected = u64::from_le_bytes(digest[..8].try_into().unwrap());
        assert_eq!(super::guard(b"D", b"abc"), expected);
        assert_ne!(super::guard(b"D", b"abc"), super::guard(b"E", b"abc"));
    }
}
