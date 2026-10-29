# Anatomia format 1

Anatomia is the description of the game: what the game contains and how to find each part in a game build. It describes the game only. What Lens offers over it (roots, events, actions, and fields plugins change) is in the [Lens catalog](./lens-catalog-1.md), and what the data means for the player's consent is in its [meaning](#meaning) files.

A description is a directory of UTF-8 TOML files. `angelica-anatomia` reads it and resolves its entries against the game's executable; `angelica anatomia verify` does so offline, and `angelica anatomia convert` writes a description from a [game map v2](./game-map-v2.md).

```text
anatomia.toml            format version, namespace, extensions
types/<area>.toml        structures with their fields, unions, enums, aliases, templates
functions/<area>.toml    functions and locations inside functions
globals/<area>.toml      static objects, pointers to them, and virtual tables
meaning/<area>.toml      domains, and the domain of each structure and field
sheets/<sheet>.toml      names for the columns of the game's Excel sheets
windows/<window>.toml    names for the elements of the game's own windows
lens/<file>.toml         the Lens catalog, which is not part of the description
builds/<version>.toml    generated build records
```

## Failures are local

An entry is something a build gives a value: a function, a location, a global, a virtual table, or a field of a structure. An entry that cannot be read, refers to what does not exist, or does not resolve is absent with its reason; the rest of the description is unaffected. A type that cannot be read makes its fields absent.

Absent in every build, with the reason, is:

- an entry with a key this reader does not know, such as one of a newer minor version, or a key of the wrong kind of value;
- a step or check kind this reader does not know;
- an entry, type, or field whose name is not valid, is not of its file's area, or is defined more than once (every definition of it);
- an entry that refers to a name that is not an entry of the right kind, or names a type that is not defined;
- an entry that refers to itself through other entries;
- an entry with extension attributes of an extension `anatomia.toml` does not declare;
- a field whose meaning cannot be read.

An entry is also absent in a build when an entry it refers to is absent there. Only these make the whole description unreadable: a file that is not TOML, a file where no file belongs, and an `anatomia.toml` that cannot be read or is of another major version.

## anatomia.toml

```toml
format = "1.0"
namespace = "ffxiv"
extensions = ["angelica.origin", "angelica.notes"]
```

- `format`: `"<major>.<minor>"`. A reader reads every minor version of its major version and refuses another major version. In a newer minor version than the reader knows, keys of `anatomia.toml` it does not know are ignored; in entries they make the entry absent.
- `namespace`: the namespace of the description's names, `ffxiv` for the description of the game.
- `extensions`: the extension attribute sets entries may use, each named `<owner>.<name>`.

## Names

An entry or type name is lowercase dotted words, `<area>.<subject>[.<member>]`: each word starts with a lowercase letter and holds lowercase letters, digits, and underscores, and a name has at least two words. The area is the name of the file the name is defined in. A field is named `<structure>.<field>`. Types are a namespace of their own; functions, locations, globals, virtual tables, fields, and aliases of fields share one.

A name is stable. A field that is renamed keeps its old name in `aliases`: a field name of the same structure, or a whole entry name for a field that was once named apart from its structure. Steps, checks, `depends`, and the Lens catalog may name a field by an alias.

## Type expressions

| Expression | Means |
| --- | --- |
| `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `bool` | a value |
| `char8`, `char16` | a code unit of UTF-8 or UTF-16 |
| `ptr<T>`, `ptr` | a pointer to a `T`, or to memory of no described type |
| `T[n]` | `n` elements of `T` in place, `n` at least 1 |
| `fn(<params>)`, `fn(<params>) -> <type>` | a function of the Windows x64 calling convention; a parameter is `<type>` or `<name>: <type>`; without `->`, or with `-> void`, it returns nothing |
| `<name>`, `<name>(<types>)` | a structure, union, enum, or alias; a template with its arguments |
| `opaque` | an object of a type the description does not give, in place: only its address is known |

`ptr<T>[n]` is `n` pointers in place; `ptr<T[n]>` is a pointer to `n` elements.

## Structures

```toml
[struct."ui.text_node"]
doc = "An element of a window that shows text."
extends = [{ type = "ui.res_node" }]
variant = { field = "kind", values = ["text"] }

