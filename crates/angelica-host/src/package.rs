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
        let key = VerifyingKey::from_bytes(
            public_key.ok_or_else(|| PackageError("no compiled release key".into()))?,
        )
        .map_err(|e| PackageError(e.to_string()))?;
        let manifest = read_limited(&directory.join("files.json"), 1024 * 1024)
            .map_err(|e| PackageError(format!("files.json: {e}")))?;
        let signature = read_limited(&directory.join("files.json.sig"), 64)
            .map_err(|e| PackageError(format!("files.json.sig: {e}")))?;
        let signature =
            Signature::from_slice(&signature).map_err(|e| PackageError(e.to_string()))?;
        key.verify_strict(&manifest, &signature)
            .map_err(|_| PackageError("files.json signature failed".into()))?;
        let set: Self =
            serde_json::from_slice(&manifest).map_err(|e| PackageError(e.to_string()))?;
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
