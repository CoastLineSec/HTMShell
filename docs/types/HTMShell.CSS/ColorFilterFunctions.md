# Color filter functions

These eight standard CSS functions are rendered by the CPU reference compositor and by the optional experimental Vello backend.

| Function | Accepted value | Identity | Normalization and alpha |
| --- | --- | --- | --- |
| `brightness()` | nonnegative number or percentage, maximum `8` | `1` | values above `8` are invalid; preserves alpha |
| `contrast()` | nonnegative number or percentage, maximum `8` | `1` | values above `8` are invalid; preserves alpha |
| `grayscale()` | nonnegative number or percentage | `0` | values above `1` normalize to `1`; preserves alpha |
| `hue-rotate()` | `deg`, `grad`, `rad`, `turn`, or unitless zero | `0deg` | maximum absolute magnitude is 100 turns; preserves alpha |
| `invert()` | nonnegative number or percentage | `0` | values above `1` normalize to `1`; preserves alpha |
| `opacity()` | nonnegative number or percentage | `1` | values above `1` normalize to `1`; scales alpha |
| `saturate()` | nonnegative number or percentage, maximum `8` | `1` | values above `8` are invalid; preserves alpha |
| `sepia()` | nonnegative number or percentage | `0` | values above `1` normalize to `1`; preserves alpha |

For functions that accept percentages, `100%` equals `1`. Negative and nonfinite values are invalid. Functions use encoded-sRGB color values and run left to right with a clamp after every stage.

## Examples

```css
.disabled {
  filter: grayscale(100%) opacity(60%);
}

.selected-art {
  filter: saturate(1.4) contrast(105%);
}

.shifted-icon {
  filter: hue-rotate(0.25turn);
}

.repeated {
  filter: brightness(1.1) brightness(1.1);
}
```

`filter: opacity(50%)` changes one stage in the ordered list. The separate `opacity: 0.5` property applies after the whole list, so using both multiplies their alpha effects.

Identity-valued lists remain represented but skip CPU or GPU effect-image allocation. Reordering or repeating functions remains semantically significant.

Vello uses a fixed bounded shader with at most 16 packed semantic operations. The isolated source and filtered output use straight-alpha encoded-sRGB `Rgba8Unorm`; final presentation performs the existing single premultiplication. Successful native frames use no CPU frame rasterization, GPU readback, or shared-memory presentation.

`blur()` and `drop-shadow()` are documented separately and may be ordered with these color functions. Color and blur combinations execute natively through Vello with explicit alpha-representation transitions. A list containing drop shadow uses complete CPU-frame fallback; a color or blur prefix or suffix is never split across backends around the shadow. `backdrop-filter`, URL filters, arbitrary matrices, and author shaders are unsupported.
