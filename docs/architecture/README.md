# Architecture documentation

Start with [`overview.md`](./overview.md). It defines the components, the repositories, and crate ownership. Then read the document for the area being changed.

| Area | Canonical document | Primary implementation area |
| --- | --- | --- |
| Components and ownership | [`overview.md`](./overview.md) | project-wide |
| Getting into the game process | [`loading.md`](./loading.md) | `angelica-proxy`, `angelica-host` |
| Hooks, trampolines, and code patches | [`hooks.md`](./hooks.md) | `angelica-hook` |
| Resolving and verifying map entries | [`game-map.md`](./game-map.md) | `angelica-pe`, `angelica-map` |
| Plugin runtime and API | [`plugins.md`](./plugins.md) | `angelica-host`, `wit/`, `angelica-sdk` |
| Permissions | [`permissions.md`](./permissions.md) | `angelica-host` |
| Host services | [`services.md`](./services.md) | `angelica-host` |
| In-game interface | [`ui.md`](./ui.md) | `angelica-host` |
| Installing Angelica | [`installer.md`](./installer.md) | `angelica-setup` |

Architecture documents own boundaries and invariants, not coding style. Implementation conventions belong under [`../development/`](../development/README.md), and versioned contracts under [`../formats/`](../formats/README.md).
