use std::{ffi::CStr, io};
use windows_sys::Win32::{
    Foundation::FreeLibrary,
    System::{
        LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        SystemInformation::GetSystemDirectoryW,
    },
};

pub(super) type Export = unsafe extern "system" fn() -> isize;

/// Resolves an export by absolute system path even if a same-named DLL is
/// already loaded. Callers cache successful exports for the process lifetime.
pub(super) fn export(dll: &str, name: &CStr) -> Result<Export, String> {
    let mut path = vec![0u16; 32768];
    // SAFETY: The buffer has room for the advertised number of UTF-16 units.
    let len = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), 32768) } as usize;
    if len == 0 || len >= path.len() {
        return Err("cannot resolve the Windows system directory".into());
    }
    path.truncate(len);
    path.push(u16::from(b'\\'));
    path.extend(dll.encode_utf16());
    path.push(0);
    // SAFETY: The path is absolute and NUL-terminated. All callers supply fixed system names.
    let module = unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(format!(
            "loading system {dll}: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: The module is live and the export name is NUL-terminated.
    if let Some(address) = unsafe { GetProcAddress(module, name.as_ptr().cast()) } {
        return Ok(address);
    }
    let error = io::Error::last_os_error();
    // SAFETY: No export escaped, so release the reference acquired by this call.
    unsafe { FreeLibrary(module) };
    Err(format!(
        "resolving {} in system {dll}: {error}",
        name.to_string_lossy()
    ))
}
