# API requests

When a plugin needs something of the game the [plugin API](../formats/plugin-api-v1.md) does not offer, its author files an API request instead of working around the gap ([growing the API](../architecture/services.md#growing-the-api)). A platform maintainer designs the capability for the game domain, for every plugin that needs it, and adds it with its permission, map entries, and tests.

A request describes the game, not an implementation. It never contains addresses, offsets, signatures, or code from the game's executable, and never asks for raw memory access, game function calls, or hooks.

## What a request contains

```markdown
# <domain>: <capability in a few words>

## Need
What the plugin does for the player, and the step it cannot take with the
current API. One paragraph.

## Game resource
What in the game is involved, as the player sees it: the inventory's items,
the party list, the target, the map's flag, a game window.

## Operation
- Kind: read objects | event | operation on the game | game data | rule
- Inputs and results: the values the plugin passes and receives, with
  their meaning (for example "the item's row in the Item sheet", "the
  quantity").
- When: how often the plugin needs it (once, every frame, on change) and
  from which instance (main or job).

## Player's view
What the player should be told when approving it, in plain words, and
whether a narrower variant would serve.

## Workaround meanwhile
What the plugin does until the capability exists, or "none".
```

## Example

```markdown
# Inventory: items the player carries

## Need
A crafting helper shows how many of each ingredient the player still needs.
It cannot see which items the player holds.

## Game resource
The player's inventory bags and the crystal bag: each slot's item and
quantity, and whether the item is high quality.

## Operation
- Kind: read objects
- Inputs and results: one row per used slot with the bag, the slot, the
  item (a row of the Item sheet), the quantity, and high quality.
- When: when the helper's window is open, at most once a second, from main.

## Player's view
"Reads the items you carry." Reading the armoury chest or retainers is not
needed.

## Workaround meanwhile
The player types the quantities into the window.
```

## Filing a request

A request is an issue in the [angelica repository](https://github.com/AngelicaProject/angelica/issues) made with the **API request** form, or from the command line with the same sections as the body:

```text
gh issue create --repo AngelicaProject/angelica --label api-request,status:new --title "<domain>: <capability>" --body-file request.md
```

Search the open `api-request` issues first: when one describes the same resource, add the plugin and anything the existing request lacks as a comment instead of filing another.

## What happens next

Labels track a request:

| Label | Meaning |
| --- | --- |
| `status:new` | filed, not triaged |
| `domain:<name>` | the game domain it belongs to |
| `status:needs-decision` | grouped with the domain's other requests into one proposal: the interface, its permissions and their wording for the player, and what stays out; the project's maintainer decides |
| `status:accepted` | accepted; implemented in its domain |
| `status:declined` | not offered to plugins, with the reason |

A proposal designs the domain for every plugin that needs it, so one interface serves the requests together. When the capability is released, the issue is closed with the API version that has it, the API contract and the [interface cards](./interfaces.md) describe it, and the plugin raises `plugin.api` in its manifest to that version. A capability whose game structures are not verified stays unavailable rather than being exposed partially.
