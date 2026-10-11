# Catalog

The Angelica catalog lists plugins that players can find and install. It lives in the `angelica-registry` repository and is built entirely by CI: the registry builds every catalog plugin from its published source, signs it, and publishes a signed index. A plugin outside the catalog still installs from its author's link with the same sandbox and permissions ([plugin file](../formats/plugin-file-v1.md)).

## How a plugin enters the catalog

1. **The author releases the plugin through CI.** A tag `v<version>` in the plugin's repository builds the component, signs the plugin file with the author key,, publishes it in the tag's release, and behind the author's link.
2. **A pull request to `angelica-registry`** adds or updates `plugins/<id>.toml`: the plugin's repository, the tag, and the author key ([format](../formats/catalog-v1.md)). The author's release CI can open it.
3. **The registry's CI checks the pull request** without secrets: the entry's form; that the tag's release in the plugin's repository holds a plugin file signed with the entry's author key, so the author agreed; that the plugin builds from the source at the tag with the pinned toolchain and the SDK of the angelica repository's `main`, and its tests pass; and what the plugin declares compared with its catalog version — permissions, hosts, and memory. The review is in the run's summary.
4. **What merges by itself.** An update of a plugin already in the catalog, from the same repository and author key, whose checks pass and which declares no permission, host, or memory the catalog version does not, is merged by the registry's CI, which reviews it again with its own code from the release files.
5. **What a maintainer reviews.** A new plugin, and an update that declares more, wait for a maintainer, who reads the review and decides; the registry's CI comments why. Nothing else needs a person.

## Publishing

When `main` changes, the registry's CI builds each changed plugin from the source at its tag, packs it as a plugin file signed with the release key — the catalog is the author of what it publishes — and puts it behind the plugin's catalog link, `https://github.com/AngelicaProject/angelica-registry/releases/download/<id>/<id>.angelica`. It then writes the index, `catalog.json`, signed with the release key, and publishes it in the release `catalog`.

What players run is what the registry built from the reviewed source, not a file the author uploaded.

Installed Angelica installs a catalog plugin from its catalog link like any plugin link; the menu marks a plugin whose file is signed with the release key as coming from the Angelica catalog. A plugin id installed from the catalog cannot be replaced by an author's file, and the other way round.
