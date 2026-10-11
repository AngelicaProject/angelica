# Host installation format 1

The installation folder contains `runtime.dll` and its detached release signature, as specified in [loading](../architecture/loading.md), or a newer pair in `staged/`. Beside the runtime are `plugin-installer.exe` from the same build and its detached release signature `plugin-installer.exe.sig` (64 raw Ed25519 bytes over the file), which the host runs to install [plugins from their authors](../architecture/plugins.md#plugins-from-their-authors).

## Configuration

`host.json` optionally configures the runtime. It has `format: 1` and absolute `map_directory`, `plugin_directory`, `storage_directory`, `grants_directory`, and `log_directory` paths, and optionally absolute `state_directory`, `cache_directory`, and `keys_directory` paths. Without the file these are `map`, `plugins`, `storage`, `grants`, `logs`, `state`, `cache`, and `keys` under the installation folder.

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

Each listed file is read and its SHA-256 checked; only these bytes reach the map reader or the component loader. A plugin package holds `angelica.toml`, `plugin.wasm`, and `plugin.cwasm`, in a folder named by the plugin id. `angelica dev install` also adds a `development` file, whose contents do not matter: the host lists such a plugin as in development, with a striped tile and a `dev` token after its name. Release packages never hold one. A map package holds `map.toml`, its `entries/*.toml`, and optionally `builds/*.toml`.

`plugin.cwasm` is the component compiled by `angelica plugin compile` from the packaged `plugin.wasm`. It is native code: the signer must compile it from the source itself and never sign an artifact a plugin author supplied. The compiler version, engine settings, architecture, operating system, and CPU features must match the runtime; otherwise the package is refused, and the game never compiles a component.

Paths use forward slashes and are relative. Traversal, empty parts, links, Windows device names, alternate streams, and trailing dots or spaces are refused. The file list is at most 1 MiB and 8192 files; each file at most 64 MiB, all together at most 256 MiB.

## Installation key

`keys/installation.key` (the secret) and `keys/installation.pub` (the public half), each 64 hexadecimal digits, sign the packages the installer makes from [plugin files](./plugin-file-v1.md) of plugins from their authors. The installer creates them on first use. The host reads only the public half and accepts it only for plugin packages, never for the runtime or the map; such a package holds `plugin.angelica` and `plugin.cwasm`, and the host also verifies the author's signature and the author bound to the plugin id in `keys/authors/`.

The installation key keeps packages from being swapped or damaged; it does not protect against a program running as the same Windows user, which can sign with it and could change the game process directly anyway. The boundary is the player's account. Native code never comes from an author: the installer compiles every component itself.

## Player grants

`grants/<plugin-id>.json` records the player's approval:

```json
{"format": 1, "plugin_id": "harmonia", "permissions": ["excel.text", "http.get", "text.case"], "http_hosts": ["harmonia.example.org"]}
```

`http_hosts` lists the approved hosts of `http.get` and is left out when there are none. A plugin runs only when its grant covers every permission and every `http.hosts` entry its manifest declares. Without the file the plugin is not approved. The Angelica menu writes the grant when the player approves the plugin, through a temporary file and a rename.

## Plugin settings

`state/<plugin-id>.json` holds `{"format": 1, "enabled": false}` when the player turned the plugin off; without the file an approved plugin is enabled. Faults do not change it. An unreadable file or another format refuses the plugin.

## Angelica settings

`state/_angelica.json` holds the player's choices for Angelica itself: `{"format": 1, "language": "ru", "theme": "dark", "size": "small", "key": "F9"}`, where `language` is `en` or `ru`, `theme` is `light` or `dark`, `size` is `small`, `normal`, or `large`, `key` is the menu key (`Insert`, `Delete`, `Home`, `End`, `Page Up`, `Page Down`, `Pause`, `Scroll Lock`, or `F1` to `F12`), and each is left out when the player has not chosen one. No plugin id begins with `_`, so the file never meets a plugin's settings. The menu writes it through a temporary file and a rename; a missing, unreadable, or other-format file means the defaults.

## Storage

`storage/<plugin-id>/` is the plugin's folder ([storage](./plugin-api-v1.md#storage)).

## Cache

`cache/files/` holds the replacement game files plugins activated, named by the first 16 bytes of their SHA-256 in hex and their extension. The game reads them from there with its own disk reader, which takes paths of at most 259 characters; a cache path that would be longer refuses replacements. Files there are never trusted as input: each one is checked again when a plugin activates it.

## Development installations

`angelica dev key` creates a development signing key, and `angelica dev install` signs a runtime, the plugin installer, a map, and plugins with it into an installation folder (the runtime and the plugin installer into `staged/`). The proxy and runtime must be built with that key's public half in `ANGELICA_RELEASE_PUBLIC_KEY`. A development key is never a release key.
