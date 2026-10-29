//! Thunks: functions of the Windows x64 convention compiled into the
//! runtime, one per slot, which the game calls where it takes a callback.
//! Each stub loads its slot's number and jumps to one common entry, which
//! saves the argument registers, points at the caller's stack arguments,
//! and calls [`dispatch`]; the host then runs the plugin's handler through
//! [`Intercepts::called`]. No code is made or written at run time: a slot's
//! address is fixed in the runtime's code section.
use crate::intercepts::{Arg, Intercepts, MAX_ARGS, MOST_THUNKS, REGISTER_ARGS, Returned};
use std::sync::{Arc, OnceLock};

static INTERCEPTS: OnceLock<Arc<Intercepts>> = OnceLock::new();

/// The bytes between two stubs.
const STUB: u64 = 16;

// The stubs, 16 bytes apart: `mov eax, <slot>` and a jump to the common
// entry. The common entry keeps the stack 16-byte aligned for the call:
// on entry `rsp` is 8 below a 16-byte boundary, `push rbp` aligns it, and
// 0x70 more keep it so. Its frame holds 32 bytes of shadow space for
// `dispatch`, the four general and four `xmm` argument registers, and the
// result; the caller's stack arguments start at `rbp + 0x30`, above the
// return address and the caller's 32 bytes of shadow space. `rax` and
// `xmm0` carry the result back.
std::arch::global_asm!(
    ".pushsection .text",
    ".balign 16",
    ".globl angelica_thunks",
    "angelica_thunks:",
    ".set angelica_thunk_slot, 0",
    ".rept {most}",
    "mov eax, angelica_thunk_slot",
    "jmp angelica_thunk_common",
    ".balign 16",
    ".set angelica_thunk_slot, angelica_thunk_slot + 1",
    ".endr",
    "angelica_thunk_common:",
    "push rbp",
    "mov rbp, rsp",
    "sub rsp, 0x70",
    "mov [rsp + 0x20], rcx",
    "mov [rsp + 0x28], rdx",
    "mov [rsp + 0x30], r8",
    "mov [rsp + 0x38], r9",
    "movq [rsp + 0x40], xmm0",
    "movq [rsp + 0x48], xmm1",
    "movq [rsp + 0x50], xmm2",
    "movq [rsp + 0x58], xmm3",
    "mov ecx, eax",
    "lea rdx, [rsp + 0x20]",
    "lea r8, [rbp + 0x30]",
    "lea r9, [rsp + 0x60]",
    "call {dispatch}",
    "mov rax, [rsp + 0x60]",
    "movq xmm0, [rsp + 0x68]",
    "mov rsp, rbp",
    "pop rbp",
    "ret",
    ".popsection",
    most = const MOST_THUNKS,
    dispatch = sym dispatch,
);

unsafe extern "C" {
    /// The first stub; slot `n` is `n * 16` bytes after it.
    static angelica_thunks: u8;
}

/// Where the game calls the thunk in `slot`, whose calls reach
/// `intercepts`.
///
/// # Errors
/// A slot past the stubs.
pub fn address(slot: usize, intercepts: &Arc<Intercepts>) -> Result<u64, String> {
    if slot >= MOST_THUNKS {
        return Err(format!("the runtime has {MOST_THUNKS} thunks"));
    }
    let _ = INTERCEPTS.set(intercepts.clone());
    let first = (&raw const angelica_thunks) as u64;
    Ok(first + STUB * slot as u64)
}

/// The common entry's call: the arguments of the thunk in `slot`, from the
/// saved registers and the caller's stack, to the host, and its result back.
extern "system" fn dispatch(
    slot: u32,
    registers: *const [u64; 2 * REGISTER_ARGS],
    stack: *const u64,
    result: *mut [u64; 2],
) {
    let returned = super::guard(Returned::default(), || {
        let Some(intercepts) = INTERCEPTS.get() else {
            return Returned::default();
        };
        let slot = slot as usize;
        #[cfg(test)]
        let probe = tests::PROBE.get();
        #[cfg(not(test))]
        let probe: Option<&fn(usize, &[Arg]) -> Returned> = None;
        let arity = match probe {
            Some(_) => MAX_ARGS,
            None => intercepts.thunk_arity(slot).unwrap_or(0).min(MAX_ARGS),
        };
        // SAFETY: the common entry passes its frame's copy of the four
        // general and the four `xmm` argument registers.
        let registers = unsafe { &*registers };
        let args: Vec<Arg> = (0..arity)
            .map(|index| {
                if index < REGISTER_ARGS {
                    Arg {
                        int: registers[index],
                        float: registers[REGISTER_ARGS + index],
                    }
                } else {
                    // SAFETY: the caller passed the thunk's arguments by
                    // its type, whose arguments past the fourth are in its
                    // stack slots from `stack`, one 8-byte slot each.
                    Arg::both(unsafe { stack.add(index - REGISTER_ARGS).read() })
                }
            })
            .collect();
        match probe {
            Some(probe) => probe(slot, &args),
            None => intercepts.called(slot, &args),
        }
    });
    // SAFETY: `result` points at two slots of the common entry's frame.
    unsafe {
        result.write([returned.integer, returned.float]);
    }
}

#[cfg(test)]
mod tests {
    use crate::intercepts::{Arg, Returned};
    use std::sync::OnceLock;

    /// Stands in for the host in these tests: sees every thunk's slot and
    /// arguments as the stub passed them.
    pub(super) static PROBE: OnceLock<fn(usize, &[Arg]) -> Returned> = OnceLock::new();

    /// Gives back the slot, the first integer, the second float, and the
    /// sixth argument, from the stack, as the result.
    fn probe(slot: usize, args: &[Arg]) -> Returned {
        let half = f64::from_bits(args[1].float);
        Returned {
            integer: slot as u64 * 1000 + args[0].int + args[5].int,
            float: (half * 2.0).to_bits(),
        }
    }

    #[test]
    fn a_thunk_passes_registers_and_stack_arguments_and_returns_both_results() {
        let _ = PROBE.set(probe);
        let intercepts = std::sync::Arc::default();
        let third = super::address(3, &intercepts).unwrap();
        let args = [7, 1.25f64.to_bits(), 0, 0, 0, 40];
        let returned = super::super::act::call(third, &args).unwrap();
        assert_eq!(returned.integer, 3047);
        assert_eq!(returned.float, 2.5f64.to_bits());
    }

    #[test]
    fn stubs_are_sixteen_bytes_apart_in_code() {
        let intercepts = std::sync::Arc::default();
        let first = super::address(0, &intercepts).unwrap();
        let last = super::address(crate::intercepts::MOST_THUNKS - 1, &intercepts).unwrap();
        assert_eq!(first % 16, 0);
        assert_eq!(
            last - first,
            16 * (crate::intercepts::MOST_THUNKS as u64 - 1)
        );
        assert!(super::address(crate::intercepts::MOST_THUNKS, &intercepts).is_err());
    }
}
