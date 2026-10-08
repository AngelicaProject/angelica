# Formats

This section owns Angelica's versioned contracts, including those whose data lives in other repositories.

- [`game-map-v1.md`](./game-map-v1.md) — game map entries and build records. The data is in `angelica-gamemap`.
- [`plugin-manifest-v1.md`](./plugin-manifest-v1.md) — a plugin's `angelica.toml`: identity, API version, memory, and permissions.
- [`plugin-api-v1.md`](./plugin-api-v1.md) — the plugin API defined in `wit/`: calls into a plugin, windows, storage, jobs, text tables, letter case, and WASI.
- [`text-table-v1.md`](./text-table-v1.md) — the file of replacement cell text a plugin activates.
- [`game-structures.md`](./game-structures.md) — the game structures the host's adapters change, and the map entries they use.
- [`host-installation-v1.md`](./host-installation-v1.md) — host configuration, authenticated file sets, player grants, plugin settings, and development installations.

Once a format is released, an incompatible change requires a new format version. Readers refuse a version they do not know instead of reinterpreting it.
