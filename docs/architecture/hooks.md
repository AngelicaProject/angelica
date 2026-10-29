# Hooks and code patches

`angelica-hook` installs every modification of game code. The host's adapters install hooks at verified game map entries only, and in the import slots of system functions the executable imports by name ([import slots](#import-slots)); plugins never install hooks or name addresses, they use the [host services](./services.md) the adapters implement.

## Kinds

| Kind | Map entry | What the handler gets |
| --- | --- | --- |
| Entry detour | function | the arguments; it may call the original and return a value |
| Mid-function hook | location inside a function | the register context; it may change registers before execution continues |
| Code patch | patch with expected original bytes | nothing; the bytes are replaced while the patch is active |
| Vtable slot | vtable and index | as an entry detour, for that object type only |
| Import slot | none: a DLL and function the executable's import directory names | as an entry detour, for the executable's own calls through its import only |

## Entry detours

The executable's entry point is never hooked: the host starts before it without a change to the executable's code ([loading](./loading.md)).

A detour overwrites the start of a function with a 5-byte `jmp rel32` to a relay, and fills the rest of the overwritten instructions with `int3`. The relay, an absolute `jmp qword ptr [rip]`, jumps to the replacement anywhere in the address space. The overwritten instructions move into a trampoline that ends with a jump back to the first instruction after them; calling the trampoline runs the original function. Relay and trampoline lie in executable memory within 2 GiB of the target.

### Relocation

Relocation decodes whole instructions with `iced-x86` until they cover 5 bytes and encodes them at the trampoline's address, keeping the targets of relative branches and RIP-relative operands. Installation fails, and the entry is reported as unhookable for the build rather than hooked incorrectly, when:

- an instruction is invalid or cut off;
- the function leaves unconditionally (a return, a jump, an interrupt) before 5 bytes;
- a RIP-relative operand cannot reach its target from the trampoline;
- the function's code cannot be shown free of branches into the overwritten bytes other than the hook's own address. The function's code is known exactly from its map entry: its ranges in the exception directory, chained chunks included, or, for a leaf function, the code its control flow reaches ([format](../formats/game-map-v2.md#locators)); a hook in code that is not known exactly is not installed. When the overwritten bytes lie within the function's prologue, which its unwind information gives, nothing branches into them: the Windows x64 conventions enter a prologue only at the function's start. Otherwise the code is followed from the start of every range, through every instruction and every branch within the function; the hook is refused when a branch targets the overwritten bytes, when the followed code reaches an invalid instruction or a jump through a register or computed address, whose targets the code does not state, or when the hook's address is not an instruction the followed code reaches.

### Installing and removing

While the bytes change, every other thread of the process is suspended. A suspended thread whose instruction pointer is on an overwritten instruction continues at the same instruction in the trampoline; when a detour is removed, a thread on a moved instruction in the trampoline continues at the original. Nothing allocates memory while threads are suspended, since a suspended thread may hold the heap lock. Only one thread at a time suspends the others: Windows suspends a thread asynchronously, so two threads doing it at once could each suspend the other and both stay suspended.

A detour is enabled only while the target still holds the bytes it was prepared from, and removed only while the target still holds its own jump. Trampolines and relays are never freed, because a thread may still be executing in one after its detour is removed.

## Mid-function hooks

A mid-function hook overwrites the instructions at a location with a `jmp rel32` to a stub near it, relocated as for a detour. The stub:

1. pushes the general-purpose registers and the flags and stores `xmm0` to `xmm15`, forming the context;
2. calls the handler with the context's address on a 16-byte aligned stack with 32 bytes of shadow space;
3. loads every register and the flags back from the context, so the handler's changes take effect;
4. runs the moved instructions and jumps back.

The stack pointer is not part of what a handler can change: it is the address just past the context. A handler must not unwind; the host catches panics inside it. A hook made with data passes the handler a number of the installer's choice too, so one handler serves many hooks, as the watch hooks of Lens do with their point.

A returning hook is a mid-function hook at a function's first instruction whose handler may answer for the function: when it returns true, the stub loads the registers back from the context and returns to the caller with the context's `rax`, without running the function; otherwise the function runs. The decision points of Lens use it.

When the hook is installed, a suspended thread on the hooked instruction continues at the start of the stub and runs the handler; a thread on a later overwritten instruction continues at that instruction among the moved ones.

