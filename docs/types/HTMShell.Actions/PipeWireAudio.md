# `PipeWireAudio`

**Module:** `HTMShell.Actions` | **Scope:** Process

Typed PipeWire audio actions control one current-generation audio node.

## Mute actions

| Action | Effect |
| --- | --- |
| `pipewire.audio.mute` | Request muted state |
| `pipewire.audio.unmute` | Request unmuted state |
| `pipewire.audio.toggle_mute` | Invert the latest authoritative state |

Inside `pipewire.nodes`, an action button targets its current item and requires `data-htm-enabled-bind="item.can_set_mute"`.

Outside a repeat, `data-htm-target` is required and must be `pipewire.default_sink` or `pipewire.default_source`. The enabled binding must be the target's matching `can_set_mute` key.

## Volume action

`pipewire.audio.set_volume` is accepted only by [`range-control`](../HTMShell.Elements/range-control.md). It cannot be dispatched by `action-button`.

`pipewire.audio.set_channel_volume` is accepted only by `range-control` inside
`pipewire.nodes` then `item.channels`. The current channel and parent node are
implicit. An explicit target, raw index, or action button is rejected.

## Confirmation

PipeWire state is authoritative. Controls use `idle`, `pending`, `failed`, and `unavailable` in runtime-owned `data-htm-state`.

Requests are generation-safe, coalesced per node, and confirmed by reported
PipeWire parameters. Average and channel writes share one full-vector
coordinator. Permission denial, timeout, target or layout removal, or reconnect
cannot update a stale control.

Raw PipeWire IDs, node names, DOM IDs, selectors, configured defaults, and non-PipeWire repeat items are invalid targets.
