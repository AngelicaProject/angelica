# Game map format v1

The game map is a directory of UTF-8 TOML files in the `angelica-gamemap` repository. `angelica-map` reads it; nothing else interprets it.

```text
map.toml                 format version
entries/<area>.toml      hand-maintained entries, one file per area
sheets/<sheet>.toml      sheet schemas: names for the columns of the game's Excel sheets
builds/<version>.toml    generated build records, one per game executable
```

`map.toml`:

```toml
format = 1
```

A reader refuses any other `format` value.

## Entry names

An entry name is lowercase dotted words, `<area>.<subject>[.<member>]`, for example `excel.hash_table_page_row_resolver.store_row` or `text.utf8_string.used_size`. Each word starts with a letter and contains lowercase letters, digits, and underscores; a name has at least two words. The area is the file the entry lives in: every entry of `entries/excel.toml` starts with `excel.`. A name is defined once in the whole map. A name is stable: renaming an entry used by published plugins keeps the old name as an alias for at least one major version of the plugin API.

## Entry kinds

Each entry is a table keyed by its name under its kind.

| Kind | Describes | Required fields |
| --- | --- | --- |
| `function` | a function | `locator`, `abi`, `thread` |
| `location` | an instruction inside a function, for a mid-function hook | `locator`, `thread` |
| `global` | a static variable | `locator`, `type` |
| `vtable` | a virtual function table | `locator`, `slots` |
| `field` | a field of a structure | `of`, `locator` or `offset`, `type` |
| `patch` | bytes to replace | `locator`, `original`, `replacement` |

A field has either a `locator`, which must be an `in` locator, or a constant `offset`. `type` is a value kind other than `void` (see below). `original` and `replacement` of a patch are hexadecimal bytes without wildcards, of the same length.

Common optional fields: `doc` (one sentence: what it is, not how it was found), `checks`, `depends` (names of entries that must be present), and `origin`.

A field that does not belong to the entry's kind, a table other than the six kinds, and an unknown key make the whole map invalid.

## Locators

| Locator | Resolves to |
| --- | --- |
| `{ signature = "48 8B ?? …" }` | the start of the single match in the code section |
| `{ signature = "…", at = n }` | the match start plus `n` |
| `{ signature = "…", follow = "rel32", at = n }` | the target of the 32-bit relative operand at match start plus `n` (calls, jumps, RIP-relative data) |
| `{ vtable = "<entry>", slot = n }` | the address held in slot `n` of a `vtable` entry with more than `n` slots, made relative to the image's load address |
| `{ in = "<function entry>", signature = "…", operand = "disp" }` | for a `field`: the displacement of the memory operand of the single instruction matching the signature inside that function |

A signature is hexadecimal bytes with `??` for any byte; it is searched in every code section. A locator that matches zero or more than one address makes the entry absent. Overlapping matches count as separate matches.

The code an `in` locator searches is the function's entry in the executable's exception directory. A leaf function has no entry; its code is the instructions from its start up to the first `int3`, the start of the next function in the exception directory, or the end of the section, at most 64 KiB. The memory operand must have a base register other than RIP and a non-negative displacement.

## Function ABI and thread

```toml
[function."excel.hash_table_page_row_resolver.store_row"]
locator = { signature = "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 44 8B 49 18 33 C0 49 8B F0 48 8B FA 45 85 C9" }
abi = { args = ["ptr", "ptr", "ptr"], ret = "u8" }
thread = "any"
checks = [{ vtable_slot = 3, code_slots = 8 }]
doc = "Stores a parsed Excel row in a page whose rows are kept in a hash table."
```

`abi` lists argument and return kinds of the Windows x64 calling convention: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `ptr`, `bool`, and, for the return kind only, `void`. `thread` is `main` or `any`.

## Checks

`checks` is a list of conditions that must hold after the locator resolves. A failed check makes the entry absent.

