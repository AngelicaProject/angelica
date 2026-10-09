# In-game interface

## Overlay

The host draws its interface over the game with **egui** and its own Direct3D 11 painter. It hooks the swap chain's `Present` and resizing through the swap chain's vtable, obtained from Direct3D 11 itself, and receives input through a window procedure hook. Neither depends on the game map, so the overlay works on a build the map does not cover and can always report what is disabled and why.

The overlay chains with other tools that draw over the game; it does not assume it is the only `Present` hook.

Bootstrap device creation requests the swap chain, device, feature level, and immediate context together. Graphics interposers can interpret a missing device output as a capability probe and omit swap-chain creation. Incomplete outputs fail initialization and release the bootstrap window; the host log distinguishes installed hooks from attachment to a rendering swap chain.

`Insert` toggles visibility. Mouse and keyboard messages are consumed only while egui requests them; hidden or disabled overlays release capture. Rendering uses a private deferred context and executes its command list with immediate-context state restoration. No back-buffer references survive a completed frame; resizing drops frame resources before forwarding to the original function.

When rendering runs outside the startup thread, a registered Windows message schedules ticks and UI events on the window thread. Main callbacks run only if it matches the startup thread. The renderer uses cached window descriptions. A mismatch disables the overlay with a diagnostic.

## What the host shows

The Angelica menu lists:

- the game map for the running build: whether a signed build record verified it, and every absent entry with the reason;
- each installed plugin: name, id, version, description, and state. A plugin awaiting approval shows its permissions in plain words, grouped by [class](./permissions.md), and an approval button; a new version of an approved plugin shows only what it adds. An approved plugin has Enable or Disable, Open (which sends the plugin `open-requested`), Open plugin folder, and its recent log. A faulted plugin shows the reason, Retry, and Keep disabled;
- recent host diagnostics.

Enabling, disabling, approving, and retrying apply in the current session, on the main thread at the next frame.

## Plugin windows

Plugins do not draw. Every frame a plugin returns the windows it wants shown as lists of widgets ([windows](../formats/plugin-api-v1.md#windows)), and the host lays them out and draws them, so the renderer can change without affecting plugins. Windows scroll vertically when their contents exceed their height.

The host owns fonts, scaling, colors, and window frames. A window's title includes the plugin's name, and a plugin cannot draw outside its windows or show Angelica's own controls. The Windows adapter uses the installed Segoe UI font for Latin and Cyrillic and the first available Meiryo, MS Gothic, or Yu Gothic font for Japanese; fonts are not bundled.
