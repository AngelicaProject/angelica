# Catalog format 1

The catalog's files in `angelica-registry` and the index it publishes ([catalog](../architecture/catalog.md)).

## Entries

`plugins/<id>.toml` in `angelica-registry`, one per plugin:

```toml
format = 1
id = "harmonia"
repository = "AngelicaProject/harmonia-angelica"
tag = "v0.2.0"
author_key = "6fb36909f2f2778f0b5fc8dc2d2eddb6fa096146699b4fd3efa360a7fbe16b62"
```

| Field | Meaning |
| --- | --- |
| `format` | `1` |
| `id` | the plugin id; the file's name without `.toml` |
| `repository` | the public GitHub repository of the plugin's source, `owner/name`; `angelica.toml` and the crate are at its root |
| `tag` | the tag to build, `v` followed by `plugin.version` |
| `author_key` | the author's Ed25519 public key, 64 lowercase hexadecimal digits; the release of `tag` in `repository` holds a plugin file signed with it |

Unknown fields are refused. An update changes `tag`; changing `repository` or `author_key` is reviewed like a new plugin.

`withdrawn.toml` lists what the catalog withdrew:

```toml
format = 1

[[withdrawn]]
id = "timers"
version = "1.2.0"    # left out: every version
reason = "Reads chat it did not declare."
```

## Index

`catalog.json` and its signature `catalog.json.sig` (64 raw Ed25519 bytes over the exact bytes of the JSON, made with the release key) are the assets of the release `catalog` of `angelica-registry`. A reader verifies the signature with the release public key compiled into it before parsing.

```json
{
  "format": 1,
  "published": "2026-10-20T12:00:00Z",
  "plugins": [
    {
      "id": "harmonia",
      "name": "Harmonia",
      "description": "Shows the game in another language with community translation packs.",
      "version": "0.2.0",
      "authors": ["pokeda"],
      "permissions": ["excel.text", "files.replace", "http.get", "text.case"],
      "hosts": ["*.github.io", "github.com", "*.githubusercontent.com"],
      "link": "https://github.com/AngelicaProject/angelica-registry/releases/download/harmonia/harmonia.angelica",
      "sha256": "<lowercase SHA-256 of the plugin file>",
      "source": "https://github.com/AngelicaProject/harmonia-angelica/tree/v0.2.0"
    }
  ],
  "withdrawn": [
    {"id": "timers", "version": "1.2.0", "reason": "Reads chat it did not declare."}
  ]
}
```

`name`, `description`, `version`, `authors`, `permissions`, and `hosts` are copied from the published plugin's manifest; `link` serves the plugin file whose SHA-256 is `sha256`. A withdrawn entry without `version` withdraws every version. A reader refuses another format, an id that is not a valid plugin id, a link that is not `https`, and unknown fields.
