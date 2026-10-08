use dinput8::trust::{Config, LoadError, verify};
use ed25519_dalek::{Signer, SigningKey};

#[test]
fn verification_rejects_changes_wrong_keys_and_malformed_signatures() {
    let key = SigningKey::from_bytes(&[41; 32]);
    let public = key.verifying_key().to_bytes();
    let bytes = b"signed runtime bytes";
    let signature = key.sign(bytes).to_bytes();
    verify(bytes, &signature, Some(&public)).unwrap();
    assert!(matches!(
        verify(b"changed runtime", &signature, Some(&public)),
        Err(LoadError::Signature)
    ));
    assert!(matches!(
        verify(
            bytes,
            &signature,
            Some(&SigningKey::from_bytes(&[42; 32]).verifying_key().to_bytes())
        ),
        Err(LoadError::Signature)
    ));
    for len in [0, 63, 65] {
        assert!(matches!(
            verify(bytes, &vec![0; len], Some(&public)),
            Err(LoadError::SignatureSize)
        ));
    }
    assert!(matches!(
        verify(bytes, &signature, None),
        Err(LoadError::NoReleaseKey)
    ));
    assert!(matches!(
        verify(bytes, &[0; 64], Some(&public)),
        Err(LoadError::Signature)
    ));
}

#[test]
fn configuration_is_strict_and_staged_versions_do_not_hide_errors() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("angelica.json");
    assert!(matches!(
        Config::read(directory.path()),
        Err(LoadError::Io { .. })
    ));
    for invalid in [
        "{}",
        "not json",
        r#"{"installation_dir":"relative"}"#,
        r#"{"installation_dir":"relative","key":"untrusted"}"#,
    ] {
        std::fs::write(&config, invalid).unwrap();
        assert!(Config::read(directory.path()).is_err());
    }
    std::fs::write(
        &config,
        serde_json::json!({"installation_dir": directory.path()}).to_string(),
    )
    .unwrap();
    let config = Config::read(directory.path()).unwrap();
    assert_eq!(
        config.runtime_path().unwrap(),
        directory.path().join("runtime.dll")
    );
    std::fs::create_dir(directory.path().join("staged")).unwrap();
    assert_eq!(
        config.runtime_path().unwrap(),
        directory.path().join("staged/runtime.dll")
    );
    // Selection must not silently load the old host when a staged update is broken.
    assert!(!config.runtime_path().unwrap().exists());
}
