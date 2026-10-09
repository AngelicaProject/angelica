# Dependency policy and major dependencies

Angelica prefers a small set of mature dependencies over internal reinvention of generic infrastructure. A dependency needs a concrete current use, a maintenance story, and a compatible license. Remove unused dependencies instead of keeping speculative framework pieces.

## Approved foundations

- `wasmtime`: plugin runtime, component model, epoch interruption, ahead-of-time compilation.
- WIT and `wit-bindgen`: the plugin API definition and its bindings.
- `iced-x86`: x64 decoding and encoding for trampolines and code-anchored offsets.
- `windows-sys`: Win32 and Direct3D 11 bindings.
- `egui`: the overlay's user interface, drawn by Angelica's own Direct3D 11 painter.

Everything that runs in the game is Rust or WebAssembly. A dependency is added with the first change that uses it, after checking its current stable version and license.

## Crate dependencies

`angelica-pe` uses `memchr` (Unlicense OR MIT) to find the fixed bytes of a signature, and `thiserror` (MIT OR Apache-2.0) for its error types.

`angelica-map` uses `toml` and `serde` (MIT OR Apache-2.0) to read map files, `iced-x86` (MIT) with only `std` and `decoder` to decode the instructions of code-anchored offsets and leaf functions, and `sha2` (MIT OR Apache-2.0) for `entries_sha256`.

`angelica-hook` uses `iced-x86` with `encoder`, `block_encoder`, and `instr_info` to move instructions into trampolines, and `windows-sys` (MIT OR Apache-2.0) for memory protection, executable memory, and thread suspension.

`angelica-cli` uses `clap` (MIT OR Apache-2.0) for its arguments, `serde_json` (MIT OR Apache-2.0) for `--json` output, `sha2` for `exe_sha256`, and `ed25519-dalek` with `getrandom` 0.4.3 (MIT OR Apache-2.0) to create development keys and sign development installations.

Tests use `tempfile` (MIT OR Apache-2.0) for temporary directories.

`angelica-proxy` uses `ed25519-dalek` 3.0.0 (BSD-3-Clause), maintained by the dalek cryptography project, for strict verification of the native runtime's release signature; `serde` and `serde_json` for the installation configuration; `thiserror` for failures; and `windows-sys` for system DLL forwarding and host loading.

`angelica-host` uses wasmtime 49.0.2 (Apache-2.0 WITH LLVM-exception) with runtime, Cranelift, and component-model features; semver 1.0.28 (MIT OR Apache-2.0) for manifests; `getrandom` for the random numbers plugins receive through WASI; and the existing signature, hash, and serialization foundations. Its egui 0.36.2 dependency (MIT OR Apache-2.0) disables bundled default fonts and uses installed Windows fonts. `windows` 0.62.2 (MIT OR Apache-2.0) supplies owned Direct3D COM interfaces alongside `windows-sys`. `num-traits` 0.2.19 (MIT OR Apache-2.0) provides checked float-to-integer clipping conversion. `angelica-text` uses `sha2` for guards and `thiserror`. The SDK uses wit-bindgen 0.62.0 (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT).

`angelica-sqpack` adapts the AGPL-3.0-only archive and Excel decoder from Aeria with attribution and original file hashes in [NOTICE.md](../../crates/angelica-sqpack/NOTICE.md). It uses crc32fast 1.5.2 and flate2 1.1.10 (both MIT OR Apache-2.0) for archive path hashes and DEFLATE blocks; flate2 uses its pure Rust backend, so no native compression library is loaded.

## Dependency checks

`cargo deny` (`deny.toml`) enforces the policy: a known vulnerability or a yanked crate fails, every license must be on the allow list of licenses compatible with AGPL-3.0-only, and every crate comes from crates.io.

## Licensing

- The host, loader, tools, and installer are AGPL-3.0-only.
- `wit/`, `angelica-sdk`, and `angelica-text` are MIT, so plugins may use any license. Their directories carry their own license file.
- Code taken from another project keeps that project's license and attribution. Behavior reimplemented from a description of formats, protocols, or user experience carries no such obligation.
