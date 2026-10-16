use std::path::{Path, PathBuf};
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

const VERSION: &str = "2026.09.15.0000.0000";

/// A map with a build record that `map verify` wrote, under a path with
/// Cyrillic and a space; the game folder is removed, as in CI.
fn fixture(root: &Path) -> PathBuf {
    let map = root.join("карта игры");
    let game = root.join("игра");
    std::fs::create_dir_all(map.join("entries")).unwrap();
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(map.join("map.toml"), "format = 2\n").unwrap();
    std::fs::write(map.join("entries/game.toml"), ENTRIES).unwrap();
    let executable = PeBuilder::new()
        .section(".text", CODE, &[0x48, 0x89, 0x5C, 0x24, 0x08, 0xC3])
        .build_file();
    std::fs::write(game.join("ffxiv_dx11.exe"), executable).unwrap();
    std::fs::write(game.join("ffxivgame.ver"), VERSION).unwrap();
    let output = run(&[
        "map".as_ref(),
        "verify".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
        "--game".as_ref(),
        game.as_os_str(),
        "--write".as_ref(),
        "--confirm".as_ref(),
        "game.thing.name".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    std::fs::remove_dir_all(&game).unwrap();
    map
}

fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(arguments)
        .output()
        .unwrap()
}

fn check(map: &Path) -> Output {
    run(&[
        "map".as_ref(),
        "check".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
        "--json".as_ref(),
    ])
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

fn record(map: &Path) -> PathBuf {
    map.join("builds").join(format!("{VERSION}.toml"))
}

/// The error a failed check reports, after checking its exit code.
fn error(map: &Path) -> Value {
    let output = check(map);
    assert_eq!(output.status.code(), Some(1));
    json(&output)["error"].clone()
}

#[test]
fn reports_the_map_and_its_build_records() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());

    let output = check(&map);
    assert_eq!(output.status.code(), Some(0));
    let report = json(&output);
    assert_eq!(report["entries"], 2);
    assert_eq!(report["builds"][0]["game_version"], VERSION);
    assert_eq!(report["builds"][0]["present"], 2);
    assert_eq!(report["builds"][0]["absent"], 0);
    assert_eq!(
        report["builds"][0]["constant_offsets"],
        serde_json::json!(["game.thing.name"])
    );
}

#[test]
fn a_map_without_build_records_is_checked() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    std::fs::remove_dir_all(map.join("builds")).unwrap();

    let output = check(&map);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(json(&output)["builds"], serde_json::json!([]));
}

#[test]
fn an_entry_changed_without_a_new_record_fails() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    std::fs::write(
        map.join("entries/game.toml"),
        ENTRIES.replace("offset = 0", "offset = 8"),
    )
    .unwrap();

    let error = error(&map);
    assert_eq!(error["code"], "build-record");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("entries_sha256"),
        "{error}"
    );
}

#[test]
fn a_record_that_misses_an_entry_fails() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    let text = std::fs::read_to_string(record(&map)).unwrap();
    let text = text
        .lines()
        .filter(|line| !line.starts_with("\"game.thing.update\""))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(record(&map), text).unwrap();

    let error = error(&map);
    assert_eq!(error["code"], "build-record");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("must classify game.thing.update exactly once"),
        "{error}"
    );
}

#[test]
fn a_record_that_confirms_a_function_fails() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    let text = std::fs::read_to_string(record(&map)).unwrap().replace(
        "names = [\"game.thing.name\"]",
        "names = [\"game.thing.update\"]",
    );
    std::fs::write(record(&map), text).unwrap();

    assert_eq!(error(&map)["code"], "build-record");
}

#[test]
fn a_record_named_for_another_version_fails() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    std::fs::rename(
        record(&map),
        map.join("builds").join("2026.09.16.0000.0000.toml"),
    )
    .unwrap();

    assert_eq!(error(&map)["code"], "build-record");
}

#[test]
fn a_file_in_builds_that_is_not_a_record_fails() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    std::fs::write(map.join("builds").join("notes.txt"), "").unwrap();

    assert_eq!(error(&map)["code"], "build-record");
}

#[test]
fn an_entry_with_an_origin_needs_the_notices() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    std::fs::write(
        map.join("entries/game.toml"),
        ENTRIES.replace(
            "thread = \"any\"",
            "thread = \"any\"\norigin = { source = \"elsewhere\", commit = \"abc\" }",
        ),
    )
    .unwrap();
    std::fs::remove_dir_all(map.join("builds")).unwrap();

    assert_eq!(error(&map)["code"], "notices");
    std::fs::write(map.join("THIRD_PARTY_NOTICES.md"), "# Notices\n").unwrap();
    assert_eq!(check(&map).status.code(), Some(0));
}

#[test]
fn an_invalid_map_is_a_structured_error() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    std::fs::write(
        map.join("entries/game.toml"),
        "[global.\"other.thing\"]\nlocator = { signature = \"90\" }\ntype = \"ptr\"\n",
    )
    .unwrap();

    let error = error(&map);
    assert_eq!(error["code"], "wrong-area");
    assert_eq!(error["entry"], "other.thing");
    assert!(error["path"].as_str().unwrap().ends_with("game.toml"));
}

#[test]
fn every_event_and_root_says_whose_objects_it_reaches() {
    let directory = tempfile::tempdir().unwrap();
    let map = fixture(directory.path());
    std::fs::create_dir_all(map.join("types")).unwrap();
    std::fs::write(
        map.join("domains.toml"),
        "[domains.things]\ndoc = \"Things.\"\nclass = \"you\"\nread = { en = \"Sees your things\", ru = \"Видит ваши вещи\" }\n",
    )
    .unwrap();
    std::fs::write(
        map.join("types/game.toml"),
        "[types.\"game.thing\"]\ndoc = \"A thing.\"\ndomain = \"things\"\n\n[types.\"game.thing\".fields.name]\ndoc = \"Its name.\"\ntext = 16\n",
    )
    .unwrap();
    let event = "[events.thing_updated]\ndoc = \"A thing is updated.\"\nat = \"game.thing.update\"\ndomain = \"things\"\n\n[events.thing_updated.args.thing]\ndoc = \"The thing.\"\narg = 0\nref = \"game.thing\"\n";
    std::fs::write(map.join("events.toml"), event).unwrap();
    let refused = error(&map);
    assert_eq!(refused["code"], "whose");
    assert!(
        refused["message"]
            .as_str()
            .unwrap()
            .contains("event `thing_updated`"),
        "{refused}"
    );

    std::fs::write(
        map.join("events.toml"),
        event.replace(
            "domain = \"things\"\n",
            "domain = \"things\"\nwhose = \"you\"\n",
        ),
    )
    .unwrap();
    assert_eq!(check(&map).status.code(), Some(0));
}
