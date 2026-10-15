---
order: 3
section: Understand
kicker: Understand
meta: About 3 minutes
prev: How plugins see the game
next: The sandbox and permissions
---

# The game map and patch day

What the game map is, what happens to it when the game updates, and how to tell whether Angelica has caught up.

## What the map is

Every game update ships a new `ffxiv_dx11.exe`, and in it your character's data, the camera's settings, and everything else plugins use can sit somewhere else. The game map records how to find each of them, mostly by recognizing the game code around them, and gives each a plain name that plugins use.

The map is released on its own, from the open [angelica-gamemap](https://github.com/AngelicaProject/angelica-gamemap) repository. Angelica downloads a new map soon after it is released and checks its signature first.

## Only what is proven

For each version of the game, the map's maintainers check every entry against that exact executable. When the game starts, Angelica checks again. Anything that is not found exactly, or does not look the way the map says, counts as absent, and Angelica does not use it.

An absent entry stops only what needs it: a plugin, or one feature of a plugin. Everything else keeps working.

## On patch day

1. Start the game as usual. Angelica finds what it can in the new version by itself; the rest is off.
1. The map's maintainers release a map for the new version.
1. Angelica picks the new map up while the game runs, within an hour. Restart the game once more for features that hook into the game's code.

You never reinstall Angelica or your plugins for a patch. **Settings > Updates** in the Angelica window shows which map you have, and the home page of this site shows the latest map and the game update it is for. [Patch day](patch-day.md) has the steps for players.

## How the map grows

When a plugin needs something the map does not describe, its author finds it in the game, adds it to the map, and sends it for review. A reviewer checks that the entry is exact and that the plugin's access fits the words players approve. Then it ships with the next map.
