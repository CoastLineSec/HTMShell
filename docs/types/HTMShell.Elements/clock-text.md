# `clock-text`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element | **Declaration:** `data-htm-element="clock-text"`

`clock-text` formats wall-clock time in an ordinary semantic `time` element.

## Usage

```html
<time id="panel-clock"
      class="clock"
      data-htm-element="clock-text"
      data-htm-format="%H:%M"
      data-htm-time-zone="UTC"></time>
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tag | `time` |
| `id` | Required, nonempty, unique in the document |
| `data-htm-element` | Must be `clock-text` |
| `data-htm-format` | Required validated format, at most 128 UTF-8 bytes |
| `data-htm-time-zone` | Optional `local`, exact `UTC`, or named IANA zone |
| `data-htm-enabled` | Optional strict `true` or `false`, default `true` |

`data-htm-bind`, `data-htm-action`, and `data-htm-target` are forbidden on the clock.

## Runtime output

HTMShell owns the element's text, `datetime`, and `data-htm-state`. Authors must not set `datetime` or `data-htm-state`.

`datetime` uses `YYYY-MM-DDTHH:MM:SS+HH:MM` at the format's effective precision. The state token is `enabled` or `disabled`.

The format is compiled once. Text and attributes update incrementally from one sampled process instant. Configure, scale, clock updates, and overlay role recreation preserve element and document identity.

Formats may produce at most 256 UTF-8 bytes. Each document may contain 64 clocks. Invalid formats, zones, booleans, tags, attributes, and target relationships reject document initialization.

## See also

- [Clock guide](../../guide/clock.md)
- [Clock service](../HTMShell.Services.Clock/Clock.md)
- [Clock actions](../HTMShell.Actions/Clock.md)
