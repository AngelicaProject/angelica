use std::{path::Path, process::Command};

/// An empty component.
const COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";

const MANIFEST: &str = r#"manifest = 1
[plugin]
id = "timers"
name = "Timers"
version = "1.0.0"
api = "1.0"
description = "Timers."
"#;

fn angelica(args: &[&std::ffi::OsStr]) -> (bool, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    let report = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stdout)));
    (output.status.success(), report)
}

/// Packs the plugin in `folder` with `key` into `output`.
fn pack(folder: &Path, key: &Path, output: &Path) -> (bool, serde_json::Value) {
    angelica(&[
        "plugin".as_ref(),
        "pack".as_ref(),
        folder.as_os_str(),
        "--component".as_ref(),
        folder.join("plugin.wasm").as_os_str(),
        "--key".as_ref(),
        key.as_os_str(),
        "--output".as_ref(),
        output.as_os_str(),
    ])
}

fn install(file: &Path, installation: &Path) -> (bool, serde_json::Value) {
    angelica(&[
        "plugin".as_ref(),
        "install".as_ref(),
        file.as_os_str(),
        "--installation".as_ref(),
        installation.as_os_str(),
    ])
}

#[test]
fn a_packed_plugin_installs_for_its_author_only() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("timers");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("angelica.toml"), MANIFEST).unwrap();
    std::fs::write(source.join("plugin.wasm"), COMPONENT).unwrap();
    let installation = root.path().join("installation");
    std::fs::create_dir(&installation).unwrap();

    // The key is created on first use.
    let key = root.path().join("author/author.key");
    let file = root.path().join("timers.angelica");
    let (ok, report) = pack(&source, &key, &file);
    assert!(ok, "{report}");
    assert_eq!(report["key_created"], true);
    let (ok, report) = install(&file, &installation);
    assert!(ok, "{report}");
    assert_eq!(report["new_author"], true);

    // The installed package authenticates with the installation's key and
    // the author it is bound to.
    let keys = installation.join("keys");
    let trust = runtime::package::Trust {
        release: None,
        installation: runtime::author::installation_key(&keys).unwrap(),
        keys,
    };
    let package =
        runtime::package::authenticate_plugin(&installation.join("plugins/timers"), &trust)
            .unwrap();
    assert_eq!(package.manifest.origin, runtime::manifest::Origin::Author);

    // Installed from a file, it has no link to update from.
    let (ok, report) = angelica(&[
        "plugin".as_ref(),
        "update".as_ref(),
        "--installation".as_ref(),
        installation.as_os_str(),
    ]);
    assert!(ok, "{report}");
    assert_eq!(report, serde_json::json!({}));

    // A new version from the same author installs; another author's does not.
    let (ok, _) = pack(&source, &key, &file);
    assert!(ok);
    let (ok, report) = install(&file, &installation);
    assert!(ok, "{report}");
    assert_eq!(report["new_author"], false);
    let (ok, _) = pack(&source, &root.path().join("stranger.key"), &file);
    assert!(ok);
    let (ok, report) = install(&file, &installation);
    assert!(!ok);
    assert!(
        report["error"]["message"]
            .as_str()
            .unwrap()
            .contains("another author"),
        "{report}"
    );

    // A plugin of the catalog is not replaced by one from an author.
    let catalog = root.path().join("catalog");
    std::fs::create_dir_all(catalog.join("plugins/timers")).unwrap();
    let (ok, report) = install(&file, &catalog);
    assert!(!ok);
    assert!(
        report["error"]["message"]
            .as_str()
            .unwrap()
            .contains("catalog"),
        "{report}"
    );

    // Links are https only.
    let (ok, report) = install(
        Path::new("http://example.org/timers.angelica"),
        &installation,
    );
    assert!(!ok);
    assert!(
        report["error"]["message"]
            .as_str()
            .unwrap()
            .contains("https"),
        "{report}"
    );

    // A damaged file is refused before anything is written.
    let mut damaged = std::fs::read(&file).unwrap();
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    std::fs::write(&file, damaged).unwrap();
    let fresh = root.path().join("fresh");
    std::fs::create_dir(&fresh).unwrap();
    let (ok, _) = install(&file, &fresh);
    assert!(!ok);
    assert!(!fresh.join("keys").exists());
}
