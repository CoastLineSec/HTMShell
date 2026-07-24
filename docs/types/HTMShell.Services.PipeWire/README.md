# `HTMShell.Services.PipeWire`

Process-scoped read-only PipeWire state.

## Types

- [`PipeWire`](PipeWire.md): service state and node collection
- [`Node`](Node.md): repeated node fields and identity
- [`NodeType`](NodeType.md): node classification tokens
- [`NodeState`](NodeState.md): runtime node state tokens
- [`Defaults`](Defaults.md): actual and configured audio defaults
- [`Properties`](Properties.md): bounded exact-key lookup

The module does not expose volume, mute, channels, links, peaks, or writable controls.
