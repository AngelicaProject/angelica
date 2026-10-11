# Plugin file format 1

A plugin file (`<id>-<version>.angelica`) carries a plugin to players, from its author or from the [catalog](../architecture/catalog.md), whose files are signed with the release key. It holds the plugin's manifest and component, signed with the author's key. It never holds native code: the installer compiles the component on the player's computer.

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

The player is not shown the key. The menu shows that the plugin comes from its author, with the manifest's `authors`, or, when the release key signed the file, from the Angelica catalog.

## Links

A plugin is shared by a link: an `https` URL of its plugin file that always serves the newest version. `angelica plugin publish --github <owner/name>` signs the plugin and puts the file `<id>.angelica` into the release `<id>` of that GitHub repository, replacing the previous version, so the link stays `https://github.com/<owner>/<name>/releases/download/<id>/<id>.angelica`; it refuses a version that is not newer than the one the link serves, and a link that serves another author's file. Any other `https` host serves a plugin as well: the signature, not the host, makes a file trusted.

The installer downloads from any public `https` host, with the rules of the plugin API's [HTTP](./plugin-api-v1.md#http) otherwise (port 443, public addresses only, at most 5 redirects), at most the size of the largest plugin file.

## Installation

A player installs from a link in the Angelica menu, where the host runs the plugin installer outside the game ([plugins from their authors](../architecture/plugins.md#plugins-from-their-authors)); `angelica plugin install <file or link> --installation <folder>` does the same from the command line. Either installs a plugin file into a [host installation](./host-installation-v1.md):

1. It verifies the signature and the manifest.
2. The first installation of a plugin id binds the id to the author's key in `keys/authors/<plugin-id>.json`: `{"format": 1, "plugin_id": "timers", "author_key": "<lowercase hex>", "url": "<link>"}`, where `url` is the link the plugin was last installed from and is absent for a file. A file of the same id signed by another key is refused, and so is a plugin id already installed from the catalog.
3. It compiles the component with the runtime's engine and writes the package `plugins/<plugin-id>/`: the plugin file as it came (`plugin.angelica`), the compiled `plugin.cwasm`, and `files.json` signed with the installation key ([authenticated file sets](./host-installation-v1.md#authenticated-file-sets)).

## Updates

The host checks the link of every installed plugin that has one at game start and every 6 hours; `angelica plugin update --installation <folder>` does it at once. A file from the same author with the same plugin id and a newer `plugin.version` is installed as above; the same or an older version leaves the plugin as it is; another author's file or another plugin at the link is refused and reported. A plugin whose package folder the player removed is not brought back.

## Loading

The host loads such a package only when the installation key verifies `files.json`, the author's signature verifies the plugin file, and the author is the one the id is bound to; the manifest and component come from the plugin file. A new version replaces the package while the game runs; one that declares more waits for approval ([permissions](../architecture/permissions.md#grants)).
