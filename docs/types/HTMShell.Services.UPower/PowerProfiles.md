# `PowerProfiles`

**Module:** `HTMShell.Services.UPower` | **Kind:** Native state source | **Scope:** Process

`PowerProfiles` presents state from `org.freedesktop.UPower.PowerProfiles`.

## State keys

| Key | Presentation |
| --- | --- |
| `power_profile.availability` | Text, token, and Boolean enable binding |
| `power_profile.current` | Text and token |
| `power_profile.performance_available` | Text, token, and Boolean enable binding |
| `power_profile.degradation` | Text and token |
| `power_profile.hold_count` | Value: `raw` |

Availability text and tokens are `available` and `unavailable`.

Performance availability text and tokens are `true`, `false`, or `unavailable`. Its Boolean projection is true only when performance can be selected.

## Updates

Profile properties, owner changes, and confirmed action replies drive updates. Missing Power Profiles clears stale profile state, disables bound buttons, and leaves UPower working.

One system bus connection is shared with UPower. Normal operation does not poll. HTMShell does not create or release holds.

## Usage

```html
<span id="profile"
      data-htm-element="state-token"
      data-htm-bind="power_profile.current"></span>
```

## See also

- [`PowerProfile`](PowerProfile.md)
- [`PerformanceDegradationReason`](PerformanceDegradationReason.md)
- [Power profile actions](../HTMShell.Actions/PowerProfile.md)
