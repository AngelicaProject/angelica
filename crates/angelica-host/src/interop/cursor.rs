//! The game's calls of `SetCursor` through its import: the game sets its
//! cursor every frame, so while Angelica shows its own cursor over its
//! interface the game's call keeps Angelica's instead
//! ([hooks](../../../../docs/architecture/hooks.md#import-slots)).
use angelica_hook::VtableSlot;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows_sys::Win32::{
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    UI::WindowsAndMessaging::{GetCursor, HCURSOR},
};

/// The function the slot held: `user32!SetCursor`, or another tool's
/// replacement of it, called for every call the game makes while Angelica
/// does not show its cursor.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// Points the game's import slot of `USER32.dll!SetCursor` at Angelica's
/// replacement. The slot is found by name in the executable's import
/// directory, read from the executable's file.
///
/// # Errors
/// Says why the slot was not replaced: the executable does not import
/// `SetCursor` by name exactly once, or the slot does not hold a function.
pub fn install() -> Result<VtableSlot, String> {
    // SAFETY: a null name asks for the process's executable module.
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    if base == 0 {
        return Err("the executable module is unavailable".into());
    }
    let file = std::fs::read(super::memory::module_path(base)?).map_err(|e| e.to_string())?;
    let image =
        angelica_pe::Image::parse(&file, angelica_pe::Layout::File).map_err(|e| e.to_string())?;
    let rva = match image.import_slots("USER32.dll", "SetCursor").as_slice() {
        [rva] => *rva,
        [] => return Err("the game does not import SetCursor by name".into()),
        slots => return Err(format!("the game imports SetCursor {} times", slots.len())),
    };
    if rva % 8 != 0 || rva >= image.size_of_image() {
        return Err(format!(
            "the SetCursor import slot at {rva:#x} is not a slot"
        ));
    }
    let slot = base + rva as usize;
    let current = super::memory::read(slot, 8)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .map(usize::from_le_bytes)
        .ok_or("the SetCursor import slot cannot be read")?;
    if current == 0 {
        return Err("the SetCursor import slot holds no function".into());
    }
    if current != user32_set_cursor() {
        super::log(&format!(
            "the game's SetCursor import is already replaced, by {current:#x}; Angelica calls through it"
        ));
    }
    // SAFETY: the slot is an 8-byte aligned import address table entry of
    // the executable, mapped for the process's life, and `set_cursor` has
    // the signature and calling convention of `SetCursor`.
    let mut hook = unsafe { VtableSlot::new(slot, set_cursor as *const () as usize) };
    ORIGINAL.store(hook.original(), Ordering::Release);
    // SAFETY: the original is published before the replacement can run.
    unsafe { hook.enable() }.map_err(|e| e.to_string())?;
    Ok(hook)
}

/// `SetCursor` of `user32.dll` as loaded, for the log.
fn user32_set_cursor() -> usize {
    let name: Vec<u16> = "user32.dll\0".encode_utf16().collect();
    // SAFETY: a NUL-terminated module name and a NUL-terminated ASCII
    // function name; both calls only read them.
    unsafe {
        let module = GetModuleHandleW(name.as_ptr());
        if module.is_null() {
            return 0;
        }
        GetProcAddress(module, c"SetCursor".as_ptr().cast()).map_or(0, |f| f as usize)
    }
}

unsafe extern "system" fn set_cursor(cursor: HCURSOR) -> HCURSOR {
    let ours = super::guard(false, || {
        super::overlay::OVERLAY
            .get()
            .is_some_and(|overlay| overlay.input.keeps_cursor())
    });
    if ours {
        // SAFETY: reads the calling thread's cursor without preconditions.
        return unsafe { GetCursor() };
    }
    let original = ORIGINAL.load(Ordering::Acquire);
    // SAFETY: the original is the slot's previous function, which has the
    // signature of `SetCursor` and stays mapped.
    let original: unsafe extern "system" fn(HCURSOR) -> HCURSOR =
        unsafe { std::mem::transmute(original) };
    // SAFETY: the game's argument passes on unchanged.
    unsafe { original(cursor) }
}
