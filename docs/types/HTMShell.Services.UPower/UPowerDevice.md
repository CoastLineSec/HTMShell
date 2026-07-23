# `UPowerDevice`

**Module:** `HTMShell.Services.UPower` | **Kind:** Collection item

`UPowerDevice` bindings are valid only inside an `upower.devices` repeat.

## Item bindings

| Binding | Presentation |
| --- | --- |
| `item.ready` | Text and Boolean token |
| `item.type` | Text and type token |
| `item.power_supply` | Text and Boolean token |
| `item.energy` | Value: `raw`, `energy` |
| `item.energy_capacity` | Value: `raw`, `energy` |
| `item.change_rate` | Value: `raw`, `power` |
| `item.time_to_empty` | Value: `raw`, `duration` |
| `item.time_to_full` | Value: `raw`, `duration` |
| `item.percentage` | Value: `raw`, `percent` |
| `item.is_present` | Text and Boolean token |
| `item.state` | Text and state token |
| `item.health_percentage` | Value: `raw`, `percent` |
| `item.health_supported` | Text and Boolean token |
| `item.icon_name` | Text |
| `item.is_laptop_battery` | Text and Boolean token |
| `item.native_path` | Text |
| `item.model` | Text |

Published collection items have `item.ready` equal to `true`. Optional Booleans use text `true`, `false`, or the unknown marker and tokens `true`, `false`, or `unknown`.

`item.native_path`, `item.model`, and `item.icon_name` are bounded display text. The D-Bus object path is not exposed. HTMShell does not resolve icon theme names.

## See also

- [`UPowerDeviceType`](UPowerDeviceType.md)
- [`UPowerDeviceState`](UPowerDeviceState.md)
- [`state-value`](../HTMShell.Elements/state-value.md)
