# Installer

`angelica-setup.exe` puts Angelica beside the game for a player who has never used plugins. The same installer, built for Linux as `angelica-setup`, does it for the game running in Wine, as Steam's Proton and `XIVLauncher.Core` run it on Linux and the Steam Deck ([on Linux](#on-linux)). It is one tall window, 380 by 500 pixels with rounded corners, that keeps its shape from the first screen to the last, in Angelica's look ([interface](./ui.md)): the light theme, actions as words, and plain labels with their values. Angelica draws its own title bar: the word `angelica`, then minimize and close; dragging the bar moves the window.

## What the player sees

1. **Installation.** The game it found (the launcher and the installation's folder; several are a choice), with **Choose another folder**, which opens Windows' folder dialog; when no game is found, only **Choose the game folder**. A chosen folder counts when it is the game's `game` folder or holds one. Then what is installed beside the game that works with Angelica (Dalamud through XIVLauncher, ReShade), and, at the bottom, Angelica's version and **Install →**. A `dinput8.dll` of another program in the game folder is said in red and keeps **Install** off; a game folder that needs administrator rights is said in one line.
2. **Installation in progress.** A thin progress line and the current step.
3. **Done.** The menu key, and that updates are installed automatically when the game starts; **Close**.

It offers no plugins: the player installs them from the Angelica menu in the game. A failure shows what happened and what to do, with **Try again**.

## What it does

- **Waits for the game to close.** While a program named `ffxiv_dx11.exe` runs, from any folder, it installs and removes nothing and says that the game is running, with **Try again**: the running game holds the proxy and the runtime open.
- **Finds the game** in, in order: Steam libraries (the Steam path from the registry and `libraryfolders.vdf`, app 39210), the Square Enix launcher's default folder, and XIVLauncher's configured game path. A folder counts only when it holds `game/ffxiv_dx11.exe`.
- **Installs Angelica** for the current user into `%LOCALAPPDATA%\Angelica` ([installation](../formats/host-installation-v1.md)): `runtime.dll`, `plugin-installer.exe`, and their release signatures, taken from inside the installer and verified against the release key before they are written. It writes the channel the installer was built for into `state/_angelica.json`. The host downloads the game map and later updates by itself.
- **Puts two files into the game's `game` folder**: the proxy `dinput8.dll` ([loading](./loading.md)) and `angelica.json` with the installation folder. When that folder is not writable for the user (as under `Program Files`), it starts a second copy of itself with administrator rights that only copies these two files and exits.
- **Never overwrites another tool's `dinput8.dll`.** A `dinput8.dll` without the mark every Angelica proxy carries (the name of its log, `angelica-loader.log`) stops the installation with an explanation; Angelica's own proxy of any version is replaced.
- **Registers itself** under the user's *Installed apps* with a copy of itself in the installation folder as the uninstaller.
- Installing again over an existing installation updates the proxy and keeps the player's plugins, grants, settings, and storage.

## On Linux

On Linux the game is the same Windows program, run by Wine, and so is Angelica inside it: the proxy, the runtime, and the plugins are the Windows release's files. The installer differs in where it finds the game, where it puts Angelica, and one setting of Wine.

- **Finds the game** in Steam's libraries (`~/.steam/steam`, `~/.local/share/Steam`, and the flatpak's `~/.var/app/com.valvesoftware.Steam/.local/share/Steam`, each with its `libraryfolders.vdf`) and in `XIVLauncher.Core`'s game path (`GamePath` in `launcher.ini` of `~/.xlcore`, or of the flatpak's `~/.var/app/dev.goats.xivlauncher/.xlcore`; its `ffxiv` folder when none is set). **Choose another folder** opens the desktop's folder dialog, KDE's or GNOME's.
- **Finds the Wine prefixes the game runs in**: Steam's `steamapps/compatdata/39210/pfx` in the game's library, the `wineprefix` of each `XIVLauncher.Core` whose game path is this game, and the prefix whose `drive_c` holds the game. A prefix counts when it has `user.reg`; with none, the game has never started, and the installer asks the player to start it once.
- **Installs Angelica** into `~/.local/share/angelica` (`XDG_DATA_HOME`), or, for a game inside a flatpak's folder `~/.var/app/<id>/`, into `~/.var/app/<id>/data/angelica`, which that flatpak can see. `angelica.json` names it as the game sees it, under Wine's drive `Z:` (`Z:\home\deck\.local\sharengelica`).
- **Makes Wine load the proxy.** Wine prefers its own `dinput8.dll` to one in the game folder, so in each prefix's `user.reg` the installer sets `"dinput8"="native,builtin"` under `Software\Wine\AppDefaultsfxiv_dx11.exe\DllOverrides`: for the game's program only, the proxy first and Wine's own when there is none. Wine keeps a running prefix's registry in memory and writes it back when it stops, so the installer changes it only while no Wine server runs in the prefix (one whose working folder is `server-<device>-<inode>` of the prefix folder) and otherwise asks the player to wait.
- **Waits for the game to close**: a process named `ffxiv_dx11.exe`, or with it in its command line.
- **Never needs administrator rights**: the game folder is the player's own.
- **Registers itself** as *Remove Angelica* among the user's applications (`~/.local/share/applications/angelica-remove.desktop`), starting the copy of itself in the installation folder with `--uninstall`.
- `game.json` in the installation also records the prefixes it changed. Removing takes the override out of those prefixes, removing the key when nothing else is in it, under the same rule about a running prefix.

The interface's language follows `LC_ALL`, `LC_MESSAGES`, or `LANG`.

## Removing

The uninstaller, run from *Installed apps*, shows the same window: that Angelica's file leaves the game folder, and the choice to keep or delete the player's plugins and their data. It starts only while the game is closed, as installing does. It removes `dinput8.dll` (only when it is Angelica's proxy) and `angelica.json` from the game folder, asking for administrator rights the same way. Without the player's data it removes the installation folder; with them it leaves `plugins`, `grants`, `state`, and `storage` in place for a later installation.

## Building

`angelica-setup` is a crate of this workspace. The release workflow builds it for Windows and for Linux with the release's proxy, runtime, plugin installer, their signatures, and the channel embedded ([releases](../development/releases.md)); a build without them refuses to install and says that it is a development build. On Linux it draws its window through X11 or Wayland, whichever the desktop has, loading their libraries at run time.
