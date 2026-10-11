//! Update feeds (`docs/formats/update-feed-v1.md`): the signed file that
//! names the newest release of a channel of Angelica or of the game map.
#![forbid(unsafe_code)]
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The largest file a feed may name.
pub const MAX_FILE: u64 = 256 * 1024 * 1024;
/// The largest feed.
pub const MAX_FEED: usize = 64 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("the feed's signature does not verify")]
    Signature,
    #[error("the feed is malformed: {0}")]
    Malformed(String),
    #[error("the feed is for {0}, not {1}")]
    Other(String, String),
    #[error("{0} does not match the feed")]
    Mismatch(String),
}

/// What a feed is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Product {
    Angelica,
    Map,
}

/// An update channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Nightly,
}

impl Channel {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }

    /// The release tag of `version` on this channel.
    #[must_use]
    pub fn tag(self, version: &semver::Version) -> String {
        match self {
            Self::Stable => format!("v{version}"),
            Self::Nightly => "nightly".to_owned(),
        }
    }
}

/// A file the release installs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// The release asset that holds it.
    pub asset: String,
    pub size: u64,
    /// Lowercase hexadecimal SHA-256.
    pub sha256: String,
}

impl File {
    /// Describes `bytes` as the asset `asset`.
    #[must_use]
    pub fn of(asset: &str, bytes: &[u8]) -> Self {
        Self {
            asset: asset.to_owned(),
            size: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
        }
    }

    /// # Errors
    /// When `bytes` have another size or hash than the feed lists.
    pub fn check(&self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() as u64 != self.size || hex(&Sha256::digest(bytes)) != self.sha256 {
            return Err(Error::Mismatch(self.asset.clone()));
        }
        Ok(())
    }
}

/// A verified feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feed {
    pub format: u32,
    pub product: Product,
    pub channel: Channel,
    pub version: semver::Version,
    /// RFC 3339, UTC.
    pub published: String,
    pub files: BTreeMap<String, File>,
}

impl Feed {
    /// Verifies `signature` over `json` with `key`, then reads the feed and
    /// checks that it is about `product` on `channel`.
    ///
    /// # Errors
    /// A signature that does not verify, a malformed feed, or a feed of
    /// another product or channel.
    pub fn verify(
        json: &[u8],
        signature: &[u8],
        key: &[u8; 32],
        product: Product,
        channel: Channel,
    ) -> Result<Self, Error> {
        if json.len() > MAX_FEED {
            return Err(Error::Malformed("larger than 64 KiB".into()));
        }
        let key = VerifyingKey::from_bytes(key).map_err(|_| Error::Signature)?;
        let signature = Signature::from_slice(signature).map_err(|_| Error::Signature)?;
        key.verify_strict(json, &signature)
            .map_err(|_| Error::Signature)?;
        let feed: Self =
            serde_json::from_slice(json).map_err(|e| Error::Malformed(e.to_string()))?;
        if feed.format != 1 {
            return Err(Error::Malformed(format!("format {}", feed.format)));
        }
        if feed.product != product || feed.channel != channel {
            return Err(Error::Other(
                format!("{:?} {}", feed.product, feed.channel.name()),
                format!("{product:?} {}", channel.name()),
            ));
        }
        for (name, file) in &feed.files {
            if !plain_name(name) || !plain_name(&file.asset) {
                return Err(Error::Malformed(format!("file name {name}")));
            }
            if file.size > MAX_FILE
                || file.sha256.len() != 64
                || !file
                    .sha256
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err(Error::Malformed(format!("file {name}")));
            }
        }
        Ok(feed)
    }

    /// The feed as JSON and its signature with `key`.
    ///
    /// # Panics
    /// Never: a feed always serializes.
    #[must_use]
    pub fn sign(&self, key: &SigningKey) -> (Vec<u8>, [u8; 64]) {
        let mut json = serde_json::to_vec_pretty(self).expect("a feed serializes");
        json.push(b'\n');
        let signature = ed25519_dalek::Signer::sign(key, &json).to_bytes();
        (json, signature)
    }
}

/// Of the feeds read, the newest version newer than `running`: Angelica
/// never installs an older version than the one it runs.
#[must_use]
pub fn newer<'a>(
    running: &semver::Version,
    feeds: impl IntoIterator<Item = &'a Feed>,
) -> Option<&'a Feed> {
    feeds
        .into_iter()
        .filter(|feed| feed.version > *running)
        .max_by(|a, b| a.version.cmp(&b.version))
}

