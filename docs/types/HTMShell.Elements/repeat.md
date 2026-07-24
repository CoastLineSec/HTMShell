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
| `data-htm-source` | `upower.devices`, `power_profile.holds`, or `pipewire.nodes` |
| Template content | Exactly one root element |
| Registered descendants | `state-text`, `state-token`, or `state-value` |
| `data-htm-local-id` | Required and template-unique on registered descendants |

Normal `id` attributes, actions, clocks, and nested repeats are rejected in the template subtree. Item bindings must match the selected source.

Instances are inserted before the retained template marker. Source keys and document generations provide identity. A property update preserves the item subtree. Insertions, removals, and deterministic moves are incremental.

The current limits are 32 repeats per document, 4,096 items per repeat, 64 registered descendants per template, depth 32, 4,096 cloned nodes per repeat, and 16,384 cloned nodes per document. A document may contain at most 16 `pipewire.nodes` repeats.

## See also

- [Device collection](../HTMShell.Services.UPower/DeviceCollection.md)
- [Power profile holds](../HTMShell.Services.UPower/PowerProfileHold.md)
- [PipeWire nodes](../HTMShell.Services.PipeWire/Node.md)
