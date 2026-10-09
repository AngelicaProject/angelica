use std::process::Command;

fn new(directory: &std::path::Path, id: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(["plugin", "new", id, "--json", "--directory"])
        .arg(directory)
        .output()
        .unwrap()
}

#[test]
fn creates_a_project_that_names_the_plugin_everywhere() {
    let directory = tempfile::tempdir().unwrap();
    let output = new(directory.path(), "loot-notes");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let folder = directory.path().join("loot-notes");
    let read = |path: &str| std::fs::read_to_string(folder.join(path)).unwrap();
    let manifest = runtime::manifest::Manifest::parse(&read("angelica.toml")).unwrap();
    assert_eq!(manifest.id(), "loot-notes");
    assert_eq!(manifest.plugin.name, "Loot Notes");
    assert!(read("Cargo.toml").contains("name = \"loot-notes\""));
    assert!(read("src/lib.rs").contains("angelica_sdk::plugin!(LootNotes);"));
    assert!(read("tests/plugin.rs").contains("\"/loot-notes\""));
    assert!(read("AGENTS.md").contains("target/wasm32-wasip2/release/loot_notes.wasm"));
    let skill = read(".claude/skills/angelica-plugin/SKILL.md");
    assert!(skill.starts_with("---") && skill.contains("name: angelica-plugin"));
    assert!(skill.contains("# Working on Loot Notes") && !skill.contains("{angelica}"));
    assert!(read("rust-toolchain.toml").contains("wasm32-wasip2"));
    assert!(!read("Cargo.toml").contains("{angelica}"));

    let again = new(directory.path(), "loot-notes");
    assert!(!again.status.success(), "an existing folder is kept");
    assert!(!new(directory.path(), "Loot").status.success());
    assert!(!new(directory.path(), "9lives").status.success());
}
