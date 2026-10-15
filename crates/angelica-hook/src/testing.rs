//! Executable copies of machine code for tests.

use crate::FunctionCode;
use crate::memory::{allocate_near, write};

/// Copies `code` into new executable memory and returns its address. The
/// memory is near this crate's code and is never freed.
///
/// # Panics
///
/// Panics when no executable memory can be allocated or the code is longer
/// than 64 KiB.
#[must_use]
pub fn executable(code: &[u8]) -> usize {
    let address = allocate_near(executable as fn(&[u8]) -> usize as usize, code.len())
        .expect("executable memory");
    // SAFETY: the memory was just allocated and nothing can reach it yet.
    unsafe { write(address, code) }.expect("the copy");
    address
}

/// The code of a function of `len` bytes at `start` in one range, without a
/// prologue.
#[must_use]
pub fn whole(start: usize, len: usize) -> FunctionCode {
    FunctionCode {
        start,
        ranges: std::iter::once(start..start + len).collect(),
        prologue: 0,
    }
}
