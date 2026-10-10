# Writing a plugin

A plugin is a Rust crate compiled to a WebAssembly component. It runs in the host's sandbox, reaches the game only through the [plugin API](../formats/plugin-api-v1.md), and keeps working across game patches because it names game data and state by the game map's names, never by address. This guide is the work loop from an idea to a published plugin; the [interface cards](./interfaces.md) map tasks to interfaces.

## 1. Create the project

```text
angelica plugin new <id>
```

`<id>` is the plugin's permanent id: lowercase letters, digits, and inner hyphens, starting with a letter, at most 32 bytes because it is also the root chat command. The project is a working plugin with a window and a `/<id>` command:

| File | Holds |
| --- | --- |
| `angelica.toml` | the [manifest](../formats/plugin-manifest-v1.md): id, name, version, API version, permissions, HTTP hosts |
| `src/lib.rs` | the plugin: a type that implements `angelica_sdk::Plugin`, exported with `angelica_sdk::plugin!` |
| `tests/plugin.rs` | tests that run the component in the real host against a scripted game |
| `AGENTS.md` | the work loop and rules for a coding agent, pointing here |
| `.claude/skills/angelica-plugin/SKILL.md` | the same work loop as a skill for coding agents that load skills |
| `rust-toolchain.toml` | the compiler the host's components are built with |
| `.cargo/config.toml` | not committed: points the dependencies at the local Angelica checkout |

The project depends on `angelica-sdk` and `angelica-fake-game` from the Angelica repository; `.cargo/config.toml` replaces them with the checkout the command came from (`--angelica <checkout>` for another), so local changes to the SDK apply at once and the checkout's path never enters the plugin's repository. The first `cargo test` fetches the repository once, with git and its credentials, and builds the host, which takes a few minutes; later runs are quick and offline. The test build runs offline, so a new dependency builds once `cargo fetch` has downloaded it. Each plugin lives in its own repository.

## 2. Find the interfaces

