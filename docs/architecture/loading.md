# Loading into the game process

## Proxy DLL

The game executable imports several system DLLs by name. Windows resolves a DLL that is not a KnownDLL from the application folder first, so a DLL with the same name in `game/` is loaded in its place. The proxy uses this; it needs no launcher and works with any way of starting the game.

- The proxy is `dinput8.dll`: the executable imports it, it is not a KnownDLL, and it has few exports. Graphics injectors commonly take `dxgi.dll` and `d3d11.dll`, so those names are never used.
- The proxy forwards every export to the system DLL of the same name, loaded by full path from the system directory.
- `DllMain` does nothing that can take the loader lock: it records its own module, installs a one-time hook on the executable's entry point, and returns.
- The entry point hook loads `runtime.dll`, calls its start function, removes itself, and continues into the game's entry point.

The proxy changes rarely. It contains no game knowledge, so a game patch never requires a new proxy.

## Finding and trusting the host

The proxy reads `angelica.json` next to itself for the installation folder, loads `runtime.dll` from there, and checks its Ed25519 signature (`runtime.dll.sig`) against the release key compiled into the proxy before loading it. A missing or invalid host means the game starts without Angelica, and the reason is written to the log next to the proxy.

## Failure behavior

- If the host fails before plugins load, it unloads its hooks and the game runs unmodified.
- The overlay does not depend on the game map (see [`ui.md`](./ui.md)), so the host can report in the game why it, a plugin, or an entry is disabled.
- A crash in the game process writes a minidump and a report naming the active plugins and hooks to the log folder.

## Updates

The host downloads updates while the game runs and stages them. The proxy loads the staged version at the next start. A file in use is never replaced.

## Coexistence

Other tools may load into the same process before or after Angelica. Angelica does not unload, patch, or detect other tools; hook chaining is described in [`hooks.md`](./hooks.md).
