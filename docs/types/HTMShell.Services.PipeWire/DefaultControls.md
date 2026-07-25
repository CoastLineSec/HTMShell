# `DefaultControls`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** `pipewire.nodes` items and process default metadata

Preferred-default controls request the configured output or input used by
session policy for future routing decisions. They do not directly assign the
actual default and do not move existing streams.

## Node capabilities

| Key | Presentation | Meaning |
| --- | --- | --- |
| `item.can_set_preferred_sink` | text, token, Boolean enable binding | The current node can be requested as the configured output |
| `item.can_set_preferred_source` | text, token, Boolean enable binding | The current node can be requested as the configured input |

Both use `true` and `false`. Eligibility, a usable `node.name`, writable
default metadata, current graph identity, and role classification are checked
separately. A readable configured relationship can remain available while
these keys are false.

## Select a node

```html
<button data-htm-element="action-button"
        data-htm-local-id="prefer-output"
        data-htm-action="pipewire.defaults.set_preferred_sink"
        data-htm-enabled-bind="item.can_set_preferred_sink">
  Use as preferred output
</button>
```

`pipewire.defaults.set_preferred_source` uses
`item.can_set_preferred_source`. Both actions are valid only in
`pipewire.nodes`. The current keyed item is implicit. `data-htm-target`, raw
IDs, node names, DOM IDs, selectors, and dynamic targets are rejected.

## Clear a preference

`pipewire.configured_sink.can_clear` and
`pipewire.configured_source.can_clear` are process Boolean bindings. They are
true only when writable configured-default metadata currently contains the
corresponding preference.

```html
<button id="clear-output"
        data-htm-element="action-button"
        data-htm-action="pipewire.defaults.clear_preferred_sink"
        data-htm-enabled-bind="pipewire.configured_sink.can_clear">
  Clear preferred output
</button>
```

The source action is `pipewire.defaults.clear_preferred_source` with
`pipewire.configured_source.can_clear`. Clear actions are valid only outside
repeats and accept no target.

## Confirmation and failure

The control state in runtime-owned `data-htm-state` is `idle`, `pending`,
`failed`, or `unavailable`. Configured metadata is authoritative. A completed
write call or a change to the actual default does not confirm the request.

Sink and source requests use independent coordinators. Each role retains at
most one in-flight target and one latest queued target. Duplicate requests are
suppressed. Permission denial, write failure, a two-second timeout, node
removal, metadata replacement, and reconnect are contained without changing
public configured state optimistically.

## Limits

- 128 preferred-default controls per document
- 8 preferred-default controls per repeated node item
- 4,096 interested control identities process-wide
- one in-flight request and one latest queued request per role
- a two-second confirmation timeout

## See also

- [`Defaults`](Defaults.md)
- [`Node`](Node.md)
- [`PipeWire`](PipeWire.md)
- [PipeWire default actions](../HTMShell.Actions/PipeWireDefaults.md)
