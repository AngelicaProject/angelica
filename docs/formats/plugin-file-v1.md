# Plugin file format 1

A plugin file (`<id>-<version>.angelica`) carries a plugin from its author to players outside the official catalog. It holds the plugin's manifest and component, signed with the author's key. It never holds native code: the installer compiles the component on the player's computer.

## Layout

All integers are little-endian.

| Bytes | Content |
| --- | --- |
| 16 | `angelica-plugin\n` |
| 4 | format, `1` |
| 32 | the author's Ed25519 public key |
| 64 | the Ed25519 signature |
| 4 + n | the length and bytes of `angelica.toml`, at most 64 KiB |
| 4 + n | the length and bytes of the component (`plugin.wasm`), at most 64 MiB |

The signature covers every byte of the file except the signature itself, the author key included, and is verified strictly before anything else is read. Nothing may follow the component. A reader refuses another format.

## The author

The author key is the plugin's identity. `angelica plugin pack` signs with it and creates it on first use, by default in `%APPDATA%/Angelica/author.key` (64 hexadecimal digits of the secret). A lost key cannot sign new versions that installations accept; the author keeps it like a password.

The player is not shown the key. The menu shows that the plugin comes from its author, with the manifest's `authors`, and not from the Angelica catalog.

## Installation

`angelica plugin install <file> --installation <folder>` installs a plugin file into a [host installation](./host-installation-v1.md):

1. It verifies the signature and the manifest.
2. The first installation of a plugin id binds the id to the author's key in `keys/authors/<plugin-id>.json`: `{"format": 1, "plugin_id": "timers", "author_key": "<lowercase hex>"}`. A file of the same id signed by another key is refused, and so is a plugin id already installed from the catalog.
3. It compiles the component with the runtime's engine and writes the package `plugins/<plugin-id>/`: the plugin file as it came (`plugin.angelica`), the compiled `plugin.cwasm`, and `files.json` signed with the installation key ([authenticated file sets](./host-installation-v1.md#authenticated-file-sets)).

The host loads such a package only when the installation key verifies `files.json`, the author's signature verifies the plugin file, and the author is the one the id is bound to; the manifest and component come from the plugin file. A new version replaces the package while the game runs; one that declares more waits for approval ([permissions](../architecture/permissions.md#grants)).
