//! Signed file sets. Verification precedes parsing or component compilation.
#![forbid(unsafe_code)]
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Component, Path},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSet {
    pub format: u32,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
#[error("signed package rejected: {0}")]
pub struct PackageError(pub String);

impl FileSet {
    /// Verifies a detached signature and every content hash before returning owned bytes.
    ///
    /// # Errors
    /// Rejects absent keys, invalid signatures, traversal, symlinks and modified files.
    pub fn load(
        directory: &Path,
        public_key: Option<&[u8; 32]>,
    ) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
        let keys: Vec<[u8; 32]> = public_key.copied().into_iter().collect();
        Self::load_by(directory, &keys).map(|(_, files)| files)
    }

    /// As [`FileSet::load`], with the first of `keys` whose signature
    /// verifies; returns its index.
    ///
    /// # Errors
    /// As [`FileSet::load`]; a file list none of the keys signed.
    pub fn load_by(
        directory: &Path,
        keys: &[[u8; 32]],
    ) -> Result<(usize, BTreeMap<String, Vec<u8>>), PackageError> {
        if keys.is_empty() {
            return Err(PackageError("no compiled release key".into()));
        }
        let manifest = read_limited(&directory.join("files.json"), 1024 * 1024)
            .map_err(|e| PackageError(format!("files.json: {e}")))?;
        let signature = read_limited(&directory.join("files.json.sig"), 64)
            .map_err(|e| PackageError(format!("files.json.sig: {e}")))?;
        let signature =
            Signature::from_slice(&signature).map_err(|e| PackageError(e.to_string()))?;
        let signer = keys
            .iter()
            .position(|key| {
                VerifyingKey::from_bytes(key)
                    .is_ok_and(|key| key.verify_strict(&manifest, &signature).is_ok())
            })
            .ok_or_else(|| PackageError("files.json signature failed".into()))?;
        Ok((signer, Self::read_listed(directory, &manifest)?))
    }

    /// The files `files.json` lists, each checked against its hash, with
    /// no signature: only for the local map of dev mode, which the player
    /// put there with `angelica map try`.
    ///
    /// # Errors
    /// As [`FileSet::load`], except that no signature is asked for.
    pub fn load_unsigned(directory: &Path) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
        let manifest = read_limited(&directory.join("files.json"), 1024 * 1024)
            .map_err(|e| PackageError(format!("files.json: {e}")))?;
        Self::read_listed(directory, &manifest)
    }

    /// Every file a file list names, checked against its hash.
    fn read_listed(
        directory: &Path,
        manifest: &[u8],
    ) -> Result<BTreeMap<String, Vec<u8>>, PackageError> {
        let set: Self =
            serde_json::from_slice(manifest).map_err(|e| PackageError(e.to_string()))?;
        if set.format != 1 || set.files.is_empty() || set.files.len() > 8192 {
            return Err(PackageError("unsupported format or empty file set".into()));
        }
        let mut bytes = BTreeMap::new();
        let mut total = 0usize;
        for (name, expected) in set.files {
            if !valid_relative_path(&name) {
                return Err(PackageError(format!("invalid package path {name}")));
            }
            let mut path = directory.to_path_buf();
            for part in name.split('/') {
                path.push(part);
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|e| PackageError(format!("{name}: {e}")))?;
                if metadata.file_type().is_symlink() {
                    return Err(PackageError(format!("symlink in package path {name}")));
                }
            }
            let content = read_limited(&path, 64 * 1024 * 1024)
                .map_err(|e| PackageError(format!("{name}: {e}")))?;
            total = total
                .checked_add(content.len())
                .ok_or_else(|| PackageError("package size overflow".into()))?;
            if total > 256 * 1024 * 1024 {
                return Err(PackageError("package exceeds 256 MiB".into()));
            }
            let actual = hex(&Sha256::digest(&content));
            if expected != actual {
                return Err(PackageError(format!("content hash mismatch for {name}")));
            }
            bytes.insert(name, content);
        }
        Ok(bytes)
    }
}

fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, std::io::Error> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(std::io::Error::other("symlinks are not allowed"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other("file exceeds package limit"));
    }
    Ok(bytes)
}

#[must_use]
pub fn valid_relative_path(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':', '\0'])
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && name.split('/').all(|part| {
            if part.is_empty() || part == "." || part == ".." || part.ends_with(['.', ' ']) {
                return false;
            }
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        },
    )
}

/// The keys plugin packages may be signed with.
#[derive(Debug, Clone, Default)]
pub struct Trust {
    /// The release key compiled into the host: packages of the official
    /// catalog.
    pub release: Option<[u8; 32]>,
    /// The installation's key: packages the installer made on this
    /// computer from plugin files signed by their authors.
    pub installation: Option<[u8; 32]>,
    /// The installation's keys folder, with the authors plugins are bound to.
    pub keys: std::path::PathBuf,
}

/// A plugin package whose every byte is authenticated.
#[derive(Debug)]
pub struct PluginPackage {
    pub manifest: crate::manifest::Manifest,
    /// The compiled component, `plugin.cwasm`.
    pub artifact: Vec<u8>,
    /// The installer compiled the component on this computer from a plugin
    /// file, so a runtime that cannot load it compiles it again.
    pub compiled_here: bool,
}

