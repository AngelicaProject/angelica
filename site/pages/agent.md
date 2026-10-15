---
order: 1
section: Build
kicker: Get started
meta: For people and coding agents · Windows · GitHub
prev: 
next: Run your plugin in the game
---

# Build a plugin with an agent

You design the plugin: what it does in the game, what it may touch, and how the player uses it. A coding agent implements the design and tests it.

## What you need

- A GitHub account. The agent asks you to install the GitHub command `gh` and sign in.
- To try the plugin in the game: the game and [Angelica](install.md).

## Start

Give your agent this:

```prompt
Read https://angelicaproject.github.io/angelica/agent-setup.md and follow it to set up an Angelica plugin project.
```

If you already know what the plugin should do, add it to the message. The agent follows [these instructions](https://angelicaproject.github.io/angelica/agent-setup.md).

## What happens next

1. The agent offers to install the missing tools and creates the project.
1. You describe the idea. Together you decide what the plugin reads in the game, what it changes, and which permissions it needs.
1. The agent implements the design and tests it without the game.
1. If Angelica's map of the game lacks something the plugin needs, the agent finds it in the game and adds it to the map, so the plugin works on your computer at once. With your agreement, it sends the addition to the map's maintainers, and once they release it, the plugin works for every player. If it cannot be added, the agent drafts a request to the Angelica developers instead.
1. The agent installs the plugin in your Angelica and tells you what to check in the game. You allow its permissions.
1. To publish the plugin, ask the agent. See [Publish a plugin](publish.md).
