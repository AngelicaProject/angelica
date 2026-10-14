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
"http.get" = true

[http]
hosts = ["harmonia.example.org", "*.cdn.example.org"]

[lens]
read = ["character", "inventory"]
act = ["actions"]
change = ["camera"]
```

## Fields

- `manifest`: the format version, `1`. A reader refuses any other value.
- `plugin.id`: 1–64 lowercase ASCII letters, digits, and hyphens, not starting or ending with a hyphen. Unique in the registry; never changes. It names the package folder, the storage folder, and the grant file.
- `plugin.name`, `plugin.description`: what the player sees; not blank, at most 1024 bytes.
- `plugin.version`: a semantic version.
- `plugin.api`: the [plugin API](./plugin-api-v1.md) version the component was built for, `major.minor`. This host runs `1.0`.
- `plugin.authors`: optional list of names.
- `plugin.memory_bytes`: optional memory limit of each instance of the plugin, 1 to 1073741824 bytes; the host default (64 MiB) when absent.
- `permissions`: each [permission](../architecture/permissions.md) the plugin needs, as `"<name>" = true`. `false` and an absent permission are the same; a plugin that needs none leaves the table out or empty. An unknown permission refuses the manifest.
- `http.hosts`: the hosts `http.get` reaches ([plugin API](./plugin-api-v1.md#http)), present exactly when the plugin declares `http.get`, which `http.send` needs too: 1–32 distinct host names of lower-case ASCII labels (letters, digits, and inner hyphens, 1–63 bytes each, at most 253 bytes), at least two labels, not an address. International names are written in their `xn--` form. A leading `*.` matches every subdomain of the name after it, not the name itself, and needs at least two labels after it.

- `messages.to`: the plugins `messages.send` sends to ([plugin API](./plugin-api-v1.md#messages-between-plugins)), present exactly when the plugin declares `messages.send`: 1–32 distinct plugin ids.
- `lens.read`: the domains of the game map's [object model](./game-map-v2.md#domains) the plugin reads through [Lens](./plugin-api-v1.md#lens), at most 64 distinct names of lowercase letters, digits, and underscores. A domain the running game map does not describe, or of a class the host does not present, is listed for the player but not granted.
- `lens.change`: the domains whose values the plugin [changes](./plugin-api-v1.md#lens), in the same form. A domain the map does not describe, or describes without words for changing it, is listed for the player but not granted.
- `lens.act`: the domains the plugin takes [actions](./plugin-api-v1.md#lens) in, in the same form. A domain the running game map does not describe, or describes without words for acting in it, is listed for the player but not granted.

Unknown fields refuse the manifest.
