# Game map format v2

The game map is a directory of UTF-8 TOML files in the `angelica-gamemap` repository. `angelica-map` reads it; nothing else interprets it.

```text
map.toml                 format version
entries/<area>.toml      hand-maintained entries, one file per area
domains.toml             the object model's domains: what plugins ask the player to read
roots.toml               the object model's roots: where paths through the objects start
events.toml              the object model's events: points of the game's code that plugins watch
actions.toml             the object model's actions: functions of the game plugins call for the player
types/<area>.toml        the object model's types: the game's structures and their fields
enums/<name>.toml        the object model's enums: names for the values of a field
sheets/<sheet>.toml      sheet schemas: names for the columns of the game's Excel sheets
windows/<window>.toml    window schemas: names for elements of the game's own windows
builds/<version>.toml    generated build records, one per game executable
```

`map.toml`:

```toml
format = 2
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

A field has either a `locator`, which must be an `in` locator, or a constant `offset`. `type` is a value kind other than `void` (see below), or `struct` for a structure embedded at the global's address or the field's offset, which is never read as a value. `original` and `replacement` of a patch are hexadecimal bytes without wildcards, of the same length.

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

## Object model

The object model describes the game's objects for Lens (`docs/architecture/lens.md`): the structures of the game as types with fields, the roots where a path through them starts, the events where the game's code hands objects to a watch, the enums that name a field's values, and the domain every field belongs to. Entries say where things are in a build; the object model says what they are. A plugin's query is a path through the model; the model never holds an address or an offset of its own, except the strides and lengths of arrays.

### Domains

`domains.toml` lists the domains. A domain is the unit of a plugin's permission to read: a plugin lists the domains it reads in its manifest, and the player approves each in the domain's own words.

```toml
[domains.inventory]
doc = "The items the player carries: the bags, the equipped gear, the currency, the crystals, and the key items."
class = "you"
read = { en = "Sees the items you carry", ru = "Видит предметы, которые вы носите" }
```

- A domain is named by a lowercase letter followed by lowercase letters, digits, and underscores, at most 64 bytes.
- `doc`: what the domain holds; not blank.
- `class`: how far the domain reaches, as the player sees it: `you` (only about the player), `display` (changes what the player sees), `others` (sees other players), `acts` (acts in the game for the player), or `acts_on_others` (acts toward other players). A host grants a domain only of a class it presents.
- `read`: what reading the domain allows, in English (`en`) and Russian (`ru`), as the approval shows it; neither blank. `act`, in the same form, says what acting in the domain allows; a domain without it has no actions. `change`, in the same form, says what changing the domain allows; the plugin API does not use it yet.

A new domain and its wording are approved by the maintainer once; types and fields join a domain without a new approval. A domain is narrow: what one kind of plugin needs, not everything about the player.

### Enums

`enums/<name>.toml` names the values of a field. The enum's name is the file's name without `.toml`: dotted words like an entry name, such as `inventory.container_kind`.

```toml
doc = "The kinds of item container, as the game numbers its inventory types."
values = { bag_1 = 0, bag_2 = 1, crystals = 2001 }
```

- `doc`: what the values are; not blank.
- `values`: at least one value. A name is a lowercase letter followed by lowercase letters, digits, and underscores; two names never have the same number.
- `flags`: when true, every value is a single bit, and a field with the enum holds a set of them.

### Types

`types/<area>.toml` holds the types of an area. A type is a structure of the game; its name is the name the `field` entries of the structure have in their `of`, and starts with the file's area.

```toml
[types."inventory.inventory_container"]
doc = "One container of items: a bag, the equipped gear, the currency, a part of the armoury chest, a saddlebag, or a retainer's page."
domain = "inventory"

[types."inventory.inventory_container".fields.kind]
doc = "Which container this is."
enum = "inventory.container_kind"

