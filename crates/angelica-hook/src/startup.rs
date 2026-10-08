//! A detour for the executable entry point, before it can execute.
use crate::HookError;
use crate::memory::{allocate_block_near, read, readable_len, write};
use crate::relocate::{JUMP_LEN, relocate};

/// An entry-point detour that needs no mutex or thread suspension.
/// Allocations and trampolines are retained for the process lifetime.
#[derive(Debug)]
pub struct StartupDetour {
    target: usize,
    original: Vec<u8>,
    patch: Vec<u8>,
    trampoline: usize,
}

impl StartupDetour {
    /// Prepares the entry-point detour without changing the executable.
    ///
    /// # Errors
    /// Returns an error if the instructions cannot be moved or allocation fails.
    ///
    /// # Safety
    /// `target` is executable code obtained from Windows and remains mapped.
    /// `replacement` has the target's calling convention and signature.
    /// No thread executes or modifies the entry point during preparation.
    pub unsafe fn prepare(target: usize, replacement: usize) -> Result<Self, HookError> {
        // SAFETY: the caller guarantees executable memory; bound the read to its region.
        let code = unsafe { read(target, readable_len(target, 64)) };
        // Bypass the ordinary allocator's shared block list and mutex under loader lock.
        let trampoline = allocate_block_near(target).ok_or(HookError::NoNearMemory(target))?;
        let moved = relocate(&code, target as u64, JUMP_LEN, trampoline as u64)?;
        if moved.code.len() > 4096 {
            return Err(HookError::LengthMismatch);
        }
        let relay = trampoline + 4096;
        let mut jump = vec![0xff, 0x25, 0, 0, 0, 0];
        jump.extend_from_slice(&(replacement as u64).to_le_bytes());
        // SAFETY: new private executable allocation, unreachable by other threads.
        unsafe {
            write(trampoline, &moved.code)?;
            write(relay, &jump)?;
        }
        let displacement = i32::try_from(relay.wrapping_sub(target + JUMP_LEN).cast_signed())
            .map_err(|_| HookError::NoNearMemory(target))?;
        let mut patch = vec![0xe9];
        patch.extend_from_slice(&displacement.to_le_bytes());
        patch.resize(moved.stolen_len, 0xcc);
        Ok(Self {
            target,
            original: code[..moved.stolen_len].to_vec(),
            patch,
            trampoline,
        })
    }

    /// The retained original code, including any earlier tool's jump.
    #[must_use]
    pub fn trampoline(&self) -> usize {
        self.trampoline
    }

    /// Installs the prepared jump.
    ///
    /// # Errors
    /// Returns an error if the bytes changed or memory protection fails.
    ///
    /// # Safety
    /// No thread can execute or modify the entry point yet. Publish the
    /// replacement's state before calling this method.
    pub unsafe fn enable(&self) -> Result<(), HookError> {
        // SAFETY: preparation and the caller guarantee mapped, exclusively accessed code.
        unsafe { self.replace(&self.original, &self.patch) }
    }

    /// Restores the entry point, refusing to overwrite a later tool's hook.
    ///
    /// # Errors
    /// Returns an error if another tool changed the jump or an OS call fails.
    /// On failure, the caller can still continue through `trampoline()`.
    ///
    /// # Safety
    /// No thread executes or modifies the entry point during this call.
    pub unsafe fn disable(&self) -> Result<(), HookError> {
        // SAFETY: the caller guarantees exclusive access to the entry point.
        unsafe { self.replace(&self.patch, &self.original) }
    }

    unsafe fn replace(&self, from: &[u8], to: &[u8]) -> Result<(), HookError> {
        // SAFETY: the caller guarantees mapped memory and exclusive access.
        if unsafe { read(self.target, from.len()) } != from {
            return Err(HookError::Changed(self.target));
        }
        // SAFETY: no thread can execute or read the changed bytes yet.
        unsafe { write(self.target, to) }
    }
}