[struct."ui.text_node".fields.text]
doc = "The text the element shows, its macros encoded."
type = "text.utf8_string"
at = 0xD0
text = 4096
```

A structure has:

- `doc`: what it is.
- `size`: its size in bytes, which is the stride of an array of it. Like a constant offset, it is a number of the description; the checks of an array field confirm it ([arrays](#arrays)).
- `align`: its alignment.
- `extends`: its bases, each `{ type = "<structure>", at = <offset> }`, `at` 0 by default. An object of the structure has the fields of its bases at those offsets.
- `vtable`: the virtual table its objects point to.
- `variant`: for a structure with a base, `{ field = "<field of the base>", values = [ … ] }`: the values of an enum field of the base, by name or number, that make an object of the base one of this structure.
- `text`: one of its fields, through which an object of the structure reads as text.
- `fields`: its fields.
- `ext`: extension attributes.

### Fields

A field has a `type` and is in one of three places:

- `at = <offset>`: a constant offset of the description. It is present only in a build whose [build record](#build-records) confirms it, because nothing in the game's code says when an update moves it.
- `find = [<steps>]`: a [pipeline](#finding) that ends at the offset, usually the displacement of an instruction of a function that reads the field.
- `bits = { in = "<field>", at = <bit>, width = <bits> }`: bits of a located integer field of the same structure, `width` 1 by default; the field's type is an integer or `bool`. A field of bits has no entry of its own: it is present when the field it is part of is present.

A located field may have `checks` and `depends` as other entries do. Every field may have:

- `doc`: what it holds.
- `text`: how it reads as text, NUL-terminated UTF-8. `true` on a `char8` array: at most its length. A number of bytes, 1 to 65535, on a pointer (the text is where it points; a zero pointer is no text) or on a structure that has `text` (the text is what its text field gives).
- `count = "<field>"`: on an array, an integer field of the same structure that says how many elements are used, at most the array's length.
- `present = "<field>"`: on an array, a field of the elements that is zero, false, or empty in an element that is not used.
- `sheet = "<sheet>"`, `enum = "<enum>"`, `flags = "<enum>"`, `element = "<structure>.<field>"`: what an integer value is: a row of the game's sheet, a value of the enum, a set of the flags enum's bits, or the id of an element of one of the game's windows, named by the [window schema](./game-map-v2.md#window-schemas) of the window whose name that text field holds.
- `unit`: the unit of a number, such as `yalms` or `s`.
- `aliases`: the field's old names ([names](#names)).
- `ext`: extension attributes.

### Arrays

An array is a field of type `T[n]` in place, or `ptr<T[n]>` behind a pointer, with `count` when only some elements are used. The stride of an array of a structure is the structure's `size`; of pointers, 8. A length and a size are numbers of the description, and an update of the game can change them. The array field's entry confirms them for every build: when its offset is found in code, its `contains` checks hold code of the game that walks or copies the array with those numbers; when its offset is a constant, the build record confirms it only after its elements were checked in the game.

## Unions, enums, aliases, and templates

```toml
[union."ui.atk_value_payload"]
fields = { int = "i32", uint = "u32", float = "f32", string = "ptr<char8>" }

[enum."ui.node_type"]
doc = "What an element of a window is, as the game numbers its node types."
values = { text = 3 }

[alias."ui.node_id"]
type = "u32"

