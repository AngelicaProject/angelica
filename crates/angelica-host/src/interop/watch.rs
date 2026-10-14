//! The hooks of watched events (Lens): a mid-function hook at the code of
//! an event's map entry hands the registers to the plugins' watches, which
//! read their rows through the checked memory copy. The hook changes no
//! register, and the game's code runs on unchanged.
use super::game::ProcessMemory;
use crate::catalog::Catalog;
use crate::lens::{Call, Lens};
use angelica_hook::{Context, MidHook};
use angelica_map::{EntryKind, Value};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// The watches the hooks fire.
static LENS: OnceLock<Arc<Lens>> = OnceLock::new();
/// The points hooked so far, by point id.
static HOOKS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Hooks the code of `entry`, a `function` or `location` of the map at
/// `address`, for the point `point` of `lens`'s watches. Once per point; the
/// hook stays until the process exits.
pub fn install(
    catalog: &Catalog,
    lens: &Arc<Lens>,
    point: usize,
    entry: &str,
    address: u64,
) -> Result<(), String> {
    let mut hooked = HOOKS.lock().unwrap_or_else(PoisonError::into_inner);
    if hooked.contains(&point) {
        return Ok(());
    }
    let resolved = catalog
        .present
        .get(entry)
        .ok_or_else(|| format!("map entry {entry} is absent"))?;
    if !matches!(
        (&resolved.entry.kind, resolved.value),
        (
            EntryKind::Function { .. } | EntryKind::Location { .. },
            Value::Address(_)
        )
    ) {
        return Err(format!("map entry {entry} is not code"));
    }
    let _ = LENS.set(lens.clone());
    let base = address - u64::from(resolved.value.get());
    #[allow(clippy::cast_possible_truncation)] // the game is a 64-bit process
    let function = resolved.function_range.as_ref().map(|range| {
        (base + u64::from(range.start)) as usize..(base + u64::from(range.end)) as usize
    });
    let location = usize::try_from(address).map_err(|e| e.to_string())?;
    // SAFETY: the map verified `location` as the start of an instruction in
    // the executable's code (a function's first instruction, or a location
    // the map names), `function` is its function's code from the exception
    // directory, and the handler catches every panic.
    let mut hook = unsafe { MidHook::with_data(location, fire, point, function) }
        .map_err(|e| format!("watch hook: {e}"))?;
    // SAFETY: the handler only reads the registers and the game's memory
    // through the checked copy; nothing else in this process changes these
    // bytes meanwhile.
    unsafe { hook.enable() }.map_err(|e| format!("watch hook: {e}"))?;
    std::mem::forget(hook);
    hooked.push(point);
    Ok(())
}

extern "C" fn fire(context: &mut Context, point: usize) {
    super::guard((), || {
        if let Some(lens) = LENS.get() {
            let call = Call {
                general: [
                    context.rax,
                    context.rbx,
                    context.rcx,
                    context.rdx,
                    context.rsi,
                    context.rdi,
                    context.rbp,
                    context.r8,
                    context.r9,
                    context.r10,
                    context.r11,
                    context.r12,
                    context.r13,
                    context.r14,
                    context.r15,
                ],
                xmm: context.xmm.map(|xmm| xmm[0]),
                rsp: context.rsp(),
            };
            lens.watches.fire(point, &call, &ProcessMemory);
        }
    });
}
