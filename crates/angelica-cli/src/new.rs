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

[dependencies]
angelica-sdk = { path = "{angelica}/crates/angelica-sdk" }

[dev-dependencies]
angelica-fake-game = { path = "{angelica}/crates/angelica-fake-game" }

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
        sandbox::{
            Limits,
            bindings::angelica::plugin::ui::{Widget, WidgetRef},
        },
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
    test.event(
        "{id}",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: "greet".into(),
        }),
    );
    let windows = test.host.plugin("{id}").unwrap().windows();
    assert!(matches!(&windows[0].widgets[0], Widget::Text(label) if label.text == "1 greetings"));
}
"#;

const AGENTS: &str = r"# {name}

An Angelica plugin for FINAL FANTASY XIV, in Rust, compiled to a WebAssembly component.

## Work loop

1. Find the capability in the plugin API: `{angelica}/docs/formats/plugin-api-v1.md`, the WIT files in `{angelica}/wit/`, and the SDK in `{angelica}/crates/angelica-sdk/`. Add the permissions it needs to `angelica.toml`.
2. Look at the game's data from the command line: `angelica excel sheets --game <game folder>`, `angelica excel show <sheet> --game <game folder> --map <map> --row <row>`. Values are read by the column names the game map gives; the game state by its state tables (`state` interface).
3. Write the plugin in `src/lib.rs` and a test in `tests/` with the scripted game: `angelica-fake-game` installs the plugin in a real host, plays the game's part (sheets, chat lines, state rows), and shows what the plugin printed and drew.
4. `cargo test` builds the component and runs the tests. `cargo build --release --target wasm32-wasip2` builds `target/wasm32-wasip2/release/{crate}.wasm`.
5. Install it in the development installation while the game runs: `angelica dev install --key <dev key> --installation <installation> --plugin .=target/wasm32-wasip2/release/{crate}.wasm`. The host loads the new version within a second; a version that declares a new permission waits for approval in the Angelica menu (Insert).

## Rules

- Use only the plugin API. There are no game addresses, offsets, hooks, or memory reads in a plugin. When the API lacks a capability, do not work around it: write an API request naming the game resource, the operation, and why the plugin needs it.
- Declare only the permissions the plugin uses; the player sees each one.
- Long work (bulk sheet reads, HTTP) runs in a job; frame callbacks have a few milliseconds. Start it with `angelica_sdk::jobs::start`, poll the `Job` in `tick`, and do the work in `Plugin::run_job` with `jobs::run`; inputs and outputs are any serde types (add the `serde` crate with its `derive` feature for your own). Requests use `angelica_sdk::http::get`.
- Settings and other saved values: `angelica_sdk::storage::load` and `save` (JSON in the plugin's folder).
- `?` turns a host error into the `String` that the plugin's methods return.
- Text printed to the chat log uses ASCII `...`, never `…`.
- Name commands for what they do; never like the game's commands that talk to other players (`say`, `shout`, `tell`).
";

const GITIGNORE: &str = "/target\n";

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
        (".gitignore", GITIGNORE),
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