| Check | Holds when |
| --- | --- |
| a locator | it resolves to the same value as the entry |
| `{ equals = "<entry>" }` | that entry is present with the same value |
| `{ vtable_slot = n, code_slots = k }` | the entry's address is slot `n` of at least one 8-byte aligned table outside code whose first `k` slots (`k > n`) all point into code |

## Resolution

Entries are resolved after every entry they refer to (through a locator, a check, or `depends`); references must name existing entries of the right kind and must not form a cycle. An entry is absent when an entry it refers to is absent. After its locator, an entry is checked for its kind:

- a `function`, `location`, or `patch` address lies in a code section;
- every slot of a `vtable` points into code;
- the bytes at a `patch` address equal `original`.

Then its checks are evaluated in order.

## Origin

`origin` records where an entry came from when it was not found in this repository's own work, as `{ source = "<name>", commit = "<id>" }`. An entry with an origin is used like any other once it is verified; the origin keeps the provenance the source's license requires.

## Sheet schemas

A sheet schema names columns of one of the game's Excel sheets. Its path is `sheets/` followed by the sheet's name as the game lists it and `.toml`; a sheet name with `/` is a path with folders.

```toml
sheet = "Item"
doc = "Every item: its names and description."

[columns.name]
type = "text"
doc = "The name as menus and tooltips show it."

[[layouts]]
fingerprint = "eaea5cde48d5801d"
columns = { name = 9 }
```

- `sheet`: the sheet's name, equal to the file's path without `sheets/` and `.toml`.
- `doc`, and each column's `doc`: what the sheet or column holds, not how it was found; not blank.
- `columns`: at least one named column. A name is a lowercase letter followed by lowercase letters, digits, and underscores, at most 64 bytes. `type` is the column's type in the game's sheet header: `text`, `bool`, `int8`, `uint8`, `int16`, `uint16`, `int32`, `uint32`, `float32`, `int64`, `uint64`, or `packed_bool_0` to `packed_bool_7` for one bit of a byte.
- `layouts`: at least one layout of the sheet the names were checked against. `fingerprint` identifies the layout; `columns` gives every named column its position among the sheet's columns, each position once.

A layout is the sheet's variant and every column's type and byte offset, as the game's sheet header lists them. Its fingerprint is 16 lowercase hexadecimal digits: the first 8 bytes of the SHA-256 of the ASCII text `rows` or `subrows` followed, for every column in order, by `;<type>@<offset>` with the offset in decimal.

A name applies in a game build only when the build's layout of the sheet has the fingerprint of one of the schema's layouts and the column at the name's position has the name's type. Otherwise every name of the sheet is absent in that build: a patch that moves, adds, or retypes any column of the sheet leaves its names absent until a layout for the new build is added. A name never reads another column.

A schema that breaks a rule makes the whole map invalid. Sheet schemas are not part of `entries_sha256`: they do not affect how entries resolve against the executable.

## Build records

Generated by `angelica map verify --write` as `builds/<game version>.toml`, never edited by hand:

```toml
# Generated by `angelica map verify`. Do not edit.
game_version = "2026.09.15.0000.0000"
exe_sha256 = "…"
entries_sha256 = "…"

[present]
"excel.hash_table_page_row_resolver.store_row" = 0x2088180
"text.utf8_string.used_size" = 0x10

[absent]
"ui.example.field" = "signature matched 2 addresses"

[constant_offsets]
names = ["text.utf8_string.string_ptr"]
```

- `game_version` is the content of the game's `ffxivgame.ver`.
- `exe_sha256` is the SHA-256 of `ffxiv_dx11.exe`. A build record applies only to the executable with this hash.
- `entries_sha256` identifies the map the record was produced from: the SHA-256 over `map.toml` and then every file of `entries/` in path order, each as its path relative to the map directory with `/` separators, a NUL byte, its length as a little-endian 64-bit integer, and its bytes.
- `present` holds the value of every present entry: an RVA (relative to the image base) for every kind except fields, the offset for a field.
- `absent` holds the reason for every absent entry.
- `constant_offsets` lists the present fields whose offset is a constant of the map rather than derived from code.

The same map and executable always produce the same text.
