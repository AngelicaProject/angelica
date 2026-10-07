#![cfg(windows)]

use std::sync::atomic::{AtomicU64, Ordering};

use angelica_hook::testing::executable;
use angelica_hook::{Context, MidHandler, MidHook};

fn function<F: Copy>(address: usize) -> F {
    assert_eq!(size_of::<F>(), size_of::<usize>());
    // SAFETY: every caller passes the address of machine code with the
    // signature `F`, and `F` is a function pointer type.
    unsafe { std::mem::transmute_copy(&address) }
}

fn hook(location: usize, handler: MidHandler) -> MidHook {
    // SAFETY: `location` starts an instruction of machine code copied for
    // the test, and the handlers do not unwind.
    let mut hook = unsafe { MidHook::new(location, handler, None) }.unwrap();
    // SAFETY: only this test changes the code.
    unsafe { hook.enable() }.unwrap();
    hook
}

#[test]
fn a_handler_reads_and_changes_registers() {
    static SEEN: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
    extern "C" fn handler(context: &mut Context) {
        SEEN[0].store(context.rax, Ordering::Relaxed);
        SEEN[1].store(context.rcx, Ordering::Relaxed);
        SEEN[2].store(context.rdx, Ordering::Relaxed);
        context.rdx = 100;
    }

    // 0: mov rax, rcx; 3: add rax, rdx; 6: add rax, 1; 10: ret
    let code = executable(&[
        0x48, 0x89, 0xC8, 0x48, 0x01, 0xD0, 0x48, 0x83, 0xC0, 0x01, 0xC3,
    ]);
    let add = function::<extern "C" fn(i64, i64) -> i64>(code);
    assert_eq!(add(5, 7), 13);

    let mut hooked = hook(code + 3, handler);
    assert_eq!(add(5, 7), 106);
    let seen = SEEN.each_ref().map(|value| value.load(Ordering::Relaxed));
    assert_eq!(seen, [5, 5, 7]);

    // SAFETY: only this test changes the code.
    unsafe { hooked.disable() }.unwrap();
    assert_eq!(add(5, 7), 13);
}

#[test]
fn registers_and_flags_survive_a_handler() {
    extern "C" fn nothing(_: &mut Context) {}
    extern "C" fn clobber_flags(_: &mut Context) {
        // Any Rust code changes the flags; a comparison makes sure.
        std::hint::black_box(std::hint::black_box(1) < std::hint::black_box(2));
    }

    // 0: mov rax, r8; 3: add rax, r9; 6: add rax, rcx; 9: add rax, rdx; 12: ret
    let sum = executable(&[
        0x4C, 0x89, 0xC0, 0x4C, 0x01, 0xC8, 0x48, 0x01, 0xC8, 0x48, 0x01, 0xD0, 0xC3,
    ]);
    let _sum_hook = hook(sum, nothing);
    assert_eq!(
        function::<extern "C" fn(i64, i64, i64, i64) -> i64>(sum)(1, 2, 3, 4),
        10
    );

    // 0: xor eax, eax; 2: cmp rcx, rdx; 5: nop x5; 10: setl al; 13: ret
    let less = executable(&[
        0x31, 0xC0, 0x48, 0x39, 0xD1, 0x90, 0x90, 0x90, 0x90, 0x90, 0x0F, 0x9C, 0xC0, 0xC3,
    ]);
    let _less_hook = hook(less + 5, clobber_flags);
    let less = function::<extern "C" fn(i64, i64) -> i64>(less);
    assert_eq!(less(1, 2), 1);
    assert_eq!(less(2, 1), 0);
}

#[test]
fn a_handler_changes_xmm_registers() {
    extern "C" fn handler(context: &mut Context) {
        context.xmm[0][0] = 42.5_f64.to_bits();
    }
    // 0: movq rax, xmm0; 5: ret
    let bits = executable(&[0x66, 0x48, 0x0F, 0x7E, 0xC0, 0xC3]);
    let _hook = hook(bits, handler);
    let bits = function::<extern "C" fn(f64) -> u64>(bits);
    assert_eq!(bits(1.0), 42.5_f64.to_bits());
}

#[test]
fn the_context_knows_the_stack_pointer() {
    static RSP: AtomicU64 = AtomicU64::new(0);
    extern "C" fn handler(context: &mut Context) {
        RSP.store(context.rsp(), Ordering::Relaxed);
    }
    // 0: mov rax, rsp; 3: nop; 4: nop; 5: ret
    let stack = executable(&[0x48, 0x89, 0xE0, 0x90, 0x90, 0xC3]);
    let _hook = hook(stack, handler);
    let rsp = function::<extern "C" fn() -> u64>(stack)();
    assert_eq!(RSP.load(Ordering::Relaxed), rsp);
}
