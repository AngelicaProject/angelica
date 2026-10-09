//! Game files plugins replace (the `file-replacements` interface). A
//! replacement is checked against the game's original, copied into the
//! host's cache under a short name, and served from there when the game
//! loads the file.
#![forbid(unsafe_code)]
use crate::{
    error::{Error, Kind, Result},
    file_formats,
    game_data::GameData,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, PoisonError, RwLock},
};

/// Files one plugin can replace.
pub const MAX_PER_PLUGIN: usize = 1024;
/// The longest cache path the game's disk reader takes, in UTF-16 units
/// before the terminator.
pub const MAX_CACHE_PATH: usize = 259;

/// A served replacement.
pub struct Served {
    pub plugin: String,
    /// The cache file, as the game's disk reader takes it: UTF-16 with a
    /// terminating zero.
    pub path: Arc<[u16]>,
}

pub struct FileReplacements {
    cache: PathBuf,
    files: RwLock<BTreeMap<String, Served>>,
}

/// The game path as the game names resources: lower case, `/`.
fn normalize(path: &str) -> Result<String> {
    let path = path.trim().to_ascii_lowercase();
    if path.is_empty()
        || path.len() > 255
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains("..")
    {
        return Err(Error::invalid(format!("{path:?} is not a game path")));
    }
    Ok(path)
}

impl FileReplacements {
    /// Replacements cached in `cache`.
    #[must_use]
    pub fn new(cache: &Path) -> Self {
        Self {
            cache: cache.to_path_buf(),
            files: RwLock::new(BTreeMap::new()),
        }
    }

    /// Serves `contents` for the game file at `path` from now on.
    ///
    /// # Errors
    /// Refuses a file the game does not have, a format the host cannot
    /// check, contents that fail the check, a file another plugin replaces,
    /// too many replacements, and a cache path the game cannot read.
    pub fn replace(
        &self,
        plugin: &str,
        path: &str,
        contents: &[u8],
        data: &GameData,
    ) -> Result<()> {
        let path = normalize(path)?;
        if let Some(served) = self
            .files
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&path)
            && served.plugin != plugin
        {
            return Err(Error::invalid(format!(
                "{path} is already replaced by {}",
                served.plugin
            )));
        }
        let original = data.read(&path)?;
        file_formats::check(&path, &original, contents)
            .map_err(|reason| Error::invalid(format!("{path}: {reason}")))?;

        let hash = Sha256::digest(contents);
        let extension = path
            .rsplit_once('.')
            .map_or("bin", |(_, extension)| extension);
        let name = format!("{}.{extension}", crate::package::hex(&hash[..16]));
        std::fs::create_dir_all(&self.cache)
            .map_err(|e| Error::new(Kind::Io, format!("{}: {e}", self.cache.display())))?;
        let file = self.cache.join(name);
        let wide: Vec<u16> = file.as_os_str().to_string_lossy().encode_utf16().collect();
        if wide.len() > MAX_CACHE_PATH {
            return Err(Error::new(
                Kind::Unavailable,
                format!("the host's cache path is longer than {MAX_CACHE_PATH} characters"),
            ));
        }
        if std::fs::read(&file).ok().as_deref() != Some(contents) {
            let temporary = file.with_extension("tmp");
            std::fs::write(&temporary, contents)
                .and_then(|()| std::fs::rename(&temporary, &file))
                .map_err(|e| Error::new(Kind::Io, format!("{}: {e}", file.display())))?;
        }
        let mut files = self.files.write().unwrap_or_else(PoisonError::into_inner);
        if !files.contains_key(&path)
            && files.values().filter(|s| s.plugin == plugin).count() >= MAX_PER_PLUGIN
        {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a plugin replaces at most {MAX_PER_PLUGIN} files"),
            ));
        }
        if let Some(served) = files.get(&path)
            && served.plugin != plugin
        {
            return Err(Error::invalid(format!(
                "{path} is already replaced by {}",
                served.plugin
            )));
        }
        let mut terminated = wide;
        terminated.push(0);
        files.insert(
            path,
            Served {
                plugin: plugin.into(),
                path: terminated.into(),
            },
        );
        Ok(())
    }

    /// Serves the game's own file again; false when the plugin did not
    /// replace it.
    pub fn restore(&self, plugin: &str, path: &str) -> bool {
        let Ok(path) = normalize(path) else {
            return false;
        };
        let mut files = self.files.write().unwrap_or_else(PoisonError::into_inner);
        if files
            .get(&path)
            .is_some_and(|served| served.plugin == plugin)
        {
            files.remove(&path);
            true
        } else {
            false
        }
    }

    pub fn remove_plugin(&self, plugin: &str) {
        self.files
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|_, served| served.plugin != plugin);
    }

    /// The cache file to serve for a game path the game is loading.
    #[must_use]
    pub fn lookup(&self, path: &[u8]) -> Option<Arc<[u16]>> {
        let files = self.files.read().unwrap_or_else(PoisonError::into_inner);
        if files.is_empty() {
            return None;
        }
        let path = std::str::from_utf8(path).ok()?.to_ascii_lowercase();
        files.get(&path).map(|served| served.path.clone())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_sqpack::testing::FakeGame;

    #[test]
    fn replacements_are_checked_cached_and_owned() {
        let folder = tempfile::tempdir().unwrap();
        let mut original = vec![0u8; 80 + 16];
        original[4] = 0x40;
        FakeGame::new("1")
            .with_file("common/font/font1.tex", original.clone())
            .write(&folder.path().join("game"))
            .unwrap();
        let data = GameData::new(Some(&folder.path().join("game")));
        let files = FileReplacements::new(&folder.path().join("cache"));

        let mut painted = original.clone();
        painted[90] = 0xFF;
        files
            .replace("a", "common/font/font1.tex", &painted, &data)
            .unwrap();
        let served = files.lookup(b"common/font/font1.tex").unwrap();
        let cache = String::from_utf16(&served[..served.len() - 1]).unwrap();
        assert_eq!(std::fs::read(cache).unwrap(), painted);
        assert_eq!(*served.last().unwrap(), 0);

        assert!(
            files
                .replace("b", "common/font/font1.tex", &painted, &data)
                .is_err(),
            "owned by a"
        );
        assert!(
            files
                .replace("a", "common/font/font1.tex", &painted[..90], &data)
                .is_err(),
            "size"
        );
        assert!(
            files
                .replace("a", "common/font/missing.tex", &painted, &data)
                .is_err(),
            "no original"
        );
        assert!(files.replace("a", "../x.tex", &painted, &data).is_err());
        assert!(
            files.lookup(b"common/font/font1.tex").is_some(),
            "the earlier replacement stays"
        );

        assert!(!files.restore("b", "common/font/font1.tex"));
        assert!(files.restore("a", "COMMON/font/font1.tex"));
        assert!(files.lookup(b"common/font/font1.tex").is_none());
        files
            .replace("a", "common/font/font1.tex", &painted, &data)
            .unwrap();
        files.remove_plugin("a");
        assert!(files.is_empty());
    }
}