[template."std.vector"]
params = ["T"]
size = 24
fields = { first = { type = "ptr<T>", at = 0 }, last = { type = "ptr<T>", at = 8 }, end = { type = "ptr<T>", at = 16 } }
range = { from = "first", to = "last" }
```

- A union's `fields` are its members, all at offset 0.
- An enum has at least one value, each a non-negative integer; `type` is the integer it is stored as, when the description gives it; with `flags = true`, every value is a single bit and a field of it holds a set of them.
- A template has at least one parameter, which its fields' types use by name; `range` names the two pointer fields that bound its elements.

Each may have `doc`.

## Functions and locations

```toml
[function."text.utf8_string.to_upper"]
doc = "Converts a string to upper case: its first letter, the first letter of every word, or all of it, skipping listed words."
type = "fn(ptr, u8, u8, u8, ptr) -> ptr"
thread = "any"
find = [{ scan = "40 53 57 41 54 41 55 41 57 48 83 EC 20 45 0F B6 E9 44 0F B6 E2 48 8B F9 41 B2 01" }]
checks = [{ contains = "41 8B 46 08 33 DB 85 C0 74 ?? 49 8B CE" }]
```

- A function has a `type` of the form `fn(…)`, a `thread`, `main` or `any`, and `find`; optionally `member_of`, the structure whose method it is.
- A location is an instruction inside a function, for a hook in the middle of it: `thread`, `find`, and optionally `registers`, `{ <register> = "<type>" }`, the registers a handler may read there.

The address of a function or location lies in a code section.

## Globals and virtual tables

```toml
[global."inventory.inventory_manager.instance"]
doc = "The inventory manager: every container of the player's items."
type = "inventory.inventory_manager"
find = [{ scan = "48 8D 0D ?? ?? ?? ?? 81 C2" }, { rel32 = 3 }]

[vtable."inventory.inventory_item"]
doc = "An inventory item's virtual functions."
code_slots = 25
find = [{ scan = "66 89 51 0C 48 8D 05" }, { rel32 = 7 }]
```

- A global has a `type`: the type of the object at its address, or `ptr<T>` for a pointer the game keeps there.
- A virtual table has `code_slots`, how many slots from the first all point into code, and `slots`, names for slots: `<name> = <slot>`, or `<name> = { slot = <slot>, type = "<fn type>" }`. It needs at least one of the two. Every slot of the first `code_slots` points into code, and every named slot holds an address in the image.

Every entry may have `doc`, `checks`, `depends` (names of entries that must be present), and `ext`.

## Finding

`find` is a list of steps from an empty cursor to the entry's value. The cursor is an address (an RVA) or a number. A field's pipeline ends at a number, its offset, from 0 to 2³²−1; every other entry's ends at an address.

| Step | From → to |
| --- | --- |
| `{ scan = "<bytes>" }` | the single match in the code sections; `??` is any byte |
| `{ scan = "<bytes>", section = "data" }` | the single match in the sections that are not code |
| `{ in = "<function>" }` | the scan right after it searches only that function's code |
| `{ offset = n }` | cursor + n; an address stays in the image |
| `{ rel32 = n }`, `{ rel8 = n }` | the target of the 32-bit or 8-bit relative operand at cursor + n: the address after the operand plus its signed value |
| `{ operand = "disp" }` | the displacement of the memory operand of the instruction at the cursor, which has a base register other than RIP and a displacement that is not negative: a number |
| `{ operand = "imm" }` | the immediate of the instruction at the cursor: a number |
| `{ operand = "target" }` | the address the instruction at the cursor branches to, or its RIP-relative memory operand reaches |
| `{ deref = "ptr" }` | the address stored at the cursor, which is in a section that is not code |
| `{ vtable = "<vtable>", slot = n \| "<name>" }` | the address in that slot of the table; a slot beyond `code_slots` is one the table names |
| `{ entry = "<entry>" }` | the value of another entry |
| `{ import = "<module>!<name>" }` | the single import address table slot through which the executable calls that function |

An `operand` step may take `index = k`, the instruction's operand at position `k`; without it, the step takes the first operand of its kind.

A function's code is every range of it in the executable's exception directory, chained chunks included; a match never spans two ranges. A function without an entry there is a leaf function, whose code is what its control flow reaches, as the [game map's `in` locator](./game-map-v2.md#locators) defines it.

A step makes the entry absent, with its index and reason, when it finds no candidate or several, when the cursor is not what it needs, or when what it reaches lies outside the image or the section it requires. `in` must be followed by `scan`.

## Checks

`checks` are conditions after `find`, evaluated in order; each failed check makes the entry absent.

| Check | Holds when |
| --- | --- |
| `{ bytes = "<bytes>", at = n }` | the bytes at the value + n are these; `at` is 0 by default |
| `{ contains = "<bytes>" }`, `{ contains = "<bytes>", in = "<function>" }` | the code of the entry's function, or of the `in` function, matches exactly once; an entry that is not a function names `in` |
| `{ find = [<steps>] }` | the pipeline resolves to the entry's value |
| `{ equals = "<entry>" }` | that entry has the entry's value |
| `{ vtable_slot = n, code_slots = k }` | the value is slot `n` of an 8-byte aligned table outside code whose first `k` slots, `k > n`, all point into code |
| `{ operand = "imm", at = n, equals = v }` | the instruction at the value + n has the immediate `v` |

A `contains` check confirms a fact of the game's code a description relies on, such as a structure's size or a constant the game compares with.

## Meaning

`meaning/<area>.toml` says what the data means for the player's consent, apart from where it is.

```toml
[domains.inventory]
doc = "The items the player carries."
class = "you"
read = { en = "See the items you carry", ru = "Видеть предметы, которые вы носите" }

