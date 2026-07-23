# Native state

Native sources publish typed values. Existing [`state-text`](../types/HTMShell.Elements/state-text.md) and [`state-token`](../types/HTMShell.Elements/state-token.md) declarations present those values.

## Clock

[`clock.time`](../types/HTMShell.Services.Clock/Clock.md) is process-scoped local time in fixed `HH:mm` form. One scheduler serves every bound document. It wakes for the next visible minute change and remains idle between deadlines.

## Battery

[`battery.percentage`, `battery.status`, and `battery.warning`](../types/HTMShell.Services.UPower/Battery.md) come from UPower's aggregate display device. One source serves every output.

An absent battery and an unavailable UPower service are different states. HTMShell does not read battery hardware directly from sysfs.

Clock and battery updates are event-driven. They do not use periodic state polling, per-output sources, or per-element timers.
