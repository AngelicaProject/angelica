# Plugin API 1.0

The plugin API is the WIT package `angelica:plugin@1.0.0` in [`wit/`](../../wit). A plugin is a WebAssembly component of its `plugin` world: it imports the host's interfaces and exports `guest`. The WIT files carry the doc comment of every function; this document states the rules they share. [`angelica-sdk`](../../crates/angelica-sdk) provides Rust bindings, a `Plugin` trait, and helpers for every interface.

A manifest declares the API version it was built for in `plugin.api` ([manifest](./plugin-manifest-v1.md)). This host runs `1.0`.

## Interfaces

| Interface | Provides | Permission |
| --- | --- | --- |
| `runtime` | the plugin's log, whether it started with the game, the game version | — |
| `storage` | the plugin's private folder | — |
| `ui` | windows the host draws (types used by `guest`) | — |
| `jobs` | long work in a separate instance on a host thread | — |
| `game-files` | files from the installed game's archives | — |
| `excel` | the game's Excel sheets as its files hold them | — |
| `excel-text` | replacement text for cells of the game's Excel sheets | `excel.text` |
| `text-case` | letters added to the game's letter case conversion | `text.case` |
| `file-replacements` | replacement game files the game reads instead of its own | `files.replace` |
| `commands` | chat commands the host parses for the plugin | `chat.commands` |
| `chat` | messages in the player's chat log | `chat.print` |
| `http` | HTTPS GET requests to the hosts the manifest lists | `http.get` |
| `state` | game state as the game map's tables: the player's character | `state.<domain>`: `state.self` |
| `events` | what the game did, delivered in the next frame: messages added to the chat log | one per kind: `chat.read` |
| `game-ui` | rules for the game's own windows: hidden elements and text in text elements | `gameui.hide`, `gameui.text` |

Every plugin has its log, folder, windows, and jobs. An operation that needs a permission the player did not grant returns `permission-denied`.

Every fallible call returns `types.error`: a stable `kind` for programs and a `message` that names the path, table, URL, or value involved. A guest that ignores an error keeps running; a trap disables it.

## Calls into the plugin

The host keeps one **main instance** per running plugin and calls it on the game's main thread:

- `start` once when the plugin is enabled. An error disables the plugin and is shown in the Angelica menu. During the game's own start, before any game code runs, `runtime.started-with-game` is true; a plugin enabled later sees a game that has already loaded data.
- `tick` every frame, then `command` for each of the plugin's [commands](#commands) the player ran since the previous frame, then `on-game-event` for each [event](#events) the plugin subscribed to, then `ui-event` for each player action, then `ui`.
- `stop` before the plugin is disabled and at game exit.

The calls of one frame — `tick`, the commands, the events, the window events, and `ui` — share one budget, and `start` and `stop` each have their own: a guest time limit, which does not count time spent in host calls, and a fuel limit (`host.json`, [installation](./host-installation-v1.md)), by default 20 ms for a frame's calls and 2 s for `start` and `stop`; and a limit on time including host calls, 2 s for a frame's calls and 30 s for `start` and `stop`, so a plugin cannot hold the game's main thread with host operations. Exceeding a limit traps. A frame in which a plugin's calls take more than 100 ms is noted in the Angelica menu.

A trap, a `start` or `tick` error, or invalid windows **fault** the plugin: the host stops it without calling `stop`, removes everything it activated (text tables, letters, replaced files, commands, event subscriptions, jobs, windows), and shows the reason. It stays off until the player retries it. The game never stops because of a plugin.

`run-job` runs in a **job instance**: a fresh instance of the component per job, with its own memory, on a host thread, for at most 10 minutes of guest time and without a fuel limit. A job instance never runs `start`. A job may use `runtime`, `storage`, `game-files`, `excel`, `http`, `state.tables`, and `jobs.report`; `excel-text`, `text-case`, `file-replacements`, `commands`, `chat`, `state.read`, `events`, `game-ui`, `jobs.start`, `jobs.poll`, and `jobs.cancel` return `wrong-context`, and `http` returns it in the main instance. A job's input and output are at most 16 MiB; a plugin has at most four jobs at once, counting finished jobs whose result `poll` has not returned yet and cancelled jobs until they stop. A trap or error fails only the job. Disabling the plugin cancels its jobs.

Each instance's memory is limited to the manifest's `plugin.memory_bytes`, or the host default of 64 MiB.

## Windows

