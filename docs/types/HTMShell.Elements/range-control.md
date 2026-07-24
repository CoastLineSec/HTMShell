# `range-control`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element

`range-control` sets the average volume of one approved PipeWire audio target.

## Usage

```html
<input id="output-volume"
       type="range"
       data-htm-element="range-control"
       data-htm-bind="pipewire.default_sink.volume"
       data-htm-action="pipewire.audio.set_volume"
       data-htm-target="pipewire.default_sink"
       data-htm-enabled-bind="pipewire.default_sink.can_set_volume"
       min="0"
       max="1"
       step="0.01">
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tag | `input` |
| `type` | Must be `range` |
| `id` | Required outside a repeat |
| `data-htm-local-id` | Required inside `pipewire.nodes` |
| `data-htm-bind` | Matching item or actual-default volume key |
| `data-htm-action` | Must be `pipewire.audio.set_volume` |
| `data-htm-target` | Required outside a repeat, forbidden inside |
| `data-htm-enabled-bind` | Matching `can_set_volume` key |
| `min` | Finite, nonnegative, defaults to `0` |
| `max` | Finite, greater than `min`, defaults to `1` |
| `step` | Finite and positive, defaults to `0.01` |
| `disabled` | Optional permanent author disable |
| `value` | Runtime-owned |
| `data-htm-state` | Runtime-owned |

The runtime hard maximum is `2.0`. Setting `max` above `1.0` deliberately enables amplification and can cause clipping or distortion.

## Targets

Inside `pipewire.nodes`, the control must bind `item.volume`; its current keyed item is the target.

Outside a repeat, the allowed pairs are:

- `pipewire.default_sink.volume` with `pipewire.default_sink`
- `pipewire.default_source.volume` with `pipewire.default_source`

Configured defaults, raw IDs, DOM IDs, names, selectors, and dynamic targets are rejected.

## Interaction

Pointer press, drag, release, cancellation, dynamic disable, target removal, document replacement, and surface destruction are contained. The thumb may follow the pointer while `data-htm-state` is `pending`. Public volume remains authoritative PipeWire state.

Control state is `idle`, `pending`, `failed`, or `unavailable`. A failed control returns to the latest confirmed value. Author `disabled` is never removed.

The element uses the rendering engine's range presentation and supports ordinary CSS for `input[type="range"]`, `:disabled`, and `data-htm-state`. It is not a general numeric assignment element and has no descendants.

## Limits

- 64 range controls per document
- 8 range controls per repeated item
- 32 bytes for each authored numeric bound
- one active volume write per target node

## See also

- [Audio controls](../HTMShell.Services.PipeWire/AudioControls.md)
- [Volume](../HTMShell.Services.PipeWire/Volume.md)
