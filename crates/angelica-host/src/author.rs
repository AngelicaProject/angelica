//! Plugin files signed by their authors (`.angelica`), for plugins
//! distributed outside the official catalog: a manifest, a component, and
//! optionally an icon, never native code. The signature is verified before
//! anything is parsed. See `docs/formats/plugin-file-v1.md`.
#![forbid(unsafe_code)]
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The first bytes of every plugin file.
pub const MAGIC: &[u8; 16] = b"angelica-plugin\n";
pub const FORMAT: u32 = 1;
pub const MAX_MANIFEST: usize = 64 * 1024;
pub const MAX_COMPONENT: usize = 64 * 1024 * 1024;
pub const MAX_ICON: usize = 256 * 1024;
/// The sides an icon may have, in pixels; it is square.
pub const ICON_SIDES: std::ops::RangeInclusive<u32> = 32..=512;
/// The largest plugin file.
pub const MAX_FILE: usize = HEAD + 12 + MAX_MANIFEST + MAX_COMPONENT + MAX_ICON;

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
    /// The plugin's icon, a square PNG ([`check_icon`]).
    pub icon: Option<Vec<u8>>,
}

/// Signs a manifest and a component into a plugin file.
///
/// # Errors
/// When the manifest or component is larger than the format allows.
pub fn sign(key: &SigningKey, manifest: &[u8], component: &[u8]) -> Result<Vec<u8>, String> {
    sign_with_icon(key, manifest, component, None)
}

/// Signs a manifest, a component, and an icon into a plugin file.
///
/// # Errors
/// When a part is larger than the format allows, or the icon is not one
/// ([`check_icon`]).
pub fn sign_with_icon(
    key: &SigningKey,
    manifest: &[u8],
    component: &[u8],
    icon: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    if manifest.len() > MAX_MANIFEST || component.len() > MAX_COMPONENT {
        return Err(format!(
            "a plugin file holds a manifest of at most {MAX_MANIFEST} bytes and a component of at most {MAX_COMPONENT} bytes"
        ));
    }
    if let Some(icon) = icon {
        check_icon(icon)?;
    }
    let mut file = Vec::with_capacity(
        HEAD + 12 + manifest.len() + component.len() + icon.map_or(0, <[u8]>::len),
    );
    file.extend_from_slice(MAGIC);
    file.extend_from_slice(&FORMAT.to_le_bytes());
    file.extend_from_slice(key.verifying_key().as_bytes());
    file.extend_from_slice(&[0; 64]);
    for part in [manifest, component].into_iter().chain(icon) {
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
    let manifest = part(&mut rest, MAX_MANIFEST)?;
    let component = part(&mut rest, MAX_COMPONENT)?;
    let icon = if rest.is_empty() {
        None
    } else {
        let icon = part(&mut rest, MAX_ICON)?;
        check_icon(&icon)?;
        Some(icon)
    };
    if !rest.is_empty() {
        return Err("the plugin file has bytes after its icon".into());
    }
    Ok(PluginFile {
        author,
        manifest,
        component,
        icon,
    })
}

/// The next part of a plugin file's body: its length, then at most `limit`
/// bytes.
fn part(rest: &mut &[u8], limit: usize) -> Result<Vec<u8>, String> {
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
    *rest = tail;
    Ok(bytes.to_vec())
}

/// Checks that `icon` is a plugin's icon: a PNG of at most [`MAX_ICON`]
/// bytes, square, with sides in [`ICON_SIDES`].
///
/// # Errors
/// Names what breaks the rule.
pub fn check_icon(icon: &[u8]) -> Result<(), String> {
    decode_icon(icon).map(|_| ())
}

/// The pixels of a plugin's icon: its side and its rows of RGBA.
///
/// # Errors
/// As [`check_icon`].
pub fn decode_icon(icon: &[u8]) -> Result<(u32, Vec<u8>), String> {
    let wrong = |reason: &str| format!("the plugin's icon {reason}");
    if icon.len() > MAX_ICON {
        return Err(wrong(&format!("is larger than {MAX_ICON} bytes")));
    }
    let mut decoder = png::Decoder::new_with_limits(
        std::io::Cursor::new(icon),
        png::Limits {
            bytes: 4 * 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| wrong(&format!("is not a PNG: {e}")))?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width != height || !ICON_SIDES.contains(&width) {
        return Err(wrong(&format!(
            "is {width}×{height}; it is square, {} to {} pixels",
            ICON_SIDES.start(),
            ICON_SIDES.end()
        )));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| wrong("is too large to decode"))?;
    let mut buffer = vec![0; size];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|e| wrong(&format!("does not decode: {e}")))?;
    buffer.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(wrong("keeps a palette after decoding")),
    };
    Ok((width, rgba))
}

