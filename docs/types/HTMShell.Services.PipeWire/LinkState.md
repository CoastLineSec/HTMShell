# `LinkState`

**Module:** `HTMShell.Services.PipeWire`

`LinkState` preserves the PipeWire link state without combining distinct
protocol states.

| Token | Text | Meaning |
| --- | --- | --- |
| `error` | `Error` | The link is in an error state |
| `unlinked` | `Unlinked` | The link is not linked |
| `init` | `Init` | Link initialization |
| `negotiating` | `Negotiating` | Format negotiation |
| `allocating` | `Allocating` | Resource allocation |
| `paused` | `Paused` | Linked but paused |
| `active` | `Active` | Active data flow |
| `unknown` | `Unknown` | An unrecognized future value |

State-only updates preserve link and group identity.
