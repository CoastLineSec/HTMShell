# Foreground filters

HTMShell supports a bounded subset of the standard CSS `filter` property. The CPU reference renderer faithfully renders these functions:

- `blur()`
- `brightness()`
- `contrast()`
- `drop-shadow()`
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

Blur accepts a nonnegative CSS length in `px`, `in`, `cm`, `mm`, `q`, `pt`, `pc`, `em`, `rem`, `ex`, `ch`, `vw`, `vh`, `vmin`, or `vmax`. Stylo resolves the computed value to logical pixels before rendering. Percentages are invalid. The value is a Gaussian standard deviation with a maximum of 64 logical pixels. `blur(0)` is an identity, and repeated blur functions execute as separate stages.

A filter list may contain at most 16 functions. Its normalized declaration may contain at most 1,024 UTF-8 bytes. An unknown function, URL filter, unsupported unit, second drop shadow, excessive value, or excessive list invalidates the complete declaration. No valid prefix is applied by itself.

## Rendering model

The filter input is the element's complete SourceGraphic: its background, border, content, text, local images and SVG, descendants, generated subparts, and existing box shadows. Descendant filters run before ancestor filters. Pixels from a parent, sibling, another surface, the compositor, or behind the element are not part of that SourceGraphic.

Color operations run on straight RGBA values in encoded sRGB. Consecutive color stages safely unpremultiply the isolated image, clamp after each function, then produce canonical premultiplied RGBA for the next spatial stage or final composition. Every color function except `opacity()` preserves alpha.

Blur processes premultiplied encoded-sRGB RGBA directly. It convolves every channel with the same spatial weights and samples transparent black beyond the SourceGraphic. Its finite support is `ceil(3 * sigma)` logical pixels on each side. Sigma values below 2 physical pixels use a direct separable Gaussian kernel. Values at or above 2 physical pixels use a deterministic three-box Gaussian approximation. The output always preserves `RGB <= alpha`, including fully transparent pixels.

`drop-shadow()` derives a scalar mask from the alpha of the current ordered stage. It optionally blurs that mask with the same bounded blur implementation, offsets and colors it, then composites the current source above the shadow. The syntax accepts X and Y lengths, an optional blur sigma, and an optional color. The blur defaults to zero, and the color defaults to computed `currentColor`. One occurrence is permitted per list. Blur is limited to 64 logical pixels, and each offset is limited to plus or minus 256 logical pixels. Spread and inset forms are invalid.

Drop shadow follows rendered alpha rather than box geometry. Transparent image holes, text glyphs, SVG silhouettes, descendants, completed child effects, and existing box shadows therefore shape its mask. This differs from `box-shadow`, which follows box geometry. Samples outside the current stage are transparent. Color alpha multiplies the mask, the colored shadow is premultiplied exactly once, and source-over composition keeps the shadow beneath the source.

The element's external clip, element opacity, and transform apply after its filter list. Consequently, blur may extend outside the SourceGraphic before the external clip is applied. `filter: opacity(50%)` is distinct from the `opacity` property and both apply when both are present.

CPU headless and CPU Wayland presentation use the same reference compositor. The optional experimental Vello path executes the eight color functions and `blur()` with bounded native GPU effect layers. It uses the same encoded-sRGB matrices, function order, repeated stages, per-stage clamping, Gaussian parameters, three-box widths, and transparent-edge semantics as the CPU reference.

Vello color and blur frames do not use CPU frame rasterization, GPU readback, or shared-memory presentation. Identity-only lists skip the GPU effect layer. The GPU pipeline converts straight RGBA to premultiplied RGBA before a blur stage and converts back only when a later color stage or Vello composition requires it. Consecutive blur stages remain premultiplied. Lists containing `drop-shadow()` remain indivisible and select one complete CPU-rendered frame; Vello does not execute a color or blur prefix or suffix around the shadow. The GPU effect layer uses canonical `Rgba8Unorm`, permits at most 16 functions, and follows the same 4,096 pixel dimension, 64 MiB image, 256 MiB per-surface, and nesting limits.

Damage-limited Vello replay expands each 64 by 64 pixel tile by the cumulative physical blur reach plus the existing two-pixel antialias guard. Only the tile core enters persistent backing. The guarded replay area, not just the core area, controls the 30 percent partial-render threshold. Unsafe transformed spatial layers and excessive guarded areas use a complete GPU render. Backing-to-surface conversion remains full target.

## Current boundary

All ten functions in this bounded foreground profile render through the CPU reference compositor. The eight nonspatial color functions and blur also have experimental native Vello execution. Drop shadow uses complete CPU-frame fallback in Vello mode. Vello remains optional and experimental. URL filters, multiple drop shadows in one list, spread, inset shadows, and `backdrop-filter` are not supported.

Foreground filters are static. Animation and transitions are not implemented.

Filter layers are bounded to 4,096 physical pixels per dimension and 64 MiB per image, with at most 256 MiB of effect images for one surface. An executable filter that cannot obtain its bounded layer reports a rendering error rather than silently drawing unfiltered content.

Filters can reduce contrast, obscure focus indicators, or make text difficult to read. Keep labels readable outside decorative filtered regions and preserve sufficient contrast.

See the [filter reference](../types/HTMShell.CSS/Filter.md), [color function reference](../types/HTMShell.CSS/ColorFilterFunctions.md), [`blur()` reference](../types/HTMShell.CSS/Blur.md), and [`drop-shadow()` reference](../types/HTMShell.CSS/DropShadow.md). The [filter example](../../examples/color-filters/index.html) demonstrates the rendered subset.
