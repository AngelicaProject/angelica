//! `angelica release`: signatures and feeds a host accepts, made only with
//! the secret half of the key the release is built with.
use ed25519_dalek::{Signature, SigningKey};
use std::process::Command;

const SECRET: [u8; 32] = [9; 32];

fn hex(bytes: &[u8]) -> String {
    angelica_update::hex(bytes)
}

fn release(args: &[&str], secret: &[u8; 32]) -> std::process::Output {
    let public = SigningKey::from_bytes(&SECRET).verifying_key();
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .arg("release")
        .args(args)
        .env("ANGELICA_RELEASE_SIGNING_KEY", hex(secret))
        .env("ANGELICA_RELEASE_PUBLIC_KEY", hex(public.as_bytes()))
        .output()
        .unwrap()
}

#[test]
fn files_and_feeds_are_signed_with_the_release_key() {
    let folder = tempfile::tempdir().unwrap();
    let runtime = folder.path().join("runtime.dll");
    let installer = folder.path().join("plugin-installer.exe");
    std::fs::write(&runtime, b"runtime").unwrap();
    std::fs::write(&installer, b"installer").unwrap();
    let public = SigningKey::from_bytes(&SECRET).verifying_key();

    let out = release(&["sign", runtime.to_str().unwrap()], &SECRET);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let signature = std::fs::read(folder.path().join("runtime.dll.sig")).unwrap();
    public
        .verify_strict(b"runtime", &Signature::from_slice(&signature).unwrap())
        .unwrap();

    let feed = folder.path().join("channel.json");
    let out = release(
        &[
            "feed",
            "--product",
            "angelica",
            "--channel",
            "nightly",
            "--version",
            "0.1.1-nightly.4",
            "--published",
            "2026-10-20T12:00:00Z",
            "--file",
            runtime.to_str().unwrap(),
            "--file",
            installer.to_str().unwrap(),
            "--output",
            feed.to_str().unwrap(),
        ],
        &SECRET,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let read = angelica_update::Feed::verify(
        &std::fs::read(&feed).unwrap(),
        &std::fs::read(folder.path().join("channel.json.sig")).unwrap(),
        public.as_bytes(),
        angelica_update::Product::Angelica,
        angelica_update::Channel::Nightly,
    )
    .unwrap();
    assert_eq!(read.version.to_string(), "0.1.1-nightly.4");
    read.files["runtime.dll"].check(b"runtime").unwrap();
    read.files["plugin-installer.exe"]
        .check(b"installer")
        .unwrap();
}

#[test]
fn a_key_other_than_the_one_the_release_is_built_with_signs_nothing() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("runtime.dll");
    std::fs::write(&file, b"runtime").unwrap();
    let out = release(&["sign", file.to_str().unwrap()], &[3; 32]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not the secret half"));
    assert!(!folder.path().join("runtime.dll.sig").exists());
}
