use std::path::Path;
use std::process::{Command, Output};

use angelica_pe::testing::{CODE, PeBuilder};
use serde_json::Value;

const ENTRIES: &str = r#"
[function."game.thing.update"]
locator = { signature = "48 89 5C 24 08" }
abi = { args = ["ptr"], ret = "void" }
thread = "any"

[field."game.thing.name"]
of = "game.thing"
type = "ptr"
offset = 0
"#;

/// A map and a game folder under a path with Cyrillic and a space.
fn fixture(root: &Path, entries: &str, code: &[u8]) -> (std::path::PathBuf, std::path::PathBuf) {
    let map = root.join("карта игры");
    let game = root.join("игра").join("game");
    std::fs::create_dir_all(map.join("entries")).unwrap();
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(map.join("map.toml"), "format = 2\n").unwrap();
    std::fs::write(map.join("entries/game.toml"), entries).unwrap();
    let executable = PeBuilder::new().section(".text", CODE, code).build_file();
    std::fs::write(game.join("ffxiv_dx11.exe"), executable).unwrap();
    std::fs::write(game.join("ffxivgame.ver"), "2026.09.15.0000.0000").unwrap();
    (map, game)
}

fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(arguments)
        .output()
        .unwrap()
}

fn verify(map: &Path, game: &Path, extra: &[&str]) -> Output {
    let mut arguments = vec![
        "map".as_ref(),
        "verify".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
        "--game".as_ref(),
        game.as_os_str(),
    ];
    arguments.extend(extra.iter().map(std::ffi::OsStr::new));
    run(&arguments)
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn reports_and_writes_the_build_record() {
    let directory = tempfile::tempdir().unwrap();
    let (map, game) = fixture(
        directory.path(),
        ENTRIES,
        &[0x48, 0x89, 0x5C, 0x24, 0x08, 0xC3],
    );

    let output = verify(&map, &game, &["--json", "--write"]);
    assert_eq!(output.status.code(), Some(0));
    let report = json(&output);
    assert_eq!(report["game_version"], "2026.09.15.0000.0000");
    assert_eq!(report["present"]["game.thing.update"], 0x1000);
    assert_eq!(report["present"]["game.thing.name"], 0);
    assert_eq!(report["absent"], serde_json::json!({}));
    assert_eq!(
        report["constant_offsets"],
        serde_json::json!(["game.thing.name"])
    );
    assert_eq!(report["exe_sha256"].as_str().unwrap().len(), 64);

    let record_path = map.join("builds").join("2026.09.15.0000.0000.toml");
    assert_eq!(report["written"], record_path.display().to_string());
    let record = std::fs::read_to_string(&record_path).unwrap();
    assert!(record.contains("\"game.thing.update\" = 0x1000\n"));
    assert!(record.contains(&format!("entries_sha256 = {}", report["entries_sha256"])));
    assert!(
        !map.join("builds")
            .join("2026.09.15.0000.0000.toml.tmp")
            .exists()
    );

    // Verifying again gives the same record.
    assert_eq!(verify(&map, &game, &["--write"]).status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&record_path).unwrap(), record);
}

#[test]
fn an_absent_entry_is_a_reported_failure() {
    let directory = tempfile::tempdir().unwrap();
    let (map, game) = fixture(directory.path(), ENTRIES, &[0xC3]);

    let output = verify(&map, &game, &["--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        json(&output)["absent"]["game.thing.update"],
        "signature has no match"
    );
    assert!(!map.join("builds").exists());

    let text = String::from_utf8(verify(&map, &game, &[]).stdout).unwrap();
    assert_eq!(
        text,
        "game 2026.09.15.0000.0000: 1 present, 1 absent\n  absent game.thing.update: signature has no match\n  constant offset game.thing.name\n"
    );
}

#[test]
fn an_invalid_map_is_reported_as_a_structured_error() {
    let directory = tempfile::tempdir().unwrap();
    let (map, game) = fixture(
        directory.path(),
        "[global.\"other.thing\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\n",
        &[0xC3],
    );

    let output = verify(&map, &game, &["--json"]);
    assert_eq!(output.status.code(), Some(1));
    let error = &json(&output)["error"];
    assert_eq!(error["code"], "wrong-area");
    assert_eq!(error["entry"], "other.thing");
    assert!(error["path"].as_str().unwrap().ends_with("game.toml"));
}

#[test]
fn refuses_a_version_that_cannot_name_a_file() {
    let directory = tempfile::tempdir().unwrap();
    let (map, game) = fixture(directory.path(), ENTRIES, &[0xC3]);
    std::fs::write(game.join("ffxivgame.ver"), "../escape").unwrap();
    let output = verify(&map, &game, &["--json", "--write"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json(&output)["error"]["code"], "game-version");
}

#[test]
fn a_usage_error_exits_with_two() {
    assert_eq!(
        run(&["map".as_ref(), "verify".as_ref()]).status.code(),
        Some(2)
    );
}

/// A schema naming column 1 of `Item` with the layout `fingerprint`.
fn item_schema(fingerprint: &str) -> String {
    format!(
        "sheet = \"Item\"
doc = \"Items.\"

[columns.name]
type = \"text\"
doc = \"The name.\"

[[layouts]]
fingerprint = \"{fingerprint}\"
columns = {{ name = 1 }}
"
    )
}

#[test]
fn sheet_schemas_are_checked_against_the_game_data() {
    use angelica_sqpack::{
        excel::{ColumnKind, Language},
        testing::{FakeGame, FakeRow, FakeSheet, FakeValue},
    };
    let directory = tempfile::tempdir().unwrap();
    let (map, game) = fixture(
        directory.path(),
        ENTRIES,
        &[0x48, 0x89, 0x5C, 0x24, 0x08, 0xC3],
    );
    FakeGame::new("2026.09.15.0000.0000")
        .with_sheet(
            "Item",
            FakeSheet::new(vec![ColumnKind::UInt32, ColumnKind::String]).with_rows(
                Language::English,
                vec![FakeRow::new(
                    1,
                    vec![FakeValue::UInt32(7), FakeValue::String(b"Potion".to_vec())],
                )],
            ),
        )
        .write(game.parent().unwrap())
        .unwrap();
    std::fs::create_dir_all(map.join("sheets")).unwrap();
    std::fs::write(
        map.join("sheets/Item.toml"),
        item_schema("0123456789abcdef"),
    )
    .unwrap();

    let output = verify(&map, &game, &["--json"]);
    assert_eq!(output.status.code(), Some(1), "an unknown layout fails");
    let report = json(&output);
    let item = &report["sheets"]["Item"];
    assert_eq!(item["known"], false);
    assert!(
        item["reason"]
            .as_str()
            .unwrap()
            .contains("does not know layout")
    );
    let layout = item["layout"].as_str().unwrap().to_owned();

    std::fs::write(map.join("sheets/Item.toml"), item_schema(&layout)).unwrap();
    let output = verify(&map, &game, &["--json"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json(&output)["sheets"]["Item"]["known"], true);

    let shown = run(&[
        "excel".as_ref(),
        "show".as_ref(),
        "Item".as_ref(),
        "--game".as_ref(),
        game.as_os_str(),
        "--map".as_ref(),
        map.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_eq!(shown.status.code(), Some(0));
    let shown = json(&shown);
    assert_eq!(shown["layout"], layout);
    assert_eq!(shown["columns"][1]["name"], "name");
    assert_eq!(shown["data"][0]["cells"][1], "Potion");
}
