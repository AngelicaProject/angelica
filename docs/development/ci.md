# Continuous integration

GitHub Actions runs the checks below for every pull request and every push to `main`. The workflow files under `.github/workflows/` are authoritative for exact runner configuration.

## Jobs

| Job | Runner | Guarantees |
| --- | --- | --- |
| `rust` | `windows-latest` | The workspace is formatted, has no Clippy warnings, and passes its tests with all features. |
| `supply-chain` | `ubuntu-latest` | `cargo deny` passes; workflows pass `actionlint` and `zizmor`. |
| `site` | `ubuntu-latest` | The [website](./website.md) builds and every link inside it leads somewhere. |
| `ci` | `ubuntu-latest` | Every job above passed. |

```text
cargo fmt --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p angelica-sdk -p angelica-hello -p angelica-hostile --target wasm32-wasip2 --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo deny --locked check
cd site && npm ci && node build.mjs
```

No job requires a game installation.

The Windows Rust job installs `wasm32-wasip2` so the plugin crates are linted for their target and the fake-game tests can build the example component with the pinned compiler.

The `main` ruleset requires only `ci`. A new job is added to the `needs` of `ci`.

## Workflow rules

- Every action is pinned by commit SHA with its version in a comment.
- Workflows have read-only permissions by default; a job that writes declares exactly the permissions it needs.
- Checkouts do not keep the token (`persist-credentials: false`).
- Expressions such as `${{ steps.x.outputs.y }}` reach `run:` scripts through `env`, never directly in the script text.
- A tool downloaded by a workflow is pinned by version and verified by checksum, or installed through a pinned action.

## Local validation

Run focused checks while iterating and the commands above before requesting review. A local pass and a CI pass are different evidence; report them separately.

## Failure handling

Confirm the run belongs to the current head, identify the failing job and step, read the logs, reproduce locally when practical, and fix the root cause rather than weakening the gate.
