# Format specifications

This section owns Angelica's versioned contracts, including those whose data lives in other repositories.

- [`game-map-v1.md`](./game-map-v1.md) — game map entries and build records. The data is in `angelica-gamemap`.
- [`plugin-manifest-v1.md`](./plugin-manifest-v1.md) — a plugin's `angelica.toml`: identity, API version, permissions, map entries, and interfaces.

The plugin API itself is defined in WIT under `wit/`; its version is the version of the WIT package.

Once a format is released, an incompatible change requires a new format version. Readers refuse a version they do not know instead of reinterpreting it.
