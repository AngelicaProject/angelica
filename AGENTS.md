# Angelica project guidelines

This file is the repository entry point for coding tools. Durable project rules live in `docs/`; task-specific execution guidance lives in `.ai/`. Do not duplicate canonical documentation here.

## Context loading

Load the instruction modules that match the task before planning, implementing, reviewing, committing, or opening a pull request:

- Any change: `.ai/development.md`
- Git branches, commits, rebases, or pushes: `.ai/git.md`
- Pull requests: `.ai/pull-requests.md`
- Code review or review feedback: `.ai/code-review.md`
- CI or workflow failures: `.ai/ci.md`
- Documentation changes: `docs/AGENTS.md`
- The website or its documentation pages: `docs/development/website.md`

Then load the canonical project documentation for the affected area:

- Product purpose and invariants: `docs/product/README.md`
- Components and their ownership: `docs/architecture/overview.md`
- Loading into the game process: `docs/architecture/loading.md`
- Hooks and code patches: `docs/architecture/hooks.md`
- The game map as the host uses it: `docs/architecture/game-map.md` and `docs/formats/game-map-v1.md`
- Plugin runtime: `docs/architecture/plugins.md` and `docs/formats/plugin-manifest-v1.md`
- Writing a plugin or the SDK: `docs/plugins/README.md`
- Permissions: `docs/architecture/permissions.md`
- Host services: `docs/architecture/services.md`
- In-game interface: `docs/architecture/ui.md`
- Rust implementation and `unsafe`: `docs/development/rust.md`
- Dependencies and toolchains: `docs/development/dependencies.md`
- Tests: `docs/development/testing.md`
- Contribution workflow: `docs/development/README.md`

Start from `docs/README.md` when the correct document is not obvious.

## Always-on rules

- Make the smallest coherent change that solves the requested problem.
- Preserve the product invariants in `docs/product/principles.md` and the ownership boundaries in `docs/architecture/overview.md`.
- Treat the game executable and released formats (game map, plugin manifest, plugin API) as contracts. Do not guess at unknown data.
- A plugin never contains game addresses or offsets; it names game map entries.
- Plugin code never runs outside the sandbox, and a permission is never granted implicitly.
- Code that runs in the game process must not let a failure take the game down: a failing plugin or hook is disabled and the original game code runs.
- Correctness-sensitive boundaries: memory access in the game process, hook installation and chaining, map resolution, permission checks, plugin isolation, signature verification of anything loaded.
- `unsafe` Rust is allowed only where `docs/development/rust.md` allows it, with a `// SAFETY:` comment on every block.
- Do not weaken tests, validation, or CI gates to make a change pass.
- Never claim a command, push, CI run, in-game check, or remote state succeeded unless it was actually observed.
- When behavior, formats, architecture, or development requirements change, update the canonical documentation in the same change.

## Related repositories

- `angelica-gamemap`: the game map data. Its format is owned here (`docs/formats/game-map-v1.md`).
- `angelica-registry`: the plugin catalog.

## Project notes

- Default branch: `main`.
- Target: the Windows DirectX 11 game client, `x86_64-pc-windows-msvc`.