/// The feed of `channel`: its release asset URL in `repository`
/// (`owner/name`).
#[must_use]
pub fn feed_url(repository: &str, product: Product, channel: Channel) -> String {
    let name = feed_name(product);
    match channel {
        Channel::Stable => {
            format!("https://github.com/{repository}/releases/latest/download/{name}")
        }
        Channel::Nightly => {
            format!("https://github.com/{repository}/releases/download/nightly/{name}")
        }
    }
}

/// The URL of a file of `feed` in `repository`.
#[must_use]
pub fn asset_url(repository: &str, feed: &Feed, file: &File) -> String {
    format!(
        "https://github.com/{repository}/releases/download/{}/{}",
        feed.channel.tag(&feed.version),
        file.asset
    )
}

/// `channel.json` or `map.json`.
#[must_use]
pub fn feed_name(product: Product) -> &'static str {
    match product {
        Product::Angelica => "channel.json",
        Product::Map => "map.json",
    }
}

/// A file name without folders, empty parts, or Windows device names.
#[must_use]
pub fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        && !name.starts_with('.')
        && !name.ends_with('.')
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The start of a pack: several files in one release asset (`map.pack`).
const PACK_MAGIC: &[u8] = b"angelica-pack 1
";
/// The most files a pack holds.
const PACK_FILES: usize = 8192;

