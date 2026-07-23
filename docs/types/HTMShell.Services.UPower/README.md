# `HTMShell.Services.UPower`

The `HTMShell.Services.UPower` module provides process-scoped UPower and Power Profiles state.

## Types

- [`UPower`](UPower.md): Service availability, power source, and device count.
- [`Battery`](Battery.md): Compatibility bindings for common panel use.
- [`DisplayDevice`](DisplayDevice.md): Complete aggregate device bindings.
- [`DeviceCollection`](DeviceCollection.md): Keyed `upower.devices` repetition.
- [`UPowerDevice`](UPowerDevice.md): Item property bindings.
- [`UPowerDeviceState`](UPowerDeviceState.md): Device state text and tokens.
- [`UPowerDeviceType`](UPowerDeviceType.md): Complete device type text and tokens.
- [`PowerProfiles`](PowerProfiles.md): Profile service state and actions.
- [`PowerProfile`](PowerProfile.md): Profile values.
- [`PowerProfileHold`](PowerProfileHold.md): Active hold collection.
- [`PerformanceDegradationReason`](PerformanceDegradationReason.md): Degradation values.

State uses existing [`state-text`](../HTMShell.Elements/state-text.md), [`state-token`](../HTMShell.Elements/state-token.md), and [`state-value`](../HTMShell.Elements/state-value.md) elements. Collections use [`repeat`](../HTMShell.Elements/repeat.md).
