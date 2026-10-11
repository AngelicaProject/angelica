# Development documentation

This section defines how changes are implemented, validated, reviewed, and shipped.

## Change workflow

- [`contributing.md`](./contributing.md) — end-to-end contribution workflow and scope discipline.
- [`git.md`](./git.md) — branch names, commit messages, history, and push expectations.
- [`pull-requests.md`](./pull-requests.md) — pull request titles, descriptions, readiness, and review preparation.
- [`code-review.md`](./code-review.md) — review methodology, severity, and merge verdicts.
- [`ci.md`](./ci.md) — CI gates and failure investigation.
- [`releases.md`](./releases.md) — versions, update channels, release assets, the release key, and how hosts and maps update.

## Implementation

- [`standards.md`](./standards.md) — toolchains and project-wide reliability standards.
- [`rust.md`](./rust.md) — Rust conventions, crate boundaries, and the rules for `unsafe`.
- [`dependencies.md`](./dependencies.md) — dependency selection, approved foundations, and licensing.

## Quality and maintenance

- [`testing.md`](./testing.md) — test layers, tests without the game, and in-game checks.
- [`documentation.md`](./documentation.md) — documentation ownership, structure, and writing standards.

For product or architecture decisions, use [`../product/`](../product/README.md) and [`../architecture/`](../architecture/README.md) instead of adding development-only rules here.
