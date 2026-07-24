# `Volume`

**Module:** `HTMShell.Services.PipeWire`

HTMShell exposes one perceptual average volume for each audio-capable node.

## Scale

`0.0` is silent and `1.0` is ordinary 100 percent volume. Values above `1.0` represent amplification and remain visible in public state.

PipeWire channel gains are converted to the same perceptual scale used by the reference behavior, then averaged. The public API does not expose a second raw-linear value.

## Writes

Setting an average scales the current perceptual channel values proportionally, preserving their balance. If every channel is zero, each channel receives the requested average. Mono, stereo, surround, and auxiliary channel counts use the same rule.

The ordered channel vector remains internal. Empty, nonfinite, or malformed vectors make volume unavailable.

## Bounds

The default authored range maximum is `1.0`. Authors must explicitly set a larger maximum to enable amplification. The runtime maximum is `2.0`.

Amplification can clip or distort. An incoming authoritative value above an authored control maximum remains available through `item.volume` or a default relationship volume key. The visual thumb may stop at its authored maximum without changing the public value.

## Authority

Pointer interaction can update local range presentation immediately. Public volume changes only after PipeWire reports the authoritative channel values. External changes update idle controls. Failure rolls a control back to the latest authoritative value.

## See also

- [`range-control`](../HTMShell.Elements/range-control.md)
- [`AudioControls`](AudioControls.md)
