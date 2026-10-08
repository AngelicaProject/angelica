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

`angelica-proxy` builds `dinput8.dll` for `x86_64-pc-windows-msvc`. It forwards the six system exports by name and ordinal (1–6): `DirectInput8Create`, `DllCanUnloadNow`, `DllGetClassObject`, `DllRegisterServer`, `DllUnregisterServer`, and `GetdfDIJoystick`. The system module is loaded lazily by its absolute system-directory path and retained until process exit.

Configuration is strict JSON with one field, an absolute installation path:

```json
{"installation_dir": "C:\\Angelica"}
```

The release build supplies `ANGELICA_RELEASE_PUBLIC_KEY` as 64 hexadecimal digits. This is a build-time public key, never a configuration-file value. A proxy built without it refuses every host and continues forwarding system exports. `runtime.dll.sig` contains exactly 64 raw signature bytes over the complete DLL file; verification uses strict Ed25519 verification. The DLL is opened without write or delete sharing and held open through verification, loading, and startup. Native dependencies are searched only in the system directory; the host cannot rely on unsigned sibling DLLs.

Diagnostics are appended to `angelica-loader.log` next to the proxy. A logging failure does not prevent game startup.

## Runtime startup ABI

The signed runtime exports `angelica_start_v1` with the Windows system calling convention and signature `int32_t angelica_start_v1(void)`. Zero means startup succeeded; any nonzero status means startup failed after removing every registered hook and callback. The proxy unloads a failed host. The host catches all panics inside this export; no exception or panic may cross the ABI. A successful host remains loaded until process exit. Plugin startup, crash reports, and the overlay belong to the host.

The proxy installs the entry hook only during static process initialization. Loading it dynamically into an already running process provides forwarding without host startup. The startup hook does not suspend threads or use the ordinary hook allocator's shared mutex. It retains an original-code trampoline, removes only its own jump after host startup, and continues through the trampoline even if restoration fails or another tool has installed a later hook.

## Failure behavior

- If the host fails before plugins load, it unloads its hooks and the game runs unmodified.
- The overlay does not depend on the game map (see [`ui.md`](./ui.md)), so the host can report in the game why it, a plugin, or an entry is disabled.
- A crash in the game process writes a minidump and a report naming the plugins and their states to the log folder.

The host's unhandled-exception filter attempts a normal Windows minidump and a JSON report with the executable and map hashes and each plugin's id, version, and state. Reading a plugin's state never waits on a lock; a state another thread holds is reported as unknown. The filter then forwards to the filter that preceded it, preserving the game's exception handling. The pinned host retains this callback until process exit. Reports are best effort if the filesystem or the crashed process cannot service the write; handled plugin traps do not produce process crash dumps.

## Updates

The host downloads updates while the game runs and stages them. The proxy loads the staged version at the next start. A file in use is never replaced.

The staging location is `<installation_dir>/staged/`, containing `runtime.dll` and `runtime.dll.sig`. The updater publishes the complete directory atomically. Its presence selects that version; missing files or an invalid signature disable Angelica for that launch rather than falling back to an older host. The proxy neither writes nor promotes updates. The updater removes or replaces a staging directory only when its files are no longer in use.

## Coexistence

Other tools may load into the same process before or after Angelica. Angelica does not unload, patch, or detect other tools; hook chaining is described in [`hooks.md`](./hooks.md).
