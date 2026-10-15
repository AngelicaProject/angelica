---
order: 4
section: Understand
kicker: Understand
meta: About 3 minutes
prev: The game map and patch day
next: 
---

# The sandbox and permissions

What a plugin has without asking, what it must ask you for, and what happens when it fails.

## Without asking

Each plugin runs in a sandbox of its own. Without any permission it has its own windows over the game, its own folder, `%LOCALAPPDATA%\Angelica\storage\<plugin id>`, and its own log, which you can read under **Settings > For developers**. It cannot read your files, other plugins' folders, or anything of the game.

## What a plugin asks for

Everything else is a permission. Before a plugin runs for the first time, the Angelica window lists what it asks for, the most serious group first:

| Group | Examples of what you see |
| --- | --- |
| **Acts in the game for you** | "Uses actions for you: abilities, spells, items, mounts"; "Answers the game's dialogs for you" |
| **Uses the internet** | "Downloads from these internet hosts", followed by the list of hosts |
| **Sees other players** | "Sees your chat log, including other players' messages" |
| **Changes what you see** | "Replaces game files: fonts, textures, materials, and models"; "Changes your camera's zoom limits" |
| **Only for you** | "Sees the items you carry"; "Prints its messages in your chat log" |

The plugin gets what you approve and nothing else. You can turn it off or remove it in the Angelica window at any time.

When a new version asks for something the old one did not, it waits for you, and the window shows only what is new. Updates that ask for nothing new install by themselves.

## On the internet

A plugin with an internet permission reaches only the hosts it listed and you approved, over HTTPS. It cannot reach devices on your home network. A proxy or VPN that your browser uses works for plugins too.

## When a plugin fails

If a plugin crashes, takes too long, or goes past one of its limits, Angelica turns it off and the game keeps running. The reason is under **Settings > For developers**. Select the plugin and **Copy report** to send its author what they need. See [When something goes wrong](troubleshooting.md).
