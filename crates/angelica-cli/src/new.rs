//! `angelica plugin new`: a plugin project that builds, tests against the
//! scripted game, and installs, with the guidance an agent needs.
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use serde_json::json;

use crate::Output;

#[derive(Debug, Args)]
pub struct NewArgs {
    /// The plugin's id: lowercase letters, digits, and inner hyphens; also
    /// its folder, crate, and chat command.
    id: String,
    /// The folder to create the project in.
    #[arg(long, default_value = ".")]
    directory: PathBuf,
    /// The Angelica checkout whose SDK and test host the project uses.
    #[arg(long)]
    angelica: Option<PathBuf>,
    #[command(flatten)]
    output: Output,
}

const CARGO: &str = r#"[package]
name = "{id}"
version = "0.1.0"
edition = "2024"
rust-version = "1.98.1"
publish = false

[lib]
crate-type = ["cdylib"]

# `.cargo/config.toml`, which is not committed, points both at a local
# Angelica checkout.
[dependencies]
angelica-sdk = { git = "https://github.com/AngelicaProject/angelica" }

[dev-dependencies]
angelica-fake-game = { git = "https://github.com/AngelicaProject/angelica" }

# The plugin is its own workspace.
[workspace]
"#;

const MANIFEST: &str = r#"manifest = 1

[plugin]
id = "{id}"
name = "{name}"
version = "0.1.0"
api = "1.0"
description = "Greets you in the chat log."

[permissions]
"chat.commands" = true
"chat.print" = true
"#;

const LIB: &str = r#"//! {name}: greets you in the chat log when you run `/{id}`, and counts
//! greetings in its window.
use angelica_sdk::{
    Plugin,
    chat::Message,
    commands::{self, Command, Invocation},
    ui::{Event, Window},
};

struct {type} {
    open: bool,
    greetings: u32,
}

impl Plugin for {type} {
    fn start() -> Result<Self, String> {
        if let Err(error) = commands::define(&[Command::new("{id}", "Greets you")]) {
            angelica_sdk::warning(&format!("no command: {}", error.message));
        }
        Ok(Self {
            open: false,
            greetings: 0,
        })
    }

    fn command(&mut self, invocation: Invocation) -> Result<(), String> {
        if invocation.is(&["{id}"]) {
            self.greetings += 1;
            Message::new()
                .text("Hello from ")
                .accent("{name}")
                .print()?;
        }
        Ok(())
    }

    fn ui(&mut self) -> Vec<Window> {
        if !self.open {
            return Vec::new();
        }
        vec![
            Window::new("main", "{name}")
                .text(format!("{} greetings", self.greetings))
                .button("greet", "Greet"),
        ]
    }

    fn ui_event(&mut self, event: Event) {
        match event {
            Event::OpenRequested => self.open = true,
            Event::Closed(_) => self.open = false,
            Event::Clicked(target) if target.id == "greet" => self.greetings += 1,
            _ => {}
        }
    }
}

angelica_sdk::plugin!({type});
"#;

const TEST: &str = r#"//! {name} in a host with a scripted game.
use angelica_fake_game::{
    TestHost, build_component_in,
    game_files::{FakeGame, TextSheet},
    runtime::{
        host::{Action, Event, Status},
        sandbox::Limits,
    },
};
use std::{path::Path, sync::OnceLock};

const MANIFEST: &str = include_str!("../angelica.toml");

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component_in(Path::new(env!("CARGO_MANIFEST_DIR")), "{id}"))
}

/// The plugin approved and running in a game whose command list has
/// `/tell`.
fn started() -> TestHost {
    let mut test = TestHost::new(Limits::default());
    FakeGame::new("2026.09.15.0000.0000")
        .with_text(
            "TextCommand",
            &TextSheet::new(1, &[0]).row(1, &[(0, "/tell")]),
        )
        .write(&test.game_folder())
        .unwrap();
    test.install(MANIFEST, component()).unwrap();
    test.host.queue(Action::Approve("{id}".into()));
    test.frames(1);
    assert_eq!(test.host.plugin("{id}").unwrap().status(), Status::Running);
    test
}

