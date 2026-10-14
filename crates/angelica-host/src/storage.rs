//! A plugin's private folder (the `storage` interface). Paths never leave
//! the folder, and the folder's total size is bounded. Paths under `cache/`
//! lead to the plugin's cache, a folder of its own with the quota the
//! player approved for `storage.cache`.
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

pub const MAX_READ: u64 = 64 * 1024 * 1024;
pub const MAX_RANGE: u32 = 16 * 1024 * 1024;
pub const MAX_PATH: usize = 512;
pub const MAX_DEPTH: usize = 16;
/// The default size limit of one plugin's folder.
pub const DEFAULT_QUOTA: u64 = 4 * 1024 * 1024 * 1024;
const TEMPORARY: &str = ".angelica-tmp";
/// The top folder whose paths lead to the plugin's cache.
pub const CACHE: &str = "cache";

pub struct Entry {
    pub name: String,
    pub directory: bool,
    pub size: u64,
}

pub struct Storage {
    root: PathBuf,
    quota: u64,
    /// Bytes in use, measured on first write.
    used: Mutex<Option<u64>>,
    /// The plugin's cache while it holds `storage.cache`.
    cache: Mutex<Option<Arc<Storage>>>,
}

fn io(path: &str, error: &std::io::Error) -> Error {
    let kind = if error.kind() == std::io::ErrorKind::NotFound {
        Kind::NotFound
    } else {
        Kind::Io
    };
    Error::new(kind, format!("{path}: {error}"))
}

/// Whether a name is a serial or parallel port device: `com` or `lpt` and
/// one digit, or a superscript one, two, or three.
fn port(stem: &str) -> bool {
    let Some(number) = stem
        .strip_prefix("com")
        .or_else(|| stem.strip_prefix("lpt"))
    else {
        return false;
    };
    let mut chars = number.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
    )
}

/// Whether `path` is a relative path that stays inside the folder: `/`
/// separators, no empty, `.` or `..` parts, no characters Windows treats
/// specially, no device names, no trailing dots or spaces.
#[must_use]
pub fn valid_path(path: &str) -> bool {
    const DEVICES: [&str; 6] = ["con", "prn", "aux", "nul", "conin$", "conout$"];
    path.len() <= MAX_PATH
        && path.split('/').count() <= MAX_DEPTH
        && !path.ends_with(TEMPORARY)
        && path.split('/').all(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with('.')
                && !part.ends_with(' ')
                && !part.chars().any(|c| {
                    c.is_control() || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                })
                && !DEVICES.contains(&stem.trim_end())
                && !port(stem.trim_end())
        })
}

impl Storage {
    /// # Errors
    /// Fails when the folder cannot be created.
    pub fn new(root: &Path, quota: u64) -> Result<Self> {
        fs::create_dir_all(root).map_err(|e| io(&root.display().to_string(), &e))?;
        Ok(Self {
            root: root.to_path_buf(),
            quota,
            used: Mutex::new(None),
            cache: Mutex::new(None),
        })
    }

    /// Makes paths under `cache/` lead to `cache`, or refuses them without
    /// one.
    pub fn set_cache(&self, cache: Option<Arc<Storage>>) {
        *self.cache.lock().unwrap_or_else(PoisonError::into_inner) = cache;
    }

