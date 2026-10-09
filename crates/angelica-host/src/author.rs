//! Plugin files signed by their authors (`.angelica`), for plugins
//! distributed outside the official catalog: a manifest and a component,
//! never native code. The signature is verified before anything is parsed.
//! See `docs/formats/plugin-file-v1.md`.
#![forbid(unsafe_code)]
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The first bytes of every plugin file.
pub const MAGIC: &[u8; 16] = b"angelica-plugin\n";
pub const FORMAT: u32 = 1;
pub const MAX_MANIFEST: usize = 64 * 1024;
pub const MAX_COMPONENT: usize = 64 * 1024 * 1024;
/// The largest plugin file.
pub const MAX_FILE: usize = HEAD + 8 + MAX_MANIFEST + MAX_COMPONENT;

/// Magic, format, author key, signature.
const HEAD: usize = 16 + 4 + 32 + 64;
const KEY: std::ops::Range<usize> = 20..52;
const SIGNATURE: std::ops::Range<usize> = 52..116;

/// A verified plugin file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFile {
    /// The author's public key: the plugin's identity across versions.
    pub author: [u8; 32],
    pub manifest: Vec<u8>,
    pub component: Vec<u8>,
}

/// Signs a manifest and a component into a plugin file.
///
/// # Errors
/// When the manifest or component is larger than the format allows.
pub fn sign(key: &SigningKey, manifest: &[u8], component: &[u8]) -> Result<Vec<u8>, String> {
    if manifest.len() > MAX_MANIFEST || component.len() > MAX_COMPONENT {
        return Err(format!(
            "a plugin file holds a manifest of at most {MAX_MANIFEST} bytes and a component of at most {MAX_COMPONENT} bytes"
        ));
    }
    let mut file = Vec::with_capacity(HEAD + 8 + manifest.len() + component.len());
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&FORMAT.to_le_bytes());
    file.extend_from_slice(key.verifying_key().as_bytes());
    file.extend_from_slice(&[0; 64]);
    for part in [manifest, component] {
        let length = u32::try_from(part.len()).map_err(|e| e.to_string())?;
        file.extend_from_slice(&length.to_le_bytes());
        file.extend_from_slice(part);
    }
    let signature = key.sign(&signed(&file));
    file[SIGNATURE].copy_from_slice(&signature.to_bytes());
    Ok(file)
}

/// Verifies a plugin file and returns its parts.
///
/// # Errors
/// Names a file that is not a plugin file, another format, a signature
/// that does not verify, and a body that does not match its lengths.
pub fn open(file: &[u8]) -> Result<PluginFile, String> {
    if file.len() < HEAD || &file[..16] != MAGIC {
        return Err("not an Angelica plugin file".into());
    }
    if file.len() > MAX_FILE {
        return Err("the plugin file is too large".into());
    }
    let format = u32::from_le_bytes([file[16], file[17], file[18], file[19]]);
    if format != FORMAT {
        return Err(format!("plugin file format {format} is not supported"));
    }
    let mut author = [0; 32];
    author.copy_from_slice(&file[KEY]);
    let key = VerifyingKey::from_bytes(&author).map_err(|_| "the author key is invalid")?;
    let signature = Signature::from_slice(&file[SIGNATURE]).map_err(|e| e.to_string())?;
    key.verify_strict(&signed(file), &signature)
        .map_err(|_| "the author's signature does not verify")?;
    let mut rest = &file[HEAD..];
    let mut part = |limit: usize| -> Result<Vec<u8>, String> {
        let (length, tail) = rest
            .split_at_checked(4)
            .ok_or("the plugin file is truncated")?;
        let length = u32::from_le_bytes([length[0], length[1], length[2], length[3]]) as usize;
        if length > limit {
            return Err("a part of the plugin file is too large".into());
        }
        let (bytes, tail) = tail
            .split_at_checked(length)
            .ok_or("the plugin file is truncated")?;
        rest = tail;
        Ok(bytes.to_vec())
    };
    let manifest = part(MAX_MANIFEST)?;
    let component = part(MAX_COMPONENT)?;
    if !rest.is_empty() {
        return Err("the plugin file has bytes after its component".into());
    }
    Ok(PluginFile {
        author,
        manifest,
        component,
    })
}

