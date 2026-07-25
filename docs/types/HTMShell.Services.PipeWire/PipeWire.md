# `PipeWire`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** Process

`PipeWire` exposes connection state, nodes, the read-only link graph, default
audio state, ordered channels, typed volume and mute controls, and configured
default selection.

## State

| Key | Presentation | Values |
| --- | --- | --- |
| `pipewire.availability` | text, token | `unavailable`, `synchronizing`, `ready` |
| `pipewire.ready` | text, token, Boolean enable binding | `true`, `false` |
| `pipewire.node_count` | numeric | Nonnegative integer |
| `pipewire.link_count` | numeric | Public link collection length |
| `pipewire.link_group_count` | numeric | Public source-target group count |

`pipewire.ready` becomes true only after the initial registry synchronization
barrier. Graph counts are zero before that point.

## Collection

`pipewire.nodes`, `pipewire.links`, and `pipewire.link_groups` are keyed
[`repeat`](../HTMShell.Elements/repeat.md) sources. One process graph serves
all live documents and output instances. The first consumer activates
PipeWire. Removing the last consumer releases its connection and reconnect
deadline.

Graph updates are event-driven. Insertions, removals, state changes, relation
changes, and keyed moves mutate only affected document instances.

## Audio demand

Reading volume or mute state activates audio parameter tracking. Declaring a mute action or [`range-control`](../HTMShell.Elements/range-control.md) also activates write demand.

Subscriptions and node write coordinators are shared process-wide. Removing the
final audio consumer releases audio tracking. `item.channels` adds
document-driven channel projection and channel-write demand without creating
link or peak demand.

Link collection, link detail, group collection, group-member, node-tracker,
and relation demand are tracked separately. Declaring graph bindings does not
create PipeWire write or peak-monitor demand.

Preferred sink and source actions add independent configured-default write
demand. All documents share the existing default metadata proxy and one
bounded coordinator per role. Removing the final role consumer releases that
role's pending presentation state while independent read demand can remain.

Peak monitor declarations add a separate explicit demand class. Only enabled
declarations on mapped surfaces activate it. They share one stream per target
node and do not create audio-write, default-write, or link-write demand.

## Lifecycle

PipeWire absence is valid. On disconnect, the current graph and defaults are
cleared. Reconnection creates a fresh collection generation, so reused
session-local node or link IDs cannot alias old items.

## See also

- [`Node`](Node.md)
- [`Defaults`](Defaults.md)
- [`DefaultControls`](DefaultControls.md)
- [`AudioNode`](AudioNode.md)
- [`AudioControls`](AudioControls.md)
- [`AudioChannel`](AudioChannel.md)
- [`Channels`](Channels.md)
- [`ChannelControls`](ChannelControls.md)
- [`Link`](Link.md)
- [`LinkGroup`](LinkGroup.md)
- [`Relations`](Relations.md)
- [`NodeLinks`](NodeLinks.md)
- [`PeakMonitoring`](PeakMonitoring.md)
- [`PeakMonitor`](PeakMonitor.md)
- [`PeakChannel`](PeakChannel.md)
- [PipeWire audio and routing guide](../../guide/audio.md)