#[test]
fn the_command_greets_in_the_chat_log() {
    let test = started();
    assert!(test.command("/{id}"));
    assert_eq!(test.game.printed_text(), ["[{name}] Hello from {name}"]);
}

#[test]
fn the_window_counts_greetings() {
    let test = started();
    test.event("{id}", Event::OpenRequested);
    test.click("{id}", "main", "greet");
    assert_eq!(test.texts("{id}"), ["1 greetings"]);
}

#[test]
fn greetings_start_again_after_a_game_restart() {
    let test = started();
    assert!(test.command("/{id}"));
    // Grants and storage carry over; this plugin keeps its count in memory.
    let test = test.restart().unwrap();
    test.event("{id}", Event::OpenRequested);
    assert_eq!(test.texts("{id}"), ["0 greetings"]);
}
"#;

/// The local Angelica checkout the dependencies build from; kept out of
/// the repository, so its path stays on this machine.
const CARGO_CONFIG: &str = r#"[patch."https://github.com/AngelicaProject/angelica"]
angelica-sdk = { path = "{angelica}/crates/angelica-sdk" }
angelica-fake-game = { path = "{angelica}/crates/angelica-fake-game" }

[net]
# Fetches the repository with git, which has the GitHub credentials.
git-fetch-with-cli = true
"#;

const AGENTS: &str = r"# {name}

An Angelica plugin for FINAL FANTASY XIV, in Rust, compiled to a WebAssembly component.

## Read first

