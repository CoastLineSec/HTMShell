# `AudioNode`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** `pipewire.nodes` item

Audio bindings are available for sinks, sources, application streams, and other audio-capable nodes.

## Bindings

| Key | Presentation | Values |
| --- | --- | --- |
| `item.audio_status` | text, token | `unsupported`, `unavailable`, `ready` |
| `item.volume` | numeric | Nonnegative perceptual average |
| `item.mute_state` | text, token | `unavailable`, `muted`, `unmuted` |
| `item.can_set_volume` | text, token, Boolean enable binding | `true`, `false` |
| `item.can_set_mute` | text, token, Boolean enable binding | `true`, `false` |

`unsupported` means the node is not audio capable. `unavailable` means authoritative audio parameters are incomplete. Missing volume is unavailable, not zero.

Read state and write capability are separate. A restricted client can read a node while both control capabilities are false.

## Lifetime

Audio state belongs to the node's connection generation. Node removal or reconnect clears stale state and invalidates pending controls.

Parameter subscriptions are activated by document demand and shared across all outputs. Public channel vectors are not exposed.

## See also

- [`AudioControls`](AudioControls.md)
- [`Volume`](Volume.md)
- [`Node`](Node.md)
