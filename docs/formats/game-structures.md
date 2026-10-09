# Game structures the host changes

The host changes game data only through the adapters described here. Each one names the [map entries](./game-map-v1.md) it uses and checks their kind, type, and ABI; a missing entry makes the adapter unavailable, and the operation that needs it returns `unavailable` with the entry's name. Layout facts that the map does not describe are stated here, and the adapter checks them where it can.

## Excel rows, version 3

The game builds a row with its row parser and immediately hands it to the sheet's row resolver through `StoreRow`; no other code sees the row before that call. The adapter detours both implementations and, for rows of sheets a [text table](./text-table-v1.md) lists, replaces the row buffer before calling the original.

### Entries

| Entry | Kind | Use |
| --- | --- | --- |
| `excel.hash_table_page_row_resolver.store_row`, `excel.ring_buffer_page_row_resolver.store_row` | function `(ptr resolver, ptr descriptor, ptr row) -> u8` | the detoured functions |
| `excel.row.data`, `excel.row.sheet` | `ptr` fields of the row | the row buffer and the owning sheet |
| `excel.sheet.version` (`u16`), `.column_count` (`u16`), `.column_definitions` (`ptr`), `.data_offset` (`u32`), `.variant` (`u32`), `.language` (`u32`), `.name_ptr` (`ptr`) | fields of the sheet | the sheet's layout and identity |
| `excel.column_definition.kind`, `.offset` (`u16`) | fields of a column definition | the type and fixed-part offset of a column |
| `excel.row_descriptor.row_id` (`u32`), `.subrow_count` (`u16`), `.subrow_ids` (`u16`) | fields of the descriptor | the row's key |
| `excel.exd_environment.instance` | `ptr` global | the environment that owns row buffers |
| `excel.exd_environment.allocate`, `.free` | `ptr` fields of the environment's vtable | its allocate and free functions |

### Layout

- A sheet's column definitions are `column_count` records of 6 bytes. The record's position is the column's index; a column of kind 0 is a text column. Text columns are matched against a table by index and offset, in index order.
- The variant is 0 for a sheet of rows and 1 for a sheet of subrows (the row parser adds a 2-byte subrow header per variant step to the fixed size); other values leave the sheet untouched. A subrow descriptor carries its subrow id as its single key: `subrow_count` must be 1.
- The language is the game's language number: 1 Japanese, 2 English, 3 German, 4 French, 5 and 6 Chinese, 7 Korean; 0 for a sheet without a language.
- The name is the NUL-terminated ASCII text `name_ptr` points to.
- Sheets of version 2 and earlier are left untouched; later versions use the version 3 row parser. A row buffer is `data_offset` bytes of fields followed by one NUL-terminated string per text column, in column order, with nothing between them. A text field is 4 bytes: its low 24 bits are the distance from the field to its string, its high byte the game's hash of the string (`hash = byte ^ (hash << 1)` over its bytes). A buffer in any other layout is left untouched and counted as refused.

### Rewriting

For a row a table changes, the adapter builds the new buffer in the same layout: the fixed part unchanged, then every string — replaced or original — with its terminator, each field pointing at its string. A replaced string's field gets the game's hash of the new text; an untouched string keeps its hash byte, because the game hashes the resolved text of `_rsv_` strings, which the host cannot recompute. Rows stay below 1 MiB.

The new buffer is allocated with the environment's `allocate(environment, size, 0)` and the old one freed with `free(environment, old)` — the calls the game's row parser and row clear make — after the row's data field points at the new buffer. The game cannot tell a rewritten row from one it parsed itself, and every consumer, including code that reads the buffer directly, sees the replaced text.

The detours stay installed until the process exits; without active tables they only pass rows on. A failure inside the detour leaves the row as the game built it.

## Game file loads

The game loads every resource file as a job on a file thread. A job's descriptor names its mode: 11 reads the resource's file from the archives by the game path in the resource handle; modes below 9 read a file on disk named by the descriptor's own path. The adapter detours the job function and, for an archive job whose resource is replaced, turns it into a disk job for the replacement in the host's cache before the original runs: it writes the cache path into the descriptor's path, gives the job a zeroed file object of its own for the duration of the call, and sets the mode to 0. The game's own disk reader then opens and reads the file into the resource, as it does for any file on disk.

| Entry | Kind | Use |
| --- | --- | --- |
| `file.file_thread.do_job` | function `(ptr thread, ptr descriptor, i32 priority, u8 sync) -> u8` | detoured |
| `file.descriptor.mode` (`u8`), `.file_interface` (`ptr`), `.resource_handle` (`ptr`), `.path` (`u16`) | fields of a job descriptor | the job's mode, file object, resource, and the first of 260 UTF-16 path units |
| `file.resource_handle.file_name` | field of a resource handle | the resource's game path |

The game path is a `std::string`: 16 bytes holding the text when its capacity is below 16, otherwise a pointer to it, then the length and capacity as `u64`. Paths longer than 255 bytes are never replaced. The game's file object uses its first 0x22 bytes; the adapter's is 256 bytes and zeroed, so the reader opens the file into it. The detour stays installed until the process exits; without replacements it only passes jobs on, and a failure inside it leaves the job as the game made it.

## Letter case

`Utf8String::ToUpper` and `ToLower` convert a string in place. The adapter detours both, calls the original, then applies the plugins' letters to the same string with the same rule.

| Entry | Kind | Use |
| --- | --- | --- |
| `text.utf8_string.to_upper` | function `(ptr string, u8 first_only, u8 every_word, u8 normalize, ptr excluded) -> ptr` | detoured |
| `text.utf8_string.to_lower` | function `(ptr string) -> ptr` | detoured |
| `text.utf8_string.string_ptr` (`ptr`), `.used_size` (`i64`) | fields of the string | its bytes and their count with the terminator |
| `text.case_exclusion.word` (`ptr`), `.length` (`i32`) | fields of an exclusion record | a word the conversion leaves unchanged |

The exclusion list is an array of 16-byte records ended by one whose length is zero; the adapter reads at most 256. A word is excluded at a position when the text there begins with it, as the game compares it. Strings longer than 1 MiB are left as the game converted them. Macros (`0x02` … `0x03`) are skipped whole; a malformed macro ends the conversion.
