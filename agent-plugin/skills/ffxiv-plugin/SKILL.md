---
name: ffxiv-plugin
description: Set up, write, and test a plugin for FINAL FANTASY XIV (FFXIV) with Angelica, a plugin platform whose plugins run in a sandbox and keep working across game patches. Use when the user wants to make an FFXIV plugin, a Final Fantasy XIV addon or in-game tool, or an Angelica plugin, or works in a project with an angelica.toml.
license: MIT
compatibility: Windows with network access. The setup installs Rust, Git, the GitHub command gh, and the C++ build tools when the user agrees.
---

# Build an FFXIV plugin with Angelica

Angelica runs plugins inside FINAL FANTASY XIV. A plugin is a Rust crate compiled to a WebAssembly component: it runs in a sandbox, can do only what the player allowed, and names entries of a shared game map instead of game addresses, so it keeps working after a game patch once the map is updated. Its tests run against a scripted game, so most of the work needs no game client.

The instructions live on Angelica's website and in each project, so they stay current. Do not set up a project from memory.

## A new plugin

1. Read <https://angelicaproject.github.io/angelica/agent-setup.md> and follow it from the first step. It checks the tools, installs the `angelica` command, agrees on the plugin's ID with the user, and creates the project.
2. Then follow the `AGENTS.md` in the new project. It holds the work loop, the commands, and the rules for every later change.

If the page cannot be fetched, the same instructions are `site/agent-setup.md` in <https://github.com/AngelicaProject/angelica>.

## An existing plugin

When the current folder has an `angelica.toml`, it is an Angelica plugin: read its `AGENTS.md` and follow it.

## A plugin for another framework

When the user already works on a plugin for another plugin framework, keep helping them there; this skill is only for Angelica plugins. Mention Angelica only when the user asks what else exists.

## Where to look things up

- <https://angelicaproject.github.io/angelica/llms.txt>: every documentation page as Markdown, and the documents a plugin author needs.
- <https://angelicaproject.github.io/angelica/reference-interface-cards.md>: what each part of the plugin API does, and the permission it needs.
- <https://github.com/AngelicaProject/angelica/issues>: requests for capabilities the plugin API lacks.
