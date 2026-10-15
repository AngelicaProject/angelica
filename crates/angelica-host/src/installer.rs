//! Installs plugin files from their authors into an installation, outside
//! the game (`docs/formats/plugin-file-v1.md`): checks the author, compiles
//! the component, and signs the package with the installation's key. A
//! plugin installed from a link is updated from the same link.
#![forbid(unsafe_code)]
use crate::{
    author::{self, Pin},
    manifest::Manifest,
    package::hex,
};
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

/// The folders of an installation the installer writes.
#[derive(Debug, Clone)]
pub struct Installation {
    pub plugins: PathBuf,
    pub keys: PathBuf,
}

impl Installation {
    /// The installation in `root`, as its `host.json` lays it out.
    ///
    /// # Errors
    /// An unreadable or invalid `host.json`.
    pub fn open(root: &Path) -> Result<Self, String> {
        // The host's configuration takes absolute folders.
        let root = &std::path::absolute(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let config = crate::config::Config::load(root)?;
        Ok(Self {
            keys: config
                .keys_directory
                .clone()
                .unwrap_or_else(|| root.join("keys")),
            plugins: config.plugin_directory,
        })
    }
}

/// What an installation did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Installed {
    pub plugin: String,
    pub version: String,
    pub folder: PathBuf,
    /// The plugin id was bound to its author now.
    pub new_author: bool,
}

/// What updating one plugin did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Update {
    Updated { from: String, to: String },
    UpToDate { version: String },
    Failed { reason: String },
}

/// Installs a plugin file; `url` is the link it came from, which later
/// updates use.
///
/// # Errors
/// A file that does not verify, a manifest the host refuses, a plugin id
/// bound to another author or installed from the catalog, a component that
/// does not compile, and a failure to write.
pub fn install(
    installation: &Installation,
    file: Vec<u8>,
    url: Option<&str>,
) -> Result<Installed, String> {
    let opened = author::open(&file)?;
    let manifest = parse(&opened.manifest)?;
    let id = manifest.id();
    let folder = installation.plugins.join(id);
    let pin = Pin::read(&installation.keys, id)?;
    match &pin {
        Some(pin) if !pin.names(&opened.author) => {
            return Err(format!(
                "this file of {id} is signed by another author than the installed {id}; not installed"
            ));
        }
        None if folder.exists() => {
            return Err(format!(
                "{id} is already installed from the Angelica catalog; not installed"
            ));
        }
        _ => {}
    }
    let compiled = compile(&opened.component)?;
    let key = installation_key(&installation.keys)?;
    let mut bound = pin.clone().unwrap_or_else(|| Pin::new(id, &opened.author));
    if let Some(url) = url {
        bound.url = Some(url.to_owned());
    }
    if pin.as_ref() != Some(&bound) {
        write(
            &installation.keys.join("authors").join(format!("{id}.json")),
            &serde_json::to_vec_pretty(&bound).map_err(|e| e.to_string())?,
        )?;
    }
    let files = BTreeMap::from([
        ("plugin.angelica".to_owned(), file),
        ("plugin.cwasm".to_owned(), compiled),
    ]);
    write_package(&folder, &files, &key)?;
    Ok(Installed {
        plugin: id.to_owned(),
        version: manifest.plugin.version.clone(),
        folder,
        new_author: pin.is_none(),
    })
}

/// Compiles the installed plugin file of `id` again, as after an update
/// of the runtime, whose engine its compiled component no longer matches.
///
/// # Errors
/// As [`install`]; a plugin not installed from its author.
pub fn repair(installation: &Installation, id: &str) -> Result<Installed, String> {
    let path = installation.plugins.join(id).join("plugin.angelica");
    let file = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    install(installation, file, None)
}

