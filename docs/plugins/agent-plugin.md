# The plugin for coding agents

`agent-plugin/` is a plugin for coding agents, not for the game. It gives an agent one skill, `ffxiv-plugin`, so that an agent asked for a FINAL FANTASY XIV plugin finds Angelica and follows its setup.

## What the skill holds

The skill is `agent-plugin/skills/ffxiv-plugin/SKILL.md`, in the Agent Skills format. It says what Angelica is in one paragraph and routes the agent:

- for a new plugin, to `site/agent-setup.md` as published on the website, then to the `AGENTS.md` of the project it creates;
- for a folder with an `angelica.toml`, to that project's `AGENTS.md`;
- for a plugin of another framework, back to the user's own work.

The skill holds no setup steps, commands, or API details. Those live in `site/agent-setup.md`, the project template's `AGENTS.md`, and this documentation, which change with the tools; the skill only points at them, so an installed copy does not go stale.

## Where it is installed from

- **Claude Code.** `.claude-plugin/marketplace.json` at the repository's root makes the repository a marketplace named `angelica` with one plugin, `angelica`, whose files are `agent-plugin/`:

  ```text
  claude plugin marketplace add AngelicaProject/angelica
  claude plugin install angelica@angelica
  ```

  The skill is then `/angelica:ffxiv-plugin`, and the agent also loads it by its description.
- **Other agents.** The GitHub command finds the skill by the Agent Skills layout (`*/skills/<name>/SKILL.md`) and installs it for the agent the user picks:

  ```text
  gh skill install AngelicaProject/angelica ffxiv-plugin
  ```

- **Anthropic's plugin directory.** A listing is submitted from the directory's developer portal with this repository and the plugin path `agent-plugin`. The directory reads the plugin's name, description, and links from `plugin.json` and its text from `agent-plugin/README.md`.

## Releasing a change

The plugin's version is `version` in `agent-plugin/.claude-plugin/plugin.json`. A change to anything under `agent-plugin/` raises it in the same pull request; Claude Code and the directory deliver a change only with a new version. A change to `site/agent-setup.md` alone needs no new version, since the skill reads the page.

The plugin's name, `angelica`, and the skill's name, `ffxiv-plugin`, are permanent: installs and commands are recorded under them.

`agent-plugin/README.md` describes everything the skill makes an agent run or fetch. When `site/agent-setup.md` starts installing or fetching something new, the README says so in the same pull request.

CI's `agent-plugin` job checks the plugin and the marketplace with `claude plugin validate --strict` ([CI](../development/ci.md)).
