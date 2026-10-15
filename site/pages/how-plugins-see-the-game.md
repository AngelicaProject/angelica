---
order: 2
section: Understand
kicker: Understand
meta: About 3 minutes
prev: How Angelica works
next: The game map and patch day
---

# How plugins see the game

What a plugin can read and do in the game, and what you approve before it can.

## Through the game map, by name

A plugin cannot touch the game's memory or code. It asks Angelica for things by name, such as your character's level or the items in your bags, and Angelica reads them using the [game map](game-map.md). The names stay the same from one version of the game to the next, so a plugin keeps working after a patch without a new version.

## What a plugin can do

Each kind of access is a permission you approve, and the approval shows it in the words below:

| Kind | What it allows | As you see it, for example |
| --- | --- | --- |
| Read | Reading values the game holds | "Sees the items you carry" |
| Watch | Being told when something happens, such as an action being used | "Sees the actions you use" |
| Decide | Rules the game follows at points the map marks, such as which chat messages to show | "Hides messages in your chat log" |
| Change | Values that exist only on your computer; Angelica puts them back when the plugin stops | "Changes your camera's zoom limits" |
| Act | Doing things for you, the way the game's own interface does | "Uses actions for you: abilities, spells, items, mounts" |

## Domains

Permissions come per **domain**, a group of related things: your character, where you are, your statuses, the items you carry, the armoury chest, the chocobo saddlebags, what your retainers hold, the actions you use, what you are doing, teleporting, the camera, the chat, the game's windows, and the game's text. A plugin with the domain of your bags cannot read your chat.

Domains about other players, such as the chat, are shown under **Sees other players**.

## When a plugin acts for you

Acting for you is listed first when you approve a plugin, with a warning: automating the game breaks its terms of service, and Square Enix can suspend your account for it.

While a plugin acts, and for 3 seconds after, a note at the top right of the screen names it. `End`, or the key you choose in **Settings**, stops every plugin from acting until you press it again.

## What no plugin can read

Login data, session tokens, and anything else that could take over your account. The game map never lets a plugin read them.
