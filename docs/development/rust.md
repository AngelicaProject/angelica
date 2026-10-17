# Rust engineering

## Design preferences

- Keep pure logic (PE parsing, map resolution, manifest checks, permission decisions) in crates without `unsafe` and without Windows dependencies, so it is tested on any machine.
- Keep code that touches game memory or Windows APIs in thin adapter modules.
- Use enums to make states and decisions exhaustive.
- Prefer owned values at plugin, thread, and component boundaries.
- Errors expected from data, the game build, or plugins are typed results.

## `unsafe`

`unsafe_code` is forbidden by workspace lint. It is allowed only in:

- `angelica-hook`;
- `angelica-proxy`;
- the `interop` module of `angelica-host`.
- the `system` module of `angelica-setup`, which only starts the installer again with administrator rights, reads the interface language, opens Windows' folder dialog, and lists the running programs' names on Windows; its Linux part has no `unsafe` code.

In those places:

- `clippy::undocumented_unsafe_blocks` is denied: every `unsafe` block has a `// SAFETY:` comment stating why its preconditions hold;
- every `unsafe fn` documents its preconditions in a `# Safety` section;
- an address used for access comes from a resolved map entry or from Windows, never from plugin input without a check against the plugin's permissions.

Adding `unsafe` to another crate or module requires changing this document in the same pull request.

## Panics and FFI

The crates that run in the game build with `panic = "unwind"` so that panics can be caught at every boundary with `std::panic::catch_unwind`. No panic crosses an `extern` function, a detour, or a callback into the game.

## Plugins

The SDK targets `wasm32-wasip2`. Plugin code is ordinary safe Rust; the SDK does not expose `unsafe` APIs.
