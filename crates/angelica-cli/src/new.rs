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
    /// A local Angelica checkout to build the SDK and the test host from,
    /// for work on Angelica itself; without one they come from GitHub.
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

# Both come from the Angelica repository. `angelica plugin new --angelica`
# writes `.cargo/config.toml`, not committed, which builds them from a local
# checkout instead.
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

/// CI for every push and pull request.
const CI: &str = r"name: CI

on:
  push:
    branches: [main]
  pull_request:

permissions:
  contents: read

jobs:
  ci:
    runs-on: windows-latest
    defaults:
      run:
        shell: bash
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
        with:
          persist-credentials: false
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master
        with:
          toolchain: 1.98.1
          components: rustfmt, clippy
          targets: wasm32-wasip2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test
";

/// A release from a tag `v<version>`: the plugin file signed with the
/// author key, in the tag's release and behind the link that stays the same.
const RELEASE: &str = r#"name: Release

# A tag v<version> matching plugin.version in angelica.toml releases the
# plugin: CI signs it with the author key (the repository secret
# ANGELICA_AUTHOR_KEY), puts it into the tag's release, which the Angelica
# catalog checks, and behind the link players install from:
# https://github.com/<owner>/<name>/releases/download/{id}/{id}.angelica
on:
  push:
    tags:
      - "v*"

permissions: {}

jobs:
  release:
    runs-on: windows-latest
    permissions:
      contents: write
    env:
      GH_TOKEN: ${{ github.token }}
    defaults:
      run:
        shell: bash
    steps:
      - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
        with:
          persist-credentials: false
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@7e38f4b43b4db5c8dd498af069a4f6196df1d067 # master
        with:
          toolchain: 1.98.1
          targets: wasm32-wasip2

      - name: Check the tag
        run: |
          declared=$(sed -n '/^\[plugin\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' angelica.toml)
          if [ "v$declared" != "$GITHUB_REF_NAME" ]; then
            echo "::error::the tag $GITHUB_REF_NAME does not match plugin.version $declared"
            exit 1
          fi

      - run: cargo test
      - run: cargo build --release --target wasm32-wasip2
      - name: Install the angelica tool
        run: cargo install --locked --git https://github.com/AngelicaProject/angelica angelica-cli --root "$RUNNER_TEMP/angelica"

      - name: Sign and publish
        env:
          ANGELICA_AUTHOR_KEY: ${{ secrets.ANGELICA_AUTHOR_KEY }}
        run: |
          key="$RUNNER_TEMP/author.key"
          printf '%s' "$ANGELICA_AUTHOR_KEY" > "$key"
          tool="$RUNNER_TEMP/angelica/bin/angelica.exe"
          component=target/wasm32-wasip2/release/{crate}.wasm
          mkdir -p dist
          "$tool" plugin pack . --component "$component" --key "$key" --output dist/{id}.angelica
          "$tool" plugin publish . --component "$component" --github "$GITHUB_REPOSITORY" --key "$key"
          rm -f "$key"
          gh release create "$GITHUB_REF_NAME" dist/{id}.angelica --title "{name} ${GITHUB_REF_NAME#v}" --generate-notes
"#;

/// With `--angelica`: the local Angelica checkout the dependencies build
/// from; kept out of the repository, so its path stays on this machine.
const CARGO_CONFIG: &str = r#"[patch."https://github.com/AngelicaProject/angelica"]
angelica-sdk = { path = "{angelica}/crates/angelica-sdk" }
angelica-fake-game = { path = "{angelica}/crates/angelica-fake-game" }
"#;

const AGENTS: &str = r#"# {name}

An Angelica plugin for FINAL FANTASY XIV, in Rust, compiled to a WebAssembly component. This file is the guidance for any coding agent working on it.

## Read first

- https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/authoring.md: the work loop from an idea to a published plugin.
- https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/interfaces.md: which interface, permission, and SDK call serve each task.
- https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/api-requests.md: what to write when the API lacks a capability.
- https://github.com/AngelicaProject/angelica/blob/main/docs/formats/plugin-api-v1.md: the contract. After the first `cargo test`, cargo's copy of the Angelica repository holds the SDK (`crates/angelica-sdk`), the test kit (`crates/angelica-fake-game`), and the WIT files (`wit/`); `cargo doc --open` shows the SDK.

## Work loop

