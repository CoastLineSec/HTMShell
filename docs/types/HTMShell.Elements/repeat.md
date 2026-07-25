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
| `data-htm-source` | A documented top-level or contextual source |
| Template content | Exactly one root element |
| Registered descendants | Source-specific read-only bindings; `pipewire.nodes` and `item.channels` also permit narrow audio controls |
| `data-htm-local-id` | Required and template-unique on registered descendants |

Normal `id` attributes and clocks are rejected in the template subtree. Item
bindings must match the selected source.

`upower.devices` and `power_profile.holds` are read-only. `pipewire.nodes` also accepts item-local `pipewire.audio.mute`, `pipewire.audio.unmute`, and `pipewire.audio.toggle_mute` action buttons plus `range-control` for `pipewire.audio.set_volume`. Other actions and controls are rejected.

## Sources

Top-level sources are:

- `upower.devices`
- `power_profile.holds`
- `pipewire.nodes`
- `pipewire.links`
- `pipewire.link_groups`

PipeWire links and link groups are read-only. They permit `state-text`,
`state-token`, `state-value`, and only the contextual graph source valid for
that parent.

## Contextual collections

Exactly one contextual repeat level is supported:

| Contextual source | Required top-level parent |
| --- | --- |
| `item.channels` | `pipewire.nodes` |
| `item.link_groups` | `pipewire.nodes` |
| `item.links` | `pipewire.link_groups` |
| `peak.channels` | nearest `peak-monitor` |

For example, `item.channels` is valid only inside the single root of a
`pipewire.nodes` template:

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

Channel clones allow ordinary HTML, `state-text`, `state-token`, `state-value`,
and channel `range-control`. Graph contextual clones allow only ordinary HTML,
`state-text`, `state-token`, and `state-value`. Action buttons, clocks, normal
`id`, another repeat, and explicit control targets are rejected.

The inner `item.*` shadows the outer item in all contextual collections. No
parent traversal exists. `item.channels` and `item.link_groups` may be sibling
repeats in one node template, but no contextual repeat may contain another.

`peak.channels` uses a monitor-local scope rather than an outer `item.*`
scope. Its inner `item.*` is one negotiated peak channel. It permits only
ordinary HTML, `state-text`, `state-token`, and `state-value`; actions,
controls, clocks, monitors, and another repeat are rejected.

Volume-only updates retain channel clone identity; a layout-generation change
replaces it. Link-state and relation-label updates retain graph clone identity.

Instances are inserted before the retained template marker. Source keys and document generations provide identity. A property update preserves the item subtree. Insertions, removals, and deterministic moves are incremental.

The current limits are 32 top-level repeats per document, 16,384 source items
per repeat, 64 registered descendants per template, depth 32, 4,096 cloned
nodes per repeat, and 16,384 cloned nodes per document. A document may contain at
most 16 `pipewire.nodes` repeats, 16 PipeWire audio controls per node item, and
8 range controls per node item.

Each outer node template permits 8 `item.channels` declarations. A document
permits 32 contextual repeats. A channel item permits 64 bindings and 8 range
controls, with 256 channel range controls per document and 64 public channels
per node.

A document may contain 16 `pipewire.links` repeats, 16
`pipewire.link_groups` repeats, and 32 contextual graph repeats. One group
template may contain 8 `item.links` declarations and one node template may
contain 8 `item.link_groups` declarations. A link or group item permits 64
graph bindings, including at most 64 relation bindings. Process projection is
bounded at `16384` links and `4096` link groups. Existing clone limits still
apply.

## See also

- [Device collection](../HTMShell.Services.UPower/DeviceCollection.md)
- [Power profile holds](../HTMShell.Services.UPower/PowerProfileHold.md)
- [PipeWire nodes](../HTMShell.Services.PipeWire/Node.md)
- [PipeWire audio controls](../HTMShell.Services.PipeWire/AudioControls.md)
- [PipeWire channels](../HTMShell.Services.PipeWire/Channels.md)
- [PipeWire links](../HTMShell.Services.PipeWire/Link.md)
- [PipeWire link groups](../HTMShell.Services.PipeWire/LinkGroup.md)
- [PipeWire node connections](../HTMShell.Services.PipeWire/NodeLinks.md)
- [PipeWire peak channels](../HTMShell.Services.PipeWire/PeakChannel.md)
