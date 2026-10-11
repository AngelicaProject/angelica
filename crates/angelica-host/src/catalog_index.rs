//! The catalog's signed index (`docs/formats/catalog-v1.md#index`), which
//! the Catalog tab of the menu shows: read on a host thread when the tab
//! first opens and every 6 hours, and verified with the release key before
//! it is parsed.
use crate::{manifest::Permission, self_update::Fetch};
use serde::Deserialize;
use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Where the catalog publishes its index.
pub const INDEX: &str =
    "https://github.com/AngelicaProject/angelica-registry/releases/download/catalog/catalog.json";
/// The largest index.
const MAX_INDEX: u64 = 4 * 1024 * 1024;
const EVERY: Duration = Duration::from_hours(6);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub format: u32,
    pub published: String,
    pub plugins: Vec<Listed>,
    #[serde(default)]
    pub withdrawn: Vec<Withdrawn>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    pub link: String,
    pub sha256: String,
    pub source: String,
}

impl Listed {
    /// The permissions this Angelica knows, for their icons and words.
    #[must_use]
    pub fn known_permissions(&self) -> Vec<Permission> {
        self.permissions
            .iter()
            .filter_map(|name| serde_json::from_value(serde_json::Value::String(name.clone())).ok())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Withdrawn {
    pub id: String,
    #[serde(default)]
    pub version: Option<String>,
    pub reason: String,
}

impl Index {
    /// Why `id` at `version` is withdrawn, if it is.
    #[must_use]
    pub fn withdrawn(&self, id: &str, version: &str) -> Option<&str> {
        self.withdrawn
            .iter()
            .find(|item| item.id == id && item.version.as_deref().is_none_or(|v| v == version))
            .map(|item| item.reason.as_str())
    }
}

/// Verifies `signature` over `json` with `key`, then reads the index.
///
/// # Errors
/// A signature that does not verify, or an index of another form.
pub fn verify(json: &[u8], signature: &[u8], key: &[u8; 32]) -> Result<Index, String> {
    let key = ed25519_dalek::VerifyingKey::from_bytes(key).map_err(|e| e.to_string())?;
    let signature = ed25519_dalek::Signature::from_slice(signature)
        .map_err(|_| "the catalog's signature is malformed".to_owned())?;
    key.verify_strict(json, &signature)
        .map_err(|_| "the catalog's signature does not verify".to_owned())?;
    let index: Index = serde_json::from_slice(json).map_err(|e| format!("the catalog: {e}"))?;
    if index.format != 1 {
        return Err(format!(
            "the catalog's format {} is unsupported",
            index.format
        ));
    }
    for plugin in &index.plugins {
        if !crate::manifest::valid_id(&plugin.id) || !plugin.link.starts_with("https://") {
            return Err(format!("the catalog lists {} wrongly", plugin.id));
        }
    }
    Ok(index)
}

/// What the Catalog tab shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    NotLoaded,
    Loading,
    Loaded(Arc<Index>),
    Failed(String),
}

/// The index, shared with the thread that reads it.
#[derive(Default)]
pub struct Catalog {
    state: Mutex<Option<State>>,
    requested: AtomicBool,
    started: AtomicBool,
}

impl Catalog {
    #[must_use]
    pub fn state(&self) -> State {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or(State::NotLoaded)
    }

    pub(crate) fn set(&self, state: State) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = Some(state);
    }

    /// Reads the index now; the first request starts the thread that reads
    /// it, so a player who never opens the tab sends nothing.
    pub fn request(self: &Arc<Self>, fetch: &Arc<dyn Fetch>, key: [u8; 32]) {
        self.requested.store(true, Ordering::Release);
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let catalog = self.clone();
        let fetch = fetch.clone();
        let _ = std::thread::Builder::new()
            .name("angelica-catalog".into())
            .spawn(move || {
                loop {
                    if catalog.requested.swap(false, Ordering::AcqRel) {
                        catalog.set(State::Loading);
                        let state = fetch
                            .get(INDEX, MAX_INDEX)
                            .and_then(|json| {
                                let signature = fetch.get(&format!("{INDEX}.sig"), 64)?;
                                verify(&json, &signature, &key)
                            })
                            .map_or_else(State::Failed, |index| State::Loaded(Arc::new(index)));
                        catalog.set(state);
                    }
                    let step = Duration::from_millis(500);
                    let mut waited = Duration::ZERO;
                    while waited < EVERY && !catalog.requested.load(Ordering::Acquire) {
                        std::thread::sleep(step);
                        waited += step;
                    }
                    catalog.requested.store(true, Ordering::Release);
                }
            });
    }
}

/// What the menu needs to read the catalog.
pub struct Source {
    pub catalog: Arc<Catalog>,
    pub fetch: Arc<dyn Fetch>,
    pub key: [u8; 32],
}

impl Source {
    /// Reads the index now.
    pub fn request(&self) {
        self.catalog.request(&self.fetch, self.key);
    }

    #[must_use]
    pub fn state(&self) -> State {
        self.catalog.state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const INDEX_JSON: &str = r#"{
  "format": 1,
  "published": "2026-10-20T12:00:00Z",
  "plugins": [{
    "id": "harmonia", "name": "Harmonia", "description": "Translations.", "version": "0.2.0",
    "authors": ["pokeda"], "permissions": ["excel.text", "http.get", "something.new"],
    "hosts": ["github.com"],
    "link": "https://github.com/AngelicaProject/angelica-registry/releases/download/harmonia/harmonia.angelica",
    "sha256": "00", "source": "https://github.com/AngelicaProject/harmonia-angelica/tree/v0.2.0"
  }],
  "withdrawn": [{"id": "timers", "version": "1.2.0", "reason": "Reads chat."}, {"id": "old", "reason": "Gone."}]
}"#;

    #[test]
    fn only_a_signed_index_is_read() {
        let key = SigningKey::from_bytes(&[5; 32]);
        let public = key.verifying_key().to_bytes();
        let signature = key.sign(INDEX_JSON.as_bytes()).to_bytes();
        let index = verify(INDEX_JSON.as_bytes(), &signature, &public).unwrap();
        assert_eq!(index.plugins[0].name, "Harmonia");
        assert_eq!(
            index.plugins[0].known_permissions(),
            vec![Permission::ExcelText, Permission::HttpGet],
            "a permission this Angelica does not know is left out of the words"
        );
        assert_eq!(index.withdrawn("timers", "1.2.0"), Some("Reads chat."));
        assert_eq!(index.withdrawn("timers", "1.3.0"), None);
        assert_eq!(index.withdrawn("old", "9.9.9"), Some("Gone."));

        let changed = INDEX_JSON.replace("0.2.0\"", "0.3.0\"");
        assert!(verify(changed.as_bytes(), &signature, &public).is_err());
        let other = SigningKey::from_bytes(&[6; 32]).verifying_key().to_bytes();
        assert!(verify(INDEX_JSON.as_bytes(), &signature, &other).is_err());

        let plain = INDEX_JSON.replace(
            "https://github.com/AngelicaProject/angelica-registry",
            "http://example.org",
        );
        let signature = key.sign(plain.as_bytes()).to_bytes();
        assert!(verify(plain.as_bytes(), &signature, &public).is_err());
    }
}
