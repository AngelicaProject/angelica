# Documentation standards

Documentation is maintained alongside the code and data it describes and is the source of truth for product behavior, architecture, formats, and development standards.

## Structure and navigation

[`docs/README.md`](../README.md) is the top-level documentation index. Each top-level section has its own `README.md` index.

When adding, moving, or deleting a document:

- update the owning section index;
- update `docs/README.md` when top-level task routing changes;
- keep relative links valid;
- avoid orphaned documents that are not reachable from an index.

## Keep one source of truth

Put a rule in the document that owns it and link to that document elsewhere. Duplicated guidance drifts.

- product invariants belong in `docs/product/principles.md`;
- component ownership belongs in `docs/architecture/`;
- versioned contracts belong in `docs/formats/`;
- contributor conventions belong in `docs/development/`.

A contract shared by several Angelica repositories is owned by exactly one of them, and the others link to it. The game map format, the plugin manifest, and the plugin API are owned by the [`angelica`](https://github.com/AngelicaProject/angelica) repository.

Root files such as `README.md`, `CONTRIBUTING.md`, and `AGENTS.md` orient readers and route them to canonical documentation rather than restating it. Files under `.ai/` contain task-specific execution guidance and link back to durable rules here.

## Update documentation with behavior

Update the relevant documentation in the same change when modifying:

- user-visible behavior;
- architecture boundaries;
- formats and their compatibility guarantees;
- development, CI, or release requirements.

## Writing style

Write directly and impersonally. Avoid promotional language, implementation history, prompt transcripts, and process commentary.

Prefer concrete statements and examples where ambiguity would affect implementation.

## Documentation, not decisions

Documentation states what the system does and what its contracts require. It does not record how a design was chosen, the alternatives that were considered, status markers, or plans; those belong in pull requests and issues. When the implementation and the documentation differ, one of them is wrong and is fixed in the same change.
