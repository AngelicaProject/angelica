#![cfg(windows)]
use ed25519_dalek::{Signer, SigningKey};
use std::fmt::Write;
use std::{path::Path, process::Command};

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn game(directory: &Path, status: i32) -> String {
    let trace = directory.join("trace.txt");
    let _ = std::fs::remove_file(&trace);
    run(Command::new(directory.join("game.exe"))
        .env("ANGELICA_TEST_TRACE", &trace)
        .env("ANGELICA_TEST_STATUS", status.to_string()));
    std::fs::read_to_string(trace).unwrap().replace('\r', "")
}

fn build_proxy(root: &Path, target: &Path, public: Option<&str>) {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args([
            "build",
            "--offline",
            "--locked",
            "-p",
            "angelica-proxy",
            "--target-dir",
        ])
        .arg(target)
        .env_remove("ANGELICA_RELEASE_PUBLIC_KEY");
    if let Some(public) = public {
        command.env("ANGELICA_RELEASE_PUBLIC_KEY", public);
    }
    run(&mut command);
}

/// The proxy built with a test key, the game that imports it, and the
/// runtime signed with that key, in `dir`; returns the key, the runtime's
/// bytes, and its signature.
fn fixture(root: &Path, dir: &Path) -> (SigningKey, Vec<u8>, [u8; 64]) {
    let key = SigningKey::from_bytes(&[41; 32]);
    let public = key
        .verifying_key()
        .to_bytes()
        .iter()
        .fold(String::new(), |mut output, byte| {
            write!(output, "{byte:02x}").unwrap();
            output
        });
    let target = dir.join("build");
    build_proxy(root, &target, Some(&public));
    std::fs::copy(target.join("debug/dinput8.dll"), dir.join("dinput8.dll")).unwrap();
    run(Command::new("rustc")
        .arg(root.join("tests/fixtures/game.rs"))
        .arg("--edition=2024")
        .arg("-o")
        .arg(dir.join("game.exe"))
        .arg("-L")
        .arg(format!("native={}", target.join("debug").display())));
    run(Command::new("rustc")
        .arg(root.join("tests/fixtures/runtime.rs"))
        .args(["--edition=2024", "--crate-type=cdylib", "-o"])
        .arg(dir.join("runtime.dll")));
    let bytes = std::fs::read(dir.join("runtime.dll")).unwrap();
    let signature = key.sign(&bytes).to_bytes();
    std::fs::write(dir.join("runtime.dll.sig"), signature).unwrap();
    (key, bytes, signature)
}

/// Writes the fixture into the folder `ANGELICA_WINE_FIXTURE` names, for
/// `tests/wine.sh`, which runs it in Wine (`docs/development/testing.md`).
#[test]
#[ignore = "builds the fixture for the Wine check"]
fn wine_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = std::path::PathBuf::from(std::env::var_os("ANGELICA_WINE_FIXTURE").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let build = tempfile::tempdir().unwrap();
    fixture(root, build.path());
    for name in ["dinput8.dll", "game.exe", "runtime.dll", "runtime.dll.sig"] {
        std::fs::copy(build.path().join(name), dir.join(name)).unwrap();
    }
}

#[test]
fn signed_host_starts_before_game_and_failures_leave_game_running() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = tempfile::tempdir().unwrap();
    let dir = directory.path();
    let (key, bytes, signature) = fixture(root, dir);
    let target = dir.join("build");
    std::fs::write(
        dir.join("angelica.json"),
        serde_json::json!({"installation_dir": dir}).to_string(),
    )
    .unwrap();
    assert_eq!(game(dir, 0), "load\nhost\ngame\n");
    assert_eq!(game(dir, 5), "load\nhost\ngame\n");
    assert!(
        std::fs::read_to_string(dir.join("angelica-loader.log"))
            .unwrap()
            .contains("status 5")
    );
    std::fs::write(dir.join("runtime.dll.sig"), [0; 64]).unwrap();
    assert_eq!(game(dir, 0), "game\n");
    std::fs::remove_file(dir.join("runtime.dll.sig")).unwrap();
    assert_eq!(game(dir, 0), "game\n");
    std::fs::write(dir.join("runtime.dll.sig"), signature).unwrap();
    let mut tampered = bytes.clone();
    tampered.push(0);
    std::fs::write(dir.join("runtime.dll"), tampered).unwrap();
    assert_eq!(game(dir, 0), "game\n");
    std::fs::write(dir.join("runtime.dll"), bytes).unwrap();
    // An update: a version folder that current.json names.
    std::fs::write(
        dir.join("current.json"),
        r#"{"format":1,"version":"0.2.0"}"#,
    )
    .unwrap();
    assert_eq!(game(dir, 0), "game\n");
    let staged = dir.join("versions/0.2.0");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::copy(dir.join("runtime.dll"), staged.join("runtime.dll")).unwrap();
    std::fs::write(staged.join("runtime.dll.sig"), signature).unwrap();
    assert_eq!(game(dir, 0), "load\nhost\ngame\n");
    run(Command::new("rustc")
        .arg(root.join("tests/fixtures/runtime.rs"))
        .args([
            "--edition=2024",
            "--crate-type=cdylib",
            "--cfg",
            "no_start",
            "-o",
        ])
        .arg(staged.join("runtime.dll")));
    let incompatible = std::fs::read(staged.join("runtime.dll")).unwrap();
    std::fs::write(
        staged.join("runtime.dll.sig"),
        key.sign(&incompatible).to_bytes(),
    )
    .unwrap();
    assert_eq!(game(dir, 0), "load\ngame\n");
    assert!(
        std::fs::read_to_string(dir.join("angelica-loader.log"))
            .unwrap()
            .contains("has no angelica_start_v1")
    );
    build_proxy(root, &target, None);
    std::fs::copy(target.join("debug/dinput8.dll"), dir.join("dinput8.dll")).unwrap();
    assert_eq!(game(dir, 0), "game\n");
    assert!(
        std::fs::read_to_string(dir.join("angelica-loader.log"))
            .unwrap()
            .contains("no compiled release public key")
    );
    std::fs::remove_file(dir.join("angelica.json")).unwrap();
    assert_eq!(game(dir, 0), "game\n");
}
