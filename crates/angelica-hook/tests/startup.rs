#![cfg(windows)]
use angelica_hook::{Detour, HookError, StartupDetour, testing::executable};

type Function = extern "system" fn() -> i32;
fn function(address: usize) -> Function {
    // SAFETY: tests pass executable machine code with the no-argument system ABI.
    unsafe { std::mem::transmute(address) }
}
extern "system" fn replacement() -> i32 {
    42
}
extern "system" fn later() -> i32 {
    99
}

#[test]
fn startup_restores_original_and_retains_a_callable_trampoline() {
    let target = executable(&[0xb8, 7, 0, 0, 0, 0xc3]);
    // SAFETY: private executable test function; replacement has the same ABI.
    let hook = unsafe { StartupDetour::prepare(target, replacement as Function as usize) }.unwrap();
    // SAFETY: no other thread can reach this private test function.
    unsafe { hook.enable() }.unwrap();
    assert_eq!(function(target)(), 42);
    assert_eq!(function(hook.trampoline())(), 7);
    // SAFETY: exclusive access to the test function.
    unsafe { hook.disable() }.unwrap();
    assert_eq!(function(target)(), 7);
    assert_eq!(function(hook.trampoline())(), 7);
}

#[test]
fn later_hook_is_not_overwritten_on_startup_removal() {
    let target = executable(&[0xb8, 7, 0, 0, 0, 0xc3]);
    // SAFETY: private no-argument test function, matching replacement.
    let hook = unsafe { StartupDetour::prepare(target, replacement as Function as usize) }.unwrap();
    // SAFETY: no other thread executes the test target.
    unsafe { hook.enable() }.unwrap();
    // SAFETY: same target and matching ABI; test owns all modifications.
    let mut other = unsafe { Detour::new(target, later as Function as usize, None) }.unwrap();
    // SAFETY: test owns the code and replacement is ready.
    unsafe { other.enable() }.unwrap();
    // SAFETY: test owns the code; removal must refuse the later tool's jump.
    let result = unsafe { hook.disable() };
    assert!(matches!(result, Err(HookError::Changed(_))));
    assert_eq!(function(target)(), 99);
    assert_eq!(function(hook.trampoline())(), 7);
    // SAFETY: remove in reverse order while test owns the target.
    unsafe {
        other.disable().unwrap();
        hook.disable().unwrap();
    }
    assert_eq!(function(target)(), 7);
}
