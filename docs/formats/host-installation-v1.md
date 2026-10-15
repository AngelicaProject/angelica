# Host installation format 1

The installation folder contains `current.json` and the version folders it chooses from, `versions/<version>/`, each with `runtime.dll` and its detached release signature, as specified in [loading](../architecture/loading.md#updates); an installation without `current.json` has them in the installation folder itself. Beside the runtime are `plugin-installer.exe` from the same build and its detached release signature `plugin-installer.exe.sig` (64 raw Ed25519 bytes over the file), which the host runs to install [plugins from their authors](../architecture/plugins.md#plugins-from-their-authors).

## Configuration

`host.json` optionally configures the runtime. It has `format: 1` and absolute `map_directory`, `plugin_directory`, `storage_directory`, `grants_directory`, and `log_directory` paths, and optionally absolute `state_directory`, `cache_directory`, and `keys_directory` paths. Without the file these are `map`, `plugins`, `storage`, `grants`, `logs`, `state`, `cache`, and `keys` under the installation folder.

The host writes `host.log` in the log folder, one line per event, each after the local time (`2026-10-13 17:54:03.120`). Every start begins with a `start:` line naming Angelica's version and channel, the game's version and executable hash, and the process; no line names the installation's path or the Windows user. Lines follow for the game map, the hooks a plugin asks for (in place or why not), the overlay (the graphics adapter, its driver, the device, and the game's swap chain when it attaches; a resize of the swap chain; why it stopped), plugins loaded and stopped, what the plugin installer did, Angelica and map updates installed or failed (a check that keeps failing the same way is logged once), a catalog index that could not be read, and for each host Angelica's requests reach, whether it goes through a proxy (which) or directly, once until that changes. A log over 2 MiB at start becomes `host.old.log`, replacing the one before.

`diagnostics.json` beside `host.json`, when present, turns parts of Angelica off to find which one a problem comes from, one start at a time, without another build: `overlay` is `on` (the default), `hook` (the overlay hooks `Present` and passes every frame on, making and drawing nothing), or `off` (no Direct3D device and no hook); `updates: false` stops Angelica, map, plugin, and catalog checks over the network; `plugins: false` loads no installed plugin. Unknown fields or values make the file count as absent, which the log says; the start line is followed by what the file turned off.

Optional limits:

| Field | Default | Range | Meaning |
| --- | --- | --- | --- |
| `memory_bytes` | 67108864 | 1–1073741824 | memory of a plugin instance whose manifest requests none |
| `fuel` | 100000000 | 1–10000000000 | guest instructions for the calls of one frame; 100 times as many for `start` and `stop` |
| `timeout_ms` | 20 | 1–1000 | guest time for the calls of one frame |

Unknown fields and other formats are refused.

## Authenticated file sets

