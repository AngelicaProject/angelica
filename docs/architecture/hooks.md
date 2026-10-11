# Hooks and code patches

`angelica-hook` installs every modification of game code. The host's adapters install hooks at verified game map entries only; plugins never install hooks or name addresses, they use the [host services](./services.md) the adapters implement.

## Kinds

| Kind | Map entry | What the handler gets |
| --- | --- | --- |
| Entry detour | function | the arguments; it may call the original and return a value |
| Mid-function hook | location inside a function | the register context; it may change registers before execution continues |
| Code patch | patch with expected original bytes | nothing; the bytes are replaced while the patch is active |
| Vtable slot | vtable and index | as an entry detour, for that object type only |

## Entry detours

The executable's entry point is never hooked: the host starts before it without a change to the executable's code ([loading](./loading.md)).

A detour overwrites the start of a function with a 5-byte `jmp rel32` to a relay, and fills the rest of the overwritten instructions with `int3`. The relay, an absolute `jmp qword ptr [rip]`, jumps to the replacement anywhere in the address space. The overwritten instructions move into a trampoline that ends with a jump back to the first instruction after them; calling the trampoline runs the original function. Relay and trampoline lie in executable memory within 2 GiB of the target.

### Relocation

Relocation decodes whole instructions with `iced-x86` until they cover 5 bytes and encodes them at the trampoline's address, keeping the targets of relative branches and RIP-relative operands. Installation fails, and the entry is reported as unhookable for the build rather than hooked incorrectly, when:

- an instruction is invalid or cut off;
- the function leaves unconditionally (a return, a jump, an interrupt) before 5 bytes;
- a RIP-relative operand cannot reach its target from the trampoline;
- a relative branch anywhere in the function targets the overwritten bytes other than the function's start. The function's code is known from its map entry (see [`game-map.md`](./game-map.md)).

### Installing and removing

While the bytes change, every other thread of the process is suspended. A suspended thread whose instruction pointer is on an overwritten instruction continues at the same instruction in the trampoline; when a detour is removed, a thread on a moved instruction in the trampoline continues at the original. Nothing allocates memory while threads are suspended, since a suspended thread may hold the heap lock. Only one thread at a time suspends the others: Windows suspends a thread asynchronously, so two threads doing it at once could each suspend the other and both stay suspended.

A detour is enabled only while the target still holds the bytes it was prepared from, and removed only while the target still holds its own jump. Trampolines and relays are never freed, because a thread may still be executing in one after its detour is removed.

## Mid-function hooks

A mid-function hook overwrites the instructions at a location with a `jmp rel32` to a stub near it, relocated as for a detour. The stub:

1. pushes the general-purpose registers and the flags and stores `xmm0` to `xmm15`, forming the context;
2. calls the handler with the context's address on a 16-byte aligned stack with 32 bytes of shadow space;
3. loads every register and the flags back from the context, so the handler's changes take effect;
4. runs the moved instructions and jumps back.

The stack pointer is not part of what a handler can change: it is the address just past the context. A handler must not unwind; the host catches panics inside it.

When the hook is installed, a suspended thread on the hooked instruction continues at the start of the stub and runs the handler; a thread on a later overwritten instruction continues at that instruction among the moved ones.

## Code patches

A code patch is prepared only when the bytes at the address equal the expected original bytes in the map, so a patch never writes over code it was not made for. Applying or reverting it suspends the other threads and fails with a retryable error when a thread is stopped inside the bytes.

## Vtable slots

A vtable slot is one aligned pointer, replaced with a single store; no thread is suspended. The slot is restored only while it still holds the replacement.

## Chaining

The host installs at most one hook per function. When several plugins' rules apply to the same calls, the adapter combines them in its own defined order (for text tables, by priority).

Other tools in the process may hook the same function. A detour installed over another tool's jump moves that jump into its trampoline, so calling the original runs the other tool's hook. A tool that installs its hook over Angelica's does the same, and Angelica's detour is then removed only after that tool removes its own; until then it stays installed.

## Hooks and plugins

No hook calls plugin code. Functions the game calls often — building rows, converting text — are handled natively by adapters from rules plugins activated; see [`services.md`](./services.md). An adapter that reports what the game did — a line the player sent, a message, a window that opened — records it in a bounded queue, and the host delivers it to plugins in the next frame. A plugin changes what the game does only through rules or host operations, never from inside a hook. An adapter's detour catches panics and passes the call on unchanged when it fails.
