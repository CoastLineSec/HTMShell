# `NodeState`

**Module:** `HTMShell.Services.PipeWire`
**Kind:** Finite node state

`item.node_state` exposes PipeWire node state as text or a token.

| Text | Token |
| --- | --- |
| Unknown | `unknown` |
| Error | `error` |
| Creating | `creating` |
| Suspended | `suspended` |
| Idle | `idle` |
| Running | `running` |

A minimally discovered node can remain `unknown` until its detailed state is requested and received. Future or malformed values also remain `unknown`.

## See also

- [`Node`](Node.md)
