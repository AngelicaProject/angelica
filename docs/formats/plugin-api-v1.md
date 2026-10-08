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
| `excel-text` | replacement text for cells of the game's Excel sheets | `excel.text` |
| `text-case` | letters added to the game's letter case conversion | `text.case` |

Every plugin has its log, folder, windows, and jobs. An operation that needs a permission the player did not grant returns `permission-denied`.

Every fallible call returns `types.error`: a stable `kind` for programs and a `message` that names the path, table, or value involved. A guest that ignores an error keeps running; a trap disables it.

## Calls into the plugin

The host keeps one **main instance** per running plugin and calls it on the game's main thread:

- `start` once when the plugin is enabled. An error disables the plugin and is shown in the Angelica menu. During the game's own start, before any game code runs, `runtime.started-with-game` is true; a plugin enabled later sees a game that has already loaded data.
- `tick` every frame, then `ui-event` for each player action since the previous frame, then `ui`.
- `stop` before the plugin is disabled and at game exit.

Each call has a time limit counted in guest time only (time spent in host calls is not counted) and a fuel limit (`host.json`, [installation](./host-installation-v1.md)): by default 20 ms for frame calls and 2 s for `start` and `stop`. Exceeding a limit traps.

A trap, a `start` or `tick` error, or invalid windows **fault** the plugin: the host stops it without calling `stop`, removes everything it activated (text tables, letters, jobs, windows), and shows the reason. It stays off until the player retries it. The game never stops because of a plugin.

`run-job` runs in a **job instance**: a fresh instance of the component per job, with its own memory, on a host thread, for at most 10 minutes of guest time and without a fuel limit. A job instance never runs `start`. A job may use `runtime`, `storage`, and `jobs.report`; `excel-text`, `text-case`, `jobs.start`, `jobs.poll`, and `jobs.cancel` return `wrong-context`. A job's input and output are at most 16 MiB; at most four jobs of a plugin run at once. A trap or error fails only the job. Disabling the plugin cancels its jobs.

Each instance's memory is limited to the manifest's `plugin.memory_bytes`, or the host default of 64 MiB.

## Windows

Every frame the host draws the windows `ui` returns; a window that is not returned is not shown. The host frames each window with the plugin's name and supplies the fonts, colors, and scaling; a plugin never draws.

Widgets are a flat list. `begin-row` … `end-row` lays widgets side by side and `begin-section` … `end-section` puts them under a heading the player can collapse; both nest up to 16 deep. Widgets are controlled: a checkbox, text input, or choice shows the value the plugin passes, and a change by the player arrives as an `event` before the next `ui`. While the player types in a text input, the host keeps the typed text; otherwise the plugin's value is shown.

The host checks the windows before drawing them: at most 16 windows and 4096 widgets, ids of 1–128 bytes unique within a window (window ids unique per plugin), texts of at most 4096 bytes and 512 KiB in total, at most 1024 options per choice, a selected option that exists, and every `begin` ended. Windows that break a rule fault the plugin.

`open-requested` arrives when the player presses the plugin's Open button in the Angelica menu; `closed` when the player closes a window.

## Storage

Paths are relative, use `/`, and stay inside the plugin's folder: no empty, `.`, or `..` parts, no `\ : * ? " < > |` or control characters, no Windows device names (`con`, `nul`, `com1`, …, with any extension), no trailing dot or space, at most 512 bytes and 16 parts. Links are not followed. `write` replaces a file atomically; `append` and `write` create parent directories. `read` returns at most 64 MiB; `read-range` reads at most 16 MiB from an offset. A plugin's folder holds at most 4 GiB; a write past it returns `limit-exceeded`.

The player can open the folder from the Angelica menu, for example to give the plugin a file.

## Text tables

`excel-text.activate(name, path, priority)` reads a [text table](./text-table-v1.md) from the plugin's storage, validates it completely, and activates it under `name` (1–64 ASCII letters, digits, `-`, `_`), replacing the plugin's table of that name. A plugin keeps at most 8 tables active. An invalid file, or a game build that lacks the [row entries](./game-structures.md), refuses activation and keeps the previous table.

From then on, whenever the game builds a row of a sheet a table lists, the host replaces the listed cells before anything else in the game reads the row. For each sheet, a table applies only when the sheet's name, variant (rows or subrows), and text columns (index and offset, all of them) equal the table's, and the sheet's language equals the table's language (sheets without a language match any table). For each cell, the table applies only when the guard of the game's original text equals the cell's guard. Otherwise the game's text stays. Rows the game built before activation keep the text they had: a plugin that activates its tables in `start` during the game's start covers everything the game loads.

When several active tables list a cell, they are tried by descending `priority`, then by plugin id and table name; the first whose guard matches applies. `deactivate` and disabling the plugin remove the plugin's tables; rows built afterwards keep the game's text.

`status` returns counters since activation: rows changed, cells replaced, guard mismatches, rows refused because their buffer did not have the expected layout, and the listed sheets the game loaded that did not match the table, with the reason.

## Letter case

The game builds capitalized and lower-case text (`<head>`, `<caps>`, `<lower>` and related macros) with `Utf8String::ToUpper` and `ToLower`, which change only the letters the game knows. `text-case.set-letters` adds letter pairs; after the game's own conversion the host applies the same rule — first letter, first letter of every word except the game's excluded words, or every letter — to the added letters, skipping macros. Both letters of a pair are non-ASCII, differ, and have the same UTF-8 length, so text never changes length. A pair that conflicts with another plugin's is refused. A plugin adds at most 4096 pairs; an empty list removes them.

## WASI

A component built by Rust's standard library imports WASI. The host provides `wasi:random` (`random`, `insecure`, `insecure-seed`; at most 1 MiB per call) and `wasi:clocks` (`monotonic-clock.now` and `resolution`, `wall-clock`), which expose nothing of the player's system and which `HashMap` and `Instant` use. Every other WASI function — files, environment, sockets, standard streams, polling — traps when called. A component that imports anything other than `angelica:plugin/*@1.0.0` and WASI is refused at installation.

## Compatibility

Interfaces and functions of `angelica:plugin@1.0.0` do not change. A later 1.x version adds interfaces or functions in a new package version, and the host keeps running components built for earlier 1.x versions.
