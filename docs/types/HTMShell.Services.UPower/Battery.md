# `Battery`

**Module:** `HTMShell.Services.UPower` | **Kind:** Native state source | **Scope:** Process

`Battery` publishes UPower's aggregate display-device state. HTMShell does not enumerate batteries or read power-supply data directly from sysfs.

## State keys

| Key | Presentation | Values |
| --- | --- | --- |
| `battery.percentage` | Text | Rounded whole percentage such as `78%`, or a `U+2014` dash placeholder when unknown |
| `battery.status` | Text and token | Availability and charge state |
| `battery.warning` | Token | Warning level |

Status text values are `Battery unavailable`, `No battery`, `Battery`, `Charging`, `Discharging`, `Empty`, `Fully charged`, `Pending charge`, and `Pending discharge`.

Status tokens are:

```text
unavailable
absent
unknown
charging
discharging
empty
full
pending-charge
pending-discharge
```

Warning tokens are:

```text
unknown
none
discharging
low
critical
action
```

## Availability

`Present` means UPower reports an aggregate display battery. `Absent` means UPower is available but reports no display battery. `Unavailable` means the system bus or UPower cannot currently provide state.

Absent and unavailable states clear the percentage. Missing, malformed, or future values map to typed unknown state. A malformed percentage never becomes an arbitrary string or token.

## Usage

```html
<div id="battery"
     data-htm-element="state-token"
     data-htm-bind="battery.status">
  <span id="battery-level"
        data-htm-element="state-text"
        data-htm-bind="battery.percentage"></span>
  <span id="battery-warning"
        data-htm-element="state-token"
        data-htm-bind="battery.warning"></span>
</div>
```

```css
#battery[data-htm-state="charging"] {
  opacity: 1;
}

#battery[data-htm-state="absent"],
#battery[data-htm-state="unavailable"] {
  opacity: 0.55;
}

#battery-warning[data-htm-state="critical"],
#battery-warning[data-htm-state="action"] {
  color: red;
}
```

## Update behavior

One event-driven UPower source serves every subscribed document. Property and service-owner changes refresh one typed snapshot. Duplicate snapshots cause no mutation. UPower absence does not stop the shell.

Normal operation does not poll. Battery controls, per-device state, time estimates, health, thresholds, power profiles, and notifications are unavailable.

## See also

- [`state-text`](../HTMShell.Elements/state-text.md)
- [`state-token`](../HTMShell.Elements/state-token.md)
- [Native state guide](../../guide/native-state.md)
