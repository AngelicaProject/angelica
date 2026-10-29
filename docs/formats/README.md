# Formats

This section owns Angelica's versioned contracts, including those whose data lives in other repositories.

- [`game-map-v2.md`](./game-map-v2.md) — game map entries, the object model, schemas, and build records. The data is in `angelica-gamemap`.
- [`anatomia-1.md`](./anatomia-1.md) — Anatomia, the description of the game: types, functions, globals, finding pipelines, checks, meaning, and build records, and its conversion from game map v2.
- [`lens-catalog-1.md`](./lens-catalog-1.md) — what Lens offers over an Anatomia description: types, roots, events, actions, and changes.
- [`plugin-manifest-v1.md`](./plugin-manifest-v1.md) — a plugin's `angelica.toml`: identity, API version, memory, and permissions.
- [`plugin-api-v1.md`](./plugin-api-v1.md) — the plugin API defined in `wit/`: calls into a plugin, windows, storage, jobs, Lens, direct access to the game's objects, intercepts, and WASI.
- [`text-table-v1.md`](./text-table-v1.md) — the file of replacement cell text a plugin activates.
- [`game-structures.md`](./game-structures.md) — the game structures the host's adapters change, and the map entries they use.
- [`host-installation-v1.md`](./host-installation-v1.md) — host configuration, authenticated file sets, player grants, plugin settings, and development installations.
- [`update-feed-v1.md`](./update-feed-v1.md) — the signed feed that names the newest release of Angelica or of the game map.
- [`catalog-v1.md`](./catalog-v1.md) — catalog entries in `angelica-registry`, withdrawals, and the signed index.
- [`plugin-file-v1.md`](./plugin-file-v1.md) — the file an author signs to distribute a plugin outside the catalog, and its installation.

Once a format is released, an incompatible change requires a new format version. Readers refuse a version they do not know instead of reinterpreting it.
