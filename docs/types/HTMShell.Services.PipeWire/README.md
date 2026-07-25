# `HTMShell.Services.PipeWire`

Process-scoped PipeWire graph state and typed audio controls.

## Types

- [`PipeWire`](PipeWire.md): service state and node collection
- [`Node`](Node.md): repeated node fields and identity
- [`NodeType`](NodeType.md): node classification tokens
- [`NodeState`](NodeState.md): runtime node state tokens
- [`Defaults`](Defaults.md): actual and configured audio defaults
- [`DefaultControls`](DefaultControls.md): preferred output and input selection
- [`Properties`](Properties.md): bounded exact-key lookup
- [`AudioNode`](AudioNode.md): volume, mute, and control capability
- [`AudioControls`](AudioControls.md): typed item-local and actual-default controls
- [`Volume`](Volume.md): perceptual average and amplification bounds
- [`AudioChannel`](AudioChannel.md): ordered channel state and position tokens
- [`Channels`](Channels.md): contextual repetition, ordering, and fallback layouts
- [`ChannelControls`](ChannelControls.md): full-vector per-channel writes
- [`Link`](Link.md): individual PipeWire port links
- [`LinkGroup`](LinkGroup.md): source-target groups and member links
- [`LinkState`](LinkState.md): complete link state tokens
- [`Relations`](Relations.md): typed source, target, and peer projections
- [`NodeLinks`](NodeLinks.md): contextual node connection tracking

The module does not expose channel mute, channel-map writes, link mutation,
peak monitoring, stream movement, or spatial graph rendering.
