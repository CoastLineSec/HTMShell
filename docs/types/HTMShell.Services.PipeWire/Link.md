# `Link`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** `pipewire.links` item or group `item.links` item

A link represents one PipeWire source port to target port connection.

## Bindings

| Key | Presentation | Values |
| --- | --- | --- |
| `item.raw_id` | numeric | Session-local link ID |
| `item.source_port_id` | numeric | Session-local source port ID |
| `item.target_port_id` | numeric | Session-local target port ID |
| `item.ready` | text, token | `unavailable`, `partial`, `ready` |
| `item.state` | text, token | [`LinkState`](LinkState.md) |
| `item.is_monitor` | text, token | `true`, `false` |

Source and target node fields use `item.source.*` and `item.target.*`. See
[`Relations`](Relations.md).

`partial` means the link is authoritative but at least one endpoint relation
does not resolve. Partial links remain visible.

## Identity and order

Identity combines the PipeWire connection generation and global link ID. A
state change or endpoint label change retains the clone. Removal invalidates
it, and a reused raw ID after reconnect creates a new identity.

Links are ordered by source identity, target identity, source port ID, target
port ID, then raw link ID. Missing values sort deterministically. Ordering is
not persistent across PipeWire sessions.

Links are read-only. HTMShell does not create, destroy, activate, or deactivate
them.
