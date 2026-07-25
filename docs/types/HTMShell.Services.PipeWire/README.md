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
- [`AudioChannel`](AudioChannel.md): ordered channel state and position tokens
- [`Channels`](Channels.md): contextual repetition, ordering, and fallback layouts
- [`ChannelControls`](ChannelControls.md): full-vector per-channel writes

The module does not expose channel mute, channel-map writes, links, peak
monitoring, preferred-default writes, or stream movement.
