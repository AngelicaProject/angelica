# Installer

`angelica-setup.exe` puts Angelica beside the game for a player who has never used plugins. It is one small window that keeps its size from the first screen to the last, in Angelica's look ([interface](./ui.md)): the light theme, actions as words, and plain sentences.

## What the player sees

1. **Install.** One sentence on what Angelica is and that it changes nothing in the game and can be removed at any time; the game folder it found; **Install →**. When it finds several installations, the game line is a choice; when it finds none, the line asks for the folder.
2. **Installing.** A thin progress line and one sentence on the current step. When the game folder needs administrator rights, the sentence says that Windows will ask for permission to put one file into the game's folder.
3. **Done.** Start the game as usual; the menu opens with the menu key, shown as a key token; Angelica updates itself when the game starts. **Close**.

It offers no plugins: the player installs them from the Angelica menu in the game. Any failure replaces the screen's sentence with what happened and what to do, and **Try again**.

## What it does

- **Finds the game** in, in order: the folder the player chose before (from an existing `angelica.json`), Steam libraries (the Steam path from the registry and `libraryfolders.vdf`, app 39210), the Square Enix launcher's default folder, and XIVLauncher's configured game path. A folder counts only when it holds `game/ffxiv_dx11.exe`.
- **Installs Angelica** for the current user into `%LOCALAPPDATA%\Angelica` ([installation](../formats/host-installation-v1.md)): `runtime.dll`, `plugin-installer.exe`, and their release signatures, taken from inside the installer and verified against the release key before they are written. It writes the channel the installer was built for into `state/_angelica.json`. The host downloads the game map and later updates by itself.
- **Puts two files into the game's `game` folder**: the proxy `dinput8.dll` ([loading](./loading.md)) and `angelica.json` with the installation folder. When that folder is not writable for the user (as under `Program Files`), it starts a second copy of itself with administrator rights that only copies these two files and exits.
- **Never overwrites another tool's `dinput8.dll`.** A `dinput8.dll` that is not Angelica's proxy (its release signature does not verify) stops the installation with an explanation; Angelica's own proxy of any version is replaced.
- **Registers itself** under the user's *Installed apps* with a copy of itself in the installation folder as the uninstaller.
- Installing again over an existing installation updates the proxy and keeps the player's plugins, grants, settings, and storage.

## Removing

The uninstaller, run from *Installed apps*, removes `dinput8.dll` (only when it is Angelica's proxy) and `angelica.json` from the game folder, asking for administrator rights the same way, and then asks whether to keep the player's plugins and their data. Without them it removes the installation folder; with them it leaves `plugins`, `grants`, `state`, and `storage` in place for a later installation.

## Building

`angelica-setup` is a crate of this workspace. The release workflow builds it with the release's proxy, runtime, plugin installer, their signatures, and the channel embedded ([releases](../development/releases.md)); a build without them refuses to install and says that it is a development build.
