# `UPower`

**Module:** `HTMShell.Services.UPower` | **Kind:** Native state source | **Scope:** Process

`UPower` presents service state from `org.freedesktop.UPower`.

## State keys

| Key | Presentation | Values |
| --- | --- | --- |
| `upower.availability` | Text and token | `available` or `unavailable` |
| `upower.on_battery` | Text and token | Power source state |
| `upower.device_count` | Value | Number of enumerated devices |

`upower.on_battery` text is `On battery`, `On external power`, or `Power state unavailable`. Tokens are `battery`, `external`, and `unavailable`.

`upower.device_count` uses the `raw` numeric format. The aggregate display device is not included.

## Updates

One system bus connection is shared with Power Profiles. Owner changes, root properties, device signals, and device properties drive updates. UPower disappearance clears aggregate and collection state. Reappearance starts a fresh source generation.

HTMShell does not read `/sys/class/power_supply`, run UPower command-line tools, or poll the service.

## Usage

```html
<span id="source"
      data-htm-element="state-token"
      data-htm-bind="upower.on_battery"></span>
```

## See also

- [`DisplayDevice`](DisplayDevice.md)
- [`DeviceCollection`](DeviceCollection.md)
