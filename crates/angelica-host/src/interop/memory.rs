use sha2::Digest;
use windows_sys::Win32::{
    Foundation::HINSTANCE,
    System::{
        Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory},
        LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
        Memory::{
            MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE,
            VirtualQuery,
        },
        Threading::GetCurrentProcess,
    },
};

/// # Errors
/// Returns an error when Windows cannot copy the complete memory range.
pub fn read(address: usize, len: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; len];
    let mut read = 0;
    // SAFETY: ReadProcessMemory validates the source range and writes only the allocated output.
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const _,
            bytes.as_mut_ptr().cast(),
            len,
            &raw mut read,
        )
    };
    if ok == 0 || read != len {
        return Err(format!(
            "game memory read failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(bytes)
}

/// Writes `bytes` at `address` when the whole range is committed, writable
/// data of this process; code and read-only memory are never written.
///
/// # Errors
/// Why the range is not written.
pub fn write(address: usize, bytes: &[u8]) -> Result<(), String> {
    let end = address
        .checked_add(bytes.len())
        .ok_or("the range wraps around")?;
    let mut at = address;
    while at < end {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: VirtualQuery only describes the region containing `at`
        // into the local structure.
        let written = unsafe {
            VirtualQuery(
                at as *const _,
                &raw mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if written == 0 || info.State != MEM_COMMIT || info.Protect != PAGE_READWRITE {
            return Err("not writable data".into());
        }
        at = (info.BaseAddress as usize).saturating_add(info.RegionSize);
    }
    let mut copied = 0;
    // SAFETY: the whole destination range is committed read-write data of
    // this process, checked above, so WriteProcessMemory changes no page's
    // protection; it reads only `bytes`.
    let ok = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as *const _,
            bytes.as_ptr().cast(),
            bytes.len(),
            &raw mut copied,
        )
    };
    if ok == 0 || copied != bytes.len() {
        return Err(format!(
            "game memory write failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// # Errors
/// Returns an error for an absent module or a truncated path.
pub fn module_path(module: usize) -> Result<std::path::PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32768];
    // SAFETY: Windows module and output capacity are valid.
    let len =
        unsafe { GetModuleFileNameW(module as HINSTANCE, buffer.as_mut_ptr(), 32768) } as usize;
    if len == 0 || len >= buffer.len() {
        return Err("GetModuleFileNameW failed".into());
    }
    Ok(std::ffi::OsString::from_wide(&buffer[..len]).into())
}

/// # Errors
/// Rejects invalid PE files and inaccessible or excessive executable mappings.
pub fn executable_snapshot() -> Result<(usize, Vec<u8>, String), String> {
    // SAFETY: NULL requests the process executable.
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    if base == 0 {
        return Err("executable module is unavailable".into());
    }
    let file = std::fs::read(module_path(base)?).map_err(|e| e.to_string())?;
    let image =
        angelica_pe::Image::parse(&file, angelica_pe::Layout::File).map_err(|e| e.to_string())?;
    let hash = crate::package::hex(&sha2::Sha256::digest(&file));
    let size = image.size_of_image() as usize;
    if size > 512 * 1024 * 1024 {
        return Err("executable image exceeds snapshot limit".into());
    }
    let mut snapshot = vec![0u8; size];
    let mut offset = 0;
    while offset < size {
        let address = base.checked_add(offset).ok_or("module address overflow")?;
        // SAFETY: zero is a valid initial value for the output-only Windows structure.
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: queries mapped pages and writes a correctly sized structure.
        if unsafe {
            VirtualQuery(
                address as *const _,
                &raw mut info,
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        } == 0
        {
            return Err("VirtualQuery failed for executable".into());
        }
        let region_end = (info.BaseAddress as usize)
            .checked_add(info.RegionSize)
            .ok_or("region size overflow")?;
        let len = (region_end - address).min(size - offset);
        if len == 0 {
            return Err("empty executable memory region".into());
        }
        if info.State == MEM_COMMIT && info.Protect & (PAGE_GUARD | PAGE_NOACCESS) == 0 {
            let bytes = read(address, len)?;
            snapshot[offset..offset + len].copy_from_slice(&bytes);
        }
        offset += len;
    }
    Ok((base, snapshot, hash))
}
