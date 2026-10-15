use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const ENTRIES: &str = r#"
[function."game.thing.update"]
locator = { signature = "48 89 5C 24 08" }
abi = { args = ["ptr"], ret = "void" }
thread = "any"
"#;

fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(arguments)
        .output()
        .unwrap()
}

/// An installation and a map, under a path with Cyrillic and a space.
fn fixture(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let installation = root.join("установка angelica");
    std::fs::create_dir_all(&installation).unwrap();
    std::fs::write(installation.join("current.json"), "{}").unwrap();
    let map = root.join("карта игры");
    std::fs::create_dir_all(map.join("entries")).unwrap();
    std::fs::create_dir_all(map.join("builds")).unwrap();
    std::fs::write(map.join("map.toml"), "format = 2\n").unwrap();
    std::fs::write(map.join("entries/game.toml"), ENTRIES).unwrap();
    (installation, map)
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn dev_mode_is_switched_from_the_command_line() {
    let directory = tempfile::tempdir().unwrap();
    let (installation, _) = fixture(directory.path());
    let switch = |state: &str| {
        run(&[
            "dev".as_ref(),
            "mode".as_ref(),
            state.as_ref(),
            "--installation".as_ref(),
            installation.as_os_str(),
            "--json".as_ref(),
        ])
    };
    let output = switch("on");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json(&output)["dev_mode"], true);
    assert!(runtime::dev::read(&installation.join("dev")));
    assert_eq!(switch("off").status.code(), Some(0));
    assert!(!runtime::dev::read(&installation.join("dev")));

    let elsewhere = directory.path().join("nothing");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let output = run(&[
        "dev".as_ref(),
        "mode".as_ref(),
        "on".as_ref(),
        "--installation".as_ref(),
        elsewhere.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(1), "not an installation");
}

#[test]
fn a_tried_map_is_the_local_map_the_host_reads() {
    let directory = tempfile::tempdir().unwrap();
    let (installation, map) = fixture(directory.path());
    let try_map = |extra: &[&std::ffi::OsStr]| {
        let mut arguments: Vec<&std::ffi::OsStr> = vec![
            "map".as_ref(),
            "try".as_ref(),
            "--installation".as_ref(),
            installation.as_os_str(),
            "--json".as_ref(),
        ];
        arguments.extend_from_slice(extra);
        run(&arguments)
    };

    let output = try_map(&["--map".as_ref(), map.as_os_str()]);
    assert_eq!(output.status.code(), Some(0));
    let report = json(&output);
    assert_eq!(report["entries"], 1);
    assert_eq!(report["dev_mode"], false);
    let local = installation.join("dev").join("map");
    let files = runtime::package::FileSet::load_unsigned(&local).unwrap();
    assert_eq!(
        files["entries/game.toml"],
        std::fs::read(map.join("entries/game.toml")).unwrap()
    );

    let dev = runtime::dev::DevMode::new(&installation);
    assert!(dev.local_map().is_none(), "not used while dev mode is off");
    runtime::dev::write(&installation.join("dev"), true).unwrap();
    assert_eq!(
        runtime::dev::DevMode::new(&installation).local_map(),
        Some(local.clone())
    );

    let output = try_map(&["--clear".as_ref()]);
    assert_eq!(output.status.code(), Some(0));
    assert!(!local.exists());
}

#[test]
fn a_map_that_fails_the_check_is_not_tried() {
    let directory = tempfile::tempdir().unwrap();
    let (installation, map) = fixture(directory.path());
    std::fs::write(map.join("map.toml"), "format = 3\n").unwrap();
    let output = run(&[
        "map".as_ref(),
        "try".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
        "--installation".as_ref(),
        installation.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(json(&output)["error"]["code"].is_string());
    assert!(!installation.join("dev").join("map").exists());
}
