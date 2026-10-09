# Host services

A host service is an operation on the game or the player's system that the host implements natively and offers to plugins through the API. Plugins compose services into their own applications; the host never knows what a plugin is for.

## API design

The API is defined in `wit/` and specified in [the API contract](../formats/plugin-api-v1.md). The SDK adds bindings and helpers for that contract; it does not add capabilities.

An operation names a resource and defines its inputs, results, validation, ownership, thread, limits, and failure. It must serve any plugin that needs the resource, without the host learning a plugin's name, file formats, or policy. A first consumer may motivate an operation; it must still be described in terms of the resource. For example, text tables replace cells of the game's sheets by sheet, row, column, and guard: a translation plugin uses them for its packs, another plugin could use them for annotations or accessibility. Reading the translation pack, trusting its publisher, and choosing which sheets to translate stay in the translation plugin.

An operation is as narrow as its verified contract. A game structure the map does not describe, or a change whose effects on the game are not known, stays unavailable rather than being exposed through a general escape hatch such as raw memory access or arbitrary function calls.

## Kinds of services

| Kind | Example | Rule |
| --- | --- | --- |
| Declarative game rules | text tables, letter case, replacement files | the plugin activates rules; the host applies them natively on the game's hot path, validates them on activation, and removes them when the plugin stops |
| Player interaction | chat commands, chat messages | the plugin declares its commands and supplies its messages; the host parses, answers help and mistakes, and presents everything under the plugin's name in the game's own look |
| Game data | game files, Excel sheets | read-only originals from the installed archives, never another plugin's changes; bounded and cached by the host |
| Host resources | storage, jobs, windows | bounded per plugin, confined to the plugin |

Each service that changes the game uses an adapter over verified map entries with a written contract ([game structures](../formats/game-structures.md)). An adapter installs its hooks the first time a plugin needs it and keeps them until the process exits; without active rules they only pass calls on.

## Adding a service

A plugin combines existing operations whenever they express its task; convenience code belongs in the plugin or in SDK helpers. A new service is added for a concrete need that existing operations cannot meet safely or fast enough, together with:

- the WIT interface and its section in the API contract;
- a permission when it reaches the game or the player's system ([permissions](./permissions.md));
- the adapter's contract and map entries when it touches game structures;
- tests through `angelica-fake-game`.

## Validation of external data

Data a plugin brings from outside — a downloaded pack, a file the player chose — is parsed inside the plugin's sandbox, typically in a job, so a malformed file can fault at most that plugin. What reaches a service is in the service's own format and is validated by the host again before it affects the game.
