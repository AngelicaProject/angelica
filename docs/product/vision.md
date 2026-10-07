# Vision

Angelica lets players extend FINAL FANTASY XIV with plugins without trusting each plugin with their computer, and without waiting days for every plugin to be rebuilt after a game patch.

## Users

- **Players** install Angelica once, browse and install plugins from an in-game catalog, see what each plugin is allowed to do, and keep playing after a patch.
- **Plugin authors** write a plugin against a typed API, test it without the game, reload it in the running game while developing, and publish it from source.
- **Map maintainers** keep the game map current: after a patch they verify and recover entries against the new executable and publish the result.

Every workflow is designed to be carried out by a coding agent as well as by a person: contracts are machine-readable, tools report in JSON, and every failure names the entry, permission, or file involved.

## What a plugin can be

The plugin API and host services support plugins of these kinds:

- **Translation** of the game's text and fonts.
- **Mod management**: replacing game files per character, with collections of mods and per-character metadata changes.
- **Automation of game activities** such as crafting, behind an explicit permission.
- **Appearance sync** between players through a plugin's own server, applying downloaded files safely.

A plugin of any of these kinds is built only from the plugin API, map entries, and host services. When a kind of plugin needs something the API does not offer, the API gains a general service rather than the plugin gaining native code.

## Primary workflows

1. **Install.** The installer places the loader in the game folder. The next game start shows Angelica's menu in the game.
2. **Add a plugin.** The catalog shows the plugin's permissions; installing grants exactly those. An update that asks for more asks again.
3. **Game patch.** On the first start after a patch, plugins whose map entries are verified for the new build run; the others wait, and the menu says which entries they wait for. A map update arrives without updating Angelica or any plugin.
4. **Develop a plugin.** Create from the SDK template, test against recorded game data, run in the game with live reload, and submit to the registry.
