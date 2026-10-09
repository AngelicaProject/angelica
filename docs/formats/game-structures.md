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

The game loads every resource file as a job on a file thread. A job's descriptor names its mode: 11 reads the resource's file from the archives by the game path in the resource handle; modes below 9 read a file on disk named by the descriptor's own path. The adapter detours the job function and, for an archive job whose resource is replaced, turns it into a disk job for the replacement in the host's cache before the original runs: it writes the cache path into the descriptor's path, sets its length and start offset to zero (an archive job leaves the archive request's values there, and the disk read takes `min(length, size − start offset)` bytes, all of the rest when the length is zero), gives the job a zeroed file object of its own for the duration of the call, and sets the mode to 0. The game's own disk reader then opens and reads the file into the resource, as it does for any file on disk.

| Entry | Kind | Use |
| --- | --- | --- |
| `file.file_thread.do_job` | function `(ptr thread, ptr descriptor, i32 priority, u8 sync) -> u8` | detoured |
| `file.descriptor.mode` (`u8`), `.length` (`u64`), `.start_offset` (`u64`), `.file_interface` (`ptr`), `.resource_handle` (`ptr`), `.path` (`u16`) | fields of a job descriptor | the job's mode, the range a disk read takes, its file object, resource, and the first of 260 UTF-16 path units |
| `file.resource_handle.file_name` | field of a resource handle | the resource's game path |
| `file.texture_handle.load` | function `(ptr handle, ptr descriptor, u8 reset) -> u8` | detoured |
| `file.texture_handle.load_unpacked` | function `(ptr handle, i32 detail, ptr descriptor, u8 reset) -> u8` | called for a refused replacement |
| `file.texture_handle.flags` (`u8`), `file.resource_handle.category` (`u16`) | fields of a texture handle | the texture's flags and the resource's category |
| `graphics.config.instance` (`ptr`), `graphics.config.texture_detail` (`u8`) | global and field | the graphics settings and their texture detail level |

The game path is a `std::string`: 16 bytes holding the text when its capacity is below 16, otherwise a pointer to it, then the length and capacity as `u64`. Paths longer than 255 bytes are never replaced. The game's file object uses its first 0x22 bytes; the adapter's is 256 bytes and zeroed, so the reader opens the file into it. The detour stays installed until the process exits; without replacements it only passes jobs on, and a failure inside it leaves the job as the game made it.

A texture is the exception to the game's disk reader: the texture load takes a texture read from disk only from the game's protected file list, and refuses any other with 0xF1 (load state 13) before it sets the handle's flags, so the texture's glyphs or pixels are missing. The adapter detours the texture load. When the original refuses a disk job whose path is a file in the host's cache, it calls the game's loader for plain `.tex` files, which reads the header and the texture from the job's file, and returns its result. It passes the detail level the refused load chose: 0 unless flag bit 0 is set, then the settings' texture detail, 2 when that is 0xFF. Then it sets the flags the refused load left unset: bit 1 when the category is 1 or 2, and bit 2, 3, or 4 when the letter before the game path's extension is `d`, `n`, or `s`. Every other result, and every file outside the cache, is the original's. All entries of this section are needed; without one, file replacements are unavailable.

## Letter case

`Utf8String::ToUpper` and `ToLower` convert a string in place. The adapter detours both, calls the original, then applies the plugins' letters to the same string with the same rule.

| Entry | Kind | Use |
| --- | --- | --- |
| `text.utf8_string.to_upper` | function `(ptr string, u8 first_only, u8 every_word, u8 normalize, ptr excluded) -> ptr` | detoured |
| `text.utf8_string.to_lower` | function `(ptr string) -> ptr` | detoured |
| `text.utf8_string.string_ptr` (`ptr`), `.used_size` (`i64`) | fields of the string | its bytes and their count with the terminator |
| `text.case_exclusion.word` (`ptr`), `.length` (`i32`) | fields of an exclusion record | a word the conversion leaves unchanged |

The exclusion list is an array of 16-byte records ended by one whose length is zero; the adapter reads at most 256. A word is excluded at a position when the text there begins with it, as the game compares it. Strings longer than 1 MiB are left as the game converted them. Macros (`0x02` … `0x03`) are skipped whole; a malformed macro ends the conversion.

## Chat

The game runs a line the player sends from the chat box or a macro with the command module's `ExecuteCommandInner`, which looks the first word up in its command list. The adapter detours it: when the line's first word is a plugin's command, the host takes the line and the original is not called; every other line, and every line while no plugin has commands, goes to the original unchanged. Plugin messages are added to the chat log with the log module's `PrintMessage`, on the game's main thread.

| Entry | Kind | Use |
| --- | --- | --- |
| `shell.command_module.execute_command_inner` | function `(ptr module, ptr line, ptr ui_module)` | detoured |
| `text.utf8_string.string_ptr` (`ptr`), `.used_size` (`i64`) | fields of the string | the line's bytes and their count with the terminator |
| `framework.instance` | `ptr` global | the framework, zero before it exists |
| `framework.get_ui_module` | function `(ptr framework) -> ptr` | the UI module |
| `ui.ui_module.get_log_module` | slot 11 of the `ui.ui_module` vtable, `(ptr ui_module) -> ptr` | the chat log module |
| `ui.log_module.print_message` | function `(ptr log, u16 kind, ptr sender, ptr message, i32 timestamp, u8 silent) -> u32` | adds a message |
| `text.utf8_string.from_sequence` | function `(ptr string, ptr bytes, u64 length) -> ptr` | constructs a game string |
| `text.utf8_string.dtor` | function `(ptr string)` | frees a game string |

The line is a `Utf8String`; the detour reads it only when its used size is 2 to 4096, otherwise the game handles it. A framework, UI module, or log module that is zero means the chat log does not exist yet, and printing returns `unavailable`.

To print, the adapter constructs two game strings in zeroed 0x80-byte buffers (the constructor uses the first 0x62 bytes and allocates when the text does not fit its 64-byte inline buffer): an empty sender and the message. It calls `PrintMessage(log, 56, sender, message, 0, 0)` — log kind 56 is the Echo channel, timestamp 0 means now — and then destroys both strings; the log keeps its own copy.

The message is the game's encoded text: `[<plugin name>]` in color row 500, a space, then each span. A tone other than `normal` sets the text color with the macro `0x48` (`UIForeground`) and the tone's row, and resets it with row 0 after the span; an italic span is wrapped in macro `0x1A` with 1 and 0; a line break is macro `0x10`. Rows are of the game's `UIColor` sheet, which holds a color for each interface theme, so the colors follow the player's theme:

| Use | `UIColor` row |
| --- | --- |
| plugin name | 500 |
| `accent` | 502 |
| `muted` | 3 |
| `good` | 504 |
| `warning` | 506 |
| `bad` | 508 |

The same `PrintMessage` is detoured to report the messages the log receives, once a plugin subscribes to chat messages: while any plugin is subscribed, the detour copies the log information, the sender, and the message — each a `Utf8String` read within its used size, at most 8192 bytes — into the host's event queue, then calls the original. A message the host prints for a plugin is marked on the printing thread and not reported. Without a subscriber the detour only passes messages on; it stays installed until the process exits, and a failure inside it lets the message through unchanged.

Without one of the command entries, `commands.define` returns `unavailable`; without one of the printing entries, `chat.print` and the host's answers to commands are unavailable. The detour stays installed until the process exits.
