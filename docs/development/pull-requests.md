# Pull request standards

A pull request lets a reviewer understand the problem, the intended outcome, the important design choices, and the evidence that the change is ready. PR metadata describes the complete base-to-head change and stays concise throughout the lifetime of the pull request.

## Title

Write a human-readable, imperative title that describes the outcome. Do not use Conventional Commit prefixes in pull request titles.

Good:

- `Load the host before the game's entry point`
- `Refuse a map entry whose signature matches twice`
- `Verify the Excel row store functions for a new game build`

Avoid:

- `feat(host): loader`
- `Milestone 1`
- `Various fixes`
- `Update files`

## Description

Use this structure unless a section genuinely does not apply:

```markdown
## Summary

A short description of the outcome.

## Why

The problem, constraint, or need that motivated the change.

## Changes

The important behavioral, architectural, format, data, or tooling changes. Do not list every edited file.

## Verification

The exact checks that were run, and any verification against a game executable or in the running game.

## Documentation

Links to updated canonical documentation, or a short explanation of why no documentation change is required.
```

Add a `## Compatibility and risk` section when a change affects a versioned format, the plugin API, permissions, game map entries that published plugins use, code that runs inside the game process, or signing and trust.

Keep the description about the final change. Do not paste a prompt transcript, implementation diary, or speculative follow-up plan into the description.

## Readiness

Before requesting review:

- review the complete branch diff against `main`;
- remove unrelated changes;
- run the required checks for the affected area;
- add or update regression coverage;
- update canonical documentation and relevant indexes;
- verify the pushed branch contains the intended commits.

If the change is intentionally not ready to merge, use GitHub's Draft pull request state. Do not encode readiness with `WIP`, `Do not merge`, or similar text.

## During review

Keep the title and description aligned with the actual diff as scope changes. Address blockers before requesting another review. Do not report CI as passing unless the successful run belongs to the current pull request head.
