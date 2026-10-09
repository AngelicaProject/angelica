use ed25519_dalek::{Signer, SigningKey};
use runtime::package::{FileSet, hex};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn signed_set(directory: &std::path::Path, key: &SigningKey, files: BTreeMap<String, String>) {
    let json = serde_json::to_vec(&FileSet { format: 1, files }).unwrap();
    std::fs::write(directory.join("files.json.sig"), key.sign(&json).to_bytes()).unwrap();
    std::fs::write(directory.join("files.json"), json).unwrap();
}

#[test]
fn authentication_precedes_parsing_and_every_content_hash_is_checked() {
    let directory = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[7; 32]);
    let public = key.verifying_key().to_bytes();
    std::fs::write(directory.path().join("plugin.wasm"), b"authenticated bytes").unwrap();
    signed_set(
        directory.path(),
        &key,
        BTreeMap::from([(
            "plugin.wasm".into(),
            hex(&Sha256::digest(b"authenticated bytes")),
        )]),
    );
    assert_eq!(
        FileSet::load(directory.path(), Some(&public)).unwrap()["plugin.wasm"],
        b"authenticated bytes"
    );
    std::fs::write(directory.path().join("plugin.wasm"), b"modified bytes").unwrap();
    assert!(
        FileSet::load(directory.path(), Some(&public))
            .unwrap_err()
            .to_string()
            .contains("hash")
    );
    std::fs::write(
        directory.path().join("files.json"),
        b"malformed unsigned json",
    )
    .unwrap();
    assert!(
        FileSet::load(directory.path(), Some(&public))
            .unwrap_err()
            .to_string()
            .contains("signature")
    );
    assert!(
        FileSet::load(directory.path(), None)
            .unwrap_err()
            .to_string()
            .contains("key")
    );
    signed_set(
        directory.path(),
        &key,
        BTreeMap::from([("../outside".into(), "00".into())]),
    );
    assert!(
        FileSet::load(directory.path(), Some(&public))
            .unwrap_err()
            .to_string()
            .contains("path")
    );
}

/// Writes `files` into `folder` with a file list signed by `key`.
fn package(folder: &std::path::Path, key: &SigningKey, files: &[(&str, &[u8])]) {
    std::fs::create_dir_all(folder).unwrap();
    let mut hashes = BTreeMap::new();
    for (name, bytes) in files {
        std::fs::write(folder.join(name), bytes).unwrap();
        hashes.insert((*name).to_owned(), hex(&Sha256::digest(bytes)));
    }
    signed_set(folder, key, hashes);
}

const MANIFEST: &str = r#"manifest = 1
[plugin]
id = "timers"
name = "Timers"
version = "1.0.0"
api = "1.0"
description = "Timers."
"#;

#[test]
fn plugins_from_their_authors_need_the_installation_key_and_the_bound_author() {
    use runtime::{
        author::{Pin, sign},
        manifest::Origin,
        package::{Trust, authenticate_plugin},
    };
    let root = tempfile::tempdir().unwrap();
    let release = SigningKey::from_bytes(&[1; 32]);
    let installation = SigningKey::from_bytes(&[2; 32]);
    let author = SigningKey::from_bytes(&[3; 32]);
    let stranger = SigningKey::from_bytes(&[4; 32]);
    let keys = root.path().join("keys");
    let trust = Trust {
        release: Some(release.verifying_key().to_bytes()),
        installation: Some(installation.verifying_key().to_bytes()),
        keys: keys.clone(),
    };
    let folder = root.path().join("plugins/timers");
    let file = sign(&author, MANIFEST.as_bytes(), b"\0asm").unwrap();
    package(
        &folder,
        &installation,
        &[("plugin.angelica", &file), ("plugin.cwasm", b"compiled")],
    );

    // Not bound to an author yet: refused.
    assert!(
        authenticate_plugin(&folder, &trust)
            .unwrap_err()
            .contains("another author")
    );
    std::fs::create_dir_all(keys.join("authors")).unwrap();
    let pin = Pin::new("timers", &author.verifying_key().to_bytes());
    std::fs::write(
        keys.join("authors/timers.json"),
        serde_json::to_vec(&pin).unwrap(),
    )
    .unwrap();
    let loaded = authenticate_plugin(&folder, &trust).unwrap();
    assert_eq!(loaded.manifest.id(), "timers");
    assert_eq!(loaded.manifest.origin, Origin::Author);
    assert_eq!(loaded.artifact, b"compiled");

    // Another author's file under the same id.
    let other = sign(&stranger, MANIFEST.as_bytes(), b"\0asm").unwrap();
    package(
        &folder,
        &installation,
        &[("plugin.angelica", &other), ("plugin.cwasm", b"compiled")],
    );
    assert!(authenticate_plugin(&folder, &trust).is_err());

    // Without an installation key, or signed by the author alone.
    package(
        &folder,
        &installation,
        &[("plugin.angelica", &file), ("plugin.cwasm", b"compiled")],
    );
    let release_only = Trust {
        installation: None,
        ..trust.clone()
    };
    assert!(authenticate_plugin(&folder, &release_only).is_err());
    package(
        &folder,
        &author,
        &[("plugin.angelica", &file), ("plugin.cwasm", b"compiled")],
    );
    assert!(authenticate_plugin(&folder, &trust).is_err());

    // An extra file in a package from its author.
    package(
        &folder,
        &installation,
        &[
            ("plugin.angelica", &file),
            ("plugin.cwasm", b"compiled"),
            ("angelica.toml", MANIFEST.as_bytes()),
        ],
    );
    assert!(authenticate_plugin(&folder, &trust).is_err());

    // A release package stays from the catalog.
    let folder = root.path().join("plugins/catalog/timers");
    package(
        &folder,
        &release,
        &[
            ("angelica.toml", MANIFEST.as_bytes()),
            ("plugin.wasm", b"\0asm"),
            ("plugin.cwasm", b"compiled"),
        ],
    );
    assert_eq!(
        authenticate_plugin(&folder, &trust)
            .unwrap()
            .manifest
            .origin,
        Origin::Catalog
    );
}
