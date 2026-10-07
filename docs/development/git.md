# Git workflow

## Branch names

Use short lowercase names that describe one coherent change. Default prefixes:

- `feature/` — new capability.
- `fix/` — bug or correctness fix.
- `docs/` — documentation and repository-guidance changes only.
- `refactor/` — behavior-preserving structural work.
- `chore/` — maintenance that does not fit the categories above.

Use hyphens inside the descriptive part. Avoid personal names, generated identifiers, and vague names such as `changes`, `work`, or `update`.

## Commit messages

Commits use a Conventional Commit-style subject:

```text
<type>(<optional-scope>): <imperative summary>
```

Common types are `feat`, `fix`, `refactor`, `test`, `docs`, `build`, `ci`, and `chore`. The scope names the component or area, for example a crate without its `angelica-` prefix, a map area, or a plugin id.

Examples:

```text
feat(hook): relocate RIP-relative instructions in trampolines
fix(map): refuse a signature that matches twice
feat(excel): verify the row store functions for 7.3

docs: describe plugin permissions
```

Keep the subject concise, imperative, and without a trailing period. Add a body when the reason, trade-off, compatibility constraint, or migration detail is not obvious from the diff. Explain why the change is necessary rather than narrating every edited file.

## Commit scope

Each commit represents one coherent review and rollback concern. Tests that directly validate an implementation travel with that implementation. Separate unrelated documentation, tooling, or refactoring work when it can be reviewed and reverted independently.

## Updating a branch

Prefer rebasing a feature branch onto current `main` over merging `main` into the feature branch. After history changes, rerun the checks relevant to the affected area.

Do not rewrite shared history or force-push `main`.

## Push completion

A push is complete only after the remote branch is observed at the intended commit. When a task depends on specific generated files, lockfiles, or documentation being present, verify them on the remote branch before reporting completion.
