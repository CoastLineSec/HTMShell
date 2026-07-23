# `PowerProfile`

**Module:** `HTMShell.Services.UPower` | **Kind:** Finite state

`power_profile.current` and hold `item.profile` use these values:

| Text | Token |
| --- | --- |
| Power saver | `power-saver` |
| Balanced | `balanced` |
| Performance | `performance` |
| Unknown profile | `unknown` |

When the service is absent, `power_profile.current` uses text `Power profiles unavailable` and token `unavailable`.

Unknown future profile strings map to `unknown`.

## Usage

```css
#profile[data-htm-state="power-saver"] {
  opacity: 0.8;
}
```

## See also

- [`PowerProfiles`](PowerProfiles.md)
- [Power profile actions](../HTMShell.Actions/PowerProfile.md)
