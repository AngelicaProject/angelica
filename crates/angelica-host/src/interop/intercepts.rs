//! The hooks of plugins' intercepts: a hook at the first instruction of
//! each intercepted function hands the call's arguments, from registers
//! and the stack, to [`Intercepts::call`]. When the function has `after`
//! handlers or a `before` handler skips it, the hook returns what that
//! decided, having called the game's code itself; otherwise the game's
//! code runs as usual with the arguments the `before` handlers left. No
//! plugin code runs here; the handlers run in the sandbox.
use crate::intercepts::{Arg, Code, Intercepts, MAX_ARGS, REGISTER_ARGS, Returned};
use angelica_hook::{Context, MidHook};
use std::arch::asm;
use std::sync::{Arc, OnceLock};

static INTERCEPTS: OnceLock<Arc<Intercepts>> = OnceLock::new();

/// What a hook is made with: the function's address, by which handlers
/// are found, and where its code runs without the hook.
struct Hooked {
    target: u64,
    original: u64,
}

/// The function's stack arguments start past the return address and 32
/// bytes of shadow space.
const STACK_ARGS: u64 = 8 + 32;

/// Runs the handlers of the function whose [`Hooked`] is at `data`.
extern "C" fn entry(context: &mut Context, data: usize) -> bool {
    super::guard(false, || {
        let Some(intercepts) = INTERCEPTS.get() else {
            return false;
        };
        // SAFETY: `data` is the address of a `Hooked` that `install` leaked,
        // so it lives as long as the process.
        let hooked = unsafe { &*(data as *const Hooked) };
        let Some(arity) = intercepts.arity(hooked.target) else {
            return false;
        };
        let registers = [context.rcx, context.rdx, context.r8, context.r9];
        let stack = context.rsp() + STACK_ARGS;
        let mut args: Vec<Arg> = (0..arity)
            .map(|index| match registers.get(index) {
                Some(&int) => Arg {
                    int,
                    float: context.xmm[index][0],
                },
                // SAFETY: the function's type in the plugin's entries, whose
                // checks held in this build, declares this many arguments;
                // the caller put the ones past the fourth in these slots of
                // its frame, above the return address and the shadow space.
                None => Arg::both(unsafe {
                    ((stack + 8 * (index - REGISTER_ARGS) as u64) as *const u64).read()
                }),
            })
            .collect();
        let decided = intercepts.call(hooked.target, &mut args, |args| {
            call_original(hooked.original, args)
        });
        if let Some(returned) = decided {
            context.rax = returned.integer;
            context.xmm[0][0] = returned.float;
            return true;
        }
        for (index, arg) in args.iter().enumerate() {
            match index {
                0 => context.rcx = arg.int,
                1 => context.rdx = arg.int,
                2 => context.r8 = arg.int,
                3 => context.r9 = arg.int,
                _ => {
                    // SAFETY: as for the read above; a function owns the
                    // slots of its stack arguments.
                    unsafe {
                        ((stack + 8 * (index - REGISTER_ARGS) as u64) as *mut u64).write(arg.int);
                    }
                }
            }
            if index < REGISTER_ARGS {
                context.xmm[index][0] = arg.float;
            }
        }
        false
    })
}

/// Calls the game's code of a hooked function at `original` with `args`.
fn call_original(original: u64, args: &[Arg]) -> Returned {
    let mut ints = [0u64; MAX_ARGS];
    let mut floats = [0u64; REGISTER_ARGS];
    for (index, arg) in args.iter().enumerate() {
        ints[index] = arg.int;
        if let Some(float) = floats.get_mut(index) {
            *float = arg.float;
        }
    }
    let integer: u64;
    let float: f64;
    // SAFETY: `original` is the hook's copy of the function's first
    // instructions followed by a jump to the rest, so this calls the game's
    // function as its caller did, with the arguments its entry declares
    // (extra slots are zeros it never reads). The stack is 16-byte aligned
    // on entry to the block, and 0x80 bytes keep it so: 32 bytes of shadow
    // space and 12 stack slots.
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
            "movq xmm0, [r14]",
            "movq xmm1, [r14 + 0x8]",
            "movq xmm2, [r14 + 0x10]",
            "movq xmm3, [r14 + 0x18]",
            "call r13",
            "add rsp, 0x80",
            in("r12") ints.as_ptr(),
            in("r13") original,
            in("r14") floats.as_ptr(),
            out("rax") integer,
            out("xmm0") float,
            clobber_abi("system"),
        );
    }
    Returned {
        integer,
        float: float.to_bits(),
    }
}

