---
order: 2
section: Build
meta: About 30 minutes · No game needed · Rust
prev: 
next: Run your plugin in the game
---

# Tutorial: Write your first plugin

This tutorial takes you from an empty folder to a working plugin that you changed and tested yourself.
You do not need the game running. It takes about 30 minutes, most of it the first build.

The plugin is called `hello`. It does three things:

- When the player types `/hello` in chat, it prints a greeting in the chat log.
- It has a small window that shows how many times it greeted the player.
- By the end, it reads the player's character from the game and greets it by name and level.

In this tutorial, you:

1. [Create the project](#create-the-project)
1. [Run the tests](#run-the-tests)
1. [Read the plugin](#read-the-plugin)
1. [Make the count survive a game restart](#make-the-count-survive-a-game-restart)
1. [Prove it with a test](#prove-it-with-a-test)
1. [Greet the character by name](#greet-the-character-by-name)
1. [Test the greeting](#test-the-greeting)

## Before you begin

You need:

- Windows 10 or later.
- [Git](https://git-scm.com/).
- [Rust](https://rustup.rs/), installed with `rustup`. The project picks the right compiler version by itself.
- Basic Rust. If you can read a `struct` and a `match`, you know enough.

You do not need to know WebAssembly, and you do not need to know anything about the game's internals.

## Create the project

A plugin is an ordinary Rust project. The `angelica` command creates one for you.

1. Install the `angelica` command:

   ```shell
   cargo install --locked --git https://github.com/AngelicaProject/angelica angelica-cli
   ```

1. Go to the folder where you keep your projects, and create the plugin:

   ```shell
   angelica plugin new hello
   ```

   You see:

   ```plaintext
   created .\hello; cargo test there builds and tests the plugin
   ```

The word `hello` is the plugin's ID. It is also the chat command, `/hello`, so choose it with care for a real plugin.
You cannot change the ID after players install the plugin.

The new `hello` folder has three files you work with:

| File               | Description |
|--------------------|-------------|
| `angelica.toml`    | The manifest. It tells Angelica and the player what the plugin is and what it is allowed to do. |
| `src/lib.rs`       | The plugin. |
| `tests/plugin.rs`  | Tests that run the plugin without the game. |

## Run the tests

Run the tests before you change anything, so you know the project works on your computer.

```shell
cd hello
cargo test
```

The first run downloads and builds Angelica's test host. This takes a few minutes. Later runs take less than a minute.

You see three passing tests:

```plaintext
test greetings_start_again_after_a_game_restart ... ok
test the_command_greets_in_the_chat_log ... ok
test the_window_counts_greetings ... ok

test result: ok. 3 passed; 0 failed
```

These tests do not use a mock of your plugin. They load the compiled plugin into the same host that runs inside the game, next to a scripted game that the test controls.
If a test passes here, the same code path works in the game.

## Read the plugin

Open `src/lib.rs`. The whole plugin is about 50 lines. It has four parts.

### The state

```rust
struct Hello {
    open: bool,
    greetings: u32,
}
```

Everything the plugin remembers is in this struct. Here, that is whether the window is open and how many greetings it counted.

### The start

```rust
fn start() -> Result<Self, String> {
    if let Err(error) = commands::define(&[Command::new("hello", "Greets you")]) {
        angelica_sdk::warning(&format!("no command: {}", error.message));
    }
    Ok(Self {
        open: false,
        greetings: 0,
    })
}
```

Angelica calls `start` one time, when the plugin starts. The plugin registers the `/hello` command and returns its first state.

### The command

```rust
fn command(&mut self, invocation: Invocation) -> Result<(), String> {
    if invocation.is(&["hello"]) {
        self.greetings += 1;
        Message::new().text("Hello from ").accent("Hello").print()?;
    }
    Ok(())
}
```

Angelica calls `command` when the player runs one of the plugin's commands. The plugin adds one to the count and prints a line in the chat log.

### The window

```rust
fn ui(&mut self) -> Vec<Window> {
    if !self.open {
        return Vec::new();
    }
    vec![
        Window::new("main", "Hello")
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
```

Angelica calls `ui` every frame and asks: which windows do you show right now?
The plugin does not create a window and keep it. It describes the window again each frame from its state.
When the player selects the button, Angelica calls `ui_event`, the plugin changes its state, and the next frame shows the new count.

### The permissions

Now open `angelica.toml`:

```toml
[permissions]
"chat.commands" = true
"chat.print" = true
```

A plugin can do only what its manifest declares, and the player approves each permission before the plugin runs.
This plugin asks for two things: its own chat command, and printing to the chat log.

## Make the count survive a game restart

The plugin has a flaw. The count is in memory, so it returns to zero each time the game starts.
The third test even says so: `greetings_start_again_after_a_game_restart`.

To fix it, save the count in the plugin's storage. Each plugin has its own folder for files, and using it needs no permission.

1. In `src/lib.rs`, add `storage` to the imports:

   ```rust
   use angelica_sdk::{
       Plugin,
       chat::Message,
       commands::{self, Command, Invocation},
       storage,
       ui::{Event, Window},
   };
   ```

1. Below the `struct Hello` definition, add a method that counts a greeting and saves the new count:

   ```rust
   impl Hello {
       fn count_greeting(&mut self) {
           self.greetings += 1;
           if let Err(error) = storage::save("greetings.json", &self.greetings) {
               angelica_sdk::warning(&format!("the count is not saved: {}", error.message));
           }
       }
   }
   ```

   If saving fails, the plugin writes a warning to its log and keeps working.

1. Use the method in both places that count. In `command`, replace `self.greetings += 1;`:

   ```rust
   self.count_greeting();
   ```

   In `ui_event`, replace the `Clicked` line:

   ```rust
   Event::Clicked(target) if target.id == "greet" => self.count_greeting(),
   ```

1. In `start`, load the saved count instead of starting from zero:

   ```rust
   Ok(Self {
       open: false,
       greetings: storage::load("greetings.json")?.unwrap_or(0),
   })
   ```

   The first time the plugin runs, the file does not exist yet. `load` returns `None`, and the count starts at zero.

## Prove it with a test

The old test checks the old behavior, so it now fails. Change it to describe what you want.

1. In `tests/plugin.rs`, replace the last test with this one:

   ```rust
   #[test]
   fn greetings_survive_a_game_restart() {
       let test = started();
       assert!(test.command("/hello"));
       let test = test.restart().unwrap();
       test.event("hello", Event::OpenRequested);
       assert_eq!(test.texts("hello"), ["1 greetings"]);
   }
   ```

   The test plays the player's part. It runs `/hello`, restarts the game, opens the window, and reads the text in it.

1. Run the tests:

   ```shell
   cargo test
   ```

   You see:

   ```plaintext
   test greetings_survive_a_game_restart ... ok
   test the_command_greets_in_the_chat_log ... ok
   test the_window_counts_greetings ... ok

   test result: ok. 3 passed; 0 failed
   ```

You changed a plugin and proved the change works, without starting the game.

## Greet the character by name

A plugin reads the game through Lens: it asks for the game's objects by name, such as the player's character, and the player approves which kinds of data it may read. In this step, `/hello` greets the character by name and level.

1. Ask to read the character. At the end of `angelica.toml`, add:

   ```toml
   [lens]
   read = ["character"]
   ```

   `character` is a domain of the game map. When the player approves the plugin, they see it as **Sees your character: name, class, level, HP and MP**. [Game data](reference-game-data.md) lists every domain and what it holds.

1. Add `serde`, which turns the rows a query returns into a Rust struct:

   ```shell
   cargo add serde --features derive
   ```

1. In `src/lib.rs`, add `lens` to the imports:

   ```rust
   use angelica_sdk::{
       Plugin,
       chat::Message,
       commands::{self, Command, Invocation},
       lens, storage,
       ui::{Event, Window},
   };
   ```

1. Give the plugin a field for the query, and a struct for the row it returns:

   ```rust
   struct Hello {
       open: bool,
       greetings: u32,
       character: Option<lens::Query>,
   }

   #[derive(serde::Deserialize)]
   struct Character {
       name: String,
       level: u8,
   }
   ```

1. In `start`, prepare the query once, and keep it:

   ```rust
   let character = lens::Query::prepare("player { name, level: data.level }")
       .inspect_err(|error| angelica_sdk::warning(&format!("no character: {}", error.message)))
       .ok();
   Ok(Self {
       open: false,
       greetings: storage::load("greetings.json")?.unwrap_or(0),
       character,
   })
   ```

   The query reads two fields of the root `player`. `level: data.level` names the column `level`, so it matches the struct's field. `prepare` checks the query against the game map and the plugin's domains; if the game lacks something it needs, the plugin logs a warning and works without it.

1. In `impl Hello`, add a method that runs the query:

   ```rust
   fn character(&self) -> Option<Character> {
       let rows = self.character.as_ref()?.rows::<Character>().ok()?;
       rows.into_iter().next()
   }
   ```

1. In `command`, greet the character when there is one:

   ```rust
   if invocation.is(&["hello"]) {
       self.count_greeting();
       match self.character() {
           Some(character) => Message::new()
               .text("Hello, ")
               .accent(character.name)
               .text(format!(", level {}", character.level))
               .print()?,
           None => Message::new().text("Hello from ").accent("Hello").print()?,
       }
   }
   ```

   Before the player logs in, the query returns no row, and the plugin greets as before.

## Test the greeting

The scripted game needs a game map to lay out the character. The test kit has a small one with the same names as the released map.

1. In `tests/plugin.rs`, in `started`, give the scripted game that map before the plugin is approved:

   ```rust
   test.install(MANIFEST, component()).unwrap();
   test.use_map(&angelica_fake_game::maps::EXAMPLE, &[]);
   test.host.queue(Action::Approve("hello".into()));
   ```

1. Add a test that sets the character and runs the command:

   ```rust
   #[test]
   fn the_command_greets_the_character_by_name() {
       let test = started();
       test.game.set(
           "player",
           &[("name", "Alisaie".into()), ("data.level", 100.into())],
       );
       assert!(test.command("/hello"));
       assert_eq!(
           test.game.printed_text(),
           ["[Hello] Hello, Alisaie, level 100"]
       );
   }
   ```

1. Run the tests:

   ```shell
   cargo test
   ```

   You see:

   ```plaintext
   test greetings_survive_a_game_restart ... ok
   test the_command_greets_in_the_chat_log ... ok
   test the_command_greets_the_character_by_name ... ok
   test the_window_counts_greetings ... ok

   test result: ok. 4 passed; 0 failed
   ```

The first test still passes: without a character set, the plugin greets as before.

## Next steps

- [Run your plugin in the game](run-in-the-game.md)
- [Read the game's sheets](https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/interfaces.md#sheets)
- [Do long work in a job](https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/interfaces.md#jobs)
- [Publish your plugin](https://github.com/AngelicaProject/angelica/blob/main/docs/plugins/authoring.md#6-publish)

## Troubleshooting

### The first `cargo test` fails to fetch the Angelica repository

The SDK and the test host come from the Angelica repository on GitHub. Check that https://github.com/AngelicaProject/angelica opens from this computer, then run `cargo test` again.

### `error: the id is also the chat command`

A plugin ID starts with a letter and is at most 32 characters. Use lowercase letters, digits, and hyphens.
