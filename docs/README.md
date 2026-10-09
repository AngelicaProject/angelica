# Angelica documentation

Documentation is maintained with the code it describes and is the source of truth for product behavior, architecture, formats, and development standards.

## Start here

- [Product](./product/README.md) — what Angelica is for and the invariants the product must preserve.
- [Architecture](./architecture/README.md) — components, ownership, and data flows.
- [Development](./development/README.md) — contribution workflow, implementation standards, testing, and CI.
- [Plugins](./plugins/README.md) — writing a plugin: the work loop, interface cards, API requests.
- [Formats](./formats/README.md) — versioned contracts: the game map, the plugin manifest and API, text tables, the installation.

## Find documentation by task

| Task | Read first |
| --- | --- |
| Understand product scope or user behavior | [`product/README.md`](./product/README.md) |
| Change a component boundary or data flow | [`architecture/overview.md`](./architecture/overview.md) |
| Change how Angelica gets into the game process | [`architecture/loading.md`](./architecture/loading.md) |
| Change hooking, trampolines, or code patches | [`architecture/hooks.md`](./architecture/hooks.md) |
| Change how map entries are resolved or verified | [`architecture/game-map.md`](./architecture/game-map.md), [`formats/game-map-v1.md`](./formats/game-map-v1.md) |
| Change the plugin runtime or the plugin API | [`architecture/plugins.md`](./architecture/plugins.md), [`formats/plugin-api-v1.md`](./formats/plugin-api-v1.md), [`formats/plugin-manifest-v1.md`](./formats/plugin-manifest-v1.md) |
| Write a plugin | [`plugins/authoring.md`](./plugins/authoring.md), [`plugins/interfaces.md`](./plugins/interfaces.md) |
| Request a capability the plugin API lacks | [`plugins/api-requests.md`](./plugins/api-requests.md) |
| Add or change a permission | [`architecture/permissions.md`](./architecture/permissions.md) |
| Add a host service | [`architecture/services.md`](./architecture/services.md), [`formats/game-structures.md`](./formats/game-structures.md) |
| Change the in-game interface | [`architecture/ui.md`](./architecture/ui.md) |
| Write Rust, especially `unsafe` | [`development/rust.md`](./development/rust.md) |
| Add or upgrade a dependency | [`development/dependencies.md`](./development/dependencies.md) |
| Add or update tests | [`development/testing.md`](./development/testing.md) |
| Prepare a branch, commit, or pull request | [`development/git.md`](./development/git.md), [`development/pull-requests.md`](./development/pull-requests.md) |
| Review a change | [`development/code-review.md`](./development/code-review.md) |
| Investigate CI | [`development/ci.md`](./development/ci.md) |
| Write or reorganize documentation | [`development/documentation.md`](./development/documentation.md) |

## Source-of-truth rule

Put each durable rule in the document that owns it and link to that document elsewhere. When documentation and implementation disagree, resolve the discrepancy in the same change.