    /// The cache and the path inside it when `path` leads there.
    ///
    /// # Errors
    /// A path under `cache/` without a cache.
    fn route<'a>(&self, path: &'a str) -> Result<Option<(Arc<Storage>, &'a str)>> {
        let rest = if path == CACHE {
            ""
        } else if let Some(rest) = path.strip_prefix("cache/") {
            rest
        } else {
            return Ok(None);
        };
        let cache = self
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| {
                Error::new(
                    Kind::PermissionDenied,
                    "cache/ is the plugin's cache, which needs storage.cache",
                )
            })?;
        Ok(Some((cache, rest)))
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The location of `path`, refusing paths that leave the folder,
    /// including through links.
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        if !valid_path(path) {
            return Err(Error::invalid(format!(
                "{path:?} is not a valid storage path"
            )));
        }
        let mut location = self.root.clone();
        for part in path.split('/') {
            location.push(part);
            if let Ok(metadata) = fs::symlink_metadata(&location)
                && metadata.file_type().is_symlink()
            {
                return Err(Error::invalid(format!("{path}: links are not followed")));
            }
        }
        Ok(location)
    }

    fn directory(&self, path: &str) -> Result<PathBuf> {
        if path.is_empty() {
            Ok(self.root.clone())
        } else {
            self.resolve(path)
        }
    }

    fn file_size(path: &str, location: &Path) -> Result<u64> {
        let metadata = fs::metadata(location).map_err(|e| io(path, &e))?;
        if !metadata.is_file() {
            return Err(Error::invalid(format!("{path} is not a file")));
        }
        Ok(metadata.len())
    }

    /// # Errors
    /// Fails for invalid paths, missing files, and files over 64 MiB.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.read(rest);
        }
        let location = self.resolve(path)?;
        if Self::file_size(path, &location)? > MAX_READ {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("{path} is larger than 64 MiB; read it in ranges"),
            ));
        }
        fs::read(&location).map_err(|e| io(path, &e))
    }

    /// Reads the whole file without the 64 MiB limit, for the host's own
    /// use (activating a text table).
    ///
    /// # Errors
    /// Fails for invalid paths and missing files.
    pub fn read_unbounded(&self, path: &str) -> Result<Vec<u8>> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.read_unbounded(rest);
        }
        let location = self.resolve(path)?;
        Self::file_size(path, &location)?;
        fs::read(&location).map_err(|e| io(path, &e))
    }

    /// # Errors
    /// Fails for invalid paths, missing files, ranges past the end, and
    /// ranges over 16 MiB.
    pub fn read_range(&self, path: &str, offset: u64, length: u32) -> Result<Vec<u8>> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.read_range(rest, offset, length);
        }
        if length > MAX_RANGE {
            return Err(Error::new(Kind::LimitExceeded, "a range is at most 16 MiB"));
        }
        let location = self.resolve(path)?;
        let size = Self::file_size(path, &location)?;
        if offset
            .checked_add(u64::from(length))
            .is_none_or(|end| end > size)
        {
            return Err(Error::invalid(format!(
                "{path}: range {offset}+{length} extends past {size} bytes"
            )));
        }
        let mut file = fs::File::open(&location).map_err(|e| io(path, &e))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| io(path, &e))?;
        let mut bytes = vec![0; length as usize];
        file.read_exact(&mut bytes).map_err(|e| io(path, &e))?;
        Ok(bytes)
    }

    /// # Errors
    /// Fails for invalid paths and missing files.
    pub fn size(&self, path: &str) -> Result<u64> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.size(rest);
        }
        let location = self.resolve(path)?;
        Self::file_size(path, &location)
    }

    /// # Errors
    /// Fails for invalid paths and missing directories.
    pub fn list(&self, path: &str) -> Result<Vec<Entry>> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.list(rest);
        }
        let location = self.directory(path)?;
        let mut entries = Vec::new();
        if path.is_empty()
            && self
                .cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        {
            entries.push(Entry {
                name: CACHE.to_owned(),
                directory: true,
                size: 0,
            });
        }
        for item in fs::read_dir(&location).map_err(|e| io(path, &e))? {
            let item = item.map_err(|e| io(path, &e))?;
            let Ok(name) = item.file_name().into_string() else {
                continue;
            };
            let Ok(metadata) = item.metadata() else {
                continue;
            };
            if name.ends_with(TEMPORARY)
                || metadata.file_type().is_symlink()
                || (path.is_empty() && name == CACHE)
            {
                continue;
            }
            entries.push(Entry {
                name,
                directory: metadata.is_dir(),
                size: if metadata.is_dir() { 0 } else { metadata.len() },
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Charges `added` bytes against the quota, `released` bytes freed by
    /// the same operation.
    fn charge(&self, added: u64, released: u64) -> Result<()> {
        let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
        let current = match *used {
            Some(current) => current,
            None => folder_size(&self.root),
        };
        let next = current.saturating_sub(released).saturating_add(added);
        if added > released && next > self.quota {
            *used = Some(current);
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("the plugin's storage would exceed {} bytes", self.quota),
            ));
        }
        *used = Some(next);
        Ok(())
    }

    fn existing_size(location: &Path) -> u64 {
        fs::metadata(location).map_or(0, |m| if m.is_file() { m.len() } else { 0 })
    }

    fn parent(path: &str, location: &Path) -> Result<()> {
        if let Some(parent) = location.parent() {
            fs::create_dir_all(parent).map_err(|e| io(path, &e))?;
        }
        Ok(())
    }

    /// # Errors
    /// Fails for invalid paths, a full quota, and file system errors.
    pub fn write(&self, path: &str, contents: &[u8]) -> Result<()> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.write(rest, contents);
        }
        let location = self.resolve(path)?;
        self.charge(contents.len() as u64, Self::existing_size(&location))?;
        Self::parent(path, &location)?;
        let mut temporary = location.clone().into_os_string();
        temporary.push(TEMPORARY);
        let result = (|| {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(contents)?;
            file.sync_all()?;
            fs::rename(&temporary, &location)
        })();
        result.map_err(|e| {
            let _ = fs::remove_file(&temporary);
            *self.used.lock().unwrap_or_else(PoisonError::into_inner) = None;
            io(path, &e)
        })
    }

    /// Checks `path` without touching the folder.
    ///
    /// # Errors
    /// Fails for paths `write` refuses.
    pub fn check(&self, path: &str) -> Result<()> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.check(rest);
        }
        self.resolve(path).map(drop)
    }

    /// Writes a file with `fill`, which receives the temporary file and the
    /// bytes it may write: what the quota leaves, counting the file it
    /// replaces. The file is replaced only when `fill` succeeds and the
    /// written size fits the quota. Returns `fill`'s result and the size.
    ///
    /// # Errors
    /// Fails for invalid paths, a full quota, file system errors, and
    /// `fill`'s error.
    pub fn write_with<T>(
        &self,
        path: &str,
        fill: impl FnOnce(&mut fs::File, u64) -> Result<T>,
    ) -> Result<(T, u64)> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.write_with(rest, fill);
        }
        let location = self.resolve(path)?;
        let existing = Self::existing_size(&location);
        let room = {
            let mut used = self.used.lock().unwrap_or_else(PoisonError::into_inner);
            let current = used.unwrap_or_else(|| folder_size(&self.root));
            *used = Some(current);
            self.quota.saturating_sub(current).saturating_add(existing)
        };
        Self::parent(path, &location)?;
        let mut temporary = location.clone().into_os_string();
        temporary.push(TEMPORARY);
        let result = (|| {
            let mut output = fs::File::create(&temporary).map_err(|e| io(path, &e))?;
            let value = fill(&mut output, room)?;
            output.sync_all().map_err(|e| io(path, &e))?;
            let size = output.metadata().map_err(|e| io(path, &e))?.len();
            drop(output);
            self.charge(size, existing)?;
            if let Err(e) = fs::rename(&temporary, &location) {
                *self.used.lock().unwrap_or_else(PoisonError::into_inner) = None;
                return Err(io(path, &e));
            }
            Ok((value, size))
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// # Errors
    /// Fails for invalid paths, a full quota, and file system errors.
    pub fn append(&self, path: &str, contents: &[u8]) -> Result<()> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.append(rest, contents);
        }
        let location = self.resolve(path)?;
        self.charge(contents.len() as u64, 0)?;
        Self::parent(path, &location)?;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&location)
            .and_then(|mut file| file.write_all(contents))
            .map_err(|e| {
                *self.used.lock().unwrap_or_else(PoisonError::into_inner) = None;
                io(path, &e)
            })
    }

    /// # Errors
    /// Fails for invalid paths, a missing source, and file system errors.
    pub fn rename(&self, from: &str, to: &str) -> Result<()> {
        match (self.route(from)?, self.route(to)?) {
            (Some((cache, from)), Some((_, to))) => return cache.rename(from, to),
            (None, None) => {}
            _ => {
                return Err(Error::invalid(
                    "files move inside the cache or outside it, not between them",
                ));
            }
        }
        let source = self.resolve(from)?;
        let destination = self.resolve(to)?;
        Self::file_size(from, &source)?;
        let replaced = Self::existing_size(&destination);
        Self::parent(to, &destination)?;
        fs::rename(&source, &destination).map_err(|e| io(from, &e))?;
        self.charge(0, replaced)
    }

    /// # Errors
    /// Fails for invalid paths, non-empty directories, and file system
    /// errors.
    pub fn remove(&self, path: &str) -> Result<()> {
        if let Some((cache, rest)) = self.route(path)? {
            return cache.remove(rest);
        }
        let location = self.resolve(path)?;
        let metadata = match fs::symlink_metadata(&location) {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io(path, &e)),
        };
        if metadata.is_dir() {
            fs::remove_dir(&location).map_err(|e| io(path, &e))
        } else {
            fs::remove_file(&location).map_err(|e| io(path, &e))?;
            self.charge(0, metadata.len())
        }
    }
}

