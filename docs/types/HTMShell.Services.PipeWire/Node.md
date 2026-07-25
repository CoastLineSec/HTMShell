# `Node`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** `pipewire.nodes` item

A node item represents one PipeWire node for the current connection generation.

## Text bindings

| Key | Value |
| --- | --- |
| `item.name` | PipeWire node name |
| `item.nickname` | Short node label |
| `item.description` | Descriptive label |
| `item.media_class` | Raw media class |
| `item.node_type` | Canonical [`NodeType`](NodeType.md) text |
| `item.node_state` | Canonical [`NodeState`](NodeState.md) text |
| `item.direction` | `Sink`, `Source`, `Bidirectional`, `No direction`, or `Unknown` |

Missing text uses the standard unavailable marker.

## Numeric binding

`item.raw_id` is the current PipeWire global ID. It is a session-local diagnostic value. It is not stable across reconnects and is not a DOM identity or action target.

## Token bindings

| Key | Tokens |
| --- | --- |
| `item.ready` | `true`, `false` |
| `item.is_audio` | `true`, `false` |
| `item.is_video` | `true`, `false` |
| `item.is_stream` | `true`, `false` |
| `item.is_sink` | `true`, `false` |
| `item.is_source` | `true`, `false` |
| `item.direction` | `sink`, `source`, `bidirectional`, `absent`, `unknown` |
| `item.default_role` | `none`, `default-sink`, `default-source`, `default-sink-and-source` |
| `item.configured_role` | `none`, `configured-sink`, `configured-source`, `configured-sink-and-source` |
| `item.can_set_preferred_sink` | `true`, `false` |
| `item.can_set_preferred_source` | `true`, `false` |

`item.node_type` and `item.node_state` also support token presentation.

Audio nodes add `item.audio_status`, `item.volume`, `item.mute_state`,
`item.can_set_volume`, `item.can_set_mute`, `item.channel_count`, and
`item.channel_status`. See [`AudioNode`](AudioNode.md).

Node connection tracking adds `item.link_group_count` and
`item.link_group_status`, plus contextual `item.link_groups`. See
[`NodeLinks`](NodeLinks.md).

The preferred-default capability keys also support text and Boolean enable
binding. They are independent from `item.is_sink` and `item.is_source` because
metadata can be readable but not writable. See
[`DefaultControls`](DefaultControls.md).

`item.can_monitor_peaks` is a typed Boolean-equivalent capability. It is true
only for a current monitorable node while PipeWire is ready. It is independent
from audio read and write capability. An item-local `peak-monitor` uses the
current generation-safe node as its implicit target.

## Identity and ordering

Identity combines the connection generation and node global ID. Updates and keyed moves preserve the repeated subtree. Removal invalidates it.

Nodes are ordered by normalized type, media class, description, name, then raw ID. Ordering is deterministic within a snapshot but is not persistent across PipeWire sessions.

## See also

- [`Properties`](Properties.md)
- [`Defaults`](Defaults.md)
- [`AudioNode`](AudioNode.md)
- [`Channels`](Channels.md)
- [`NodeLinks`](NodeLinks.md)
- [`DefaultControls`](DefaultControls.md)
- [`PeakMonitor`](PeakMonitor.md)
