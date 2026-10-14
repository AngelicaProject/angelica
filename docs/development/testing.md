# Testing strategy

Tests protect contracts, not implementation trivia. Tests must not depend on an installed game, credentials, or network access, except the explicitly ignored tests described below.

Development and test builds compile the workspace's dependencies with optimization (`[profile.dev.package."*"]` in `Cargo.toml`): plugin tests compile components with wasmtime's compiler, which is many times slower unoptimized. The workspace's own crates stay unoptimized so they rebuild quickly; the first build after a dependency changes takes longer.

## Test layers

- **Pure crates** (`angelica-pe`, `angelica-map`, manifest and permission logic): unit tests and property tests over synthetic PE images and map files built in the tests.
- **Hook engine**: unit tests of relocation on any platform, and, on Windows, tests that install hooks on machine code copied into executable memory (`angelica-hook`'s `testing` feature): RIP-relative operands and branches in the moved instructions, chaining and removal order, refusals, code patches with matching and non-matching original bytes, vtable slots, mid-function hooks that read and change general-purpose and XMM registers while flags survive, and a thread that spins inside the overwritten bytes while a detour is installed, repeated so that it is stopped at every instruction.
- **Plugin runtime**: test components built in the workspace exercise limits, traps, permission refusals, and main/worker isolation. `crates/angelica-fake-game/hostile` is a plugin that tries to break each boundary — endless guest and host loops, a frame's calls together, memory, stack, panics, invalid windows, unpolled jobs, storage paths, oversized files, operations in the wrong instance, undeclared permissions — and `tests/hostile.rs` checks that the host faults it or refuses the operation while the example plugin keeps running.
- **Plugins**: `angelica-fake-game` runs a plugin against recorded or scripted game data.

## Synthetic images

The `testing` feature of `angelica-pe` builds x64 PE images with chosen sections (`PeBuilder`) and lays out a file image as Windows maps it (`map_file`). Tests of anything that reads an image use it instead of real executables, and check the file and the mapped layout give the same answers.

## Loader tests

`angelica-proxy` tests configuration, the choice of the current version, and strict signature verification without an installed game.

The Windows bootstrap integration test builds an isolated proxy with a test signing key, a signed synthetic runtime, and an executable that statically imports the proxy. It checks that the host starts before executable code and that invalid signatures, missing files, failed host startup, and incomplete updates leave the executable running:

```text
cargo test -p angelica-proxy --test bootstrap
```

The test invokes the pinned compiler and an offline Cargo build in a temporary directory and runs in Windows CI. It requires the workspace dependencies in the Cargo cache, but no game, network, or release key. It also checks export names and ordinals and confirms rejected DLLs never execute their `DllMain`.

## Host tests

Pure host modules have unit tests: text tables against rows (layout checks, guards, priorities, refused buffers), letter case against the game's rules, storage confinement and quota, manifests and grants. Every parser and check that reads what a plugin, a package, or the player provides — text tables, replacement fonts, textures, materials, and models, command lines, chat messages, manifests — is also fed tens of thousands of damaged inputs from a seeded generator (`mutations.rs` in the host, `tests/text.rs` in `angelica-text`) and must refuse them without panicking.

`angelica-fake-game` runs actual components in the real host: `build_component` builds a package for `wasm32-wasip2` with the pinned compiler, and `TestHost` gives the host temporary folders, a scripted game, and the calling thread as its main thread. `tests/hello.rs` drives [`examples/hello`](../../examples/hello) through its window: approval, events, storage, jobs, text tables applied to scripted rows, letters, permissions and missing map entries, and the player's choices across game starts. A plugin's own tests use the same crate.

The Windows-only tests install the row, letter case, and file job detours on synthetic machine code and drive them with synthetic rows, sheets, strings, and an allocator vtable, checking the rebuilt buffers, the allocate and free calls, and the strings converted in place. They also cover the overlay painter on a software Direct3D 11 device, loading trusted compiler output, and the crash writer producing a real minidump of the test process. `angelica-host/tests/package.rs` checks that file sets are authenticated before parsing and that every content hash is checked. The compile CLI test checks that invalid source leaves an existing destination intact.

## Tests against the real game executable

The PE model and map resolution are also checked against the installed game with ignored tests that read `ANGELICA_GAME_PATH`, the game's `game` folder:

```text
cargo test -p angelica-pe --test game -- --ignored
```

Run them after changing the PE model or the resolver. They are not part of CI, which has no game installation.

The archive and sheet reader is checked the same way: `cargo test -p angelica-host --lib game_data -- --ignored` reads the sheet list, `Item` and `Quest`, `TextCommand` in every installed language, a font table, and a font texture from the installation. The checks of replacement game files are calibrated on every file of their format the game ships: `cargo test --release -p angelica-host --lib file_formats -- --ignored` checks every texture of the `common`, `bgcommon`, `chara`, `ui`, and `vfx` archives and every material and model of `bg`, `bgcommon`, `chara`, `common`, and `vfx` (several minutes; `GameData::each_file` of the `testing` feature walks an archive). A rule that a shipped file breaks is a wrong rule. `angelica-sqpack` and the host's `game_data` module otherwise run against synthetic installations that `angelica_sqpack::testing::FakeGame` writes, and `TestHost::game_folder` gives a plugin test its own.

## Tests over the internet

The HTTP service is checked against real servers with ignored tests that need the internet: `cargo test -p angelica-host --lib http -- --ignored` fetches a page, follows a redirect to a host that is not listed, meets an expired certificate, and uploads a file to an echo service; `cargo test -p angelica-host --lib sockets -- --ignored` opens, echoes over, and closes a public WebSocket; `cargo test -p angelica-fake-game --test hello -- --ignored` fetches and downloads a page from a plugin's job. Run them after changing the HTTP service. CI does not run them; every other HTTP test checks requests before anything is sent.

## In-game checks

A plugin or the game map is changed without restarting the game: `angelica dev install` with `--plugin` and `--map`, but without `--runtime`, writes the signed packages while the game runs, and the host loads them within a second ([reload](../architecture/plugins.md#lifecycle)). Changes to the host itself need a game restart.

A change to code that runs in the game process is checked in the running game before review. The pull request names what was checked: the game build, the plugins loaded, and the behavior observed.

A new invariant check is confirmed to fail against a deliberately broken implementation before it is relied on.