## Code patches

A code patch is prepared only when the bytes at the address equal the expected original bytes in the map, so a patch never writes over code it was not made for. Applying or reverting it suspends the other threads and fails with a retryable error when a thread is stopped inside the bytes.

## Vtable slots

A vtable slot is one aligned pointer, replaced with a single store; no thread is suspended. The slot is restored only while it still holds the replacement.

## Import slots

An import slot is the entry of the executable's import address table that the loader fills with the address of a function the executable imports from a system DLL. It is found by the DLL's and the function's names in the import directory of the executable's file (`angelica-pe` `Image::import_slots`), never by an address or a signature, so a game update that keeps importing the function keeps the slot findable; the slot is replaced as a vtable slot, with one aligned store. Installation is refused, and the feature that needs it works without it, when the executable does not import the function by name exactly once, or the slot does not hold a function. A slot that another tool already replaced keeps that tool's function as the original, which Angelica calls through. The executable's calls through other means, such as an address from `GetProcAddress`, and every other module's calls are not affected.

The overlay replaces the slot of `USER32.dll!SetCursor`: the game sets its cursor every frame, so while the pointer is over Angelica's interface and Angelica shows a cursor of its own (a hand, a resize or text cursor), the game's call leaves Angelica's cursor in place; otherwise it passes on unchanged. Without the slot, the game's cursor shows through Angelica's for part of every frame.

## Chaining

The host installs at most one hook per function. When several plugins' rules apply to the same calls, the adapter combines them in its own defined order (for rules, the order they were added).

Other tools in the process may hook the same function. A detour installed over another tool's jump moves that jump into its trampoline, so calling the original runs the other tool's hook. A tool that installs its hook over Angelica's does the same, and Angelica's detour is then removed only after that tool removes its own; until then it stays installed.

## Intercepts

A plugin's [intercepts](../formats/plugin-api-v1.md#intercepts) use a returning mid hook at the first instruction of each intercepted function, installed the first time a plugin intercepts it and kept until the process exits. The hook's handler reads the arguments the handlers' types declare, at most 16: the first four from both the general and the `xmm` registers, the rest from the caller's stack slots. It runs the `before` handlers in the sandbox. When the function has no `after` handler and none skipped it, it writes the arguments back to the registers and stack slots, which the function owns, and the game's code runs as usual from the moved instructions. Otherwise it returns at once with `rax` and `xmm0` set: to the skipped value, or to what the game's code returned when the hook called it through the moved instructions with the arguments and then ran the `after` handlers. A pointer a handler passes on is one the call gave, so no plugin makes up an address. The hook's handler catches panics, a handler that fails is disabled and its changes dropped, and with no handler left the function runs as if unhooked.

## Hooks and plugins

## Thunks

A [thunk](../formats/plugin-api-v1.md#calls-and-thunks) is one of 256 stubs compiled into Angelica's runtime, 16 bytes apart in its code section: each loads its slot's number and jumps to a common entry that saves the four general and four `xmm` argument registers, points at the caller's stack arguments, and calls the host with them; the host runs the plugin's handler in the sandbox, and the entry returns its result in `rax` and `xmm0`. No code is written at run time. A slot belongs to one thunk of one plugin until the process exits, so an address the game kept never reaches another plugin; a stopped plugin's thunk runs its fallback or returns zero. The entry catches panics and returns zero.

No adapter's hook calls plugin code; only the detours of intercepts and thunks do, through the sandbox. Functions the game calls often — building rows, converting text — are handled natively by adapters from rules plugins activated; see [`services.md`](./services.md). An adapter that reports what the game did — a line the player sent, a message, a window that opened — records it in a bounded queue, and the host delivers it to plugins in the next frame. A plugin changes what the game does only through rules or host operations, never from inside a hook. An adapter's detour catches panics and passes the call on unchanged when it fails.

The watches of Lens use a mid-function hook at the map entry of each watched event: it reads the plugins' watches from the registers through the checked memory copy, keeps the rows for the next frame, and changes no register ([watches](./lens.md#watches)). Its decision points use a returning hook at the start of each decided function: the host applies the plugins' rules there itself and either lets the function run or returns the outcome's value ([decisions](./lens.md#decisions)).
