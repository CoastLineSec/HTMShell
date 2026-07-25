# `PipeWireDefaults`

**Module:** `HTMShell.Actions` | **Scope:** Process

These fixed actions update configured PipeWire default metadata. They never
move streams or mutate links.

| Action | Valid context | Required enable binding |
| --- | --- | --- |
| `pipewire.defaults.set_preferred_sink` | `action-button` in `pipewire.nodes` | `item.can_set_preferred_sink` |
| `pipewire.defaults.set_preferred_source` | `action-button` in `pipewire.nodes` | `item.can_set_preferred_source` |
| `pipewire.defaults.clear_preferred_sink` | top-level `action-button` | `pipewire.configured_sink.can_clear` |
| `pipewire.defaults.clear_preferred_source` | top-level `action-button` | `pipewire.configured_source.can_clear` |

Set actions capture the current generation-safe node item. Clear actions have
no node target. All four reject `data-htm-target`.

Configured metadata confirms the operation. Actual defaults remain a separate
session-policy result and existing streams can stay routed to their current
nodes. Controls expose `idle`, `pending`, `failed`, and `unavailable` through
runtime-owned `data-htm-state`.

Raw PipeWire IDs, node names, DOM IDs, selectors, placeholder interpolation,
channels, links, groups, UPower items, and Power Profile items are invalid
targets or contexts.
