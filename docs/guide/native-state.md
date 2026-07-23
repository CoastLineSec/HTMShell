# Native state

Native sources publish typed values. Existing [`state-text`](../types/HTMShell.Elements/state-text.md) and [`state-token`](../types/HTMShell.Elements/state-token.md) declarations present those values.

## Clock

[`clock.time`](../types/HTMShell.Services.Clock/Clock.md) is process-scoped local time in fixed `HH:mm` form. For custom time and date output, use [`clock-text`](../types/HTMShell.Elements/clock-text.md). It supports validated formats, local time, UTC, and named IANA zones.

One scheduler serves every clock declaration and fixed binding. It uses the earliest visible deadline and remains idle between deadlines. See [clocks and dates](clock.md).

## Battery

[`battery.percentage`, `battery.status`, and `battery.warning`](../types/HTMShell.Services.UPower/Battery.md) come from UPower's aggregate display device. One source serves every output.

An absent battery and an unavailable UPower service are different states. HTMShell does not read battery hardware directly from sysfs.

Clock and battery updates are event-driven. They do not use state polling, per-output sources, or per-element timers.
