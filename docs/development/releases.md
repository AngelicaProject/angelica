# Releases and updates

Angelica is distributed through GitHub Releases of `AngelicaProject/angelica`; the game map is released separately from `AngelicaProject/angelica-gamemap`. Windows is the only platform.

## Versions

- Angelica follows [Semantic Versioning](https://semver.org/). Before 1.0, a minor version may change installation formats or behavior incompatibly; the release notes say so.
- The whole workspace has one version, `[workspace.package] version` in `Cargo.toml`. A stable release tag is `v` followed by exactly that version (`v0.1.0`); the release workflow refuses a tag that does not match. The declared version on `main` is always a plain `MAJOR.MINOR.PATCH`.
- The plugin API has its own version (`angelica:plugin@1.0.0`, [plugin API](../formats/plugin-api-v1.md)); a host release never changes it.

## Channels

| Channel | Source | Version | Feed |
| --- | --- | --- | --- |
| Stable | Tag `vX.Y.Z` on a green `main` commit | `X.Y.Z` | `releases/latest/download/channel.json` |
| Nightly | Every successful CI run of a push to `main` | `<next>-nightly.<build>` | `releases/download/nightly/channel.json` |

Stable releases are ordinary GitHub releases marked *latest*. Nightly is one rolling pre-release, tag `nightly`, replaced by each build and never marked *latest*. `<next>` is the declared version while it has not been tagged, and the next patch version once it has; `<build>` is the release workflow's run number. Nightly versions sort before the stable release they lead to.

The channel is chosen in **Settings → General → Updates** and stored as `channel` in `state/_angelica.json` ([installation](../formats/host-installation-v1.md#angelica-settings)). Without it, a copy follows the channel its installer came from. The nightly channel checks both feeds and takes the newer version, so a stable release newer than the last nightly arrives at once. Angelica never installs an older version than the one running: switching from nightly to stable keeps the nightly build until a newer stable release.

Nightly runs inside the game process and may stop the game; it is for testers. The installer on the website installs stable.

## Release assets

| Asset | Purpose |
| --- | --- |
| `angelica-setup.exe` | The installer; the release's runtime, plugin installer, and proxy are inside it |
| `runtime.dll`, `runtime.dll.sig` | The host and its release signature |
| `plugin-installer.exe`, `plugin-installer.exe.sig` | The plugin installer and its release signature |
| `dinput8.dll`, `dinput8.dll.sig` | The proxy and its release signature |
| `channel.json`, `channel.json.sig` | The update feed of this release |
| `SHA256SUMS.txt` | SHA-256 of every asset |

`channel.json` is signed with the release key ([feed format](../formats/update-feed-v1.md)). It names the version, the channel, and for each file its asset name, size, and SHA-256.

Executables are not Authenticode-signed; Windows SmartScreen may warn when `angelica-setup.exe` first runs, and that is accepted. Every file Angelica loads is still verified against the release key: the installer verifies what it installs, the proxy verifies the runtime, and the host verifies the plugin installer, the map, and catalog plugins.

## The release key

The release key is an Ed25519 key. Its public half is compiled into the proxy, the host, the plugin installer, and the installer as `ANGELICA_RELEASE_PUBLIC_KEY` (the repository variable of the same name); its secret half is the repository secret `ANGELICA_RELEASE_SIGNING_KEY`, also in `angelica-gamemap` for map releases, and the maintainers keep an offline backup. Only the release workflows sign with it. Losing it means installed copies can no longer update themselves and must be installed again after the key is replaced. A development key (`angelica dev key`) is never a release key.

## Host updates

The host updates itself; the player never has to act ([loading](../architecture/loading.md#updates)).

- **Checks.** 20 seconds after the game starts and every 6 hours, on a host thread, the host reads the feed of its channel (both feeds for nightly). A failure is shown in **Settings → General → Updates** and never interrupts the game.
- **Download.** A newer version's runtime and plugin installer are downloaded, checked against the feed's sizes and hashes and against their release signatures, and written with their signatures into a new version folder `versions/<version>/`; then `current.json` is replaced to name it ([loading](../architecture/loading.md#updates)). A file in use is never replaced, and older version folders are removed once nothing uses them.
- **Apply.** The proxy loads the new version at the next game start. The menu says **Angelica updates at the next game start** until then.
- **The proxy** is loaded by the game and changes rarely; only the installer replaces it. A host that needs a newer proxy says so in the menu.
- **Plugins** are compiled on the player's computer, from their author or from the [catalog](../architecture/catalog.md); a runtime update compiles them again (`plugin-installer.exe` repair).

## Map updates

The game map changes with game patches, not with Angelica, so it has its own releases and feed in `angelica-gamemap`: a stable release per map version and no nightly. A map version is `<year>.<month>.<number>` without leading zeros (`2026.10.1`), a valid SemVer version that sorts by release. Its `map.json` feed has the same format and key as `channel.json` and names one file, `map.pack`, with the signed file set ([format](../formats/update-feed-v1.md#map-pack)). The host reads it 20 seconds after the game starts and every hour, installs a newer map at once (map data reloads while the game runs; entries that hooks use change at the next start), and never installs an older one.

## Release workflow

`.github/workflows/release.yml` builds on `windows-latest`:

1. For a tag, it waits for CI on `main` to pass for the tagged commit and fails if the commit is not on `main` or CI failed. A nightly starts only after a successful CI run of a push to `main` whose commit is still the head of `main`; a newer nightly cancels one still building.
2. It checks or computes the version; a nightly writes its version into `Cargo.toml` and the lockfile before building.
3. It builds the proxy, the runtime, the plugin installer, and the installer in release mode with `ANGELICA_RELEASE_PUBLIC_KEY`, signs the proxy, the runtime, and the plugin installer with `angelica release sign`, embeds them in the installer, and writes and signs `channel.json` with `angelica release feed`.
4. It writes `SHA256SUMS.txt` and publishes the stable release with generated notes, or replaces the `nightly` pre-release.

## Cutting a stable release

1. On a branch, set the new version in `Cargo.toml`, run `cargo update --workspace`, and merge it to `main` through a pull request.
2. Tag the merge commit and push the tag:

   ```text
   git tag -a vX.Y.Z -m "Angelica X.Y.Z" <merge-commit>
   git push origin vX.Y.Z
   ```

3. Watch the Release workflow, then add highlights and compatibility notes to the generated release notes.

A published version is never rebuilt or re-tagged; fix problems with a new patch release.