[types."inventory.inventory_container".fields.items]
doc = "The container's slots; an empty slot is skipped."
array = { of = "inventory.inventory_item", count = "size", most = 200, stride = 72, present = "item" }
```

- `doc`: what the type is; not blank.
- `domain`: the domain of the type's fields, unless a field names another.
- `extends`: optional; a type whose fields this one has too, at the same offsets, such as a character that is also a game object. A field of the base keeps the base's entry and domain.
- `fields`: the type's fields, each named by a lowercase letter followed by lowercase letters, digits, and underscores, at most 64 bytes.

A field is located by a `field` entry: `at`, or by default the entry `<type>.<field>`. The entry's `of` is the type that declares the field. The field's shape is one of:

| Shape | Written as | The entry's `type` | Read as |
| --- | --- | --- | --- |
| value | nothing, or `sheet`, `enum`, `flags`, `unit` | a number or `bool` | the value of the entry's type |
| text | `text = n` | `struct` | NUL-terminated UTF-8 of at most `n` bytes (1 to 4096), stored in the object |
| reference | `ref = "<type>"` | `ptr` | a pointer to an object of the type; zero is no object |
| embedded object | `embed = "<type>"` | `struct` | an object of the type at the field's offset |
| array | `array = { … }` | `struct` (in the object) or `ptr` (behind a pointer) | elements of a type |

A value's meaning is at most one of: `sheet = "<sheet>"`, a row of the game's sheet; `enum = "<enum>"`, one of the enum's values; `flags = "<enum>"`, a set of a flags enum's bits. A sheet row, an enum value, and flags are integers. `unit` names the unit of a number, such as `yalms` or `s`.

An array gives:

- `of`: the type of the elements.
- `length = n`, the number of elements, or `count = "<field>"` with `most = n`: an integer field of the object holding the array that says how many elements are used, at most `n`. `n` is 1 to 4096.
- `stride`: the bytes from one element to the next, 1 to 65536; with `pointers = true`, every element is a pointer to the object, a zero pointer is skipped, and the stride is 8.
- `present`: optional; a value or text field of the element. An element whose value of it is zero, false, or empty is skipped.
- `partition`: optional; `{ field = "<field>", domains = { <domain> = ["<value>", …] } }`. The elements belong to domains by the value of `field`, a field of the element with an enum: a value listed under a domain belongs to it, any other named value to the array field's own domain, and an element whose value has no name in the enum is never read.

Every field may also have `domain`, the field's domain when it is not the type's, and `forbidden = true`, which describes the field for completeness while no plugin ever reads it.

### Roots

`roots.toml` lists where paths start.

```toml
[roots.inventory]
doc = "The player's items."
global = "inventory.inventory_manager.instance"
type = "inventory.inventory_manager"
```

- A root is named like a domain. `doc` is not blank.
- `global`: a `global` entry; of type `struct`, it is the object of `type`; of type `ptr`, it holds a pointer to it, and the root has no object while the pointer is zero.

### Events

`events.toml` lists the points of the game's code that plugins watch. An event is a `function` entry, which fires when the game calls the function, or a `location` entry, which fires when the instruction runs; its arguments are the values the game's code has there. A watch is a query that starts at an event instead of a root.

```toml
[events.action_used]
doc = "The player asked to use an action: from a hotbar, a menu, a macro, or the duty finder."
at = "action.action_manager.use_action"
domain = "actions"
rate = 32

[events.action_used.args.kind]
doc = "The kind of action."
arg = 1
enum = "action.action_type"

[events.action_used.args.action]
doc = "The action's id: a row of the Action sheet for an action, of the Item sheet for an item, and so on by kind."
arg = 2
```

- An event is named like a domain, and never like a root. `doc` is not blank.
- `at`: a `function` or `location` entry.
- `domain`: the domain of the event's arguments, unless an argument names another.
- `rate`: optional, 1 to 1024, 64 by default: the most firings a watch keeps between two frames.
- `args`: at least one argument, each named like a field.

An argument gives where its value is and what it is:

- `arg = n`, for a function: the argument at position `n` from 0, its kind from the function's `abi`. `register = "<register>"` with `kind = "<value kind>"`, for a location: one of `rax`, `rbx`, `rcx`, `rdx`, `rsi`, `rdi`, `rbp`, `r8` to `r15`, or the low 64 bits of `xmm0` to `xmm15`. A float is in an `xmm` register, and only a float.
- Its shape: a value, with `sheet`, `enum`, `flags`, and `unit` as for a field; `text = n`, NUL-terminated UTF-8 of at most `n` bytes behind a pointer; or `ref = "<type>"`, a pointer to an object of the type, from which a watch's path goes on. Text and references have the kind `ptr`.
- `domain` and `forbidden`, as for a field.

### Actions

`actions.toml` lists what plugins may do in the game for the player. An action is a `function` entry of the main thread that the game's own interface calls, and how each of its arguments is given.

```toml
[actions.teleport]
doc = "Teleports to an aetheryte the player has attuned to, paying the fee, as the teleport window does."
call = "teleport.telepo.teleport"
domain = "teleport"
rate = 1
ok = true
when = [
    { path = "player", says = "your character is not in the world" },
    { path = "activity[in_combat = false]", says = "you are in combat" },
]

