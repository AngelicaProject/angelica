#![cfg(windows)]

use std::sync::atomic::{AtomicUsize, Ordering};

use angelica_hook::testing::executable;
use angelica_hook::{CodePatch, Detour, HookError, RelocateError, VtableSlot};

type Add = extern "C" fn(i64, i64) -> i64;
type Unary = extern "C" fn(i64) -> i64;
type Nullary = extern "C" fn() -> i64;

/// lea rax, [rcx+rdx]; add rax, 0; ret
const ADD: [u8; 9] = [0x48, 0x8D, 0x04, 0x11, 0x48, 0x83, 0xC0, 0x00, 0xC3];

fn function<F: Copy>(address: usize) -> F {
    assert_eq!(size_of::<F>(), size_of::<usize>());
    // SAFETY: every caller passes the address of machine code with the
    // signature `F`, and `F` is a function pointer type.
    unsafe { std::mem::transmute_copy(&address) }
}

#[test]
fn a_detour_replaces_the_function_and_can_call_the_original() {
    static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn replacement(a: i64, b: i64) -> i64 {
        function::<Add>(ORIGINAL.load(Ordering::Acquire))(a, b) + 100
    }

    let target = executable(&ADD);
    let add = function::<Add>(target);
    // SAFETY: `target` is a whole function and `replacement` matches it.
    let mut detour = unsafe { Detour::new(target, replacement as Add as usize, None) }.unwrap();
    ORIGINAL.store(detour.trampoline(), Ordering::Release);
    assert_eq!(add(2, 3), 5);

    // SAFETY: only this test changes the target.
    unsafe { detour.enable() }.unwrap();
    assert!(detour.is_enabled());
    assert_eq!(add(2, 3), 105);
    assert_eq!(function::<Add>(detour.trampoline())(2, 3), 5);

    // SAFETY: as above.
    unsafe { detour.disable() }.unwrap();
    assert_eq!(add(2, 3), 5);
}

#[test]
fn moved_instructions_keep_rip_relative_data_and_branches() {
    static LOAD: AtomicUsize = AtomicUsize::new(0);
    static BRANCH: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn load_plus_one() -> i64 {
        function::<Nullary>(LOAD.load(Ordering::Acquire))() + 1
    }
    extern "C" fn branch_times_ten(value: i64) -> i64 {
        function::<Unary>(BRANCH.load(Ordering::Acquire))(value) * 10
    }

    // mov rax, [rip+0x19]; ret; data 0x1234 at offset 0x20.
    let mut load_code = vec![0xCC; 0x28];
    load_code[..8].copy_from_slice(&[0x48, 0x8B, 0x05, 0x19, 0x00, 0x00, 0x00, 0xC3]);
    load_code[0x20..].copy_from_slice(&0x1234_u64.to_le_bytes());
    let load = executable(&load_code);
    // test ecx, ecx; je +3; xor eax, eax; ret; mov eax, 7; ret
    let branch = executable(&[
        0x85, 0xC9, 0x74, 0x03, 0x31, 0xC0, 0xC3, 0xB8, 0x07, 0x00, 0x00, 0x00, 0xC3,
    ]);

    // SAFETY: whole functions with matching replacements.
    let mut load_detour =
        unsafe { Detour::new(load, load_plus_one as Nullary as usize, None) }.unwrap();
    // SAFETY: as above.
    let mut branch_detour =
        unsafe { Detour::new(branch, branch_times_ten as Unary as usize, None) }.unwrap();
    LOAD.store(load_detour.trampoline(), Ordering::Release);
    BRANCH.store(branch_detour.trampoline(), Ordering::Release);
    // SAFETY: only this test changes these functions.
    unsafe {
        load_detour.enable().unwrap();
        branch_detour.enable().unwrap();
    }
    assert_eq!(function::<Nullary>(load)(), 0x1235);
    assert_eq!(function::<Unary>(branch)(0), 70);
    assert_eq!(function::<Unary>(branch)(1), 0);
}

#[test]
fn detours_chain_and_unwind_in_order() {
    static FIRST: AtomicUsize = AtomicUsize::new(0);
    static SECOND: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn plus_ten(a: i64, b: i64) -> i64 {
        function::<Add>(FIRST.load(Ordering::Acquire))(a, b) + 10
    }
    extern "C" fn times_two(a: i64, b: i64) -> i64 {
        function::<Add>(SECOND.load(Ordering::Acquire))(a, b) * 2
    }

    let target = executable(&ADD);
    let add = function::<Add>(target);
    // SAFETY: whole function, matching replacements; only this test changes it.
    unsafe {
        let mut first = Detour::new(target, plus_ten as Add as usize, None).unwrap();
        FIRST.store(first.trampoline(), Ordering::Release);
        first.enable().unwrap();
        // The second detour moves the first one's jump into its trampoline.
        let mut second = Detour::new(target, times_two as Add as usize, None).unwrap();
        SECOND.store(second.trampoline(), Ordering::Release);
        second.enable().unwrap();
        assert_eq!(add(1, 2), (3 + 10) * 2);

        // The first cannot be removed from under the second.
        assert_eq!(first.disable(), Err(HookError::Changed(target)));
        assert!(first.is_enabled());

        second.disable().unwrap();
        assert_eq!(add(1, 2), 13);
        first.disable().unwrap();
        assert_eq!(add(1, 2), 3);
    }
}