/// Updates every installed plugin that came from a link: a newer version
/// from the same link and the same author is installed. `fetch` downloads
/// a link, normally [`fetch`].
pub fn update(
    installation: &Installation,
    fetch: impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Vec<(String, Update)> {
    let mut outcomes = Vec::new();
    for id in bound_plugins(&installation.keys) {
        let outcome = update_one(installation, &id, &fetch);
        if let Some(outcome) = outcome {
            outcomes.push((id, outcome));
        }
    }
    outcomes
}

/// `None` for a plugin without a link or one no longer installed.
fn update_one(
    installation: &Installation,
    id: &str,
    fetch: &impl Fn(&str) -> Result<Vec<u8>, String>,
) -> Option<Update> {
    let failed = |reason: String| Some(Update::Failed { reason });
    let pin = match Pin::read(&installation.keys, id) {
        Ok(pin) => pin?,
        Err(reason) => return failed(reason),
    };
    let url = pin.url.clone()?;
    let folder = installation.plugins.join(id);
    if !folder.exists() {
        return None;
    }
    let installed = match installed_version(&folder) {
        Ok(version) => version,
        Err(reason) => return failed(reason),
    };
    let file = match fetch(&url) {
        Ok(file) => file,
        Err(reason) => return failed(format!("{url}: {reason}")),
    };
    let offered = match author::open(&file).and_then(|opened| {
        if !pin.names(&opened.author) {
            return Err(format!(
                "{url} now offers a file signed by another author; not updated"
            ));
        }
        let manifest = parse(&opened.manifest)?;
        if manifest.id() != id {
            return Err(format!("{url} now offers {}; not updated", manifest.id()));
        }
        semver::Version::parse(&manifest.plugin.version).map_err(|e| e.to_string())
    }) {
        Ok(version) => version,
        Err(reason) => return failed(reason),
    };
    if offered <= installed {
        return Some(Update::UpToDate {
            version: installed.to_string(),
        });
    }
    Some(match install(installation, file, Some(&url)) {
        Ok(done) => Update::Updated {
            from: installed.to_string(),
            to: done.version,
        },
        Err(reason) => Update::Failed { reason },
    })
}

/// The ids of the plugins bound to an author, sorted.
fn bound_plugins(keys: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(keys.join("authors"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|item| {
            item.file_name()
                .to_str()?
                .strip_suffix(".json")
                .map(str::to_owned)
        })
        .collect();
    ids.sort();
    ids
}

/// The version of the plugin file installed in `folder`.
fn installed_version(folder: &Path) -> Result<semver::Version, String> {
    let file = std::fs::read(folder.join("plugin.angelica"))
        .map_err(|e| format!("{}: {e}", folder.display()))?;
    // An earlier version may declare permissions this Angelica removed.
    let manifest = std::str::from_utf8(&author::open(&file)?.manifest)
        .map_err(|e| format!("angelica.toml: {e}"))
        .and_then(|text| Manifest::parse_earlier(text).map_err(|e| e.to_string()))?;
    semver::Version::parse(&manifest.plugin.version).map_err(|e| e.to_string())
}

/// Whether the semantic version `offered` is newer than `current`.
///
/// # Errors
/// A version that is not a semantic version.
pub fn newer(offered: &str, current: &str) -> Result<bool, String> {
    let version = |text: &str| semver::Version::parse(text).map_err(|e| format!("{text}: {e}"));
    Ok(version(offered)? > version(current)?)
}

fn parse(manifest: &[u8]) -> Result<Manifest, String> {
    let text = std::str::from_utf8(manifest).map_err(|e| format!("angelica.toml: {e}"))?;
    Manifest::parse(text).map_err(|e| e.to_string())
}

/// Downloads a plugin file from an `https` link, at most
/// [`author::MAX_FILE`] bytes.
///
/// # Errors
/// An invalid link, a private address, a status other than 200, and a
/// download that did not complete.
pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let client = crate::http::Client::for_plugin_files();
    let request = crate::http::Request {
        url: url.to_owned(),
        headers: Vec::new(),
    };
    let (head, mut body) = client
        .send(
            &request,
            crate::http::DOWNLOAD_TIME,
            author::MAX_FILE as u64,
            || false,
        )
        .map_err(|e| e.message)?;
    if head.status != 200 {
        return Err(format!("the server answered {}", head.status));
    }
    let mut file = Vec::new();
    body.read_to_end(&mut file)
        .map_err(|e| crate::http::read_failure(e).message)?;
    Ok(file)
}

/// Compiles a component with the runtime's engine, which also checks it.
///
/// # Errors
/// A component the engine does not compile.
pub fn compile(component: &[u8]) -> Result<Vec<u8>, String> {
    let sandbox = crate::sandbox::Sandbox::new()?;
    wasmtime::component::Component::new(&sandbox.engine, component)
        .and_then(|component| component.serialize())
        .map_err(|e| format!("the component does not compile: {e}"))
}

/// The installation's key, created with its public half when missing.
fn installation_key(keys: &Path) -> Result<SigningKey, String> {
    let path = keys.join("installation.key");
    if path.exists() {
        return read_key(&path);
    }
    let key = create_key(&path)?;
    write(
        &keys.join("installation.pub"),
        hex(key.verifying_key().as_bytes()).as_bytes(),
    )?;
    Ok(key)
}

/// Creates a key at `path`, which must not exist: 64 hexadecimal digits of
/// the secret.
///
/// # Errors
/// An existing file, or a failure to write.
pub fn create_key(path: &Path) -> Result<SigningKey, String> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // A key is never overwritten.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, hex(&secret).as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(SigningKey::from_bytes(&secret))
}

