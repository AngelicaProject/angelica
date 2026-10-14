---
order: 1
section: Build
kicker: Get started
meta: For people and coding agents · Windows · GitHub
prev: 
next: Run your plugin in the game
---

# Build a plugin with an agent

You design the plugin: what it does in the game, what it may touch, and how the player uses it. A coding agent implements your design and proves it with tests, and you take every decision on the way.

## What you need

- A GitHub account. The agent asks you to install the GitHub command `gh` and sign in.
- To try the plugin in the game: the game and [Angelica](install.md).

## Start

Give your agent this:

```prompt
Read https://angelicaproject.github.io/angelica/agent-setup.md and follow it to set up an Angelica plugin project.
```

The agent offers to install what is missing, creates the project, and asks what the plugin should do. If you already know, add your idea to the message. [The instructions it follows](https://angelicaproject.github.io/angelica/agent-setup.md) are plain Markdown that you can read too.

## What happens next

1. The agent offers to install the tools that are missing, then creates the plugin's project.
1. You tell the agent your idea and settle the design together: what the plugin reads in the game, what it changes, and which permissions it needs. The agent works it out from your idea, and you shape it until it is right.
1. The agent implements the design and covers it with tests that run without the game.
1. Where Angelica cannot do something yet, the agent drafts a request to the Angelica developers. You decide whether it goes out; meanwhile the agent builds the rest.
1. The agent installs the plugin into your Angelica and tells you what to check in the game. You allow its permissions yourself.
1. When the plugin is ready, ask the agent to [publish it](publish.md).
