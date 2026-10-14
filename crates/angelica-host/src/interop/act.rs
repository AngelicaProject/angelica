//! Calls of the game's functions for Lens actions. A function of the map is
//! called with the Windows x64 convention: the first four arguments in
//! `rcx`, `rdx`, `r8`, and `r9` or, for floats, `xmm0` to `xmm3`, the rest
//! on the stack above 32 bytes of shadow space. Every argument is passed as
//! its 64 bits in both its general and its `xmm` register, so one call
//! serves every mix of integers and floats: the function reads the register
//! its own type uses.
use crate::lens::act::{MOST_ARGS, Returned};
use std::arch::asm;

/// Calls the function at `address` with `args`.
///
/// # Errors
/// More than [`MOST_ARGS`] arguments.
pub fn call(address: u64, args: &[u64]) -> Result<Returned, String> {
    if args.len() > MOST_ARGS {
        return Err(format!("a function takes at most {MOST_ARGS} arguments"));
    }
    if address == 0 {
        return Err("the function's address is unknown".into());
    }
    let mut slots = [0u64; MOST_ARGS];
    slots[..args.len()].copy_from_slice(args);
    let integer: u64;
    let float: f64;
    // SAFETY: `address` is a `function` entry of the game map resolved in
    // the running executable, whose ABI the map verified against the
    // function's code; `args` has one value of the ABI's kind per argument,
    // and extra slots are zeros the function never reads. The call is made
    // on the game's main thread, which the entry's `thread` requires. The
    // stack is 16-byte aligned on entry to the block (as for any call), and
    // 0x80 bytes keep it so: 32 bytes of shadow space and 12 stack slots.
    unsafe {
        asm!(
            "sub rsp, 0x80",
            "mov rax, [r12 + 0x20]",
            "mov [rsp + 0x20], rax",
            "mov rax, [r12 + 0x28]",
            "mov [rsp + 0x28], rax",
            "mov rax, [r12 + 0x30]",
            "mov [rsp + 0x30], rax",
            "mov rax, [r12 + 0x38]",
            "mov [rsp + 0x38], rax",
            "mov rax, [r12 + 0x40]",
            "mov [rsp + 0x40], rax",
            "mov rax, [r12 + 0x48]",
            "mov [rsp + 0x48], rax",
            "mov rax, [r12 + 0x50]",
            "mov [rsp + 0x50], rax",
            "mov rax, [r12 + 0x58]",
            "mov [rsp + 0x58], rax",
            "mov rax, [r12 + 0x60]",
            "mov [rsp + 0x60], rax",
            "mov rax, [r12 + 0x68]",
            "mov [rsp + 0x68], rax",
            "mov rax, [r12 + 0x70]",
            "mov [rsp + 0x70], rax",
            "mov rax, [r12 + 0x78]",
            "mov [rsp + 0x78], rax",
            "mov rcx, [r12]",
            "mov rdx, [r12 + 0x8]",
            "mov r8, [r12 + 0x10]",
            "mov r9, [r12 + 0x18]",
            "movq xmm0, rcx",
            "movq xmm1, rdx",
            "movq xmm2, r8",
            "movq xmm3, r9",
            "call r13",
            "add rsp, 0x80",
            in("r12") slots.as_ptr(),
            in("r13") address,
            out("rax") integer,
            out("xmm0") float,
            clobber_abi("system"),
        );
    }
    Ok(Returned {
        integer,
        float: float.to_bits(),
    })
}

#[cfg(test)]
mod tests {
    use super::call;

    extern "system" fn mixed(
        first: u32,
        half: f32,
        third: u64,
        fourth: f64,
        fifth: u8,
        quarter: f32,
        last: i64,
    ) -> u64 {
        let floats = f64::from(half) + fourth + f64::from(quarter);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let whole = floats as u64;
        (u64::from(first) + third + u64::from(fifth) + whole * 1000)
            .wrapping_add(last.cast_unsigned())
    }

    extern "system" fn halve(value: f64) -> f64 {
        value / 2.0
    }

    #[test]
    fn integers_and_floats_reach_their_registers_and_the_stack() {
        let args = [
            5,
            u64::from(1.5f32.to_bits()),
            100,
            2.25f64.to_bits(),
            7,
            u64::from(0.25f32.to_bits()),
            (-2i64).cast_unsigned(),
        ];
        let mixed = mixed as *const () as usize as u64;
        let returned = call(mixed, &args).unwrap();
        assert_eq!(returned.integer, 5 + 100 + 7 - 2 + 4000);
        let halve = halve as *const () as usize as u64;
        let returned = call(halve, &[9.0f64.to_bits()]).unwrap();
        assert_eq!(returned.float, 4.5f64.to_bits());
        assert!(call(mixed, &[0; 17]).is_err());
    }
}
