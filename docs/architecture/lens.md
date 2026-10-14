# Lens

Lens is how plugins reach the game's objects. The host is an engine with a few verbs over the game; everything the verbs apply to is described as data in the game map. A new readable structure is a description in the map's [object model](../formats/game-map-v2.md#object-model), not new host code. The plugin still holds no address and runs no code in the game: it names roots, fields, and domains, and the host reads by the map.

The plugin API's `lens` interface offers `query`, reading objects by path, and `watch`, reading the objects the game's code works on when it reaches an event of the map ([contract](../formats/plugin-api-v1.md#lens)).

## Parts

| Part | Owns | Code |
| --- | --- | --- |
| object model | types, fields, roots, events, enums, domains, and their rules | `angelica-map` (`objects`, `events`) |
| schema | the model resolved for the running build: each root's address, each field's offset, or why the build lacks it | `angelica-host` (`lens::schema`) |
| query language | the text of a query, parsed without the model | `angelica-host` (`lens::query`) |
| planner | a query checked against the schema and the plugin's domains, turned into a plan of reads | `angelica-host` (`lens::plan`) |
| reader | a plan run over the game's memory, from a root or from an event's call | `angelica-host` (`lens::read`) |
| watches | the points of the game's code that watches use, their subscribers, and the firings kept for the next frame | `angelica-host` (`lens::watch`) |
| watch hooks | a hook at each watched point that hands the registers to the watches | `angelica-host` (`interop::watch`), `test.fire` of the scripted game in tests |
| memory | reads of the game process that fail instead of faulting | `angelica-host` (`interop::game`), the scripted game in tests |

All parts but the memory are platform independent and tested without the game; the Windows adapter only supplies the reads. The scripted game of `angelica-fake-game` lays out objects in memory as a test's map describes them, so a test runs a plugin's queries through the same planner and reader.

## Prepare and run

A plugin prepares a query once and runs it as often as it needs: every frame, or on an event.

- **Prepare** parses the query and checks every name against the schema: the root, each field on the path, in the conditions, and in the answers. It checks that the plugin holds each field's domain, that no field is forbidden, and that the running build has every entry the plan needs. The result is a plan: offsets, pointers to follow, arrays to unfold, conditions with their numbers. Every mistake is named before anything is read.
- **Run** follows the plan from the root's address and only reads. Memory is read a page of 4 KiB at a time, each page once per run, through the operating system's checked copy: an unreadable address makes a value absent instead of faulting the game. A run answers at most 4096 rows from at most 4096 pages.
- When the game map is reloaded, the schema is replaced, and every held query is planned again at its next run.

Reads happen on the game's main thread, between frames, from a plugin's main instance: the game does not change its objects meanwhile, and the values of one run belong to one moment.

## Watches

A watch is a query from an event of the map. Its plan starts at an argument of the call instead of a root's address: a value is read from its register, a reference is followed into the objects as from a root.

- A point is the map entry of an event, a `function` or a `location`. The host installs one [mid-function hook](./hooks.md#mid-function-hooks) per point, the first time a watch or `angelica look` needs it, and keeps it until the process exits. While nothing watches the point, the hook records nothing.
- When the game reaches the point, the hook passes the registers to every watch of the point. Each reads its rows at once, through the same checked copy, at most 256 rows from at most 64 pages, and keeps them for the plugin. The hook changes no register and calls no plugin code; the game's code runs on unchanged.
- In the next frame the host delivers each plugin's firings as `watched` game events. A watch keeps at most the event's `rate` of firings between two frames and counts the rest as `watch-dropped`.
- When the game map is reloaded, every watch is planned again; one that no longer fits receives nothing until its plugin watches again.

Values read in a hook belong to the moment of the call, on whatever thread made it, not to a moment between frames.

## Domains

Each field belongs to a domain of the map, by its type or its own `domain`. A query reads only the domains the plugin's manifest lists and the player approved ([permissions](./permissions.md#domains-of-the-game-map)). The root itself needs no domain: reading anything of it is reading one of its fields.

An array whose elements belong to different domains has a partition: the value of a field of the element decides each element's domain, as the kind of an item container does for the bags, the armoury chest, the saddlebags, and the retainers. The planner adds the partition as a condition on the elements: only elements of permitted domains with a named value are read, and the fields below them count under those domains. A condition on that field with `=` or `in` names the elements the query wants, and each must be permitted.

## Tools

- `angelica schema search <words> --map <map> [--game <game folder>]` lists the roots, types, fields, events, domains, and enums whose names or descriptions have every word; with a game folder, whether each is present in that build.
- `angelica schema show <root, type, event, domain, or enum> --map <map> [--game <game folder>]` shows it with everything in it: a type's fields or an event's arguments with their shapes, domains, and descriptions.
- `angelica query "<query>" --map <map> [--game <game folder>] [--plugin <folder>]` checks a query as `prepare` does, or a watch as `watch` does, without the game: the columns, the domains it reads, and with a game folder whether the build has everything; with a plugin folder, against the domains its manifest lists.
- `angelica look "<query>" --installation <folder>` runs a query in the running game through a development build of the host and prints the rows, for work on the map: describe a type, query it, compare with what the game shows. Every domain is readable; forbidden fields are not. A release build never answers ([format](../formats/host-installation-v1.md#lens-requests)). Its other modes help find what to describe:
  - `--watch <seconds> "<watch>"` watches an event for that long and prints each firing;
  - `--dump <bytes> "<path>"` prints the first bytes of every object a path reaches; with `--json`, it saves them for `--compare <before> <after>`, which prints the bytes that changed between two dumps, such as before and after something happens in the game;
  - `--trace <entry> [--trace <entry>…] [--seconds <n>]` counts how often the game's code reaches `function` and `location` entries, to find the one an action of the player calls.

Every command can report in JSON with `--json`.
