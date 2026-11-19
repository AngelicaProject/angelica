# Loading into the game process

## Proxy DLL

The game executable imports several system DLLs by name. Windows resolves a DLL that is not a KnownDLL from the application folder first, so a DLL with the same name in `game/` is loaded in its place. The proxy uses this; it needs no launcher and works with any way of starting the game.

- The proxy is `dinput8.dll`: the executable imports it, it is not a KnownDLL, and it has few exports. Graphics injectors commonly take `dxgi.dll` and `d3d11.dll`, so those names are never used.
- The proxy forwards every export to the system DLL of the same name, loaded by full path from the system directory.
- `DllMain` does nothing that can take the loader lock: it records its own module, queues the host's start to its own thread, and returns.
- Windows runs the queued start on the process's initial thread when process initialization ends: every library is initialized, the loader lock is released, and the executable's entry point has not run. The start loads `runtime.dll` and calls its start function; then the game's entry point runs.
- The proxy changes no code of the executable. A tool that rewrites the executable's entry point to load itself, or that compares the executable's code with the file, finds the entry point as it expects, and runs after the host has started.

The proxy changes rarely. It contains no game knowledge, so a game patch never requires a new proxy.

## Finding and trusting the host

The proxy reads `angelica.json` next to itself for the installation folder, loads `runtime.dll` of the current version from there (below), and checks its Ed25519 signature (`runtime.dll.sig`) against the release key compiled into the proxy before loading it. A missing or invalid host means the game starts without Angelica, and the reason is written to the log next to the proxy.

`angelica-proxy` builds `dinput8.dll` for `x86_64-pc-windows-msvc`. It forwards the six system exports by name and ordinal (1–6): `DirectInput8Create`, `DllCanUnloadNow`, `DllGetClassObject`, `DllRegisterServer`, `DllUnregisterServer`, and `GetdfDIJoystick`. The system module is loaded lazily by its absolute system-directory path and retained until process exit.

Configuration is strict JSON with one field, an absolute installation path:

```json
{"installation_dir": "C:\\Angelica"}
```

The release build supplies `ANGELICA_RELEASE_PUBLIC_KEY` as 64 hexadecimal digits. This is a build-time public key, never a configuration-file value. A proxy built without it refuses every host and continues forwarding system exports. `runtime.dll.sig` contains exactly 64 raw signature bytes over the complete DLL file; verification uses strict Ed25519 verification. The DLL is opened without write or delete sharing and held open through verification, loading, and startup. Native dependencies are searched only in the system directory; the host cannot rely on unsigned sibling DLLs.

Diagnostics are appended to `angelica-loader.log` next to the proxy, or in the installation or per-user log folder if the game folder is read-only. Each line carries UTC time to the millisecond, the game PID and elapsed milliseconds. The loader records configuration selection, signature validation, DLL loading, and the host ABI call and result. It starts logging after process initialization releases the loader lock. Failures before the proxy runs can appear only in external diagnostics. A logging failure does not prevent game startup.

## Runtime startup ABI

The signed runtime exports `angelica_start_v1` with the Windows system calling convention and signature `int32_t angelica_start_v1(void)`. Zero means startup succeeded; any nonzero status means startup failed after removing every registered hook and callback. The proxy unloads a failed host. The host catches all panics inside this export; no exception or panic may cross the ABI. A successful host remains loaded until process exit. Plugin startup, crash reports, and the overlay belong to the host.

The proxy queues the start only during static process initialization. Loading it dynamically into an already running process provides forwarding without host startup. When the start cannot be queued, the game starts without Angelica, and the log says so.

## Failure behavior

- If the host fails before plugins load, it unloads its hooks and the game runs unmodified.
- The overlay does not depend on the game map (see [`ui.md`](./ui.md)), so the host can report in the game why it, a plugin, or an entry is disabled.
- A host load failure after proxy initialization records the Windows error and loaded module paths. The proxy shows a Windows warning for the game process and points to the standalone report tool.
- A crash in the game process writes a minidump and a report naming the plugins and their states to the log folder.

The host's unhandled-exception filter attempts a normal Windows minidump and a JSON report with the executable and map hashes and each plugin's id, version, and state. Reading a plugin's state never waits on a lock; a state another thread holds is reported as unknown. The filter then forwards to the filter that preceded it, preserving the game's exception handling. The pinned host retains this callback until process exit. Reports are best effort if the filesystem or the crashed process cannot service the write; handled plugin traps do not produce process crash dumps.

`host-bootstrap.log` retains startup phases and `host.log` adds runtime events. Both use UTC timestamps and the game PID. The host logs map, sandbox and overlay initialization, activation policy, plugin lifecycle and background job outcomes. It logs the first game-thread frame and a periodic overlay heartbeat. These diagnostics record transitions and counts rather than frame-by-frame game data.

## Updates

The host downloads updates of its [channel](../development/releases.md#host-updates) while the game runs into a new version folder. The proxy loads it at the next start. A file in use is never replaced.

Each version of the host lives in its own folder, `<installation_dir>/versions/<version>/`, with `runtime.dll`, `plugin-installer.exe`, and their signatures. `<installation_dir>/current.json`, strict JSON `{"format": 1, "version": "0.2.0"}`, names the version the proxy loads; without it, the proxy loads `runtime.dll` from the installation folder itself. The updater writes a complete new version folder first and then replaces `current.json` by renaming a new file over it, so the running version's files are never touched. A present but malformed `current.json`, a version name that is a path, missing files, or an invalid signature disable Angelica for that launch rather than falling back to an older host. The proxy neither writes nor promotes updates. The updater removes a version folder only when it is neither current nor in use.

## In Wine

On Linux the game runs in Wine (Steam's Proton or the Wine of a launcher), and the same proxy and runtime load the same way. Two things differ:

- Wine prefers its own `dinput8.dll` to one in the application folder. The proxy loads only when the prefix's registry sets `dinput8` to `native,builtin` for the game's program; the [installer](./installer.md#on-linux) sets it. Without the setting the game starts without Angelica and writes no log.
- Wine's own `dinput8.dll` has no `GetdfDIJoystick`. A forwarded export the system DLL lacks returns the export's failure value and is logged; the game does not use it.

Wine runs the queued start before the game's entry point, as Windows does. `installation_dir` is a Windows path as the game sees it, under drive `Z:` for a folder of the Linux file system.

## Coexistence

Other tools may load into the same process before or after Angelica. Hook chaining is described in [`hooks.md`](./hooks.md).

Angelica checks for a competing plugin runtime before starting its plugins. It checks the launch environment for the injector's runtime marker and checks for the runtime's modules in the current game process at startup and on the main thread each frame. A launcher that skips the injector for an unsupported game build uses the direct launch path without the marker. Injector launches that leave the marker set are treated as competing launches for the session.

Detection puts Angelica in standby for the rest of the game session. Any running plugins go through the normal stop path, and all starts, reload activations, approvals and enable actions are blocked. Saved permissions and enabled choices carry over to the next launch. The Angelica menu shows the standby reason and keeps host settings available.

When standby is detected at startup, Angelica skips plugin discovery, the plugin installer, the catalog, and the game hooks that add chat and System menu access. The overlay keeps the menu key available. If standby starts later, the host stops polling for package and map reloads.
