use ed25519_dalek::{Signer, SigningKey};
use runtime::{
    author,
    installer::{self, Installation},
    plugin_installer::PluginInstaller,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// The installer program, signed with `key`, in `folder`.
fn program(folder: &Path, key: &SigningKey) -> PathBuf {
    let program = folder.join("plugin-installer.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_plugin-installer"), &program).unwrap();
    let bytes = std::fs::read(&program).unwrap();
    std::fs::write(
        folder.join("plugin-installer.exe.sig"),
        key.sign(&bytes).to_bytes(),
    )
    .unwrap();
    program
}

fn wait_for(installer: &PluginInstaller, text: &str) -> Vec<String> {
    let start = Instant::now();
    loop {
        let messages: Vec<String> = installer
            .messages()
            .iter()
            .map(|(_, report)| report.to_string())
            .collect();
        if messages.iter().any(|message| message.contains(text)) {
            return messages;
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "no {text:?} in {messages:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_host_runs_the_signed_installer_outside_the_game() {
    let root = tempfile::tempdir().unwrap();
    let release = SigningKey::from_bytes(&[1; 32]);
    let program = program(root.path(), &release);
    let installation = root.path().join("installation");
    std::fs::create_dir(&installation).unwrap();
    let author = SigningKey::from_bytes(&[2; 32]);
    let manifest = "manifest = 1\n[plugin]\nid = \"timers\"\nname = \"Timers\"\nversion = \"1.0.0\"\napi = \"1.0\"\ndescription = \"Timers.\"\n";
    let file = author::sign(&author, manifest.as_bytes(), b"\0asm\x0d\0\x01\0").unwrap();
    installer::install(&Installation::open(&installation).unwrap(), file, None).unwrap();

    let host = Arc::new(PluginInstaller::new(
        program.clone(),
        Some(release.verifying_key().to_bytes()),
        installation.clone(),
        Arc::new(|_: &str| {}),
    ));
    host.start(None);
    host.repair(vec!["timers".into()]);
    wait_for(&host, "Compiled timers again");
    // Once per session.
    host.repair(vec!["timers".into()]);

    assert!(host.install("http://example.org/timers.angelica").is_err());
    assert!(host.install("https://example.org/a b").is_err());
    host.install("https://127.0.0.1/timers.angelica").unwrap();
    let messages = wait_for(&host, "Could not install from https://127.0.0.1");
    assert!(!host.busy());
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.contains("Compiled timers again"))
            .count(),
        1,
        "{messages:?}"
    );

    // A program the release key did not sign does not run.
    let stranger = Arc::new(PluginInstaller::new(
        program,
        Some(author.verifying_key().to_bytes()),
        installation,
        Arc::new(|_: &str| {}),
    ));
    stranger.start(None);
    stranger.repair(vec!["timers".into()]);
    wait_for(&stranger, "not signed by Angelica");
}
