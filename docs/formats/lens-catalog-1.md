# Lens catalog format 1

The Lens catalog says what Lens offers over an [Anatomia](./anatomia-1.md) description: the structures and fields a Lens path goes through, the roots where paths start, the events Lens watches and decides at, the actions it takes for the player, and the fields plugins may change through it. It lives in `lens/` of the description's directory and is not part of the description: changing it changes no entry and no build record.

```text
lens/types.toml      the structures Lens offers, and their fields
lens/roots.toml      where paths start
lens/events.toml     functions and locations Lens watches, their arguments, and outcomes
lens/actions.toml    functions Lens calls for the player, with conditions
lens/changes.toml    fields plugins may change through Lens
```

Every name in the catalog is a name of the description: a structure, a field by its name or an alias, a function, a location, or a global. A catalog item is usable in a build when everything of the description it names exists, is of the right kind, and is present there.

## Types

```toml
[types."inventory.inventory_container"]
fields = ["items", "kind", "size"]
```

Each structure Lens offers, with the fields a path may go through. A field the list does not name is part of the description but not of Lens. The structure's fields read as the description types them, in the domains its [meaning](./anatomia-1.md#meaning) gives.

## Roots, events, and actions

`roots.toml`, `events.toml`, and `actions.toml` hold `[roots.<name>]`, `[events.<name>]`, and `[actions.<name>]`, with the keys and meanings of the game map's [roots](./game-map-v2.md#roots), [events](./game-map-v2.md#events), and [actions](./game-map-v2.md#actions). A root's `global`, an event's `at`, an action's `call`, and an action argument's `global` are entries of the description; a root's `type` and a reference's `ref` are structures it has.

## Changes

```toml
[changes."camera.camera.min_distance"]
range = [0.5, 20.0]

[changes."ui.res_node.visible"]
call = "ui.res_node.set_visible"
hold = true
```

A field plugins may change through Lens, by its name, with what a field's [`set`](./game-map-v2.md#types) gives in the game map: `range`, `call`, and `hold`. An empty table allows the change without them.
