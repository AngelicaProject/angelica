# Permissions

A plugin's manifest declares its permissions; see [`../formats/plugin-manifest-v1.md`](../formats/plugin-manifest-v1.md). The player grants them when installing. An update that declares a permission the installed version did not have is not applied until the player grants it.

## Permissions

| Permission | Allows | Shown to the player as |
| --- | --- | --- |
| `ui` | windows in the overlay, chat output, commands | — (always granted) |
| `game.read` | reading the map fields and game data the manifest names | reads game state |
| `game.hook` | the hooks the manifest names | changes game behavior |
| `game.patch` | the code patches the manifest names | changes game code |
| `game.call` | calling the game functions the manifest names | uses game functions |
| `game.act` | actions on behalf of the player: using actions, operating game windows, moving items | acts for the player |
| `game.memory` | reading and writing game memory outside named fields | full access to the game |
| `files.redirect` | serving replacement game files through the file service | replaces game files |
| `storage` | the plugin's own folder | — (always granted) |
| `folder` | a folder the player chooses | reads and writes the chosen folder |
| `network` | HTTP and WebSocket to the domains the manifest names | connects to the named domains |
| `audio` | playing sound | plays sound |
| `clipboard` | reading and writing the clipboard | uses the clipboard |
| `plugin.import` | calling the interfaces of the plugins the manifest names | uses other plugins |

Every permission except `ui` and `storage` is listed at installation with its details: the hooks, functions, domains, and plugins named in the manifest.

## Combinations

`game.memory` together with `network` could send account data to a server. The installation screen states this explicitly for that combination.

## Data no permission exposes

Session tokens, login data, and other data that could take over the account are not available through any typed API. The host denies `game.memory` reads of the memory regions that hold them.
