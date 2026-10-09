//! Replacement game files served when the game loads them
//! (`docs/formats/game-structures.md`). A file thread runs every load as a
//! job; a job that would read a replaced resource from the archives is
//! turned into the game's own disk read of the replacement in the host's
//! cache before the job runs.
use super::entries::{Entries, Function};
use crate::{catalog::Catalog, file_replacements::FileReplacements};
use angelica_hook::Detour;
use angelica_map::ValueKind::{I32, Ptr, U8, U16};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

const DO_JOB: &str = "file.file_thread.do_job";
/// A job that reads a resource's file from the archives.
const MODE_ARCHIVE: u8 = 11;
/// A job that reads a file on disk into a resource.
const MODE_DISK: u8 = 0;
/// UTF-16 units of the descriptor's path, with its terminator.
const PATH_UNITS: usize = 260;
/// The longest game path read from a resource.
const MAX_NAME: u64 = 255;
/// The zeroed file object a disk job opens the file into; the game's
/// object uses its first 0x22 bytes.
const FILE_OBJECT: usize = 0x100;

struct Layout {
    mode: usize,
    file_interface: usize,
    resource_handle: usize,
    path: usize,
    file_name: usize,
}

struct FileReads {
    files: Arc<FileReplacements>,
    layout: Layout,
    served: AtomicU64,
}

static ADAPTER: OnceLock<FileReads> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

type DoJob = unsafe extern "system" fn(usize, usize, i32, u8) -> u8;

/// Installs the detour once.
pub fn install(catalog: &Catalog, base: usize, files: Arc<FileReplacements>) -> Result<(), String> {
    if ADAPTER.get().is_some() {
        return Ok(());
    }
    let entries = Entries { catalog, base };
    let layout = Layout {
        mode: entries.field("file.descriptor.mode", U8)?,
        file_interface: entries.field("file.descriptor.file_interface", Ptr)?,
        resource_handle: entries.field("file.descriptor.resource_handle", Ptr)?,
        path: entries.field("file.descriptor.path", U16)?,
        file_name: entries.field("file.resource_handle.file_name", Ptr)?,
    };
    let function = entries.function(DO_JOB, &[Ptr, Ptr, I32, U8], U8)?;
    install_resolved(layout, &function, files)
}

fn install_resolved(
    layout: Layout,
    function: &Function,
    files: Arc<FileReplacements>,
) -> Result<(), String> {
    // SAFETY: the map verified this address as the start of a function with
    // this ABI in the executable's code; the replacement has the same ABI.
    let mut detour =
        unsafe { Detour::new(function.address, do_job as DoJob as usize, function.length) }
            .map_err(|e| format!("file job detour: {e}"))?;
    ORIGINAL.store(detour.trampoline(), Ordering::Release);
    if ADAPTER
        .set(FileReads {
            files,
            layout,
            served: AtomicU64::new(0),
        })
        .is_err()
    {
        return Ok(());
    }
    // SAFETY: the adapter and the trampoline are published; nothing else in
    // this process changes these bytes meanwhile.
    unsafe { detour.enable() }.map_err(|e| format!("file job detour: {e}"))?;
    // Installed until the process exits; without replacements it only passes
    // jobs on.
    std::mem::forget(detour);
    Ok(())
}

unsafe extern "system" fn do_job(thread: usize, descriptor: usize, priority: i32, sync: u8) -> u8 {
    // The game's file object for a redirected job, alive for the whole call.
    let mut file_object = [0u64; FILE_OBJECT / 8];
    if let Some(adapter) = ADAPTER.get() {
        let object = file_object.as_mut_ptr() as usize;
        super::guard((), || adapter.redirect(descriptor, object));
    }
    // SAFETY: the trampoline runs the original with its own ABI; the
    // descriptor is either untouched or a valid disk job.
    unsafe {
        std::mem::transmute::<usize, DoJob>(ORIGINAL.load(Ordering::Acquire))(
            thread, descriptor, priority, sync,
        )
    }
}

