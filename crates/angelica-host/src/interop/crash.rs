//! Best-effort diagnostics for unhandled process exceptions, preserving the prior filter.
use crate::{catalog::Catalog, host::Host};
use std::{
    os::windows::io::AsRawHandle,
    path::PathBuf,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use windows_sys::Win32::System::{
    Diagnostics::Debug::{
        EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MiniDumpNormal, MiniDumpWriteDump,
        SetUnhandledExceptionFilter,
    },
    Threading::{GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId},
};

static WRITER: OnceLock<Writer> = OnceLock::new();
static PREVIOUS: AtomicUsize = AtomicUsize::new(0);
struct Writer {
    directory: PathBuf,
    host: Option<Arc<Host>>,
    catalog: Arc<Catalog>,
    captured: AtomicBool,
}

pub(super) fn install(host: Arc<Host>, catalog: Arc<Catalog>, directory: PathBuf) {
    if WRITER
        .set(Writer {
            directory,
            host: Some(host),
            catalog,
            captured: AtomicBool::new(false),
        })
        .is_err()
    {
        return;
    }
    // SAFETY: callback has the documented exception-filter ABI and the host is pinned.
    let previous = unsafe { SetUnhandledExceptionFilter(Some(filter)) };
    PREVIOUS.store(
        previous.map_or(0, |function| function as usize),
        Ordering::Release,
    );
}

impl Writer {
    fn capture(&self, exception: *const EXCEPTION_POINTERS) -> Result<(), String> {
        if self.captured.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        let name = format!("crash-{}-{stamp}", std::process::id());
        let path = self.directory.join(format!("{name}.dmp"));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        // SAFETY: Windows provides the thread ID; exception is either NULL for a diagnostic
        // snapshot or the unhandled-exception pointer supplied by Windows for this callback.
        let info = MINIDUMP_EXCEPTION_INFORMATION {
            // SAFETY: querying the current thread ID has no pointer preconditions.
            ThreadId: unsafe { GetCurrentThreadId() },
            ExceptionPointers: exception.cast_mut(),
            ClientPointers: 0,
        };
        // SAFETY: owned output file and current-process handles are valid; the exception
        // structure lives throughout the synchronous dump operation and uses local pointers.
        let dumped = unsafe {
            MiniDumpWriteDump(
                GetCurrentProcess(),
                GetCurrentProcessId(),
                file.as_raw_handle(),
                MiniDumpNormal,
                if exception.is_null() {
                    std::ptr::null()
                } else {
                    &raw const info
                },
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        let plugins: Vec<_> = self
            .host
            .iter()
            .flat_map(|host| host.crash_snapshot())
            .map(|(id, version, status)| serde_json::json!({"id": id, "version": version, "status": status}))
            .collect();
        let report = serde_json::json!({
            "executable_sha256": self.catalog.executable_sha256,
            "entries_sha256": self.catalog.entries_sha256,
            "plugins": plugins,
            "dump_written": dumped != 0,
        });
        let report = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(self.directory.join(format!("{name}.json")), report)
            .map_err(|e| e.to_string())?;
        if dumped == 0 {
            return Err("MiniDumpWriteDump failed".into());
        }
        Ok(())
    }
}

/// # Safety
/// Windows supplies a live exception context for the duration of the filter call.
unsafe extern "system" fn filter(exception: *const EXCEPTION_POINTERS) -> i32 {
    super::guard((), || {
        if let Some(writer) = WRITER.get() {
            let _ = writer.capture(exception);
        }
    });
    let previous = PREVIOUS.load(Ordering::Acquire);
    if previous == 0 {
        return 0;
    } // EXCEPTION_CONTINUE_SEARCH
    // SAFETY: the retained previous callback was supplied by SetUnhandledExceptionFilter.
    let previous: unsafe extern "system" fn(*const EXCEPTION_POINTERS) -> i32 =
        unsafe { std::mem::transmute(previous) };
    // SAFETY: forward the original exception pointer without changing exception handling.
    unsafe { previous(exception) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writes_a_real_minidump_and_structured_report_without_crashing() {
        let directory = tempfile::tempdir().unwrap();
        let writer = Writer {
            directory: directory.path().into(),
            host: None,
            catalog: Arc::new(Catalog::default()),
            captured: AtomicBool::new(false),
        };
        writer.capture(std::ptr::null()).unwrap();
        let files: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|item| item.unwrap().path())
            .collect();
        let dump = files
            .iter()
            .find(|path| path.extension().is_some_and(|extension| extension == "dmp"))
            .unwrap();
        assert_eq!(&std::fs::read(dump).unwrap()[..4], b"MDMP");
        let report = files
            .iter()
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
        assert_eq!(report["dump_written"], true);
        assert_eq!(report["plugins"], serde_json::json!([]));
    }
}
