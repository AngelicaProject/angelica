# In-game interface

## Overlay

The host draws its interface over the game with **egui** and its own Direct3D 11 painter. It hooks the swap chain's `Present` and resizing through the swap chain's vtable, obtained from Direct3D 11 itself, and receives input through a window procedure hook. Neither depends on the game map, so the overlay works on a build the map does not cover and can always report what is disabled and why.

The overlay chains with other tools that draw over the game; it does not assume it is the only `Present` hook.

The bootstrap swap chain that finds `Present` is made on the software rasterizer (WARP), never on the graphics adapter, and released with its window once `Present` is hooked; DXGI, which holds the vtable and its functions, is pinned in the process instead. One kept on a GeForce RTX 3060 after its window was destroyed made the game's device fail with a graphics exception after some seconds. Its vtable is DXGI's and the same as the game's swap chain's. Bootstrap device creation requests the swap chain, device, feature level, and immediate context together. Graphics interposers can interpret a missing device output as a capability probe and omit swap-chain creation. Incomplete outputs fail initialization and release the bootstrap window; the host log distinguishes installed hooks from attachment to a rendering swap chain.

The menu key, `Insert` unless the player chose another, opens and closes the Angelica menu; `Esc` and the close button in its title bar close it, and the game starts with it closed. The game does not receive the menu key. Plugin windows are shown whether the menu is open or closed. In text fields, Ctrl+V pastes text from the Windows clipboard and Ctrl+C and Ctrl+X copy and cut to it. Mouse messages are consumed only while egui requests them. Keyboard messages reach Angelica's windows and are consumed only while a text field has focus — a pressed button or a chosen row does not take the keyboard — so the player moves and types in the game with the menu open, and a key never moves the focus between Angelica's controls. A key's or a mouse button's release always reaches the game, so one the game saw pressed never stays pressed for it, as when the camera turns with a button held and the pointer ends over Angelica's window; the one exception is `Esc` while the menu is open, which closes the menu and does not reach the game. System keys such as Alt+F4 always reach the game while the menu is open. A disabled overlay releases capture. The overlay draws into the game's back buffer on the game's immediate context during `Present`, skipping presents with `DXGI_PRESENT_TEST`: it saves every part of the context's state it sets (render targets, viewports, scissor rectangles, rasterizer, blend, and depth-stencil states, input layout, topology, vertex and index buffers, the vertex, hull, domain, geometry, and pixel shaders with their class instances, the first constant buffers, shader resource, and sampler) and puts it back after drawing. A frame's vertices and indices go into one dynamic vertex buffer and one dynamic index buffer that are made once and grow, not into new buffers every frame. When it attaches, it logs the graphics adapter, its driver, the device, and the game's swap chain; a device that stops is logged with its reason and whether it stopped before Angelica drew a frame. A frame with nothing to draw does not touch the back buffer or the context. The render target view on the back buffer is made once for that buffer and kept between frames, as proven overlays do, instead of being made again every frame; only a resize of the game's own swap chain drops it, before forwarding to the original function, and the interface keeps its windows, tabs, and text. A new swap chain of the game's window gets a new painter, with the interface kept. A partial update of a texture, as when new letters enter the font atlas, is written into the texture in place.

When rendering runs outside the startup thread, a registered Windows message schedules ticks and UI events on the window thread. Main callbacks run only if it matches the startup thread. The renderer uses cached window descriptions. A mismatch disables the overlay with a diagnostic.

## What the host shows

Angelica's interface is a small window over the game, the size of the game's own windows, and follows the same principles: a title bar, tabs, a dense list, actions in the row and in the right-click menu, details under the chosen row, and short actions at the bottom. It does not copy the game's look and does not dim the game. The window can be moved, and keeps its place while the game runs.

The face of Angelica uses plain words; ids, paths, permission names, logs, and technical errors appear only under **For developers**.

Every window of Angelica and of plugins has the same title bar, and each of its parts has one role. On the left is where the player is: the word `angelica` in lowercase, small and in a muted color, in the menu only; a dialog's title; or a plugin window's title, followed, when the plugin gives one, by the path below the window's top level with a back arrow, where the title and each earlier part lead back. Before the controls is what goes on: one short line with a mark, a dot in the "on" color or the attention mark. On the right edge are the window's controls. A long path is cut first and keeps at most six tenths of the room when there is a status; a status that does not fit is cut with an ellipsis and shown whole under the pointer; the controls always stay.

