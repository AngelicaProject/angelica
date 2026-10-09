# Vision

Angelica lets players extend FINAL FANTASY XIV with plugins without trusting each plugin with their computer, and without waiting days for every plugin to be rebuilt after a game patch.

## Users

- **Players** install Angelica once, browse and install plugins from an in-game catalog, see what each plugin is allowed to do, and keep playing after a patch.
- **Plugin authors**, most of them coding agents, write a plugin against a typed API, explore the game's data from the command line, test the plugin without the game, reload it in the running game while developing, and publish it from source. When the API lacks a capability, they request it instead of working around it.
- **Platform maintainers** add capabilities to the API one game domain at a time, as host services over verified map entries.
- **Map maintainers** keep the game map current: after a patch they verify and recover entries against the new executable and publish the result.

Every workflow is designed to be carried out by a coding agent as well as by a person: contracts are machine-readable, tools report in JSON, and every failure names the entry, permission, or file involved.

## What a plugin can be

Angelica is a general plugin platform. A plugin's purpose is defined by its author; there is no fixed taxonomy or closed list of supported plugin kinds. Authors compose reusable API capabilities to build applications within the sandbox, granted permissions, and verified game contracts.

Examples of applications include:

- **Translation** of the game's text and fonts.
- **Mod management**: replacing game files per character, with collections of mods and per-character metadata changes.
- **Automation of game activities**, from crafting to combat, behind explicit permissions for each kind of action.
- **Appearance sync** between players through a plugin's own server, applying downloaded files safely.

These examples illustrate product scope, not plugin categories or a claim that every required API is already implemented. The same capabilities can support applications outside this list. When a concrete use case requires a missing capability, the API gains a reusable operation on game or host resources; application policy remains in the plugin. API design requirements belong in [host services](../architecture/services.md).

## Primary workflows

1. **Install.** The installer places the loader in the game folder. The next game start shows Angelica's menu in the game.
2. **Add a plugin.** The catalog shows the plugin's permissions; installing grants exactly those. An update that asks for more asks again.
3. **Game patch.** On the first start after a patch, plugins whose map entries are verified for the new build run; the others wait, and the menu says which entries they wait for. A map update arrives without updating Angelica or any plugin.
4. **Develop a plugin.** Create from the SDK template, test against recorded game data, run in the game with live reload, and submit to the registry.
