# Permissions

A plugin's manifest declares its permissions ([manifest](../formats/plugin-manifest-v1.md)). The player sees them before approving the plugin, and approval grants exactly those. A new version that declares a permission or an `http.hosts` entry the grant does not cover waits for approval again.

Every plugin, once approved, has its own windows, log, storage folder, and jobs; these need no permission. Everything that reaches the game needs one:

| Permission | Allows | Shown to the player as |
| --- | --- | --- |
| `excel.text` | activating [text tables](../formats/plugin-api-v1.md#text-tables) that replace text of the game's data sheets | Replaces text the game shows from its data sheets |
| `text.case` | adding letters to the game's [letter case conversion](../formats/plugin-api-v1.md#letter-case) | Changes how the game capitalizes letters |
| `files.replace` | serving [replacement game files](../formats/plugin-api-v1.md#replacement-files) of formats the host checks | Replaces game files: fonts and textures |
| `chat.commands` | defining [chat commands](../formats/plugin-api-v1.md#commands) the host parses for the plugin; the game's own commands stay the game's | Adds its own chat commands |
| `chat.print` | printing [messages](../formats/plugin-api-v1.md#chat) in the chat log, always under the plugin's name | Prints its messages in your chat log |
| `http.get` | HTTPS GET [requests](../formats/plugin-api-v1.md#http) to the public hosts the manifest lists in `http.hosts` | Downloads from these internet hosts, followed by the list |
| `chat.read` | receiving the [messages](../formats/plugin-api-v1.md#events) the game adds to the chat log, including other players' | Reads the messages in your chat log, including other players' |
| `state.self` | reading the [game state](../formats/plugin-api-v1.md#game-state) of the `self` domain: the player's character and where the player is | Reads your character: name, class, level, HP and MP, position, and zone |

Permissions follow game domains and are as narrow as the domain allows: reading a domain's state, receiving its events, and each kind of operation on it are separate permissions. Automation is the use of operations, so each kind of action a plugin may take for the player is its own permission, shown to the player in plain words.

A permission exists only together with the API operation it guards. A capability the API does not offer has no permission; it is added with the operation that needs it, as a narrow permission describing that operation.

## Grants

The host records the player's approval in `grants/<plugin-id>.json` ([installation](../formats/host-installation-v1.md#player-grants)). A plugin runs only with a grant that covers every permission its manifest declares; an operation checks both the declaration and the grant. A package without a grant appears in the Angelica menu with its permissions and an approval button, and runs no code.

## Data no permission exposes

Session tokens, login data, and other data that could take over the account are not available through any operation.