The menu's bar shows how many plugins wait for the player and how many run, a menu button (**Check for updates**, **Settings**, **For developers**), and the close button. A dialog's bar has its title and the close button. Below the menu's bar are the tabs **Installed**, **Catalog** ([catalog](./catalog.md#in-the-game)), and **Settings**; all three keep the same height.

**Installed** has a search field that filters by name and description, and a filter by permission class. The list is grouped under small section headers with a count and a thin rule: needs attention (awaiting approval, also when a new version asks for more, or stopped after a failure), on, and off, each sorted by name. The list scrolls; with fewer plugins it keeps the height of nine rows, and Settings have the same height as Installed, so the window keeps its size. A row is 36 pixels high and has, in columns:

- a square tile with rounded corners holding the first letter of the plugin's name;
- the name;
- a small icon for each permission class the plugin holds; a class it asks for and does not yet hold is drawn in the attention color;
- the version;
- the state: an attention mark (`!`) for a plugin that needs the player, a small dot in the "on" color for a running plugin, and a hollow ring for one that is off. An off plugin's text, tile, and icons are muted.

Under the pointer, a row shows its actions as words in place of its permissions and version: **Open** and **Turn off** for a running plugin, **Turn on** for one that is off, **Review permissions…** for one awaiting approval, and **Try again** and **Keep it off** for one stopped after a failure. A double click opens a running plugin. Clicking a row chooses it, and clicking it again lets it go; the chosen row is highlighted across the whole width and opens its details under it: the description, why it stopped after a failure, permissions in plain words (with the sites of a network permission), the version and where the plugin comes from (the Angelica catalog, or its authors), what the plugin installer did with it in this game session (installed, updated from one version to another, compiled again, or failed), and every action with the main one first, **Open folder**, **Copy report**, and **Remove…**. A right click opens a menu headed by the plugin's tile, name, and state, with the actions that apply: **Review permissions…** for a plugin awaiting approval, **Open** (which sends the plugin `open-requested` and brings its windows in front of the menu, which stays open) and **Turn off** for a running one, **Turn on** for one that is off, **Try again** and **Keep it off** for one stopped after a failure, **Open folder** for all but one awaiting approval, **Copy report**, and **Remove…**. Turning a plugin on or off is never a checkbox in its row.

**Copy report** puts on the clipboard what the player sends a plugin's author when something goes wrong, written by Angelica in English: Angelica's version, the game build, the map's state, the plugin's name, id, version, and origin, its state with the reason it stopped, its permissions, what the plugin installer did with it in this session, and the last lines of its log. A plugin tells about its work through its log; it has no report of its own and no access to the clipboard. After the copy the action reads **Report copied**.

Clicking a plugin awaiting approval, or **Review permissions…**, opens a small dialog window with its tile, name, version, and origin, and what it would be able to do, in plain words with the sites of a network permission. For a new version of an approved plugin, the dialog shows what it asks for the first time, highlighted in the attention color, and below it what is already allowed. Its actions are **Allow and turn on** and **Not now**, which leaves the plugin waiting. A permission is granted only there.

**Remove…** opens a dialog with the plugin's tile, name, and version and two ways to remove it: keeping its data (its storage folder, which a plugin with the same id finds again when installed later) or with its data. Removing stops the plugin and takes it out of the list at once ([lifecycle](./plugins.md#lifecycle)).

Below the list, a field takes an `https` link to a [plugin file](../formats/plugin-file-v1.md); the installed plugin appears in the list within a second and waits for approval.

**Settings** has a column of sections on the left, each with a small icon, on a background slightly darker than the window; the chosen section is a light rounded plate inset in that column and does not merge with the right side. The sections are:

- **General**: the key that opens Angelica, shown as a small key token; the language; the theme; the size of Angelica's windows: small, normal, or large. The size grows with the square root of the screen's height, so a high resolution makes the windows only a little larger. A click on the key token asks for another key, and the next key pressed becomes the menu key when it is `Insert`, `Delete`, `Home`, `End`, `Page Up`, `Page Down`, `Pause`, `Scroll Lock`, or `F1` to `F12`; `Esc` keeps the current key, and other keys are waited out. The new key applies at once;
- **Updates**: Angelica's version and its last check, the game map's, the channel, and **Check now** for plugins with when they were last checked in this session; plugin updates install automatically, and a version that needs new permissions waits for the player. Every note wraps within the window, and the section scrolls;
- **For developers**: the game map for the running build (whether a signed build record verified it, and every absent entry with the reason), each plugin with its id, version, state, folder, and recent log, what the plugin installer reported, and recent host diagnostics.

A setting is a row with its name on the left and its value on the right. Choices are short words, one word each where possible.

