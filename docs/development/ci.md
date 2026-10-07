# Continuous integration

GitHub Actions runs the checks below for every pull request and every push to `main`. The workflow files under `.github/workflows/` are authoritative for exact runner configuration.

## Jobs

| Job | Runner | Guarantees |
| --- | --- | --- |
| `rust` | `windows-latest` | The workspace is formatted, has no Clippy warnings, and passes its tests, including the hook engine tests. |
| `plugins` | `windows-latest` | The SDK template and the test plugins build for `wasm32-wasip2` and pass under `angelica-fake-game`. |
| `supply-chain` | `ubuntu-latest` | `cargo deny` passes; workflows pass `actionlint` and `zizmor`. |
| `ci` | `ubuntu-latest` | Every job above passed. |

```text
cargo fmt --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check
```

No job requires a game installation.

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
