# Code review instructions

Canonical review standards live in `docs/development/code-review.md`.

Review the change in this order:

1. **Scope and intent** — understand the requested outcome and check for unrelated scope.
2. **Contracts and architecture** — verify product invariants, component ownership, formats, and compatibility rules.
3. **Correctness and safety** — look for regressions, silent fallback behavior, nondeterminism, unsound memory access, permission bypasses, and missing error handling.
4. **Tests** — verify important behavior and failure modes have meaningful coverage.
5. **Documentation** — ensure changed behavior or contracts are reflected in the owning documents and indexes.
6. **Maintainability** — naming, complexity, duplication, and smaller improvements.

## Severity

- **Blocker** — must be fixed before merge.
- **Improvement** — meaningful but non-blocking.
- **Nitpick** — minor style or consistency issue.

Any unresolved blocker means the verdict is request changes. Nitpicks alone do not block approval.

## Review evidence

- Review the current pull request head, not a stale commit.
- Verify CI against that exact head when CI status affects the verdict.
- Prefer concrete file/behavior references over speculative concerns.
- When correctness depends on the game executable, compare against the executable itself or a verification report produced from it, not against the project's own tests alone.
