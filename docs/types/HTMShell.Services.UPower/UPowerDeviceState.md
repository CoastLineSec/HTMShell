# `UPowerDeviceState`

**Module:** `HTMShell.Services.UPower` | **Kind:** Finite state

`item.state` supports text and token presentation.

| Text | Token |
| --- | --- |
| Unknown | `unknown` |
| Charging | `charging` |
| Discharging | `discharging` |
| Empty | `empty` |
| Fully charged | `fully-charged` |
| Pending charge | `pending-charge` |
| Pending discharge | `pending-discharge` |

Unknown future UPower enum values map to `unknown`.

The aggregate convenience binding `battery.status` keeps its earlier `full` token for compatibility. Device items use `fully-charged`.

## Usage

```html
<span class="device-state"
      data-htm-element="state-token"
      data-htm-local-id="state"
      data-htm-bind="item.state"></span>
```

## See also

- [`UPowerDevice`](UPowerDevice.md)
- [`Battery`](Battery.md)
