# Testing strategy

Tests protect contracts, not implementation trivia. Tests must not depend on an installed game, credentials, or network access, except the explicitly ignored tests described below.

## Test layers

- **Pure crates** (`angelica-pe`, `angelica-map`, manifest and permission logic): unit tests and property tests over synthetic PE images and map files built in the tests.
- **Hook engine**: tests that hook functions compiled into the test binary, on Windows, covering relocation of RIP-relative instructions and branches, mid-function context, code patches with matching and non-matching original bytes, and chaining over a hook installed by another hook library.
- **Plugin runtime**: test components built in the workspace exercise limits, traps, permission refusals, and hot reload.
- **Plugins**: `angelica-fake-game` runs a plugin against recorded or scripted game data.

## Tests against the real game executable

Map resolution is also checked against a real executable with ignored tests that read `ANGELICA_GAME_PATH`. Run them after changing the resolver or the PE model; they are not part of CI, which has no game installation.

## In-game checks

A change to code that runs in the game process is checked in the running game before review. The pull request names what was checked: the game build, the plugins loaded, and the behavior observed.

A new invariant check is confirmed to fail against a deliberately broken implementation before it is relied on.
