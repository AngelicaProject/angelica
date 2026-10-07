# Angelica

Angelica is a plugin platform for FINAL FANTASY XIV. Plugins run in a sandbox inside the game, reach the game only through a shared game map instead of hard-coded addresses, and keep working after a game patch as soon as the map is updated.

## Repositories

- `angelica` (this repository): the loader, the host that runs in the game, the plugin API and SDK, the installer, and the command-line tools.
- [`angelica-gamemap`](https://github.com/AngelicaProject/angelica-gamemap): the game map data, verified against each game build.
- [`angelica-registry`](https://github.com/AngelicaProject/angelica-registry): the plugin catalog.

## Development

Start with:

- [Contributing](./CONTRIBUTING.md)
- [Documentation index](./docs/README.md)
- [Product vision](./docs/product/vision.md)
- [Architecture overview](./docs/architecture/overview.md)

## License

Angelica is licensed under the [GNU Affero General Public License v3.0](./LICENSE). The plugin API definitions (`wit/`) and the SDK are licensed under the MIT License so that plugins can use any license; see [`docs/development/dependencies.md`](./docs/development/dependencies.md#licensing).
