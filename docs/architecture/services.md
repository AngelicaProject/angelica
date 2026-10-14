# Host services

A host service is an operation on the game or the player's system that the host implements natively and offers to plugins through the API. Plugins compose services into their own applications; the host never knows what a plugin is for.

## API design

The API is defined in `wit/` and specified in [the API contract](../formats/plugin-api-v1.md). The SDK adds bindings and helpers for that contract; it does not add capabilities.

An operation names a resource and defines its inputs, results, validation, ownership, thread, limits, and failure. It must serve any plugin that needs the resource, without the host learning a plugin's name, file formats, or policy. A first consumer may motivate an operation; it must still be described in terms of the resource. For example, text tables replace cells of the game's sheets by sheet, row, column, and guard: a translation plugin uses them for its packs, another plugin could use them for annotations or accessibility. Reading the translation pack, trusting its publisher, and choosing which sheets to translate stay in the translation plugin.

An operation is as narrow as its verified contract. A game structure the map does not describe, or a change whose effects on the game are not known, stays unavailable rather than being exposed through a general escape hatch such as raw memory access or arbitrary function calls. Lens grows by data: a new type or field in the map's object model is readable, a new event watchable, and a new action takeable without new host code; an action calls only the function the map names, with arguments it checks.

## Kinds of services

| Kind | Example | Rule |
| --- | --- | --- |
| Declarative game rules | text tables, letter case, replacement files, hidden elements and text in the game's own windows | the plugin activates rules; the host applies them natively on the game's hot path, validates them on activation, and removes them when the plugin stops |
| Game objects | the player, the inventory | the plugin reads the game's objects through [Lens](./lens.md), by paths through the game map's object model, under a read permission for each domain it reads; reads happen on the main thread between frames and never write |
| Messages between plugins | asking another plugin for its state | the plugin sends bytes to a plugin its manifest lists; the host delivers them in the next frame as a game event, and answers go back the same way; no plugin runs another's code |
| Drawing over the world | a circle on the ground, a label above a spot | the plugin sets shapes at places in the world under `draw.world`; the host projects them with the game camera's matrix from the map every frame and draws them under the windows |
| Game decisions | hiding a chat message | the plugin adds a rule at a decision point of the game map through [Lens](./lens.md) under a change permission for its domain; the host applies it in the hook itself and the function returns the outcome's value without running |
| Game actions | using an action, teleporting | the plugin takes an action of the game map through [Lens](./lens.md) under an act permission for its domain; the host checks the parameters, the rate, the conditions, and the player's stop, then calls the game's function on the main thread |
| Game events | a chat message arrived, a window opened, the zone changed | an adapter's hook records the event; the plugin receives it in the next frame and cannot change what the game did |
| Operations on the game | using an action, moving an item | one host operation per kind of action, with its own permission, validated arguments, and a rate the host enforces; automation is built from these operations |
| Player interaction | chat commands, chat messages | the plugin declares its commands and supplies its messages; the host parses, answers help and mistakes, and presents everything under the plugin's name in the game's own look |
| Game data | game files, Excel sheets | read-only originals from the installed archives, never another plugin's changes; bounded and cached by the host; sheet columns are addressed by the names the game map's sheet schemas give them |
| Host resources | storage, jobs, windows | bounded per plugin, confined to the plugin |
| The internet | HTTPS requests | only to the public hosts the manifest names and the player approved, from jobs, with the host's TLS and limits |

Each service that changes the game uses an adapter over verified map entries with a written contract ([game structures](../formats/game-structures.md)). An adapter installs its hooks the first time a plugin needs it and keeps them until the process exits; without active rules they only pass calls on.

## Growing the API

The API grows by game domain (the inventory, the party, the chat, the game's own windows), not by plugin. A domain's interface is designed once for every plugin that needs the domain, so a patch that changes the domain is absorbed in one adapter and one set of map entries.

A plugin author whose plugin needs a capability the API lacks does not work around it: there is no raw memory access, no game function call, and no hook to fall back on. The author files an [API request](../plugins/api-requests.md) that names the game resource, the operation, and the plugins that need it. A platform maintainer adds the capability to its domain as described below. Requests that share a domain are designed together.

Published research on the game's structures may serve as a starting point for map entries, under the game map repository's import rules. Angelica never depends on it: nothing is fetched from it at build or run time, and every entry is verified against the game's executable before it is used.

## Adding a service

A plugin combines existing operations whenever they express its task; convenience code belongs in the plugin or in SDK helpers. A new service is added for a concrete need that existing operations cannot meet safely or fast enough, together with:

- the WIT interface and its section in the API contract;
- a permission when it reaches the game or the player's system ([permissions](./permissions.md));
- the adapter's contract and map entries when it touches game structures;
- tests through `angelica-fake-game`.

## Validation of external data

Data a plugin brings from outside — a downloaded pack, a file the player chose — is parsed inside the plugin's sandbox, typically in a job, so a malformed file can fault at most that plugin. What reaches a service is in the service's own format and is validated by the host again before it affects the game.
