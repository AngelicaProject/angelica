//! The hooks of decision points (Lens): a hook at the start of an event's
//! function asks the plugins' rules, applied by the host, whether the
//! function returns at once; when one holds, the function returns its
//! outcome's value without running, otherwise it runs unchanged. No plugin
//! code runs in the hook.
use super::game::ProcessMemory;
use super::watch::call_of;
use crate::catalog::Catalog;
use crate::lens::Lens;
use angelica_hook::{Context, MidHook};
use angelica_map::{EntryKind, Value};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/// The rules the hooks ask.
static LENS: OnceLock<Arc<Lens>> = OnceLock::new();
/// The points hooked so far, by point id.
static HOOKS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Hooks the start of `entry`, a `function` of the map at `address`, for
/// the point `point` of `lens`'s decisions. Once per point; the hook stays
/// until the process exits.
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
        (EntryKind::Function { .. }, Value::Address(_))
    ) {
        return Err(format!("map entry {entry} is not a function"));
    }
    let _ = LENS.set(lens.clone());
    let base = address - u64::from(resolved.value.get());
    #[allow(clippy::cast_possible_truncation)] // the game is a 64-bit process
    let function = resolved.function_range.as_ref().map(|range| {
        (base + u64::from(range.start)) as usize..(base + u64::from(range.end)) as usize
    });
    let start = usize::try_from(address).map_err(|e| e.to_string())?;
    // SAFETY: the map verified `start` as a function's first instruction in
    // the executable's code, where the stack pointer points to the return
    // address; `function` is its code from the exception directory, and the
    // handler catches every panic.
    let mut hook = unsafe { MidHook::returning(start, decide, point, function) }
        .map_err(|e| format!("decision hook: {e}"))?;
    // SAFETY: the handler only reads the registers and the game's memory
    // through the checked copy and sets rax; nothing else in this process
    // changes these bytes meanwhile.
    unsafe { hook.enable() }.map_err(|e| format!("decision hook: {e}"))?;
    std::mem::forget(hook);
    hooked.push(point);
    Ok(())
}

extern "C" fn decide(context: &mut Context, point: usize) -> bool {
    super::guard(false, || {
        let Some(lens) = LENS.get() else {
            return false;
        };
        match lens
            .decisions
            .decide(point, &call_of(context), &ProcessMemory)
        {
            Some(returns) => {
                context.rax = returns;
                true
            }
            None => false,
        }
    })
}
