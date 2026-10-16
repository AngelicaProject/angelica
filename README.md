# Angelica

A plugin platform for FINAL FANTASY XIV (FFXIV).

[Website](https://angelicaproject.github.io/angelica/) · [Build a plugin with an agent](https://angelicaproject.github.io/angelica/agent.html) · [Write your first plugin](https://angelicaproject.github.io/angelica/first-plugin.html) · [Download for Windows](https://github.com/AngelicaProject/angelica/releases/download/nightly/angelica-setup.exe) · [Discord](https://discord.gg/mv24mq53h2)

## Build a plugin with your agent

Send your coding agent this, followed by what the plugin should do:

```text
Read https://angelicaproject.github.io/angelica/agent-setup.md and follow it to set up an Angelica plugin project.
```

The agent installs what is missing once you agree, creates the project, writes the plugin, and tests it against a scripted game, so the game does not need to run. It asks you whenever the decision is yours: the plugin's name, what it may do in the game, and when to publish.

To keep these instructions in your agent, install Angelica's skill. In Claude Code:

```shell
claude plugin marketplace add AngelicaProject/angelica
claude plugin install angelica@angelica
```

In other agents that read Agent Skills:

```shell
gh skill install AngelicaProject/angelica ffxiv-plugin
```

## What a plugin is

- **A Rust crate compiled to WebAssembly.** It runs in a sandbox inside the game and can do only what the player allowed; the player approves each permission in the game.
- **Free of game addresses.** A plugin names entries of a shared [game map](https://github.com/AngelicaProject/angelica-gamemap). When a patch moves things in the game, the map is updated and the plugin keeps working without a rebuild.
- **Tested without the game.** Tests run the plugin in the real host against a scripted game.
- **Drawn by Angelica.** Windows, panels, and dialogs come from the host, so every plugin looks like part of the same interface.

When the API lacks something a plugin needs, the author adds it to the game map or [asks for it](https://github.com/AngelicaProject/angelica/issues).

## Repositories

- `angelica` (this repository): the loader, the host that runs in the game, the plugin API and SDK, the installer, the command-line tools, and the plugin for coding agents (`agent-plugin/`).
- [`angelica-gamemap`](https://github.com/AngelicaProject/angelica-gamemap): the game map data, verified against each game build.
- [`angelica-registry`](https://github.com/AngelicaProject/angelica-registry): the plugin catalog.

## Development

Start with:

- [Contributing](./CONTRIBUTING.md)
- [Documentation index](./docs/README.md)
- [Product vision](./docs/product/vision.md)
- [Architecture overview](./docs/architecture/overview.md)

## License

Angelica is licensed under the [GNU Affero General Public License v3.0](./LICENSE). The plugin API definitions (`wit/`), the SDK, and the plugin for coding agents are licensed under the MIT License so that plugins can use any license; see [`docs/development/dependencies.md`](./docs/development/dependencies.md#licensing).
