# `PipeWire`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** Process

`PipeWire` exposes connection state and the current node collection.

## State

| Key | Presentation | Values |
| --- | --- | --- |
| `pipewire.availability` | text, token | `unavailable`, `synchronizing`, `ready` |
| `pipewire.ready` | text, token, Boolean enable binding | `true`, `false` |
| `pipewire.node_count` | numeric | Nonnegative integer |

`pipewire.ready` becomes true only after the initial registry synchronization barrier. The node count is zero before that point.

## Collection

`pipewire.nodes` is a keyed [`repeat`](../HTMShell.Elements/repeat.md) source. One source serves all live documents and output instances. The first consumer activates PipeWire. Removing the last consumer releases its connection and reconnect deadline.

Node updates are event-driven. Insertions, removals, property changes, and ordering changes mutate only affected document instances.

## Lifecycle

PipeWire absence is valid. On disconnect, current nodes and defaults are cleared. Reconnection creates a fresh collection generation, so a reused session-local node ID cannot alias an old item.

## See also

- [`Node`](Node.md)
- [`Defaults`](Defaults.md)
- [PipeWire nodes guide](../../guide/audio.md)
