//! "Who writes this": a hardware data breakpoint on one address for
//! `angelica look` in dev mode, to find the code that changes a field the
//! map does not describe yet. The processor stops after every write to the
//! address; a vectored exception handler counts where the writing
//! instruction ended and lets the thread go on.
//!
//! The debug registers are set in every thread the process has when the
//! watch starts, by a worker thread while each thread is suspended; a thread
//! started later is not watched. Between suspending a thread and resuming
//! it the worker only gets and sets the thread's context: a suspended
//! thread may hold the heap's lock. The handler stays registered for the
//! life of the process and touches only atomics, so a trap still on its way
//! after the watch ended is taken as ours instead of reaching the game.
use crate::game::Written;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use windows_sys::Win32::Foundation::{CloseHandle, EXCEPTION_SINGLE_STEP, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, CONTEXT, CONTEXT_DEBUG_REGISTERS_AMD64,
    EXCEPTION_CONTINUE_EXECUTION, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS, GetThreadContext,
    SetThreadContext,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
    THREAD_GET_CONTEXT, THREAD_SET_CONTEXT, THREAD_SUSPEND_RESUME,
};

/// The processor's four debug address registers.
const SLOTS: usize = 4;
/// The most places a watch tells apart; writes from others are counted as
/// missed.
const MOST_PLACES: usize = 64;

/// The address each slot was last armed with. A trap whose slot register
/// holds it is ours, also after the watch ended.
static ADDRESSES: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
/// Where writing instructions ended, and how often.
static PLACES: [AtomicU64; MOST_PLACES] = [const { AtomicU64::new(0) }; MOST_PLACES];
static COUNTS: [AtomicU64; MOST_PLACES] = [const { AtomicU64::new(0) }; MOST_PLACES];
static MISSED: AtomicU64 = AtomicU64::new(0);
/// Whether a watch runs; one at a time.
static BUSY: AtomicBool = AtomicBool::new(false);
/// The slot and address of the running watch.
static ARMED: AtomicUsize = AtomicUsize::new(usize::MAX);
static HANDLER: OnceLock<()> = OnceLock::new();

/// `CONTEXT` with the 16-byte alignment `GetThreadContext` needs.
#[repr(C, align(16))]
struct Context(CONTEXT);

impl Context {
    fn debug_registers() -> Self {
        // SAFETY: CONTEXT is plain data; all zeros is a valid value.
        let mut context = Self(unsafe { std::mem::zeroed() });
        context.0.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64;
        context
    }

    fn register(&self, slot: usize) -> u64 {
        [self.0.Dr0, self.0.Dr1, self.0.Dr2, self.0.Dr3][slot]
    }

    fn set_register(&mut self, slot: usize, value: u64) {
        match slot {
            0 => self.0.Dr0 = value,
            1 => self.0.Dr1 = value,
            2 => self.0.Dr2 = value,
            _ => self.0.Dr3 = value,
        }
    }
}

/// The bits of DR7 that enable `slot` and say what it watches.
fn control_mask(slot: usize) -> u64 {
    (0b11 << (slot * 2)) | (0b1111 << (16 + slot * 4))
}

/// DR7's bits for a write of `size` bytes in `slot`, locally enabled.
fn control(slot: usize, size: u8) -> u64 {
    let length = match size {
        1 => 0b00,
        2 => 0b01,
        8 => 0b10,
        _ => 0b11,
    };
    (1 << (slot * 2)) | ((0b01 | (length << 2)) << (16 + slot * 4))
}