[types."inventory.inventory_container"]
domain = "inventory"

[fields."inventory.inventory_manager.containers"]
partition = { field = "kind", domains = { armoury = ["armoury_head"] } }
```

- `domains`: units of consent, in any file of `meaning/`. A domain is named by one word and has `doc`; `class`, one of `you`, `display`, `others`, `acts`, `acts_on_others`; `read`, and optionally `change` and `act`, in English (`en`) and Russian (`ru`), none blank; and `act_local`, only with `act`. What each means is the [game map's domains](./game-map-v2.md#domains).
- `types`: the `domain` of a structure's fields.
- `fields`, by a field's name: `domain`, when it is not its structure's; `forbidden = true`, for a field nothing ever reads or writes; `whose`, `you` or `others`, and `as`, domains renamed below the field ([whose objects](./game-map-v2.md#whose-objects)); `partition`, which domains an array's elements belong to by the value of one of their enum fields.
- `functions`, by a function's name: `server = true` for a function that leads to a request to the game's servers.

The keys of `types`, `fields`, and `functions` are of the file's area. A domain must be defined. Meaning that cannot be read makes the field, the structure, or the function it is about unusable.

## Extensions

Every entry, structure, and field may carry `ext = { "<extension>" = { … } }`, attributes of extensions that `anatomia.toml` declares. The resolution ignores them; tools read them. A conversion from game map v2 uses two:

- `angelica.origin`: `{ source, commit }`, where an entry came from, as the license of its source requires; the description then has `THIRD_PARTY_NOTICES.md`.
- `angelica.notes`: `{ entry_doc }`, the description a field's game map entry had, where the object model's description became the field's `doc`.

## Sheet and window schemas

`sheets/` and `windows/` hold [sheet schemas](./game-map-v2.md#sheet-schemas) and [window schemas](./game-map-v2.md#window-schemas), in the same files as the game map. A schema that cannot be read is reported as unknown.

## Build records

`builds/<game version>.toml`, generated by `angelica anatomia verify --write`, never edited by hand:

```toml
# Generated by `angelica anatomia verify`. Do not edit.
game_version = "2026.10.16.0000.0000"
exe_sha256 = "…"
anatomia_sha256 = "…"

[present]
"text.utf8_string.to_upper" = 0x8A140
"text.utf8_string.used_size" = 0x10

[absent]
"ui.atk_value.unsigned" = "a constant offset that no build record confirms for this executable"

