# `Clock`

**Module:** `HTMShell.Services.Clock` | **Kind:** Native state source | **Scope:** Process

`Clock` publishes local civil time to `clock.time`.

## State keys

### `clock.time`

**Presentation:** Text

The fixed format is zero-padded 24-hour `HH:mm`, such as `09:07` or `17:42`.

## Usage

```html
<span id="clock"
      data-htm-element="state-text"
      data-htm-bind="clock.time"></span>
```

## Update behavior

One scheduler serves every bound document. It samples the system wall clock, converts it through the system-local time zone, and publishes one shared value. If local time-zone discovery fails, it uses UTC.

The initial value is available when the first clock binding appears. Later updates align to the next visible minute change. Duplicate display values cause no document mutation.

The scheduler is disarmed when no live document binds `clock.time`. It uses an event deadline instead of polling. There is no timer per output, surface, or element.

## Limitations

Seconds, dates, custom formats, user-selected time zones, and live time-zone configuration watching are unavailable.

## See also

- [`state-text`](../HTMShell.Elements/state-text.md)
- [Native state guide](../../guide/native-state.md)
