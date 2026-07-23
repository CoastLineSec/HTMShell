# ADR 0007: Event-driven service state

## Status

Accepted for the experimental portable shell host. Service and runtime
interfaces remain unstable.

## Context

The built-in element registry lets ordinary HTML display typed host state
through `state-text` without reparsing the document. Native shell data should
reuse that boundary instead of introducing service-specific elements,
JavaScript, or timers attached to individual outputs and elements.

Local time is the first service-backed value. Its visible `HH:mm` value changes
at discrete deadlines and is identical for every output in one process.

## Decision

Native services publish typed state; they do not own HTML elements, runtime
node identities, Wayland objects, or presentation resources. Existing
document-owned binding indexes determine which elements consume a changed
state key.

`clock.time` is a process-scoped, read-only binding consumed by the existing
`state-text` element. One process-level scheduler samples the system wall
clock, converts it to local civil time, formats one immutable zero-padded
24-hour `HH:mm` snapshot, and fans that snapshot out to all subscribed
documents.

The scheduler uses one absolute realtime deadline for the next visible minute
change. It recalculates that deadline from a fresh sample after every wakeup
and handles a discontinuous wall-clock change by resampling rather than
replaying missed minutes. The Wayland and clock descriptors share the existing
blocking poll loop. No timer thread, asynchronous runtime, or periodic polling
loop is introduced.

A document subscribes when its one-time built-in declaration index contains
`clock.time`. The first subscriber receives an immediate snapshot and arms the
timer. Additional subscribers reuse the current snapshot. Removing the last
subscriber disarms the timer. Updates mutate only matching binding targets;
unchanged strings produce no mutation or frame.

Local time uses the system time zone. Failure to discover it falls back to UTC
with a diagnostic. Runtime time-zone reconfiguration behavior is deferred.

## Consequences

- panels on different outputs receive the same sampled clock sequence while
  retaining independent documents and frame scheduling;
- unbound documents incur no clock mutation or rendering work;
- closed bound surfaces may retain current document state without requesting a
  frame;
- output addition receives the current snapshot without creating another
  scheduler;
- output removal releases only that document generation's subscription;
- scale, configure, and overlay-role changes do not rebuild declarations or
  alter clock scheduling;
- JavaScript and a clock-specific element kind remain unnecessary.

The initial clock format is deliberately fixed. Arbitrary formatting, seconds,
time-zone selection, alarms, calendar data, dynamic services, and a general
service registry remain deferred.

## Acceptance criteria

- The built-in registry still contains only `state-text` and `action-button`.
- Exactly one scheduler and timer descriptor serve all live outputs.
- The initial value appears immediately and subsequent values align to visible
  minute changes.
- One sample and one formatted snapshot fan out to every subscribed document.
- Duplicate display values schedule no mutation or frame.
- Subscription changes are generation-safe and the no-subscriber state is
  timer-free.
- Wayland input and clock deadlines coexist in one blocking event loop.
- Clock updates do not reparse HTML, rescan declarations, or redraw unbound
  surfaces.
- Scale-1 and fractional-scale presentation retain existing behavior.

## Final decision

```text
CONTINUE WITH EVENT-DRIVEN SERVICE STATE
```
