# `filter`

The CSS `filter` property applies an ordered foreground effect list to an element's complete visual subtree.

## Syntax

```css
filter: none;
filter: brightness(1.2) saturate(110%);
filter: blur(4px) brightness(1.1);
filter: drop-shadow(4px 6px 3px rgb(0 0 0 / 60%));
filter: grayscale(100%) contrast(1.1);
```

`none` is an empty list. Functions run left to right and may repeat. A list contains at most 16 functions and at most one `drop-shadow()`. The normalized declaration limit is 1,024 UTF-8 bytes.

HTMShell renders `blur()`, `brightness()`, `contrast()`, `drop-shadow()`, `grayscale()`, `hue-rotate()`, `invert()`, `opacity()`, `saturate()`, and `sepia()` through the CPU reference compositor. URL filters, a second drop shadow, spread, inset shadows, and `backdrop-filter` are unsupported.

## Processing

The input includes the element background, border, content, descendants, text, images, SVG, generated subparts, and box shadows. It excludes parent and sibling pixels, backdrop pixels, compositor content, and other surfaces.

Color runs process straight RGBA in encoded sRGB and clamp after every function. Blur processes canonical premultiplied RGBA with transparent-black edge samples. Drop shadow uses the current stage alpha, applies the shared blur, offset, and encoded-sRGB color, then composites the source above the shadow. The compositor returns to canonical premultiplied RGBA at every color or spatial boundary. External clipping, element opacity, and the element transform follow filtering.

The CPU renderer is authoritative. The experimental Vello presenter uses complete CPU-frame fallback for every supported nonidentity foreground filter list.

Invalid or excessive syntax invalidates the complete declaration. An accepted executable filter that exceeds a runtime layer allocation limit reports a bounded render failure rather than becoming visually unfiltered.

See [color filter functions](ColorFilterFunctions.md), [`blur()`](Blur.md), and [`drop-shadow()`](DropShadow.md) for values and examples.
