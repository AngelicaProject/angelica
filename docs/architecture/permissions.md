# Permissions

A plugin's manifest declares its permissions ([manifest](../formats/plugin-manifest-v1.md)). The player sees them before approving the plugin, and approval grants exactly those. A new version that declares a permission or an `http.hosts` entry the grant does not cover waits for approval again.

Every plugin, once approved, has its own windows, log, storage folder, and jobs; these need no permission. Everything that reaches the game needs one: a permission of the host for what the host does for the plugin, and a [domain of the game map](#domains-of-the-game-map) for what the plugin reads of the game or does in it.

| Permission | Allows | Class | Shown to the player as |
| --- | --- | --- | --- |
| `excel.text` | activating [text tables](../formats/plugin-api-v1.md#text-tables) that replace text of the game's data sheets | what you see | Replaces text the game shows from its data sheets |
| `text.case` | adding letters to the game's [letter case conversion](../formats/plugin-api-v1.md#letter-case) | what you see | Changes how the game capitalizes letters |
| `files.replace` | serving [replacement game files](../formats/plugin-api-v1.md#replacement-files) of formats the host checks | what you see | Replaces game files: fonts and textures |
| `chat.commands` | defining [chat commands](../formats/plugin-api-v1.md#commands) the host parses for the plugin; the game's own commands stay the game's | only for you | Adds its own chat commands |
| `chat.print` | printing [messages](../formats/plugin-api-v1.md#chat) in the chat log, always under the plugin's name | only for you | Prints its messages in your chat log |
| `http.get` | HTTPS GET [requests](../formats/plugin-api-v1.md#http) to the hosts the manifest lists in `http.hosts` | internet | Downloads from these internet hosts, followed by the list |
| `http.send` | sending data with POST, PUT, PATCH, and DELETE [requests](../formats/plugin-api-v1.md#http) to the same hosts; needs `http.get` | internet | Sends data to these internet hosts |
| `chat.read` | receiving the [messages](../formats/plugin-api-v1.md#events) the game adds to the chat log, including other players' | other players | Reads the messages in your chat log, including other players' |
| `gameui.hide` | keeping named elements of the game's own [windows](../formats/plugin-api-v1.md#game-windows) hidden | what you see | Hides parts of the game's interface |
| `gameui.text` | putting text in named text elements of the game's own [windows](../formats/plugin-api-v1.md#game-windows) | what you see | Puts its own text in the game's windows |

## Domains of the game map

Reading the game's objects through [Lens](./lens.md) needs a permission per domain of the game map's [object model](../formats/game-map-v2.md#domains), such as `inventory` (the items the player carries) or `character` (the player's name, class, level, HP, and MP). A plugin lists them in its manifest's `[lens] read`; the player sees each in the words the map gives it, in the group of its class. A query may read only fields of the domains the plugin holds; for an array whose elements belong to different domains, only the elements of those domains. The map's maintainer approves a new domain and its wording once; fields and types join a domain without a new approval, so what a domain lets a plugin read grows only within what its wording says.

Acting in a domain through Lens [actions](../formats/plugin-api-v1.md#lens) is a permission of its own: the manifest lists the domain in `[lens] act`, and the player sees the domain's `act` words in the class **Acts in the game for you**, above every other class, with a warning that acting for the player in the game breaks the game's terms of service and the game's company can suspend the account for it. A domain the map describes without `act` words has no actions and is never granted for acting.

Changing values of a domain through Lens [`set`](../formats/plugin-api-v1.md#lens) is a permission of its own too: the manifest lists the domain in `[lens] change`, and the player sees the domain's `change` words in the class **Changes what you see**. Only values that exist on the player's computer alone are ever changeable, and the host puts every one back when the plugin stops. A rule at a decision point ([`decide`](../formats/plugin-api-v1.md#lens)) changes what the game does there for the player alone, and needs the same permission for the event's domain.

## Acting for the player

Whatever a plugin does in the game for the player, the player sees and stops at once:

- While a plugin has acted in the last 3 seconds, a note at the top right names it and the stop key.
- The stop key, `End` unless the player chose another in Settings, stops every plugin from acting at once: every action is refused until the player presses it again. A note says so while plugins are stopped.
- An action is taken only as the map describes it: one function the game's own interface calls, its conditions, and its rate.

A domain is shown only when the running game map describes it, in a class the host presents: `you` as **Only for you**, `display` as **Changes what you see**, `others` as **Sees other players**. A domain of another class, or one the map does not describe, is listed as not allowed and is never granted; when the map later describes it, the plugin asks again.

## Classes

The player sees permissions in plain words, grouped by class, the most serious first: **Acts in the game for you**, **Uses the internet**, **Sees other players**, **Changes what you see**, **Only for you**. Permission names stay with authors and are not shown. Every new permission belongs to one class.

Permissions follow game domains and are as narrow as the domain allows: reading a domain of the game map, receiving its events, and each kind of operation on it are separate permissions. Automation is the use of operations, so each kind of action a plugin may take for the player is its own permission, shown to the player in plain words.

A permission exists only together with the API operation it guards. A capability the API does not offer has no permission; it is added with the operation that needs it, as a narrow permission describing that operation.

## Grants

Requests reach the network the way the player's browser does, since players behind a proxy or a VPN, or with a host mapped in the `hosts` file, need plugins to work there as well. The guarantees rest on what stays: only the approved hosts, only HTTPS on port 443, and a certificate the system trusts for the requested host. The home network (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, link-local and shared ranges) stays out of reach of a direct connection; this computer and the VPN range `198.18.0.0/15` do not, because a local proxy or a VPN uses them. A plugin whose listed host resolves to this computer connects to port 443 there and learns whether something listens on it, but no request reaches a service that holds no certificate for that host. A proxy sees the hosts a request goes to, as it does for the browser; a proxy that inspects TLS is one the player trusted with its certificate. Angelica's own requests to fixed hosts (updates, the map, the catalog) reach whatever the system resolves, because every file they bring carries a release signature: a redirected address can only make a download fail.

The host records the player's approval in `grants/<plugin-id>.json` ([installation](../formats/host-installation-v1.md#player-grants)). A plugin runs only with a grant that covers every permission and grantable domain its manifest declares; an operation checks both the declaration and the grant. A package without a grant appears in the Angelica menu with its permissions and an approval button, and runs no code. When the player approved an earlier version, the menu shows only what the new version adds: permissions and domains the grant does not cover, and for `http.get` only the new hosts. Approving replaces the grant with everything the new version declares.

## Data no permission exposes

Session tokens, login data, and other data that could take over the account are not available through any operation.