Describe what the plugin does in terms of the game: what it reads (sheets, the player's state, chat messages), what it changes (text, files, the chat log), and how the player drives it (windows, commands). Then find each piece in the [interface cards](./interfaces.md). Each card names the permission, the instance the calls work in, and the SDK entry point.

Explore the game's data from the command line before writing code:

```text
angelica excel sheets --game <game folder>
angelica excel show <sheet> --game <game folder> --map <map> --row <row>
```

`show` prints a sheet's layout, the names the game map gives its columns, and rows. A plugin reads a column only by such a name. State tables and their fields are listed in the game map's `state/` folder and returned by `state::tables()` at run time.

When a piece is missing — a sheet column without a name, a state field, an event, an operation on the game — the plugin does not work around it. File an [API request](./api-requests.md) and build the rest of the plugin meanwhile.

## 3. Write the plugin

The host calls the plugin's main instance on the game's main thread ([calls into the plugin](../formats/plugin-api-v1.md#calls-into-the-plugin)):

- `start` once; define commands, subscribe to events, load settings, activate text tables here.
- `tick` every frame; poll jobs here.
- `command`, `game_event`, `ui_event` as the player and the game act, then `ui` for the windows of this frame.
- `stop` before the plugin is disabled.
- `run_job` in a separate instance for long work.

Rules that keep a plugin within its limits:

- The calls of one frame share about 20 ms of guest time. Anything that reads many sheet rows, parses a large file, or makes a request runs in a [job](./interfaces.md#jobs).
- Windows are declared every frame from the plugin's state; widgets are controlled, so the plugin stores every value it shows.
- An error returned from `start` or `tick`, or a trap, faults the plugin: it stops, everything it activated is removed, and the player sees the reason. Return errors from `command` (printed to the player) and handle the rest.
- `?` converts a host error into the `String` the plugin's methods return.
- Declare only the permissions the plugin uses: the player approves each one, and a new version that declares more waits for approval again.

## 4. Test without the game

`cargo test` builds the component and runs `tests/`. `angelica_fake_game::TestHost` installs the component in the real host with temporary folders and a scripted game, and the test plays the game's part:

| The test wants | Use |
| --- | --- |
| the player approves the plugin | `test.host.queue(Action::Approve(id))`, then `test.frames(1)` |
| frames to pass | `test.frames(n)`: frames of 1/60 second each |
| time to pass | `test.advance(seconds)`: one frame in which `seconds` passed |
| the player runs a command | `test.command("/<id> ...")`; false when the game would handle the line |
| the player opens the window | `test.event(id, Event::OpenRequested)` |
| the player clicks, types, ticks, picks, or sets a number | `test.click(id, window, widget)`, `test.edit(.., value)`, `test.toggle(.., checked)`, `test.choose(.., index)` (also a tab), `test.set_number(.., value)` |
| a job to finish | `test.wait_until(\|test\| ...)`: frames until the condition holds, at most ten seconds |
| the game to restart | `let test = test.restart()?`: the same folders and packages; grants, settings, and storage carry over |
| a chat message to arrive | `test.chat_message(kind, sender, text)` |
| a sheet in the installed game | `game_files::FakeGame::new(version).with_text(sheet, &TextSheet::new(columns, &[text columns]).row(id, &[(column, text)]))` written to `test.game_folder()`, then `test.name_columns(sheet, &[(name, column)])`; `FakeSheet` with `ColumnKind` and `Language` for other column kinds |
| state rows | `test.define_state_table(...)` and rows in `test.game.state` |
| what the plugin printed | `test.game.printed_text()`; `test.game.clear_printed()` forgets it |
| what the plugin shows | `test.texts(id)`: the headings and texts of its windows; `test.host.plugin(id).unwrap().windows()` for every widget (types in `angelica_fake_game::ui`) |

The generated `tests/plugin.rs` shows the setup; [`crates/angelica-fake-game/tests/hello.rs`](../../crates/angelica-fake-game/tests/hello.rs) drives an example plugin through every interface. Cover each command, each window action, and each failure the plugin handles: a refused permission, an absent map entry, a failing job.

Run the lints the SDK is held to, for the plugin and for its tests:

```text
cargo clippy --target wasm32-wasip2 -- -D warnings -W clippy::pedantic
cargo clippy --tests -- -D warnings -W clippy::pedantic
cargo fmt --check
```

## 5. Run it in the game

Build the component and install it into a [development installation](../formats/host-installation-v1.md#development-installations):

```text
cargo build --release --target wasm32-wasip2
angelica dev install --key <dev key> --installation <installation> --plugin .=target/wasm32-wasip2/release/<crate>.wasm
```

While the game runs, the host loads the new version within a second ([reload](../architecture/plugins.md#lifecycle)). The Angelica menu (`Insert` by default) shows the plugin and its approval; Settings → For developers shows its log, a fault's reason, and the host's diagnostics. Check in the game what the tests cannot: how the windows look, the game's own data and messages, and behavior across a zone change or log-out.

## 6. Publish

There are two ways to players.

**From you, by a link.** `angelica plugin publish --github <owner/name> --component target/wasm32-wasip2/release/<crate>.wasm` signs the plugin with your author key and puts it behind a link that stays the same across versions, which it prints; it uses the `gh` command and its sign-in. The first run creates the key in `%APPDATA%/Angelica/author.key`. Keep the key: it is the plugin's identity, and installations accept new versions only with the same key. Players paste the link into the Angelica menu. Raise `plugin.version` for every release; installations update from the link on their own ([plugin file](../formats/plugin-file-v1.md)). `angelica plugin pack` writes the signed file without publishing it, for any other host. Players see that the plugin comes from you and not from the Angelica catalog, and approve its permissions as for any plugin; a version that needs more permissions waits for their approval.

**From the Angelica catalog.** Catalog plugins come from the [plugin registry](https://github.com/AngelicaProject/angelica-registry): it records which source commit of a plugin is published and the build that produced it, compiles the component outside the game with `angelica plugin compile`, and signs the result. Before submitting, bump `plugin.version`, make sure `plugin.description` tells the player what the plugin does, and that the manifest declares only the permissions and hosts the plugin uses.