/// Puts `files` (relative paths with `/`) into one pack: the magic, then
/// for each file its path length (`u32`, little-endian), path, size (`u64`),
/// and bytes.
///
/// # Errors
/// A path [`pack_path`] refuses.
pub fn pack(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, Error> {
    let mut out = PACK_MAGIC.to_vec();
    for (path, bytes) in files {
        if !pack_path(path) {
            return Err(Error::Malformed(format!("pack path {path}")));
        }
        let length = u32::try_from(path.len()).map_err(|_| Error::Malformed(path.clone()))?;
        out.extend_from_slice(&length.to_le_bytes());
        out.extend_from_slice(path.as_bytes());
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

/// The files of a pack, every path checked before a caller writes it.
///
/// # Errors
/// A pack that is truncated, has another magic, a refused or repeated path,
/// or more than 8192 files or [`MAX_FILE`] bytes.
pub fn unpack(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let fail = |what: &str| Error::Malformed(format!("pack: {what}"));
    let mut rest = bytes
        .strip_prefix(PACK_MAGIC)
        .ok_or_else(|| fail("not a pack"))?;
    let mut take = |count: usize| -> Result<&[u8], Error> {
        if rest.len() < count {
            return Err(fail("truncated"));
        }
        let (head, tail) = rest.split_at(count);
        rest = tail;
        Ok(head)
    };
    let mut files = BTreeMap::new();
    let mut total = 0u64;
    while let Ok(length) = take(4) {
        let length = u32::from_le_bytes(length.try_into().map_err(|_| fail("length"))?) as usize;
        if length > 512 {
            return Err(fail("path too long"));
        }
        let path = std::str::from_utf8(take(length)?)
            .map_err(|_| fail("path is not UTF-8"))?
            .to_owned();
        if !pack_path(&path) {
            return Err(Error::Malformed(format!("pack path {path}")));
        }
        let size = u64::from_le_bytes(take(8)?.try_into().map_err(|_| fail("size"))?);
        total = total.saturating_add(size);
        if total > MAX_FILE {
            return Err(fail("larger than 256 MiB"));
        }
        let content = take(usize::try_from(size).map_err(|_| fail("size"))?)?.to_vec();
        if files.insert(path.clone(), content).is_some() {
            return Err(Error::Malformed(format!("pack path {path} twice")));
        }
        if files.len() > PACK_FILES {
            return Err(fail("more than 8192 files"));
        }
    }
    if !rest.is_empty() {
        return Err(fail("truncated"));
    }
    Ok(files)
}

/// A relative path of plain names joined by `/`.
#[must_use]
pub fn pack_path(path: &str) -> bool {
    !path.is_empty() && path.split('/').all(plain_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(channel: Channel, version: &str) -> Feed {
        Feed {
            format: 1,
            product: Product::Angelica,
            channel,
            version: version.parse().unwrap(),
            published: "2026-10-20T12:00:00Z".into(),
            files: BTreeMap::from([("runtime.dll".into(), File::of("runtime.dll", b"runtime"))]),
        }
    }

    #[test]
    fn a_signed_feed_verifies_and_a_changed_one_does_not() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        let (json, signature) = feed(Channel::Stable, "0.2.0").sign(&key);
        let read = Feed::verify(
            &json,
            &signature,
            &public,
            Product::Angelica,
            Channel::Stable,
        )
        .unwrap();
        assert_eq!(read, feed(Channel::Stable, "0.2.0"));

        let mut changed = json.clone();
        let at = changed.iter().position(|b| *b == b'2').unwrap();
        changed[at] = b'3';
        assert_eq!(
            Feed::verify(
                &changed,
                &signature,
                &public,
                Product::Angelica,
                Channel::Stable
            ),
            Err(Error::Signature)
        );
        let other = SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes();
        assert_eq!(
            Feed::verify(
                &json,
                &signature,
                &other,
                Product::Angelica,
                Channel::Stable
            ),
            Err(Error::Signature)
        );
        assert!(matches!(
            Feed::verify(&json, &signature, &public, Product::Map, Channel::Stable),
            Err(Error::Other(..))
        ));
        assert!(matches!(
            Feed::verify(
                &json,
                &signature,
                &public,
                Product::Angelica,
                Channel::Nightly
            ),
            Err(Error::Other(..))
        ));
    }

    #[test]
    fn names_that_reach_outside_the_release_are_refused() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let public = key.verifying_key().to_bytes();
        for name in ["../runtime.dll", "a/b", "", ".hidden", "con.", "a b"] {
            let mut bad = feed(Channel::Stable, "0.2.0");
            bad.files.insert(name.into(), File::of("x", b"x"));
            let (json, signature) = bad.sign(&key);
            assert!(
                matches!(
                    Feed::verify(
                        &json,
                        &signature,
                        &public,
                        Product::Angelica,
                        Channel::Stable
                    ),
                    Err(Error::Malformed(_))
                ),
                "{name:?}"
            );
        }
    }

    #[test]
    fn only_a_newer_version_is_offered_and_stable_wins_over_its_nightlies() {
        let running: semver::Version = "0.1.1-nightly.7".parse().unwrap();
        let stable = feed(Channel::Stable, "0.1.1");
        let nightly = feed(Channel::Nightly, "0.1.2-nightly.9");
        let old = feed(Channel::Stable, "0.1.0");
        assert_eq!(newer(&running, [&old]), None);
        assert_eq!(newer(&running, [&stable, &old]), Some(&stable));
        assert_eq!(newer(&running, [&stable, &nightly]), Some(&nightly));
        let running: semver::Version = "0.1.2".parse().unwrap();
        assert_eq!(newer(&running, [&stable, &nightly]), None, "never older");
    }

    #[test]
    fn downloaded_bytes_must_match() {
        let file = File::of("runtime.dll", b"runtime");
        assert!(file.check(b"runtime").is_ok());
        assert_eq!(
            file.check(b"runtimf"),
            Err(Error::Mismatch("runtime.dll".into()))
        );
    }

    #[test]
    fn urls_name_the_release_of_the_channel() {
        let stable = feed(Channel::Stable, "0.2.0");
        let file = &stable.files["runtime.dll"];
        assert_eq!(
            asset_url("AngelicaProject/angelica", &stable, file),
            "https://github.com/AngelicaProject/angelica/releases/download/v0.2.0/runtime.dll"
        );
        assert_eq!(
            feed_url(
                "AngelicaProject/angelica",
                Product::Angelica,
                Channel::Nightly
            ),
            "https://github.com/AngelicaProject/angelica/releases/download/nightly/channel.json"
        );
    }

    #[test]
    fn a_pack_keeps_its_files_and_refuses_paths_that_leave_the_folder() {
        let files = BTreeMap::from([
            ("map.toml".to_owned(), b"format = 1".to_vec()),
            ("entries/ui.toml".to_owned(), b"[x]".to_vec()),
            ("files.json.sig".to_owned(), vec![0; 64]),
        ]);
        let packed = pack(&files).unwrap();
        assert_eq!(unpack(&packed).unwrap(), files);
        assert!(unpack(&packed[..packed.len() - 1]).is_err(), "truncated");
        assert!(unpack(b"zip").is_err());
        for bad in ["../x", "a//b", "/a", "a/../b", "C:x", ".git/config"] {
            let files = BTreeMap::from([(bad.to_owned(), b"x".to_vec())]);
            assert!(pack(&files).is_err(), "{bad}");
            let mut forged = PACK_MAGIC.to_vec();
            forged.extend_from_slice(&u32::try_from(bad.len()).unwrap().to_le_bytes());
            forged.extend_from_slice(bad.as_bytes());
            forged.extend_from_slice(&1u64.to_le_bytes());
            forged.push(b'x');
            assert!(unpack(&forged).is_err(), "{bad}");
        }
    }
}
