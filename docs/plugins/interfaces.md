# Interface cards

One card per task a plugin performs: the SDK entry point, the permission, the instance the calls work in, and what to remember. The [plugin API](../formats/plugin-api-v1.md) is the contract; each card links to its section, and the WIT files in [`wit/`](../../wit) document every function. "Main" is the plugin's main instance on the game's main thread; "job" is a [job](#jobs) instance.

A task the cards do not cover is not available to plugins yet: file an [API request](./api-requests.md).

## Windows

Show the plugin's own windows and react to the player.

- SDK: `ui::Window` built in `Plugin::ui`: headings and texts, buttons, checkboxes, text inputs, choices, numbers (`number`, `slider`), `tabs`, `table`, progress, game icons (`icon(number, size)`, the number from a sheet such as `Item`), `row`, `section`, and a `tooltip` for the widget before it. Actions arrive in `Plugin::ui_event` as `ui::Event`: `Clicked` (a button), `Edited` (a text input's new value), `Toggled` (a checkbox), `Chosen` (a choice's option or a tab, by index), `Set` (a number), `Closed`, and `OpenRequested`; each names the window and widget.
- Permission: none. Instance: main.
- Return the windows every frame; a window not returned is closed. Widgets are controlled: keep every checkbox, input, choice, number, and selected tab in the plugin and update it from the event. Tabs only draw the tab row: add the selected tab's widgets after it. An icon needs no permission; one the game does not have shows as an empty frame.
- `Event::OpenRequested` comes from the plugin's Open button in the Angelica menu; open the main window on it.
- Contract: [windows](../formats/plugin-api-v1.md#windows).

```rust
Window::new("main", "Timers")
    .tabs("view", &["Running", "Settings"], self.tab)
    .table("timers", &["", "Timer", "Left"], |table| {
        self.timers.iter().fold(table, |table, timer| {
            table.icon(timer.icon, 20).text(&timer.name).text(timer.left())
        })
    })
    .slider("warning", "Warn before (s)", self.warning, 0.0, 30.0)
    .tooltip("Seconds before the end to print a warning")
    .input("name", "Name", &self.name, "a timer name")
    .row(|row| row.button("add", "Add").button_enabled("clear", "Clear", !self.timers.is_empty()))
```

## Commands

Let the player drive the plugin from the chat box or macros.

- SDK: `commands::define(&[Command])` in `start`; runs arrive in `Plugin::command` as `commands::Invocation`.
- Permission: `chat.commands`. Instance: main.
- Parameters are `word`, `text` (the rest of the line, so only last), `integer` with bounds, `number`, `choice`, and `switch`; `.optional()` ones follow the required ones. Read them with `invocation.word(name)`, `integer`, `number`, `switch`.
- The host parses arguments by their kinds and answers `help` and malformed lines itself. An error returned from `command` is printed to the player.
- A root name must not be a game command in any language, `angelica`, or another plugin's root. Name commands for what they do; never like the game's commands that talk to other players (`say`, `shout`, `tell`).
- Contract: [commands](../formats/plugin-api-v1.md#commands).

```rust
commands::define(&[Command::new("timer", "Starts a timer")
    .parameter(Parameter::integer("seconds", "How long", 1, 3600))])?;
// In `command`:
if invocation.is(&["timer"]) { let seconds = invocation.integer("seconds").unwrap_or(60); }
```

## Chat messages from the plugin

Tell the player something in the chat log.

- SDK: `chat::Message::new().text(..).accent(..).print()`, or `chat::print(text)`.
- Permission: `chat.print`. Instance: main.
- Every message starts with the plugin's name. At most 20 messages a second; before the chat log exists (during the game's start) printing returns `unavailable`.
- Use ASCII `...`, never `…`.
- Contract: [chat](../formats/plugin-api-v1.md#chat).

## Chat messages the game shows

React to what appears in the chat log.

- SDK: `events::subscribe(&[EventKind::ChatMessage])` in `start`; messages arrive in `Plugin::game_event` as `GameEvent::ChatMessage`.
- Permission: `chat.read`. Instance: main.
- `kind` is a row of the `LogKind` sheet; `sender` and `message` are the game's text with its macros. Messages printed by plugins are not delivered.
- Events arrive in the next frame, after `tick` and commands. A `Dropped` event reports how many were lost when the game produced too many.
- Contract: [events](../formats/plugin-api-v1.md#events).

## The player's state

Read the player's character and where the player is.

- SDK: `state::read_one("self", &["level", "class_job"])` for a table of one object, `state::read("self.statuses", &[..])` for a list, `state::tables()` for every table and field with whether it is present.
- Permission: `state.self`. Instance: main.
- No row means the object does not exist, such as before log-in. A field whose map entries the running game build lacks returns `unavailable`; handle it as "not known yet", not as a fault.
- Numbers that identify game data (`class_job`, `territory_type`, `status_id`) are rows of the sheets the field documentation names; read their names with [sheets](#sheets).
- Contract: [game state](../formats/plugin-api-v1.md#game-state).

## Sheets

Read the game's data: items, actions, places, and the names the game shows.

- SDK: `excel::row(sheet, excel::ENGLISH, &["name"], row)`, `excel::rows(sheet, language, &[..])` to iterate.
- Permission: none. Instance: main and job.
- Columns are read by the names the game map's sheet schemas give them (`angelica excel show`); a sheet the map does not know in the running build returns `unavailable`, a name the schema lacks `not-found`.
- Text values are the game's encoded strings with macros: `value.plain()` is the text as the player reads it; `angelica_sdk::text` checks strings and builds text tables.
- `excel::sheet(name)?.rows` is the row count, for progress.
- A single row is fast; iterating a large sheet belongs in a job.
- Contract: [game files and sheets](../formats/plugin-api-v1.md#game-files-and-sheets).

## Game files

Read a file from the game's archives, such as a font or a texture.

- SDK: `api::game_files::read(path)`.
- Permission: none. Instance: main and job.
- Always the game's original file, never another plugin's replacement. At most 64 MiB.
- Contract: [game files and sheets](../formats/plugin-api-v1.md#game-files-and-sheets).

## Text the game shows from its sheets

Replace text cells of the game's sheets, for translations or annotations.

- SDK: `text::table::Writer` builds a [text table](../formats/text-table-v1.md); write it with `api::storage::write` and activate it with `api::excel_text::activate(name, path, priority)`.
- Permission: `excel.text`. Instance: main; build large tables in a job.
- Each cell has a guard of the game's original text, so a patched text is left alone. Activate in `start` while the game starts to cover everything it loads; `api::excel_text::status` reports what applied.
- Contract: [text tables](../formats/plugin-api-v1.md#text-tables).

## Letter case

Teach the game's capitalization letters it does not know.

- SDK: `api::text_case::set_letters(&[Letter { lower, upper }])`.
- Permission: `text.case`. Instance: main.
- Contract: [letter case](../formats/plugin-api-v1.md#letter-case).

## The game's own windows

Hide parts of the game's interface, or put your own text after, before, or instead of the text of one of its elements.

- SDK: `game_ui::hide(&[game_ui::element("_Exp", "window")])`, `game_ui::texts(&[game_ui::after("_Exp", "text", " (75%)")])`; `game_ui::elements("_Exp")` lists the names a window has.
- Permissions: `gameui.hide` for hiding, `gameui.text` for text. Instance: main.
- Each call replaces the plugin's whole list; an empty list removes the rules. Call `texts` again when your value changes; the host keeps the text in place when the game rewrites its own.
- Both return the elements the running game build has no names for; decide whether your feature works without them. A window or element the game map does not name needs an [API request](./api-requests.md) for the name.
- Contract: [game windows](../formats/plugin-api-v1.md#game-windows).

## Replacement game files

Serve a changed font or texture instead of the game's.

- SDK: write the file with `api::storage::write`, then `api::file_replacements::replace(game_path, storage_path)`.
- Permission: `files.replace`. Instance: main.
- Only formats the host can check: font tables and textures with the original's header and size.
- Contract: [replacement files](../formats/plugin-api-v1.md#replacement-files).

## Jobs

Do long work without stalling the game: bulk sheet reads, parsing downloads, requests.

- SDK: `jobs::start::<Output>(name, &input)` returns a `Job`; poll it in `tick` with `job.poll()`; in `Plugin::run_job` do the work with `jobs::run(&input, |input: Input| ...)`. Inputs and outputs are any serde types. `jobs::report(fraction, message)` reports progress.
- Permission: none. Instance: started from main; runs in a job instance.
- A job has its own memory and never runs `start`: pass it everything it needs in its input, or in storage. A plugin has at most four jobs at once, counting finished ones until they are polled and cancelled ones until they stop; dropping a `Job` cancels it.
- A job may read storage, game files, sheets, and the internet; it cannot change the game, print, or read state.
- Contract: [calls into the plugin](../formats/plugin-api-v1.md#calls-into-the-plugin).

```rust
self.job = Some(jobs::start::<Vec<String>>("names", &language)?);
// In `tick`:
if let Some(job) = &mut self.job
    && let Poll::Done(result) = job.poll()
{
    self.job = None;
    self.names = result?;
}
// In `run_job`:
"names" => jobs::run(&input, |language: u8| {
    excel::rows("Item", language, &["name"])
        .map(|row| Ok(row?.values[0].plain().unwrap_or_default()))
        .collect::<Result<Vec<String>, angelica_sdk::Error>>()
        .map_err(String::from)
}),
```

## Time

Count time, run timers, show countdowns.

- SDK: `Plugin::tick(elapsed)` receives the seconds since the previous frame; sum it for timers. `std::time::Instant` and `SystemTime` also work in a plugin.
- Permission: none. Instance: main; `Instant` also in a job.
- In tests, `test.advance(seconds)` runs one frame in which that much time passed; `test.frames(n)` runs frames of 1/60 second, so compare sums with a small tolerance.

## Settings and files

Keep settings and data across game starts.

- SDK: `storage::load::<T>(path)` (`None` the first time) and `storage::save(path, &value)` keep serde values as JSON; `api::storage` reads and writes bytes, lists, renames, and removes.
- Permission: none. Instance: main and job.
- Paths are relative and stay in the plugin's folder; at most 4 GiB in total. The player can open the folder from the Angelica menu.
- Contract: [storage](../formats/plugin-api-v1.md#storage).

## The internet

Fetch data from the plugin's own servers.

- SDK: `http::get(url)`, or `http::Request::new(url).header(..).get()`; `.download(path)` writes a large body to storage. A response has `is_success`, `header`, `text`, and `json`.
- Permission: `http.get`, with each host listed in the manifest's `[http] hosts`. Instance: job only.
- HTTPS GET only, to public addresses of the listed hosts, also after redirects. Any status is a response; `network` means the request did not complete.
- Data from the internet is untrusted: parse it in the job and handle malformed input as an error.
- Contract: [HTTP](../formats/plugin-api-v1.md#http).

## Log

Leave a trace for the player and for debugging.

- SDK: `angelica_sdk::info`, `warning`, `error`.
- Permission: none. Instance: main and job.
- The log is shown under Settings → For developers in the Angelica menu.
