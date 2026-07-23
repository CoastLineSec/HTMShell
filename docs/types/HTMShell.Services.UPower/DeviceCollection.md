# `DeviceCollection`

**Module:** `HTMShell.Services.UPower` | **Kind:** Keyed collection | **Scope:** Process

`upower.devices` contains every enumerated UPower device except the aggregate display device.

## Ordering and identity

Items are ordered by device type number, model, then internal object path. The key combines UPower service generation with the device object path. The path is not exposed as a DOM ID or item binding.

A property change preserves item and descendant identity. A sort-key change moves the existing item. Removal invalidates the item. A service restart creates fresh identities.

Each document owns independent repeated DOM instances. Unmapped retained documents update without scheduling a frame.

## Usage

```html
<template id="device"
          data-htm-element="repeat"
          data-htm-source="upower.devices">
  <section class="device">
    <span data-htm-element="state-text"
          data-htm-local-id="model"
          data-htm-bind="item.model"></span>
  </section>
</template>
```

The source limit is 128 devices. Oversized or duplicate-key snapshots are rejected rather than silently truncated.

## See also

- [`repeat`](../HTMShell.Elements/repeat.md)
- [`UPowerDevice`](UPowerDevice.md)
