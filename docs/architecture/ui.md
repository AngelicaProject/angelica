# In-game interface

## Overlay

The host draws its interface over the game with **egui** and its own Direct3D 11 painter. It hooks the swap chain's `Present` and resizing through the swap chain's vtable, obtained from Direct3D 11 itself, and receives input through a window procedure hook. Neither depends on the game map, so the overlay works on a build the map does not cover and can always report what is disabled and why.

The overlay chains with other tools that draw over the game; it does not assume it is the only `Present` hook.

## What the host shows

- Angelica's menu: installed plugins, their permissions and state, the catalog, updates, logs.
- The state of the map for the running build: verified, unverified, and absent entries, and which plugins wait for them.
- Plugin windows.

## Plugin windows

Plugins do not draw. They describe their interface through the UI interface of the plugin API in an immediate-mode style: every frame a plugin's window callback declares its widgets, and the host lays them out and draws them. The renderer is therefore a host detail that can change without affecting plugins.

The host owns fonts (including glyphs for Cyrillic and Japanese), scaling, colors, and the frames of plugin windows. A plugin cannot draw outside its windows or imitate Angelica's permission dialogs.
