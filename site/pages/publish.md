---
order: 3
section: Build
kicker: Get started
meta: About 10 minutes · GitHub · The gh command
prev: Run your plugin in the game
next: 
---

# Publish a plugin

Players get your plugin in one of two ways: by a link that you share, or from the catalog in the Angelica window.
Either way, Angelica checks your signature and shows players what the plugin asks for before it runs.

## Before you begin

- A plugin that you [ran in the game](run-in-the-game.md).
- The plugin's project on GitHub, made by `angelica plugin new`.
- The [`gh`](https://cli.github.com/) command, signed in to your GitHub account.
- Your author key in `%APPDATA%\Angelica\author.key`. `angelica plugin pack` created it the first time.

## Release with a tag

The project already has a `release.yml` workflow. It tests the plugin, builds it, signs it with your author key, and publishes it.

1. In the project's folder, give the workflow your author key, one time:

   ```shell
   Get-Content "$env:APPDATA\Angelica\author.key" -Raw | gh secret set ANGELICA_AUTHOR_KEY
   ```

1. In `angelica.toml`, raise `plugin.version`, for example to `0.2.0`. Every release needs a newer version.
1. Commit, then push a tag with the same version:

   ```shell
   git tag v0.2.0
   git push origin v0.2.0
   ```

When the workflow finishes, the plugin is behind a link that stays the same for every version:

```plaintext
https://github.com/<you>/<repository>/releases/download/<id>/<id>.angelica
```

## Share the link

Give players the link. They paste it on the **Installed** tab of the Angelica window and select **+ Add**.
Angelica shows them that the plugin comes from its author, not from the Angelica catalog, and checks the link for new versions by itself.

## Add it to the catalog

The catalog is how most players find plugins. Plugins in it are built by Angelica from your source code, so players run exactly the code that was reviewed.

1. [Release with a tag](#release-with-a-tag) first.
1. Open a pull request to [angelica-registry](https://github.com/AngelicaProject/angelica-registry) that adds `plugins/<id>.toml` with your repository, the tag, and your author key. The [catalog entry format](https://github.com/AngelicaProject/angelica/blob/main/docs/formats/catalog-v1.md) lists the fields.
1. Wait for the review. The registry builds and tests your plugin, and a maintainer reads what it asks for.

After that, a new version that asks for nothing new goes into the catalog by itself when you open the pull request for its tag. A version that asks for more waits for a maintainer again.

Before you submit, check that `plugin.description` tells a player what the plugin does, and that the manifest asks only for the permissions and sites the plugin uses.

## Keep your author key

The author key is your plugin's identity. Players' Angelica accepts new versions only when they are signed with the same key.
If you lose it, you cannot update the plugin for players who already have it. Keep a copy outside your computer, the way you would keep a password.

## Troubleshooting

### `gh secret set` says that it cannot find the repository

Run it inside the project's folder, which must have the GitHub repository as its `origin`.

### Players do not get the new version

Angelica checks links when the game starts and every 6 hours. Check that `plugin.version` is newer than the last release; a release with the same version changes nothing for players.
