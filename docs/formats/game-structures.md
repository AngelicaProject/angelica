# Game structures the host changes

The host changes game data only through the adapters described here. Each one names the [map entries](./game-map-v2.md) it uses and checks their kind, type, and ABI; a missing entry makes the adapter unavailable, and the operation that needs it returns `unavailable` with the entry's name. Layout facts that the map does not describe are stated here, and the adapter checks them where it can. A fact that shows in the game's code is confirmed by a `contains` check on the entry whose code shows it: when an update changes the fact, the entry is absent and the adapter unavailable, instead of misreading the game's memory. Each section names those checks.

## Excel rows, version 3

The game builds a row with its row parser and immediately hands it to the sheet's row resolver through `StoreRow`; no other code sees the row before that call. The adapter detours both implementations and derives the map's `sheet_cell` events there: for rows that plugins' rules at `sheet_text` name, single rules or [text tables](./text-table-v1.md), it replaces the row buffer before calling the original.

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

- A sheet's column definitions are `column_count` records of 6 bytes, as the check on `excel.sheet.column_offset` confirms. The record's position is the column's index; a column of kind 0 is a text column. Text columns are matched against a table by index and offset, in index order.
- The variant is 0 for a sheet of rows and 1 for a sheet of subrows (the row parser adds a 2-byte subrow header per variant step to the fixed size); other values leave the sheet untouched. A subrow descriptor carries its subrow id as its single key: `subrow_count` must be 1.
- The language is the game's language number: 1 Japanese, 2 English, 3 German, 4 French, 5 and 6 Chinese, 7 Korean; 0 for a sheet without a language.
- The name is the NUL-terminated ASCII text `name_ptr` points to.
- Only sheets of version 3 are changed; every other version is left untouched. The game builds the row buffer of every version above 2 with its version 3 parser, and the map's check on `excel.row.construct` confirms that comparison. A row buffer is `data_offset` bytes of fields followed by one NUL-terminated string per text column, in column order, with nothing between them. A text field is 4 bytes: its low 24 bits are the distance from the field to its string, its high byte the game's hash of the string (`hash = byte ^ (hash << 1)` over its bytes). A buffer in any other layout is left untouched and counted as refused.

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

The game path is a `std::string`: 16 bytes holding the text when its capacity is below 16, otherwise a pointer to it, then the length and capacity as `u64`. Paths longer than 255 bytes are never replaced. The checks on `file.file_thread.do_job` confirm the mode byte at the descriptor's start and the cases below 9, 9, 10, and 11. The game's file object uses its first 0x22 bytes; the adapter's is 256 bytes and zeroed, so the reader opens the file into it. The detour stays installed until the process exits; without replacements it only passes jobs on, and a failure inside it leaves the job as the game made it.

A texture is the exception to the game's disk reader: the texture load takes a texture read from disk only from the game's protected file list, and refuses any other with 0xF1 (load state 13) before it sets the handle's flags, so the texture's glyphs or pixels are missing. The adapter detours the texture load. When the original refuses a disk job whose path is a file in the host's cache, it calls the game's loader for plain `.tex` files, which reads the header and the texture from the job's file, and returns its result. It passes the detail level the refused load chose: 0 unless flag bit 0 is set, then the settings' texture detail, 2 when that is 0xFF. Then it sets the flags the refused load left unset: bit 1 when the category is 1 or 2, and bit 2, 3, or 4 when the letter before the game path's extension is `d`, `n`, or `s`. Every other result, and every file outside the cache, is the original's. The checks on `file.texture_handle.load` confirm the detail level it chooses, the game path as a `std::string` at 0x48 of the resource handle with its capacity at 0x60, the refusal with 0xF1, which lies in a chunk of the function chained to its first range, and the flags it sets: bit 1 for the categories 1 and 2, and bits 2, 3, and 4 for the letter `d`, `n`, or `s` before the game path's extension. All entries of this section are needed; without one, file replacements are unavailable.

## Letter case

`Utf8String::ToUpper` and `ToLower` convert a string in place. The adapter detours both and derives the map's `case_letter` events there: it calls the original, then gives each letter beyond ASCII that the same rule converts the letter that plugins' rules at the event of that function (`letter_upper`, `letter_lower`) give it.

| Entry | Kind | Use |
| --- | --- | --- |
| `text.utf8_string.to_upper` | function `(ptr string, u8 first_only, u8 every_word, u8 normalize, ptr excluded) -> ptr` | detoured |
| `text.utf8_string.to_lower` | function `(ptr string) -> ptr` | detoured |
| `text.utf8_string.string_ptr` (`ptr`), `.used_size` (`i64`) | fields of the string | its bytes and their count with the terminator |
| `text.case_exclusion.word` (`ptr`), `.length` (`i32`) | fields of an exclusion record | a word the conversion leaves unchanged |

