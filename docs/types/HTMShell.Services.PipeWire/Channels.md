# `Channels`

**Module:** `HTMShell.Services.PipeWire`

`item.channels` is the ordered contextual collection of the current
`pipewire.nodes` item.

## Syntax

```html
<template id="audio-nodes"
          data-htm-element="repeat"
          data-htm-source="pipewire.nodes">
  <article>
    <template data-htm-element="repeat"
              data-htm-source="item.channels">
      <div data-htm-local-id="channel">
        <span data-htm-element="state-text"
              data-htm-local-id="name"
              data-htm-bind="item.position_name"></span>
      </div>
    </template>
  </article>
</template>
```

The inner `item.*` scope is the channel and shadows the outer node item. The
parent node remains an implicit runtime target. Parent traversal, selectors,
interpolation, and a third repeat level are invalid.

The contextual clone permits ordinary HTML, `state-text`, `state-token`,
`state-value`, and a channel [`range-control`](../HTMShell.Elements/range-control.md).
Normal `id`, action buttons, clocks, and nested repeats are rejected.

`item.channels` and node `item.link_groups` may be siblings in one node
template. Neither contextual collection may contain the other.

## Ordering and fallback

Collection order is PipeWire's volume-vector order. It is not alphabetic or
sorted by position code.

When the channel map is absent, vector lengths one through eight use these
layouts:

| Count | Positions |
| --- | --- |
| 1 | mono |
| 2 | front left, front right |
| 3 | front left, front right, low frequency effects |
| 4 | front left, front right, rear left, rear right |
| 5 | front left, front right, front center, side left, side right |
| 6 | front left, front right, front center, low frequency effects, side left, side right |
| 7 | front left, front right, front center, rear left, rear right, side left, side right |
| 8 | front left, front right, front center, low frequency effects, rear left, rear right, side left, side right |

Larger vectors use `unknown`. A short map is extended with `unknown`; a long
map is ignored past the volume count. No volume entry is discarded.

## Node state and limits

`item.channel_status` on the outer node is `unsupported`, `unavailable`, or
`ready`. `item.channel_count` equals the contextual collection length when
ready and is zero otherwise.

Limits are 8 `item.channels` repeats per outer node template, 32 contextual
repeats per document, 64 public channels per node, 64 registered bindings per
channel item, 8 channel range controls per channel item, 256 channel range
controls per document, and exactly one contextual repeat level. Existing repeat
depth and clone limits still apply.

## See also

- [`AudioChannel`](AudioChannel.md)
- [`repeat`](../HTMShell.Elements/repeat.md)
- [`AudioNode`](AudioNode.md)
