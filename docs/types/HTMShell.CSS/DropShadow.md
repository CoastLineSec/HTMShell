# `drop-shadow()`

`drop-shadow()` adds one bounded shadow derived from the rendered alpha of the current ordered filter stage.

```css
.icon {
  filter: drop-shadow(4px 6px 3px rgb(0 0 0 / 60%));
}

.label {
  color: #80d8ff;
  filter: drop-shadow(2px 2px currentColor);
}
```

## Grammar and limits

The function accepts an X offset, a Y offset, an optional blur, and an optional color:

```text
drop-shadow(<length> <length> <length>? <color>?)
```

Offsets are finite CSS lengths resolved to logical pixels and are limited to plus or minus 256 logical pixels. The optional blur is a nonnegative Gaussian sigma limited to 64 logical pixels; it defaults to zero. Percentages are invalid for all three lengths. The optional color defaults to the element's computed `currentColor`.

One `drop-shadow()` occurrence is allowed in a filter list. A second occurrence invalidates the declaration. Spread, inset, multiple shadows, URL filters, and arbitrary filter graphs are unsupported.

## Rendering

The input is the complete image produced by earlier functions in the list. The CPU reference compositor copies only that image's alpha into an exact 8-bit scalar mask. Source RGB does not affect the silhouette. A nonzero sigma blurs the mask with transparent-zero edge samples and the same direct Gaussian or deterministic three-box implementation used by `blur()`.

The blurred mask is translated by the logical offsets and multiplied by the encoded-sRGB shadow color and its alpha. The result is canonical premultiplied RGBA. The current stage source is then composited above the shadow with premultiplied source-over. Later filter functions consume the combined source and shadow.

Drop shadow follows actual rendered alpha. Text glyphs, SVG silhouettes, transparent raster-image holes, rounded geometry, descendants, completed child effects, and existing box shadows shape the result. `box-shadow` remains separate and follows box geometry.

The conservative support of the optional blur is:

```text
ceil(3 * sigma)
```

Shadow bounds are the union of the current stage bounds and the expanded bounds translated by the offsets. External clipping, element opacity, and the element transform apply after the complete filter list.

Transparent shadow color and a fully transparent SourceGraphic are visual identity fast paths, but the semantic effect stage remains represented. Allocation failure reports a bounded rendering error rather than dropping the shadow.

Effect images are limited to 4,096 physical pixels per dimension and 64 MiB each. All effect images and scratch for one surface share a 256 MiB budget. Filter nesting is limited to eight.

The CPU headless and live renderers share this implementation. The optional experimental Vello path does not execute drop shadow natively. Any list containing drop shadow uses one complete CPU-rendered fallback frame, including lists that also contain GPU-native color functions. `backdrop-filter`, filter animation, and transitions are unsupported.

Drop shadows can reduce contrast or obscure focus indicators. Keep essential text and focus feedback readable.
