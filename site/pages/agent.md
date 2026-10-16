---
order: 1
section: Build
kicker: Get started
meta: Windows · A GitHub account
prev: 
next: Run your plugin in the game
---

# Build a plugin with an agent

Tell a coding agent what you want a FINAL FANTASY XIV plugin to do. It sets up the project, writes and tests the plugin, and asks you whenever the decision is yours.

## What you need

- A GitHub account. The agent asks you to install the GitHub command `gh` and to sign in.
- To try the plugin in the game: the game and [Angelica](install.md).

## Start

Send your agent this, followed by what the plugin should do:

```prompt
Read https://angelicaproject.github.io/angelica/agent-setup.md and follow it to set up an Angelica plugin project.
```

The card at the top of the home page writes the same message with your idea in it. The agent works from [these instructions](https://angelicaproject.github.io/angelica/agent-setup.md), and then from the `AGENTS.md` in the project it creates.

## Add Angelica to your agent

Angelica's skill, `ffxiv-plugin`, keeps these instructions in your agent, so asking it for an FFXIV plugin is enough. Install it once.

In Claude Code:

```shell
claude plugin marketplace add AngelicaProject/angelica
claude plugin install angelica@angelica
```

In other agents that read Agent Skills, with the GitHub command:

```shell
gh skill install AngelicaProject/angelica ffxiv-plugin
```

## What the agent does, and what it asks you

1. It checks for Rust, Git, `gh`, and the C++ build tools, and installs what is missing once you agree.
1. It agrees with you on the plugin's ID. The ID is permanent and is also the plugin's chat command: a plugin called `timers` answers `/timers`.
1. It works out with you what the plugin reads in the game, what it changes or does there, and which permissions that needs.
1. It writes the plugin and tests it against a scripted game, so the game does not need to run.
1. If the game map lacks something the plugin needs, it asks before turning on dev mode in your Angelica, finds the missing piece in the running game, and adds it to the map. The plugin then works on your computer. With your agreement it sends the addition for review; once a map with it is released, the plugin works for every player.
1. It installs the plugin in your Angelica and tells you what to check in the game. You approve the plugin's permissions in the game yourself.
1. When you ask, it publishes the plugin. See [Publish a plugin](publish.md).
