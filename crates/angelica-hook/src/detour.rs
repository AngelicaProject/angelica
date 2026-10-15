use crate::memory::{allocate_near, read, readable_len, write};
use crate::relocate::{JUMP_LEN, check_flow, relocate};
use crate::splice::Splice;
use crate::{FunctionCode, HookError};

/// Space reserved for a trampoline: the moved instructions, which are at
/// most a few instructions of up to 15 bytes, and the jump back.
const TRAMPOLINE_LEN: usize = 128;
/// `jmp qword ptr [rip+0]` followed by the 8-byte target.
const RELAY_LEN: usize = 14;
/// How many bytes at the target are decoded to find whole instructions.
pub(crate) const READ_LEN: usize = 64;

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
    splice: Splice,
    trampoline: usize,
}

impl Detour {
    /// Prepares a detour from `target`, the start of `function`, to
    /// `replacement` without changing the target. The function's code is
    /// checked for branches into the bytes the detour overwrites.
    ///
    /// # Errors
    ///
    /// Returns a [`HookError`] when the first instructions cannot be moved,
    /// the function's code cannot be shown free of branches into them, or
    /// no memory is free near the target.
    ///
    /// # Safety
    ///
    /// `target` is the start of an instruction in executable code that stays
    /// mapped for the life of the process, and every range of `function` is
    /// readable code. `replacement` is a function with the same
    /// signature and calling convention as the target.
    pub unsafe fn new(
        target: usize,
        replacement: usize,
        function: &FunctionCode,
    ) -> Result<Self, HookError> {
        // SAFETY: `readable_len` bounds the read to readable memory.
        let code = unsafe { read(target, readable_len(target, READ_LEN)) };
        // SAFETY: the caller guarantees the function's ranges are readable.
        unsafe { check_branches(function, target, &code) }?;

        let trampoline = allocate_near(target, TRAMPOLINE_LEN)?;
        let relocation = relocate(&code, target as u64, JUMP_LEN, trampoline as u64)?;
        if relocation.code.len() > TRAMPOLINE_LEN {
            return Err(
                crate::RelocateError::Encode("the trampoline is too long".to_owned()).into(),
            );
        }
        // SAFETY: the trampoline memory was just allocated and nothing can
        // reach it yet.
        unsafe { write(trampoline, &relocation.code)? };

        let relay = allocate_near(target, RELAY_LEN)?;
        let mut relay_code = vec![0xFF, 0x25, 0, 0, 0, 0];
        relay_code.extend_from_slice(&(replacement as u64).to_le_bytes());
        // SAFETY: as for the trampoline.
        unsafe { write(relay, &relay_code)? };

        let moved = relocation
            .offsets
            .iter()
            .map(|&(old, new)| (old, trampoline + new as usize))
            .collect();
        let splice = Splice::new(target, code[..relocation.stolen_len].to_vec(), relay, moved)?;
        Ok(Self { splice, trampoline })
    }

    /// The address that runs the original function.
    #[must_use]
    pub fn trampoline(&self) -> usize {
        self.trampoline
    }

    #[must_use]
    pub fn target(&self) -> usize {
        self.splice.target()
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.splice.is_enabled()
    }

    /// Writes the jump at the target. A suspended thread on an overwritten
    /// instruction continues at the same instruction in the trampoline.
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
        // SAFETY: the caller accepts the contract of `Splice::enable`.
        unsafe { self.splice.enable() }
    }

    /// Restores the target's original bytes. A suspended thread on a moved
    /// instruction in the trampoline continues at the original.
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
        // SAFETY: as in `enable`.
        unsafe { self.splice.disable() }
    }
}

/// Refuses a function whose code cannot be shown free of branches into
/// the instructions a jump at `location`, whose bytes are `code`, would
/// overwrite.
///
/// # Safety
///
/// Every range of `function` is readable code.
pub(crate) unsafe fn check_branches(
    function: &FunctionCode,
    location: usize,
    code: &[u8],
) -> Result<(), HookError> {
    let stolen = relocate(code, location as u64, JUMP_LEN, location as u64)?.stolen_len;
    let chunks = function
        .ranges
        .iter()
        .map(|range| {
            // SAFETY: the caller guarantees the range is readable.
            let bytes = unsafe { read(range.start, range.len()) };
            (range.start as u64, bytes)
        })
        .collect::<Vec<_>>();
    let chunks = chunks
        .iter()
        .map(|(start, bytes)| (*start, bytes.as_slice()))
        .collect::<Vec<_>>();
    check_flow(
        &chunks,
        function.start as u64,
        function.prologue,
        location as u64,
        stolen,
    )?;
    Ok(())
}
