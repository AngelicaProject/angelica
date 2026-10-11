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
    assert!(read(".github/workflows/ci.yml").contains("cargo test"));
    let release = read(".github/workflows/release.yml");
    assert!(
        release.contains("release/loot_notes.wasm") && release.contains("dist/loot-notes.angelica")
    );
    assert!(release.contains("${{ secrets.ANGELICA_AUTHOR_KEY }}") && !release.contains("{id}"));
    // The checkout's path stays in the uncommitted cargo configuration.
    let checkout = env!("CARGO_MANIFEST_DIR").replace('\\', "/");
    let checkout = checkout.trim_end_matches("/crates/angelica-cli");
    for file in [
        "Cargo.toml",
        "AGENTS.md",
        ".claude/skills/angelica-plugin/SKILL.md",
    ] {
        assert!(
            !read(file).contains("{angelica}") && !read(file).contains(checkout),
            "{file}"
        );
    }
    assert!(read(".cargo/config.toml").contains(&format!("{checkout}/crates/angelica-sdk")));
    assert!(read(".gitignore").contains("/.cargo/config.toml"));

    let again = new(directory.path(), "loot-notes");
    assert!(!again.status.success(), "an existing folder is kept");
    assert!(!new(directory.path(), "Loot").status.success());
    assert!(!new(directory.path(), "9lives").status.success());
}

#[test]
fn a_relative_checkout_path_is_resolved() {
    let directory = tempfile::tempdir().unwrap();
    let checkout = env!("CARGO_MANIFEST_DIR").replace('\\', "/");
    let checkout = checkout.trim_end_matches("/crates/angelica-cli");
    let output = Command::new(env!("CARGO_BIN_EXE_angelica"))
        .current_dir(checkout)
        .args([
            "plugin",
            "new",
            "timers",
            "--json",
            "--angelica",
            ".",
            "--directory",
        ])
        .arg(directory.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let config =
        std::fs::read_to_string(directory.path().join("timers/.cargo/config.toml")).unwrap();
    assert!(
        config.contains(&format!("{checkout}/crates/angelica-sdk")),
        "{config}"
    );
}