Enabling, disabling, approving, and retrying apply in the current session, on the main thread at the next frame.

While the menu is closed, nothing of Angelica is drawn except plugin windows and short notes. A note is a small window at the top right with `angelica`, a mark, and one line: how many plugins wait for the player and how many run, for the first seconds of the game and as long as a plugin needs the player; or what the plugin installer did (a plugin installed, a plugin updated with or without new permissions, or a link that could not be installed), for a few seconds. A note says which key opens Angelica and takes no input.

## Language

Angelica's own interface is in English and Russian. Without the player's choice, it follows the language of the Windows interface: Russian for Russian, English otherwise. The player's choices of language, theme, size, and menu key in Settings are kept in the installation's [state folder](../formats/host-installation-v1.md#angelica-settings). Plugins' windows show what plugins write.

## Look

Angelica's look applies to the menu and to plugin windows alike; plugin authors do nothing for it. There are two themes, light and dark; the player chooses in Settings, and the light theme is the default. Each theme is a fixed set of colors:

| Role | Light | Dark |
|---|---|---|
| Window, 96–97% opaque | `#F6F3ED` | `#1E1B19` |
| Section column in Settings | `#7A5C2E` at 8% over the window | `#181614` |
| Chosen item | white at 75% | `#2E2A26` |
| Text | `#24211C` | `#ECE6DC` |
| Secondary text | text at 60% | `#A8A094` |
| Muted text | text at 42% | `#80796F` |
| Lines and borders | `#3C2D14` at 12–22% | `#ECE6DC` at 8–13% |
| Action words, focus | `#7A5C2E` | `#E8735A` |
| Needs attention | `#B76E00` | `#E0A84E` |
| On | `#2E7D46` | `#7CC48E` |
| Danger | `#B23A3A` | `#EF8FA8` |

Color carries meaning only: the action color for what can be pressed or changed, and the attention, on, and danger colors for states. Everything else uses the text and line colors. There are no gradients, glows, or decorative animations.

Controls are words, not boxes. The main action of a window is a bold word in the action color followed by an arrow (**Allow and turn on →**); secondary actions are words in the secondary text color; an action in a list row is a word in the action color (**+ Install**). A choice is a row of short words with the chosen one in bold text color and the others muted. A list of values is the current value in the action color with a small chevron. A key is a small token in the action color with a thin border. Text fields keep a plain field with a thin border. Plugin windows are drawn with the same controls: a plugin's window has Angelica's frame and title bar, a heading is a small header with a thin rule, a button is an action word of its kind (main, normal, quiet, or danger), a fact is a name in the secondary color and its value, an entry is a row like the rows of the menu's list — tile, title, note, value, mark, actions under the pointer, and the chosen row's facts and actions under it — a checkbox is its label with the choice of **On** and **Off**, a choice is its current value with a chevron and a menu, tabs are the menu's tabs, and progress is a thin line that fills with the action color. Number fields and sliders keep egui's widgets in the theme's colors.

Windows have 6-pixel corners, a 1-pixel border, and a soft shadow. A title bar is 32 pixels high, list rows 36 pixels, and section headers are small text in the secondary color with a thin rule. Three fonts have three roles: Golos Text for everything the player reads and presses, Quicksand for the word `angelica`, and JetBrains Mono for ids, paths, and logs under **For developers**.

## Plugin windows

Plugins do not draw. Every frame a plugin returns the windows it wants shown as lists of widgets ([windows](../formats/plugin-api-v1.md#windows)), and the host lays them out and draws them, so the renderer can change without affecting plugins. Windows scroll vertically when their contents exceed their height.

The host owns fonts, scaling, colors, and window frames. A plugin window's title bar shows the window's title, its path and status when the plugin gives them, and three controls the host draws for every plugin: collapse, a menu (**Open folder**, **Turn off**), and close. A new window opens in the middle of the screen, 480 by 460 points, each next one a step lower and to the right, and stays where the player moves it for the game session. It keeps its size when its content changes: what does not fit scrolls inside it, and the player resizes it by its corner. A collapsed window is its title bar alone, with the title, the status, and a button that expands it; it is a small window with its own place on the screen, so a collapsed window serves as a small indicator while the player plays. Which windows are collapsed is kept for the game session. A plugin cannot draw outside its windows or show Angelica's own controls. The host bundles its fonts for Latin and Cyrillic ([dependencies](../development/dependencies.md#fonts)); after them, the Windows adapter uses the installed Segoe UI font and the first available Meiryo, MS Gothic, or Yu Gothic font for Japanese.
