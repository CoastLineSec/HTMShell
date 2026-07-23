# `Clock`

**Module:** `HTMShell.Services.Clock` | **Kind:** Native state source | **Scope:** Process

`Clock` supplies one wall-clock sample to fixed bindings and independent formatted declarations. It covers the author-facing time, date, precision, and enabled use cases of Quickshell `SystemClock` through semantic HTML rather than a QML object API.

## Fixed binding

`clock.time` remains a convenience text binding. It is local, always enabled while subscribed, formatted as zero-padded `HH:mm`, and updated at minute boundaries.

```html
<span id="clock"
      data-htm-element="state-text"
      data-htm-bind="clock.time"></span>
```

## Formatted declarations

Use [`clock-text`](../HTMShell.Elements/clock-text.md) for independent formats, zones, and enabled state:

```html
<time id="date"
      data-htm-element="clock-text"
      data-htm-format="%F %H:%M"
      data-htm-time-zone="Europe/London"></time>
```

Formats use this finite profile:

| Group | Accepted conversions |
| --- | --- |
| Literal and names | `%%`, `%A`, `%a`, `%B`, `%b`, `%h`, `%P`, `%p` |
| Date | `%C`, `%D`, `%d`, `%e`, `%F`, `%G`, `%g`, `%j`, `%m`, `%q`, `%U`, `%u`, `%V`, `%W`, `%w`, `%Y`, `%y` |
| Time | `%H`, `%I`, `%k`, `%l`, `%M`, `%R`, `%S`, `%T` |
| Zone | `%Q`, `%:Q`, `%Z`, `%z`, `%:z`, `%::z`, `%:::z` |

Numeric fields accept padding flags `-`, `_`, and `0`, plus a minimum width from 1 through 20 where it affects output. Name fields accept case flags `^` and `#`. Other flag and conversion combinations are rejected.

`%c`, `%r`, `%X`, `%x`, `%f`, `%.f`, `%N`, `%s`, `%n`, `%t`, and `%+` are unsupported. Locale composites, fractional seconds, timestamps, and control output are outside this API.

## Scheduling and zones

Cadence is inferred from visible fields: second, minute, hour, local day, zone transition, or static. The scheduler selects the earliest deadline across enabled consumers. Date boundaries use each declaration's zone. Offset and abbreviation output observes zone transitions.

The zone is `local` by default. Exact `UTC` and validated named IANA zones are supported. Local-zone discovery failure falls back to UTC. Live host zone configuration watching is unavailable.

One process timer serves every declaration and output. Each sequence samples once, converts each active zone once, and reuses each identical format-zone result. Disabled clocks freeze and contribute no deadline. Closed retained documents update without requesting a frame.

## Limitations

Names and meridiem output use fixed English text. Locale selection, alarms, countdowns, relative time, runtime format or zone mutation, and general expressions are unavailable.

## See also

- [Clocks and dates](../../guide/clock.md)
- [`clock-text`](../HTMShell.Elements/clock-text.md)
- [Clock actions](../HTMShell.Actions/Clock.md)
