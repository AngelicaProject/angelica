---
order: 1
section: Play
kicker: Get started
meta: About 2 minutes · Windows
prev: 
next: Turn on a plugin
---

# Install Angelica

Angelica installs beside FINAL FANTASY XIV and starts together with the game.

## Before you begin

You need:

- Windows 10 or Windows 11.
- FINAL FANTASY XIV for Windows. Angelica works with the Steam version, the Square Enix launcher, and XIVLauncher.

Other tools you use, such as Dalamud or ReShade, can stay installed.

## Install

1. Close the game, if it is running.
1. Download [`angelica-setup.exe`](https://github.com/AngelicaProject/angelica/releases/download/nightly/angelica-setup.exe) and run it.

   The first time, Windows might say **Windows protected your PC**. Select **More info**, then **Run anyway**.
   Windows shows this for programs that are not signed with a paid certificate. Angelica still checks the signature of every file it installs or loads.

1. Check the game folder. The installer looks for the game in your Steam libraries, in the Square Enix launcher's folder, and in XIVLauncher's settings.

   If it found the wrong game or none, select **Choose another folder** and pick the folder of the game.

1. Select **Install**.

   If the game is in a folder like `Program Files`, Windows asks for administrator rights. The installer needs them only to put two files into the game folder.

1. When the installer says it is done, it shows the key that opens Angelica. Select **Close**.

## Open Angelica in the game

1. Start the game.

   For the first seconds, a small note at the top right says that Angelica is running and which key opens it.

1. Press <kbd>Insert</kbd>, type `/angelica` in the chat, or select **Angelica** in the game's **System** menu.

   The Angelica window opens over the game.

1. To close it, press <kbd>Insert</kbd> again, type `/angelica` again, or press <kbd>Esc</kbd>.

## What the installer changes

- Angelica goes to `%LOCALAPPDATA%\Angelica`, for your Windows user only.
- Two files go into the game's `game` folder: `dinput8.dll`, which starts Angelica with the game, and `angelica.json`, which tells it where Angelica is. Nothing else in the game folder changes.
- Angelica appears in **Settings > Apps > Installed apps**, where you can [remove it](remove.md).

You do not need to run the installer again. Angelica checks for updates while you play, and a new version starts the next time you start the game.
After a game patch, Angelica gets what it needs for the new version of the game by itself.

## Next steps

- [Turn on a plugin](turn-on-a-plugin.md)

## Troubleshooting

### The installer says that another program's `dinput8.dll` is in the game folder

Another mod or overlay uses the same file to start with the game, and Angelica never replaces another program's file.
To install Angelica, remove that program or move its `dinput8.dll` out of the game folder, then run the installer again.

### The installer says that the game is running

Close the game, then select **Try again**.
If the game is already closed, wait a few seconds until it has fully exited, then try again.

### The installer does not find the game

Select **Choose the game folder** and pick the folder that has a `game` folder with `ffxiv_dx11.exe` inside. For example:

| Version | Usual folder |
|---|---|
| Steam | `C:\Program Files (x86)\Steam\steamapps\common\FINAL FANTASY XIV Online` |
| Square Enix launcher | `C:\Program Files (x86)\SquareEnix\FINAL FANTASY XIV - A Realm Reborn` |

### Nothing happens when I press Insert

- If you chose another key in Angelica's settings, press that key.
- If no note appeared at the top right when the game started, Angelica did not start. Check that `dinput8.dll` and `angelica.json` are in the game's `game` folder, and run the installer again.
