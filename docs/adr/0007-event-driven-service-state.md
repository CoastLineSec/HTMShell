# ADR 0007: Event-driven service state

## Status

Accepted for the experimental portable shell host. Service and runtime
interfaces remain unstable.

## Context

The built-in element registry lets ordinary HTML display typed host state
through `state-text` without reparsing the document. Native shell data should
reuse that boundary instead of introducing service-specific elements,
JavaScript, or timers attached to individual outputs and elements.

Local time is the first service-backed value. Clock consumers may use the fixed
`clock.time` binding or a validated `clock-text` declaration. All declarations
represent one sampled process instant while retaining independent presentation
settings.

## Decision

Native services publish typed state; they do not own HTML elements, runtime
node identities, Wayland objects, or presentation resources. Existing
document-owned binding indexes determine which elements consume a changed
state key.

`clock.time` is a process-scoped, read-only binding consumed by the existing
`state-text` element. It remains a zero-padded 24-hour `HH:mm` convenience
value.

The semantic `clock-text` element adds validated time and date formats,
per-declaration local, UTC, or named IANA zones, inferred update cadence, and
an enabled state. Typed actions may enable, disable, or toggle an exact
document-local clock target. The runtime owns its text, `datetime`, and finite
enabled-state token.

One process-level scheduler samples the system wall clock and uses one absolute
realtime deadline for the earliest visible change. It supports second, minute,
hour, date, and zone-transition deadlines. It recalculates from a fresh sample
after every wakeup and handles a discontinuous wall-clock change by resampling
rather than replaying missed intervals. The Wayland and clock descriptors
share the existing blocking poll loop. No timer thread, asynchronous runtime,
or polling loop is introduced.

A document subscribes when its one-time built-in declaration index contains
`clock.time` or `clock-text`. The first enabled consumer receives an immediate
snapshot and arms the required deadline. Additional consumers share that
sample. Removing or disabling the last enabled consumer disarms the timer.
Updates mutate only matching declarations; unchanged text and attributes
produce no mutation or frame.

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
- JavaScript remains unnecessary.

The public format profile is finite and validated. Locale selection, translated
names, live host time-zone configuration watching, alarms, countdowns,
relative time, runtime format mutation, dynamic services, and a general
service registry remain deferred.

## Acceptance criteria

- Clock state uses the existing fixed binding or semantic `clock-text`.
- Exactly one scheduler and timer descriptor serve all live outputs.
- The initial value appears immediately and later values align to inferred
  visible changes.
- One sample and one formatted snapshot fan out to every subscribed document.
- Duplicate display values schedule no mutation or frame.
- Subscription and enabled-state changes are generation-safe. Consumers
  without a deadline leave the timer disarmed.
- Wayland input and clock deadlines coexist in one blocking event loop.
- Clock updates do not reparse HTML, rescan declarations, or redraw unbound
  surfaces.
- Scale-1 and fractional-scale presentation retain existing behavior.

## Final decision

```text
CONTINUE WITH EVENT-DRIVEN SERVICE STATE
```
