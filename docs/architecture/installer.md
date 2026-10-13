# Installer

`angelica-setup.exe` puts Angelica beside the game for a player who has never used plugins. It is one tall window, 380 by 500 pixels with rounded corners, that keeps its shape from the first screen to the last, in Angelica's look ([interface](./ui.md)): the light theme, actions as words, and plain labels with their values. Angelica draws its own title bar: the word `angelica`, then minimize and close; dragging the bar moves the window.

## What the player sees

1. **Installation.** The game it found (the launcher and the installation's folder; several are a choice), with **Choose another folder**, which opens Windows' folder dialog; when no game is found, only **Choose the game folder**. A chosen folder counts when it is the game's `game` folder or holds one. Then what is installed beside the game that works with Angelica (Dalamud through XIVLauncher, ReShade), and, at the bottom, Angelica's version and **Install →**. A `dinput8.dll` of another program in the game folder is said in red and keeps **Install** off; a game folder that needs administrator rights is said in one line.
2. **Installation in progress.** A thin progress line and the current step.
3. **Done.** The menu key, and that updates are installed automatically when the game starts; **Close**.

It offers no plugins: the player installs them from the Angelica menu in the game. A failure shows what happened and what to do, with **Try again**.

## What it does

- **Finds the game** in, in order: Steam libraries (the Steam path from the registry and `libraryfolders.vdf`, app 39210), the Square Enix launcher's default folder, and XIVLauncher's configured game path. A folder counts only when it holds `game/ffxiv_dx11.exe`.
- **Installs Angelica** for the current user into `%LOCALAPPDATA%\Angelica` ([installation](../formats/host-installation-v1.md)): `runtime.dll`, `plugin-installer.exe`, and their release signatures, taken from inside the installer and verified against the release key before they are written. It writes the channel the installer was built for into `state/_angelica.json`. The host downloads the game map and later updates by itself.
- **Puts two files into the game's `game` folder**: the proxy `dinput8.dll` ([loading](./loading.md)) and `angelica.json` with the installation folder. When that folder is not writable for the user (as under `Program Files`), it starts a second copy of itself with administrator rights that only copies these two files and exits.
- **Never overwrites another tool's `dinput8.dll`.** A `dinput8.dll` without the mark every Angelica proxy carries (the name of its log, `angelica-loader.log`) stops the installation with an explanation; Angelica's own proxy of any version is replaced.
- **Registers itself** under the user's *Installed apps* with a copy of itself in the installation folder as the uninstaller.
- Installing again over an existing installation updates the proxy and keeps the player's plugins, grants, settings, and storage.

## Removing

The uninstaller, run from *Installed apps*, shows the same window: that Angelica's file leaves the game folder, and the choice to keep or delete the player's plugins and their data. It removes `dinput8.dll` (only when it is Angelica's proxy) and `angelica.json` from the game folder, asking for administrator rights the same way. Without the player's data it removes the installation folder; with them it leaves `plugins`, `grants`, `state`, and `storage` in place for a later installation.

## Building

`angelica-setup` is a crate of this workspace. The release workflow builds it with the release's proxy, runtime, plugin installer, their signatures, and the channel embedded ([releases](../development/releases.md)); a build without them refuses to install and says that it is a development build.
