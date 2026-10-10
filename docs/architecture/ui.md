# In-game interface

## Overlay

The host draws its interface over the game with **egui** and its own Direct3D 11 painter. It hooks the swap chain's `Present` and resizing through the swap chain's vtable, obtained from Direct3D 11 itself, and receives input through a window procedure hook. Neither depends on the game map, so the overlay works on a build the map does not cover and can always report what is disabled and why.

The overlay chains with other tools that draw over the game; it does not assume it is the only `Present` hook.

Bootstrap device creation requests the swap chain, device, feature level, and immediate context together. Graphics interposers can interpret a missing device output as a capability probe and omit swap-chain creation. Incomplete outputs fail initialization and release the bootstrap window; the host log distinguishes installed hooks from attachment to a rendering swap chain.

`Insert` opens and closes the Angelica menu; `Esc` and the close button in its title bar close it, and the game starts with it closed. Plugin windows are shown while the menu is closed. In text fields, Ctrl+V pastes text from the Windows clipboard and Ctrl+C and Ctrl+X copy and cut to it. Mouse messages are consumed only while egui requests them. Keyboard messages are consumed while a text field has focus and, except system keys such as Alt+F4, while the menu is open. A disabled overlay releases capture. Rendering uses a private deferred context and executes its command list with immediate-context state restoration. No back-buffer references survive a completed frame; resizing drops frame resources before forwarding to the original function.

When rendering runs outside the startup thread, a registered Windows message schedules ticks and UI events on the window thread. Main callbacks run only if it matches the startup thread. The renderer uses cached window descriptions. A mismatch disables the overlay with a diagnostic.

## What the host shows

Angelica's interface is a small window over the game, the size of the game's own windows, and follows the same principles: a title bar, tabs, a dense list, details in a tooltip and in the right-click menu, and short actions at the bottom. It does not copy the game's look and does not dim the game. The window can be moved, and keeps its place while the game runs.

The face of Angelica uses plain words; ids, paths, permission names, logs, and technical errors appear only under **For developers**.

The title bar has the word `angelica` in lowercase, small and in a muted color, and a close button. Below it are the tabs **Installed** and **Settings**.

**Installed** has a search field that filters by name and description, and a filter by permission class. The list is grouped under small section headers with a count and a thin rule: needs attention (awaiting approval, also when a new version asks for more, or stopped after a failure), on, and off, each sorted by name. The list scrolls and draws only its visible rows; with fewer plugins it keeps the height of nine rows, and Settings have the same height as Installed, so the window keeps its size. A row is 36 pixels high and has, in columns:

- a square tile with rounded corners holding the first letter of the plugin's name;
- the name;
- a small icon for each permission class the plugin holds; a class it asks for and does not yet hold is drawn in the attention color;
- the version;
- the state: an attention mark (`!`) for a plugin that needs the player, a small dot in the "on" color for a running plugin, and a hollow ring for one that is off. An off plugin's text, tile, and icons are muted.

The chosen row is highlighted across the whole width. Hovering a row shows a tooltip with the name, version, description, permissions in plain words (with the sites of a network permission), state, and where the plugin comes from: the Angelica catalog, or its authors. A right click opens a menu headed by the plugin's tile, name, and state, with the actions that apply: **Review permissions…** for a plugin awaiting approval, **Open** (which sends the plugin `open-requested` and closes the menu) and **Turn off** for a running one, **Turn on** for one that is off, **Try again** and **Keep it off** for one stopped after a failure, and **Open folder** for all but one awaiting approval. Turning a plugin on or off is never a checkbox in its row.

Clicking a plugin awaiting approval, or **Review permissions…**, opens a small dialog window with its tile, name, version, and origin, and what it would be able to do, in plain words with the sites of a network permission. For a new version of an approved plugin, the dialog shows what it asks for the first time, highlighted in the attention color, and below it what is already allowed. Its actions are **Allow and turn on** and **Not now**, which leaves the plugin waiting. A permission is granted only there.

Below the list, a field takes an `https` link to a [plugin file](../formats/plugin-file-v1.md); the installed plugin appears in the list within a second and waits for approval.

