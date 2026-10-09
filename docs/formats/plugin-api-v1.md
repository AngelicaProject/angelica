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

Every plugin has its log, folder, windows, and jobs. An operation that needs a permission the player did not grant returns `permission-denied`.

Every fallible call returns `types.error`: a stable `kind` for programs and a `message` that names the path, table, URL, or value involved. A guest that ignores an error keeps running; a trap disables it.

## Calls into the plugin

The host keeps one **main instance** per running plugin and calls it on the game's main thread:

- `start` once when the plugin is enabled. An error disables the plugin and is shown in the Angelica menu. During the game's own start, before any game code runs, `runtime.started-with-game` is true; a plugin enabled later sees a game that has already loaded data.
- `tick` every frame, then `command` for each of the plugin's [commands](#commands) the player ran since the previous frame, then `ui-event` for each player action, then `ui`.
- `stop` before the plugin is disabled and at game exit.

Each call has a time limit counted in guest time only (time spent in host calls is not counted) and a fuel limit (`host.json`, [installation](./host-installation-v1.md)): by default 20 ms for frame calls and 2 s for `start` and `stop`. Exceeding a limit traps.

A trap, a `start` or `tick` error, or invalid windows **fault** the plugin: the host stops it without calling `stop`, removes everything it activated (text tables, letters, replaced files, commands, jobs, windows), and shows the reason. It stays off until the player retries it. The game never stops because of a plugin.

`run-job` runs in a **job instance**: a fresh instance of the component per job, with its own memory, on a host thread, for at most 10 minutes of guest time and without a fuel limit. A job instance never runs `start`. A job may use `runtime`, `storage`, `game-files`, `excel`, `http`, and `jobs.report`; `excel-text`, `text-case`, `file-replacements`, `commands`, `chat`, `jobs.start`, `jobs.poll`, and `jobs.cancel` return `wrong-context`, and `http` returns it in the main instance. A job's input and output are at most 16 MiB; at most four jobs of a plugin run at once. A trap or error fails only the job. Disabling the plugin cancels its jobs.

Each instance's memory is limited to the manifest's `plugin.memory_bytes`, or the host default of 64 MiB.

## Windows

Every frame the host draws the windows `ui` returns; a window that is not returned is not shown. The host frames each window with the plugin's name and supplies the fonts, colors, and scaling; a plugin never draws.

Widgets are a flat list. `begin-row` … `end-row` lays widgets side by side and `begin-section` … `end-section` puts them under a heading the player can collapse; both nest up to 16 deep. Widgets are controlled: a checkbox, text input, or choice shows the value the plugin passes, and a change by the player arrives as an `event` before the next `ui`. While the player types in a text input, the host keeps the typed text; otherwise the plugin's value is shown.

The host checks the windows before drawing them: at most 16 windows and 4096 widgets, ids of 1–128 bytes unique within a window (window ids unique per plugin), texts of at most 4096 bytes and 512 KiB in total, at most 1024 options per choice, a selected option that exists, and every `begin` ended. Windows that break a rule fault the plugin.

`open-requested` arrives when the player presses the plugin's Open button in the Angelica menu; `closed` when the player closes a window.

## Storage

Paths are relative, use `/`, and stay inside the plugin's folder: no empty, `.`, or `..` parts, no `\ : * ? " < > |` or control characters, no Windows device names (`con`, `nul`, `com1`, …, with any extension), no trailing dot or space, at most 512 bytes and 16 parts. Links are not followed. `write` replaces a file atomically; `append` and `write` create parent directories. `read` returns at most 64 MiB; `read-range` reads at most 16 MiB from an offset. A plugin's folder holds at most 4 GiB; a write past it returns `limit-exceeded`.

The player can open the folder from the Angelica menu, for example to give the plugin a file.

## Game files and sheets

`game-files` reads a file from the installed game's archives by its game path (`common/font/AXIS_12.fdt`), case-insensitively, up to 64 MiB; standard files and textures are supported. `excel` lists the game's sheets, describes one (its columns with index, kind, and offset, its variant, the languages the installed game has data for, and row count), and returns rows: up to 1024 per call sorted by row and subrow, or one row by key. Text cells are the game's encoded strings without terminator; numbers are signed or unsigned by their column kind. A sheet without data in the requested language is read in its language-neutral data; with neither the call returns `not-found`, also for a language the sheet lists that the installed game has no data files for, such as another region's. Languages use the game's numbers (0 none, 1 Japanese, 2 English, 3 German, 4 French, 5 and 6 Chinese, 7 Korean, 8 Traditional Chinese).

Both read the original files: never another plugin's text tables or replaced files, and nothing is written. They need no permission and work in the main instance and in jobs. A file or sheet the host has not read yet is decoded from the archives, which takes milliseconds to a few hundred for the largest sheets; the host keeps the 32 most recently read sheets. Bulk reading belongs in a job.

