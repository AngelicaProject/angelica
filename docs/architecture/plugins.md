# Plugin runtime

## Plugins are WebAssembly components

A plugin is a WebAssembly component of the `plugin` world in [`wit/`](../../wit), the [plugin API](../formats/plugin-api-v1.md). The host runs it with wasmtime. Any language that builds a component can be used; the SDK and documentation target Rust (`wasm32-wasip2`).

A plugin comes as a [plugin file](../formats/plugin-file-v1.md): from its author, signed with the author's key, or from the [catalog](./catalog.md), which builds it from the author's source and signs it with the release key. The installer compiles it on the player's computer and signs the package with the installation's key. A package signed with the release key that holds a compiled component ([installation](../formats/host-installation-v1.md)) still loads; only development installations make them. The host loads only authenticated artifacts and never compiles in the game.

## Plugins from their authors

A plugin outside the catalog comes from its author's link ([plugin file](../formats/plugin-file-v1.md)). The host never downloads or compiles one in the game process: it runs `plugin-installer.exe`, which ships beside `runtime.dll`, is signed like it, and is built with the same engine. The host checks the program's signature with the release key and holds it open, without write or delete sharing, until the process has started. One job runs at a time on a host thread:

- **install**, when the player gives a link in the Angelica menu: download, check the author, compile, write the package; the reload below picks it up and it waits for approval;
- **update**, at game start and every 6 hours: newer versions of the same author from each plugin's link, installed the same way; a version that declares more waits for approval;
- **repair**, when this runtime cannot load the compiled component of a plugin from its author, as after a runtime update: the installed plugin file is compiled again, once per game session.

The menu shows what each job did. Without the program, the menu says so when the player installs from a link, and nothing is updated.

## Isolation

- A plugin sees only its own linear memory and the host's imports. Game memory is never mapped into it; everything a plugin gives or gets is copied.
- Every operation on the game is a host service with a verified adapter and a permission ([services](./services.md)). There is no access to game memory, game functions, or addresses.
- The calls of a frame are bounded together in guest time, fuel, and time including host calls; each instance in memory. A trap or an exceeded limit faults the plugin: its effects are removed and it stays off until the player retries it. The game keeps running.
- WASI imports Rust's standard library adds expose only random numbers and clocks; all others trap.

## Instances

A component instance is single-threaded. A running plugin has:

- a **main instance** on the game's main thread: `start`, `tick`, window events, `ui`, `stop`, and every operation that changes the game;
- **job instances**, one per job, on host threads: long work such as verifying and converting a large file, with storage access and progress reports. They share nothing with the main instance except the plugin's storage.

Hot paths never call a plugin. Work the game does for every row, string, or file — replacing row text, converting letter case — is done natively from rules the plugin activated.

## Lifecycle

1. **Discovery.** At startup the host reads each authenticated package in `plugins/`, parses its manifest, loads its compiled component, and links the host's imports. A package that fails is listed with the reason. A plugin from its author is marked as such in the menu, and one installed by `angelica dev install` is marked as in development; a new version must come from the same place as the installed one.
2. **Approval.** A package without a grant covering its permissions waits in the Angelica menu, which lists its permissions. Approving writes the grant and starts the plugin.
3. **Start.** Approved and enabled plugins start before the game's own code runs, so rules they activate in `start` apply to everything the game loads. A plugin enabled later starts at once; `runtime.started-with-game` tells the two apart.
4. **Running.** Every frame: `tick`, the plugin's commands the player ran, the game events it subscribed to, pending window events, `ui`.
5. **Reload.** While the game runs, the host looks at the plugin directory every second. A package whose signed file list changed — a new version written by the installer or `angelica dev install` — is authenticated and prepared; if that fails, the old version keeps running and the menu shows why. Otherwise the old version stops as when the player disables it, and the new one starts if the player approved everything it declares and left it enabled; a version that declares more waits for approval. Storage, grants, and the player's choices carry over. A new package folder is installed the same way.
6. **Stop.** Disabling calls `stop`, then removes the plugin's text tables, letters, replaced files, commands, event subscriptions, jobs, and windows. A fault does the same without calling `stop`. At game exit every plugin stops.
7. **Removal.** The player removes a plugin from the menu: it stops as when disabled and leaves the list; its grant is deleted first, so a removal cut short never leaves it approved, then its settings and its package folder, and, when the player chooses, its storage folder. The host forgets the removed package, so the same package installed again loads and waits for approval.

The player's choice to disable a plugin is kept across game starts ([settings](../formats/host-installation-v1.md#plugin-settings)). A faulted plugin is never restarted automatically.

## Developing a plugin

The [plugin author guide](../plugins/authoring.md) walks through the work loop; this section describes the pieces it uses.

`angelica plugin new <id>` creates a plugin project in its own folder: a working plugin with a window and a chat command, its manifest, a test against the scripted game, the toolchain the components build with, and an `AGENTS.md` that tells any coding agent the work loop, the commands, and the rules. The project uses the SDK and the scripted game from the Angelica repository on GitHub, or from a local checkout with `--angelica`, and `cargo test` builds the component and runs the tests.

A Rust plugin depends on `angelica-sdk`, implements `angelica_sdk::Plugin`, and exports it with `angelica_sdk::plugin!`. Beside the bindings, the SDK passes jobs' inputs and outputs as typed values (`jobs`), saves values in the plugin's folder (`storage`), and sends requests from a URL (`http`); values travel as JSON. [`examples/hello`](../../examples/hello) uses every interface. `angelica-fake-game` runs a plugin's component in the real host against a scripted game, so a plugin's own tests cover its windows, storage, jobs, and the rows its tables change without the game.

To run a plugin in the game during development: build it for `wasm32-wasip2`, then `angelica dev install` signs it with a development key into a local installation ([development installations](../formats/host-installation-v1.md#development-installations)). While the game runs, the host loads the new version within a second (see Reload above).
