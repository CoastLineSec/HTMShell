# `NodeLinks`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** `pipewire.nodes` item

Node link tracking exposes non-monitor connection groups relative to the
current repeated node.

## Node bindings

| Key | Presentation | Values |
| --- | --- | --- |
| `item.link_group_count` | numeric | Selected group count |
| `item.link_group_status` | text, token | `unavailable`, `ready` |

## Contextual collection

`item.link_groups` is valid only inside `pipewire.nodes`:

```html
<template data-htm-element="repeat"
          data-htm-source="item.link_groups">
  <div>
    <span data-htm-element="state-token"
          data-htm-local-id="direction"
          data-htm-bind="item.connection_direction"></span>
    <span data-htm-element="state-text"
          data-htm-local-id="peer"
          data-htm-bind="item.peer.description"></span>
  </div>
</template>
```

The inner `item.*` is the group and shadows the outer node. There is no parent
traversal.

## Selection

A sink node selects groups that target it. Every other node selects groups
that source from it. This means a bidirectional node follows the sink rule.
Groups whose target node has PipeWire media category `Monitor` or `Manager`
are excluded.

`item.connection_direction` is `incoming`, `outgoing`, `self`, or `unknown`.
`item.peer.*` uses the supported peer fields from
[`Relations`](Relations.md). For incoming connections the peer is the source;
for outgoing connections it is the target. A self-link resolves to the
tracked node.

Tracking is automatic and document-driven. Group state, member changes, and
peer label updates preserve contextual identity while the group remains
selected. The per-node limit is 4,096 groups and each node template permits at
most 8 `item.link_groups` declarations.
