# Testing strategy

Tests protect contracts, not implementation trivia. Tests must not depend on an installed game, credentials, or network access, except the explicitly ignored tests described below.

Development and test builds compile the workspace's dependencies with optimization (`[profile.dev.package."*"]` in `Cargo.toml`): plugin tests compile components with wasmtime's compiler, which is many times slower unoptimized. The workspace's own crates stay unoptimized so they rebuild quickly; the first build after a dependency changes takes longer.

## Test layers

- **Pure crates** (`angelica-pe`, `angelica-map`, `angelica-view`, manifest and permission logic): unit tests and property tests over synthetic PE images and map files built in the tests.
- **Hook engine**: unit tests of relocation on any platform, and, on Windows, tests that install hooks on machine code copied into executable memory (`angelica-hook`'s `testing` feature): RIP-relative operands and branches in the moved instructions, chaining and removal order, refusals, code patches with matching and non-matching original bytes, vtable slots, mid-function hooks that read and change general-purpose and XMM registers while flags survive, and a thread that spins inside the overwritten bytes while a detour is installed, repeated so that it is stopped at every instruction.
- **Plugin runtime**: test components built in the workspace exercise limits, traps, permission refusals, and main/worker isolation. `crates/angelica-fake-game/hostile` is a plugin that tries to break each boundary — endless guest and host loops, a frame's calls together, memory, stack, panics, an invalid view, unpolled jobs, storage paths, oversized files, operations in the wrong instance, undeclared permissions — and `tests/hostile.rs` checks that the host faults it or refuses the operation while the example plugin keeps running.
- **Plugins**: `angelica-fake-game` runs a plugin against recorded or scripted game data.
- **Interface examples**: `angelica-view`'s `tests/examples.rs` checks that every kind of place and part has an example that passes the host's checks; the host's ignored `gallery` test draws them for the website ([website](./website.md)).

## Synthetic images

The `testing` feature of `angelica-pe` builds x64 PE images with chosen sections (`PeBuilder`) and lays out a file image as Windows maps it (`map_file`). Tests of anything that reads an image use it instead of real executables, and check the file and the mapped layout give the same answers.

## Loader tests

`angelica-proxy` tests configuration, the choice of the current version, and strict signature verification without an installed game.

The Windows bootstrap integration test builds an isolated proxy with a test signing key, a signed synthetic runtime, and an executable that statically imports the proxy. It checks that the host starts before executable code and that invalid signatures, missing files, failed host startup, and incomplete updates leave the executable running:

```text
cargo test -p angelica-proxy --test bootstrap
```

The test invokes the pinned compiler and an offline Cargo build in a temporary directory and runs in Windows CI. It requires the workspace dependencies in the Cargo cache, but no game, network, or release key. It also checks export names and ordinals and confirms rejected DLLs never execute their `DllMain`.

The same fixture runs in Wine, as the game does on Linux ([loading](../architecture/loading.md#in-wine)). The ignored `wine_fixture` test writes the proxy, the game, and the signed runtime into the folder `ANGELICA_WINE_FIXTURE` names, and `tests/wine.sh` runs the game in a new Wine prefix: without the `dinput8` override the host must not start, and with the override the installer sets it must start before the game's entry point. Wine's own `dinput8` has no `GetdfDIJoystick`, so with `ANGELICA_TEST_WINE` set the game accepts the null that call returns:

```text
ANGELICA_WINE_FIXTURE=<folder> cargo test -p angelica-proxy --test bootstrap -- --ignored --exact wine_fixture
sh crates/angelica-proxy/tests/wine.sh <folder>   # on Linux, with Wine installed
```

## Host tests

Pure host modules have unit tests: rules and text tables against rows (layout checks, guards, their order, refused buffers), letter case against the game's rules, storage confinement and quota, manifests and grants. Every parser and check that reads what a plugin, a package, or the player provides — text tables, replacement fonts, textures, materials, and models, command lines, chat messages, manifests, the places and parts plugins show — is also fed tens of thousands of damaged inputs from a seeded generator (`mutations.rs` in the host, `tests/text.rs` in `angelica-text`, `tests/data.rs` in `angelica-view`) and must refuse them without panicking.

`angelica-fake-game` runs actual components in the real host: `build_component` builds a package for `wasm32-wasip2` with the pinned compiler, and `TestHost` gives the host temporary folders, a scripted game, and the calling thread as its main thread. `TestHost::new` gives the guest 1 s of time per frame instead of the game's 20 ms, because a loaded test machine pauses threads for longer than a frame and the guest time counts the pause; fuel still bounds the calls. `TestHost::exact` keeps the limits it is given, for tests of the time limit. `tests/hello.rs` drives [`examples/hello`](../../examples/hello) through its window: approval, events, storage, jobs, text tables applied to scripted rows, letters, permissions and missing map entries, and the player's choices across game starts. A plugin's own tests use the same crate.

The Windows-only tests install the row, letter case, and file job detours on synthetic machine code and drive them with synthetic rows, sheets, strings, and an allocator vtable, checking the rebuilt buffers, the allocate and free calls, and the strings converted in place. They also cover the overlay painter on a software Direct3D 11 device, loading trusted compiler output, and the crash writer producing a real minidump of the test process. `angelica-host/tests/package.rs` checks that file sets are authenticated before parsing and that every content hash is checked. The compile CLI test checks that invalid source leaves an existing destination intact.

## Tests against the real game executable

The PE model and map resolution are also checked against the installed game with ignored tests that read `ANGELICA_GAME_PATH`, the game's `game` folder:

```text
cargo test -p angelica-pe --test game -- --ignored
```

Run them after changing the PE model or the resolver. They are not part of CI, which has no game installation.

Intercepts run against the scripted game, which runs a call as the game's hook does: `before` handlers, then the test's stand-in for the game's code unless one skipped it, then `after` handlers. `angelica-fake-game`'s `typing_fold` test installs the typing fold example with its own entries, resolves them in a synthetic executable, and calls the intercepted function, whose handler changes the game's string in place in an intercept instance; it also prints what one handler call costs. Its `intercept_calls` test installs the `angelica-intercepting` test plugin, once or under several ids, and checks changed arguments and results, skips, the order manifests ask for and contradicting wishes, values of the wrong type, a failing handler whose changes are dropped, floats and stack arguments, text arguments, and a function that calls itself; it also prints what a handler costs that replaces a row's text from a table or looks a file's path up among replaced ones, with tables as large as a whole translation's, which the intercept instance prepares in the background; and that a snapshot that fails to prepare leaves handlers with the one they had. Its `intercepts` test checks that the watchdog stops a handler that never returns and disables the plugin's handlers, and that a handler reaches no import but `runtime.log` and `intercepts`. Direct access runs against the scripted game too: `use_map` gives the host the description it converts from the test's map, laid out by the same constants. `angelica-fake-game`'s `memory` test installs the `angelica-memory` test plugin, which reads the player and the bags through the SDK's views and checks the domains it holds, an element of a domain it does not hold, a frame handle used in a later frame, a held handle across frames and once its path reaches another object, a batch, and central writes: refused where no change is offered, refused past the range, written within it, and put back when the plugin stops. `intercept_calls` checks that a handler reads and writes an object of the plugin's own structure through its call handle and cannot hold it, and, with functions the test defines in the scripted game (`define_function`), that the plugin calls its own functions through the views its `build.rs` generates, only with `server`, and that the game calls its thunks back (`call_thunk`): through the handler, through the fallback when the handler fails, and to zero once the plugin stopped or when the handler gives another type. The Lens library runs against the scripted game, whose functions of the game map have a place of their own that the host may intercept: `angelica-fake-game`'s `lens_library` test installs the `angelica-lens-library` test plugin and checks a query through handles, a query over divided bags, a watch whose handler reads each chat message and hands the rows to the main instance, a rule that hides a message by skipping the game's function, and that the core refuses a watch without the domain and lets a rule without the domain to change skip nothing.

Surfaces are checked at three levels: the host's `gpu` tests check objects, limits, renders, focus, and the translation of WGSL; the `interop::surfaces` tests run a plugin's WGSL through the translation, Direct3D 11's compiler, and a render on the software rasterizer (WARP) and read the surface's texels back; and `angelica-fake-game`'s `surface` test installs the surface example, whose interface kit renders a window on its surface every frame, and routes the pointer to the window and past it to the game. The host's `interop::thunks` tests call a native stub with registers and stack arguments and check the slot, the arguments, and both results. `angelica-views`' tests check the views a description gives, and `angelica-anatomia`'s that a map given as files converts as its folder does. The host's `interop::intercepts` tests run a hook on synthetic machine code and call a function through the hook's path with integers, floats, and stack arguments.

Anatomia is checked against the published game map the same way. `angelica-anatomia`'s tests convert a fixture map that holds every construct of game map v2 to Anatomia and back and compare the result with the map as the v2 reader reads it, and resolve the converted fixture against a synthetic image as `angelica-map` resolves the map. The ignored test does the same round trip for a checkout of `angelica-gamemap` that `ANGELICA_GAMEMAP` names:

```text
ANGELICA_GAMEMAP=<checkout> cargo test -p angelica-anatomia --test convert -- --ignored
```

Run it, and compare `angelica anatomia verify` of the converted map with `angelica map verify` of the map on the installed game, after changing the conversion or the resolver.

The archive and sheet reader is checked the same way: `cargo test -p angelica-host --lib game_data -- --ignored` reads the sheet list, `Item` and `Quest`, `TextCommand` in every installed language, a font table, and a font texture from the installation. The checks of replacement game files are calibrated on every file of their format the game ships: `cargo test --release -p angelica-host --lib file_formats -- --ignored` checks every texture of the `common`, `bgcommon`, `chara`, `ui`, and `vfx` archives and every material and model of `bg`, `bgcommon`, `chara`, `common`, and `vfx` (several minutes; `GameData::each_file` of the `testing` feature walks an archive). A rule that a shipped file breaks is a wrong rule. `angelica-sqpack` and the host's `game_data` module otherwise run against synthetic installations that `angelica_sqpack::testing::FakeGame` writes, and `TestHost::game_folder` gives a plugin test its own.

## Tests over the internet

The HTTP service is checked against real servers with ignored tests that need the internet: `cargo test -p angelica-host --lib http -- --ignored` fetches a page, follows a redirect to a host that is not listed, meets an expired certificate, and uploads a file to an echo service; `cargo test -p angelica-host --lib sockets -- --ignored` opens, echoes over, and closes a public WebSocket; `cargo test -p angelica-fake-game --test hello -- --ignored` fetches and downloads a page from a plugin's job. Run them after changing the HTTP service. CI does not run them; every other HTTP test checks requests before anything is sent.

## In-game checks

A plugin or the game map is changed without restarting the game: `angelica dev install` with `--plugin` and `--map`, but without `--runtime`, writes the signed packages while the game runs, and the host loads them within a second ([reload](../architecture/plugins.md#lifecycle)). Changes to the host itself need a game restart.

A change to code that runs in the game process is checked in the running game before review. The pull request names what was checked: the game build, the plugins loaded, and the behavior observed.

A new invariant check is confirmed to fail against a deliberately broken implementation before it is relied on.
