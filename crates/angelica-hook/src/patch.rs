use crate::HookError;
use crate::memory::{read, write};
use crate::threads::Frozen;

/// Bytes of code replaced while the patch is applied.
#[derive(Debug)]
pub struct CodePatch {
    address: usize,
    original: Vec<u8>,
    replacement: Vec<u8>,
    applied: bool,
}

impl CodePatch {
    /// Prepares a patch, checking that the code at `address` is `original`.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::LengthMismatch`] when the byte strings differ in
    /// length, or [`HookError::Mismatch`] when the code is not `original`.
    ///
    /// # Safety
    ///
    /// `address..address + original.len()` is mapped code of this process
    /// that stays mapped for the life of the process.
    pub unsafe fn new(
        address: usize,
        original: &[u8],
        replacement: &[u8],
    ) -> Result<Self, HookError> {
        if original.len() != replacement.len() {
            return Err(HookError::LengthMismatch);
        }
        // SAFETY: the caller guarantees the range is mapped.
        if unsafe { read(address, original.len()) } != original {
            return Err(HookError::Mismatch(address));
        }
        Ok(Self {
            address,
            original: original.to_vec(),
            replacement: replacement.to_vec(),
            applied: false,
        })
    }

    #[must_use]
    pub fn is_applied(&self) -> bool {
        self.applied
    }

    /// Writes the replacement.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::Changed`] when the code is no longer the
    /// original, [`HookError::ThreadInside`] when a thread is stopped inside
    /// the bytes (the caller may try again), or an OS error.
    ///
    /// # Safety
    ///
    /// The replacement is valid code where it is written.
    pub unsafe fn apply(&mut self) -> Result<(), HookError> {
        if self.applied {
            return Ok(());
        }
        // SAFETY: the range was checked in `new` and stays mapped.
        unsafe { self.swap(&self.original, &self.replacement)? };
        self.applied = true;
        Ok(())
    }

    /// Writes the original bytes back.
    ///
    /// # Errors
    ///
    /// As for [`CodePatch::apply`], with the roles of the bytes exchanged.
    ///
    /// # Safety
    ///
    /// Nothing still depends on the replacement being in place.
    pub unsafe fn revert(&mut self) -> Result<(), HookError> {
        if !self.applied {
            return Ok(());
        }
        // SAFETY: as in `apply`.
        unsafe { self.swap(&self.replacement, &self.original)? };
        self.applied = false;
        Ok(())
    }

    /// Replaces `from` with `to` while every other thread is suspended.
    unsafe fn swap(&self, from: &[u8], to: &[u8]) -> Result<(), HookError> {
        // SAFETY: the range is mapped (checked in `new`).
        if unsafe { read(self.address, from.len()) } != from {
            return Err(HookError::Changed(self.address));
        }
        let frozen = Frozen::others();
        let start = self.address as u64;
        let end = start + from.len() as u64;
        if frozen
            .instruction_pointers()
            .any(|(_, rip)| rip > start && rip < end)
        {
            return Err(HookError::ThreadInside(self.address));
        }
        // SAFETY: every other thread is suspended and none is inside the range.
        unsafe { write(self.address, to) }
    }
}
