# Plugin API 1.0

The plugin API is the WIT package `angelica:plugin@1.0.0` in [`wit/`](../../wit). A plugin is a WebAssembly component of its `plugin` world: it imports the host's interfaces and exports `guest`; one that [intercepts](#intercepts) exports `handler` too, as the SDK's `intercepting` world does. The WIT files carry the doc comment of every function; this document states the rules they share. [`angelica-sdk`](../../crates/angelica-sdk) provides Rust bindings, a `Plugin` trait, and helpers for every interface.

A manifest declares the API version it was built for in `plugin.api` ([manifest](./plugin-manifest-v1.md)). This host runs `1.0`.

## Interfaces

| Interface | Provides | Permission |
| --- | --- | --- |
| `runtime` | the plugin's log, whether it started with the game, the game version | — |
| `storage` | the plugin's private folder | — |
| `ui` | places and parts the host draws (types used by `guest`) | — |
| `jobs` | long work in a separate instance on a host thread | — |
| `game-files` | files from the installed game's archives | — |
| `excel` | the game's Excel sheets as its files hold them | — |
| `file-replacements` | replacement game files the game reads instead of its own | `files.replace` |
| `commands` | chat commands the host parses for the plugin | `chat.commands` |
| `chat` | messages in the player's chat log, with links to items | `chat.print` |
| `http` | HTTPS requests to the hosts the manifest lists: GET, and data sent | `http.get`, `http.send` |
| `websocket` | live connections to the hosts the manifest lists, their messages delivered as game events | `http.websocket`, with `http.get` |
| `lens` | the game's objects read by path, events watched, rules at decision points, values changed, and actions taken, through the game map's object model | one per domain and verb, in `[lens] read`, `change`, and `act` |
| `events` | what reaches `on-game-event`: firings of Lens watches, messages from other plugins, and what happened on the plugin's connections | — |
| `draw` | marks, lines, and text drawn over the game world | `draw.world` |
| `messages` | messages to other plugins, delivered as game events | `messages.send`, with the plugins in `[messages] to` |
| `clipboard` | text put on the player's clipboard for the player's action | `clipboard.write` |
| `links` | pages of the sites the manifest lists opened in the player's browser for the player's action | `links.open` |
| `notifications` | one line at the top right of the screen under the plugin's name | `notifications` |
| `sound` | sounds from WAV files in the plugin's storage | `sound.play` |
| `intercepts` | handlers in the game's calls of functions the plugin's own Anatomia entries describe | `unreviewed` |

Every plugin has its log, folder, windows, and jobs. The host keeps the last 200 lines of the log; they go into the report the player copies from the Angelica menu for the plugin's author, so a plugin logs what it did and why it failed in words its author can act on. An operation that needs a permission the player did not grant returns `permission-denied`.

Every fallible call returns `types.error`: a stable `kind` for programs and a `message` that names the path, table, URL, or value involved. A guest that ignores an error keeps running; a trap disables it.

## Calls into the plugin

The host keeps one **main instance** per running plugin and calls it on the game's main thread:

- `start` once when the plugin is enabled. An error disables the plugin and is shown in the Angelica menu. During the game's own start, before any game code runs, `runtime.started-with-game` is true; a plugin enabled later sees a game that has already loaded data.
- `tick` every frame, then `command` for each of the plugin's [commands](#commands) the player ran since the previous frame, then `on-game-event` for each [event](#events) that reached the plugin, then `ui-event` for each player action, then `ui`.
- `stop` before the plugin is disabled and at game exit.

The calls of one frame — `tick`, the commands, the events, the window events, and `ui` — share one budget, and `start` and `stop` each have their own: a guest time limit, which does not count time spent in host calls, and a fuel limit (`host.json`, [installation](./host-installation-v1.md)), by default 20 ms for a frame's calls and 2 s for `start` and `stop`; and a limit on time including host calls, 2 s for a frame's calls and 30 s for `start` and `stop`, so a plugin cannot hold the game's main thread with host operations. Exceeding a limit traps. A frame in which a plugin's calls take more than 100 ms is noted in the Angelica menu.

