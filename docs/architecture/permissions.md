# Permissions

A plugin's manifest declares its permissions ([manifest](../formats/plugin-manifest-v1.md)). The player sees them before approving the plugin, and approval grants exactly those. A new version that declares a permission or an `http.hosts` entry the grant does not cover waits for approval again.

Every plugin, once approved, has its own windows, log, storage folder, and jobs; these need no permission. Everything that reaches the game needs one:

| Permission | Allows | Class | Shown to the player as |
| --- | --- | --- | --- |
| `excel.text` | activating [text tables](../formats/plugin-api-v1.md#text-tables) that replace text of the game's data sheets | what you see | Replaces text the game shows from its data sheets |
| `text.case` | adding letters to the game's [letter case conversion](../formats/plugin-api-v1.md#letter-case) | what you see | Changes how the game capitalizes letters |
| `files.replace` | serving [replacement game files](../formats/plugin-api-v1.md#replacement-files) of formats the host checks | what you see | Replaces game files: fonts and textures |
| `chat.commands` | defining [chat commands](../formats/plugin-api-v1.md#commands) the host parses for the plugin; the game's own commands stay the game's | only for you | Adds its own chat commands |
| `chat.print` | printing [messages](../formats/plugin-api-v1.md#chat) in the chat log, always under the plugin's name | only for you | Prints its messages in your chat log |
| `http.get` | HTTPS GET [requests](../formats/plugin-api-v1.md#http) to the hosts the manifest lists in `http.hosts` | internet | Downloads from these internet hosts, followed by the list |
| `chat.read` | receiving the [messages](../formats/plugin-api-v1.md#events) the game adds to the chat log, including other players' | other players | Reads the messages in your chat log, including other players' |
| `gameui.hide` | keeping named elements of the game's own [windows](../formats/plugin-api-v1.md#game-windows) hidden | what you see | Hides parts of the game's interface |
| `gameui.text` | putting text in named text elements of the game's own [windows](../formats/plugin-api-v1.md#game-windows) | what you see | Puts its own text in the game's windows |
| `state.self` | reading the [game state](../formats/plugin-api-v1.md#game-state) of the `self` domain: the player's character and where the player is | only for you | Reads your character: name, class, level, HP and MP, position, and zone |

The player sees permissions in plain words, grouped by class, the most serious first: **Uses the internet**, **Sees other players**, **Changes what you see**, **Only for you**. Permission names stay with authors and are not shown. Every new permission belongs to one class; operations that act for the player will form a class of their own, above the others.

Permissions follow game domains and are as narrow as the domain allows: reading a domain's state, receiving its events, and each kind of operation on it are separate permissions. Automation is the use of operations, so each kind of action a plugin may take for the player is its own permission, shown to the player in plain words.

A permission exists only together with the API operation it guards. A capability the API does not offer has no permission; it is added with the operation that needs it, as a narrow permission describing that operation.

## Grants

Requests reach the network the way the player's browser does, since players behind a proxy or a VPN, or with a host mapped in the `hosts` file, need plugins to work there as well. The guarantees rest on what stays: only the approved hosts, only HTTPS on port 443, and a certificate the system trusts for the requested host. The home network (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, link-local and shared ranges) stays out of reach of a direct connection; this computer and the VPN range `198.18.0.0/15` do not, because a local proxy or a VPN uses them. A plugin whose listed host resolves to this computer connects to port 443 there and learns whether something listens on it, but no request reaches a service that holds no certificate for that host. A proxy sees the hosts a request goes to, as it does for the browser; a proxy that inspects TLS is one the player trusted with its certificate. Angelica's own requests to fixed hosts (updates, the map, the catalog) reach whatever the system resolves, because every file they bring carries a release signature: a redirected address can only make a download fail.

The host records the player's approval in `grants/<plugin-id>.json` ([installation](../formats/host-installation-v1.md#player-grants)). A plugin runs only with a grant that covers every permission its manifest declares; an operation checks both the declaration and the grant. A package without a grant appears in the Angelica menu with its permissions and an approval button, and runs no code. When the player approved an earlier version, the menu shows only what the new version adds: permissions the grant does not cover, and for `http.get` only the new hosts. Approving replaces the grant with everything the new version declares.

## Data no permission exposes

Session tokens, login data, and other data that could take over the account are not available through any operation.
