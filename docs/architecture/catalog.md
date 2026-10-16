# Catalog

The Angelica catalog lists plugins that players can find and install from the Angelica menu. It lives in the `angelica-registry` repository and is built entirely by CI: the registry builds every catalog plugin from its published source, signs it, and publishes a signed index. A plugin outside the catalog still installs from its author's link with the same sandbox and permissions ([plugin file](../formats/plugin-file-v1.md)).

## How a plugin enters the catalog

1. **The author releases the plugin through CI.** A tag `v<version>` in the plugin's repository builds the component, signs the plugin file with the author key,, publishes it in the tag's release, and behind the author's link (`angelica plugin new` writes these workflows).
2. **A pull request to `angelica-registry`** adds or updates `plugins/<id>.toml`: the plugin's repository, the tag, and the author key ([format](../formats/catalog-v1.md)). The author's release CI can open it.
3. **The registry's CI checks the pull request** without secrets: the entry's form; that the tag's release in the plugin's repository holds a plugin file signed with the entry's author key, so the author agreed; that the plugin builds from the source at the tag with the pinned toolchain and the SDK of the angelica repository's `main`, and its tests pass; and what the plugin declares compared with its catalog version — permissions, the game map domains it reads, acts in, and changes through Lens, hosts, and memory. The review is in the run's summary.
4. **What merges by itself.** The registry's CI merges, after reviewing it again with its own code from the release files:
   - an update of a plugin already in the catalog, from the same repository and author key, whose checks pass and which declares no permission, game map domain, host, or memory the catalog version does not;
   - a new plugin whose checks pass and which asks for nothing above the class **Changes what you see** ([permissions](./permissions.md#classes)): it does not act for the player, reach the internet, or see other players, and every game map domain it reads or changes is one the released map describes for that. The review reads the classes of domains from the released map.
5. **What a maintainer reviews.** A new plugin that acts for the player, uses the internet, or sees other players, a change of repository or author key, and an update that declares more wait for a maintainer, who reads the review and decides; the registry's CI comments why. Nothing else needs a person.

## Publishing

When `main` changes, the registry's CI builds each changed plugin from the source at its tag, packs it as a plugin file signed with the release key — the catalog is the author of what it publishes — and puts it behind the plugin's catalog link, `https://github.com/AngelicaProject/angelica-registry/releases/download/<id>/<id>.angelica`. It then writes the index, `catalog.json`, signed with the release key, and publishes it in the release `catalog`.

What players run is what the registry built from the reviewed source, not a file the author uploaded.

## In the game

The **Catalog** tab of the Angelica menu reads the index when it first opens and then every 6 hours; a player who never opens it sends nothing. It lists each plugin with its tile, name, version, and description, and **Install**, or **Installed** for one the player has. **Install** installs the plugin from its catalog link through the plugin installer, exactly as a link the player pasted: the installer verifies the file, compiles it on the player's computer, and the player approves its permissions. The menu marks a plugin whose file is signed with the release key as coming from the Angelica catalog. A plugin installed from the catalog updates from its catalog link; a plugin id installed from the catalog cannot be replaced by an author's file, and the other way round.

## Withdrawing

The index lists withdrawn versions and plugins. When the index lists an installed catalog plugin's version as withdrawn, Angelica turns the plugin off and says why under Settings → For developers; the player can remove it. A maintainer withdraws by a pull request that adds the version or plugin to `withdrawn.toml`; it is published like any change.
