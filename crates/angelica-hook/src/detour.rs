use crate::HookError;
use crate::memory::{allocate_near, read, readable_len, write};
use crate::relocate::{JUMP_LEN, branch_into, relocate};
use crate::threads::Frozen;

/// Space reserved for a trampoline: the moved instructions, which are at
/// most a few instructions of up to 15 bytes, and the jump back.
const TRAMPOLINE_LEN: usize = 128;
/// `jmp qword ptr [rip+0]` followed by the 8-byte target.
const RELAY_LEN: usize = 14;
/// How many bytes at the target are decoded to find whole instructions.
const READ_LEN: usize = 64;

/// A jump from the start of a function to a replacement function.
///
/// The target's first instructions move into a trampoline, which calls the
/// original function. The target starts with a `jmp rel32` to a relay near
/// it, which jumps to the replacement anywhere in the address space. The
/// detour may be installed over a jump another tool wrote, and another tool
/// may install its own over this one; it is then removed only after that
/// tool's.
#[derive(Debug)]
pub struct Detour {
    target: usize,
    original: Vec<u8>,
    patch: Vec<u8>,
    trampoline: usize,
    /// For each moved instruction, its offset at the target and in the
    /// trampoline.
    offsets: Vec<(u32, u32)>,
    enabled: bool,
}

impl Detour {
    /// Prepares a detour from `target` to `replacement` without changing the
    /// target. With `function_len`, the function's code is checked for
    /// branches into the bytes the detour overwrites.
    ///
    /// # Errors
    ///
    /// Returns a [`HookError`] when the first instructions cannot be moved,
    /// the function branches into them, or no memory is free near the
    /// target.
    ///
    /// # Safety
    ///
    /// `target` is the start of an instruction in executable code that stays
    /// mapped for the life of the process, with `function_len` bytes of
    /// readable code when given. `replacement` is a function with the same
    /// signature and calling convention as the target.
    pub unsafe fn new(
        target: usize,
        replacement: usize,
        function_len: Option<usize>,
    ) -> Result<Self, HookError> {
        let available = readable_len(target, READ_LEN);
        // SAFETY: `readable_len` bounded the read to readable memory.
        let code = unsafe { read(target, available) };
        if let Some(len) = function_len {
            // SAFETY: the caller guarantees `len` readable bytes of code.
            let function = unsafe { read(target, len) };
            let stolen = relocate(&code, target as u64, JUMP_LEN, target as u64)?.stolen_len;
            if let Some((from, to)) = branch_into(&function, target as u64, target as u64, stolen) {
                return Err(HookError::BranchIntoPatch { from, to });
            }
        }

        let trampoline = allocate_near(target, TRAMPOLINE_LEN)?;
        let relocation = relocate(&code, target as u64, JUMP_LEN, trampoline as u64)?;
        if relocation.code.len() > TRAMPOLINE_LEN {
            return Err(HookError::Relocate(crate::RelocateError::Encode(
                "the trampoline is too long".to_owned(),
            )));
        }
        // SAFETY: the trampoline memory was just allocated and nothing can
        // reach it yet.
        unsafe { write(trampoline, &relocation.code)? };

        let relay = allocate_near(target, RELAY_LEN)?;
        let mut relay_code = vec![0xFF, 0x25, 0, 0, 0, 0];
        relay_code.extend_from_slice(&(replacement as u64).to_le_bytes());
        // SAFETY: as for the trampoline.
        unsafe { write(relay, &relay_code)? };

        let mut patch = vec![0xE9];
        // The relay is within 2 GiB of the target, so the difference fits.
        let displacement = i32::try_from(relay.wrapping_sub(target + JUMP_LEN).cast_signed())
            .map_err(|_| HookError::NoNearMemory(target))?;
        patch.extend_from_slice(&displacement.to_le_bytes());
        patch.resize(relocation.stolen_len, 0xCC);

        Ok(Self {
            target,
            original: code[..relocation.stolen_len].to_vec(),
            patch,
            trampoline,
            offsets: relocation.offsets,
            enabled: false,
        })
    }

    /// The address that runs the original function.
    #[must_use]
    pub fn trampoline(&self) -> usize {
        self.trampoline
    }

    #[must_use]
    pub fn target(&self) -> usize {
        self.target
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Writes the jump at the target. A suspended thread inside the bytes
    /// it overwrites continues at the same instruction in the trampoline.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::Changed`] when the target's bytes changed since
    /// the detour was prepared, or an OS error.
    ///
    /// # Safety
    ///
    /// No code other than this crate changes the target's bytes while this
    /// call runs, and the replacement may be called from now on.
    pub unsafe fn enable(&mut self) -> Result<(), HookError> {
        if self.enabled {
            return Ok(());
        }
        // SAFETY: the bytes at the target were readable in `new` and stay mapped.
        if unsafe { read(self.target, self.original.len()) } != self.original {
            return Err(HookError::Changed(self.target));
        }
        let frozen = Frozen::others();
        for (handle, rip) in frozen.instruction_pointers() {
            if let Some(offset) = self.offset_at_target(rip)
                && let Some(&(_, new)) = self.offsets.iter().find(|&&(old, _)| old == offset)
            {
                Frozen::set_instruction_pointer(handle, (self.trampoline + new as usize) as u64);
            }
        }
        // SAFETY: every other thread is suspended and none is inside the
        // bytes being replaced any more.
        unsafe { write(self.target, &self.patch)? };
        drop(frozen);
        self.enabled = true;
        Ok(())
    }

    /// Restores the target's original bytes. A suspended thread inside the
    /// moved instructions of the trampoline continues at the same
    /// instruction at the target.
    ///
    /// # Errors
    ///
    /// Returns [`HookError::Changed`] when the jump at the target is no
    /// longer this detour's, as when another tool installed its own over it,
    /// or an OS error. The detour then stays enabled.
    ///
    /// # Safety
    ///
    /// As for [`Detour::enable`].
    pub unsafe fn disable(&mut self) -> Result<(), HookError> {
        if !self.enabled {
            return Ok(());
        }
        // SAFETY: as in `enable`.
        if unsafe { read(self.target, self.patch.len()) } != self.patch {
            return Err(HookError::Changed(self.target));
        }
        let frozen = Frozen::others();
        for (handle, rip) in frozen.instruction_pointers() {
            let moved_end = self.offsets.last().map_or(0, |&(_, new)| new as usize);
            if let Some(offset) = rip.checked_sub(self.trampoline as u64)
                && offset <= moved_end as u64
                && let Some(&(old, _)) = self
                    .offsets
                    .iter()
                    .find(|&&(_, new)| u64::from(new) == offset)
            {
                Frozen::set_instruction_pointer(handle, (self.target + old as usize) as u64);
            }
        }
        // SAFETY: every other thread is suspended and none is about to run
        // the moved instructions.
        unsafe { write(self.target, &self.original)? };
        drop(frozen);
        self.enabled = false;
        Ok(())
    }

    fn offset_at_target(&self, rip: u64) -> Option<u32> {
        let offset = rip.checked_sub(self.target as u64)?;
        (offset < self.original.len() as u64).then(|| u32::try_from(offset).unwrap_or(u32::MAX))
    }
}

impl Drop for Detour {
    /// Removes the jump if this detour still owns it. The trampoline and the
    /// relay stay in memory.
    fn drop(&mut self) {
        // SAFETY: the same contract as `disable`, which `enable` required
        // from the caller for the life of the detour.
        let _ = unsafe { self.disable() };
    }
}
