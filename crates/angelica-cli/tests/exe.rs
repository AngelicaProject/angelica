use std::path::Path;
use std::process::Command;

use angelica_pe::testing::{CODE, PeBuilder};
use serde_json::Value;

/// Two functions that start alike; the second reads a field at 0x2A.
const FIRST: [u8; 8] = [0x40, 0x53, 0x48, 0x83, 0xEC, 0x20, 0x5B, 0xC3];
const SECOND: [u8; 20] = [
    0x40, 0x53, 0x48, 0x83, 0xEC, 0x20, // push rbx; sub rsp, 20h
    0xE8, 0x10, 0x20, 0x30, 0x40, // call rel32
    0x0F, 0xB6, 0x43, 0x2A, // movzx eax, byte ptr [rbx+2Ah]
    0x48, 0x83, 0xC4, 0x20, // add rsp, 20h
    0xC3,
];

/// A game folder whose executable holds the two functions at 0x1000 and
/// 0x1010.
fn game(root: &Path) -> std::path::PathBuf {
    let game = root.join("игра").join("game");
    std::fs::create_dir_all(&game).unwrap();
    let mut code = FIRST.to_vec();
    code.resize(0x10, 0xCC);
    code.extend_from_slice(&SECOND);
    let executable = PeBuilder::new()
        .section(".text", CODE, &code)
        .functions(&[(0x1000, 0x1008), (0x1010, 0x1024)])
        .build_file();
    std::fs::write(game.join("ffxiv_dx11.exe"), executable).unwrap();
    game
}

fn run(game: &Path, arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_angelica"))
        .arg("exe")
        .arg(arguments[0])
        .arg("--game")
        .arg(game)
        .args(&arguments[1..])
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn disassembles_a_function() {
    let directory = tempfile::tempdir().unwrap();
    let game = game(directory.path());
    let report = run(&game, &["disasm", "--rva", "0x1010"]);
    assert_eq!(report["function"]["start"], 0x1010);
    assert_eq!(report["function"]["end"], 0x1024);
    let instructions = report["instructions"].as_array().unwrap();
    assert_eq!(instructions.len(), 6, "to the function's end");
    assert_eq!(instructions[0]["text"], "push rbx");
    assert_eq!(instructions[3]["bytes"], "0F B6 43 2A");
}

#[test]
fn finds_every_match() {
    let directory = tempfile::tempdir().unwrap();
    let game = game(directory.path());
    let report = run(&game, &["find", "--signature", "40 53 48 83 EC 20"]);
    assert_eq!(report["matches"], serde_json::json!([0x1000, 0x1010]));
}

#[test]
fn proposes_unique_signatures_that_survive_moved_code() {
    let directory = tempfile::tempdir().unwrap();
    let game = game(directory.path());
    let function = run(&game, &["signature", "--rva", "0x1010"]);
    assert_eq!(
        function["signature"], "40 53 48 83 EC 20 E8 ?? ?? ?? ??",
        "the call's target is left out, and the shared start is extended until unique"
    );
    let field = run(&game, &["signature", "--rva", "0x101B", "--within"]);
    assert_eq!(
        field["signature"], "0F B6 43 ??",
        "the read offset is left out"
    );
    assert_eq!(field["within"], 0x1010);
}

#[test]
fn a_leaf_functions_field_is_found_from_its_start() {
    let directory = tempfile::tempdir().unwrap();
    let game = directory.path().join("игра").join("game");
    std::fs::create_dir_all(&game).unwrap();
    // A leaf function at 0x1000, which the exception directory does not
    // record: mov [rcx+124h], eax; ret.
    let code = [0x89, 0x81, 0x24, 0x01, 0x00, 0x00, 0xC3];
    let executable = PeBuilder::new().section(".text", CODE, &code).build_file();
    std::fs::write(game.join("ffxiv_dx11.exe"), executable).unwrap();

    let field = run(
        &game,
        &[
            "signature",
            "--rva",
            "0x1000",
            "--within",
            "--function",
            "0x1000",
        ],
    );
    assert_eq!(field["signature"], "89 81 ?? ?? ?? ??");
    assert_eq!(field["within"], 0x1000);

    let output = Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(["exe", "signature", "--game"])
        .arg(&game)
        .args(["--rva", "0x1000", "--within"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--function"));
}
