# ADR 0009: UPower battery state

## Status

Accepted for the experimental portable shell host. The state and transport
interfaces remain unstable.

## Context

HTMShell can project typed process state into ordinary text and finite CSS
tokens without JavaScript. Battery state is the first external system service
needed to test whether that model remains event-driven, output-independent,
and contained when a provider is absent or restarts.

Battery aggregation and hardware policy should not be recreated inside the
shell. Dynamic D-Bus values must also remain outside the runtime document
boundary.

## Decision

Aggregate battery state comes from the UPower display device at
`/org/freedesktop/UPower/devices/DisplayDevice` on the system bus. The source
reads only `IsPresent`, `Percentage`, `State`, and `WarningLevel` from
`org.freedesktop.UPower.Device`.

One process-scoped, read-only source publishes immutable typed snapshots.
UPower property changes request one complete property refresh, and service
owner changes control availability and source generations. Relevant signal
bursts are coalesced, duplicate normalized snapshots are suppressed, and
bounded reconnect deadlines apply only after a bus connection failure. Normal
battery operation is not polled.

Battery absence and source unavailability are distinct:

- `Absent` means UPower is available and reports no aggregate display battery;
- `Unavailable` means the system bus or UPower cannot currently provide state;
- `Present` carries an independently normalized percentage, charge state, and
  warning state.

Unknown future enum values and malformed percentages map to typed unknown
values. Raw D-Bus variants, names, paths, and property maps stop at the battery
source boundary.

Existing `state-text` and `state-token` elements consume
`battery.percentage`, `battery.status`, and `battery.warning` through their
document-owned binding indexes. The source owns no DOM, Wayland, surface,
buffer, frame, CSS, icon, or output state.

## Consequences

- one source and one snapshot sequence serve all outputs;
- only documents with battery bindings are visited;
- only mapped surfaces with changed visible projections schedule frames;
- service loss clears stale displayed state without stopping shell
  presentation;
- output removal releases document subscriptions without affecting the source
  for remaining outputs;
- scale and compositor identity do not affect battery state;
- no new built-in element kind or general service framework is introduced.

At the time of this decision, per-device presentation, health, time estimates,
and power profiles were deferred. [ADR 0010](0010-keyed-collections-and-power-state.md)
adds those read-only projections and typed profile selection. Direct sysfs
aggregation, battery controls, history, notifications, and automation remain
excluded.

## Acceptance criteria

- UPower's display device is the authoritative aggregate source;
- availability, percentage, charge, and warning values are typed and bounded;
- property and service-owner signals drive updates without normal polling;
- one source serves every subscribed output and shuts down deterministically;
- absent and unavailable states have explicit text and token projections;
- duplicate snapshots produce no mutation or frame;
- document parsing, declaration discovery, element identity, output lifecycle,
  clock behavior, actions, and scale profiles remain intact;
- D-Bus types do not enter runtime or Wayland surface code;
- no general async-runtime migration, per-output connection, worker pool, or
  service framework is introduced.

## Final decision

```text
CONTINUE WITH EVENT-DRIVEN BATTERY STATE
```
