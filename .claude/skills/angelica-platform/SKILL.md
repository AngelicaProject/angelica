---
name: angelica-platform
description: Add or extend a plugin API capability in the Angelica host from an API request - a state table or field, an event kind, an operation on the game, or a new domain interface - with its WIT, permission, adapter, map entries, SDK helper, tests, and docs. Use when implementing what a plugin author requested, or when the plugin API, permissions, or host services change.
---

# Implementing a plugin API capability

Work in `D:\angelica` (and `D:\angelica-gamemap` for map data). Load `AGENTS.md`, `.ai/development.md`, and `.ai/git.md` first. The rules live in the docs; this is the order of work.

## 1. Design against the domain

Read `docs/architecture/services.md` (API design, kinds of services, growing the API), `docs/architecture/permissions.md`, and `docs/product/principles.md`. Then decide:

- **Kind.** State (a table or field in the game map, read under `state.<domain>`), event (an adapter hook queues it, delivered next frame), operation on the game (one host operation per kind of action, own permission, validated arguments, host-enforced rate), game data (a sheet schema column), or a declarative rule.
- **Domain.** Group with other requests for the same domain; design the interface for every plugin that needs the domain, in terms of the game resource, never of the requesting plugin.
- **Permission.** As narrow as the domain allows; reading, events, and each kind of operation are separate. The player-facing text is plain words.
- **What stays unavailable.** A structure the map cannot verify is not exposed partially.

A state field or table often needs no host code at all: only game map data (step 3) and a permission for a new domain.

## 2. Host changes, in the order of past additions

| Piece | Where |
| --- | --- |
| WIT interface or function, with doc comments | `wit/<interface>.wit`, imported in `wit/plugin.wit` |
| Permission: variant, name, player text | `crates/angelica-host/src/manifest.rs`; a state domain's permission in `crates/angelica-host/src/state.rs` |
| Linking the import, permission and context checks | `crates/angelica-host/src/sandbox.rs` |
| Service logic, unit tests | `crates/angelica-host/src/<service>.rs` |
| What the host needs from the game | the `Game` trait in `crates/angelica-host/src/game.rs`; its Windows adapter in `crates/angelica-host/src/interop/` |
| Scripted game for tests | `crates/angelica-fake-game/src/lib.rs` (implement the new `Game` method, add a helper the tests call) |
| SDK module or helper | `crates/angelica-sdk/src/<module>.rs`, exported from `lib.rs` |
| Example use | `examples/hello/src/lib.rs` and its `angelica.toml` permissions |
| Integration tests | `crates/angelica-fake-game/tests/hello.rs`: success, missing permission, absent map entry, wrong instance |

Code in the game process follows `docs/development/rust.md` (`// SAFETY:` on every `unsafe` block) and `docs/architecture/hooks.md`; a failing hook disables itself and the game's code runs.

## 3. Game map data

In `D:\angelica-gamemap`, use the `angelica-map` skill: entries the adapter uses, state tables in `state/`, sheet columns in `sheets/`, then `angelica map verify` against the current executable. Find code with `angelica exe find|disasm|signature`. Community research is a hint only; verify every entry against the exe and keep `origin` for imports. Never name the other FFXIV plugin framework or its plugins anywhere.

## 4. Documentation in the same change

- `docs/formats/plugin-api-v1.md`: the interface table row and the section with rules and limits.
- `docs/architecture/permissions.md`: the permission row.
- `docs/formats/game-structures.md`: the adapter's contract, when it touches game structures.
- `docs/plugins/interfaces.md`: the card plugin authors read (SDK call, permission, instance, what to remember).
- `docs/development/dependencies.md` for a new dependency.

## 5. Checks

Before every cargo command: `export ANGELICA_RELEASE_PUBLIC_KEY=<dev public key>`; build with `-j 4`. Run the CI set from `docs/development/ci.md` (fmt, clippy workspace and wasm, all tests, cargo deny).

Then check it in the game: host changes need the game closed for `angelica dev install --runtime ...` (check `tasklist | grep -i ffxiv` as a separate command first); plugin and map changes install while the game runs. Ask the player for what to click and report only what was observed. Commit without any AI attribution.
