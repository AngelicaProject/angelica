# Continuous integration

GitHub Actions runs the checks below for every pull request and every push to `main`. The workflow files under `.github/workflows/` are authoritative for exact runner configuration.

## Jobs

| Job | Runner | Guarantees |
| --- | --- | --- |
| `rust` | `windows-latest` | The workspace is formatted, has no Clippy warnings, and passes its tests with all features. |
| `linux` | `ubuntu-22.04` | The installer and game discovery build for Linux without Clippy warnings and pass their tests. |
| `wine-fixture` | `windows-latest` | Builds the proxy's bootstrap fixture for the `wine` job. |
| `wine` | `ubuntu-latest`, in `debian:trixie` pinned by digest, without Docker's default seccomp profile, which refuses a socket call Wine makes when it starts | In Wine, the proxy starts the host before the game's entry point only with the installer's `dinput8` override ([testing](./testing.md#loader-tests)). |
| `supply-chain` | `ubuntu-latest` | `cargo deny` passes; workflows pass `actionlint` and `zizmor`. |
| `site` | `ubuntu-latest` | The [website](./website.md) builds and every link inside it leads somewhere. |
| `agent-plugin` | `ubuntu-latest` | The [plugin for coding agents](../plugins/agent-plugin.md) and the marketplace that lists it pass `claude plugin validate --strict`. |
| `ci` | `ubuntu-latest` | Every job above passed. |

```text
cargo fmt --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy -p angelica-sdk -p angelica-hello -p angelica-hostile --target wasm32-wasip2 --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo clippy -p angelica-games -p angelica-setup --all-targets --locked -- -D warnings  # on Linux
cargo test -p angelica-games -p angelica-setup --locked  # on Linux
ANGELICA_WINE_FIXTURE=<folder> cargo test -p angelica-proxy --test bootstrap --locked -- --ignored --exact wine_fixture
sh crates/angelica-proxy/tests/wine.sh <folder>  # on Linux, with Wine
cargo deny --locked check
cd site && npm ci && node build.mjs
npx @anthropic-ai/claude-code@2.1.301 plugin validate --strict ./agent-plugin
npx @anthropic-ai/claude-code@2.1.301 plugin validate --strict .
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
- A job that runs in a container names its image by digest.

## Local validation

Run focused checks while iterating and the commands above before requesting review. A local pass and a CI pass are different evidence; report them separately.

## Failure handling

Confirm the run belongs to the current head, identify the failing job and step, read the logs, reproduce locally when practical, and fix the root cause rather than weakening the gate.
