# Plugin manifest v1

Every plugin has an `angelica.toml` at the root of its source and inside its package.

```toml
manifest = 1

[plugin]
id = "harmonia"
name = "Harmonia"
version = "0.2.0"
api = "1.0"
description = "Shows the game in another language with community translation packs."
authors = ["pokeda"]
memory_bytes = 1073741824

[permissions]
"excel.text" = true
"text.case" = true
```

## Fields

- `manifest`: the format version, `1`. A reader refuses any other value.
- `plugin.id`: 1–64 lowercase ASCII letters, digits, and hyphens, not starting or ending with a hyphen. Unique in the registry; never changes. It names the package folder, the storage folder, and the grant file.
- `plugin.name`, `plugin.description`: what the player sees; not blank, at most 1024 bytes.
- `plugin.version`: a semantic version.
- `plugin.api`: the [plugin API](./plugin-api-v1.md) version the component was built for, `major.minor`. This host runs `1.0`.
- `plugin.authors`: optional list of names.
- `plugin.memory_bytes`: optional memory limit of each instance of the plugin, 1 to 1073741824 bytes; the host default (64 MiB) when absent.
- `permissions`: each [permission](../architecture/permissions.md) the plugin needs, as `"<name>" = true`. `false` and an absent permission are the same. An unknown permission refuses the manifest.

Unknown fields refuse the manifest.
