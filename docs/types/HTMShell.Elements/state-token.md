# `state-token`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element | **Declaration:** `data-htm-element="state-token"`

`state-token` projects one typed finite value into the runtime-owned `data-htm-state` attribute.

## Usage

```html
<span id="overlay-indicator"
      class="status-dot"
      data-htm-element="state-token"
      data-htm-bind="overlay.status"></span>
```

```css
#overlay-indicator[data-htm-state="open"] {
  opacity: 1;
}

#overlay-indicator[data-htm-state="closed"] {
  opacity: 0.5;
}
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tags | `div`, `span`, `section` |
| `id` | Required, nonempty, and unique in the document |
| `data-htm-element` | Must be `state-token` |
| `data-htm-bind` | Required approved token binding |
| `data-htm-state` | Reserved for HTMShell |

Author-provided `data-htm-state` is rejected. Unknown behavior attributes, unsupported tags, and text-only bindings are also rejected.

## Behavior

Initial state is applied during document initialization. Changed values use incremental attribute mutation and normal CSS style resolution. Unchanged values produce no mutation.

Token domains are compile-time finite. Arbitrary attribute names, class replacement, style mutation, expressions, and token lists are unavailable. Author classes and unrelated attributes remain intact.

## See also

- [`state-text`](state-text.md)
- [`Overlay`](../HTMShell.State/Overlay.md)
- [`Surface`](../HTMShell.State/Surface.md)
- [`Battery`](../HTMShell.Services.UPower/Battery.md)
