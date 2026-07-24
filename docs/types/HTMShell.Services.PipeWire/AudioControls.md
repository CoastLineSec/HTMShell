# `AudioControls`

**Module:** `HTMShell.Services.PipeWire`

Audio controls use typed actions and generation-safe node targets.

## Actions

- `pipewire.audio.mute`
- `pipewire.audio.unmute`
- `pipewire.audio.toggle_mute`
- `pipewire.audio.set_volume`

The mute actions use [`action-button`](../HTMShell.Elements/action-button.md). Set-volume uses only [`range-control`](../HTMShell.Elements/range-control.md).

Inside `pipewire.nodes`, controls target the current keyed item. Outside a repeat, only `pipewire.default_sink` and `pipewire.default_source` are writable targets.

Configured defaults, raw IDs, names, selectors, arbitrary DOM IDs, and dynamic target values are rejected.

## State

Each exact control owns runtime `data-htm-state`:

| Token | Meaning |
| --- | --- |
| `idle` | Controllable with no request pending |
| `pending` | Waiting for authoritative confirmation |
| `failed` | The latest request failed or timed out |
| `unavailable` | Missing, stale, unsupported, or not writable |

The identity includes the document generation, control element, target generation, target node, and operation. Several controls may share one node write coordinator without sharing DOM identity.

## Request behavior

Mute, unmute, and toggle are bounded to one active mute operation per node. Volume motion retains only the latest desired value and permits one active volume operation per node. Duplicate desired values are suppressed. Queued volume writes are spaced by at least 16 milliseconds.

The confirmation timeout is two seconds. Denial, timeout, removal, reconnect, and stale replies leave public volume and mute state unchanged.

## Limits

- 128 PipeWire audio controls per document
- 16 PipeWire audio controls per repeated item
- 4,096 node coordinators, bounded by the graph limit
- one pending mute intent and one pending volume intent per node

## See also

- [PipeWire audio actions](../HTMShell.Actions/PipeWireAudio.md)
- [`AudioNode`](AudioNode.md)
- [`Volume`](Volume.md)
