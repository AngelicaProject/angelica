//! The structure of the game's encoded strings (`SeString`): UTF-8 text with
//! macros of the form STX, code, length, body, ETX, where the body is a
//! sequence of expressions.
//!
//! The check needs no macro catalog: any macro code and any nullary value of
//! the reserved ranges is accepted, so macros the game adds later are never
//! refused. It is the well-formed string rule of the Harmonia pack format,
//! checked against the same vectors (`tests/well_formed.vectors.txt`).

const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const STRING_EXPRESSION: u8 = 0xFF;

/// Bounds recursion. Game strings reach 46 levels (chains of `<if>`), far
/// below this.
pub const MAX_DEPTH: usize = 256;

/// The longest text the game stores in a row cell, without its terminator.
pub const MAX_LENGTH: usize = 65535;

/// Whether `bytes` is a well-formed, non-empty string without a terminator.
#[must_use]
pub fn is_well_formed(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.len() <= MAX_LENGTH && text(bytes, 0)
}

/// Whether `bytes` can be written into a row cell: empty or well-formed.
#[must_use]
pub fn is_cell_text(bytes: &[u8]) -> bool {
    bytes.is_empty() || is_well_formed(bytes)
}

/// The text of a game string as a player reads it, without its macros: a
/// line break macro becomes `\n`, a non-breaking space a space, a hyphen
/// macro `-`, and a soft hyphen nothing. Text that macros produce, such as
/// names or numbers filled in by the game, is left out. Bytes that are not
/// UTF-8 or a malformed macro are kept as replacement characters.
#[must_use]
pub fn plain(mut bytes: &[u8]) -> String {
    const NEW_LINE: u8 = 0x10;
    const NON_BREAKING_SPACE: u8 = 0x1D;
    const HYPHEN: u8 = 0x1F;
    let mut out = String::new();
    while let Some(&first) = bytes.first() {
        if first == STX
            && let Some(length) = payload(bytes, 0)
        {
            match bytes[1] {
                NEW_LINE => out.push('\n'),
                NON_BREAKING_SPACE => out.push(' '),
                HYPHEN => out.push('-'),
                // A soft hyphen and every other macro add nothing.
                _ => {}
            }
            bytes = &bytes[length..];
            continue;
        }
        let length = bytes[1..]
            .iter()
            .position(|&b| b == STX)
            .map_or(bytes.len(), |at| at + 1);
        out.push_str(&String::from_utf8_lossy(&bytes[..length]));
        bytes = &bytes[length..];
    }
    out
}

fn text(mut bytes: &[u8], depth: usize) -> bool {
    while let Some(&first) = bytes.first() {
        let length = match first {
            0 => return false,
            STX => match payload(bytes, depth) {
                Some(length) => length,
                None => return false,
            },
            _ => {
                let length = bytes
                    .iter()
                    .position(|&b| b == STX || b == 0)
                    .unwrap_or(bytes.len());
                if std::str::from_utf8(&bytes[..length]).is_err() {
                    return false;
                }
                length
            }
        };
        bytes = &bytes[length..];
    }
    true
}

fn payload(bytes: &[u8], depth: usize) -> Option<usize> {
    let (length, size) = canonical_integer(bytes.get(2..)?)?;
    let start = 2 + size;
    let end = start.checked_add(usize::try_from(length).ok()?)?;
    if *bytes.get(end)? != ETX {
        return None;
    }
    let mut body = &bytes[start..end];
    while !body.is_empty() {
        let used = expression(body, depth + 1)?;
        body = &body[used..];
    }
    Some(end + 1)
}

fn expression(bytes: &[u8], depth: usize) -> Option<usize> {
    if depth > MAX_DEPTH {
        return None;
    }
    let kind = *bytes.first()?;
    if integer(bytes).is_some() {
        return canonical_integer(bytes).map(|(_, size)| size);
    }
    match kind {
        STRING_EXPRESSION => {
            let (length, size) = canonical_integer(&bytes[1..])?;
            let end = (1 + size).checked_add(usize::try_from(length).ok()?)?;
            let inner = bytes.get(1 + size..end)?;
            text(inner, depth).then_some(end)
        }
        // Nullary values such as the hour of the set time.
        0xD0..=0xDF | 0xEC => Some(1),
        // Parameters: a type byte and an operand.
        0xE8..=0xEB => Some(1 + expression(&bytes[1..], depth + 1)?),
        // Comparisons: a type byte and two operands.
        0xE0..=0xE5 => {
            let left = expression(&bytes[1..], depth + 1)?;
            let right = expression(&bytes[1 + left..], depth + 1)?;
            Some(1 + left + right)
        }
        _ => None,
    }
}

/// An integer: one byte (value + 1) below 0xD0; otherwise a marker
/// 0xF0..=0xFE whose low bits, plus one, flag which of four big-endian bytes
/// follow. Present bytes are never zero. Returns the value and its size.
fn integer(bytes: &[u8]) -> Option<(u32, usize)> {
    let first = *bytes.first()?;
    if (0x01..=0xCF).contains(&first) {
        return Some((u32::from(first) - 1, 1));
    }
    if !(0xF0..=0xFE).contains(&first) || bytes.len() < 2 {
        return None;
    }
    let flags = (first + 1) & 0x0F;
    let mut value = 0u32;
    let mut at = 1;
    for (flag, shift) in [(8, 24), (4, 16), (2, 8), (1, 0)] {
        if flags & flag == 0 {
            continue;
        }
        let byte = *bytes.get(at)?;
        if byte == 0 {
            return None;
        }
        value |= u32::from(byte) << shift;
        at += 1;
    }
    Some((value, at))
}

/// An integer in its canonical form: the one-byte form whenever the value
/// fits it.
fn canonical_integer(bytes: &[u8]) -> Option<(u32, usize)> {
    integer(bytes).filter(|&(value, size)| size == 1 || value >= 0xCF)
}