`<angelica>` is the Angelica checkout that `.cargo/config.toml` names (not committed; https://github.com/AngelicaProject/angelica otherwise).

- `<angelica>/docs/plugins/authoring.md`: the work loop from an idea to a published plugin.
- `<angelica>/docs/plugins/interfaces.md`: which interface, permission, and SDK call serve each task.
- `<angelica>/docs/plugins/api-requests.md`: what to file when the API lacks a capability.
- `<angelica>/docs/formats/plugin-api-v1.md` and `<angelica>/wit/`: the contract; `<angelica>/crates/angelica-sdk/`: the SDK; `<angelica>/crates/angelica-fake-game/`: the test kit.

## Commands

- `cargo test`: builds the component and runs `tests/` against the scripted game. The first run fetches the Angelica repository once and builds the host, which takes a few minutes; later runs are quick and offline.
- `cargo clippy --target wasm32-wasip2 -- -D warnings -W clippy::pedantic` for the plugin, `cargo clippy --tests -- -D warnings -W clippy::pedantic` for the tests, and `cargo fmt --check`.
- `cargo build --release --target wasm32-wasip2` builds `target/wasm32-wasip2/release/{crate}.wasm`.
- `angelica dev install --key <dev key> --installation <installation> --plugin .=target/wasm32-wasip2/release/{crate}.wasm` loads it in the running game within a second.

## Rules

- Use only the plugin API. There are no game addresses, offsets, hooks, or memory reads in a plugin. When the API lacks a capability, do not work around it: file an API request.
- Declare only the permissions the plugin uses; the player sees each one.
- Long work (bulk sheet reads, HTTP) runs in a job; frame callbacks have a few milliseconds.
- For your own serde types, add the `serde` crate with its `derive` feature. The test build runs offline: a new dependency builds once `cargo fetch` has downloaded it.
- Text printed to the chat log uses ASCII `...`, never `…`.
- Name commands for what they do; never like the game's commands that talk to other players (`say`, `shout`, `tell`).
";

const SKILL: &str = r"---
name: angelica-plugin
description: Write, test, and run this Angelica plugin for FINAL FANTASY XIV - find the plugin API interface and permission for a feature, write it with angelica-sdk, test it against the scripted game, install it in the running game, or file an API request for a missing capability. Use for any change to this plugin.
---

# Working on {name}

Read `AGENTS.md` in this project, then `<angelica>/docs/plugins/authoring.md` for the work loop (`<angelica>` is the checkout `.cargo/config.toml` names).

1. Before any code, design the whole feature in game terms: what it reads, what it changes, how the player drives it. Split it into pieces.
2. Look up every piece in `<angelica>/docs/plugins/interfaces.md`: SDK call, permission, main instance or job. Add the permissions to `angelica.toml`; nothing else.
3. Collect every piece the cards do not cover at once: those are not available. Check the open `api-request` issues of AngelicaProject/angelica (`gh issue list --repo AngelicaProject/angelica --label api-request`), write the new requests in the format of `<angelica>/docs/plugins/api-requests.md`, show them to the user, and file them as issues once the user agrees (or comment on an existing one). Then build everything else, so the plugin is useful while it waits.
4. Write the code in `src/lib.rs`; long work goes in a job.
5. Test in `tests/` with `angelica-fake-game`: every command, window action, and handled failure. Run the checks in `AGENTS.md`.
6. Build and install while the game runs (commands in `AGENTS.md`), then tell the user exactly what to click or type in the game and what they should see. Report only what was observed.
";

const GITIGNORE: &str = "/target\n/.cargo/config.toml\n";

/// The toolchain of the Angelica checkout, which builds the component.
const TOOLCHAIN: &str = include_str!("../../../rust-toolchain.toml");

/// `abc-def` as `AbcDef`.
fn type_name(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect()
}

/// `abc-def` as `Abc Def`.
fn display_name(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + chars.as_str()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn fill(template: &str, id: &str, angelica: &Path) -> String {
    template
        .replace("{id}", id)
        .replace("{crate}", &id.replace('-', "_"))
        .replace("{type}", &type_name(id))
        .replace("{name}", &display_name(id))
        .replace(
            "{angelica}",
            &angelica.display().to_string().replace('\\', "/"),
        )
}

fn create(args: &NewArgs) -> Result<PathBuf, String> {
    let manifest = fill(MANIFEST, &args.id, Path::new(""));
    runtime::manifest::Manifest::parse(&manifest).map_err(|e| e.to_string())?;
    if args.id.len() > 32 || args.id.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(
            "the id is also the chat command: at most 32 bytes, starting with a letter".into(),
        );
    }
    let angelica = match &args.angelica {
        Some(path) => path.clone(),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .map_err(|e| format!("the Angelica checkout: {e}"))?,
    };
    if !angelica.join("crates/angelica-sdk/Cargo.toml").is_file() {
        return Err(format!(
            "{} is not an Angelica checkout; give --angelica",
            angelica.display()
        ));
    }
    let angelica = PathBuf::from(
        angelica
            .display()
            .to_string()
            .trim_start_matches(r"\\?\")
            .to_owned(),
    );
    let folder = args.directory.join(&args.id);
    if folder.exists() {
        return Err(format!("{} already exists", folder.display()));
    }
    for (path, template) in [
        ("Cargo.toml", CARGO),
        ("angelica.toml", MANIFEST),
        ("src/lib.rs", LIB),
        ("tests/plugin.rs", TEST),
        ("AGENTS.md", AGENTS),
        (".claude/skills/angelica-plugin/SKILL.md", SKILL),
        (".gitignore", GITIGNORE),
        (".cargo/config.toml", CARGO_CONFIG),
        ("rust-toolchain.toml", TOOLCHAIN),
    ] {
        let path = folder.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, fill(template, &args.id, &angelica))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    // The templates are formatted for a short id; formatting is best effort.
    let _ = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .arg("fmt")
        .current_dir(&folder)
        .output();
    Ok(folder)
}

pub fn run(args: &NewArgs) -> ExitCode {
    match create(args) {
        Ok(folder) => {
            if args.output.json {
                println!("{}", json!({ "created": folder.display().to_string() }));
            } else {
                println!(
                    "created {}; cargo test there builds and tests the plugin",
                    folder.display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            if args.output.json {
                println!("{}", json!({ "error": { "message": message } }));
            } else {
                eprintln!("error: {message}");
            }
            ExitCode::FAILURE
        }
    }
}
