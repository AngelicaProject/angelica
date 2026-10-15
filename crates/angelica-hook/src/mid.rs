use iced_x86::IcedError;
use iced_x86::code_asm::{
    AsmRegister64, AsmRegisterXmm, CodeAssembler, al, ptr, r8, r9, r10, r11, r12, r13, r14, r15,
    rax, rbp, rbx, rcx, rdi, rdx, rsi, rsp, xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7, xmm8,
    xmm9, xmm10, xmm11, xmm12, xmm13, xmm14, xmm15,
};

use crate::detour::{READ_LEN, check_branches};
use crate::memory::{allocate_near, read, readable_len, write};
use crate::relocate::{JUMP_LEN, relocate};
use crate::splice::Splice;
use crate::{FunctionCode, HookError, RelocateError};

/// Space reserved for a stub: saving and restoring the context, the call,
/// the moved instructions, and the jump back.
const STUB_LEN: usize = 768;

/// The registers at a hooked location, as a handler sees and may change
/// them. `rsp` cannot be changed: it is the address just past this context.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// `xmm0` to `xmm15`, each as its low and high 64 bits.
    pub xmm: [[u64; 2]; 16],
    pub rflags: u64,
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
}

impl Context {
    /// The stack pointer at the hooked location.
    #[must_use]
    pub fn rsp(&self) -> u64 {
        std::ptr::from_ref(self) as u64 + size_of::<Self>() as u64
    }
}

/// A function called with the context at a hooked location. It must not
/// unwind: a panic is caught inside it.
pub type MidHandler = extern "C" fn(&mut Context);

/// A handler that also gets the value its hook was made with, so one
/// handler serves several hooks.
pub type MidDataHandler = extern "C" fn(&mut Context, usize);

/// A handler at a function's first instruction that may return from the
/// function at once: when it returns true, the registers are restored from
/// the context and the function returns to its caller with the context's
/// `rax` (and `xmm0`) without running; otherwise the function runs.
pub type MidReturnHandler = extern "C" fn(&mut Context, usize) -> bool;

/// The general-purpose registers in the order they are pushed; `Context`
/// lists them in reverse.
const PUSHED: [AsmRegister64; 15] = [
    rax, rcx, rdx, rbx, rbp, rsi, rdi, r8, r9, r10, r11, r12, r13, r14, r15,
];
const XMM: [AsmRegisterXmm; 16] = [
    xmm0, xmm1, xmm2, xmm3, xmm4, xmm5, xmm6, xmm7, xmm8, xmm9, xmm10, xmm11, xmm12, xmm13, xmm14,
    xmm15,
];

/// A call to a handler at an instruction inside a function, with the
/// registers the handler may read and change.
///
/// The location is overwritten with a `jmp rel32` to a stub that saves the
/// registers and flags into a [`Context`], calls the handler on a 16-byte
/// aligned stack, restores the registers from the context, runs the moved
/// instructions, and jumps back.
#[derive(Debug)]
pub struct MidHook {
    splice: Splice,
}

impl MidHook {
    /// Prepares a hook at `location` in `function` without changing it. The
    /// function's code is checked for branches into the bytes the hook
    /// overwrites.
    ///
    /// # Errors
    ///
    /// As for [`crate::Detour::new`].
    ///
    /// # Safety
    ///
    /// `location` is the start of an instruction in executable code that
    /// stays mapped for the life of the process, every range of `function`
    /// is readable code, and `handler` does not unwind.
    pub unsafe fn new(
        location: usize,
        handler: MidHandler,
        function: &FunctionCode,
    ) -> Result<Self, HookError> {
        // SAFETY: the caller's guarantees are those of `prepare`.
        unsafe { Self::prepare(location, handler as usize as u64, None, function) }
    }

    /// As [`MidHook::new`], with a handler that is called with `data` too.
    ///
    /// # Errors
    ///
    /// As for [`crate::Detour::new`].
    ///
    /// # Safety
    ///
    /// As for [`MidHook::new`].
    pub unsafe fn with_data(
        location: usize,
        handler: MidDataHandler,
        data: usize,
        function: &FunctionCode,
    ) -> Result<Self, HookError> {
        // SAFETY: the caller's guarantees are those of `prepare`.
        unsafe {
            Self::prepare(
                location,
                handler as usize as u64,
                Some(data as u64),
                function,
            )
        }
    }

    /// A hook at the first instruction of a function whose handler may
    /// make the function return at once ([`MidReturnHandler`]).
    ///
    /// # Errors
    ///
    /// As for [`crate::Detour::new`].
    ///
    /// # Safety
    ///
    /// As for [`MidHook::new`], and `location` is the first instruction of a
    /// function, where the stack pointer points to the return address, so
    /// returning from there returns to the caller.
    pub unsafe fn returning(
        location: usize,
        handler: MidReturnHandler,
        data: usize,
        function: &FunctionCode,
    ) -> Result<Self, HookError> {
        // SAFETY: the caller's guarantees are those of `prepare_with`.
        unsafe {
            Self::prepare_with(
                location,
                save_call_restore_or_return(handler as usize as u64, data as u64),
                function,
            )
        }
    }

