use std::sync::atomic::{AtomicUsize, Ordering};

use crate::HookError;
use crate::memory::write;

/// A vtable slot that points to a replacement function while enabled.
///
/// A slot is one aligned pointer, so other threads see either the old or the
/// new function, never a mix; no thread needs to be suspended.
#[derive(Debug)]
pub struct VtableSlot {
    slot: usize,
    original: usize,
    replacement: usize,
    enabled: bool,
}

impl VtableSlot {
    /// Prepares the replacement of the function in the slot at `slot`.
    ///
    /// # Safety
    ///
    /// `slot` is an 8-byte aligned slot of a vtable that stays mapped for the
    /// life of the process, and `replacement` has the signature and calling
    /// convention of the function in it.
    #[must_use]
    pub unsafe fn new(slot: usize, replacement: usize) -> Self {
        Self {
            slot,
            // SAFETY: the caller guarantees an aligned, mapped slot.
            original: unsafe { load(slot) },
            replacement,
            enabled: false,
        }
    }

    /// The function the slot held, which the replacement may call.
    #[must_use]
    pub fn original(&self) -> usize {
        self.original
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Points the slot at the replacement.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::Changed`] when the slot no longer holds the
    /// original function, or an OS error.
    ///
    /// # Safety
    ///
    /// The replacement may be called from now on.
    pub unsafe fn enable(&mut self) -> Result<(), HookError> {
        if self.enabled {
            return Ok(());
        }
        // SAFETY: the slot was valid in `new` and stays mapped.
        unsafe { self.exchange(self.original, self.replacement)? };
        self.enabled = true;
        Ok(())
    }

    /// Points the slot back at the original function.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::Changed`] when the slot no longer holds the
    /// replacement, as when another tool replaced it in turn; the slot then
    /// stays enabled.
    ///
    /// # Safety
    ///
    /// Nothing else requires the replacement to stay in place.
    pub unsafe fn disable(&mut self) -> Result<(), HookError> {
        if !self.enabled {
            return Ok(());
        }
        // SAFETY: as in `enable`.
        unsafe { self.exchange(self.replacement, self.original)? };
        self.enabled = false;
        Ok(())
    }

    unsafe fn exchange(&self, from: usize, to: usize) -> Result<(), HookError> {
        // SAFETY: the slot is aligned and mapped.
        if unsafe { load(self.slot) } != from {
            return Err(HookError::Changed(self.slot));
        }
        // SAFETY: an aligned 8-byte write is a single store on x64, so
        // threads reading the slot see the old or the new pointer.
        unsafe { write(self.slot, &to.to_le_bytes()) }
    }
}

impl Drop for VtableSlot {
    fn drop(&mut self) {
        // SAFETY: the contract of `disable` was accepted by `enable`.
        let _ = unsafe { self.disable() };
    }
}

/// Reads the slot with an atomic load.
///
/// # Safety
///
/// `slot` is an 8-byte aligned, mapped address.
unsafe fn load(slot: usize) -> usize {
    // SAFETY: the caller guarantees alignment and mapping; vtables are
    // shared memory, so the read is atomic.
    unsafe { AtomicUsize::from_ptr(slot as *mut usize) }.load(Ordering::Acquire)
}
