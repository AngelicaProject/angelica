//! Executable memory near a target, and writes to code.

use std::sync::Mutex;

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_FREE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READWRITE,
    PAGE_PROTECTION_FLAGS, VirtualAlloc, VirtualProtect, VirtualQuery,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::HookError;

/// The size of each block of executable memory, the Windows allocation
/// granularity.
const BLOCK: usize = 0x1_0000;
/// The farthest a block may be from a target so that a `rel32` operand in
/// the target reaches anything in the block.
const REACH: usize = 0x7FF0_0000;

#[derive(Debug)]
struct Block {
    base: usize,
    used: usize,
}

static BLOCKS: Mutex<Vec<Block>> = Mutex::new(Vec::new());

/// `len` bytes of readable, writable, executable memory, 16-byte aligned,
/// within reach of a `rel32` operand at `near`. The memory is never freed.
pub(crate) fn allocate_near(near: usize, len: usize) -> Result<usize, HookError> {
    let len = len.next_multiple_of(16);
    assert!(len <= BLOCK, "allocations are smaller than a block");
    let mut blocks = BLOCKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(block) = blocks
        .iter_mut()
        .find(|block| block.base.abs_diff(near) < REACH && block.used + len <= BLOCK)
    {
        let address = block.base + block.used;
        block.used += len;
        return Ok(address);
    }
    let base = allocate_block_near(near).ok_or(HookError::NoNearMemory(near))?;
    blocks.push(Block { base, used: len });
    Ok(base)
}

/// A new block in the first free region found going outward from `near`.
pub(crate) fn allocate_block_near(near: usize) -> Option<usize> {
    let start = near - near % BLOCK;
    for step in 1..REACH / BLOCK {
        for candidate in [
            start.checked_sub(step * BLOCK),
            start.checked_add(step * BLOCK),
        ] {
            let Some(candidate) = candidate else { continue };
            if candidate == 0 || !is_free(candidate) {
                continue;
            }
            // SAFETY: allocating new memory at a free address has no
            // preconditions; a null result is handled.
            let base = unsafe {
                VirtualAlloc(
                    candidate as *const _,
                    BLOCK,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if !base.is_null() {
                return Some(base as usize);
            }
        }
    }
    None
}

fn is_free(address: usize) -> bool {
    let mut info = MEMORY_BASIC_INFORMATION {
        BaseAddress: std::ptr::null_mut(),
        AllocationBase: std::ptr::null_mut(),
        AllocationProtect: 0,
        PartitionId: 0,
        RegionSize: 0,
        State: 0,
        Protect: 0,
        Type: 0,
    };
    // SAFETY: VirtualQuery only reads the address space and writes `info`,
    // whose size is passed.
    let written = unsafe {
        VirtualQuery(
            address as *const _,
            &raw mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    written != 0 && info.State == MEM_FREE && info.RegionSize >= BLOCK
}

/// Copies `bytes` to `address`, making the memory writable for the copy and
/// flushing the instruction cache.
///
/// # Safety
///
/// `address..address + bytes.len()` is mapped memory of this process, and no
/// thread executes or reads it while it changes: the caller has suspended
/// every other thread that could, or the bytes are not yet reachable.
pub(crate) unsafe fn write(address: usize, bytes: &[u8]) -> Result<(), HookError> {
    let mut old: PAGE_PROTECTION_FLAGS = 0;
    // SAFETY: the caller guarantees the range is mapped; VirtualProtect
    // writes the previous protection to `old`.
    if unsafe {
        VirtualProtect(
            address as *const _,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &raw mut old,
        )
    } == 0
    {
        return Err(last_error("VirtualProtect"));
    }
    // SAFETY: the range is mapped and now writable, and the caller
    // guarantees nothing else uses it during the copy. `bytes` is a separate
    // allocation, so the ranges do not overlap.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len()) };
    let mut ignored: PAGE_PROTECTION_FLAGS = 0;
    // SAFETY: restores the protection the range had; same range as above.
    unsafe { VirtualProtect(address as *const _, bytes.len(), old, &raw mut ignored) };
    // SAFETY: flushing the instruction cache of this process for a mapped
    // range has no other preconditions.
    unsafe { FlushInstructionCache(GetCurrentProcess(), address as *const _, bytes.len()) };
    Ok(())
}

/// Reads `len` bytes at `address`.
///
/// # Safety
///
/// `address..address + len` is mapped, readable memory of this process.
pub(crate) unsafe fn read(address: usize, len: usize) -> Vec<u8> {
    // SAFETY: the caller guarantees the range is readable.
    unsafe { std::slice::from_raw_parts(address as *const u8, len) }.to_vec()
}

/// The number of readable bytes from `address` to the end of its region,
/// up to `limit`.
pub(crate) fn readable_len(address: usize, limit: usize) -> usize {
    let mut info = MEMORY_BASIC_INFORMATION {
        BaseAddress: std::ptr::null_mut(),
        AllocationBase: std::ptr::null_mut(),
        AllocationProtect: 0,
        PartitionId: 0,
        RegionSize: 0,
        State: 0,
        Protect: 0,
        Type: 0,
    };
    // SAFETY: VirtualQuery only reads the address space and writes `info`.
    let written = unsafe {
        VirtualQuery(
            address as *const _,
            &raw mut info,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0 || info.State != MEM_COMMIT {
        return 0;
    }
    let end = info.BaseAddress as usize + info.RegionSize;
    (end - address).min(limit)
}

pub(crate) fn last_error(call: &'static str) -> HookError {
    HookError::Os {
        call,
        code: std::io::Error::last_os_error()
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
            .unwrap_or(0),
    }
}