1. Before any code, design the whole feature in game terms: what it reads, what it changes, how the player drives it. Show the plan and the permissions it needs to the user.
2. Look up every piece in the interface cards: SDK call, permission, main instance or job. For the game's objects, find the roots, types, and fields with `angelica schema search` and check each query with `angelica query`. Add the permissions to `angelica.toml`, and the domains the queries read to its `[lens] read`; nothing else.
3. Collect every piece the cards do not cover at once: those are not available. Look for open requests first (`gh issue list --repo AngelicaProject/angelica --label api-request`). Write each new request in the format of `api-requests.md` and show it to the user. File it with `gh issue create`, as `api-requests.md` shows, only once the user agrees. Then build everything else, so the plugin is useful while it waits.
4. Write the code in `src/lib.rs`; long work goes in a job.
5. Test in `tests/` with `angelica-fake-game`: every command, window action, and handled failure. Run the checks below.
6. When the user has Angelica installed, build, pack, and install the plugin (commands below), then tell the user exactly what to click or type in the game and what they should see. The user approves the plugin's permissions in the game; never approve them for the user. Report only what was observed.
7. Publish only when the user asks: https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/authoring.md#6-publish.

## Commands

The work needs `cargo`, `git`, and the GitHub command `gh` signed in to the user's account (https://cli.github.com/). When `gh` is missing or not signed in, ask the user to install it and run `gh auth login`.

- `cargo test`: builds the component and runs `tests/` against the scripted game. The first run fetches the Angelica repository and builds the host, which takes a few minutes; later runs are quick.
- `cargo clippy --target wasm32-wasip2 -- -D warnings -W clippy::pedantic` for the plugin, `cargo clippy --tests -- -D warnings -W clippy::pedantic` for the tests, and `cargo fmt --check`.
- `cargo build --release --target wasm32-wasip2` builds `target/wasm32-wasip2/release/{crate}.wasm`.
- `angelica plugin pack --component target/wasm32-wasip2/release/{crate}.wasm` signs it into `{id}-<version>.angelica` with the author key.
- `angelica plugin install {id}-<version>.angelica --installation <installation>` installs it into Angelica (`%LOCALAPPDATA%\Angelica` for an installed Angelica); the running game loads it within a second.
- `angelica excel sheets --game <game folder>` and `angelica excel show <sheet> --game <game folder>` show the game's own data, such as item names and recipes, from the installed game's `game` folder.
- `angelica schema search <words> --map <map> --game <game folder>` and `angelica schema show <root or type> --map <map>` show what the game map describes of the game's objects: roots, types, fields, and the domain of each; `angelica query "<query>" --map <map> --game <game folder> --plugin .` checks a query and the domains `angelica.toml` lists. The map of an installed Angelica is its `map` folder (`%LOCALAPPDATA%\Angelica\map`).

## Rules

- Use only the plugin API. There are no game addresses, offsets, hooks, or memory reads in a plugin. When the API lacks a capability, do not work around it: write an API request.
- Declare only the permissions the plugin uses; the player sees each one.
- Never read, print, or copy the author key (`%APPDATA%\Angelica\author.key`). The user puts it into the repository secret `ANGELICA_AUTHOR_KEY` themselves.
- Long work (bulk sheet reads, HTTP) runs in a job; frame callbacks have a few milliseconds.
- For your own serde types, add the `serde` crate with its `derive` feature.
- Text printed to the chat log uses ASCII `...`, never `…`.
- Name commands for what they do; never like the game's commands that talk to other players (`say`, `shout`, `tell`).
"#;

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

/// The Angelica checkout at `path`, as the project's `.cargo/config.toml`
/// names it.
fn checkout(path: &Path) -> Result<PathBuf, String> {
    // The project's `.cargo/config.toml` resolves paths from the project.
    let angelica = path
        .canonicalize()
        .map_err(|e| format!("{} is not an Angelica checkout: {e}", path.display()))?;
    if !angelica.join("crates/angelica-sdk/Cargo.toml").is_file() {
        return Err(format!(
            "{} is not an Angelica checkout",
            angelica.display()
        ));
    }
    Ok(PathBuf::from(
        angelica
            .display()
            .to_string()
            .trim_start_matches(r"\\?\")
            .to_owned(),
    ))
}

fn create(args: &NewArgs) -> Result<PathBuf, String> {
    let manifest = fill(MANIFEST, &args.id, Path::new(""));
    runtime::manifest::Manifest::parse(&manifest).map_err(|e| e.to_string())?;
    if args.id.len() > 32 || args.id.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(
            "the id is also the chat command: at most 32 bytes, starting with a letter".into(),
        );
    }
    let angelica = args.angelica.as_deref().map(checkout).transpose()?;
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
        (".github/workflows/ci.yml", CI),
        (".github/workflows/release.yml", RELEASE),
    ] {
        let path = folder.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, fill(template, &args.id, Path::new("")))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    if let Some(angelica) = angelica {
        let path = folder.join(".cargo/config.toml");
        std::fs::create_dir_all(folder.join(".cargo"))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::write(&path, fill(CARGO_CONFIG, &args.id, &angelica))
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