/// Everything but the signature: magic, format, key, and body.
fn signed(file: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(file.len() - 64);
    bytes.extend_from_slice(&file[..SIGNATURE.start]);
    bytes.extend_from_slice(&file[SIGNATURE.end..]);
    bytes
}

/// The author a plugin id is bound to in an installation:
/// `keys/authors/<plugin-id>.json`. The first installation binds it; a
/// file signed by another author is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub format: u32,
    pub plugin_id: String,
    /// The author's public key, lowercase hex.
    pub author_key: String,
}

impl Pin {
    #[must_use]
    pub fn new(plugin_id: &str, author: &[u8; 32]) -> Self {
        Self {
            format: 1,
            plugin_id: plugin_id.into(),
            author_key: crate::package::hex(author),
        }
    }

    /// The pin of `plugin_id` in the keys folder, if there is one.
    ///
    /// # Errors
    /// An unreadable or malformed pin, or one for another plugin.
    pub fn read(keys: &Path, plugin_id: &str) -> Result<Option<Self>, String> {
        let path = keys.join("authors").join(format!("{plugin_id}.json"));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let pin: Self =
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        if pin.format != 1 || pin.plugin_id != plugin_id {
            return Err(format!("{} is not a pin of {plugin_id}", path.display()));
        }
        Ok(Some(pin))
    }

    /// Whether the pin names this author.
    #[must_use]
    pub fn names(&self, author: &[u8; 32]) -> bool {
        self.author_key == crate::package::hex(author)
    }
}

/// The installation's public key in the keys folder (`installation.pub`, 64
/// hexadecimal digits), which signs packages of plugins from their authors;
/// `None` when the installation has none yet.
///
/// # Errors
/// An unreadable or malformed key file.
pub fn installation_key(keys: &Path) -> Result<Option<[u8; 32]>, String> {
    let path = keys.join("installation.pub");
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_key(text.trim())
            .map(Some)
            .ok_or_else(|| format!("{}: a key is 64 hexadecimal digits", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// 32 bytes from 64 hexadecimal digits.
#[must_use]
pub fn parse_key(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn a_signed_file_opens_with_its_parts_and_author() {
        let file = sign(&key(1), b"manifest = 1", b"\0asm").unwrap();
        let opened = open(&file).unwrap();
        assert_eq!(opened.manifest, b"manifest = 1");
        assert_eq!(opened.component, b"\0asm");
        assert_eq!(&opened.author, key(1).verifying_key().as_bytes());
    }

    #[test]
    fn changed_or_damaged_files_are_refused() {
        let file = sign(&key(1), b"manifest = 1", b"\0asm").unwrap();
        // Any changed byte, including the key, breaks the signature or the format.
        for at in 0..file.len() {
            let mut changed = file.clone();
            changed[at] ^= 1;
            assert!(open(&changed).is_err(), "byte {at}");
        }
        // Another author's key with the old signature.
        let mut other = file.clone();
        other[KEY].copy_from_slice(key(2).verifying_key().as_bytes());
        assert!(open(&other).is_err());
        for length in [0, 15, HEAD, file.len() - 1] {
            assert!(open(&file[..length]).is_err(), "length {length}");
        }
        let mut longer = file.clone();
        longer.push(0);
        assert!(open(&longer).is_err());
    }

    #[test]
    fn keys_and_pins_read_back() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(installation_key(folder.path()), Ok(None));
        let public = key(3).verifying_key().to_bytes();
        std::fs::write(
            folder.path().join("installation.pub"),
            crate::package::hex(&public),
        )
        .unwrap();
        assert_eq!(installation_key(folder.path()), Ok(Some(public)));

        assert_eq!(Pin::read(folder.path(), "timers"), Ok(None));
        std::fs::create_dir(folder.path().join("authors")).unwrap();
        let pin = Pin::new("timers", &public);
        std::fs::write(
            folder.path().join("authors/timers.json"),
            serde_json::to_vec(&pin).unwrap(),
        )
        .unwrap();
        let read = Pin::read(folder.path(), "timers").unwrap().unwrap();
        assert!(read.names(&public));
        assert!(!read.names(&[0; 32]));
        std::fs::copy(
            folder.path().join("authors/timers.json"),
            folder.path().join("authors/other.json"),
        )
        .unwrap();
        assert!(Pin::read(folder.path(), "other").is_err());
    }
}
