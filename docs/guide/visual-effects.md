# Foreground color filters

HTMShell supports a bounded subset of the standard CSS `filter` property. The CPU reference renderer faithfully renders these functions:

- `brightness()`
- `contrast()`
- `grayscale()`
- `hue-rotate()`
- `invert()`
- `opacity()`
- `saturate()`
- `sepia()`

Use ordinary CSS:

```css
.media-art {
  filter: saturate(1.25) contrast(105%);
}
```

Functions run from left to right, and repeated functions remain separate stages. Each stage clamps its result before the next stage runs, so changing the order can change the output.

## Values and limits

Brightness, contrast, and saturation accept nonnegative numbers or percentages up to `8` or `800%`. Grayscale, invert, opacity, and sepia accept nonnegative numbers or percentages; values above `1` or `100%` normalize to `1`. Hue rotation accepts `deg`, `grad`, `rad`, `turn`, or unitless zero, with a maximum absolute authored magnitude of 100 turns.

A filter list may contain at most 16 functions. Its normalized declaration may contain at most 1,024 UTF-8 bytes. An unknown function, URL filter, unsupported unit, second drop shadow, excessive value, or excessive list invalidates the complete declaration. No valid prefix is applied by itself.

## Rendering model

The filter input is the element's complete SourceGraphic: its background, border, content, text, local images and SVG, descendants, generated subparts, and existing box shadows. Descendant filters run before ancestor filters. Pixels from a parent, sibling, another surface, the compositor, or behind the element are not part of that SourceGraphic.

Color operations run on straight RGBA values in encoded sRGB. The renderer safely unpremultiplies the isolated image, clamps after each function, then premultiplies once when producing canonical pixels. Every function except `opacity()` preserves alpha.

The element's external clip, element opacity, and transform apply after its filter list. Consequently, `filter: opacity(50%)` is distinct from the `opacity` property and both apply when both are present.

CPU headless and CPU Wayland presentation use the same reference compositor. The optional experimental Vello path does not execute filters on the GPU yet; a nonidentity color filter selects one complete CPU-rendered frame instead.

## Current boundary

`blur()` and `drop-shadow()` are parsed and retained but are not yet faithfully rendered. A list containing either function remains pending as a complete ordered list; HTMShell does not apply only its color functions. `backdrop-filter` is not supported.

Foreground filters are static. Animation and transitions are not implemented.

Filter layers are bounded to 4,096 physical pixels per dimension and 64 MiB per image, with at most 256 MiB of effect images for one surface. An executable filter that cannot obtain its bounded layer reports a rendering error rather than silently drawing unfiltered content.

Filters can reduce contrast, obscure focus indicators, or make text difficult to read. Keep labels readable outside decorative filtered regions and preserve sufficient contrast.

See the [filter reference](../types/HTMShell.CSS/Filter.md) and the [color function reference](../types/HTMShell.CSS/ColorFilterFunctions.md). The [color-filter example](../../examples/color-filters/index.html) demonstrates the rendered subset.