The exclusion list is an array of 16-byte records ended by one whose length is zero, as the checks on `text.utf8_string.to_upper` confirm; the adapter reads at most 256. A word is excluded at a position when the text there begins with it, as the game compares it. Strings longer than 1 MiB are left as the game converted them. Macros (`0x02` … `0x03`) are skipped whole; a malformed macro ends the conversion.

## Game windows

The game's windows are objects of the map's object model (`types/ui.toml`), read and changed through [Lens](../architecture/lens.md); no adapter of their own remains. The window manager the UI stage points to lists the loaded windows; a window's widget lists its elements, each a pointer to an element whose id the window's layout file gives. An element's visibility is one bit of its flags; the game's function sets it and marks the element for drawing again. A text element keeps its text as a game string and copies the text it is given.

| Entry | Kind | Use |
| --- | --- | --- |
| `ui.stage` | `ptr` global | the stage, zero before it exists |
| `ui.stage.unit_manager` (`ptr`) | field of the stage | the window manager |
| `ui.unit_manager.units` (`struct`), `.unit_count` (`i16`) | fields of the manager | the loaded windows, by pointer |
| `ui.unit_base.name` (`struct`) | field of a window | its name |
| `ui.unit_base.widget` (`ptr`) | field of a window | its widget |
| `ui.widget_info.nodes` (`ptr`), `.node_count` (`u32`) | fields of the widget | the window's elements, by pointer |
| `ui.res_node.id` (`u32`), `.type` (`u16`) | fields of an element | its id in the layout file, and 3 for a text element |
| `ui.res_node.flags` (`u32`) | field of an element | bit 20 is set while the element is visible |
| `ui.res_node.set_visible` | function `(ptr element, u8 visible)` | shows or hides an element |
| `ui.text_node.text` (`struct`) | field of a text element | its text, a game string |
| `ui.text_node.set_text` | function `(ptr element, ptr text)` | sets a text element's text from NUL-terminated bytes |
| `text.utf8_string.string_ptr` (`ptr`) | field of the string | the text's bytes |

The fields of the manager, the window, the widget, and the element are located in the code of the game's own lookups: `ui.unit_manager.get_unit_by_name`, which finds a window by its name, and `ui.unit_base.get_node_by_id` and `ui.unit_base.get_text_node_by_id`, which walk a window's elements for an id, the second only for elements of type 3. Element ids come from the game map's [window schemas](./game-map-v2.md#window-schemas), never from a plugin. Plugins change an element only through `set_visible` and `set_text`, on the game's main thread between frames, and the host applies those values again once per frame while a plugin holds them.

## System menu

The HUD agent's `OpenSystemMenu` builds the System menu from the values of the main menu bar's event and the number of entries: each entry is a `MainCommand` row in an int value, from value 5 on, and the text shown for it in the value 20 places later, from value 25 on, when that value holds text; otherwise the game names the row. The menu holds at most 20 entries. The adapter detours it: with fewer than 20 entries, it gives the original a copy of the values with Angelica's entry first — its command, `0x41470001`, which no `MainCommand` row has, and the text `U+E071 Angelica`, the game's boxed letter A — and the game's entries after it, and the number of entries plus one; the copy lives until the original returns, which copies the texts it keeps. With 20 entries, or when the copy cannot be made, the original runs with the game's values.

Choosing an entry runs the UI module's `ExecuteMainCommand` with its command. The adapter detours it: Angelica's command opens or closes the menu and does not reach the original; every other command goes to the original unchanged.

| Entry | Kind | Use |
| --- | --- | --- |
| `ui.agent_hud.open_system_menu` | function `(ptr agent, ptr values, u32 entries)` | detoured; its checks confirm that values are 16 bytes, the commands start at value 5 and the texts at value 25, and the limit of 20 entries |
| `ui.ui_module.execute_main_command` | slot 206 of the `ui.ui_module` vtable, `(ptr ui_module, u32 command)` | detoured |
| `ui.atk_value.kind` (`u8`), `.integer` (`i32`), `.text` (`ptr`) | fields of a value | an entry's command (kind 3, int) and its text (kind 8, text) |

Without one of these entries, the System menu has no Angelica entry, and the menu key and `/angelica` still open the menu. Both detours stay installed until the process exits; the command's is enabled first, so an entry the menu shows is always answered.

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

