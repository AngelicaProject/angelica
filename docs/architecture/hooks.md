# Hooks and code patches

`angelica-hook` installs every modification of game code. Plugins and host services request modifications by game map entry; nothing can be installed at an address that is not a verified entry.

## Kinds

| Kind | Map entry | What the handler gets |
| --- | --- | --- |
| Entry detour | function | the arguments; it may call the original and return a value |
| Mid-function hook | location inside a function | the register context; it may change registers before execution continues |
| Code patch | patch with expected original bytes | nothing; the bytes are replaced while the patch is active |
| Vtable slot | vtable and index | as an entry detour, for that object type only |

## Entry detours

A detour overwrites the start of a function with a 5-byte `jmp rel32` to a relay, and fills the rest of the overwritten instructions with `int3`. The relay, an absolute `jmp qword ptr [rip]`, jumps to the replacement anywhere in the address space. The overwritten instructions move into a trampoline that ends with a jump back to the first instruction after them; calling the trampoline runs the original function. Relay and trampoline lie in executable memory within 2 GiB of the target.

### Relocation

Relocation decodes whole instructions with `iced-x86` until they cover 5 bytes and encodes them at the trampoline's address, keeping the targets of relative branches and RIP-relative operands. Installation fails, and the entry is reported as unhookable for the build rather than hooked incorrectly, when:

- an instruction is invalid or cut off;
- the function leaves unconditionally (a return, a jump, an interrupt) before 5 bytes;
- a RIP-relative operand cannot reach its target from the trampoline;
- a relative branch anywhere in the function targets the overwritten bytes other than the function's start. The function's code is known from its map entry (see [`game-map.md`](./game-map.md)).

### Installing and removing

While the bytes change, every other thread of the process is suspended. A suspended thread whose instruction pointer is on an overwritten instruction continues at the same instruction in the trampoline; when a detour is removed, a thread on a moved instruction in the trampoline continues at the original. Nothing allocates memory while threads are suspended, since a suspended thread may hold the heap lock.

A detour is enabled only while the target still holds the bytes it was prepared from, and removed only while the target still holds its own jump. Trampolines and relays are never freed, because a thread may still be executing in one after its detour is removed.

## Code patches

A code patch is prepared only when the bytes at the address equal the expected original bytes in the map, so a patch never writes over code it was not made for. Applying or reverting it suspends the other threads and fails with a retryable error when a thread is stopped inside the bytes.

## Vtable slots

A vtable slot is one aligned pointer, replaced with a single store; no thread is suspended. The slot is restored only while it still holds the replacement.

## Chaining

Several plugins may hook the same function. The host installs one native hook per function and calls the handlers in priority order, each one able to call the next; the last calls the original.

Other tools in the process may hook the same function. A detour installed over another tool's jump moves that jump into its trampoline, so calling the original runs the other tool's hook. A tool that installs its hook over Angelica's does the same, and Angelica's detour is then removed only after that tool removes its own; until then it stays installed.

## Calling plugin code from a hook

A native hook calls a plugin handler through the plugin runtime. The handler runs on the instance bound to the thread kind declared by the map entry (see [`plugins.md`](./plugins.md#instances-and-threads)). If the handler traps, exceeds its time budget, or its plugin is disabled, the hook calls the next handler or the original as if the handler were absent.

Hot functions are not given to plugin handlers. A host service handles them natively from rules plugins configure; see [`services.md`](./services.md).
