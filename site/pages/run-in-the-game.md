---
order: 3
section: Build
kicker: Get started
meta: About 5 minutes · Angelica installed · The game
prev: Write your first plugin
next: Publish a plugin
---

# Run your plugin in the game

Tests prove that a plugin works. In the game, you see how its windows look and how it behaves with real data, a zone change, or a log-out.

## Before you begin

- [Install Angelica](install.md) and start the game with it at least once.
- Have a plugin project that builds, such as the one from [Write your first plugin](first-plugin.md).

## Build and install the plugin

Run these commands in PowerShell, in your plugin's folder. The example uses the `hello` plugin.

1. Build the plugin:

   ```shell
   cargo build --release --target wasm32-wasip2
   ```

1. Sign it into a plugin file:

   ```shell
   angelica plugin pack --component target/wasm32-wasip2/release/hello.wasm
   ```

   The first time, this creates your author key in `%APPDATA%\Angelica\author.key`. Players' Angelica accepts new versions of your plugin only when they are signed with this key, so keep a copy of it somewhere safe.

   It writes the file `hello-0.1.0.angelica`, named after the plugin's ID and version.

1. Install the file into your Angelica:

   ```shell
   angelica plugin install hello-0.1.0.angelica --installation "$env:LOCALAPPDATA\Angelica"
   ```

You can do this while the game is running. Angelica picks up the plugin within a second.

## Turn it on

1. In the game, press <kbd>Insert</kbd>. The plugin is under **Needs attention**.
1. Select **Allow…** next to it, and check that it asks for what you expect.
1. Select **Allow and turn on**.

## Change it and try again

Repeat the three commands after each change. Angelica replaces the running plugin with the new version within a second, and keeps its storage and the permissions you allowed.
If the new version asks for a new permission, it waits under **Needs attention** until you allow it.

## See what the plugin does

Open the menu next to the window's close button, then **For developers**. For each plugin, it shows its state, its folder, and the last lines of its log, including why it stopped.

Write to the log what the plugin did and why something failed. When a player has a problem, they send you the plugin's report from **Copy report for the author**, and the report ends with the same log.

## Troubleshooting

### `this file of hello is signed by another author than the installed hello`

This Angelica remembers the author key that signed `hello` the first time, and accepts only that key for it. Sign with that key (`--key <path>`), or give your plugin another ID.

### `hello is already installed from the Angelica catalog`

A plugin with the same ID came from the catalog. Use another ID for your plugin, or remove the catalog plugin first.