To print, the adapter constructs two game strings in zeroed 0x100-byte buffers (the constructor uses the first 0x62 bytes and allocates when the text does not fit its 64-byte inline buffer; the rest is a margin): an empty sender and the message. The checks on `text.utf8_string.from_sequence` confirm the inline buffer at 0x22 and its capacity of 0x40 bytes, and the check on `text.utf8_string.dtor` the flag that says the text is inline. It calls `PrintMessage(log, 56, sender, message, 0, 0)` — log kind 56 is the Echo channel, timestamp 0 means now — and then destroys both strings; the log keeps its own copy.

The message is the game's encoded text: `[<plugin name>]` in color row 500, a space, then each span. A tone other than `normal` sets the text color with the macro `0x48` (`UIForeground`) and the tone's row, and resets it with row 0 after the span; an italic span is wrapped in macro `0x1A` with 1 and 0; a line break is macro `0x10`. Rows are of the game's `UIColor` sheet, which holds a color for each interface theme, so the colors follow the player's theme:

| Use | `UIColor` row |
| --- | --- |
| plugin name | 500 |
| `accent` | 502 |
| `muted` | 3 |
| `good` | 504 |
| `warning` | 506 |
| `bad` | 508 |

An item link is the game's link macro around the item's name, as the game writes an item the player links:

| Part | Encoding |
| --- | --- |
| the color of the item's rarity | `0x48` with `UIColor` row `0x223 + 2 × rarity`, and `0x49` (`UIGlow`) with row `0x224 + 2 × rarity` |
| the link | macro `0x27` with the integers 2 (an item), the item's row (plus 1 000 000 for high quality), 1, and 0 |
| the arrow | `U+E0BB` in color row 500 and glow row 501, both reset to 0 after it |
| the name | the `name` cell of the item's `Item` row, then a space and `U+E03C` for high quality |
| the end of the link | macro `0x27` with the integers 206, 0, 0, 0 and an empty text: `02 27 07 CF 01 01 01 FF 01 03` |
| the colors restored | `0x49` and `0x48` with row 0 |

The game shows the item's tooltip when the player clicks the link, as for its own links.

The adapter does not detour `PrintMessage`: plugins see the messages the log receives through a Lens watch of the map's `chat_message_added` event at the same function. While the adapter prints, watches on its thread record nothing, so a plugin never hears its own message.

Without one of the command entries, `commands.define` returns `unavailable`; without one of the printing entries, `chat.print` and the host's answers to commands are unavailable. The detour stays installed until the process exits.

## Item menus

The menu of an item the player carries belongs to the game's item menu agent, which fills it with the entries the item allows and then opens it. The adapter detours the function that opens it and the agent's event function:

| Entry | Kind | Use |
| --- | --- | --- |
| `ui.agent_inventory_context.open_menu` | function `(ptr agent, bool, u8)` | detoured: the plugins' entries are added before the game opens the menu |
| `ui.agent_inventory_context.add_menu_item` | function `(ptr agent, u8 event, u32 addon_text, bool disabled, bool submenu)` | adds each entry as the game adds its own |
| `ui.agent_inventory_context.receive_event` | function `(ptr agent, ptr answer, ptr values, u32 count, u64 kind) -> ptr` | detoured: notes a chosen entry, then runs unchanged |
| `ui.agent_inventory_context.entry_start`, `.entry_count` (`i32`) | fields | the first entry's position among the events and values, and the number of entries |
| `ui.agent_inventory_context.events` (`u8`) | field | the first of the bytes that hold each position's event |
| `ui.agent_inventory_context.values` (`struct`) | field | the first of the 16-byte values that hold each position's text |
| `ui.agent_inventory_context.item` (`ptr`) | field | the inventory item whose menu it is |
| `inventory.inventory_item.item_id` (`u32`) | field | the item's row of `Item` |

When the game opens the menu, the adapter reads the number of entries. Unless one of them already holds an event of a plugin (0xF0 and up), as when the game opens the same menu again, it adds each plugin's entry with `add_menu_item`, event 0xF0 plus its position among the plugins' entries and Addon row 0x97A, while the menu has fewer than 32 entries, which the agent's masks hold. The function writes the entry's text as a value of kind 8, which the game never frees; the adapter then points the value at the label, the game's text in color row 500 and NUL-terminated, which it keeps until two menus later. A chosen entry reaches the event function with the kind 0x47 or 0x48 and the integer values 0 and the entry's position; the adapter reads the event at `entry_start` plus the position, and for an event of a plugin it notes the plugin's entry with the item's row, read from the agent's item. Then the game's function runs: the game's handler does nothing for an event it does not know, and closes the menu.
