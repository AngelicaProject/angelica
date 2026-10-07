//! A jump written over the first instructions at a target, with the other
//! threads suspended and moved off the overwritten instructions.

use crate::HookError;
use crate::memory::{read, write};
use crate::relocate::JUMP_LEN;
use crate::threads::Frozen;

#[derive(Debug)]
pub(crate) struct Splice {
    target: usize,
    original: Vec<u8>,
    patch: Vec<u8>,
    /// For each overwritten instruction, its offset at the target and the
    /// address where a thread stopped on it continues while the jump is in
    /// place.
    moved: Vec<(u32, usize)>,
    enabled: bool,
}

impl Splice {
    /// A `jmp rel32` from `target` to `destination`, padded with `int3` over
    /// the rest of `original`.
    pub(crate) fn new(
        target: usize,
        original: Vec<u8>,
        destination: usize,
        moved: Vec<(u32, usize)>,
    ) -> Result<Self, HookError> {
        // The destination was allocated within 2 GiB of the target.
        let displacement = i32::try_from(destination.wrapping_sub(target + JUMP_LEN).cast_signed())
            .map_err(|_| HookError::NoNearMemory(target))?;
        let mut patch = vec![0xE9];
        patch.extend_from_slice(&displacement.to_le_bytes());
        patch.resize(original.len(), 0xCC);
        Ok(Self {
            target,
            original,
            patch,
            moved,
            enabled: false,
        })
    }

    pub(crate) fn target(&self) -> usize {
        self.target
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Writes the jump while the target holds the original bytes.
    ///
    /// # Safety
    ///
    /// The target stays mapped, no code other than this crate changes its
    /// bytes meanwhile, and the destination may run from now on.
    pub(crate) unsafe fn enable(&mut self) -> Result<(), HookError> {
        if self.enabled {
            return Ok(());
        }
        // SAFETY: the caller guarantees the target is mapped.
        unsafe { self.replace(&self.original, &self.patch, true)? };
        self.enabled = true;
        Ok(())
    }

    /// Writes the original bytes back while the target holds the jump.
    ///
    /// # Safety
    ///
    /// As for [`Splice::enable`].
    pub(crate) unsafe fn disable(&mut self) -> Result<(), HookError> {
        if !self.enabled {
            return Ok(());
        }
        // SAFETY: as in `enable`.
        unsafe { self.replace(&self.patch, &self.original, false)? };
        self.enabled = false;
        Ok(())
    }

    /// # Safety
    ///
    /// The target is mapped and no other code changes it meanwhile.
    unsafe fn replace(&self, from: &[u8], to: &[u8], enabling: bool) -> Result<(), HookError> {
        // SAFETY: the caller guarantees the target is mapped.
        if unsafe { read(self.target, from.len()) } != from {
            return Err(HookError::Changed(self.target));
        }
        let frozen = Frozen::others();
        for (handle, rip) in frozen.instruction_pointers() {
            let destination = if enabling {
                rip.checked_sub(self.target as u64)
                    .and_then(|offset| {
                        self.moved
                            .iter()
                            .find(|&&(old, _)| u64::from(old) == offset)
                    })
                    .map(|&(_, new)| new)
            } else {
                self.moved
                    .iter()
                    .find(|&&(_, new)| new as u64 == rip)
                    .map(|&(old, _)| self.target + old as usize)
            };
            if let Some(destination) = destination {
                Frozen::set_instruction_pointer(handle, destination as u64);
            }
        }
        // SAFETY: every other thread is suspended, and none is on an
        // instruction whose bytes change.
        unsafe { write(self.target, to) }
    }
}

impl Drop for Splice {
    /// Removes the jump if it is still this splice's. The memory it jumps to
    /// stays allocated.
    fn drop(&mut self) {
        // SAFETY: `enable` required the same contract for the life of the
        // splice.
        let _ = unsafe { self.disable() };
    }
}
