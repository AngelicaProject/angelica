# Plugin manifest v1

Every plugin has an `angelica.toml` at the root of its source and inside its package.

```toml
manifest = 1

[plugin]
id = "harmonia"
name = "Harmonia"
version = "0.2.0"
api = "1.0"
description = "Shows the game in another language with community translations."

[entries]
required = ["excel.store_row.hash_table", "excel.store_row.ring_buffer", "text.utf8_string.to_upper"]
optional = ["text.utf8_string.to_lower"]

[permissions]
"game.hook" = ["excel.store_row.hash_table", "excel.store_row.ring_buffer", "text.utf8_string.to_upper", "text.utf8_string.to_lower"]
"files.redirect" = true
network = ["github.com", "objects.githubusercontent.com"]

[interfaces]
exports = ["harmonia:dictionary@1.0.0"]
imports = []
```

## Fields

- `manifest`: format version. A reader refuses any other value.
- `plugin.id`: lowercase letters, digits, and hyphens; unique in the registry; never changes.
- `plugin.version`: semantic version.
- `plugin.api`: the plugin API version the plugin was built for. The host runs the plugin when it supports that major version at an equal or higher minor version.
- `entries.required`: map entries without which the plugin does not start.
- `entries.optional`: map entries that enable part of the plugin; the plugin is told at start which are absent.
- `permissions`: the permissions of [`../architecture/permissions.md`](../architecture/permissions.md). Permissions about map entries list the entries; `network` lists domains; others are `true`.
- `interfaces.exports` and `interfaces.imports`: WIT interfaces with versions.

Every entry named under `permissions` must also appear under `entries`. A manifest that names an entry not in the map, or that is not consistent with itself, is refused at packing and at installation.
