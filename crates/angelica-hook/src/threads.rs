//! Suspending the other threads of the process while code changes.

use std::sync::{Mutex, MutexGuard, PoisonError};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    CONTEXT, CONTEXT_CONTROL_AMD64, GetThreadContext, SetThreadContext,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
    THREAD_GET_CONTEXT, THREAD_SET_CONTEXT, THREAD_SUSPEND_RESUME,
};

/// One freeze at a time in the process: two threads that froze the others
/// at once could each suspend the other before resuming anyone, and both
/// would stay suspended.
static FREEZE: Mutex<()> = Mutex::new(());

/// `GetThreadContext` needs a 16-byte aligned `CONTEXT`.
#[repr(C, align(16))]
struct AlignedContext(CONTEXT);

/// Every other thread of the process, suspended until dropped.
///
/// Threads created after the list of threads is taken are not suspended.
/// Nothing allocates while threads are suspended: a suspended thread may
/// hold the heap lock.
pub(crate) struct Frozen {
    handles: Vec<HANDLE>,
    /// Released after every thread is resumed: fields drop after `drop`.
    _freeze: MutexGuard<'static, ()>,
}

impl Frozen {
    pub(crate) fn others() -> Self {
        // Taken before any thread is suspended; a thread waiting for it may
        // be suspended by the holder and goes on once resumed.
        let freeze = FREEZE.lock().unwrap_or_else(PoisonError::into_inner);
        let ids = other_thread_ids();
        let mut handles = Vec::with_capacity(ids.len());
        for id in ids {
            // SAFETY: opening a thread by id has no preconditions; a null
            // handle is skipped.
            let handle = unsafe {
                OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_SET_CONTEXT,
                    0,
                    id,
                )
            };
            if handle.is_null() {
                continue;
            }
            // SAFETY: `handle` is a thread handle opened with
            // THREAD_SUSPEND_RESUME.
            if unsafe { SuspendThread(handle) } == u32::MAX {
                // SAFETY: closes the handle opened above.
                unsafe { CloseHandle(handle) };
                continue;
            }
            // The capacity was reserved before any thread was suspended.
            handles.push(handle);
        }
        Self {
            handles,
            _freeze: freeze,
        }
    }

    /// The instruction pointer of every suspended thread.
    pub(crate) fn instruction_pointers(&self) -> impl Iterator<Item = (HANDLE, u64)> + '_ {
        self.handles.iter().filter_map(|&handle| {
            let mut context = control_context();
            // SAFETY: the thread is suspended and the handle has
            // THREAD_GET_CONTEXT; the context is aligned and has its flags set.
            (unsafe { GetThreadContext(handle, &raw mut context.0) } != 0)
                .then_some((handle, context.0.Rip))
        })
    }

    /// Moves the instruction pointer of the suspended thread `handle` to `rip`.
    pub(crate) fn set_instruction_pointer(handle: HANDLE, rip: u64) {
        let mut context = control_context();
        // SAFETY: as in `instruction_pointers`.
        if unsafe { GetThreadContext(handle, &raw mut context.0) } != 0 {
            context.0.Rip = rip;
            // SAFETY: the thread is suspended and the handle has
            // THREAD_SET_CONTEXT; only the control registers change.
            unsafe { SetThreadContext(handle, &raw const context.0) };
        }
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        for &handle in &self.handles {
            // SAFETY: each handle was suspended once in `others` and is
            // resumed and closed once here.
            unsafe {
                ResumeThread(handle);
                CloseHandle(handle);
            }
        }
    }
}

fn control_context() -> AlignedContext {
    // SAFETY: CONTEXT is plain data for which all zero bytes are valid.
    let mut context = AlignedContext(unsafe { std::mem::zeroed() });
    context.0.ContextFlags = CONTEXT_CONTROL_AMD64;
    context
}

fn other_thread_ids() -> Vec<u32> {
    let mut ids = Vec::new();
    // SAFETY: these calls have no preconditions.
    let (process, current) = unsafe { (GetCurrentProcessId(), GetCurrentThreadId()) };
    // SAFETY: taking a snapshot of all threads has no preconditions; an
    // invalid handle is handled.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return ids;
    }
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>()).unwrap_or(0),
        cntUsage: 0,
        th32ThreadID: 0,
        th32OwnerProcessID: 0,
        tpBasePri: 0,
        tpDeltaPri: 0,
        dwFlags: 0,
    };
    // SAFETY: `snapshot` is a valid snapshot handle and `entry` has its size set.
    let mut more = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == process && entry.th32ThreadID != current {
            ids.push(entry.th32ThreadID);
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: closes the snapshot taken above.
    unsafe { CloseHandle(snapshot) };
    ids
}