#[test]
fn refuses_what_it_cannot_install_safely() {
    extern "C" fn nothing() -> i64 {
        0
    }
    let replacement = nothing as Nullary as usize;

    // 0: xor eax, eax; 2: inc eax; 4: cmp eax, 10; 7: jl 2; 9: ret
    let looping = [0x31, 0xC0, 0xFF, 0xC0, 0x83, 0xF8, 0x0A, 0x7C, 0xF9, 0xC3];
    let target = executable(&looping);
    // SAFETY: `target` is a whole function of `looping.len()` bytes.
    let error = unsafe { Detour::new(target, replacement, Some(looping.len())) }.unwrap_err();
    assert_eq!(
        error,
        HookError::BranchIntoPatch {
            from: target as u64 + 7,
            to: target as u64 + 2
        }
    );

    let short = executable(&[0xC3, 0x90, 0x90, 0x90, 0x90, 0x90]);
    // SAFETY: as above.
    let error = unsafe { Detour::new(short, replacement, None) }.unwrap_err();
    assert_eq!(error, HookError::Relocate(RelocateError::TooShort(0)));

    // A detour prepared before another one was installed refuses to enable.
    let target = executable(&ADD);
    // SAFETY: whole function; only this test changes it.
    unsafe {
        let mut stale = Detour::new(target, replacement, None).unwrap();
        let mut other = Detour::new(target, replacement, None).unwrap();
        other.enable().unwrap();
        assert_eq!(stale.enable(), Err(HookError::Changed(target)));
        other.disable().unwrap();
    }
}

#[test]
fn a_thread_inside_the_overwritten_bytes_continues_in_the_trampoline() {
    extern "C" fn answer() -> i64 {
        42
    }

    // 0: pause; 2: cmp dword [rip+0x17], 0; 9: jne 0; 11: mov eax, 0; 16: ret
    // The flag at 0x20 is 1, so the loop spins inside the first 9 bytes and
    // leaves only through the detour, when `jne 0` reaches the jump.
    let mut code = vec![0xCC; 0x24];
    code[..17].copy_from_slice(&[
        0xF3, 0x90, 0x83, 0x3D, 0x17, 0x00, 0x00, 0x00, 0x00, 0x75, 0xF5, 0xB8, 0x00, 0x00, 0x00,
        0x00, 0xC3,
    ]);
    code[0x20..].copy_from_slice(&1_u32.to_le_bytes());

    for _ in 0..50 {
        let target = executable(&code);
        let spinning = std::thread::spawn(move || function::<Nullary>(target)());
        std::thread::sleep(std::time::Duration::from_millis(1));
        // SAFETY: whole function, matching replacement; only this test
        // changes it. A thread that stops inside the first 9 bytes must be
        // moved to the trampoline, or it would run the jump's bytes from the
        // middle.
        let mut detour = unsafe { Detour::new(target, answer as Nullary as usize, None) }.unwrap();
        // SAFETY: as above.
        unsafe { detour.enable() }.unwrap();
        assert_eq!(spinning.join().unwrap(), 42);
        std::mem::forget(detour);
    }
}

#[test]
fn a_code_patch_replaces_and_restores_bytes() {
    // mov eax, 1; ret
    let target = executable(&[0xB8, 0x01, 0x00, 0x00, 0x00, 0xC3]);
    let one = function::<Nullary>(target);
    // SAFETY: the bytes are mapped code of this test.
    unsafe {
        assert_eq!(
            CodePatch::new(target + 1, &[0x02], &[0x03]).unwrap_err(),
            HookError::Mismatch(target + 1)
        );
        assert_eq!(
            CodePatch::new(target + 1, &[0x01], &[0x02, 0x00]).unwrap_err(),
            HookError::LengthMismatch
        );
        let mut patch = CodePatch::new(target + 1, &[0x01], &[0x02]).unwrap();
        patch.apply().unwrap();
        assert!(patch.is_applied());
        assert_eq!(one(), 2);
        patch.revert().unwrap();
        assert_eq!(one(), 1);
    }
}

#[test]
fn a_vtable_slot_points_at_the_replacement_while_enabled() {
    extern "C" fn first() -> i64 {
        1
    }
    extern "C" fn second() -> i64 {
        2
    }
    extern "C" fn replacement() -> i64 {
        3
    }
    let table: &'static mut [usize; 2] = Box::leak(Box::new([
        first as Nullary as usize,
        second as Nullary as usize,
    ]));
    let slot = std::ptr::from_ref(&table[1]) as usize;
    let call = || function::<Nullary>(table[1])();

    // SAFETY: an aligned slot in leaked memory; the replacement matches.
    let mut replaced = unsafe { VtableSlot::new(slot, replacement as Nullary as usize) };
    assert_eq!(replaced.original(), second as Nullary as usize);
    // SAFETY: only this test uses the table.
    unsafe {
        replaced.enable().unwrap();
        assert_eq!(call(), 3);
        replaced.disable().unwrap();
        assert_eq!(call(), 2);
    }
}