A trap, a `start` or `tick` error, or places that break the [rules](#rules-for-every-place-and-part) **fault** the plugin: the host stops it without calling `stop`, removes everything it activated (rules, replaced files, commands, watches, jobs, windows), and shows the reason. It stays off until the player retries it. The game never stops because of a plugin.

`run-job` runs in a **job instance**: a fresh instance of the component per job, with its own memory, on a host thread, for at most 10 minutes of guest time and without a fuel limit. A job instance never runs `start`. A job may use `runtime`, `storage`, `game-files`, `excel`, `http`, and `jobs.report`; `file-replacements`, `commands`, `chat`, `lens`, `events`, `jobs.start`, `jobs.poll`, and `jobs.cancel` return `wrong-context`, and `http` returns it in the main instance. A job's input and output are at most 16 MiB; a plugin has at most four jobs at once, counting finished jobs whose result `poll` has not returned yet and cancelled jobs until they stop. A trap or error fails only the job. Disabling the plugin cancels its jobs.

`handler.intercepted` runs in **intercept instances**; see [intercepts](#intercepts).

Each instance's memory is limited to the manifest's `plugin.memory_bytes`, or the host default of 64 MiB.

## Places and parts

A plugin shows what it has in **places** — windows, panels, and popups — built from **parts** ([interface](../architecture/ui.md#places-and-parts)). Both are data: a `kind` and named `properties` ([`ui.wit`](../../wit/ui.wit)). The kinds and their properties are listed below, each with the API version that added it; later versions add kinds and properties here without changing the WIT types, and none ever changes its meaning or goes away. The host draws every part in Angelica's look and moves what changes; a plugin never draws.

Every frame `ui` returns the plugin's windows and panels, and a window's dialog right after it; one that is not returned is not shown. `notifications.pop-up` shows a popup ([notifications](#notifications)). Place ids are unique per plugin, and part ids within a place.

### Rules for every place and part

The host checks the places before showing them, and a place that breaks a rule faults the plugin (a popup is refused with `invalid-argument` instead):

- every `kind` and property is one listed here, added in an API version no newer than the manifest's `plugin.api`, and its value has the listed shape;
- a property marked *required* is present, a property appears at most once, and an absent one takes its default;
- ids are 1–128 bytes where a part needs one and empty where it does not; texts are at most 4096 bytes and 512 KiB in all; numbers are finite;
- at most 16 windows, 8 panels, and 4096 parts in all; containers nest at most 16 deep and every container is closed by its `end`;
- the limits and ranges given with each property below.

Shapes: *text*, *number* (`f64`), *integer* (`s64`), *flag* (`bool`), *texts*, and *numbers* (lists). A *tone* is a text, one of `normal`, `muted`, `good`, `warning`, `bad`. A *role* is a text, one of `body`, `title`, `value` (figures of one width), `big`, `label` (the secondary color), `small`.

### Places

| Kind | Since | Properties |
|---|---|---|
| `window` | 1.0 | `title` text, *required*: the name in the title bar, usually the plugin's; `path` texts, at most 4 non-blank: where the player is below the top level, outermost first; `status` text: one short line in the title bar; `status-tone` tone, `good` by default |
| `panel` | 1.0 | `name` text, 1–64 characters, *required*: its name in the layout mode; `backing` text, `plate` (default) or `bare`; `anchor` text, where it starts before the player moves it: `top-left`, `top`, `top-right`, `left`, `center`, `right`, `bottom-left` (default), `bottom`, `bottom-right`; `shown` text, `always` (default), `in-combat`, or `out-of-combat` |
| `dialog` | 1.0 | `title` text, *required*: the question, such as `Trust this publisher?`; `window` text, *required*: the id of the window that asks it, returned by the same `ui` |
| `menu-entry` | 1.0 | `label` text, 1–40 characters without control characters, *required*: the entry's words in the game's menu; `for` text, `item` (default): the menu of an item the player carries. Holds no parts |
| `popup` | 1.0 | `at` text, `corner` (default), `center`, or `panel`; `panel` text: the id of the plugin's panel it appears beside, required with `at = panel`; `seconds` integer, 2–10, 5 by default; `attention` flag, as for `notifications.show` |

A window's status tone chooses its mark: `good` the mark of something running, `warning` and `bad` the mark of something that needs the player. When the player presses the back arrow, the title, or an earlier part of the path, the plugin receives `navigated` with the number of parts that stay (0 for the top level) and returns the window with that path; the host does not change the path itself. The title bar's other controls — collapse, the window's menu, and close — are the host's. `closed` arrives when the player closes a window.

A dialog is a question its window asks: the window waits under it, dimmed and taking no input, until the plugin stops returning the dialog. A window has at most one dialog, and its only parts that take input are buttons; its clicks name the dialog's id as their place, and when the player closes it without an answer the plugin receives `closed` with the dialog's id. Return the dialog every frame while the question stands, and stop once the player answered or closed it.

A menu entry is the plugin's line in one of the game's own menus: with `for = item`, the menu the game opens for an item the player carries, such as in the inventory or the armoury chest. The game draws the menu; the host adds each entry after the game's own, in the color of plugin names, so the player tells it from the game's entries. While the plugin returns the entry, every such menu shows it; when the player chooses it, the game closes the menu as for its own entries, and the plugin receives `menu-chosen` with the entry's id as its place and the item's row of `Item`, without its quality. A plugin returns at most 4 menu entries, and a menu shows at most 8 entries of plugins, by plugin id, and no more than the game's 32 entries in all. A game build whose map lacks the menu's entries shows none, and the host notes it under **For developers**.

A panel's clicks pass to the game except on parts that take input. The player can choose otherwise than `shown`, or hide the panel, in the layout mode. A popup's parts take no input: only text, layout, and value parts (below) are allowed in it, at most 32.

### Parts

**Layout.** A container holds the parts after it up to its `end`.

| Kind | Since | Properties |
|---|---|---|
| `row` | 1.0 | side by side |
| `column` | 1.0 | one under another |
| `grid` | 1.0 | `columns` integer, 2–8, *required*: equal columns, filled left to right, row by row |
| `section` | 1.0 | id; `title` text, *required*; `open` flag, true by default: under a heading the player can collapse |
| `table` | 1.0 | `columns` texts, 1–16, *required*: column headings; each cell is one part, or one `row`, left to right, row by row; holds no sections or tables |
| `end` | 1.0 | closes the innermost container |
| `spacer` | 1.0 | takes the free room of its row or column, pushing what follows to its end |
| `separator` | 1.0 | a thin line |
| `tabs` | 1.0 | id; `labels` texts, 1–32, *required*; `selected` integer, an index of `labels`; the plugin shows the selected tab's parts after it; sends `chosen` |
| `split` | 1.0 | id; `labels` texts, 1–64, *required*: sections in a column at the side; `values` texts, at most one per section: a short value at the right of each, empty for none; `groups` texts, at most one per section: a small heading above each, empty for none; `selected` integer, an index of `labels`; `width` integer, 96–320, 160 by default: the column's width in points; `action` text: an action word under the sections, such as `+ Group`. A container: the selected section's parts beside the column. Sends `chosen` with a section, and `clicked` with its id for the action |

A split is a column of sections at the side of a window, like the one in Angelica's Settings, with the selected section's parts beside it. It is one of the window's own parts, outside any container, at most one, and the window's last part; a panel, a dialog, or a popup holds none. It takes the rest of the window: while the window's height follows its content, it is as tall as its taller side; once the player sets the window's size, each side scrolls on its own. As with tabs, the host draws the sections and the plugin shows the selected section's parts.

**Text.**

| Kind | Since | Properties |
|---|---|---|
| `text` | 1.0 | `text` text, *required*; `role` role, `body` by default; `tone` tone, `normal` by default |
| `heading` | 1.0 | `text` text, *required*: a small header with a thin rule |
| `fact` | 1.0 | `name` text and `value` text, *required*: a name and its value on one line |
| `tooltip` | 1.0 | `text` text, *required*: shown while the player points at the part right before it |
| `tile` | 1.0 | `text` text, *required*: a square tile with its first letter |
| `icon` | 1.0 | `icon` integer, *required*: one of the game's icons by the number game sheets hold (such as `Item`'s icon column); `size` integer, 1–256, 24 by default |

The host reads an icon from the game's archives, the high-resolution texture first; an icon the game does not have, or one not read yet, shows as an empty frame. It spends at most 2 ms of a frame reading icons and keeps up to 512 of them.

**Values.** The host moves meters and rings to a new value instead of jumping, shows a readout's change for a moment, and counts countdowns down itself.

| Kind | Since | Properties |
|---|---|---|
| `meter` | 1.0 | `value` number and `max` number above 0, *required*; `label` text; `text` text: the value in words, such as `35 250 / 52 000`; `segments` integer, 0 (a bar, the default) or 2–32; `tone` tone |
| `ring` | 1.0 | `value` number and `max` number above 0, *required*; `size` integer, 16–128, 40 by default; `icon` integer: a game icon inside; `text` text, at most 8 characters, inside when there is no icon; `tone` tone |
| `readout` | 1.0 | id; `value` number, *required*; `decimals` integer, 0–6; `unit` text, such as `%`; `role` role, `value` by default; `tone` tone. When the value differs from the previous frame's, the host shows the change beside it |
| `countdown` | 1.0 | `until` integer, *required*: Unix time in milliseconds, as `wasi:clocks/wall-clock` gives it; `done` text, shown in the warning tone once it runs out, `0:00` by default; `role` role, `value` by default |
| `sparkline` | 1.0 | `values` numbers, 2–256, *required*, oldest first, drawn scaled between the smallest and the largest; `height` integer, 16–128, 36 by default |

**Actions and input.** Parts that take input have an id, show the value the plugin passes, and send what the player did as an event before the next `ui`. While the player types in a text field, the host keeps the typed text; otherwise the plugin's value is shown.

| Kind | Since | Properties |
|---|---|---|
| `button` | 1.0 | id; `label` text, *required*; `style` text: `normal` (default), `main` (the main action of a window, at most one in view, bold with an arrow), `quiet` (a side action), `danger` (what cannot be undone); `enabled` flag, true by default; sends `clicked` |
| `checkbox` | 1.0 | id; `label` text, *required*; `checked` flag; `words` texts, 2, each 1–32 characters: the words for checked and not checked, in place of **On** and **Off**, such as `Translated` and `Original`; `hint` text: a few words under it; sends `toggled` with a flag |
| `text-field` | 1.0 | id; `label` text; `value` text; `hint` text; sends `edited` with a text |
| `choice` | 1.0 | id; `label` text; `options` texts, at most 1024, *required*; `selected` integer, an index of `options`, none by default; `hint` text: a few words under it; sends `chosen` with an integer |
| `number` | 1.0 | id; `label` text; `value`, `min`, and `max` numbers with `min` ≤ `value` ≤ `max`, *required*; `step` number, at least 0 (0, the default, for any): the value set is a multiple of it from `min`; `decimals` integer, 0–6; `slider` flag: a slider instead of a field the player drags or types into; sends `set` with a number |
| `entry` | 1.0 | id; `title` text, *required*; `note` text; `value` text; `mark` text, `none` (default), `on`, `off`, or `attention`; `chosen` flag; `progress` number, 0–1: a thin line in place of the note; `icon` integer: one of the game's icons, as for `icon`, in place of the letter tile; `plain` flag: no tile, and the note under the title. A container: a `button` in it with `in-row` flag true is one of up to 4 actions always shown at the right of the row, before its value; under a chosen entry, its `fact`s (up to 16) and other `button`s (up to 8) are shown under the row. Sends `clicked` |

A checkbox or choice among parts under each other is a setting: its label on the left, its value at the right edge of its place, its hint under them in small muted text, and a thin line between it and a setting right above it. In a row it is its label followed by its value.

An entry is drawn like the rows of Angelica's own list: a tile with the first letter of its title, or the game's icon it names, the title, the note after it, the value at the right, and the mark — `on` for what is in use or running, `off` for what is not (the row is drawn muted), `attention` for what needs the player. A `plain` entry has no tile; with a note or progress it is taller, the note under the title, for rows that a letter or an icon would not tell apart, such as languages. A click on the row sends `clicked` with the entry's id, and a pressed button `clicked` with its own; the plugin decides which entry is chosen. A `button` takes `in-row` only inside an entry.

### Events

`ui-event` receives what the player did as an `event`: its `kind`, the id of the place and of the part, and a value for kinds that carry one. Kinds: `clicked`, `toggled` (a flag), `edited` (a text), `chosen` (an integer: an option, a tab, or a split's section), `set` (a number), `navigated` (an integer), `closed` (a window or a dialog, with an empty part), `open-requested` (the player pressed the plugin's Open button in the Angelica menu; empty place and part), and `menu-chosen` (the player chose a menu entry: the entry's id as the place, an empty part, and an integer, for an item menu the item's row of `Item`). Later API versions may add kinds of events; a plugin ignores kinds it does not know.

`runtime.language` returns the language of Angelica's interface, `en` or `ru`, which the player can change while the plugin runs; a plugin shows its own text in that language when it has it and in English otherwise.

## Storage

Paths are relative, use `/`, and stay inside the plugin's folder: no empty, `.`, or `..` parts, no `\ : * ? " < > |` or control characters, no Windows device names (`con`, `prn`, `aux`, `nul`, `conin$`, `conout$`, `com` and `lpt` with a digit or a superscript ¹ ² ³, with any extension), no trailing dot or space, at most 512 bytes and 16 parts. Links are not followed. `write` replaces a file atomically; `append` and `write` create parent directories. `read` returns at most 64 MiB; `read-range` reads at most 16 MiB from an offset. A plugin's folder holds at most 4 GiB; a write past it returns `limit-exceeded`.

Paths under `cache/` lead to the plugin's cache, for files it can fetch again, such as other players' files a synchronizing plugin downloads. The manifest names its size in `[storage] cache`, and the player approves it with `storage.cache`; without the grant these paths return `permission-denied`. The cache is a folder of its own outside the plugin's folder, with that size as its quota, so it never takes room from the plugin's data; files move inside it with `rename` but not between it and the rest of the folder, `invalid-argument` otherwise. Listing the root shows it as the directory `cache`. Angelica removes the cache with the plugin, whether or not the player keeps the plugin's data.

The player can open the folder from the Angelica menu, for example to give the plugin a file.

## Game files and sheets

`game-files` reads a file from the installed game's archives by its game path (`common/font/AXIS_12.fdt`), case-insensitively, up to 64 MiB; standard files and textures are supported. `excel` lists the game's sheets, describes one (its columns with index, kind, and offset, the columns the game map names, its variant, the languages the installed game has data for, and row count), and returns the values of named columns in rows: up to 1024 per call sorted by row and subrow, or one row by key. `excel.row` and `excel.rows` always return the game's own text, whatever a plugin shows in its place, so a plugin's logic never depends on a translation. `excel.shown` returns one text cell as the game shows it now: the text the first rule of any plugin at `sheet_text` that holds for the cell puts there ([Lens](#lens)), or the game's own text when none does. A plugin asks for it only to show the text to the player — a name of an action in the player's translation — and never to find something by it. A plugin reads values only by the names the game map's [sheet schemas](./game-map-v2.md#sheet-schemas) give, which keep their meaning across patches: a sheet whose layout in the running game the map does not know returns `unavailable` until the map knows it, and a name the map does not give the sheet returns `not-found`. Column positions and offsets serve text tables, which check them; a plugin never finds a value by position. Text cells are the game's encoded strings without terminator; numbers are signed or unsigned by their column kind. A sheet without data in the requested language is read in its language-neutral data; with neither the call returns `not-found`, also for a language the sheet lists that the installed game has no data files for, such as another region's. Languages use the game's numbers (0 none, 1 Japanese, 2 English, 3 German, 4 French, 5 and 6 Chinese, 7 Korean, 8 Traditional Chinese).

Both read the original files: never another plugin's rules or replaced files, and nothing is written. They need no permission and work in the main instance and in jobs. A file or sheet the host has not read yet is decoded from the archives, which takes milliseconds to a few hundred for the largest sheets; the host keeps the 32 most recently read sheets. Bulk reading belongs in a job.

## Lens

`lens` reads the game's objects as the [game map's object model](./game-map-v2.md#object-model) describes them. A query is a path from a root through fields, with filters on arrays, and the values to answer with:

```text
inventory.containers[kind in (bag_1, bag_2, bag_3, bag_4)].items[symbolic = false] { item, quantity, flags }
```

- The path starts at a root, then names fields with `.`: references, embedded objects, and arrays. `[<conditions>]` right after the root skips its object when a condition fails.
- An array on the path gives a row per element. `[n]` takes the element at position `n`. `[<conditions>]` takes the elements that meet every condition, joined by `and`; a condition on an embedded object or a reference skips the object when it fails.
- A condition compares a value of the element, a field name or names through references and embedded objects joined by `.`, with `=`, `!=`, `<`, `<=`, `>`, `>=`, or `in (…)`. A value is a number (decimal, `0x` hexadecimal, or with a fraction), `true` or `false`, the name of one of the field's enum values, or text in double quotes with `\"` and `\\` as escapes. Text compares with `=`, `!=`, and `in`; an enum value or flags with `=`, `!=`, and `in`.
- `{ … }` names the values to answer with, separated by commas: a field, a path through references and embedded objects, or `alias: path`. A column is named by its alias, or by the path's names joined by `_` (`data.level` is `data_level`). A path that ends in a value, without braces, answers with that value alone.

A query is at most 4096 bytes and goes through at most 16 fields, its conditions and answers included.

`prepare` checks a query against the object model of the running build and plans its reads; its error names what is wrong: the syntax or the model's rules (`invalid-argument`), an unknown root or field (`not-found`), a domain the plugin may not read or a field no plugin reads (`permission-denied`), anything the running build lacks (`unavailable`). `run` reads the game's memory along the plan and returns the rows: none while the root's object does not exist, such as the player's character before log-in. A row holds the position in each array on the path and a value per column, in the order of the braces. A value is a boolean, a signed or unsigned integer, a float, text (the game's bytes without terminator), a row of a sheet, the name of an enum's value (a number without a name stays a number), or a set of flags with the names of its named bits; it is none where a pointer on its way is zero or the memory cannot be read. A run answers at most 4096 rows and reads at most 4096 pages of 4 KiB of the game's memory; more returns `limit-exceeded`. All values of one run are read in one moment between two frames.

A plugin holds at most 256 prepared queries; `release` forgets one, and the SDK releases a query it drops. When the game map is reloaded, a held query is planned again at its next run and returns `prepare`'s errors when it no longer fits.

Every field belongs to a domain, and a query may read only the domains the manifest lists in `[lens] read` and the player approved: every field on the path, in the conditions, and in the answers. An array whose elements belong to different domains by the value of a field, such as item containers by kind, is read only in the elements of the permitted domains; a condition on that field with `=` or `in` names the elements the query reads, each of which must be permitted, and an element whose value has no name is never read. Fields the map marks forbidden are never read. Reads happen in the main instance, on the game's main thread between frames; jobs get `wrong-context`. Reading never changes the game.

`watch` starts a watch: a query that starts at an [event](./game-map-v2.md#events) of the map instead of a root, such as `action_used[kind = action] { action, target }`.

- Conditions in brackets after the event's name test its arguments; a firing that fails them is not delivered.
- Without a path, the braces name the event's arguments. A path goes on only through an argument that is a reference, and then reads the object it points to as a query reads from a root: `<event>.<argument>.<field> { … }`.
- The event's arguments and everything read through them count under their domains, as for `prepare`; the errors are `prepare`'s, and `unavailable` also when the host cannot hook the event's point in the running build.

When the game reaches the event's point, the host reads the rows at once, on the thread that reached it, and in the next frame delivers each firing as a `watched` [game event](#events) with the handle `watch` returned. A watch keeps at most the event's `rate` of firings between two frames; a firing past that, past 256 rows, or past 64 pages of 4 KiB is left out, and the plugin receives `watch-dropped` with how many before the firings it kept. `release` ends a watch; a watch ends when the plugin stops. After the game map is reloaded, a watch is planned again, and one that no longer fits receives nothing until the plugin watches again. Watches count toward the 256 held queries.

`act` takes an [action](./game-map-v2.md#actions) of the map for the player now: `act("use_action", [("kind", named("general_action")), ("action", unsigned(4))])` uses Sprint as its hotbar slot does.

- Each parameter is given by name, at most once; one left out takes the map's default, and one without a default must be given. An enum parameter takes the value's name or its number, flags their names or bits, a sheet row a `sheet` value of that sheet or a number, a float a number, a `bool` a boolean. A value outside the argument's kind or the map's range is `invalid-argument`.
- The action's domain must be listed in the manifest's `[lens] act` and approved by the player; otherwise `permission-denied`. While the player has stopped plugins from acting, every action is `permission-denied`.
- A plugin takes one action at most its `rate` of times in any second, refusals included; more is `limit-exceeded`.
- Before the call, every condition of the action must hold; one that does not is `unavailable` with the condition's words, such as "you are in combat". The host then calls the game's function on the main thread, as the game's own interface does, and the game checks the action as for the player's own press. A return value other than the map's `ok` is `unavailable`: the game refused it now. Anything the running build lacks is `unavailable` too.
- `act` works in the main instance after the game has started, from `tick`, a command, a window event, or a game event; jobs get `wrong-context`.

`decide` adds a rule at a [decision point](./game-map-v2.md#events): `decide("chat_message_added[kind = 57]", "hide")` hides the game's system messages.

- The rule is a query from an event with outcomes, without braces: conditions in brackets after the event's name and paths through its arguments, as for `watch`. It holds when it reaches anything.
- Whenever the game calls the event's function and the rule holds, the host takes the outcome itself: the function returns at once with the outcome's value and does not run. No plugin code runs at that moment, so a rule costs the game only its reads. Rules apply in the order they were added, across plugins; the first that holds decides, and a rule that cannot be read holds not.
- The event's domain must be listed in the manifest's `[lens] change` and approved; the domains on the rule's paths in `[lens] read` or `[lens] change`.
- `decide` returns a handle; `release` ends the rule, and every rule ends when the plugin stops. Rules count toward the 256 held queries. After the game map is reloaded, a rule is planned again, and one that no longer fits holds never until the plugin decides again.
- A call a rule decides runs no further: watches and other tools' hooks after Angelica's at that function do not see it.

At an event the host [derives](./game-map-v2.md#events), such as each text cell of a sheet row the game builds or each letter its case conversion meets, a rule is equality conditions on the event's arguments joined by `and`, and the outcome takes `value`, text of the game: `decide("sheet_text[sheet = \"Item\" and row = 4 and column = 9]", "replace", text(…))`.

- The host looks such rules up by their values, so a rule costs nothing where its values differ. `>`, `in`, paths, and a condition on the same argument twice are `invalid-argument`; a value outside an argument's kind never holds and is `invalid-argument`.
- The text is UTF-8 and the game's macros, well-formed by their structure, without NUL and not beginning with `_rsv_`, within the outcome's length; an outcome with `same_length` takes text as long as the rule's value of that argument, which the rule compares.
- `decide-file(event, path)` adds the rules of a file in the plugin's storage at an event that takes rule files, in the format the map names for it, such as a [text table](./text-table-v1.md) for `sheet_text`, as one rule added now. The host reads the file whole and checks it; a file has at most 512 MiB, and a plugin keeps at most 8 files and 1 GiB together, past which it is `limit-exceeded`. It returns the handle, how many parts of the game (sheets) and how many rules (cells) the file has.
- `decisions(handle)` says what such a rule or file did: how often its outcome was taken, how often a rule of a file held for its place but not for the game's text there, which changed since the file was made (`stale`), how often the host could not take the outcome, such as a row whose layout it did not expect (`refused`), and the parts of the game the file does not fit, with why.
- A rule that takes effect when the game builds something applies to what it builds afterwards: rows the game built before keep their text, so a plugin that decides in `start` during the game's start covers everything the game loads.

`set` changes a value the map marks [changeable](./game-map-v2.md#types): `set("camera.camera.max_distance", float(40))`.

- The path is a query's path without braces, ending at the field; filters on the way pick the objects. The value is changed in every object the path reaches, at most 256, and `set` returns how many: none while it reaches no object.
- The value takes the forms an action's parameters take, within the map's range; outside it is `invalid-argument`. A text field takes `text`: UTF-8 shorter than the map's length (longer is `limit-exceeded`), without control characters, so it never carries the game's macros; it replaces the game's whole text.
- The field's domain must be listed in the manifest's `[lens] change` and approved; the domains on the way in `[lens] read` or `[lens] change`. A field the map does not mark is `permission-denied`.
- A value another plugin changed is `unavailable` until that plugin restores it or stops; so is memory that cannot be written now. A plugin keeps at most 64 paths changed.
- A field the map changes through a function of the game is handed to that function, which changes it as the game itself does, such as showing an element of a window; a setting function the build lacks is `unavailable`.
- A field the map marks held is one the game sets again itself, such as what its windows show. The path stays the plugin's: every frame the host applies the value again wherever the path reaches, also in objects the game makes later, and takes what the game wrote meanwhile as the original to put back.
- The host keeps each value's original and puts it back on `restore` with the same path and when the plugin stops, only where the path still reaches the same place and the value is still the one written: a value the game changed meanwhile, or an object the game made again, is left as the game has it.

## Events

The `events` interface has no functions: it names what the host delivers to the plugin's `on-game-event`, which it calls in the next frame once per event, after `tick` and the plugin's commands. An event reports what happened; the plugin cannot change it. Nothing needs a subscription; what the game does reaches a plugin only through its [Lens watches](#lens).

- `watched` and `watch-dropped`: the firings of a [Lens watch](#lens), in the order they happened.
- `message` and `messages-dropped`: [messages](#messages-between-plugins) from other plugins, after the watches' firings.
- `socket-opened`, `socket-message`, and `socket-closed`: what happened on the plugin's [connections](#websocket), after the messages.

Messages added to the chat log are watched like any event: `chat_message_added { kind, sender.text, message.text }` under the `chat` domain. Watches record nothing while the host prints a plugin's message, so a plugin that answers messages never hears itself.

## Game windows

The game's own windows are objects of the map's `windows` domain: a query reads them and `set` changes what the map marks changeable, as for any [Lens](#lens) value.

```text
windows.unit_manager.windows[name = "_Exp"].layout.elements[name = window].visible   set to boolean(false): the experience bar is hidden
windows.unit_manager.windows[name = "_Exp"].layout.elements[name = text].text        set to text: the plugin's text instead of the game's
```

- An element is named by the [window schema](./game-map-v2.md#window-schemas) of its window, for the window's layout file in the running game build; the path names the window before its elements, with `[name = "<window>"]`. An element name the map does not have is `not-found`; a build whose layout of the window the map does not know is `unavailable`. An element the map does not name is never reached by a name.
- `text` exists only on text elements: a path through it reaches only those.
- Whether an element is shown and a text element's text are held: the host hides the element again and writes the text again whenever the game changes them, and in a window the game builds later, until the plugin restores the path or stops. Then the host shows the element and writes back the game's latest text where the plugin's value is still there; the game shows, hides, and writes as it wants from its next update.
- One plugin owns an element's value at a time; changing it needs `[lens] change = ["windows"]`.

## Replacement files

`file-replacements.replace(game-path, storage-path)` reads a file of at most 64 MiB from the plugin's storage, checks it against the game's original file, copies it into the host's cache, and from then on the game reads it whenever it loads `game-path`. Only formats the host can check are accepted:

- font tables (`.fdt`): the original's file and font headers (texture size, font size, line height, ascent) and kerning stay; glyph records stay sorted and lie inside the atlas, on one of the 28 pages of the game's seven font textures;
- textures (`.tex`): a 2D, 3D, or cube texture as the original is, in a format the game uses (L8, A8, B4G4R4A4, B5G5R5A1, B8G8R8A8, B8G8R8X8, R16G16B16A16F, BC1, BC2, BC3, BC4, BC5, BC7), sides of 1 to 4096 (multiples of 4 for block formats), a 3D texture 1 to 32 deep, no array, 1 to 13 mip levels that the size allows and levels of detail among them, every level's surfaces after the 80-byte header and inside the file, and unused level offsets zero. Every texture of the installed game passes these checks (`every_texture_of_the_installed_game_passes`). Font textures (`common/font/`) keep the original's header and size, which the font tables place their glyphs in, so only their pixels change;
- materials (`.mtrl`): the game's version, a size the header states and the sections fill exactly (at most 64 KiB), names that end inside the string table, texture paths of 1 to 259 lower-case ASCII characters, color tables of the size their flags give (16 rows, or 8 values by 32 rows, each with its dye table or without), constants inside the shader values on 4-byte boundaries, samplers that name one of the material's textures or none, a shader package one of the game's materials uses, or the original's, and at most 6 textures, 4 UV sets, 2 color sets, 5 shader keys, 31 shader constants, and 6 samplers. Every material of the installed game passes (`every_material_of_the_installed_game_passes`);
- models (`.mdl`, versions 5 and 6): a stack that holds the vertex declarations exactly, declarations of known element types inside their streams' strides, one per mesh, and a runtime that holds every section; every name inside the strings; every range of meshes, submeshes, terrain shadow meshes, shape meshes, shape values, and submesh bones inside its list; bone tables that name the model's bones; materials and bone tables a mesh names that exist; each level's vertex and index data inside the file, each mesh's vertices and indices inside its level's data, and every index naming a vertex of its mesh; every shape value inside the mesh of its level that it changes, which the game writes into; and at most 235 meshes, 1567 submeshes, 83 materials, 270 bones, 6 bone tables of at most 245 bones each (64 in version 5), 1652 submesh bones, 19 attributes, 26 shapes, 166 shape meshes, 49295 shape values, 15 element ids, 3 terrain shadow meshes, 236 terrain shadow submeshes, and 818 bone indices. Every model of the installed game passes (`every_model_of_the_installed_game_passes`).

The count and size limits are the largest the game's own files have: the game is known to load files up to them, and nothing shows it takes more. A file the game has no original for, another format, a failed check, and a file another plugin replaces are refused. Files the game loaded before keep what it read until it loads them again; replacing files in `start` while the game starts covers everything it loads. `restore` and disabling the plugin serve the game's own file from its next load. A plugin replaces at most 1024 files.

## Commands

`commands.define` replaces the plugin's commands with a tree: root commands such as `/hello`, subcommands such as `/hello greet` (at most 3 words deep), and typed parameters. Words — command names, aliases (at most 8 per command), and parameter names — are 1–32 lower-case ASCII letters, digits, and hyphens; descriptions are one line of at most 256 bytes. A plugin defines at most 64 commands of at most 16 parameters each. Optional parameters follow every required one, and only the last takes the rest of the line (`text`). A choice offers 1–64 distinct words without spaces or quotes. `help` is never a subcommand. A root name or alias must be free: not a command of the game in any of its languages — the host reads them from the game's `TextCommand` sheet, and without that sheet `define` returns `unavailable` — not `angelica`, which opens Angelica's menu, and not another running plugin's. A tree that breaks a rule is refused whole and the previous one stays.

When the player sends a line from the chat box or a macro whose first word is one of the plugin's roots or root aliases, case-insensitively, the game never sees it. In the next frame the host parses it: words are separated by spaces, and text in double quotes is one word. Subcommands and aliases are matched case-insensitively; then each parameter takes its argument by kind — a `word`, the rest of the line as typed for `text`, a whole number within the bounds for `integer`, a finite decimal number for `number`, one of the words for `choice` (passed as listed), `on` or `off` for `switch`. The plugin's `command` receives the command's path as defined and the arguments in parameter order; an optional parameter left out has no argument.

The host answers some lines itself, as the plugin's message in the chat log: `/<root> help` and `/<root> help <subcommand>` print help built from the descriptions; a line that does not fit (a missing or extra argument, a value of the wrong kind, an unclosed quote) prints what is wrong and the command's usage; a line with links, auto-translate phrases, or more than 1000 bytes is refused. An error returned by `command` is printed the same way. These answers need no `chat.print`. The host keeps at most 32 lines waiting for the next frame; a line past that does not run, and the host answers that it was not run. Disabling the plugin removes its commands; the game then handles those lines again.

## Chat

`chat.print` adds a message to the chat log, in the Echo channel. A message is a list of spans of plain text, each in a tone — `normal`, `accent`, `muted`, `good`, `warning`, or `bad` — and optionally italic; a line break starts a new line, and no other control character is allowed. A message has at most 64 spans and 2000 bytes of text, and is not empty. The host renders the tones with colors of the game's interface theme and starts every message with `[<plugin name>]` in a color no tone uses, so a plugin cannot pass its message off as the game's or another plugin's ([game structures](./game-structures.md#chat)). A plugin prints at most 20 messages in any second; past that `print` returns `limit-exceeded`. Before the game's chat log exists, during the game's start, it returns `unavailable`.

`chat.print-pieces` prints a message whose pieces are spans and up to 8 item links, under the same rules; a link alone makes a message. An item link names a row of the game's `Item` sheet, whether it is the high-quality version, and the language of its name by the game's number. The host writes it as the game writes an item the player links: the link's arrow, then the item's name as `excel.shown` gives it in that language, in the color of the item's rarity, with the mark of high quality; a click on it shows the item's tooltip as the game does for its own links ([game structures](./game-structures.md#chat)). The host takes the name and the rarity from the sheet by the game map's names `name` and `rarity`, so a link always names the item it shows; a sheet whose layout the map does not know is `unavailable`, and a row the sheet does not have, or one without a name, `not-found`.

## HTTP

A plugin with `http.get` sends HTTPS GET requests, and with `http.send` also data with POST, PUT, PATCH, and DELETE, to the hosts its manifest lists in `[http] hosts` ([manifest](./plugin-manifest-v1.md)), which the player approves with the plugin. Requests block, so they are made from jobs; the main instance gets `wrong-context`. Time spent in a request does not count against the job's 10 minutes.

Before anything is sent, the host checks the request: an absolute `https://` URL of at most 4096 bytes, without user name or password, on port 443, to a listed host — otherwise `invalid-argument`, or `permission-denied` for a host the manifest does not list. A plugin sets only the headers `accept`, `accept-language`, `if-none-match`, `if-modified-since`, and `range`, each once, with values of at most 1024 bytes and no control characters; a request that sends data may also set `authorization` and `content-type`. The host adds no credentials of its own: an `authorization` a plugin sets is its own service's. The host sets the user agent (`Angelica/<version> (plugin <id>/<version>)`), asks for gzip and decompresses it, sends no cookies or credentials, and ignores proxies configured on the system.

The same rules hold for every connection, including each of at most 5 redirects: the scheme is `https`, the port 443, and the host a listed one. A request goes through the proxy Windows uses for its URL, as the browser's do: the player's proxy settings, with automatic detection and a configuration script, or the manual proxy and its bypass list; it tunnels with `CONNECT`, and the proxy resolves the host. Without a proxy, the host name is resolved by the system, the `hosts` file included, and only public addresses, this computer's (`127.0.0.0/8`, `::1`), and the range VPNs hand out in place of real addresses (`198.18.0.0/15`) are used; a name that resolves only to private, shared, link-local, multicast, documentation, or reserved addresses is refused with `permission-denied`, so a plugin cannot reach the player's home network. TLS is the operating system's, with its certificate store and TLS 1.2 or later; a certificate the system does not trust for the requested host fails the request, so a service on the player's computer or network that holds no certificate for a listed host never receives a request, through a proxy or not.

`get` returns the final response's status, headers (names in lower case), and body, at most 16 MiB, within 60 seconds. `download` writes a 2xx response's body to a file in the plugin's storage, at most 1 GiB and within the storage quota, within 15 minutes; the file is replaced only when the whole body arrived, and another status leaves it as it was. `send` sends a body of at most 16 MiB with the method, none with `DELETE`, and returns the response as `get` does, within 60 seconds. `upload` sends a file of the plugin's storage, its cache included, of at most 1 GiB with `POST`, `PUT`, or `PATCH` and its length as `content-length`, reading it as it goes, and returns the response as `get` does, within 15 minutes. Any status is a response, not an error. A request that does not complete — the name does not resolve, the connection or TLS fails, the time runs out, the job is cancelled — returns `network`; a body over its limit returns `limit-exceeded`. Resolution and connection each take at most 15 seconds, and the response's head arrives within 30.

## WebSocket

`websocket.open` opens a connection to a `wss://` URL under the rules of [HTTP](#http): a host the manifest lists, on port 443, at a public address or through the proxy Windows names, with the system's TLS and the host's certificate checked. It offers at most 8 subprotocols and may send an `authorization` header of at most 1024 bytes; no other header is set. It returns the connection's id at once; the host connects and shakes hands on a thread of its own, and `socket-opened`, with the subprotocol the server chose, or `socket-closed` arrives in a later frame. A URL that is not `wss://` or names an unlisted host is refused by `open` itself.

`send` queues a text or binary message of at most 16 MiB; at most 64 wait to be sent on a connection, `limit-exceeded` otherwise. The host answers the server's pings. Messages the server sends arrive as `socket-message`, at most 16 MiB each, in the order they came; a larger one ends the connection. The host keeps at most 256 events of a plugin's connections between frames and delivers at most 64 in a frame: when the plugin falls behind, its connections wait instead of dropping messages. `close` closes the connection with 1000 once what was queued is sent. `socket-closed` carries the server's close code and reason, or 1006 and what went wrong when the connection failed or ended without one; its id is not used again, and `send` on it is `not-found`. A plugin keeps at most 4 connections open; they close with 1001 when it stops. Main instance only; needs `http.websocket`, which comes with `http.get` and its hosts.

## Messages between plugins

`messages.send` sends a payload of at most 1 MiB under a name of 1 to 64 ASCII letters, digits, dots, underscores, and hyphens to another plugin, and returns the message's id. In the next frame the receiver gets it in `on-game-event` as `message`, with the sender's plugin id, the name, the payload, and the id; it keeps at most 256 messages between two frames and is told how many more were left out with `messages-dropped`. No plugin runs another's code: each reads its messages in its own calls, and what a payload means is between the two plugins.

A plugin sends to the plugins its manifest lists in `[messages] to`, which the player approves with `messages.send`; another receiver is `permission-denied`. Any plugin answers a message it received by naming it in `in-reply-to`: the answer goes to the message's sender, listed or not, and needs no permission; naming a message the plugin did not receive from that plugin is `invalid-argument`. A receiver that does not run is `unavailable`. Main instance only.

## Clipboard

`clipboard.copy` puts text of at most 64 KiB without NUL characters on the player's clipboard at the end of the frame, replacing what was there; the last text of a frame wins. A plugin copies only for an action of the player: from the main instance while it handles `ui-event` or `command`, and `wrong-context` anywhere else, so no plugin changes the clipboard on its own. No plugin reads the clipboard; the player pastes into a plugin's text fields as anywhere else.

## Links

`links.open` opens a page in the player's browser: the one Windows uses for links, never a program the plugin names. The URL is an absolute `https://` URL of at most 2048 bytes without spaces or control characters, user name, or password, on port 443, to a site the manifest lists in `[links] hosts` ([manifest](./plugin-manifest-v1.md)), which the player approves with `links.open`; another URL is `invalid-argument`, and an unlisted site `permission-denied`. A plugin opens a page only for an action of the player: from the main instance while it handles `ui-event` or `command`, and `wrong-context` anywhere else, so no page opens on a plugin's own. It opens at most one page a second, `limit-exceeded` otherwise. The host opens the page at the end of the frame, on a thread of its own, so the game never waits for the browser; the plugin learns nothing of what the browser does.

## Notifications

`notifications.show` shows one line of 1 to 200 characters without control characters at the top right of the screen for 7 seconds, drawn by the host as Angelica's own notes are, under the plugin's name and with a mark: the attention mark when `attention` is set, the running mark otherwise. With `attention`, the game's button on the taskbar also flashes while the game is in the background, until the player returns to it. A plugin notifies at most once in 2 seconds, `limit-exceeded` otherwise; at most 3 notifications of all plugins are shown at once, the oldest leaving first, and a stopped plugin's notifications leave with it. Notes are drawn while the menu is closed. Main instance only.

`notifications.pop-up` shows a place of the kind `popup` for its `seconds`: in the corner with the notes, large above the middle of the screen (`center`), or beside one of the plugin's panels (`panel`; in the corner while that panel is not shown). It needs `notifications` and shares `show`'s limits; a popup that breaks the [rules of places and parts](#rules-for-every-place-and-part) is `invalid-argument`.

## Sound

`sound.play` plays a WAV file from the plugin's storage at a volume from 0 to 1. The host reads the file, at most 8 MiB, and checks it: a RIFF WAVE file with one format chunk and one data chunk inside the file, PCM samples of 8, 16, or 24 bits or 32-bit float (also inside `WAVE_FORMAT_EXTENSIBLE`), 1 or 2 channels, 8 to 96 kHz, at most 10 seconds; anything else is `invalid-argument`. It then writes its own 16-bit PCM copy with the volume applied, float samples outside -1 to 1 clamped and others silenced, and Windows plays that copy, never the plugin's file. One sound plays at a time: a new one, of any plugin, stops the one playing. A plugin plays at most one sound a second, `limit-exceeded` otherwise. Main instance only.

## Drawing over the world

`draw.set` replaces what the plugin draws over the game world: lines between places, circles on the ground level with their centers, filled or as outlines, and text centered on places. A place is in yalms with `y` up, as the positions of the game's objects are; a color is `0xRRGGBBAA`, a thickness in points of Angelica's interface. The shapes stay until the plugin sets others or stops; an empty list draws nothing.

At most 1024 shapes, with finite places, radii, lengths, and thicknesses of at most 1000 yalms, and texts of at most 256 bytes without control characters; more returns `limit-exceeded`, a shape that breaks the rules `invalid-argument`. Every frame the host projects the shapes through the game camera's view-projection matrix from the game map and draws them under every window; a part behind the camera is not drawn, and a filled circle partly behind it is drawn as its outline. Nothing is drawn while the game has no camera or the build lacks the matrix's map entries. Requires `draw.world`; main instance only.

## Intercepts

A plugin that holds `unreviewed` describes functions of the game in its own [Anatomia](./anatomia-1.md#a-plugins-own-entries) entries and intercepts them. When it starts, the host resolves its entries against the game's executable as it was mapped before any hook, and logs every entry that is absent with the reason.

- `intercepts.before(function, handler)` and `intercepts.after(function, handler)`, from the main instance, hook a function of the plugin's own entries that is present in the build: on each call of it the host calls `handler.intercepted` with the function's name, the handler's number, the phase, the arguments by position, and, after the game's code, the result and whether a `before` handler skipped the code. The function has at most 16 arguments, each an integer, a float, a `bool`, an enum with a `type`, or a pointer, and returns one of these or nothing; another type returns `invalid-argument`, an absent function `unavailable`. A plugin intercepts a function once per phase.
- Values arrive as the function's type in the entries gives them: a signed or unsigned integer of its width (an enum as its type), a `bool`, a 32- or 64-bit float, an `object` handle for a non-zero pointer to a structure of the entries, `text` for a non-zero `ptr<char8>`, `pointer` for another non-zero pointer, and `none` for a zero pointer. A handle is valid only in the call it came with; the plugin never sees an address.
- In a `before` handler, `intercepts.set-argument(index, value)` changes an argument for the handlers after it and the game's code, and `intercepts.skip(value)` makes the function return `value` without running the game's code. In an `after` handler, `intercepts.set-result(value)` changes what the function returns. A value has the type of what it replaces: a number in its range, `none` or a handle of this call of the same type for a pointer; another value returns `invalid-argument`, the wrong phase `wrong-context`. A plugin cannot make up a pointer: it can only pass on one the game gave in the same call.
- `intercepts.text(of)` reads the text of a `text` handle, up to 64 KiB, or of an `object` whose structure reads as text, and `intercepts.edit-text(of, bytes)` replaces it in place with as many bytes and no NUL, in writable memory of the game only.
- `intercepts.publish-snapshot(bytes)`, from the main instance, gives handlers up to 64 MiB of read-only data from their next call on, and `intercepts.snapshot()` returns it in a handler; a handler that is running keeps the bytes it started with.

A handler runs in an **intercept instance**: one instance of the component per plugin and game thread that reached one of its handlers, made at the first call and kept while the plugin runs, with its own memory and nothing shared with the main instance but the snapshot. It never runs `start`. It may use `runtime.log` and `intercepts`; every other import traps there. A call runs for at most 50 ms of wall time. A trap or a call past the limit disables every handler of the plugin and is logged; the changes it made to the call are dropped, the call proceeds as if it had not been there, and the main instance goes on. Changes it made to the game's memory with `edit-text` stay. A call of an intercepted function from inside a handler runs no handlers; a function the game's code calls again from inside a call runs them again. Disabling the plugin removes its handlers; their hooks stay and run only the game's code.

Several plugins' handlers of one function run in one order: the order the plugins intercepted it, adjusted by what their manifests' [`[intercepts]`](./plugin-manifest-v1.md) tables ask for. `before` handlers run from first to last, each seeing the arguments the one before it left; a `skip` ends them. When the function has `after` handlers, the host runs the game's code itself and then the `after` handlers from last to first, so a plugin that runs before another sees the result after it. Wishes that contradict each other are ignored: the plugin that intercepts last goes last, and its log says so.

## WASI

A component built by Rust's standard library imports WASI. The host provides `wasi:random` (`random`, `insecure`, `insecure-seed`; at most 1 MiB per call) and `wasi:clocks` (`monotonic-clock.now` and `resolution`, `wall-clock`), which expose nothing of the player's system and which `HashMap` and `Instant` use. Every other WASI function — files, environment, sockets, standard streams, polling — traps when called. A component that imports anything other than `angelica:plugin/*@1.0.0` and WASI is refused at installation.

## Compatibility

Interfaces and functions of `angelica:plugin@1.0.0` do not change. New interfaces join the package, as `intercepts` and `handler` did, and a component that neither imports nor exports them runs unchanged. Places and parts grow without a new package version: a later 1.x adds [kinds and properties](#places-and-parts), which a plugin uses by raising `plugin.api` in its manifest to that version; a host refuses a plugin whose `plugin.api` is newer than the one it runs.