fn folder_size(path: &Path) -> u64 {
    fs::read_dir(path).map_or(0, |items| {
        items
            .filter_map(std::result::Result::ok)
            .map(|item| match item.metadata() {
                Ok(m) if m.is_dir() => folder_size(&item.path()),
                Ok(m) if m.is_file() => m.len(),
                _ => 0,
            })
            .sum()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_folder() {
        for path in ["a", "a/b.txt", "import/pack.hpk", "Мой файл.json"] {
            assert!(valid_path(path), "{path}");
        }
        for path in [
            "",
            "/a",
            "a/",
            "a//b",
            ".",
            "..",
            "a/../b",
            "a\\b",
            "C:/x",
            "con",
            "nul.txt",
            "Com1.log",
            "CONIN$",
            "conout$.txt",
            "lpt0",
            "com\u{b9}.log",
            "a.",
            "a ",
            "a\0b",
            "x.angelica-tmp",
        ] {
            assert!(!valid_path(path), "{path}");
        }
    }

    #[test]
    fn the_cache_is_a_folder_of_its_own_with_its_quota() {
        let folder = tempfile::tempdir().unwrap();
        let storage = Storage::new(&folder.path().join("storage"), 4).unwrap();
        assert_eq!(
            storage.write("cache/a", b"1").unwrap_err().kind,
            Kind::PermissionDenied
        );
        let cache = Storage::new(&folder.path().join("cache"), 8).unwrap();
        storage.set_cache(Some(Arc::new(cache)));
        storage.write("cache/a/b", b"12345678").unwrap();
        assert_eq!(
            storage.write("c", b"12345").unwrap_err().kind,
            Kind::LimitExceeded,
            "the folder keeps its own quota"
        );
        assert_eq!(
            fs::read(folder.path().join("cache/a/b")).unwrap(),
            b"12345678"
        );
        assert_eq!(storage.read_range("cache/a/b", 2, 2).unwrap(), b"34");
        let names: Vec<_> = storage
            .list("")
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["cache"]);
        let names: Vec<_> = storage
            .list("cache")
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["a"]);
        assert_eq!(
            storage.write("cache/x", b"1").unwrap_err().kind,
            Kind::LimitExceeded
        );
        storage.write("d", b"1").unwrap();
        assert_eq!(
            storage.rename("d", "cache/d").unwrap_err().kind,
            Kind::InvalidArgument
        );
        storage.rename("cache/a/b", "cache/e").unwrap();
        storage.remove("cache/e").unwrap();
        storage.write("cache/x", b"1").unwrap();
        assert!(storage.read("cache").is_err());
        storage.set_cache(None);
        assert_eq!(
            storage.read("cache/x").unwrap_err().kind,
            Kind::PermissionDenied
        );
    }

    #[test]
    fn files_are_written_atomically_within_the_quota() {
        let folder = tempfile::tempdir().unwrap();
        let storage = Storage::new(folder.path(), 10).unwrap();
        storage.write("dir/a", b"12345").unwrap();
        assert_eq!(storage.read("dir/a").unwrap(), b"12345");
        storage.write("dir/a", b"1234567890").unwrap();
        assert_eq!(
            storage.write("b", b"1").unwrap_err().kind,
            Kind::LimitExceeded
        );
        storage.remove("dir/a").unwrap();
        storage.append("b", b"12").unwrap();
        storage.append("b", b"34").unwrap();
        assert_eq!(storage.read_range("b", 1, 2).unwrap(), b"23");
        assert!(storage.read_range("b", 3, 2).is_err());
        storage.rename("b", "c/d").unwrap();
        assert_eq!(storage.size("c/d").unwrap(), 4);
        assert_eq!(storage.read("b").unwrap_err().kind, Kind::NotFound);
        let names: Vec<_> = storage
            .list("")
            .unwrap()
            .into_iter()
            .map(|e| (e.name, e.directory))
            .collect();
        assert_eq!(names, [("c".to_owned(), true), ("dir".to_owned(), true)]);
        storage.remove("missing").unwrap();
        assert!(storage.remove("c").is_err(), "not empty");
    }
}
