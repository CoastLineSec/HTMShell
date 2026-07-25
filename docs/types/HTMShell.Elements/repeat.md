# `repeat`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element

`repeat` expands one keyed service collection from an inert HTML template.

## Usage

```html
<template id="device-row"
          data-htm-element="repeat"
          data-htm-source="upower.devices">
  <div class="device">
    <span data-htm-element="state-text"
          data-htm-local-id="model"
          data-htm-bind="item.model"></span>
  </div>
</template>
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tag | `template` |
| `id` | Required and document-unique |
| `data-htm-source` | `upower.devices`, `power_profile.holds`, `pipewire.nodes`, or contextual `item.channels` |
| Template content | Exactly one root element |
| Registered descendants | `state-text`, `state-token`, or `state-value`; `pipewire.nodes` also permits narrow audio controls |
| `data-htm-local-id` | Required and template-unique on registered descendants |

Normal `id` attributes and clocks are rejected in the template subtree. Item
bindings must match the selected source.

`upower.devices` and `power_profile.holds` are read-only. `pipewire.nodes` also accepts item-local `pipewire.audio.mute`, `pipewire.audio.unmute`, and `pipewire.audio.toggle_mute` action buttons plus `range-control` for `pipewire.audio.set_volume`. Other actions and controls are rejected.

## Contextual channels

Exactly one contextual repeat level is supported. `item.channels` is valid only
inside the single root of a `pipewire.nodes` template:

```html
<template data-htm-element="repeat"
          data-htm-source="item.channels">
  <div data-htm-local-id="channel">
    <span data-htm-element="state-text"
          data-htm-local-id="name"
          data-htm-bind="item.position_name"></span>
  </div>
</template>
```

The inner `item.*` scope is the channel and shadows the node item. The parent
node is implicit only for `pipewire.audio.set_channel_volume`; no parent
traversal syntax exists.

Contextual clones allow ordinary HTML, `state-text`, `state-token`,
`state-value`, and channel `range-control`. Action buttons, clocks, normal
`id`, another repeat, and explicit control targets are rejected. Volume-only
updates retain channel clone identity; a layout-generation change replaces it.

Instances are inserted before the retained template marker. Source keys and document generations provide identity. A property update preserves the item subtree. Insertions, removals, and deterministic moves are incremental.

The current limits are 32 top-level repeats per document, 4,096 items per
repeat, 64 registered descendants per template, depth 32, 4,096 cloned nodes
per repeat, and 16,384 cloned nodes per document. A document may contain at
most 16 `pipewire.nodes` repeats, 16 PipeWire audio controls per node item, and
8 range controls per node item.

Each outer node template permits 8 `item.channels` declarations. A document
permits 32 contextual repeats. A channel item permits 64 bindings and 8 range
controls, with 256 channel range controls per document and 64 public channels
per node.

## See also

- [Device collection](../HTMShell.Services.UPower/DeviceCollection.md)
- [Power profile holds](../HTMShell.Services.UPower/PowerProfileHold.md)
- [PipeWire nodes](../HTMShell.Services.PipeWire/Node.md)
- [PipeWire audio controls](../HTMShell.Services.PipeWire/AudioControls.md)
- [PipeWire channels](../HTMShell.Services.PipeWire/Channels.md)