## Text tables

`excel-text.activate(name, path, priority)` reads a [text table](./text-table-v1.md) from the plugin's storage, validates it completely, and activates it under `name` (1–64 ASCII letters, digits, `-`, `_`), replacing the plugin's table of that name. A plugin keeps at most 8 tables active. An invalid file, or a game build that lacks the [row entries](./game-structures.md), refuses activation and keeps the previous table.

From then on, whenever the game builds a row of a sheet a table lists, the host replaces the listed cells before anything else in the game reads the row. For each sheet, a table applies only when the sheet's name, variant (rows or subrows), and text columns (index and offset, all of them) equal the table's, and the sheet's language equals the table's language (sheets without a language match any table). For each cell, the table applies only when the guard of the game's original text equals the cell's guard. Otherwise the game's text stays. Rows the game built before activation keep the text they had: a plugin that activates its tables in `start` during the game's start covers everything the game loads.

When several active tables list a cell, they are tried by descending `priority`, then by plugin id and table name; the first whose guard matches applies. `deactivate` and disabling the plugin remove the plugin's tables; rows built afterwards keep the game's text.

`status` returns counters since activation: rows changed, cells replaced, guard mismatches, rows refused because their buffer did not have the expected layout, and the listed sheets the game loaded that did not match the table, with the reason.

## Replacement files

`file-replacements.replace(game-path, storage-path)` reads a file from the plugin's storage, checks it against the game's original file, copies it into the host's cache, and from then on the game reads it whenever it loads `game-path`. Only formats the host can check are accepted:

- font tables (`.fdt`): the original's file and font headers (texture size, font size, line height, ascent) and kerning stay; glyph records stay sorted and lie inside the atlas, on one of the 28 pages of the game's seven font textures;
- textures (`.tex`): the original's 80-byte header and size stay, so only pixels change.

A file the game has no original for, another format, a failed check, and a file another plugin replaces are refused. Files the game loaded before keep what it read until it loads them again; replacing files in `start` while the game starts covers everything it loads. `restore` and disabling the plugin serve the game's own file from its next load. A plugin replaces at most 1024 files.

## Letter case

The game builds capitalized and lower-case text (`<head>`, `<caps>`, `<lower>` and related macros) with `Utf8String::ToUpper` and `ToLower`, which change only the letters the game knows. `text-case.set-letters` adds letter pairs; after the game's own conversion the host applies the same rule — first letter, first letter of every word except the game's excluded words, or every letter — to the added letters, skipping macros. Both letters of a pair are non-ASCII, differ, and have the same UTF-8 length, so text never changes length. A pair that conflicts with another plugin's is refused. A plugin adds at most 4096 pairs; an empty list removes them.

## Commands

`commands.define` replaces the plugin's commands with a tree: root commands such as `/hello`, subcommands such as `/hello greet` (at most 3 words deep), and typed parameters. Words — command names, aliases (at most 8 per command), and parameter names — are 1–32 lower-case ASCII letters, digits, and hyphens; descriptions are one line of at most 256 bytes. A plugin defines at most 64 commands of at most 16 parameters each. Optional parameters follow every required one, and only the last takes the rest of the line (`text`). A choice offers 1–64 distinct words without spaces or quotes. `help` is never a subcommand. A root name or alias must be free: not a command of the game in any of its languages — the host reads them from the game's `TextCommand` sheet, and without that sheet `define` returns `unavailable` — not `angelica`, and not another running plugin's. A tree that breaks a rule is refused whole and the previous one stays.

When the player sends a line from the chat box or a macro whose first word is one of the plugin's roots or root aliases, case-insensitively, the game never sees it. In the next frame the host parses it: words are separated by spaces, and text in double quotes is one word. Subcommands and aliases are matched case-insensitively; then each parameter takes its argument by kind — a `word`, the rest of the line as typed for `text`, a whole number within the bounds for `integer`, a finite decimal number for `number`, one of the words for `choice` (passed as listed), `on` or `off` for `switch`. The plugin's `command` receives the command's path as defined and the arguments in parameter order; an optional parameter left out has no argument.

The host answers some lines itself, as the plugin's message in the chat log: `/<root> help` and `/<root> help <subcommand>` print help built from the descriptions; a line that does not fit (a missing or extra argument, a value of the wrong kind, an unclosed quote) prints what is wrong and the command's usage; a line with links, auto-translate phrases, or more than 1000 bytes is refused. An error returned by `command` is printed the same way. These answers need no `chat.print`. The host keeps at most 32 lines waiting for the next frame. Disabling the plugin removes its commands; the game then handles those lines again.

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
