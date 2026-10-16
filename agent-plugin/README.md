# Angelica for coding agents

This plugin teaches a coding agent to build plugins for FINAL FANTASY XIV (FFXIV) with [Angelica](https://angelicaproject.github.io/angelica/). Angelica plugins are written in Rust and compiled to WebAssembly. They run in a sandbox inside the game, can do only what the player allowed, and keep working across game patches because they name entries of a shared game map instead of game addresses.

## What it adds

One skill, `ffxiv-plugin`. Ask your agent for an FFXIV plugin, or run `/angelica:ffxiv-plugin` in Claude Code. The skill sends the agent to Angelica's setup instructions for a new project, and to the project's own `AGENTS.md` for an existing one.

## Install

In Claude Code:

```shell
claude plugin marketplace add AngelicaProject/angelica
claude plugin install angelica@angelica
```

In other agents that read Agent Skills, with the GitHub command:

```shell
gh skill install AngelicaProject/angelica ffxiv-plugin
```

## What it runs and fetches

The skill itself runs nothing. It tells the agent to read [the setup instructions](https://angelicaproject.github.io/angelica/agent-setup.md) on Angelica's website, which ask the agent to:

- check for Rust, Git, the GitHub command `gh`, and the C++ build tools, and install the missing ones with `winget` only after the user agrees;
- install the `angelica` command with `cargo install` from this repository;
- create the plugin project with `angelica plugin new` and run its tests with `cargo test`, which fetches this repository.

Setup works on Windows. The plugin sends no data anywhere and stores nothing.

## License

MIT. See [LICENSE](./LICENSE).
