# Native state

Native sources publish typed values. Existing [`state-text`](../types/HTMShell.Elements/state-text.md) and [`state-token`](../types/HTMShell.Elements/state-token.md) declarations present those values.

## Clock

[`clock.time`](../types/HTMShell.Services.Clock/Clock.md) is process-scoped local time in fixed `HH:mm` form. For custom time and date output, use [`clock-text`](../types/HTMShell.Elements/clock-text.md). It supports validated formats, local time, UTC, and named IANA zones.

One scheduler serves every clock declaration and fixed binding. It uses the earliest visible deadline and remains idle between deadlines. See [clocks and dates](clock.md).

## Power

UPower supplies aggregate battery state, external-power state, and a live device collection. Power Profiles supplies the active power mode, availability, holds, and degradation state. Both services share one process connection when used.

An absent battery and an unavailable UPower service are different states. The Power Profiles service may also be unavailable without affecting UPower. HTMShell does not read battery hardware directly from sysfs.

See the [power guide](power.md) and [`HTMShell.Services.UPower`](../types/HTMShell.Services.UPower/README.md).

Clock and power updates are event-driven. They do not use state polling, per-output sources, or per-element timers.
