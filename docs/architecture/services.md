# Host services

A host service is a general capability implemented natively in the host and offered to plugins through the API. A service is added when a kind of plugin needs something that cannot be done safely or fast enough from plugin code. A service is never specific to one plugin.

## Services

| Service | Purpose | Permission |
| --- | --- | --- |
| Game data | Sheets and rows of the game's Excel data, files from the game's archives | `game.read` |
| Excel rows | Observe and rewrite rows as the game stores them | `game.hook` on the row store entries |
| File redirection | Serve a replacement for a game file path, per collection and per character | `files.redirect` |
| Character metadata | Per-character changes to the game's equipment and body metadata tables | `files.redirect` |
| Redraw | Redraw a character after its files or metadata changed | `game.call` |
| Game windows | Read the game's window tree and agents, operate windows | `game.read`, `game.act` |
| Player actions | Use actions, move items | `game.act` |
| Chat and commands | Print to chat, register commands | `ui` |
| Overlay UI | Windows and widgets drawn by the host | `ui` |
| World drawing | Lines, shapes, and markers at world positions | `ui` |
| File validation | Check that a file has a valid structure for its game format before the game reads it | applied to every redirected file |
| Network | HTTP and WebSocket to declared domains, streamed to the plugin's folder | `network` |
| Storage | The plugin's own folder and a folder the player chose | `storage`, `folder` |
| Audio, clipboard | Play sound, use the clipboard | `audio`, `clipboard` |

## Declarative hot paths

File redirection and character metadata run on the game's resource threads thousands of times during loading. Plugins do not handle these calls; they give the service rules (collections of path replacements, which collection applies to which character, metadata changes), and the service applies them natively. A plugin changes the rules at any time; the service takes the new rules at a consistent point.

## Validation of external files

Every file served through file redirection is validated for its format before the game reads it. A file that fails is not served, and the plugin that supplied it is told why. Files downloaded from other players are parsed inside the downloading plugin's sandbox first, so a malformed file can crash at most that plugin's instance.
