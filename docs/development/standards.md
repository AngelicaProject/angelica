# Development standards

## Toolchains

Pinned toolchains:

- Rust 1.98.1, edition 2024
- Target `x86_64-pc-windows-msvc` for everything that runs in the game; `wasm32-wasip2` for plugins

When changing these versions, verify the current stable upstream release first and update pins and lockfiles together.

## Reliability

- Code that runs in the game process never lets a failure propagate into game code. Every detour, exported function, and callback catches panics at its boundary and falls back to the original game behavior.
- Expected failures (an absent entry, a refused permission, a malformed file) are typed errors and visible diagnostics, not panics.
- Files the host writes (settings, `current.json`, caches) are written to a temporary file and atomically replaced.
- Nothing is decided by guessing: an ambiguous match, an unknown version, or an unverifiable condition is reported and treated as absent.

## Machine-readable tools

Every `angelica` command accepts `--json` and then writes one JSON document to standard output, with errors as structured values naming the entry, permission, file, or plugin involved. Exit codes distinguish success, a reported failure, and a usage error.

## Documentation

Repository documentation follows [`documentation.md`](./documentation.md).
