# `LinkGroup`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** `pipewire.link_groups` item or node `item.link_groups` item

A group contains all current links with one source node and one target node.

## Bindings

| Key | Presentation | Values |
| --- | --- | --- |
| `item.member_count` | numeric | Current member count |
| `item.representative_link_raw_id` | numeric | Session-local representative link ID |
| `item.ready` | text, token | `unavailable`, `partial`, `ready` |
| `item.state` | text, token | [`LinkState`](LinkState.md) |
| `item.is_monitor` | text, token | `true`, `false` |

Source and target projections use the fields in
[`Relations`](Relations.md).

## Representative state

Group state is the state of one retained member, not an aggregate. The first
representative remains while it is a member. Removing another member does not
change it. When the representative disappears, the lowest remaining
session-local link ID becomes the next representative. This choice does not
affect group identity.

## Members

Inside `pipewire.link_groups`, repeat over the current member links with
`item.links`:

```html
<template data-htm-element="repeat"
          data-htm-source="item.links">
  <div>
    <span data-htm-element="state-token"
          data-htm-local-id="state"
          data-htm-bind="item.state"></span>
  </div>
</template>
```

Member order follows the deterministic top-level link order. Member clones and
top-level link clones have separate document identities backed by the same
process graph identity.

Group identity combines the connection generation and source-target pair.
Adding or removing a nonfinal member preserves it. The final member removal
removes the group.