[confirmed]
offsets = []
```

- `game_version`: the content of the game's `ffxivgame.ver`; `exe_sha256`: the SHA-256 of `ffxiv_dx11.exe`. A record applies only to the executable with this hash.
- `anatomia_sha256`: the SHA-256 over `anatomia.toml` and then every file of `types/`, `functions/`, and `globals/` in path order, each as its path relative to the description with `/` separators, a NUL byte, its length as a little-endian 64-bit integer, and its bytes.
- `present`: the value of every present entry, an RVA or a field's offset; `absent`: the reason for every absent one. Every entry, those that cannot be read included, is in exactly one.
- `confirmed.offsets`: the fields with a constant offset that were checked in the game for this executable; they are present with that offset.

The same description and executable always give the same text.

## A plugin's own entries

A plugin carries its own description in `anatomia/` beside its `angelica.toml`, in the layout above, with `namespace` set to its plugin id. `angelica dev install` puts the folder in the plugin's package, which the installation's file set authenticates, and refuses entries the host would refuse: an `anatomia/` without `anatomia.toml`, or another namespace. The host reads the entries when it loads the package and resolves them against the game's executable when the plugin starts, for a plugin that holds the `unreviewed` permission; an entry absent there is logged. A field at a constant offset is present without a build record: the plugin's entries are its author's word. Types and `find` steps of the plugin's entries refer to its own entries and types only. A plugin [intercepts](./plugin-api-v1.md#intercepts) the functions they describe and [reads and writes](./plugin-api-v1.md#direct-access-to-the-games-objects) the objects they describe; when it names an entry or field, its own entries come before the central description's.

## Conversion from game map v2

`angelica anatomia convert` reads a valid game map v2 and writes a description and its Lens catalog. Converting the result back gives the same map as the v2 reader reads it. TOML comments are not carried over. The host converts the game map it loads the same way, in memory, and gives each converted entry the value its v2 entry resolved to; plugins read the game directly through that description. `angelica anatomia views` writes the Rust views of a description, or of a map it converts first.

| Game map v2 | Anatomia 1 |
| --- | --- |
| `function`, with `abi` | `function`, with `type = "fn(…)"` |
| `location`, `global`, `vtable` (`slots`) | `location`, `global` (its root's type, else `ptr` or `opaque`), `vtable` (`code_slots`) |
| `field`, with `of` | a field of the structure `of`, named by the object model's field that reads it, else by the entry's last word; another v2 name stays as an alias |
| `patch` | no place: the conversion stops |
| `{ signature, at, follow }` | `[{ scan }, { offset }]` or `[{ scan }, { rel32 }]` |
| `{ vtable, slot }` | `[{ vtable, slot }]` |
| `{ in, signature, operand = "disp" }` | `[{ in }, { scan }, { operand = "disp" }]` |
| a locator check, `equals`, `vtable_slot`, `contains` | `{ find }`, `equals`, `vtable_slot`, `contains` |
| `origin` | `ext."angelica.origin"` |
| a type's `doc`, `extends`, `variant` | the structure's |
| a field's `text`, `ref`, `embed`, `array` | `char8[n]` with `text = true`, `ptr<char8>` or a structure with `text`, `ptr<T>`, `T`, `T[n]` or `ptr<T[n]>`; an array's stride becomes its element's `size` |
| a field's `bit` | `bits` of the field of the entry it reads |
| `sheet`, `enum`, `flags`, `element`, `unit`, `present`, `count` | the field's |
| `domain`, `forbidden`, `whose`, `as`, `partition`, `domains.toml` | `meaning/` |
| `enums/` | `enum` in `types/` |
| `roots.toml`, `events.toml`, `actions.toml`, a field's `set`, which types and fields the object model has | the [Lens catalog](./lens-catalog-1.md) |
| `sheets/`, `windows/` | the same files |
| `builds/` | records under the new names, with `confirmed.offsets` from `constant_offsets` |

## Coverage of the structures library

The community's structures library for the game (FFXIVClientStructs) describes the game in C# attributes. Each of its constructs has a form here:

| Structures library | Anatomia |
| --- | --- |
| `[StructLayout(Size = n)]` struct | `struct` with `size` |
| `[FieldOffset(n)]` field | field with `at = n` |
| inheritance, `[Inherits<T>]` | `extends` with offsets |
| `[FixedSizeArray]`, `fixed T x[n]` | `T[n]` |
| `StdVector<T>`, `StdMap<K, V>`, `StdString`, `Utf8String` | templates and structures |
| `[MemberFunction("sig")]` | `function` with `member_of` and `find = [{ scan }]` |
| `[VirtualFunction(n)]` | `function` with `find = [{ vtable, slot }]` |
| `[StaticAddress("sig", offset, isPointer)]` | `global` with `find`, of type `ptr<T>` or `T` |
| `[VTableAddress]` | `vtable` with `find` |
| enums, `[Flags]` | `enum`, with `flags` |
| bit properties over a byte | `bits` |
| overlapping `[FieldOffset(0)]` fields | `union` |
| function pointer fields | a field of type `fn(…)` |