Map releases and plugin packages contain `files.json` and `files.json.sig`. The signature is exactly 64 raw Ed25519 bytes over the exact bytes of `files.json`, verified with the compiled release public key before the JSON is parsed; a plugin package may instead be signed with the [installation key](#installation-key):

```json
{"format":1,"files":{"angelica.toml":"<lowercase SHA-256>","plugin.wasm":"<lowercase SHA-256>","plugin.cwasm":"<lowercase SHA-256>"}}
```

Each listed file is read and its SHA-256 checked; only these bytes reach the map reader or the component loader. A plugin package holds `angelica.toml`, `plugin.wasm`, and `plugin.cwasm`, in a folder named by the plugin id. `angelica dev install` also adds a `development` file, whose contents do not matter: the host lists such a plugin as in development, with a striped tile and a `dev` token after its name. Release packages never hold one. A map package holds `map.toml`, its `entries/*.toml`, and optionally `domains.toml`, `roots.toml`, `types/*.toml`, `enums/*.toml`, `sheets/**/*.toml`, `windows/*.toml`, and `builds/*.toml`.

`plugin.cwasm` is the component compiled by `angelica plugin compile` from the packaged `plugin.wasm`. It is native code: the signer must compile it from the source itself and never sign an artifact a plugin author supplied. The compiler version, engine settings, architecture, operating system, and CPU features must match the runtime; otherwise the package is refused, and the game never compiles a component.

Paths use forward slashes and are relative. Traversal, empty parts, links, Windows device names, alternate streams, and trailing dots or spaces are refused. The file list is at most 1 MiB and 8192 files; each file at most 64 MiB, all together at most 256 MiB.

## Installation key

`keys/installation.key` (the secret) and `keys/installation.pub` (the public half), each 64 hexadecimal digits, sign the packages the installer makes from [plugin files](./plugin-file-v1.md) of plugins from their authors. The installer creates them on first use; a host started before that reads the public half when it next looks for new packages, and never replaces a key it already read. The host reads only the public half and accepts it only for plugin packages, never for the runtime or the map; such a package holds `plugin.angelica` and `plugin.cwasm`, and the host also verifies the author's signature and the author bound to the plugin id in `keys/authors/`.

The installation key keeps packages from being swapped or damaged; it does not protect against a program running as the same Windows user, which can sign with it and could change the game process directly anyway. The boundary is the player's account. Native code never comes from an author: the installer compiles every component itself.

## Player grants

`grants/<plugin-id>.json` records the player's approval:

```json
{"format": 1, "plugin_id": "harmonia", "permissions": ["files.replace", "http.get"], "http_hosts": ["harmonia.example.org"], "lens_read": ["inventory"], "lens_act": ["actions"], "lens_change": ["camera"]}
```

`http_hosts` lists the approved hosts of `http.get`, `message_to` the approved receivers of `messages.send`, `cache_gib` the approved size of the cache of `storage.cache`, `lens_read` the domains of the game map approved for reading, `lens_act` those approved for acting in, and `lens_change` those approved for changing; each is left out when empty. A plugin runs only when its grant covers every permission, every `http.hosts` entry, and every `messages.to` entry its manifest declares, a cache no larger than the approved one, every domain of its `[lens] read` that the running game map describes in a class the host presents, every domain of its `[lens] act` that the map describes with words for acting, and every domain of its `[lens] change` that the map describes with words for changing. Without the file the plugin is not approved. The Angelica menu writes the grant when the player approves the plugin, through a temporary file and a rename.

## Lens requests

A development build of the runtime, and any build in [dev mode](#dev-mode), answers `angelica look` through the `look` folder of the installation: the command writes `look/request.json`, `{"format": 1, "id": "<random>", "query": "<query>"}`, and the host, between two frames, removes it, runs the [query](./plugin-api-v1.md#lens) with every domain readable, and writes `look/response.json` with the same `id` and either `columns` (each with `name`, `field`, `doc`, `domain`) and `rows` (each with `indexes` and `values` as JSON: numbers, booleans, plain text, `{"sheet", "row"}`, names, `{"names", "bits"}`, or `null`), or `error` with `kind` and `message`. Fields no plugin reads stay unread. A release build outside dev mode never reads the `look` folder.

A request may also have `mode`, `rows` by default:

- `watch`: `query` is a [watch](./plugin-api-v1.md#lens) and `seconds` (1 to 60) how long it lasts; the response has `columns`, `firings` (the rows of each firing), and `dropped`.
- `dump`: `query` is a path to objects and `bytes` (1 to 4096) how much of each object to read; with `follow`, up to 8 offsets, the host first follows from each object the pointer at each offset in turn, for memory the map does not describe yet. The response has `objects`, each with `indexes`, `address` (where the bytes were read, or 0 when a followed pointer was null or unreadable), and `bytes` as lowercase hexadecimal, or `null` where the memory cannot be read.
- `trace`: `entries` lists up to 64 `function` and `location` entries and `seconds` (1 to 60) how long to count; the response has `calls`, the count of each entry.
- `writes`: `query` is a path that reaches exactly one object, `follow` as for `dump`, `offset` the field's offset in the object reached, `size` its size (1, 2, 4, or 8 bytes; the address a multiple of it), and `seconds` (1 to 60) how long to watch. The host sets a hardware data breakpoint on the field in every thread the game has, counts where each writing instruction ended, and removes the breakpoint when the seconds are over; a thread started meanwhile is not watched. The response has `watched` (the address), `writes` (each with `after`, the address where the instruction ended, `offset` in the game's executable when it is there, and `count`, most first), and `missed`, the writes from places past the first 64. It is refused while another `writes` runs, while a debugger is attached, or when every debug register is in use.

The host answers a `watch`, a `trace`, or a `writes` when its seconds are over.

## Dev mode

Dev mode is for work on the game map on the player's own Angelica: it lets `angelica look` read the running game and lets a local, unsigned game map take the signed map's place. It never changes what a plugin may do: plugins keep their grants and domains, and no plugin can reach the `dev` folder.

`dev/mode.json` holds `{"format": 1, "on": true}` while dev mode is on; without the file, or with anything else in it, dev mode is off. The **Dev mode** switch under **For developers** and `angelica dev mode on|off --installation <folder>` write it through a temporary file and a rename. The host reads it at start and every second after, and logs each change. While dev mode is on, a note on the screen says so ([interface](../architecture/ui.md)).

`dev/map/` holds a local game map, written by `angelica map try`: the map's files and `files.json` with the SHA-256 of each, as a [file set](#authenticated-file-sets) without `files.json.sig`. While dev mode is on and `dev/map/files.json` exists, the host loads the local map instead of the signed one, every file checked against its hash, and its build records are used as a signed map's are; **For developers** names the map as local. When dev mode turns off, or the local map is removed, the signed map is loaded again. Signed map updates keep installing into the map folder meanwhile.

## Plugin settings

`state/<plugin-id>.json` holds `{"format": 1, "enabled": false}` when the player turned the plugin off; without the file an approved plugin is enabled. Faults do not change it. An unreadable file or another format refuses the plugin.

## Angelica settings

`state/_angelica.json` holds the player's choices for Angelica itself: `{"format": 1, "language": "ru", "theme": "dark", "size": "small", "key": "F9", "stop_key": "End"}`, where `language` is `en` or `ru`, `theme` is `light` or `dark`, `size` is `small`, `normal`, or `large`, `key` is the menu key (`Insert`, `Delete`, `Home`, `End`, `Page Up`, `Page Down`, `Pause`, `Scroll Lock`, or `F1` to `F12`), `stop_key` the key that stops plugins acting, one of the same keys and not the menu key, and `channel` is the [update channel](../development/releases.md#channels), `stable` or `nightly`, which the installer writes; each is left out when nobody has chosen one. No plugin id begins with `_`, so the file never meets a plugin's settings. The menu writes it through a temporary file and a rename; a missing, unreadable, or other-format file means the defaults.

## Storage

`storage/<plugin-id>/` is the plugin's folder ([storage](./plugin-api-v1.md#storage)).

## Cache

`cache/files/` holds the replacement game files plugins activated, named by the first 16 bytes of their SHA-256 in hex and their extension. The game reads them from there with its own disk reader, which takes paths of at most 259 characters; a cache path that would be longer refuses replacements. Files there are never trusted as input: each one is checked again when a plugin activates it.

`cache/plugins/<plugin-id>/` is the plugin's [cache](./plugin-api-v1.md#storage), with the size its grant approves; it is removed with the plugin.

## Development installations

`angelica dev key` creates a development signing key, and `angelica dev install` signs a runtime, the plugin installer, a map, and plugins with it into an installation folder (the runtime and the plugin installer into a new version folder `versions/dev-<seconds>/`, which `current.json` then names, so it can be installed while the game runs and takes effect at the next start). The proxy and runtime must be built with that key's public half in `ANGELICA_RELEASE_PUBLIC_KEY`. A development key is never a release key.
