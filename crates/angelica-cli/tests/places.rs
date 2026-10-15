use std::process::{Command, Output};

use serde_json::{Value, json};

fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(arguments)
        .output()
        .unwrap()
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn a_game_folder_names_the_installation_it_loads() {
    let root = tempfile::tempdir().unwrap();
    let game = root.path().join("игра").join("game");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(game.join("ffxiv_dx11.exe"), b"").unwrap();
    let installation = root.path().join("установка angelica");
    std::fs::create_dir_all(installation.join("map")).unwrap();
    std::fs::create_dir_all(installation.join("plugins/timers")).unwrap();
    std::fs::create_dir_all(installation.join("plugins/hello")).unwrap();
    std::fs::create_dir_all(installation.join("dev")).unwrap();
    std::fs::write(
        installation.join("current.json"),
        r#"{"format":1,"version":"2026.10.16"}"#,
    )
    .unwrap();
    std::fs::write(installation.join("map/map.toml"), "format = 2\n").unwrap();
    std::fs::write(
        installation.join("dev/mode.json"),
        r#"{"format":1,"on":true}"#,
    )
    .unwrap();
    std::fs::write(
        game.join("angelica.json"),
        json!({"installation_dir": installation}).to_string(),
    )
    .unwrap();

    // The game's installation folder resolves to its `game` folder.
    let output = run(&[
        "where".as_ref(),
        "--game".as_ref(),
        game.parent().unwrap().as_os_str(),
        "--json".as_ref(),
    ]);
    assert!(output.status.success(), "{output:?}");
    let found = &report(&output)["games"][0];
    assert_eq!(found["game"], json!(game));
    assert_eq!(found["installation"], json!(installation));
    assert_eq!(found["angelica"], "2026.10.16");
    assert_eq!(found["map"]["format"], 2);
    assert_eq!(found["plugins"]["installed"], json!(["hello", "timers"]));
    assert_eq!(found["dev_mode"], true);
    assert_eq!(found["local_map"], false);
}

#[test]
fn a_game_without_angelica_has_no_installation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("ffxiv_dx11.exe"), b"").unwrap();
    let output = run(&[
        "where".as_ref(),
        "--game".as_ref(),
        root.path().as_os_str(),
        "--json".as_ref(),
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(report(&output)["games"][0]["installation"], Value::Null);
}

#[test]
fn a_folder_without_the_game_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let output = run(&[
        "where".as_ref(),
        "--game".as_ref(),
        root.path().as_os_str(),
        "--json".as_ref(),
    ]);
    assert!(!output.status.success());
    assert_eq!(report(&output)["error"]["code"], "where");
}