/// Reads a key [`create_key`] wrote.
///
/// # Errors
/// An unreadable file, or one that is not 64 hexadecimal digits.
pub fn read_key(path: &Path) -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    author::parse_key(text.trim())
        .map(|secret| SigningKey::from_bytes(&secret))
        .ok_or_else(|| format!("{}: a key is 64 hexadecimal digits", path.display()))
}

/// Writes a file through a temporary one and a rename.
///
/// # Errors
/// A failure to write.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let temporary = path.with_extension("tmp-angelica");
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Replaces `folder` with `files` and their signed `files.json`, which is
/// written last: the host loads a package only once its file list is there.
///
/// # Errors
/// An invalid package path, or a failure to write.
pub fn write_package(
    folder: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    key: &SigningKey,
) -> Result<(), String> {
    if folder.exists() {
        std::fs::remove_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    let mut hashes = serde_json::Map::new();
    for (name, bytes) in files {
        if !crate::package::valid_relative_path(name) {
            return Err(format!("{name} is not a valid package path"));
        }
        hashes.insert(name.clone(), hex(&Sha256::digest(bytes)).into());
        write(&folder.join(name), bytes)?;
    }
    let list = serde_json::to_vec(&serde_json::json!({"format": 1, "files": hashes}))
        .map_err(|e| e.to_string())?;
    write(&folder.join("files.json.sig"), &key.sign(&list).to_bytes())?;
    write(&folder.join("files.json"), &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK: &str = "https://example.org/timers.angelica";

    /// An empty component.
    const COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";

    fn file(key: &SigningKey, id: &str, version: &str) -> Vec<u8> {
        let manifest = format!(
            "manifest = 1\n[plugin]\nid = \"{id}\"\nname = \"Timers\"\nversion = \"{version}\"\napi = \"1.0\"\ndescription = \"Timers.\"\n"
        );
        author::sign(key, manifest.as_bytes(), COMPONENT).unwrap()
    }

    #[test]
    fn plugins_from_links_update_from_the_same_link_and_author() {
        let root = tempfile::tempdir().unwrap();
        let installation = Installation::open(root.path()).unwrap();
        let author = SigningKey::from_bytes(&[1; 32]);
        let stranger = SigningKey::from_bytes(&[2; 32]);
        let installed = install(&installation, file(&author, "timers", "1.0.0"), Some(LINK));
        assert!(installed.unwrap().new_author);
        let pin = Pin::read(&installation.keys, "timers").unwrap().unwrap();
        assert_eq!(pin.url.as_deref(), Some(LINK));

        // The same version: nothing to do.
        let same = |_: &str| Ok(file(&author, "timers", "1.0.0"));
        assert_eq!(
            update(&installation, same),
            [(
                "timers".to_owned(),
                Update::UpToDate {
                    version: "1.0.0".into()
                }
            )]
        );
        // Another author or another plugin at the link: refused.
        for offered in [
            file(&stranger, "timers", "2.0.0"),
            file(&author, "other", "2.0.0"),
        ] {
            let outcome = update(&installation, |_: &str| Ok(offered.clone()));
            assert!(matches!(outcome[0].1, Update::Failed { .. }), "{outcome:?}");
        }
        assert_eq!(
            installed_version(&installation.plugins.join("timers"))
                .unwrap()
                .to_string(),
            "1.0.0"
        );
        // A newer version from the author.
        let newer = |url: &str| {
            assert_eq!(url, LINK);
            Ok(file(&author, "timers", "1.1.0"))
        };
        assert_eq!(
            update(&installation, newer),
            [(
                "timers".to_owned(),
                Update::Updated {
                    from: "1.0.0".into(),
                    to: "1.1.0".into()
                }
            )]
        );
        let trust = crate::package::Trust {
            release: None,
            installation: author::installation_key(&installation.keys).unwrap(),
            keys: installation.keys.clone(),
        };
        let package =
            crate::package::authenticate_plugin(&installation.plugins.join("timers"), &trust)
                .unwrap();
        assert_eq!(package.manifest.plugin.version, "1.1.0");

        // Compiling again keeps the version and the link.
        let repaired = repair(&installation, "timers").unwrap();
        assert_eq!(repaired.version, "1.1.0");
        assert!(!repaired.new_author);
        let pin = Pin::read(&installation.keys, "timers").unwrap().unwrap();
        assert_eq!(pin.url.as_deref(), Some(LINK));

        // A plugin the player removed is not brought back.
        std::fs::remove_dir_all(installation.plugins.join("timers")).unwrap();
        assert!(update(&installation, newer).is_empty());
    }
}
