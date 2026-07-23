# `Overlay`

**Module:** `HTMShell.State` | **Kind:** State group | **Scope:** Output

Overlay state belongs to one output's panel and overlay pair.

## Keys

### `overlay.status`

**Presentation:** Text and token

Text values:

```text
Overlay: open
Overlay: closed
```

Token values:

| Token | Meaning |
| --- | --- |
| `open` | The output-local overlay is mapped or opening. |
| `closed` | The overlay is not mapped. |

### `overlay.activation_count`

**Presentation:** Text

Format:

```text
Activations: 3
```

The count increments when `overlay.activate` dispatches from that output's overlay.

## Usage

```html
<span id="overlay-status"
      data-htm-element="state-token"
      data-htm-bind="overlay.status"></span>
```

Opening, closing, and activating an overlay update only the documents on the same output. Unchanged projections do not schedule a frame.

## See also

- [Overlay actions](../HTMShell.Actions/Overlay.md)
- [`OverlaySurface`](../HTMShell/OverlaySurface.md)
