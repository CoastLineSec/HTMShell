# `filter`

The CSS `filter` property applies an ordered foreground effect list to an element's complete visual subtree.

## Syntax

```css
filter: none;
filter: brightness(1.2) saturate(110%);
filter: grayscale(100%) contrast(1.1);
```

`none` is an empty list. Functions run left to right and may repeat. A list contains at most 16 functions and at most one `drop-shadow()`. The normalized declaration limit is 1,024 UTF-8 bytes.

HTMShell currently renders `brightness()`, `contrast()`, `grayscale()`, `hue-rotate()`, `invert()`, `opacity()`, `saturate()`, and `sepia()` through the CPU reference compositor. `blur()` and `drop-shadow()` are recognized but their pixel execution is pending. Any list containing a pending spatial function remains wholly pending. URL filters and `backdrop-filter` are unsupported.

## Processing

The input includes the element background, border, content, descendants, text, images, SVG, generated subparts, and box shadows. It excludes parent and sibling pixels, backdrop pixels, compositor content, and other surfaces.

Functions process straight RGBA in encoded sRGB. Results clamp after every function and become premultiplied RGBA once after the complete list. External clipping, element opacity, and the element transform follow filtering.

The CPU renderer is authoritative. The experimental Vello presenter uses complete CPU-frame fallback for nonidentity filters.

Invalid or excessive syntax invalidates the complete declaration. An accepted executable filter that exceeds a runtime layer allocation limit reports a bounded render failure rather than becoming visually unfiltered.

See [color filter functions](ColorFilterFunctions.md) for values and examples.
