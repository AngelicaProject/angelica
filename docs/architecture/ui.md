# In-game interface

## Overlay

The host draws its interface over the game with **egui** and its own Direct3D 11 painter. It hooks the swap chain's `Present` and resizing through the swap chain's vtable, obtained from Direct3D 11 itself, and receives input through a window procedure hook. Neither depends on the game map, so the overlay works on a build the map does not cover and can always report what is disabled and why.

The overlay chains with other tools that draw over the game; it does not assume it is the only `Present` hook.

Bootstrap device creation requests the swap chain, device, feature level, and immediate context together. Graphics interposers can interpret a missing device output as a capability probe and omit swap-chain creation. Incomplete outputs fail initialization and release the bootstrap window; the host log distinguishes installed hooks from attachment to a rendering swap chain.

`Insert` opens and closes the Angelica menu; `Esc` and "back to the game" close it, and the game starts with it closed. Plugin windows are shown while the menu is closed. In text fields, Ctrl+V pastes text from the Windows clipboard and Ctrl+C and Ctrl+X copy and cut to it. Mouse messages are consumed only while egui requests them. Keyboard messages are consumed while a text field has focus and, except system keys such as Alt+F4, while the menu is open. A disabled overlay releases capture. Rendering uses a private deferred context and executes its command list with immediate-context state restoration. No back-buffer references survive a completed frame; resizing drops frame resources before forwarding to the original function.

When rendering runs outside the startup thread, a registered Windows message schedules ticks and UI events on the window thread. Main callbacks run only if it matches the startup thread. The renderer uses cached window descriptions. A mismatch disables the overlay with a diagnostic.

## What the host shows

The face of Angelica uses plain words; ids, paths, permission names, logs, and technical errors appear only on a plugin card's back and under **For developers**.

The menu covers the game with a dimmed night layer. Its header has Angelica's living feather, the tabs, a line with how many plugins wait for the player and how many run, and the way back to the game.

**My plugins** lists the installed plugins in groups: waiting for the player (awaiting approval, or stopped after a failure), on, and off, each sorted by name. A search field filters by name and description; the list scrolls and draws only its visible rows. A row has a light for the plugin's state, its name and description, and a switch, or what it waits for. The chosen plugin's card is on the right, joined to its row by a dashed leader:

- the face says where the plugin comes from: the Angelica catalog, or its author, outside the catalog. A plugin awaiting approval shows its permissions in plain words, grouped by [class](./permissions.md), each class with its color, and **Allow and turn on**; a new version of an approved plugin shows only what it adds. An approved plugin has Open (which sends the plugin `open-requested` and closes the menu), its switch, and its version. A faulted plugin has Try again and Keep it off;
- **How it works** turns the card: the back lists what the plugin may do, what it cannot, its sandbox memory and frame time limit, who signed it, its version, and its id. The turn squashes the card horizontally, changes the side, and opens it again.

Below the list, a field takes an `https` link to a [plugin file](../formats/plugin-file-v1.md); the installed plugin appears in the list within a second and waits for approval.

**Settings** has the language, the key that opens Angelica, how plugins update, and **For developers**: the game map for the running build (whether a signed build record verified it, and every absent entry with the reason), each plugin with its id, version, state, folder, and recent log, what the plugin installer reported, and recent host diagnostics.

Enabling, disabling, approving, and retrying apply in the current session, on the main thread at the next frame.

While the menu is closed, a small living feather in the bottom right corner shows Angelica's state without taking input: it breathes when all is well, a pulse runs along its barbs while the plugin installer works, a warm node glows when a plugin waits for the player, and its light dims when a plugin stopped after a failure. For the first seconds of the game, and while something needs the player, it shows that `Insert` opens Angelica. Short notes at the top right tell what the plugin installer did: a plugin installed, a plugin updated with or without new permissions, or a link that could not be installed.

## Language

Angelica's own interface is in English and Russian. Without the player's choice, it follows the language of the Windows interface: Russian for Russian, English otherwise. The player's choice in Settings is kept in the installation's [state folder](../formats/host-installation-v1.md#angelica-settings). Plugins' windows show what plugins write.

## Look

Angelica's style applies to the menu and to plugin windows alike; plugin authors do nothing for it. Its colors are a dark night glass with pearl text, one warm light for the main action and for what waits for the player, and pale blueprint lines for the mechanism. Three fonts have three roles: Cormorant Garamond italic for names and titles, Manrope for what the player reads and presses, and JetBrains Mono for the mechanism. Animations are drawn by egui each frame and kept cheap: the living feather is a few hundred small dots, and the large blueprint feather behind the menu does not move.

## Plugin windows

Plugins do not draw. Every frame a plugin returns the windows it wants shown as lists of widgets ([windows](../formats/plugin-api-v1.md#windows)), and the host lays them out and draws them, so the renderer can change without affecting plugins. Windows scroll vertically when their contents exceed their height.

The host owns fonts, scaling, colors, and window frames. A window's title includes the plugin's name, and a plugin cannot draw outside its windows or show Angelica's own controls. The host bundles its fonts for Latin and Cyrillic ([dependencies](../development/dependencies.md#fonts)); after them, the Windows adapter uses the installed Segoe UI font and the first available Meiryo, MS Gothic, or Yu Gothic font for Japanese.
