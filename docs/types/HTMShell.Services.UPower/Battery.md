# `Battery`

**Module:** `HTMShell.Services.UPower` | **Kind:** Convenience state | **Scope:** Process

`Battery` keeps the compact aggregate bindings used by existing panel documents.

## State keys

| Key | Presentation | Values |
| --- | --- | --- |
| `battery.percentage` | Text or value | Whole percent text, numeric percent, or unknown |
| `battery.status` | Text and token | Aggregate availability and charge state |
| `battery.warning` | Token | Warning level |

Status text includes `Battery unavailable`, `No battery`, `Battery`, `Charging`, `Discharging`, `Empty`, `Fully charged`, `Pending charge`, and `Pending discharge`.

Status tokens are `unavailable`, `absent`, `unknown`, `charging`, `discharging`, `empty`, `full`, `pending-charge`, and `pending-discharge`. The compatibility token for fully charged is `full`; device collection items use `fully-charged`.

Warning tokens are `unknown`, `none`, `discharging`, `low`, `critical`, and `action`.

## Availability

Present means the aggregate display device reports a battery. Absent means UPower is available but no aggregate battery is present. Unavailable means UPower cannot currently provide state. The latter two states clear percentage data.

## Usage

```html
<div id="battery"
     data-htm-element="state-token"
     data-htm-bind="battery.status">
  <span id="level"
        data-htm-element="state-text"
        data-htm-bind="battery.percentage"></span>
</div>
```

UPower events update one process snapshot. Duplicate state causes no mutation. Normal operation does not poll.

## See also

- [`DisplayDevice`](DisplayDevice.md)
- [Power state guide](../../guide/power.md)
