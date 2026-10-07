# Architecture overview

```text
┌──────────────────────────── game process ────────────────────────────┐
│ proxy DLL ── loads ──> host (runtime.dll)                            │
│                         ├─ hook engine        (angelica-hook)        │
│                         ├─ game map resolver  (angelica-map, -pe)    │
│                         ├─ plugin runtime     (wasmtime components)  │
│                         │    └─ plugins: sandboxed, per-plugin       │
│                         │       permissions, main + worker instances │
│                         ├─ host services      (files, Excel, UI, …)  │
│                         └─ overlay            (egui on D3D11)        │
└──────────────────────────────────────────────────────────────────────┘
         ▲ verified map (signed feed)            ▲ signed plugins
   angelica-gamemap repository             angelica-registry repository
         ▲                                       ▲
   `angelica map …` tools (this repository)   plugin sources
```

## Repositories

| Repository | Owns |
| --- | --- |
| `angelica` | All code: loader, host, hook engine, map resolver, plugin API (`wit/`), SDK, installer, the `angelica` command-line tool. All shared contracts: [game map format](../formats/game-map-v1.md), [plugin manifest](../formats/plugin-manifest-v1.md), and the plugin API. |
| `angelica-gamemap` | Game map data: entries, build records, verification reports, provenance of imported data. It contains no code; its CI runs the `angelica` tool at a pinned version. |
| `angelica-registry` | The plugin catalog: which plugin source commits are published, the build that produced them, and the signed index. |
| plugin repositories | One per plugin, owned by its author. |

## Authority

- **Host** is authoritative for what runs in the game: which map entries are present, which plugins load, which permissions they hold, and the order of hooks.
- **Game map** is authoritative for where things are in a given game build. The host never derives an address that the map does not describe.
- **Plugin** is authoritative only for its own state and for the rules it gives host services.
- **Registry** is authoritative for which plugin builds are published and signed.

## Crates

Each crate owns one boundary:

- `angelica-pe`: PE image model over bytes, from a file on disk or from the mapped module, and signature scanning. No `unsafe`.
- `angelica-map`: the game map format, resolution of entries against a PE image, offset derivation, and verification reports. Used by the host in the game and by the tools offline, so the same code decides both. No `unsafe`.
- `angelica-hook`: x64 hooking: entry detours, mid-function hooks, code patches, vtable slot replacement, and chaining with hooks installed by other tools. `unsafe` allowed.
- `angelica-proxy`: the proxy DLL placed in the game folder. `unsafe` allowed.
- `angelica-host`: `runtime.dll`: start-up, the plugin runtime, permissions, host services, and the overlay. `unsafe` only in its `interop` module.
- `angelica-sdk`: the Rust crate plugin authors use: generated bindings for `wit/`, typed wrappers for map entries, and test helpers.
- `angelica-fake-game`: a test host for plugins: implements the plugin API over recorded or scripted data.
- `angelica-cli`: the `angelica` command: `map` (verify, recover, derive, diff, import), `plugin` (new, build, test, dev, pack), and installation checks. Every command can report in JSON.
- `angelica-installer`: places, repairs, and removes the proxy DLL in the game folder.

## Data flow at game start

1. The game loads the proxy DLL with its imports.
2. The proxy loads `runtime.dll` from the installation folder after checking its signature.
3. The host computes the executable's hash and selects the map build record for it. Without a record, it resolves entries from their locators and verifies each one; see [`game-map.md`](./game-map.md).
4. The host loads installed plugins whose required entries are present, instantiates them, and binds their hooks and services.
5. The game's entry point runs. Hooks are in place before the game reads any data.
