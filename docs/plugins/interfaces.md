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

## The game's objects

Read the player's character, where the player is, the bags, the game's open windows and what their dialogs ask, and whatever else the game map's object model describes, by path.

- SDK: `lens::Query::prepare("inventory.containers[kind = bag_1].items { item, quantity }")?` once, in `start` or when first needed; then `query.rows::<T>()?` with a `serde::Deserialize` structure whose fields are named like the columns, or `query.run()?` and `query.get(&row, "item")`.
- Permission: each domain the query reads, listed in the manifest as `[lens] read = ["inventory"]`. Instance: main.
- No row means the root's object does not exist, such as the character before log-in. A query that names something the running build lacks fails with `unavailable`; handle it as "not known yet", not as a fault. After a map update a held query is planned again.
- Numbers that identify game data are sheet rows (`Value::Sheet`, a `u32` in a structure); read their names with [sheets](#sheets). An enum value is its name; flags are their names (a `Vec<String>` in a structure) and bits.
- Find roots, types, fields, and domains with `angelica schema search` and `schema show`; check a query with `angelica query`.
- Contract: [Lens](../formats/plugin-api-v1.md#lens).

## The game's events

Learn when the game does something the game map describes as an event, such as the player using an action, with the values of that moment.

- SDK: `let watch = lens::Watch::new("action_used[kind in (action, item)] { kind, action }")?` in `start`, kept in the plugin; then in `Plugin::game_event`, `watch.decode::<T>(&event)` or `watch.rows(&event)` for the rows of one firing, and `watch.dropped(&event)` for firings that were left out. Dropping the `Watch` ends it.
- Permission: each domain the watch reads, as for queries. Instance: main.
- Firings arrive in the next frame, one game event each, in the order they happened. A watch keeps a limited number per frame, set by the event in the map; the rest arrive counted as dropped.
- An event the running build lacks, or whose point the host cannot hook, fails with `unavailable`.
- Find events and their arguments with `angelica schema search` and `schema show`; check a watch with `angelica query`; see it fire in the game with `angelica look --watch 10 "<watch>"`.
- Contract: [Lens](../formats/plugin-api-v1.md#lens).

## Deciding what the game does

Make the game skip something it is about to do, such as adding a message to the chat log, by a rule the host applies itself.

- SDK: `let quiet = lens::Rule::new("chat_message_added[kind = 57]", "hide")?;` and keep it in the plugin; dropping it ends the rule.
- Permission: the event's domain in the manifest's `[lens] change = ["chat"]`; domains on the rule's paths in `read` or `change`. Instance: main.
- The plugin is not called when the rule decides: put everything the decision needs into the rule's conditions. Find decision points and their outcomes with `angelica schema show <event>`.
- Contract: [Lens](../formats/plugin-api-v1.md#lens).

## Changing what you see

Change a value the game keeps on the player's computer alone, such as how far the camera zooms out.

- SDK: `lens::set("camera.camera.max_distance", 40.0f32)?` returns how many values it changed; `lens::restore("camera.camera.max_distance")?` puts the originals back. The host puts them back when the plugin stops too.
- Permission: each domain the plugin changes, listed in the manifest as `[lens] change = ["camera"]`. Instance: main.
- Only fields the map marks changeable, within their range; `schema show` says which. Set again when the game makes the object again: a change lives in the object it was written to.
- Contract: [Lens](../formats/plugin-api-v1.md#lens).

## Acting in the game

Do something in the game for the player, as the game's own interface would: use an action or an item, mount, teleport, answer a yes or no dialog, choose an entry of a list of choices.

- SDK: `lens::act("use_action", &[("kind", lens::named("general_action")), ("action", 4u32.into())])?`; parameters by name, numbers with `.into()`, enum values with `lens::named`. A parameter left out takes the map's default.
- Permission: each domain the plugin acts in, listed in the manifest as `[lens] act = ["actions"]`; the player sees it under **Acts in the game for you**, with a warning that automation breaks the game's terms. Instance: main, from `tick`, a command, or an event.
- `unavailable` is normal: a condition does not hold now ("you are in combat"), or the game refused. Say what happened to the player; do not retry every frame. `limit-exceeded` means the action's rate; `permission-denied` with the stop message means the player stopped plugins from acting, so stop until they let them act again.
- Find actions and their parameters with `angelica schema search` and `schema show <action>`.
- Contract: [Lens](../formats/plugin-api-v1.md#lens).

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
- A job may read storage, game files, sheets, and the internet; it cannot change the game, print, or read the game's objects.
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
- Files the plugin can fetch again go under `cache/`, with `storage.cache` and the size in the manifest's `[storage] cache = 64` (GiB); the cache has its own quota and goes away with the plugin.
- Contract: [storage](../formats/plugin-api-v1.md#storage).

## The internet

Fetch data from the plugin's own servers, and send data to them.

- SDK: `http::get(url)`, or `http::Request::new(url).header(..).get()`; `.download(path)` writes a large body to storage; `.header("content-type", ..).send(http::Method::Post, &body)` sends data, and `.upload(http::Method::Put, path)` sends a file from storage of up to 1 GiB. A response has `is_success`, `header`, `text`, and `json`.
- Permission: `http.get`, with each host listed in the manifest's `[http] hosts`; `http.send` too for sending data. Instance: job only.
- HTTPS only, to the listed hosts, also after redirects, through the player's proxy when Windows has one; never to the player's home network. Any status is a response; `network` means the request did not complete.
- Data from the internet is untrusted: parse it in the job and handle malformed input as an error.
- Contract: [HTTP](../formats/plugin-api-v1.md#http).

## Staying connected

Keep a live connection to the plugin's own service, which pushes data as it happens: a party's state, a synchronization server.

- SDK: `websocket::Socket::open("wss://sync.example.org/live")?`, then in `Plugin::game_event`: `GameEvent::SocketOpened`, `GameEvent::SocketMessage` with `Data::Text` or `Data::Binary`, and `GameEvent::SocketClosed`; `socket.send_text(..)`, `socket.send_binary(..)`, `socket.close()`.
- Permission: `http.websocket`, with `http.get` and the host in `[http] hosts`. Instance: main.
- `open` returns at once; nothing can be sent before `SocketOpened`. Messages are at most 16 MiB; at most 4 connections. A closed connection does not reopen by itself: open a new one, waiting a little longer after each failure.
- Contract: [WebSocket](../formats/plugin-api-v1.md#websocket).

## Talking to other plugins

Exchange data with another plugin, such as asking an appearance plugin for a character's state.

- SDK: `messages::send("glamourer", "state", &payload)?`; the answer arrives in `Plugin::game_event` as `GameEvent::Message` with `in_reply_to` set. Answer a message with `messages::reply(&message, &payload)?`.
- Permission: `messages.send`, with each plugin listed in the manifest's `[messages] to = ["glamourer"]`; answering needs none. Instance: main.
- Delivery is in the next frame, never inside the sender's call. Agree on names and payloads with the other plugin's author, and treat a payload as untrusted input.
- Contract: [messages](../formats/plugin-api-v1.md#messages-between-plugins).

## Copying to the clipboard

Give the player text to paste elsewhere: a position, a link, a macro.

- SDK: `clipboard::copy("text")?` in `Plugin::ui_event` or `Plugin::command`.
- Permission: `clipboard.write`. Instance: main, while handling the player's click or command; anywhere else the call is refused.
- The text reaches the clipboard at the end of the frame, at most 64 KiB.
- Contract: [clipboard](../formats/plugin-api-v1.md#clipboard).

## Notifying the player

Tell the player something happened: a duty is ready, a timer ran out.

- SDK: `notifications::show("The duty is ready")?`, or `notifications::alert(...)` to also flash the game on the taskbar while the player is in another window.
- Permission: `notifications`. Instance: main.
- One line of at most 200 characters, at most one every 2 seconds; the host draws it under the plugin's name for 7 seconds.
- Contract: [notifications](../formats/plugin-api-v1.md#notifications).

## Playing a sound

Alert the player by ear: a ready check, a timer.

- SDK: `sound::play("ready.wav", 0.8)?` with a WAV file in the plugin's storage: shipped with the plugin, downloaded, or put in its folder by the player.
- Permission: `sound.play`. Instance: main.
- PCM or float WAV, 1 or 2 channels, at most 10 seconds and 8 MiB; at most one sound a second. A new sound stops the one playing.
- Contract: [sound](../formats/plugin-api-v1.md#sound).

## Drawing over the world

Mark places in the game world: a circle on the ground, a line between two places, a label above a spot.

- SDK: `draw::set(&[draw::circle([x, y, z], 5.0, 0xE8735AFF), draw::text([x, y + 2.5, z], "here", 0xFFFFFFFF)])?`; `draw::line`, `draw::disc` for a filled circle, `draw::clear()`. Places come from Lens, such as `player { position_x, position_y, position_z }`.
- Permission: `draw.world`. Instance: main.
- Shapes stay until replaced: set them again when what they mark moves, not every frame when nothing changed. Colors are `0xRRGGBBAA`.
- Contract: [drawing](../formats/plugin-api-v1.md#drawing-over-the-world).

## Log

Leave a trace for the player and for debugging.

- SDK: `angelica_sdk::info`, `warning`, `error`.
- Permission: none. Instance: main and job.
- The log is shown under Settings → For developers in the Angelica menu.
