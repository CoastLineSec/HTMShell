# `Properties`

**Module:** `HTMShell.Services.PipeWire`
**Scope:** `pipewire.nodes` item

`item.property` reads one exact scalar PipeWire property from the current repeated node.

## Usage

```html
<span data-htm-element="state-text"
      data-htm-local-id="application"
      data-htm-bind="item.property"
      data-htm-property-key="application.name"></span>
```

`data-htm-property-key` is required, static, nonempty, and limited to 128 bytes. It is valid only with `item.property` inside `pipewire.nodes`. Whitespace, control characters, wildcard syntax, and placeholder syntax are rejected.

`state-text` displays the scalar value or the standard unavailable marker. `state-token` sets `data-htm-state` to `available` or `unavailable`.

## Limits

- 32 exact property lookups per repeated item
- 64 unique property keys per document
- 256 unique property keys per process
- 1,024 bytes per stored value

Keys are matched exactly. Dots are literal characters in an upstream property name. Wildcards, prefixes, enumeration, object traversal, compound values, and writes are not supported.

Common upstream keys include `application.name`, `application.icon-name`, `media.name`, `media.title`, and `media.artist`. Their presence is not guaranteed.

## See also

- [`Node`](Node.md)
- [`state-text`](../HTMShell.Elements/state-text.md)
- [`state-token`](../HTMShell.Elements/state-token.md)