/// The plugin's own Anatomia entries: the files under `anatomia/`, whose
/// namespace is the plugin id; none without `anatomia/anatomia.toml`.
///
/// # Errors
/// Entries that cannot be read at all, another namespace, and a file under
/// `anatomia/` that is not UTF-8.
pub fn own_entries(
    files: &std::collections::BTreeMap<String, Vec<u8>>,
    id: &str,
) -> Result<Option<angelica_anatomia::Anatomia>, String> {
    let Some(manifest) = files.get("anatomia/anatomia.toml") else {
        if files.keys().any(|path| path.starts_with("anatomia/")) {
            return Err("anatomia/ has no anatomia.toml".into());
        }
        return Ok(None);
    };
    let text = |path: &str, bytes: &[u8]| {
        String::from_utf8(bytes.to_vec()).map_err(|_| format!("{path} is not UTF-8"))
    };
    let manifest = text("anatomia/anatomia.toml", manifest)?;
    let sources = files
        .iter()
        .filter(|(path, _)| path.starts_with("anatomia/") && *path != "anatomia/anatomia.toml")
        .map(|(path, bytes)| Ok((path["anatomia/".len()..].to_owned(), text(path, bytes)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let anatomia = angelica_anatomia::Anatomia::from_sources(&manifest, &sources)
        .map_err(|error| format!("anatomia: {error}"))?;
    if anatomia.namespace != id {
        return Err(format!(
            "anatomia: the namespace is {}, not the plugin id {id}",
            anatomia.namespace
        ));
    }
    Ok(Some(anatomia))
}

/// Authenticates the plugin package in `folder`: a package signed with the
/// release key, or one signed with the installation key that holds the
/// author's plugin file, whose author is the one the plugin is bound to.
///
/// # Errors
/// A signature or hash that does not verify, a missing or unexpected file,
/// a malformed manifest, a folder not named by the plugin id, and an author
/// other than the bound one.
pub fn authenticate_plugin(folder: &Path, trust: &Trust) -> Result<PluginPackage, String> {
    use crate::manifest::{Manifest, Origin};
    let keys: Vec<[u8; 32]> = trust
        .release
        .iter()
        .chain(&trust.installation)
        .copied()
        .collect();
    let (signer, mut files) = FileSet::load_by(folder, &keys).map_err(|e| e.to_string())?;
    let from_author = trust.release != Some(keys[signer]);
    let (manifest, origin, icon) = if from_author {
        if files.len() != 2 {
            return Err("a package from its author holds plugin.angelica and plugin.cwasm".into());
        }
        let file = files
            .remove("plugin.angelica")
            .ok_or("the package has no plugin.angelica")?;
        let file = crate::author::open(&file)?;
        let manifest = String::from_utf8(file.manifest).map_err(|e| e.to_string())?;
        let manifest = Manifest::parse(&manifest).map_err(|e| e.to_string())?;
        let pin = crate::author::Pin::read(&trust.keys, manifest.id())?;
        if !pin.is_some_and(|pin| pin.names(&file.author)) {
            return Err(format!(
                "{} is signed by another author than the one it was installed from",
                manifest.id()
            ));
        }
        // A plugin file the catalog built and signed with the release key.
        let origin = if trust.release == Some(file.author) {
            Origin::Catalog
        } else {
            Origin::Author
        };
        (manifest, origin, file.icon)
    } else {
        let manifest = std::str::from_utf8(
            files
                .get("angelica.toml")
                .ok_or("the package has no angelica.toml")?,
        )
        .map_err(|e| e.to_string())?;
        let origin = if files.contains_key("development") {
            Origin::Development
        } else {
            Origin::Catalog
        };
        let icon = files.get("icon.png").cloned();
        if let Some(icon) = &icon {
            crate::author::check_icon(icon)?;
        }
        (
            Manifest::parse(manifest).map_err(|e| e.to_string())?,
            origin,
            icon,
        )
    };
    let mut manifest = manifest;
    manifest.origin = origin;
    manifest.icon = icon.map(std::sync::Arc::from);
    manifest.anatomia = own_entries(&files, manifest.id())?.map(std::sync::Arc::new);
    if folder.file_name() != Some(std::ffi::OsStr::new(manifest.id())) {
        return Err(format!(
            "the folder name differs from the plugin id {}",
            manifest.id()
        ));
    }
    let artifact = files
        .remove("plugin.cwasm")
        .ok_or("the package has no compiled component; compile it outside the game first")?;
    Ok(PluginPackage {
        manifest,
        artifact,
        compiled_here: from_author,
    })
}

#[cfg(test)]
mod tests {
    use super::own_entries;
    use std::collections::BTreeMap;

    fn files(entries: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
        entries
            .iter()
            .map(|(path, text)| ((*path).to_owned(), text.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn own_entries_are_in_the_plugins_namespace() {
        let manifest = "format = \"1.0\"\nnamespace = \"typing-fold\"\n";
        let function = "[function.\"game.first\"]\ntype = \"fn()\"\nthread = \"main\"\nfind = [{ scan = \"C3\" }]\n";
        let found = own_entries(
            &files(&[
                ("angelica.toml", "ignored"),
                ("anatomia/anatomia.toml", manifest),
                ("anatomia/functions/game.toml", function),
            ]),
            "typing-fold",
        )
        .unwrap()
        .unwrap();
        assert!(found.get("game.first").is_some());

        assert!(
            own_entries(&files(&[("angelica.toml", "")]), "typing-fold")
                .unwrap()
                .is_none()
        );
        let other =
            own_entries(&files(&[("anatomia/anatomia.toml", manifest)]), "hello").unwrap_err();
        assert!(other.contains("not the plugin id hello"), "{other}");
        let headless = own_entries(
            &files(&[("anatomia/functions/game.toml", function)]),
            "typing-fold",
        )
        .unwrap_err();
        assert!(headless.contains("no anatomia.toml"), "{headless}");
    }
}
