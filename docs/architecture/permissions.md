# Permissions

A plugin's manifest declares its permissions ([manifest](../formats/plugin-manifest-v1.md)). The player sees them before approving the plugin, and approval grants exactly those. A new version that declares a permission the grant does not cover waits for approval again.

Every plugin, once approved, has its own windows, log, storage folder, and jobs; these need no permission. Everything that reaches the game needs one:

| Permission | Allows | Shown to the player as |
| --- | --- | --- |
| `excel.text` | activating [text tables](../formats/plugin-api-v1.md#text-tables) that replace text of the game's data sheets | Replaces text the game shows from its data sheets |
| `text.case` | adding letters to the game's [letter case conversion](../formats/plugin-api-v1.md#letter-case) | Changes how the game capitalizes letters |
| `files.replace` | serving [replacement game files](../formats/plugin-api-v1.md#replacement-files) of formats the host checks | Replaces game files: fonts and textures |

A permission exists only together with the API operation it guards. A capability the API does not offer has no permission; it is added with the operation that needs it, as a narrow permission describing that operation.

## Grants

The host records the player's approval in `grants/<plugin-id>.json` ([installation](../formats/host-installation-v1.md#player-grants)). A plugin runs only with a grant that covers every permission its manifest declares; an operation checks both the declaration and the grant. A package without a grant appears in the Angelica menu with its permissions and an approval button, and runs no code.

## Data no permission exposes

Session tokens, login data, and other data that could take over the account are not available through any operation.
