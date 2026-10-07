# Code review

Review protects correctness, safety, compatibility, and maintainable component boundaries. Style feedback is secondary to those concerns.

## Review order

1. **Scope and intent** — confirm the change solves the requested problem without unrelated work.
2. **Product and architecture** — check product invariants, component ownership, public interfaces, and format boundaries.
3. **Correctness and safety** — check failure handling, determinism, malformed input, memory soundness, permission enforcement, and silent fallback behavior.
4. **Compatibility** — compare against authoritative contracts: the game executable, versioned formats, and the plugin API.
5. **Tests** — confirm important success and failure modes are covered and that tests can detect contract drift.
6. **Documentation** — confirm changed contracts and behavior are documented in the owning files.
7. **Maintainability** — naming, complexity, duplication, and smaller improvements.

## Feedback severity

- **Blocker** — correctness, crash risk, security or permission bypass, contract incompatibility, missing required coverage, architecture violation, or another issue that must be resolved before merge.
- **Improvement** — meaningful change that does not make the current change unsafe or incorrect.
- **Nitpick** — minor style, wording, or local consistency issue.

Any unresolved blocker means request changes.

## Review evidence

Review the current head commit and its complete diff. When CI matters to the verdict, verify the successful run belongs to that exact head.

For contracts with the game, do not accept tests that only compare one implementation against another copy of the same logic. A game map change is accepted on a verification report produced from the game executable; a change to code that runs in the game names the in-game check that was done.
