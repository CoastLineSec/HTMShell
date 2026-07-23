# `DisplayDevice`

**Module:** `HTMShell.Services.UPower` | **Kind:** Aggregate device state | **Scope:** Process

`DisplayDevice` exposes UPower's aggregate display device through `battery.*` bindings.

## Bindings

| Key | Presentation |
| --- | --- |
| `battery.ready` | Text and Boolean token |
| `battery.type` | Text and device-type token |
| `battery.energy` | Value: `raw`, `energy` |
| `battery.energy_capacity` | Value: `raw`, `energy` |
| `battery.change_rate` | Value: `raw`, `power` |
| `battery.time_to_empty` | Value: `raw`, `duration` |
| `battery.time_to_full` | Value: `raw`, `duration` |
| `battery.is_present` | Text and Boolean token |
| `battery.health_percentage` | Value: `raw`, `percent` |
| `battery.health_supported` | Text and Boolean token |
| `battery.icon_name` | Text |
| `battery.is_laptop_battery` | Text and Boolean token |
| `battery.power_supply` | Text and Boolean token |
| `battery.native_path` | Text |
| `battery.model` | Text |

Boolean text is `true`, `false`, or the unknown marker. Tokens are `true`, `false`, and `unknown`.

`change_rate` follows Quickshell sign behavior: charging is positive and discharging is negative. `health_supported` is true when UPower capacity is nonzero. `is_laptop_battery` requires battery type and power-supply status.

Unknown, malformed, unavailable, or inapplicable values remain explicit. They never become arbitrary HTML or tokens.

## Usage

```html
<data id="capacity"
      data-htm-element="state-value"
      data-htm-bind="battery.energy_capacity"
      data-htm-format="energy"></data>
```

## See also

- [`Battery`](Battery.md)
- [`UPowerDevice`](UPowerDevice.md)
