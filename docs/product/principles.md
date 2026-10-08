# Product principles

1. **The player's system is never exposed.** Plugin code runs only in the sandbox. Anything outside the game (files, network, audio, clipboard) reaches a plugin only as a host service behind a permission the player granted. There is no native plugin code and no plugin-supplied process.
2. **A plugin cannot take the game down.** A trap, a panic, a timeout, or a limit hit by a plugin disables that plugin and lets the original game code run. Angelica's own failures disable Angelica, not the game.
3. **Plugins name, the map locates.** A plugin refers to game functions, fields, and patches by game map entry names. Addresses and offsets exist only in the game map.
4. **A patch breaks only what changed.** An entry that is not verified for the running build is absent; only plugins and features that need it stop. Everything else keeps working.
5. **Unknown means off.** An unknown build, an ambiguous signature, or a failed consistency check makes an entry absent. Angelica never runs on a guess.
6. **Permissions are visible, narrow, and explicit.** A plugin gets exactly the permissions its manifest declares and the player granted. A new version that declares more asks again. Data that could compromise the account is never exposed through any permission.
7. **Hot paths are declarative.** Work the game does thousands of times per frame (resolving file paths, reading metadata) is executed by the host from rules a plugin configures, not by calling into plugin code each time.
8. **Contracts are versioned.** The plugin API, the plugin manifest, and the game map format are versioned. A newer host keeps running plugins built for an older API version of the same major version.
9. **Angelica coexists.** Angelica works alongside graphics injectors and other in-process tools, chains hooks with them, and never requires their absence.
10. **Agents are first-class users.** Every contract is machine-readable, every tool can report in JSON, and every failure names what failed.
11. **Documentation stays current.** Architecture and development documentation evolve with the code.
12. **Plugin purposes are open-ended.** The API exposes reusable game and host capabilities that authors compose into their own applications. A plugin's identity or category never determines its API; verified resource contracts and explicit permissions do.