    /// # Safety
    ///
    /// As for [`MidHook::new`]; `handler` is the address of a `MidHandler`,
    /// or with `data` of a `MidDataHandler`.
    unsafe fn prepare(
        location: usize,
        handler: u64,
        data: Option<u64>,
        function: &FunctionCode,
    ) -> Result<Self, HookError> {
        // SAFETY: the caller's guarantees are those of `prepare_with`.
        unsafe { Self::prepare_with(location, save_call_restore(handler, data), function) }
    }

    /// # Safety
    ///
    /// As for [`MidHook::new`]; `saving` is the stub's code before the moved
    /// instructions, which falls through to them.
    unsafe fn prepare_with(
        location: usize,
        saving: Result<CodeAssembler, IcedError>,
        function: &FunctionCode,
    ) -> Result<Self, HookError> {
        // SAFETY: `readable_len` bounds the read to readable memory.
        let code = unsafe { read(location, readable_len(location, READ_LEN)) };
        // SAFETY: the caller guarantees the function's ranges are readable.
        unsafe { check_branches(function, location, &code) }?;

        let stub = allocate_near(location, STUB_LEN)?;
        let saving = saving
            .and_then(|mut assembler| assembler.assemble(stub as u64))
            .map_err(|error| RelocateError::Encode(error.to_string()))?;
        let moved_at = stub + saving.len();
        let relocation = relocate(&code, location as u64, JUMP_LEN, moved_at as u64)?;
        let mut stub_code = saving;
        stub_code.extend_from_slice(&relocation.code);
        if stub_code.len() > STUB_LEN {
            return Err(RelocateError::Encode("the stub is too long".to_owned()).into());
        }
        // SAFETY: the stub memory was just allocated and nothing can reach it yet.
        unsafe { write(stub, &stub_code)? };

        // A thread stopped on the hooked instruction runs the handler; one
        // stopped on a later overwritten instruction has passed it.
        let moved = relocation
            .offsets
            .iter()
            .map(|&(old, new)| {
                (
                    old,
                    if old == 0 {
                        stub
                    } else {
                        moved_at + new as usize
                    },
                )
            })
            .collect();
        let splice = Splice::new(
            location,
            code[..relocation.stolen_len].to_vec(),
            stub,
            moved,
        )?;
        Ok(Self { splice })
    }

    #[must_use]
    pub fn location(&self) -> usize {
        self.splice.target()
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.splice.is_enabled()
    }

    /// Writes the jump at the location.
    ///
    /// # Errors
    ///
    /// As for [`crate::Detour::enable`].
    ///
    /// # Safety
    ///
    /// As for [`crate::Detour::enable`]; the handler may be called from now on.
    pub unsafe fn enable(&mut self) -> Result<(), HookError> {
        // SAFETY: the caller accepts the contract of `Splice::enable`.
        unsafe { self.splice.enable() }
    }

    /// Restores the original bytes at the location.
    ///
    /// # Errors
    ///
    /// As for [`crate::Detour::disable`].
    ///
    /// # Safety
    ///
    /// As for [`crate::Detour::enable`].
    pub unsafe fn disable(&mut self) -> Result<(), HookError> {
        // SAFETY: as in `enable`.
        unsafe { self.splice.disable() }
    }
}

/// Saves the context, calls a [`MidReturnHandler`] with it and `data`, and
/// restores it; then returns from the function when the handler said so,
/// or falls through to the moved instructions.
fn save_call_restore_or_return(handler: u64, data: u64) -> Result<CodeAssembler, IcedError> {
    let mut code = CodeAssembler::new(64)?;
    save_and_call(&mut code, handler, Some(data))?;
    let mut run = code.create_label();
    code.test(al, al)?;
    code.jz(run)?;
    restore(&mut code)?;
    code.ret()?;
    code.set_label(&mut run)?;
    restore(&mut code)?;
    Ok(code)
}

/// Saves the context, calls the handler with it (and `data` as the second
/// argument), and restores it.
fn save_call_restore(handler: u64, data: Option<u64>) -> Result<CodeAssembler, IcedError> {
    let mut code = CodeAssembler::new(64)?;
    save_and_call(&mut code, handler, data)?;
    restore(&mut code)?;
    Ok(code)
}

/// Saves the context and calls the handler with it; `rbx` and the stack
/// pointer are left at the context.
fn save_and_call(
    code: &mut CodeAssembler,
    handler: u64,
    data: Option<u64>,
) -> Result<(), IcedError> {
    for register in PUSHED {
        code.push(register)?;
    }
    code.pushfq()?;
    code.sub(rsp, 256)?;
    for (index, register) in (0_i32..).zip(XMM) {
        code.movdqu(ptr(rsp + 16 * index), register)?;
    }
    // rbx keeps the context address across the call; it is restored from
    // the context afterwards.
    code.mov(rcx, rsp)?;
    if let Some(data) = data {
        code.mov(rdx, data)?;
    }
    code.mov(rbx, rsp)?;
    code.and(rsp, -16)?;
    code.sub(rsp, 32)?;
    code.mov(rax, handler)?;
    code.call(rax)?;
    code.mov(rsp, rbx)?;
    Ok(())
}

/// Loads the registers and flags back from the context at the stack
/// pointer and pops it.
fn restore(code: &mut CodeAssembler) -> Result<(), IcedError> {
    for (index, register) in (0_i32..).zip(XMM) {
        code.movdqu(register, ptr(rsp + 16 * index))?;
    }
    code.add(rsp, 256)?;
    code.popfq()?;
    for register in PUSHED.into_iter().rev() {
        code.pop(register)?;
    }
    Ok(())
}
