# `state-text`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element | **Declaration:** `data-htm-element="state-text"`

`state-text` replaces an ordinary element's text with one typed state value.

## Usage

```html
<span id="overlay-status"
      data-htm-element="state-text"
      data-htm-bind="overlay.status"></span>
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tags | `span`, `p`, `output` |
| `id` | Required, nonempty, and unique in the document |
| `data-htm-element` | Must be `state-text` |
| `data-htm-bind` | Required approved text binding |

Normal HTML attributes are allowed. Unknown `data-htm-*` behavior attributes are rejected.

## Behavior

Initial state is applied after the document is parsed. Later updates use incremental text mutation. The element and document identities remain stable.

Several elements may bind the same key. One changed state value updates every matching target through the saved binding index.

The element may contain initial text but cannot contain child elements. Bindings provide complete display strings. Formatting expressions and arbitrary state paths are unavailable.

## See also

- [`state-token`](state-token.md)
- [State reference](../HTMShell.State/README.md)
- [Clock state](../HTMShell.Services.Clock/Clock.md)
- [Battery state](../HTMShell.Services.UPower/Battery.md)
- [PipeWire node properties](../HTMShell.Services.PipeWire/Properties.md)
