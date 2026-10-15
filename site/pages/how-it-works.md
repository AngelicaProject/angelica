---
order: 1
section: Understand
kicker: Understand
meta: About 3 minutes
prev: 
next: How plugins see the game
---

# How Angelica works

What Angelica puts on your computer, what happens when the game starts, and how plugins and updates reach you.

## What is on your computer

Angelica lives in `%LOCALAPPDATA%\Angelica`: Angelica itself, the game map, your plugins, and their settings and files.

The game's `game` folder gets two files. `dinput8.dll` is loaded by the game at start and starts Angelica; `angelica.json` tells it where Angelica is. Nothing else in the game folder changes, and [removing Angelica](remove.md) deletes both files.

## When the game starts

`dinput8.dll` checks Angelica's signature and loads it only if the Angelica project signed it. Angelica then reads the game map, finds what it can in your version of the game, and starts the plugins you approved. `Insert` opens its window.

A plugin that crashes, hangs, or goes past its limits is turned off, and the game keeps running. After a game patch, whatever the map cannot find in the new version stays off until a new map arrives; only the plugins that need it stop working. [When something goes wrong](troubleshooting.md) says what to do.

## Where plugins come from

From the **Catalog** tab of the Angelica window. The Angelica project builds each catalog plugin from the source its author published and signs it. A person reviews every new plugin and every update that asks for more permissions; other updates pass automatic checks.

From a link an author gives you. The file is signed with the author's key, and once the plugin is installed, Angelica accepts its updates only when the same key signed them.

Either way, a plugin runs only after you approve what it asks for. See [The sandbox and permissions](permissions.md).

## Updates

| What | When Angelica checks | When the new version takes effect |
| --- | --- | --- |
| Angelica | 20 seconds after the game starts, then every 6 hours | At the next start of the game |
| Game map | 20 seconds after the game starts, then every hour | At once; parts that hook into the game's code at the next start |
| Plugins | When the game starts, then every 6 hours | At once; an update that asks for more permissions waits for you |

Angelica checks the signature of every download before it uses it.
