# `blur()`

`blur()` applies a bounded Gaussian blur to the current result of the ordered `filter` list.

```css
.soft-card {
  filter: blur(4px);
}

.muted-art {
  filter: grayscale(1) blur(2px) brightness(1.1);
}
```

The argument is a nonnegative CSS length. HTMShell accepts `px`, `in`, `cm`, `mm`, `q`, `pt`, `pc`, `em`, `rem`, `ex`, `ch`, `vw`, `vh`, `vmin`, and `vmax` through Stylo's computed-value resolution. Percentages and negative lengths are invalid. The computed maximum is 64 logical pixels.

The computed length is the Gaussian standard deviation, or sigma. `blur(0)` is an identity. The finite logical support on each side is:

```text
ceil(3 * sigma)
```

Samples beyond the SourceGraphic are transparent black. The CPU reference compositor blurs premultiplied encoded-sRGB RGBA directly, using identical weights for red, green, blue, and alpha. This avoids colored halos and preserves `RGB <= alpha`.

Sigma is retained in logical pixels. Each renderer derives physical sigma from the output's rational scale. Sigma below 2 physical pixels uses a normalized direct separable Gaussian. Sigma at or above 2 physical pixels uses a deterministic three-box approximation. CPU and Vello share the kernel and box-width derivation.

Functions execute left to right, and repeated blur stages remain distinct:

```css
filter: brightness(1.2) blur(2px);
filter: blur(2px) brightness(1.2);
filter: blur(1px) blur(3px);
```

The SourceGraphic includes the element background, border, content, descendants, text, images, SVG, generated subparts, and existing box shadows. External clipping, element opacity, and element transforms apply after the complete filter list.

Each effect image is limited to 4,096 physical pixels per dimension and 64 MiB. All live effect images and spatial scratch for one surface share a 256 MiB budget. Allocation failure reports a render error; it never substitutes an unblurred subtree.

The optional experimental Vello path executes blur natively with finite separable render passes over premultiplied `Rgba8Unorm`. Explicit conversion passes preserve the straight-alpha contract of color functions before and after blur. Ordered color and blur combinations and repeated blur stages remain on the GPU. Successful live frames use no CPU frame rasterization, readback, or shared-memory presentation.

Damage-limited Vello rendering expands replay tiles by the cumulative physical blur reach plus two antialias pixels, then copies only the authoritative tile core into persistent backing. Excessive guarded area, unsafe spatial transforms, initialization, resize, scale changes, and recovery use a complete GPU render. The complete backing is still converted to every acquired surface image.

Any list containing `drop-shadow()` remains indivisible and uses complete CPU-frame fallback. `backdrop-filter`, filter animation, and transitions are unsupported.

Blur can reduce readability and obscure visual focus indicators. Keep essential labels and focus feedback clear.
