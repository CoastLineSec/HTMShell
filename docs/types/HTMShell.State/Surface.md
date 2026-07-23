# `Surface`

**Module:** `HTMShell.State` | **Kind:** State group | **Scope:** Surface

Surface state describes the document's manifest template and presentation profile.

## Keys

### `surface.template_id`

**Presentation:** Text

Format:

```text
Surface: panel
```

The value uses the stable manifest surface ID.

### `surface.scale_profile`

**Presentation:** Token

| Token | Meaning |
| --- | --- |
| `scale-1` | The direct scale 1 fallback is active. |
| `fractional` | Fractional-scale and viewporter are active with a preferred scale. |

Changing between fractional numerators does not change the `fractional` token.

## Usage

```html
<span id="scale-profile"
      data-htm-element="state-token"
      data-htm-bind="surface.scale_profile"></span>
```

A scale update changes only the affected surface. It does not reconstruct the document or its binding index.

## See also

- [`Output`](Output.md)
- [Surfaces guide](../../guide/surfaces.md)
