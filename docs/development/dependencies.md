# Dependency policy and major dependencies

Angelica prefers a small set of mature dependencies over internal reinvention of generic infrastructure. A dependency needs a concrete current use, a maintenance story, and a compatible license. Remove unused dependencies instead of keeping speculative framework pieces.

## Approved foundations

- `wasmtime`: plugin runtime, component model, epoch interruption, ahead-of-time compilation.
- WIT and `wit-bindgen`: the plugin API definition and its bindings.
- `iced-x86`: x64 decoding and encoding for trampolines and code-anchored offsets.
- `windows`: Win32 and Direct3D 11 bindings.
- `egui`: the overlay's user interface, drawn by Angelica's own Direct3D 11 painter.

Everything that runs in the game is Rust or WebAssembly. A dependency is added with the first change that uses it, after checking its current stable version and license.

## Dependency checks

`cargo deny` (`deny.toml`) enforces the policy: a known vulnerability or a yanked crate fails, every license must be on the allow list of licenses compatible with AGPL-3.0-only, and every crate comes from crates.io.

## Licensing

- The host, loader, tools, and installer are AGPL-3.0-only.
- `wit/` and `angelica-sdk` are MIT, so plugins may use any license. Their directories carry their own license file.
- Code taken from another project keeps that project's license and attribution. Behavior reimplemented from a description of formats, protocols, or user experience carries no such obligation.
