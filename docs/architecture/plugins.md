# Plugin runtime

## Plugins are WebAssembly components

A plugin is a WebAssembly component that targets the Angelica API defined in WIT under `wit/`. The host runs it with wasmtime. Any language that builds a component can be used; the SDK and documentation target Rust.

The host compiles each plugin ahead of time when it is installed and loads the compiled artifact at game start. Compiled artifacts are a cache tied to the wasmtime version and are rebuilt when it changes.

## Isolation and limits

- A plugin sees only its own linear memory and the API functions its permissions allow; see [`permissions.md`](./permissions.md).
- Game memory is never mapped into plugin memory. Reads and writes go through API calls that copy data and check the access against the plugin's permissions and map entries.
- Each plugin has a memory limit and a time budget per call, enforced with epoch interruption. A plugin that traps or exceeds a limit is disabled for the session, and its hooks and service rules are removed.

## Instances and threads

A component instance is not thread-safe. Each plugin has:

- a **main instance** that runs on the game's main thread: start-up, the frame tick, events, the user interface, and hooks on main-thread functions;
- optionally a **worker instance** for hooks on other game threads. Main and worker share data only through host-provided shared tables, never through plugin memory.

Every function entry in the map declares the thread kind it runs on, and the host binds a handler to the matching instance. A plugin cannot call a main-thread game function from a worker. The host checks the calling thread on every hooked call: a call from a thread that does not match the entry's declaration skips the handler, runs the original, and reports the entry as declared wrongly.

## Lifecycle

1. Load: the manifest is checked against the granted permissions and the present map entries.
2. Start: the main instance runs its start export and registers hooks, commands, windows, and service rules.
3. Run: frame ticks, events, and hook calls.
4. Stop: on disable, update, or game exit, hooks and rules are removed before the instance is dropped.

## Development mode

In development mode, a plugin is reloaded in the running game when its build changes: the host stops it, removes its hooks and rules, and starts the new build. Development mode also allows local map entries that are not verified. A plugin that uses local entries cannot be packed for the registry.

## Interfaces between plugins

A plugin may export a WIT interface and another may import it. The dependency is declared in both manifests and checked at install time, so an incompatible version is reported before the game starts. Calls between plugins go through the host and are subject to the caller's permissions.

## Recording and replay

The host can record a plugin's hook calls and API results during a session. `angelica-fake-game` replays a recording against a plugin build, so plugin behavior can be tested without the game.
