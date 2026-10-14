# Game map resolution and verification

The game map describes where functions, globals, vtables, fields, and patch sites are in each game build, and, in its object model, what the game's objects are. The format is [`../formats/game-map-v2.md`](../formats/game-map-v2.md); the data lives in the [`angelica-gamemap`](https://github.com/AngelicaProject/angelica-gamemap) repository.

## One resolver, two places

`angelica-map` resolves entries against a PE image. The same code runs:

- **offline** in `angelica map verify`, over the executable file on disk, to produce a verification report and a build record;
- **in the game**, over the mapped module, when the running executable has no build record.

Offline and in-game resolution therefore cannot disagree about what an entry means. The executable's code section is not packed, so the file on disk contains everything resolution needs.

## Resolution

An entry is present for a build when:

1. its locator resolves to exactly one address;
2. every consistency check declared on the entry holds (for example, the function is the expected slot of its vtable, or a derived offset equals the value an independent locator gives);
3. every entry it depends on is present.

Otherwise the entry is absent, with a reason. There is no partial or best-effort result.

## Sheet schemas

Plugins read the game's Excel sheets by column name. The map's sheet schemas give those names, each for the exact layouts of the sheet it was checked against ([format](../formats/game-map-v2.md#sheet-schemas)). The layout comes from the sheet's header in the installed game's archives, not from the executable, so it is checked against the game's data:

- the host computes the fingerprint of each sheet's layout when a plugin first reads the sheet by name, and uses the schema's positions only for a layout the schema lists;
- `angelica map verify` checks every schema against the game data next to the executable, reports each sheet as known or unknown with the reason, and fails when a sheet is unknown;
- `angelica excel show <sheet> --map <map>` shows a sheet's layout and fingerprint, every column with its type, offset, and name, and rows of values — what a maintainer needs to name a column or to add the layout of a new build.

A column is named when a plugin needs it, from the values it holds, never speculatively.

## Object model

Plugins read the game's objects through [Lens](./lens.md), by paths through the map's object model ([format](../formats/game-map-v2.md#object-model)): types whose fields are verified `field` entries, roots at verified `global` entries, events at verified code, actions that call verified functions, enums, and the domains everything belongs to. The host resolves the model for the running build at start and with every map reload: a root or field whose entry is absent is unavailable, and the rest of the model works.

A field is visible to plugins because a type names it. Types and fields are a contract: they keep their names and meaning across builds, a field that would expose data that could compromise the account is never added or is marked forbidden, and a field's domain is one whose approved wording covers it. `angelica map verify` reports every root and field present or absent for the build; `angelica look` reads them in the running game with a development build, to confirm that a new type is what its `doc` says.

## Window schemas

Plugins reach elements of the game's own windows by window and element name, through a field of the object model whose meaning is `element`. The map's window schemas give the element names, each for the exact layout files of the window it was checked against ([format](../formats/game-map-v2.md#window-schemas)). The layout file is in the installed game's archives, so it is checked against the game's data, as sheets are:

- the host computes the fingerprint of every schema's layout file when it resolves the object model for the build, at start and with every map reload, and uses the schema's ids only for a file the schema lists;
- `angelica map verify` checks every schema against the game data next to the executable, reports each window as known or unknown with the file's fingerprint, and fails when a window is unknown.

An element is named when a plugin needs it, after it was seen in the game to be what its `doc` says, never speculatively. A plugin cannot change an element the map does not name.

## Reloading the map

While the game runs, the host looks at the signed map every second. When its file list changes, the host authenticates it, resolves it against the running executable again, and replaces the sheet schemas and the object model plugins use, with the window schemas' names. Entries that installed hooks use keep their addresses until the next game start.

## Build records

A build record maps the hash of one executable to the resolved address of every entry and the result of every check. It is produced offline by `angelica map verify` and published with the map. With a record for the running executable, the host does not scan: it checks the hash and uses the recorded addresses.

Without a record (a patch the map has not caught up with), the host resolves in the game. Entries that pass resolution are used. The overlay marks the build as unverified until a record is published.

## Offsets anchored in code

A field offset is preferably derived from code: "the displacement of the memory operand of the instruction matched by this pattern in this function". A derived offset is recomputed for every build. An offset given as a constant is allowed only while no code anchor is known, and the verification report lists every constant offset, so the list of unanchored offsets is always visible.

## Patch-day tools

- `angelica map verify --map <map> --game <game folder> [--write] [--json]` resolves every entry against `ffxiv_dx11.exe` and reports present and absent entries; `--write` writes the build record. It exits with 0 when every entry is present, 1 when an entry is absent or the map or the game cannot be read, and 2 on a usage error.
- `angelica map recover` proposes locators for entries that no longer resolve, by matching each lost function's fingerprint (normalized instructions, calls, constants, referenced strings, vtable positions) from the previous build against the new one. Proposals are verified like any other change before they are accepted.
- `angelica map diff` compares two builds' reports.
- `angelica map import` converts externally maintained data into entries marked as imported; see the game map repository for its rules.
- `angelica exe disasm` disassembles the executable from an address or a map entry to the end of its function; `angelica exe find` lists every match of a signature with the function it is in; `angelica exe signature` proposes the shortest signature that starts at an instruction and matches only there — in the whole code, or with `--within` inside the instruction's function for an `in` locator — leaving out the bytes that move between builds: branch and call targets, RIP-relative displacements, and, with `--within`, the displacement the locator reads.