unsafe extern "system" fn handler(pointers: *mut EXCEPTION_POINTERS) -> i32 {
    // SAFETY: the system hands a vectored handler valid exception pointers
    // for the duration of the call.
    let (record, context) = unsafe {
        (
            &*(*pointers).ExceptionRecord,
            &mut *(*pointers).ContextRecord,
        )
    };
    if record.ExceptionCode != EXCEPTION_SINGLE_STEP {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let registers = [context.Dr0, context.Dr1, context.Dr2, context.Dr3];
    let ours = (0..SLOTS).any(|slot| {
        let address = ADDRESSES[slot].load(Ordering::Acquire);
        context.Dr6 & (1 << slot) != 0 && address != 0 && registers[slot] == address
    });
    if !ours {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    note(context.Rip);
    context.Dr6 &= !0b1111;
    EXCEPTION_CONTINUE_EXECUTION
}

/// Counts a write that ended at `place`.
fn note(place: u64) {
    for index in 0..MOST_PLACES {
        let held = PLACES[index].load(Ordering::Acquire);
        if held == place {
            COUNTS[index].fetch_add(1, Ordering::AcqRel);
            return;
        }
        if held == 0 {
            match PLACES[index].compare_exchange(0, place, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    COUNTS[index].fetch_add(1, Ordering::AcqRel);
                    return;
                }
                Err(other) if other == place => {
                    COUNTS[index].fetch_add(1, Ordering::AcqRel);
                    return;
                }
                Err(_) => {}
            }
        }
    }
    MISSED.fetch_add(1, Ordering::AcqRel);
}

/// The ids of the process's threads but the calling one.
fn threads() -> Result<Vec<u32>, String> {
    // SAFETY: plain calls; the snapshot handle is closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err("the process's threads cannot be listed".into());
    }
    // SAFETY: plain calls.
    let (process, current) = unsafe { (GetCurrentProcessId(), GetCurrentThreadId()) };
    let mut ids = Vec::new();
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).unwrap_or(0),
        cntUsage: 0,
        th32ThreadID: 0,
        th32OwnerProcessID: 0,
        tpBasePri: 0,
        tpDeltaPri: 0,
        dwFlags: 0,
    };
    // SAFETY: `entry` is a THREADENTRY32 with its size set, as the calls ask.
    let mut more = unsafe { Thread32First(snapshot, &raw mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == process && entry.th32ThreadID != current {
            ids.push(entry.th32ThreadID);
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: the snapshot is a handle this function opened.
    unsafe { CloseHandle(snapshot) };
    Ok(ids)
}

/// Runs `change` on the debug registers of the thread `id` while it is
/// suspended; whether the thread could be changed. `change` returns
/// whether to set the context back, and must neither allocate nor lock: the
/// suspended thread may hold the heap's lock.
fn with_thread(id: u32, change: &mut dyn FnMut(&mut Context) -> bool) -> bool {
    // SAFETY: plain call; the handle is closed below.
    let thread = unsafe {
        OpenThread(
            THREAD_GET_CONTEXT | THREAD_SET_CONTEXT | THREAD_SUSPEND_RESUME,
            0,
            id,
        )
    };
    if thread.is_null() {
        return false;
    }
    let mut context = Context::debug_registers();
    // SAFETY: the thread handle is open with the rights asked for. While
    // the thread is suspended only its context is read and written.
    unsafe {
        if SuspendThread(thread) == u32::MAX {
            CloseHandle(thread);
            return false;
        }
        let read = GetThreadContext(thread, &raw mut context.0) != 0;
        let written =
            read && (!change(&mut context) || SetThreadContext(thread, &raw const context.0) != 0);
        ResumeThread(thread);
        CloseHandle(thread);
        written
    }
}

/// Runs `work` on a thread of its own and waits for it, so that every
/// other thread, the calling one included, can be suspended.
fn on_worker<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    std::thread::Builder::new()
        .name("angelica-writes".into())
        .spawn(work)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the watch's worker failed".to_owned())
}

/// Starts watching writes of `size` bytes at `address`.
///
/// # Errors
/// A watch already runs, the address does not fit the size, every debug
/// register is in use, or a debugger is attached.
pub fn start(address: u64, size: u8) -> Result<(), String> {
    if !matches!(size, 1 | 2 | 4 | 8) || address == 0 || !address.is_multiple_of(u64::from(size)) {
        return Err(format!(
            "a watch is of 1, 2, 4, or 8 bytes at an address aligned to its size, not {size} at {address:#x}"
        ));
    }
    // SAFETY: plain call.
    if unsafe { windows_sys::Win32::System::Diagnostics::Debug::IsDebuggerPresent() } != 0 {
        return Err("a debugger is attached: it takes the debug registers".into());
    }
    if BUSY.swap(true, Ordering::AcqRel) {
        return Err("another watch of writes runs".into());
    }
    HANDLER.get_or_init(|| {
        // SAFETY: `handler` has the vectored handler ABI, touches only
        // statics, and stays valid for the life of the process.
        unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    });
    for (place, count) in PLACES.iter().zip(&COUNTS) {
        place.store(0, Ordering::Release);
        count.store(0, Ordering::Release);
    }
    MISSED.store(0, Ordering::Release);
    let armed = on_worker(move || arm(address, size)).and_then(|armed| armed);
    if armed.is_err() {
        BUSY.store(false, Ordering::Release);
    }
    armed
}

fn arm(address: u64, size: u8) -> Result<(), String> {
    let ids = threads()?;
    let mut used = 0u64;
    for id in &ids {
        with_thread(*id, &mut |context| {
            used |= context.0.Dr7;
            false
        });
    }
    let slot = (0..SLOTS)
        .find(|slot| used & (0b11 << (slot * 2)) == 0)
        .ok_or("every debug register is in use by another tool")?;
    ADDRESSES[slot].store(address, Ordering::Release);
    ARMED.store(slot, Ordering::Release);
    let mut armed = 0;
    for id in &ids {
        armed += usize::from(with_thread(*id, &mut |context| {
            context.set_register(slot, address);
            context.0.Dr7 = (context.0.Dr7 & !control_mask(slot)) | control(slot, size);
            true
        }));
    }
    if armed == 0 {
        ARMED.store(usize::MAX, Ordering::Release);
        return Err("no thread of the game could be watched".into());
    }
    Ok(())
}

/// Stops the watch and returns where writes ended, how often, and how many
/// writes came from places past the ones it tells apart.
#[must_use]
pub fn finish() -> (Vec<Written>, u64) {
    let slot = ARMED.swap(usize::MAX, Ordering::AcqRel);
    if slot < SLOTS {
        let address = ADDRESSES[slot].load(Ordering::Acquire);
        let _ = on_worker(move || {
            for id in threads().unwrap_or_default() {
                with_thread(id, &mut |context| {
                    if context.register(slot) != address {
                        return false;
                    }
                    context.set_register(slot, 0);
                    context.0.Dr7 &= !control_mask(slot);
                    true
                });
            }
        });
    }
    let written = PLACES
        .iter()
        .zip(&COUNTS)
        .map(|(place, count)| (place.load(Ordering::Acquire), count.load(Ordering::Acquire)))
        .filter(|(place, _)| *place != 0)
        .map(|(after, count)| Written {
            after,
            offset: None,
            count,
        })
        .collect();
    let missed = MISSED.load(Ordering::Acquire);
    BUSY.store(false, Ordering::Release);
    (written, missed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dr7_watches_writes_of_the_size_in_the_slot() {
        // Slot 0, write (01), 4 bytes (11): L0 and bits 16-19 = 1101.
        assert_eq!(control(0, 4), 0b1 | (0b1101 << 16));
        // Slot 3, write, 8 bytes (10): L3 and bits 28-31 = 1001.
        assert_eq!(control(3, 8), (0b1 << 6) | (0b1001 << 28));
        // Slot 1, write, 1 byte (00).
        assert_eq!(control(1, 1), (0b1 << 2) | (0b0001 << 20));
        for slot in 0..SLOTS {
            assert_eq!(control(slot, 2) & !control_mask(slot), 0, "within its bits");
        }
    }

    /// One test, since the watch and its table are the process's own.
    #[test]
    fn writes_are_counted_by_place_and_the_watch_ends() {
        static FIELD: AtomicU64 = AtomicU64::new(0);
        let address = std::ptr::from_ref(&FIELD) as u64;
        start(address, 8).unwrap();
        assert!(start(address, 8).is_err(), "one watch at a time");
        for value in 1..=3 {
            FIELD.store(value, Ordering::SeqCst);
        }
        std::thread::spawn(|| FIELD.store(9, Ordering::SeqCst))
            .join()
            .unwrap();
        let (written, missed) = finish();
        assert_eq!(missed, 0);
        assert_eq!(
            written.iter().map(|place| place.count).sum::<u64>(),
            3,
            "the writes of this thread; a thread started later is not watched"
        );
        FIELD.store(4, Ordering::SeqCst);
        let (written, _) = finish();
        assert!(written.iter().all(|place| place.count == 3) || written.is_empty());

        assert!(start(address + 1, 8).is_err(), "unaligned");
        assert!(start(address, 3).is_err(), "no such size");

        for (place, count) in PLACES.iter().zip(&COUNTS) {
            place.store(0, Ordering::Release);
            count.store(0, Ordering::Release);
        }
        MISSED.store(0, Ordering::Release);
        note(0x10);
        note(0x10);
        for place in 0..MOST_PLACES as u64 {
            note(0x100 + place);
        }
        assert_eq!(COUNTS[0].load(Ordering::Acquire), 2);
        assert_eq!(MISSED.load(Ordering::Acquire), 1, "the 65th place");
    }
}
