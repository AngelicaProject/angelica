//! The Windows clipboard, for text the player pastes into the overlay or
//! copies from it.
use windows_sys::Win32::{
    Foundation::{GlobalFree, HWND},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
};

/// `CF_UNICODETEXT` from the Windows SDK.
const UNICODE_TEXT: u32 = 13;
/// The most text pasted at once, in UTF-16 units.
const MAX_UNITS: usize = 64 * 1024;

/// The clipboard's text, at most [`MAX_UNITS`] long; `None` without text.
pub fn text(window: HWND) -> Option<String> {
    // SAFETY: OpenClipboard takes any window handle or null; it is closed below.
    if unsafe { OpenClipboard(window) } == 0 {
        return None;
    }
    // SAFETY: the clipboard is open; the handle stays valid until it is closed.
    let handle = unsafe { GetClipboardData(UNICODE_TEXT) };
    let text = (!handle.is_null()).then(|| read(handle)).flatten();
    // SAFETY: this thread opened the clipboard above.
    unsafe { CloseClipboard() };
    text
}

/// The UTF-16 text in a global memory block, up to its terminating zero.
fn read(handle: *mut core::ffi::c_void) -> Option<String> {
    // SAFETY: handle is the clipboard's global memory block of text.
    let units = unsafe { GlobalSize(handle) } / 2;
    // SAFETY: as above; the lock is released below.
    let data = unsafe { GlobalLock(handle) }.cast::<u16>();
    if data.is_null() {
        return None;
    }
    // SAFETY: the locked block holds `units` UTF-16 units.
    let slice = unsafe { std::slice::from_raw_parts(data, units.min(MAX_UNITS)) };
    let end = slice
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(slice.len());
    let text = String::from_utf16_lossy(&slice[..end]);
    // SAFETY: the block was locked above.
    unsafe { GlobalUnlock(handle) };
    Some(text)
}

/// Puts `text` on the clipboard; whether it did.
pub fn set_text(window: HWND, text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = units.len() * 2;
    // SAFETY: allocating a movable block has no preconditions; it is freed
    // below unless the clipboard takes it.
    let block = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if block.is_null() {
        return false;
    }
    // SAFETY: block is the movable block allocated above.
    let data = unsafe { GlobalLock(block) }.cast::<u16>();
    if data.is_null() {
        // SAFETY: the clipboard never took the block.
        unsafe { GlobalFree(block) };
        return false;
    }
    // SAFETY: the locked block has room for exactly `units.len()` units.
    unsafe {
        std::ptr::copy_nonoverlapping(units.as_ptr(), data, units.len());
        GlobalUnlock(block);
    }
    // SAFETY: OpenClipboard takes any window handle; it is closed below.
    let taken = unsafe { OpenClipboard(window) } != 0
        // SAFETY: the clipboard is open; on success it owns the block.
        && unsafe { EmptyClipboard() } != 0
        && !unsafe { SetClipboardData(UNICODE_TEXT, block) }.is_null();
    // SAFETY: closing a clipboard this thread did not open fails harmlessly.
    unsafe { CloseClipboard() };
    if !taken {
        // SAFETY: the clipboard did not take the block.
        unsafe { GlobalFree(block) };
    }
    taken
}
