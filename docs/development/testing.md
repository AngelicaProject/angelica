# Testing strategy

Tests protect contracts, not implementation trivia. Tests must not depend on an installed game, credentials, or network access, except the explicitly ignored tests described below.

## Test layers

- **Pure crates** (`angelica-pe`, `angelica-map`, manifest and permission logic): unit tests and property tests over synthetic PE images and map files built in the tests.
- **Hook engine**: tests that hook functions compiled into the test binary, on Windows, covering relocation of RIP-relative instructions and branches, mid-function context, code patches with matching and non-matching original bytes, and chaining over a hook installed by another hook library.
- **Plugin runtime**: test components built in the workspace exercise limits, traps, permission refusals, and hot reload.
- **Plugins**: `angelica-fake-game` runs a plugin against recorded or scripted game data.

## Synthetic images

The `testing` feature of `angelica-pe` builds x64 PE images with chosen sections (`PeBuilder`) and lays out a file image as Windows maps it (`map_file`). Tests of anything that reads an image use it instead of real executables, and check the file and the mapped layout give the same answers.

## Tests against the real game executable

The PE model and map resolution are also checked against the installed game with ignored tests that read `ANGELICA_GAME_PATH`, the game's `game` folder:

```text
cargo test -p angelica-pe --test game -- --ignored
```

Run them after changing the PE model or the resolver. They are not part of CI, which has no game installation.

## In-game checks

A change to code that runs in the game process is checked in the running game before review. The pull request names what was checked: the game build, the plugins loaded, and the behavior observed.

A new invariant check is confirmed to fail against a deliberately broken implementation before it is relied on.
