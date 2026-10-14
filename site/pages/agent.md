---
order: 1
section: Build
kicker: Get started
meta: For people and coding agents · Windows · GitHub
prev: 
next: Run your plugin in the game
---

# Build a plugin with an agent

You describe what the plugin should do, and a coding agent writes it, tests it, and installs it for you to try.
You decide what the plugin may do, which requests go to the Angelica developers, and when it goes to players.

## What you need

- A coding agent that can run commands in a terminal on your computer.
- A GitHub account. The agent asks you to install the GitHub command `gh` and sign in.
- To try the plugin in the game: the game and [Angelica](install.md).

## Start

Give your agent this, with your idea at the end:

```plaintext
Read https://angelicaproject.github.io/angelica/agent.md and follow it to build an Angelica plugin that shows where to gather any material I search for.
```

## What happens next

1. The agent sets up the tools and creates the plugin's project.
1. It shows you a plan: what the plugin reads in the game, what it changes, and which permissions it asks for. Correct it until it is what you want.
1. It writes the plugin and tests it without the game.
1. If Angelica cannot do something the plugin needs yet, the agent shows you a request to the Angelica developers. It sends the request only if you agree, and builds the rest of the plugin meanwhile.
1. It installs the plugin into your Angelica and tells you what to do in the game. You allow the plugin's permissions yourself.
1. When you are happy with it, ask the agent to [publish it](publish.md).

## For the agent

Follow these steps in order. Ask the user before installing anything.

1. Check the tools:

   ```shell
   cargo --version
   git --version
   gh auth status
   ```

   If Rust is missing, ask the user to install it from https://rustup.rs. If `gh` is missing or not signed in, ask the user to install it from https://cli.github.com and run `gh auth login`.

1. Install the `angelica` command:

   ```shell
   cargo install --locked --git https://github.com/AngelicaProject/angelica angelica-cli
   ```

1. Agree on the plugin's ID with the user. It is permanent and is also the plugin's chat command: lowercase letters, digits, and inner hyphens, starting with a letter, at most 32 characters.
1. Create the project in the folder the user chooses:

   ```shell
   angelica plugin new <id>
   ```

1. Run `cargo test` in the project. The first run fetches the Angelica repository and builds the test host, which takes a few minutes. All tests pass on a new project.
1. Read the project's `AGENTS.md` and follow its work loop. It holds the commands, the rules, and links to the rest of this documentation, and it stays with the project for every later change.
