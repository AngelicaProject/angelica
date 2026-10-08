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
