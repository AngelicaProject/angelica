# Text table v1

A text table holds replacement text for cells of the game's Excel sheets, each tied to the original text it replaces by a guard. A plugin writes one to its storage and activates it with [`excel-text.activate`](./plugin-api-v1.md#text-tables). [`angelica-text`](../../crates/angelica-text) reads and writes the format; the SDK re-exports its `Writer`.

All integers are little-endian. A file is the header followed by five sections with nothing between or after them:

```text
header        128 bytes
sheets        sheetCount × 32 bytes
columns       columnCount × 4 bytes
cells         cellCount × 24 bytes
names         namesLength bytes, UTF-8 sheet names without separators
strings       stringsLength bytes, cell texts without terminators
```

## Header

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `u8[8]` | magic ASCII `ANGTEXT1` |
| 8 | `u32` | version, `1` |
| 12 | `u32` | sheetCount |
| 16 | `u64` | columnCount |
| 24 | `u64` | cellCount |
| 32 | `u64` | namesLength |
| 40 | `u64` | stringsLength |
| 48 | `u8` | language: the game language of the sheets (0–8), or `0xFF` for any |
| 49 | `u8` | domainLength, at most 64 |
| 50 | `u8[14]` | reserved, zero |
| 64 | `u8[64]` | guard domain: domainLength bytes, then zero |

## Sheets

32-byte records, sorted by name in bytewise order, names unique:

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `u32` | nameOffset into names |
| 4 | `u32` | nameLength, 1–256 |
| 8 | `u8` | variant: `0` rows, `1` subrows |
| 9 | `u8[3]` | reserved, zero |
| 12 | `u32` | columnStart, index of the sheet's first column record |
| 16 | `u32` | columnCount, 1–4096 |
| 20 | `u64` | cellStart, index of the sheet's first cell record |
| 28 | `u32` | cellCount, at least 1 |

Column and cell ranges are contiguous in sheet order and cover their sections exactly.

## Columns

4-byte records: every text column of the sheet, sorted by index, unique.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `u16` | index: the column's position among all of the sheet's columns |
| 2 | `u16` | offset: the byte offset of the column's field in a row |

These are the sheet's whole text layout, not only the columns with cells: a sheet whose text columns differ in the running game does not match the table.

## Cells

24-byte records, sorted by `(row, subrow, column)` within a sheet, unique:

| Offset | Type | Field |
| --- | --- | --- |
| 0 | `u32` | row |
| 4 | `u16` | subrow, `0` in a sheet without subrows |
| 6 | `u16` | column: the index of one of the sheet's text columns |
| 8 | `u64` | guard of the original text |
| 16 | `u32` | textOffset into strings |
| 20 | `u32` | textLength |

A cell's text lies inside strings. Cells may share text. Every text is empty or a well-formed game string — UTF-8 text runs and macros by the structural rule `angelica-text::sestring` implements, at most 65535 bytes, without NUL — and does not begin with `_rsv_`: the game resolves and hashes reserved string tokens itself, so they are never written.

## Guard

The guard of an original string is the first 8 bytes, read as a little-endian `u64`, of

```text
SHA-256(domain || u32le(length) || original)
```

where `original` is the string's bytes in the game's row, without its terminator. The domain lets a format keep guards it already has; the Harmonia pack format, for example, uses `HARMONIA-HXS-V1-RAW-STRING`.

## Validation

The host refuses the whole file when any rule above fails: wrong magic or version, nonzero reserved bytes, a language other than 0–8 or `0xFF`, sections that do not end exactly at the end of the file, unsorted or repeated sheets, columns, or cells, ranges outside their sections, a cell naming a column that is not one of its sheet's text columns, a subrow in a sheet without subrows, or malformed text.
