# Clocks and dates

Use `clock-text` on a semantic `time` element. Each declaration has its own format, time zone, and enabled state.

## Time and date

```html
<time id="local-time"
      data-htm-element="clock-text"
      data-htm-format="%H:%M"></time>

<time id="twelve-hour"
      data-htm-element="clock-text"
      data-htm-format="%-I:%M:%S %p"></time>

<time id="local-date"
      data-htm-element="clock-text"
      data-htm-format="%A, %B %-d, %Y"></time>
```

`%H` is a zero-padded 24-hour value. `%I` is a zero-padded 12-hour value. `%-I` removes padding. `%p` and `%P` produce fixed English meridiem names.

The format determines update cadence. Seconds update at second boundaries. Minute-only output updates at minute boundaries. Hour-only output updates at hour boundaries. Date-only output updates at midnight in its selected zone. Literal-only output has no recurring deadline.

## Time zones

Omit `data-htm-time-zone` for local time. Use exact `UTC` for UTC or an IANA identifier for another zone:

```html
<time id="tokyo"
      data-htm-element="clock-text"
      data-htm-format="%H:%M %Z"
      data-htm-time-zone="Asia/Tokyo"></time>
```

Unknown identifiers and file paths are rejected. If the system-local zone cannot be found, a local declaration uses UTC. Host time-zone configuration changes are not watched live.

## Pause and resume

Set `data-htm-enabled="false"` to start frozen. A disabled clock receives one initial value and contributes no deadline.

```html
<button id="toggle-clock"
        data-htm-element="action-button"
        data-htm-action="clock.toggle"
        data-htm-target="local-time">
  Toggle
</button>
```

`clock.enable`, `clock.disable`, and `clock.toggle` target an exact clock ID in the same document. A disabled clock retains its text and runtime-owned `datetime`. Its `data-htm-state` token is `disabled`; an active clock uses `enabled`.

All declarations share one process timer and one sampled instant per update sequence. No clock creates a private timer.

See the tracked [`formatted-clock`](../../examples/formatted-clock/shell.json) example, [`clock-text`](../types/HTMShell.Elements/clock-text.md), [clock actions](../types/HTMShell.Actions/Clock.md), and [clock service](../types/HTMShell.Services.Clock/Clock.md).

Locale selection, translated names, alarms, countdowns, relative time, live format changes, and expressions are not supported.
