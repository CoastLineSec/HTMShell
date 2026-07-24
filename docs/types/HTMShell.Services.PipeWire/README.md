# `HTMShell.Services.PipeWire`

Process-scoped PipeWire node state and typed audio controls.

## Types

- [`PipeWire`](PipeWire.md): service state and node collection
- [`Node`](Node.md): repeated node fields and identity
- [`NodeType`](NodeType.md): node classification tokens
- [`NodeState`](NodeState.md): runtime node state tokens
- [`Defaults`](Defaults.md): actual and configured audio defaults
- [`Properties`](Properties.md): bounded exact-key lookup
- [`AudioNode`](AudioNode.md): volume, mute, and control capability
- [`AudioControls`](AudioControls.md): typed item-local and actual-default controls
- [`Volume`](Volume.md): perceptual average and amplification bounds

The module does not expose public channels, links, peak monitoring, preferred-default writes, or stream movement.
