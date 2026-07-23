# `PerformanceDegradationReason`

**Module:** `HTMShell.Services.UPower` | **Kind:** Finite state

`power_profile.degradation` reports why performance mode is limited.

| Text | Token |
| --- | --- |
| Not degraded | `none` |
| High operating temperature | `high-temperature` |
| Lap detected | `lap-detected` |
| Unknown degradation | `unknown` |

When Power Profiles is unavailable, the text is `Power profiles unavailable` and the token is `unavailable`.

Unknown future service strings map to `unknown`.

## Usage

```html
<span id="degradation"
      data-htm-element="state-token"
      data-htm-bind="power_profile.degradation"></span>
```

## See also

- [`PowerProfiles`](PowerProfiles.md)