impl FileReads {
    /// Turns an archive job for a replaced file into a disk job for its
    /// replacement.
    fn redirect(&self, descriptor: usize, file_object: usize) {
        if descriptor == 0 || self.files.is_empty() {
            return;
        }
        let layout = &self.layout;
        // SAFETY: the file thread passes a live descriptor and resource
        // handle; fields are at verified offsets, and the game path is the
        // handle's std::string, read within its length.
        let name = unsafe {
            if std::ptr::read_unaligned((descriptor + layout.mode) as *const u8) != MODE_ARCHIVE {
                return;
            }
            let handle =
                std::ptr::read_unaligned((descriptor + layout.resource_handle) as *const usize);
            if handle == 0 {
                return;
            }
            let string = handle + layout.file_name;
            let length = std::ptr::read_unaligned((string + 0x10) as *const u64);
            let capacity = std::ptr::read_unaligned((string + 0x18) as *const u64);
            if length == 0 || length > MAX_NAME {
                return;
            }
            let text = if capacity >= 16 {
                std::ptr::read_unaligned(string as *const usize)
            } else {
                string
            };
            if text == 0 {
                return;
            }
            std::slice::from_raw_parts(text as *const u8, usize::try_from(length).unwrap_or(0))
        };
        let Some(path) = self.files.lookup(name) else {
            return;
        };
        if path.len() > PATH_UNITS {
            return;
        }
        if self.served.fetch_add(1, Ordering::Relaxed) < 8 {
            super::log(&format!(
                "file replacements: serving {}",
                String::from_utf8_lossy(name)
            ));
        }
        // SAFETY: the descriptor's path holds 260 UTF-16 units and `path` is
        // at most that long with its terminator; the zeroed file object
        // outlives the job, which the original runs before `do_job` returns.
        unsafe {
            std::ptr::copy_nonoverlapping(
                path.as_ptr(),
                (descriptor + layout.path) as *mut u16,
                path.len(),
            );
            std::ptr::write_unaligned(
                (descriptor + layout.file_interface) as *mut usize,
                file_object,
            );
            std::ptr::write_unaligned((descriptor + layout.mode) as *mut u8, MODE_DISK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_sqpack::testing::FakeGame;

    /// A descriptor as the file thread passes it: mode, file object, and
    /// resource handle at the synthetic layout's offsets, then the path.
    #[repr(C, align(8))]
    struct Descriptor([u8; 0x278]);

    fn handle(name: &[u8]) -> Vec<u8> {
        // The resource's game path as a std::string at 0x48: inline text
        // when shorter than 16 bytes, otherwise a pointer.
        let mut handle = vec![0u8; 0x80];
        let text: &'static [u8] = Vec::leak(name.to_vec());
        if name.len() < 16 {
            handle[0x48..0x48 + name.len()].copy_from_slice(name);
            handle[0x60..0x68].copy_from_slice(&15u64.to_le_bytes());
        } else {
            handle[0x48..0x50].copy_from_slice(&(text.as_ptr() as usize).to_le_bytes());
            handle[0x60..0x68].copy_from_slice(&(name.len() as u64).to_le_bytes());
        }
        handle[0x58..0x60].copy_from_slice(&(name.len() as u64).to_le_bytes());
        handle
    }

    #[test]
    fn archive_jobs_for_replaced_files_become_disk_jobs() {
        let folder = tempfile::tempdir().unwrap();
        let mut texture = vec![0u8; 96];
        texture[4] = 0x40;
        FakeGame::new("1")
            .with_file("common/font/font1.tex", texture.clone())
            .write(&folder.path().join("game"))
            .unwrap();
        let data = crate::game_data::GameData::new(Some(&folder.path().join("game")));
        let files = Arc::new(FileReplacements::new(&folder.path().join("cache")));
        texture[95] = 1;
        files
            .replace("p", "common/font/font1.tex", &texture, &data)
            .unwrap();

        // `mov eax, 7; ret`, long enough to detour.
        let code = [0xB8, 7, 0, 0, 0, 0xC3, 0x90, 0x90, 0x90, 0x90];
        let function = Function {
            address: angelica_hook::testing::executable(&code),
            length: Some(code.len()),
        };
        let layout = Layout {
            mode: 0,
            file_interface: 0x30,
            resource_handle: 0x50,
            path: 0x70,
            file_name: 0x48,
        };
        install_resolved(layout, &function, files.clone()).unwrap();
        // SAFETY: the detoured synthetic function has the file job ABI.
        let run = unsafe { std::mem::transmute::<usize, DoJob>(function.address) };

        let job = |name: &[u8], mode: u8| {
            let handle = handle(name);
            let mut descriptor = Box::new(Descriptor([0; 0x278]));
            descriptor.0[0] = mode;
            descriptor.0[0x30..0x38].copy_from_slice(&0x1234usize.to_le_bytes());
            descriptor.0[0x50..0x58].copy_from_slice(&(handle.as_ptr() as usize).to_le_bytes());
            // SAFETY: the descriptor and handle live across the call.
            let result = unsafe { run(0, descriptor.0.as_ptr() as usize, 1, 0) };
            (result, descriptor)
        };

        let (result, descriptor) = job(b"common/font/font1.tex", MODE_ARCHIVE);
        assert_eq!(result, 7, "the original runs");
        assert_eq!(descriptor.0[0], MODE_DISK);
        assert_ne!(
            usize::from_le_bytes(descriptor.0[0x30..0x38].try_into().unwrap()),
            0x1234
        );
        let units: Vec<u16> = descriptor.0[0x70..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect();
        let end = units.iter().position(|&u| u == 0).unwrap();
        let path = String::from_utf16(&units[..end]).unwrap();
        assert_eq!(std::fs::read(path).unwrap(), texture);

        for (name, mode) in [
            (&b"common/font/font2.tex"[..], MODE_ARCHIVE),
            (b"common/font/font1.tex", 1),
            (b"short.tex", MODE_ARCHIVE),
        ] {
            let (result, descriptor) = job(name, mode);
            assert_eq!(result, 7);
            assert_eq!(descriptor.0[0], mode, "untouched");
            assert_eq!(
                usize::from_le_bytes(descriptor.0[0x30..0x38].try_into().unwrap()),
                0x1234
            );
        }
    }
}
