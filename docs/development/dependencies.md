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

`angelica-cli` uses `clap` (MIT OR Apache-2.0) for its arguments, `iced-x86` with its Intel formatter for `angelica exe`, `serde_json` (MIT OR Apache-2.0) for `--json` output, `sha2` for `exe_sha256`, `ed25519-dalek` with `getrandom` 0.4.3 (MIT OR Apache-2.0) to create keys and sign installations and releases, `angelica-update` and `semver` for update feeds, and `toml` with `serde` to read catalog entries.

Tests use `tempfile` (MIT OR Apache-2.0) for temporary directories.

`angelica-proxy` uses `ed25519-dalek` 3.0.0 (BSD-3-Clause), maintained by the dalek cryptography project, for strict verification of the native runtime's release signature; `serde` and `serde_json` for the installation configuration; `thiserror` for failures; and `windows-sys` for system DLL forwarding and host loading.

`angelica-host` uses wasmtime 49.0.2 (Apache-2.0 WITH LLVM-exception) with runtime, Cranelift, and component-model features; semver 1.0.28 (MIT OR Apache-2.0) for manifests; `getrandom` for the random numbers plugins receive through WASI; and the existing signature, hash, and serialization foundations. Its egui 0.36.2 dependency (MIT OR Apache-2.0) disables egui's bundled default fonts; the host bundles its own ([fonts](#fonts)). `windows` 0.62.2 (MIT OR Apache-2.0) supplies owned Direct3D COM interfaces alongside `windows-sys`. `num-traits` 0.2.19 (MIT OR Apache-2.0) provides checked float-to-integer clipping conversion. `ureq` 3.4.2 (MIT OR Apache-2.0), without its TLS features and with `gzip`, sends plugins' and Angelica's HTTPS requests, through an HTTP proxy with its `CONNECT` connector when Windows names one (the Windows adapter asks WinHTTP through `windows-sys` for the user's proxy, automatic detection, and configuration script); the host supplies its resolver, which enforces the approved hosts and the addresses a client may reach, and its TLS connector over `native-tls` 0.2.18 (MIT OR Apache-2.0), which is SChannel and the system's certificate store on Windows. ureq's resolver and transport interfaces are outside its semantic versioning, so the version is pinned exactly. ureq's own TLS features are not used: they bundle Mozilla's root certificates under a license outside the allow list. `angelica-text` uses `sha2` for guards and `thiserror`. The SDK uses wit-bindgen 0.62.0 (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT), and `serde` with `serde_json` to pass jobs' inputs and outputs, save values in a plugin's storage, and decode JSON responses.

`angelica-setup`, the installer, uses eframe 0.36.2 (MIT OR Apache-2.0) with only its `glow` renderer for its one window, with the same egui and the host's fonts, `windows-sys` to start itself with administrator rights, and `windows` 0.62.2 for the COM interfaces of Windows' folder dialog; it reads and writes the registry through the system's `reg.exe`. `angelica-update` uses `ed25519-dalek`, `semver` with `serde`, `serde_json`, `sha2`, and `thiserror` for update feeds and packs.

`angelica-sqpack` adapts the AGPL-3.0-only archive and Excel decoder from Aeria with attribution and original file hashes in [NOTICE.md](../../crates/angelica-sqpack/NOTICE.md). It uses crc32fast 1.5.2 and flate2 1.1.10 (both MIT OR Apache-2.0) for archive path hashes and DEFLATE blocks; flate2 uses its pure Rust backend, so no native compression library is loaded.

## Fonts

`crates/angelica-host/fonts/` holds the fonts the host draws its interface with, each under the SIL Open Font License 1.1 with its license file beside it:

| File | Font | Source |
| --- | --- | --- |
| `GolosText-Regular.ttf`, `GolosText-Medium.ttf`, `GolosText-SemiBold.ttf` | Golos Text, weights 400, 500, and 600 | `ofl/golostext` in the Google Fonts repository |
| `Quicksand-SemiBold.ttf` | Quicksand, weight 600 | `ofl/quicksand` |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono, weight 400 | `ofl/jetbrainsmono` |

Each file is a static instance of the variable font at that weight, cut with fontTools (`varLib.instancer`, then `subset` with only the `kern` feature and no hinting) to U+0020–007E, U+00A0–017F, U+0400–04FF, U+2000–206F, U+20AC, U+2116, U+2122, U+2190–2193, U+2212, and U+2264–2265. A test checks that every interface string in both languages has its glyphs. Fonts are not code and are outside `cargo deny`; a new font needs a license on the same terms and its license file.

## Dependency checks

`cargo deny` (`deny.toml`) enforces the policy: a known vulnerability or a yanked crate fails, every license must be on the allow list of licenses compatible with AGPL-3.0-only, and every crate comes from crates.io.

## Licensing

- The host, loader, tools, and installer are AGPL-3.0-only.
- `wit/`, `angelica-sdk`, and `angelica-text` are MIT, so plugins may use any license. Their directories carry their own license file.
- Code taken from another project keeps that project's license and attribution. Behavior reimplemented from a description of formats, protocols, or user experience carries no such obligation.