[actions.teleport.args.manager]
arg = 0
global = "teleport.telepo.instance"

[actions.teleport.args.aetheryte]
arg = 1
doc = "The aetheryte."
sheet = "Aetheryte"

[actions.teleport.args.shard]
arg = 2
doc = "The index of an aethernet shard within the aetheryte; 0 for the aetheryte itself."
default = 0
```

- An action is named like a domain. `doc` is not blank.
- `call`: a `function` entry with `thread = "main"`, of at most 16 arguments.
- `domain`: a domain with `act` wording; acting in it is the plugin's permission.
- `rate`: optional, 1 to 20, 2 by default: the most times a second one plugin takes the action.
- `ok`: optional, a number or a boolean: the return value that means the game took the action; any other is a refusal. Only for a function that returns an integer or a `bool`.
- `when`: optional, at most 16 conditions, each a `path` through the object model, as a query's path without braces, that must reach an object for the action to be taken, and what the plugin is told when it reaches none (`says`). Conditions read every domain: they are the host's checks, not the plugin's reads.
- `args`: every argument of the function, each once, by its position `arg` from 0. Its kind comes from the function's `abi`. It is one of:
  - a parameter the plugin gives, with `doc`: a number or a `bool`, never a pointer, with `sheet`, `enum`, `flags`, and `unit` as for a field, an optional `default` used when the plugin gives none, and an optional `range = [least, most]`;
  - `value`: always this number or boolean;
  - `global = "<entry>"`: a `global` entry of type `struct`, passed as its address, or `ptr`, passed as the pointer it holds; the kind is `ptr`;
  - `object = "<path>"`: the address of the first object a path through the object model reaches; the kind is `ptr`.

### Rules and resolution

The whole map is invalid when the object model names a domain, type, enum, entry, or field that does not exist; when a field's shape does not fit its entry's `type` or its entry's `of` is not the type that declares it; when a value's meaning does not fit its kind; when an array's `count`, `present`, or `partition` names a field of the wrong shape or a partition names an unknown value or domain; or when types contain each other through `extends`, embedded objects, or arrays in place, which would make an object infinitely large. It is invalid too when an event's `at` is not a function or a location, an argument's position is past the function's arguments, a function's argument names a register or a location's argument a position, or an argument's shape does not fit its kind; when an action's `call` is not a function of the main thread, its arguments do not cover the function's exactly once, its domain has no `act` wording, a parameter is a pointer, or a value, default, or range does not fit the argument's kind.

In a build, a root is absent when its global is absent, a field when its entry is absent, an event when its `at` is absent, and an action when its function or a global it passes is absent; everything else of the model stays usable. The object model is not part of `entries_sha256`; `angelica map verify` reports its roots, fields, events, and actions under `objects`, and an action whose condition or object path does not fit the model as absent.

## Window schemas

A window schema names elements of one of the game's own windows. Its path is `windows/` followed by the window's name as the game has it and `.toml`.

```toml
window = "_Exp"
doc = "The experience bar."
layout = "ui/uld/Exp.uld"

[elements.text]
kind = "text"
doc = "The class or job, its level, and the experience points, above the bar."

[[layouts]]
fingerprint = "f136eacebcde360e"
elements = { text = 4 }
```

- `window`: the window's name in the game: ASCII letters, digits, and underscores, at most 31 bytes, equal to the file's name without `.toml`.
- `doc`, and each element's `doc`: what the window or element shows, not how it was found; not blank.
- `layout`: the path of the window's layout file in the game's archives, `ui/uld/<name>.uld`. The game builds the window from it, and it gives every element its id.
- `elements`: at least one named element. A name is a lowercase letter followed by lowercase letters, digits, and underscores, at most 64 bytes. `kind` is `text` for an element that shows text and `other` for any other.
- `layouts`: at least one layout file the names were checked against. `fingerprint` is 16 lowercase hexadecimal digits: the first 8 bytes of the SHA-256 of the layout file's bytes; `elements` gives every named element its id in that file, each id above zero and once.

A name applies in a game build only when the build's layout file has the fingerprint of one of the schema's layouts. Otherwise every name of the window is absent in that build: a patch that changes the window's layout file in any way leaves its names absent until a layout for the new build is added. A name never means another element.

A schema that breaks a rule makes the whole map invalid. Window schemas are not part of `entries_sha256`.

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
