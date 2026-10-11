# Update feed format 1

An update feed tells an installed Angelica what the newest release of a channel is ([releases](../development/releases.md)). Angelica's releases publish `channel.json`; the game map's releases publish `map.json` in the same format.

## Files

`channel.json` (or `map.json`) is UTF-8 JSON; `channel.json.sig` (or `map.json.sig`) is exactly 64 raw Ed25519 bytes over the exact bytes of the JSON file, made with the release key. A reader verifies the signature with the release public key compiled into it before parsing the JSON, and refuses a feed whose signature does not verify.

```json
{
  "format": 1,
  "product": "angelica",
  "channel": "stable",
  "version": "0.2.0",
  "published": "2026-10-20T12:00:00Z",
  "files": {
    "runtime.dll": {"asset": "runtime.dll", "size": 9123456, "sha256": "<lowercase SHA-256>"},
    "plugin-installer.exe": {"asset": "plugin-installer.exe", "size": 4567890, "sha256": "<lowercase SHA-256>"}
  }
}
```

| Field | Meaning |
| --- | --- |
| `format` | `1` |
| `product` | `angelica` for the host, `map` for the game map |
| `channel` | `stable` or `nightly`; a map feed is always `stable` |
| `version` | the release's [SemVer](https://semver.org/) version |
| `published` | when the release was built, RFC 3339 in UTC |
| `files` | each installed file by its name: the release asset that holds it, its size in bytes, and its SHA-256 |

An Angelica feed lists `runtime.dll`, `plugin-installer.exe`, and `dinput8.dll`, each with its `.sig` asset beside it in the release; a map feed lists `map.pack` ([below](#map-pack)). Assets are downloaded from the same release as the feed: `https://github.com/<repository>/releases/download/<tag>/<asset>`, where `<tag>` is `v<version>` for stable and `nightly` for nightly.

A reader refuses a feed of another format, product, or channel, a version that is not valid SemVer, a file name that is not a plain file name, and a size above 256 MiB. It installs a file only when the downloaded bytes have the listed size and SHA-256, and, for the runtime and the plugin installer, when their `.sig` also verifies. Unknown fields are refused.

## Map pack

`map.pack` holds the map's [signed file set](./host-installation-v1.md#authenticated-file-sets), `files.json` and `files.json.sig` included, in one file: the 16 bytes `angelica-pack 1\n`, then for each file the length of its path as a little-endian `u32`, the path in UTF-8, its size as a little-endian `u64`, and its bytes. A path is plain names of letters, digits, `.`, `-`, and `_` joined by `/`; a name does not begin or end with `.`. A reader refuses a pack with another start, a truncated entry, a path longer than 512 bytes or of another form, a path twice, more than 8192 files, or more than 256 MiB, before it writes anything. The host unpacks it beside its map folder, loads it as a file set signed with the release key, writes `release.json` (`{"format": 1, "version": "<the feed's version>"}`) into it, and then lets it take the map folder's place.
