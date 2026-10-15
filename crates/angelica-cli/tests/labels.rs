use std::process::{Command, Output};

use angelica_pe::testing::{CODE, PeBuilder};

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

fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn the_script_names_the_resolved_entries_of_this_executable() {
    let directory = tempfile::tempdir().unwrap();
    let map = directory.path().join("карта игры");
    let game = directory.path().join("игра").join("game");
    std::fs::create_dir_all(map.join("entries")).unwrap();
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(map.join("map.toml"), "format = 2\n").unwrap();
    std::fs::write(map.join("entries/game.toml"), ENTRIES).unwrap();
    let executable = PeBuilder::new()
        .section(".text", CODE, &[0xC3, 0x48, 0x89, 0x5C, 0x24, 0x08, 0xC3])
        .build_file();
    std::fs::write(game.join("ffxiv_dx11.exe"), &executable).unwrap();
    let script = directory.path().join("AngelicaMapLabels.java");
    let arguments = |output: &std::path::Path| {
        run(&[
            "map".as_ref(),
            "labels".as_ref(),
            "--map".as_ref(),
            map.as_os_str(),
            "--game".as_ref(),
            game.as_os_str(),
            "--output".as_ref(),
            output.as_os_str(),
            "--json".as_ref(),
        ])
    };

    let output = arguments(&script);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["labels"], 1, "a field has no address to name");
    let text = std::fs::read_to_string(&script).unwrap();
    assert!(text.contains("public class AngelicaMapLabels extends GhidraScript"));
    assert!(text.contains("\"game.thing.update\", true);"), "{text}");
    let hash = sha2_hex(&executable);
    assert!(text.contains(&format!("EXECUTABLE_SHA256 = \"{hash}\"")));

    let elsewhere = directory.path().join("labels.java");
    assert_eq!(
        arguments(&elsewhere).status.code(),
        Some(1),
        "Ghidra needs the class's name"
    );
}

fn sha2_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
