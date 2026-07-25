# `Relations`

**Module:** `HTMShell.Services.PipeWire`

PipeWire links and groups expose finite source, target, and peer node
projections. Relations resolve by connection-generation-safe node identity,
not by raw numeric ID.

## Status

| Token | Meaning |
| --- | --- |
| `available` | The endpoint resolves to a current node |
| `unresolved` | The graph names an endpoint that is not currently present |
| `unavailable` | No authoritative endpoint identity is available |

## Fields

Source and target relations accept exactly these fields:

| Source key | Target key | Presentation |
| --- | --- | --- |
| `item.source.status` | `item.target.status` | text, token |
| `item.source.name` | `item.target.name` | text |
| `item.source.nickname` | `item.target.nickname` | text |
| `item.source.description` | `item.target.description` | text |
| `item.source.media_class` | `item.target.media_class` | text |
| `item.source.node_type` | `item.target.node_type` | text, token |
| `item.source.node_state` | `item.target.node_state` | text, token |
| `item.source.direction` | `item.target.direction` | text, token |
| `item.source.raw_id` | `item.target.raw_id` | numeric |

Contextual node link groups accept exactly these peer fields:

| Peer key | Presentation |
| --- | --- |
| `item.peer.status` | text, token |
| `item.peer.name` | text |
| `item.peer.nickname` | text |
| `item.peer.description` | text |
| `item.peer.media_class` | text |
| `item.peer.node_type` | text, token |
| `item.peer.node_state` | text, token |
| `item.peer.direction` | text, token |
| `item.peer.raw_id` | numeric |

Missing text uses the standard unavailable marker. A raw ID is session-local
diagnostic output only.

Only these exact paths are accepted. There is no general dotted traversal,
raw-ID join, selector lookup, parent traversal, or relation chain. Relations
are read-only and cannot be DOM identities or action targets. A reconnect
invalidates every old relation.