**Settings** has a column of sections on the left, each with a small icon, on a background slightly darker than the window; the chosen section is a light rounded plate inset in that column and does not merge with the right side. The sections are:

- **General**: the key that opens Angelica, `Insert`, shown as a small key token; the language; the theme;
- **Updates**: plugins update by themselves, and a version that asks for more waits for the player;
- **For developers**: the game map for the running build (whether a signed build record verified it, and every absent entry with the reason), each plugin with its id, version, state, folder, and recent log, what the plugin installer reported, and recent host diagnostics.

A setting is a row with its name on the left and its value on the right. Choices are short words, one word each where possible.

Enabling, disabling, approving, and retrying apply in the current session, on the main thread at the next frame.

While the menu is closed, nothing of Angelica is drawn except plugin windows and short notes. A note is a small window at the top right with `angelica`, a mark, and one line: how many plugins wait for the player and how many run, for the first seconds of the game and as long as a plugin needs the player; or what the plugin installer did (a plugin installed, a plugin updated with or without new permissions, or a link that could not be installed), for a few seconds. A note says that `Insert` opens Angelica and takes no input.

## Language

Angelica's own interface is in English and Russian. Without the player's choice, it follows the language of the Windows interface: Russian for Russian, English otherwise. The player's choices of language and theme in Settings are kept in the installation's [state folder](../formats/host-installation-v1.md#angelica-settings). Plugins' windows show what plugins write.

## Look

Angelica's look applies to the menu and to plugin windows alike; plugin authors do nothing for it. There are two themes, light and dark; the player chooses in Settings, and the light theme is the default. Each theme is a fixed set of colors:

| Role | Light | Dark |
|---|---|---|
| Window, 96–97% opaque | `#F6F3ED` | `#2E3138` |
| Section column in Settings | `#7A5C2E` at 8% over the window | `#282A30` |
| Chosen item | white at 75% | `#3C3F47` |
| Text | `#24211C` | `#DCDDE0` |
| Secondary text | text at 60% | `#A3A6AD` |
| Muted text | text at 42% | `#80838B` |
| Lines and borders | `#3C2D14` at 12–22% | white at 8–13% |
| Action words, focus | `#7A5C2E` | `#B4ABD6` |
| Needs attention | `#B76E00` | `#E0A84E` |
| On | `#2E7D46` | `#7CC48E` |
| Danger | `#B23A3A` | `#E58A8A` |

Color carries meaning only: the action color for what can be pressed or changed, and the attention, on, and danger colors for states. Everything else uses the text and line colors. There are no gradients, glows, or decorative animations.

Controls are words, not boxes. The main action of a window is a bold word in the action color followed by an arrow (**Allow and turn on →**); secondary actions are words in the secondary text color; an action in a list row is a word in the action color (**+ Install**). A choice is a row of short words with the chosen one in bold text color and the others muted. A list of values is the current value in the action color with a small chevron. A key is a small token in the action color with a thin border. Text fields keep a plain field with a thin border. Plugin windows keep egui's widgets in the theme's colors.

Windows have 6-pixel corners, a 1-pixel border, and a soft shadow. A title bar is 32 pixels high, list rows 36 pixels, and section headers are small text in the secondary color with a thin rule. Three fonts have three roles: Golos Text for everything the player reads and presses, Quicksand for the word `angelica`, and JetBrains Mono for ids, paths, and logs under **For developers**.

## Plugin windows

Plugins do not draw. Every frame a plugin returns the windows it wants shown as lists of widgets ([windows](../formats/plugin-api-v1.md#windows)), and the host lays them out and draws them, so the renderer can change without affecting plugins. Windows scroll vertically when their contents exceed their height.

The host owns fonts, scaling, colors, and window frames. A window's title includes the plugin's name, and a plugin cannot draw outside its windows or show Angelica's own controls. The host bundles its fonts for Latin and Cyrillic ([dependencies](../development/dependencies.md#fonts)); after them, the Windows adapter uses the installed Segoe UI font and the first available Meiryo, MS Gothic, or Yu Gothic font for Japanese.
