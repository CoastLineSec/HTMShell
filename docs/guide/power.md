# Power state

HTMShell reads UPower and Power Profiles through one event-driven system bus connection. Both services are optional.

## Aggregate state

The aggregate display device provides the `battery.*` keys. `upower.on_battery` distinguishes battery power, external power, and an unavailable service.

```html
<span id="power-source"
      data-htm-element="state-text"
      data-htm-bind="upower.on_battery"></span>
<data id="battery-level"
      data-htm-element="state-value"
      data-htm-bind="battery.percentage"
      data-htm-format="percent"></data>
```

A desktop without a battery reports `No battery`. Missing UPower reports `Battery unavailable`. Neither state stops the shell.

## Devices

Use a standard inert `template` to render every connected UPower device:

```html
<ul>
  <template id="device-row"
            data-htm-element="repeat"
            data-htm-source="upower.devices">
    <li>
      <span data-htm-element="state-text"
            data-htm-local-id="model"
            data-htm-bind="item.model"></span>
      <data data-htm-element="state-value"
            data-htm-local-id="percentage"
            data-htm-bind="item.percentage"
            data-htm-format="percent"></data>
    </li>
  </template>
</ul>
```

The repeat is keyed by service generation and device identity. Insertions, removals, moves, and property changes preserve unchanged items. Repeats are read-only and cannot be nested.

## Power profiles

`power_profile.current` presents the active profile. Profile changes use typed actions:

```html
<button id="performance"
        data-htm-element="action-button"
        data-htm-action="power_profile.set_performance"
        data-htm-enabled-bind="power_profile.performance_available">
  Performance
</button>
```

The button is disabled when performance is unavailable. HTMShell waits for confirmed service state instead of changing the binding optimistically. It does not create or release profile holds.

`power_profile.holds` is a second repeat source with `item.profile`, `item.application_id`, and `item.reason`.

See the tracked [`examples/power`](../../examples/power/) shell and the [`UPower` reference](../types/HTMShell.Services.UPower/README.md).
