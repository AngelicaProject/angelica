# Architecture overview

```text
┌──────────────────────────── game process ────────────────────────────┐
│ proxy DLL ── loads ──> host (runtime.dll)                            │
│                         ├─ hook engine        (angelica-hook)        │
│                         ├─ game map resolver  (angelica-map, -pe)    │
│                         ├─ plugin runtime     (wasmtime components)  │
│                         │    └─ plugins: sandboxed, per-plugin       │
│                         │       permissions, main + job instances    │
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
- `angelica-sqpack`: read-only access to the installed game's archives and Excel sheets. No `unsafe`.
- `angelica-text`: the game's encoded strings and the [text table](../formats/text-table-v1.md) format, shared by the host and plugins. No `unsafe`.
- `angelica-sdk`: the Rust crate plugin authors use: bindings for `wit/`, the `Plugin` trait, and helpers.
- `angelica-fake-game`: runs plugins in the real host against a scripted game.
- `angelica-cli`: the `angelica` command: `map verify`, `excel` to explore the installed game's sheets, `exe` to disassemble the executable and propose signatures, `plugin new` to create a plugin project, `plugin compile`, and `dev` for signed local installations. Every command can report in JSON.
- `angelica-installer`: places, repairs, and removes the proxy DLL in the game folder.

## Data flow at game start

1. The game loads the proxy DLL with its imports.
2. The proxy loads `runtime.dll` from the installation folder after checking its signature.
3. The host computes the executable's hash and selects the map build record for it. Without a record, it resolves entries from their locators and verifies each one; see [`game-map.md`](./game-map.md).
4. The host loads the authenticated plugin packages and starts the approved, enabled ones. Rules they activate in `start` install the adapters they need.
5. The game's entry point runs. Every row the game builds already goes through the active rules.

## Inside the host

`angelica-host` separates the platform-independent host from the Windows adapter:

- `host` owns installed plugins, their lifecycle, grants, settings, and the work of each frame; `sandbox` and `jobs` own wasmtime stores, the imports, and per-call limits; `storage` the plugins' folders; `ui` and `panel` the windows and the Angelica menu.
- `game_data` reads the installed game's files and sheets through `angelica-sqpack` and caches decoded sheets.
- `excel_text` and `text_case` own the rules plugins activate and apply them to bytes: rewriting a row buffer, converting a string's letters. They read no game memory.
- `game::Game` is the boundary to the game process: the version and the adapters a service needs.
- `interop` (Windows only, the one module with `unsafe`) resolves map entries, installs the row and letter case detours through `angelica-hook`, reads and writes the game structures [their contracts](../formats/game-structures.md) describe, draws the overlay through a private deferred Direct3D 11 context, and writes crash reports. Every detour catches panics and passes the call on unchanged on failure.

`angelica-fake-game` implements `Game` from scripted data and drives rows and strings through the same `excel_text` and `text_case` code, so plugins and the host are tested without Windows or the game.
