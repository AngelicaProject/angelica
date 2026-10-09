//! Replacement game files served when the game loads them
//! (`docs/formats/game-structures.md`). A file thread runs every load as a
//! job; a job that would read a replaced resource from the archives is
//! turned into the game's own disk read of the replacement in the host's
//! cache before the job runs. The texture load takes a texture read from
//! disk only from the game's protected file list, so a replaced texture it
//! refuses is loaded again with the game's loader for plain files.
use super::entries::{Entries, Function};
use crate::{catalog::Catalog, file_replacements::FileReplacements};
use angelica_hook::Detour;
use angelica_map::ValueKind::{I32, Ptr, U8, U16, U64};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

const DO_JOB: &str = "file.file_thread.do_job";
const LOAD_TEXTURE: &str = "file.texture_handle.load";
const LOAD_UNPACKED: &str = "file.texture_handle.load_unpacked";
/// A job that reads a resource's file from the archives.
const MODE_ARCHIVE: u8 = 11;
/// A job that reads a file on disk into a resource.
const MODE_DISK: u8 = 0;
/// The texture load's result for a disk file the protected list lacks.
const NOT_PROTECTED: u8 = 0xF1;
/// UTF-16 units of the descriptor's path, with its terminator.
const PATH_UNITS: usize = 260;
/// The longest game path read from a resource.
const MAX_NAME: u64 = 255;
/// The zeroed file object a disk job opens the file into; the game's
/// object uses its first 0x22 bytes.
const FILE_OBJECT: usize = 0x100;
/// Texture flag: the texture loads at the graphics settings' detail level.
const FOLLOWS_SETTINGS: u8 = 1;
/// Texture flag: the resource's category is 1 or 2.
const CATEGORY_FLAG: u8 = 2;
/// The settings' detail value that loads detail level 2.
const DETAIL_UNSET: u8 = 0xFF;