/// Hooks the function at `address`, whose code is `code`, so that its
/// handlers in `intercepts` run on each call.
pub fn install(address: u64, code: &Code, intercepts: &Arc<Intercepts>) -> Result<(), String> {
    let _ = INTERCEPTS.set(intercepts.clone());
    let to_usize = |value: u64| usize::try_from(value).map_err(|e| e.to_string());
    let function = angelica_hook::FunctionCode {
        start: to_usize(code.start)?,
        ranges: code
            .ranges
            .iter()
            .map(|range| Ok(to_usize(range.start)?..to_usize(range.end)?))
            .collect::<Result<_, String>>()?,
        prologue: to_usize(code.prologue)?,
    };
    let hooked: &'static mut Hooked = Box::leak(Box::new(Hooked {
        target: address,
        original: 0,
    }));
    // SAFETY: `address` is the first instruction of a function that the
    // plugin's entries describe and whose checks held in this executable,
    // where the stack pointer points to the return address; `function` is
    // its code from the exception directory or its control flow; `entry`
    // catches panics, and its data is the leaked `Hooked`.
    let mut hook = unsafe {
        MidHook::returning(
            to_usize(address)?,
            entry,
            std::ptr::from_mut(hooked) as usize,
            &function,
        )
    }
    .map_err(|e| format!("intercept hook: {e}"))?;
    hooked.original = hook.original() as u64;
    // SAFETY: the hook's data is complete; nothing else in this process
    // changes these bytes meanwhile.
    unsafe { hook.enable() }.map_err(|e| format!("intercept hook: {e}"))?;
    std::mem::forget(hook);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hook_runs_the_function_without_handlers_and_through_its_original() {
        // `lea rax, [rcx+rdx]; nop; nop; nop; ret`: returns the sum of its
        // two arguments.
        let code = [0x48, 0x8D, 0x04, 0x11, 0x90, 0x90, 0x90, 0xC3];
        let function = crate::interop::entries::Function::synthetic(&code);
        let start = function.address as u64;
        let ranges = function.code.as_ref().unwrap();
        let code = Code {
            start,
            ranges: ranges
                .ranges
                .iter()
                .map(|range| range.start as u64..range.end as u64)
                .collect(),
            prologue: 0,
        };
        let intercepts = Arc::new(Intercepts::default());
        install(start, &code, &intercepts).unwrap();
        // SAFETY: the hooked synthetic function has this ABI.
        let sum = unsafe {
            std::mem::transmute::<usize, unsafe extern "system" fn(u64, u64) -> u64>(
                function.address,
            )(2, 40)
        };
        assert_eq!(sum, 42, "the game's code runs, without handlers too");
    }

    extern "system" fn mixed(first: u64, half: f64, third: u64, fourth: f32, fifth: u64) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        let whole = (first + third + fifth) as f64;
        whole + half + f64::from(fourth)
    }

    #[test]
    fn the_original_gets_integers_floats_and_stack_arguments() {
        let args = [
            Arg::both(1),
            Arg {
                int: 0,
                float: 0.5f64.to_bits(),
            },
            Arg::both(10),
            Arg {
                int: 0,
                float: u64::from(0.25f32.to_bits()),
            },
            Arg::both(100),
        ];
        let returned = call_original(mixed as *const () as usize as u64, &args);
        assert_eq!(returned.float, 111.75f64.to_bits());
    }
}
