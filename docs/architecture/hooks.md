# Hooks and code patches

`angelica-hook` installs every modification of game code. Plugins and host services request modifications by game map entry; nothing can be installed at an address that is not a verified entry.

## Kinds

| Kind | Map entry | What the handler gets |
| --- | --- | --- |
| Entry detour | function | the arguments; it may call the original and return a value |
| Mid-function hook | location inside a function | the register context; it may change registers before execution continues |
| Code patch | patch with expected original bytes | nothing; the bytes are replaced while the patch is active |
| Vtable slot | vtable and index | as an entry detour, for that object type only |

A code patch is applied only when the bytes at the address equal the expected original bytes in the map, so a patch never writes over code it was not made for.

## Trampolines

An entry detour or mid-function hook relocates the instructions it overwrites into a trampoline. Relocation decodes instructions with `iced-x86` and rewrites RIP-relative operands and relative branches. An instruction that cannot be relocated safely (a branch into the overwritten range, an unsupported encoding) makes installation fail, and the entry is reported as unhookable for this build rather than being hooked incorrectly.

## Chaining

Several plugins may hook the same function. The host installs one native hook per function and calls the handlers in priority order, each one able to call the next; the last calls the original. Other tools in the process may already have hooked the function, in which case the overwritten instructions are their jump, and relocation follows it as any other branch.

## Calling plugin code from a hook

A native hook calls a plugin handler through the plugin runtime. The handler runs on the instance bound to the thread kind declared by the map entry (see [`plugins.md`](./plugins.md#instances-and-threads)). If the handler traps, exceeds its time budget, or its plugin is disabled, the hook calls the next handler or the original as if the handler were absent.

Hot functions are not given to plugin handlers. A host service handles them natively from rules plugins configure; see [`services.md`](./services.md).