Every frame the host draws the windows `ui` returns; a window that is not returned is not shown. The host frames each window and supplies the fonts, colors, and scaling; a plugin never draws.

A window's title bar shows its `title`, then its `path` — where the player is below the window's top level, outermost first, at most 4 non-blank parts — and its `status`, one short line about what the window is for, whose tone chooses its mark: `good` the mark of something running, `warning` and `bad` the mark of something that needs the player. When the player presses the back arrow, the title, or an earlier part of the path, the plugin receives `navigated` with the number of parts that stay (0 for the top level) and returns the window with that path; the host does not change the path itself. The bar's other controls — collapse, the window's menu, and close — are the host's ([interface](../architecture/ui.md#plugin-windows)).

Widgets are a flat list. `begin-row` … `end-row` lays widgets side by side, `begin-section` … `end-section` puts them under a heading the player can collapse, and `begin-table` … `end-table` shows column headings and fills the cells left to right, row by row, with one widget or one row per cell; they nest up to 16 deep, and a table holds no sections or tables. A `tooltip` shows its text while the player points at the widget right before it. Widgets are controlled: a checkbox, text input, choice, number, or tabs shows the value the plugin passes, and a change by the player arrives as an `event` before the next `ui` — `toggled`, `edited`, `chosen` (an option or a tab), or `set` (a number). While the player types in a text input, the host keeps the typed text; otherwise the plugin's value is shown. A number is a slider or a field the player drags or types into, between its `min` and `max`; with a `step`, the value set is a multiple of the step from `min`. An `icon` shows one of the game's icons by its number, the number game sheets hold (such as `Item`'s icon column), in a square of `size` 1–256; the host reads it from the game's archives, the high-resolution texture first, and an icon the game does not have, or one not read yet, shows as an empty frame. The host spends at most 2 ms of a frame reading icons and keeps up to 512 of them.

The host checks the windows before drawing them: at most 16 windows and 4096 widgets, ids of 1–128 bytes unique within a window (window ids unique per plugin), texts of at most 4096 bytes and 512 KiB in total, a path of at most 4 non-blank parts, at most 1024 options per choice and a selected option that exists, 1–32 tabs with a selected tab that exists, 1–16 table columns, icons of size 1–256, numbers with finite `min` ≤ `value` ≤ `max`, a step of at least 0, and at most 6 decimals, a tooltip right after a widget the player sees, and every `begin` ended by its own `end`. Windows that break a rule fault the plugin.

A `button` is an action word of a `kind`: `normal`, `main` (the main action of a window, at most one in view), `quiet` (a side action), or `danger` (what cannot be undone). A `fact` is a name and its value on one line.

An `entry` is a row of a list, drawn like the rows of Angelica's own list: a tile with the first letter of its `title`, the title, a `note` after it, a short `value` at the right, and a `mark` — `on` for what is in use or running, `off` for what is not (the row is drawn muted), `attention` for what needs the player. With `progress`, a thin line takes the note's place. Under the pointer the row shows its `actions`, up to 4, in place of the note and the value. A click on the row arrives as `clicked` with the entry's id and a pressed action as `clicked` with the action's id; the plugin decides which entry is `chosen`. The chosen entry is highlighted and shows under its row its `facts`, up to 16, and its `more` actions, up to 8. The ids of an entry and of all its actions are unique within the window like any widget's id, so a plugin gives the facts and `more` actions only to the chosen entry or makes their ids differ per entry.

`runtime.language` returns the language of Angelica's interface, `en` or `ru`, which the player can change while the plugin runs; a plugin shows its own text in that language when it has it and in English otherwise.

`open-requested` arrives when the player presses the plugin's Open button in the Angelica menu; `closed` when the player closes a window.

## Storage

Paths are relative, use `/`, and stay inside the plugin's folder: no empty, `.`, or `..` parts, no `\ : * ? " < > |` or control characters, no Windows device names (`con`, `prn`, `aux`, `nul`, `conin$`, `conout$`, `com` and `lpt` with a digit or a superscript ¹ ² ³, with any extension), no trailing dot or space, at most 512 bytes and 16 parts. Links are not followed. `write` replaces a file atomically; `append` and `write` create parent directories. `read` returns at most 64 MiB; `read-range` reads at most 16 MiB from an offset. A plugin's folder holds at most 4 GiB; a write past it returns `limit-exceeded`.

The player can open the folder from the Angelica menu, for example to give the plugin a file.

## Game files and sheets

`game-files` reads a file from the installed game's archives by its game path (`common/font/AXIS_12.fdt`), case-insensitively, up to 64 MiB; standard files and textures are supported. `excel` lists the game's sheets, describes one (its columns with index, kind, and offset, the columns the game map names, its variant, the languages the installed game has data for, and row count), and returns the values of named columns in rows: up to 1024 per call sorted by row and subrow, or one row by key. `excel.row` and `excel.rows` always return the game's own text, whatever a plugin shows in its place, so a plugin's logic never depends on a translation. `excel.shown` returns one text cell as the game shows it now: the text of the first active [text table](#text-tables) of any plugin, by priority, that has the cell and whose guard matches the game's text, or the game's own text when none has. A plugin asks for it only to show the text to the player — a name of an action in the player's translation — and never to find something by it. A plugin reads values only by the names the game map's [sheet schemas](./game-map-v1.md#sheet-schemas) give, which keep their meaning across patches: a sheet whose layout in the running game the map does not know returns `unavailable` until the map knows it, and a name the map does not give the sheet returns `not-found`. Column positions and offsets serve text tables, which check them; a plugin never finds a value by position. Text cells are the game's encoded strings without terminator; numbers are signed or unsigned by their column kind. A sheet without data in the requested language is read in its language-neutral data; with neither the call returns `not-found`, also for a language the sheet lists that the installed game has no data files for, such as another region's. Languages use the game's numbers (0 none, 1 Japanese, 2 English, 3 German, 4 French, 5 and 6 Chinese, 7 Korean, 8 Traditional Chinese).

Both read the original files: never another plugin's text tables or replaced files, and nothing is written. They need no permission and work in the main instance and in jobs. A file or sheet the host has not read yet is decoded from the archives, which takes milliseconds to a few hundred for the largest sheets; the host keeps the 32 most recently read sheets. Bulk reading belongs in a job.

## Game state

`state` reads the game's state as tables the [game map](./game-map-v1.md#state-tables) defines: a table describes one object of the game, such as the player's character (`self`), as named fields, and `read` returns the values of the fields asked for, in the order asked. A table has one row while its object exists and none otherwise, such as before the player logs in. A list table, such as the statuses on the player's character (`self.statuses`), has one row per used element of a game array, and its field `index` is the element's position in the array. A value is text (the game's bytes without terminator), a boolean, a signed or unsigned integer, or a float, by the field's type; it is none when the game holds nothing there. Numbers that identify game data are rows of the sheet the field's documentation names.

Names keep their meaning across game patches: the map resolves them for each build. A field whose map entries are absent in the running build returns `unavailable` until the map catches up; an unknown table or field returns `not-found`. `tables` lists every table with its fields and whether each is present, so a plugin can check before it reads.

Every table belongs to a domain, and reading it requires the domain's permission: `state.self` for the player's character and where the player is. Reads happen in the main instance, on the game's main thread between frames; jobs get `wrong-context`. Reading never changes the game.

## Events

`events.subscribe` replaces the kinds of events the plugin receives; each kind needs its permission, and an empty list ends the subscriptions. When the game does something of a subscribed kind, the host records it, and in the next frame calls the plugin's `on-game-event` once per event, in the order the game did them, after `tick` and the plugin's commands. An event reports what happened; the plugin cannot change it.

- `chat-message` (`chat.read`): a message the game added to the chat log, whatever the channel, with its kind (a row of the LogKind sheet), the game's full log information, the sender's name and the message as game text of at most 8192 bytes each, and the time in seconds since 1970. Messages printed by Angelica's plugins are not delivered, so a plugin that answers messages never hears itself.

The host keeps at most 256 events between two frames, for all plugins together; when more arrive, every subscribed plugin first receives `dropped` with how many were left out. A subscription ends when the plugin stops. Subscribing to a kind whose map entries the game build lacks returns `unavailable`.

## Game windows

`game-ui` changes the game's own windows through rules the host applies. A rule names a window as the game names it and an element by the name the [game map](./game-map-v1.md#window-schemas) gives it for the window's layout in the running game build. An element the map does not name cannot be changed. `elements(window)` lists the names of a window with whether each shows text, and returns `unavailable` when the map names none or does not know the build's layout of the window.

- `set-hidden` (`gameui.hide`) replaces the elements the plugin keeps hidden, at most 256. The host hides an element again whenever the game shows it.
- `set-texts` (`gameui.text`) replaces the texts the plugin puts in text elements, at most 64, one per element: after the game's text, before it, or instead of it. A text has at most 256 bytes and no control characters. The host writes the text again whenever the game writes its own, and a plugin whose value changes calls `set-texts` again. An element that does not show text is refused.

Both return the elements that are not available in the running game build: those rules wait, and the others apply. Rules are applied on the game's main thread once per frame, after the plugins' calls of the frame, so a change is visible in the next frame the game draws; a window the game has not built is left alone until it is. When several plugins hide one element, it is hidden while any of them does. When several put text in one element, their texts are joined in the order of plugin ids, and the first `instead` replaces the game's text; an element's text with the plugins' texts has at most 4096 bytes, and longer text is left as the game wrote it.

An empty list, and disabling the plugin, remove the rules: in the next frame the host shows the elements it hid and writes back the game's text where its own is still there. An element the host hid is shown again even when the game would have hidden it meanwhile for its own reasons; the game shows or hides it as it wants from its next update. A game build that lacks the [window entries](./game-structures.md#game-windows) returns `unavailable`.

## Text tables

`excel-text.activate(name, path, priority)` reads a [text table](./text-table-v1.md) from the plugin's storage, validates it completely, and activates it under `name` (1–64 ASCII letters, digits, `-`, `_`), replacing the plugin's table of that name. The host keeps active tables in memory: a table file has at most 512 MiB, a plugin keeps at most 8 tables active, and they have at most 1 GiB together; past that `activate` returns `limit-exceeded`. An invalid file, or a game build that lacks the [row entries](./game-structures.md), refuses activation and keeps the previous table.

From then on, whenever the game builds a row of a sheet a table lists, the host replaces the listed cells before anything else in the game reads the row. For each sheet, a table applies only when the sheet's name, variant (rows or subrows), and text columns (index and offset, all of them) equal the table's, and the sheet's language equals the table's language (sheets without a language match any table). For each cell, the table applies only when the guard of the game's original text equals the cell's guard. Otherwise the game's text stays. Rows the game built before activation keep the text they had: a plugin that activates its tables in `start` during the game's start covers everything the game loads.

When several active tables list a cell, they are tried by descending `priority`, then by plugin id and table name; the first whose guard matches applies. `deactivate` and disabling the plugin remove the plugin's tables; rows built afterwards keep the game's text.

`status` returns counters since activation: rows changed, cells replaced, guard mismatches, rows refused because their buffer did not have the expected layout, and the listed sheets the game loaded that did not match the table, with the reason.

## Replacement files

`file-replacements.replace(game-path, storage-path)` reads a file of at most 64 MiB from the plugin's storage, checks it against the game's original file, copies it into the host's cache, and from then on the game reads it whenever it loads `game-path`. Only formats the host can check are accepted:

- font tables (`.fdt`): the original's file and font headers (texture size, font size, line height, ascent) and kerning stay; glyph records stay sorted and lie inside the atlas, on one of the 28 pages of the game's seven font textures;
- textures (`.tex`): the original's 80-byte header and size stay, so only pixels change.

A file the game has no original for, another format, a failed check, and a file another plugin replaces are refused. Files the game loaded before keep what it read until it loads them again; replacing files in `start` while the game starts covers everything it loads. `restore` and disabling the plugin serve the game's own file from its next load. A plugin replaces at most 1024 files.

## Letter case

The game builds capitalized and lower-case text (`<head>`, `<caps>`, `<lower>` and related macros) with `Utf8String::ToUpper` and `ToLower`, which change only the letters the game knows. `text-case.set-letters` adds letter pairs; after the game's own conversion the host applies the same rule — first letter, first letter of every word except the game's excluded words, or every letter — to the added letters, skipping macros. Both letters of a pair are non-ASCII, differ, and have the same UTF-8 length, so text never changes length. A pair that conflicts with another plugin's is refused. A plugin adds at most 4096 pairs; an empty list removes them.

## Commands

`commands.define` replaces the plugin's commands with a tree: root commands such as `/hello`, subcommands such as `/hello greet` (at most 3 words deep), and typed parameters. Words — command names, aliases (at most 8 per command), and parameter names — are 1–32 lower-case ASCII letters, digits, and hyphens; descriptions are one line of at most 256 bytes. A plugin defines at most 64 commands of at most 16 parameters each. Optional parameters follow every required one, and only the last takes the rest of the line (`text`). A choice offers 1–64 distinct words without spaces or quotes. `help` is never a subcommand. A root name or alias must be free: not a command of the game in any of its languages — the host reads them from the game's `TextCommand` sheet, and without that sheet `define` returns `unavailable` — not `angelica`, and not another running plugin's. A tree that breaks a rule is refused whole and the previous one stays.

When the player sends a line from the chat box or a macro whose first word is one of the plugin's roots or root aliases, case-insensitively, the game never sees it. In the next frame the host parses it: words are separated by spaces, and text in double quotes is one word. Subcommands and aliases are matched case-insensitively; then each parameter takes its argument by kind — a `word`, the rest of the line as typed for `text`, a whole number within the bounds for `integer`, a finite decimal number for `number`, one of the words for `choice` (passed as listed), `on` or `off` for `switch`. The plugin's `command` receives the command's path as defined and the arguments in parameter order; an optional parameter left out has no argument.

The host answers some lines itself, as the plugin's message in the chat log: `/<root> help` and `/<root> help <subcommand>` print help built from the descriptions; a line that does not fit (a missing or extra argument, a value of the wrong kind, an unclosed quote) prints what is wrong and the command's usage; a line with links, auto-translate phrases, or more than 1000 bytes is refused. An error returned by `command` is printed the same way. These answers need no `chat.print`. The host keeps at most 32 lines waiting for the next frame; a line past that does not run, and the host answers that it was not run. Disabling the plugin removes its commands; the game then handles those lines again.

## Chat

`chat.print` adds a message to the chat log, in the Echo channel. A message is a list of spans of plain text, each in a tone — `normal`, `accent`, `muted`, `good`, `warning`, or `bad` — and optionally italic; a line break starts a new line, and no other control character is allowed. A message has at most 64 spans and 2000 bytes of text, and is not empty. The host renders the tones with colors of the game's interface theme and starts every message with `[<plugin name>]` in a color no tone uses, so a plugin cannot pass its message off as the game's or another plugin's ([game structures](./game-structures.md#chat)). A plugin prints at most 20 messages in any second; past that `print` returns `limit-exceeded`. Before the game's chat log exists, during the game's start, it returns `unavailable`.

## HTTP

A plugin with `http.get` sends HTTPS GET requests to the hosts its manifest lists in `[http] hosts` ([manifest](./plugin-manifest-v1.md)), which the player approves with the plugin. Requests block, so they are made from jobs; the main instance gets `wrong-context`. Time spent in a request does not count against the job's 10 minutes.

Before anything is sent, the host checks the request: an absolute `https://` URL of at most 4096 bytes, without user name or password, on port 443, to a listed host — otherwise `invalid-argument`, or `permission-denied` for a host the manifest does not list. A plugin sets only the headers `accept`, `accept-language`, `if-none-match`, `if-modified-since`, and `range`, each once, with values of at most 1024 bytes and no control characters. The host sets the user agent (`Angelica/<version> (plugin <id>/<version>)`), asks for gzip and decompresses it, sends no cookies or credentials, and ignores proxies configured on the system.

The same rules hold for every connection, including each of at most 5 redirects: the scheme is `https`, the port 443, and the host a listed one. A host name is resolved and only public addresses are used; a name that resolves only to loopback, private, shared, link-local, multicast, documentation, or reserved addresses is refused with `permission-denied`, so a plugin cannot reach the player's computer or network. TLS is the operating system's, with its certificate store and TLS 1.2 or later; a certificate the system does not trust fails the request.

`get` returns the final response's status, headers (names in lower case), and body, at most 16 MiB, within 60 seconds. `download` writes a 2xx response's body to a file in the plugin's storage, at most 1 GiB and within the storage quota, within 15 minutes; the file is replaced only when the whole body arrived, and another status leaves it as it was. Any status is a response, not an error. A request that does not complete — the name does not resolve, the connection or TLS fails, the time runs out, the job is cancelled — returns `network`; a body over its limit returns `limit-exceeded`. Resolution and connection each take at most 15 seconds, and the response's head arrives within 30.

## WASI

A component built by Rust's standard library imports WASI. The host provides `wasi:random` (`random`, `insecure`, `insecure-seed`; at most 1 MiB per call) and `wasi:clocks` (`monotonic-clock.now` and `resolution`, `wall-clock`), which expose nothing of the player's system and which `HashMap` and `Instant` use. Every other WASI function — files, environment, sockets, standard streams, polling — traps when called. A component that imports anything other than `angelica:plugin/*@1.0.0` and WASI is refused at installation.

## Compatibility

Interfaces and functions of `angelica:plugin@1.0.0` do not change. A later 1.x version adds interfaces or functions in a new package version, and the host keeps running components built for earlier 1.x versions.