struct Layout {
    mode: usize,
    length: usize,
    start_offset: usize,
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

/// What loading a refused replacement texture again needs.
struct Textures {
    files: Arc<FileReplacements>,
    layout: TextureLayout,
    /// The game's loader for a plain .tex file.
    load_unpacked: usize,
    /// The global holding the graphics settings.
    settings: usize,
    loaded: AtomicU64,
}

struct TextureLayout {
    mode: usize,
    path: usize,
    file_name: usize,
    category: usize,
    flags: usize,
    detail: usize,
}

static ADAPTER: OnceLock<FileReads> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static TEXTURES: OnceLock<Textures> = OnceLock::new();
static ORIGINAL_LOAD: AtomicUsize = AtomicUsize::new(0);

type DoJob = unsafe extern "system" fn(usize, usize, i32, u8) -> u8;
type LoadTexture = unsafe extern "system" fn(usize, usize, u8) -> u8;
type LoadUnpacked = unsafe extern "system" fn(usize, i32, usize, u8) -> u8;

/// Installs the detours once.
pub fn install(catalog: &Catalog, base: usize, files: Arc<FileReplacements>) -> Result<(), String> {
    if ADAPTER.get().is_some() {
        return Ok(());
    }
    let entries = Entries { catalog, base };
    let layout = Layout {
        mode: entries.field("file.descriptor.mode", U8)?,
        length: entries.field("file.descriptor.length", U64)?,
        start_offset: entries.field("file.descriptor.start_offset", U64)?,
        file_interface: entries.field("file.descriptor.file_interface", Ptr)?,
        resource_handle: entries.field("file.descriptor.resource_handle", Ptr)?,
        path: entries.field("file.descriptor.path", U16)?,
        file_name: entries.field("file.resource_handle.file_name", Ptr)?,
    };
    let textures = Textures {
        files: files.clone(),
        layout: TextureLayout {
            mode: layout.mode,
            path: layout.path,
            file_name: layout.file_name,
            category: entries.field("file.resource_handle.category", U16)?,
            flags: entries.field("file.texture_handle.flags", U8)?,
            detail: entries.field("graphics.config.texture_detail", U8)?,
        },
        load_unpacked: entries
            .function(LOAD_UNPACKED, &[Ptr, I32, Ptr, U8], U8)?
            .address,
        settings: entries.global("graphics.config.instance", Ptr)?,
        loaded: AtomicU64::new(0),
    };
    let load = entries.function(LOAD_TEXTURE, &[Ptr, Ptr, U8], U8)?;
    let function = entries.function(DO_JOB, &[Ptr, Ptr, I32, U8], U8)?;
    // Textures first: no job is redirected before its texture can load.
    install_textures(textures, &load)?;
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

fn install_textures(textures: Textures, function: &Function) -> Result<(), String> {
    // SAFETY: as in `install_resolved`, for the texture load's ABI.
    let mut detour = unsafe {
        Detour::new(
            function.address,
            load_texture as LoadTexture as usize,
            function.length,
        )
    }
    .map_err(|e| format!("texture load detour: {e}"))?;
    ORIGINAL_LOAD.store(detour.trampoline(), Ordering::Release);
    if TEXTURES.set(textures).is_err() {
        return Ok(());
    }
    // SAFETY: as in `install_resolved`.
    unsafe { detour.enable() }.map_err(|e| format!("texture load detour: {e}"))?;
    // Installed until the process exits; it acts only on the host's files.
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

unsafe extern "system" fn load_texture(handle: usize, descriptor: usize, reset: u8) -> u8 {
    // SAFETY: the trampoline runs the original with its own ABI and the
    // game's arguments.
    let result = unsafe {
        std::mem::transmute::<usize, LoadTexture>(ORIGINAL_LOAD.load(Ordering::Acquire))(
            handle, descriptor, reset,
        )
    };
    if result != NOT_PROTECTED {
        return result;
    }
    match TEXTURES.get() {
        Some(textures) => super::guard(result, || {
            textures.load(handle, descriptor, reset).unwrap_or(result)
        }),
        None => result,
    }
}

/// The game path of a resource handle, its `std::string` at `file_name`.
///
/// # Safety
/// `handle` is zero or a live resource handle whose game path is at
/// `file_name` and outlives the result.
unsafe fn game_path<'a>(handle: usize, file_name: usize) -> Option<&'a [u8]> {
    if handle == 0 {
        return None;
    }
    let string = handle + file_name;
    // SAFETY: the caller's handle holds a std::string there: 16 bytes of
    // inline text or a pointer, then its length and capacity; the text is
    // read within its length.
    unsafe {
        let length = std::ptr::read_unaligned((string + 0x10) as *const u64);
        let capacity = std::ptr::read_unaligned((string + 0x18) as *const u64);
        if length == 0 || length > MAX_NAME {
            return None;
        }
        let text = if capacity >= 16 {
            std::ptr::read_unaligned(string as *const usize)
        } else {
            string
        };
        if text == 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(
            text as *const u8,
            usize::try_from(length).ok()?,
        ))
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
        // handle; fields are at verified offsets.
        let name = unsafe {
            if std::ptr::read_unaligned((descriptor + layout.mode) as *const u8) != MODE_ARCHIVE {
                return;
            }
            let handle =
                std::ptr::read_unaligned((descriptor + layout.resource_handle) as *const usize);
            game_path(handle, layout.file_name)
        };
        let Some(name) = name else { return };
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
            // An archive job leaves these from the archive request; the disk
            // read must take the whole replacement from its start.
            std::ptr::write_unaligned((descriptor + layout.length) as *mut u64, 0);
            std::ptr::write_unaligned((descriptor + layout.start_offset) as *mut u64, 0);
            std::ptr::write_unaligned((descriptor + layout.mode) as *mut u8, MODE_DISK);
        }
    }
}

impl Textures {
    /// Loads a texture the texture load refused with the game's loader for
    /// plain files, when its job reads a file in the host's cache, and sets
    /// the flags the refused load left unset; `None` keeps the refusal.
    fn load(&self, handle: usize, descriptor: usize, reset: u8) -> Option<u8> {
        if handle == 0 || descriptor == 0 {
            return None;
        }
        let layout = &self.layout;
        // SAFETY: the texture load passes a live descriptor and texture
        // handle; fields are at verified offsets, the path is read within
        // its 260 units, and the settings global holds zero or the live
        // settings.
        let (detail, name) = unsafe {
            if std::ptr::read_unaligned((descriptor + layout.mode) as *const u8) != MODE_DISK {
                return None;
            }
            let path =
                std::slice::from_raw_parts((descriptor + layout.path) as *const u16, PATH_UNITS);
            if !self.files.is_cached(path) {
                return None;
            }
            let flags = std::ptr::read_unaligned((handle + layout.flags) as *const u8);
            // The detail level the refused load chose, by its rule.
            let detail = if flags & FOLLOWS_SETTINGS == 0 {
                0
            } else {
                let settings = std::ptr::read_unaligned(self.settings as *const usize);
                if settings == 0 {
                    return None;
                }
                match std::ptr::read_unaligned((settings + layout.detail) as *const u8) {
                    DETAIL_UNSET => 2,
                    level => level,
                }
            };
            (detail, game_path(handle, layout.file_name)?)
        };
        // SAFETY: the map verified the loader's ABI; it takes the texture
        // load's handle, descriptor, and reset flag, and a detail level.
        let result = unsafe {
            std::mem::transmute::<usize, LoadUnpacked>(self.load_unpacked)(
                handle,
                i32::from(detail),
                descriptor,
                reset,
            )
        };
        // SAFETY: as above; the category and flags are the handle's, and
        // the texture load sets the flags the same way on this thread.
        unsafe {
            let category = std::ptr::read_unaligned((handle + layout.category) as *const u16);
            let flags = (handle + layout.flags) as *mut u8;
            let mut set = std::ptr::read_unaligned(flags) | kind_flag(name);
            if category.wrapping_sub(1) <= 1 {
                set |= CATEGORY_FLAG;
            }
            std::ptr::write_unaligned(flags, set);
        }
        if self.loaded.fetch_add(1, Ordering::Relaxed) < 8 {
            super::log(&format!(
                "file replacements: loaded texture {} -> {result}",
                String::from_utf8_lossy(name)
            ));
        }
        Some(result)
    }
}

/// The texture flag for the letter before the extension of a game path:
/// d, n, or s.
fn kind_flag(name: &[u8]) -> u8 {
    let Some(dot) = name.iter().rposition(|&b| b == b'.') else {
        return 0;
    };
    match dot.checked_sub(1).map(|at| name[at]) {
        Some(b'd') => 4,
        Some(b'n') => 8,
        Some(b's') => 0x10,
        _ => 0,
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
            length: 0x10,
            start_offset: 0x18,
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
            descriptor.0[0x10..0x18].copy_from_slice(&77u64.to_le_bytes());
            descriptor.0[0x18..0x20].copy_from_slice(&99u64.to_le_bytes());
            descriptor.0[0x50..0x58].copy_from_slice(&(handle.as_ptr() as usize).to_le_bytes());
            // SAFETY: the descriptor and handle live across the call.
            let result = unsafe { run(0, descriptor.0.as_ptr() as usize, 1, 0) };
            (result, descriptor)
        };

        let (result, descriptor) = job(b"common/font/font1.tex", MODE_ARCHIVE);
        assert_eq!(result, 7, "the original runs");
        assert_eq!(descriptor.0[0], MODE_DISK);
        assert_eq!(
            descriptor.0[0x10..0x20],
            [0; 16],
            "the whole file from its start"
        );
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

    #[test]
    fn refused_replacement_textures_load_as_plain_files() {
        let folder = tempfile::tempdir().unwrap();
        let mut texture = vec![0u8; 96];
        texture[4] = 0x40;
        FakeGame::new("1")
            .with_file("chara/common/texture/skin_d.tex", texture.clone())
            .write(&folder.path().join("game"))
            .unwrap();
        let data = crate::game_data::GameData::new(Some(&folder.path().join("game")));
        let files = Arc::new(FileReplacements::new(&folder.path().join("cache")));
        texture[95] = 1;
        files
            .replace("p", "chara/common/texture/skin_d.tex", &texture, &data)
            .unwrap();
        let cached = files.lookup(b"chara/common/texture/skin_d.tex").unwrap();

        // The texture load refuses every disk file: `mov eax, 0xF1; ret`.
        let refuse = [0xB8, 0xF1, 0, 0, 0, 0xC3, 0x90, 0x90, 0x90, 0x90];
        let load = Function {
            address: angelica_hook::testing::executable(&refuse),
            length: Some(refuse.len()),
        };
        // The plain loader returns the detail level plus 0x20:
        // `lea eax, [rdx + 0x20]; ret`.
        let load_unpacked = angelica_hook::testing::executable(&[0x8D, 0x42, 0x20, 0xC3]);
        let mut settings = Box::new([0u8; 16]);
        let global = Box::new(settings.as_mut_ptr() as usize);
        let textures = Textures {
            files: files.clone(),
            layout: TextureLayout {
                mode: 0,
                path: 0x70,
                file_name: 0x48,
                category: 0x8,
                flags: 0x40,
                detail: 0xE,
            },
            load_unpacked,
            settings: std::ptr::from_ref(&*global) as usize,
            loaded: AtomicU64::new(0),
        };
        install_textures(textures, &load).unwrap();
        // SAFETY: the detoured synthetic function has the texture load ABI.
        let run = unsafe { std::mem::transmute::<usize, LoadTexture>(load.address) };

        let mut handle = handle(b"chara/common/texture/skin_d.tex");
        handle[0x8] = 2;
        handle[0x40] = FOLLOWS_SETTINGS;
        let load_with = |handle: &mut Vec<u8>, mode: u8, path: &[u16]| {
            let mut descriptor = Box::new(Descriptor([0; 0x278]));
            descriptor.0[0] = mode;
            for (at, unit) in path.iter().enumerate() {
                descriptor.0[0x70 + 2 * at..0x72 + 2 * at].copy_from_slice(&unit.to_le_bytes());
            }
            // SAFETY: the descriptor and handle live across the call.
            unsafe {
                run(
                    handle.as_mut_ptr() as usize,
                    descriptor.0.as_ptr() as usize,
                    1,
                )
            }
        };

        settings[0xE] = DETAIL_UNSET;
        assert_eq!(load_with(&mut handle, MODE_DISK, &cached), 0x22, "level 2");
        assert_eq!(
            handle[0x40],
            FOLLOWS_SETTINGS | CATEGORY_FLAG | 4,
            "category and d flags"
        );
        settings[0xE] = 1;
        assert_eq!(load_with(&mut handle, MODE_DISK, &cached), 0x21);
        handle[0x40] = 0;
        assert_eq!(load_with(&mut handle, MODE_DISK, &cached), 0x20, "level 0");

        let elsewhere: Vec<u16> = folder
            .path()
            .join("game")
            .join("skin_d.tex")
            .to_string_lossy()
            .encode_utf16()
            .chain([0])
            .collect();
        assert_eq!(load_with(&mut handle, MODE_DISK, &elsewhere), NOT_PROTECTED);
        assert_eq!(load_with(&mut handle, MODE_ARCHIVE, &cached), NOT_PROTECTED);
        drop(global);
    }

    #[test]
    fn texture_kinds_come_from_the_letter_before_the_extension() {
        assert_eq!(kind_flag(b"a/b_d.tex"), 4);
        assert_eq!(kind_flag(b"a/b_n.tex"), 8);
        assert_eq!(kind_flag(b"a/b_s.tex"), 0x10);
        assert_eq!(kind_flag(b"common/font/font_lobby6.tex"), 0);
        assert_eq!(kind_flag(b".tex"), 0);
        assert_eq!(kind_flag(b"tex"), 0);
    }
}