/// The side a plugin's icon is kept at for the menu and the catalog's
/// index: larger than the menu ever draws it.
pub const ICON_PIXELS: usize = 64;

/// A square RGBA image of `side` made at most `to` pixels a side, each
/// pixel the average of the pixels it covers, weighted by their alpha.
#[must_use]
pub fn shrink_icon(side: usize, rgba: &[u8], to: usize) -> (usize, Vec<u8>) {
    if side <= to {
        return (side, rgba.to_vec());
    }
    let mut out = Vec::with_capacity(to * to * 4);
    for y in 0..to {
        for x in 0..to {
            let (mut sum, mut alpha, mut count) = ([0u64; 3], 0u64, 0u64);
            for sy in y * side / to..(y + 1) * side / to {
                for sx in x * side / to..(x + 1) * side / to {
                    let at = (sy * side + sx) * 4;
                    let a = u64::from(rgba[at + 3]);
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u64::from(rgba[at + channel]) * a;
                    }
                    alpha += a;
                    count += 1;
                }
            }
            for total in sum {
                out.push(num_traits::cast(total.checked_div(alpha).unwrap_or(0)).unwrap_or(0));
            }
            out.push(num_traits::cast(alpha / count.max(1)).unwrap_or(0));
        }
    }
    (to, out)
}

/// `icon` made at most [`ICON_PIXELS`] a side and written again as an RGBA
/// PNG, for the catalog's index.
///
/// # Errors
/// As [`check_icon`].
pub fn small_icon(icon: &[u8]) -> Result<Vec<u8>, String> {
    let (side, rgba) = decode_icon(icon)?;
    let (side, rgba) = shrink_icon(
        usize::try_from(side).unwrap_or_default(),
        &rgba,
        ICON_PIXELS,
    );
    let side = u32::try_from(side).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, side, side);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// The bytes of lowercase or uppercase hexadecimal `text`.
#[must_use]
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
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
    /// The link the plugin was last installed from, which updates use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Pin {
    #[must_use]
    pub fn new(plugin_id: &str, author: &[u8; 32]) -> Self {
        Self {
            format: 1,
            plugin_id: plugin_id.into(),
            author_key: crate::package::hex(author),
            url: None,
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

    /// A PNG of `width` by `height` gray pixels.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![128; (width * height) as usize])
            .unwrap();
        writer.finish().unwrap();
        bytes
    }

    #[test]
    fn an_icon_follows_the_component_and_is_checked() {
        let icon = png(64, 64);
        let file = sign_with_icon(&key(1), b"manifest = 1", b"\0asm", Some(&icon)).unwrap();
        let opened = open(&file).unwrap();
        assert_eq!(opened.component, b"\0asm");
        assert_eq!(opened.icon.as_deref(), Some(icon.as_slice()));
        let (side, rgba) = decode_icon(&icon).unwrap();
        assert_eq!(
            (side, rgba.len(), &rgba[..4]),
            (64, 64 * 64 * 4, &[128, 128, 128, 255][..])
        );
        assert_eq!(
            open(&sign(&key(1), b"m", b"c").unwrap()).unwrap().icon,
            None
        );
        let small = small_icon(&png(512, 512)).unwrap();
        assert_eq!(decode_icon(&small).unwrap().0, 64);
        assert_eq!(unhex(&crate::package::hex(&small)), Some(small));
        assert_eq!(unhex("0g"), None);

        for wrong in [
            png(64, 32),
            png(16, 16),
            png(600, 600),
            b"not a png".to_vec(),
        ] {
            assert!(check_icon(&wrong).is_err());
            assert!(sign_with_icon(&key(1), b"m", b"c", Some(&wrong)).is_err());
        }
        // The signature covers the icon, and nothing may follow it.
        let mut changed = file.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert!(open(&changed).is_err());
        let mut longer = file;
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
