# `PeakMonitor`

Monitor-local public state for an explicit `peak-monitor`.

## Keys

- `peak.status`: `disabled`, `suspended`, `unavailable`, `starting`, `ready`,
  or `failed`
- `peak.enabled`: requested runtime enabled state
- `peak.active`: whether the shared target stream is active for this declaration
- `peak.can_enable`: true when a disabled declaration has an eligible target
- `peak.can_disable`: true while monitoring is requested
- `peak.maximum`: latest maximum perceptual channel peak
- `peak.channel_count`: ready monitor-layout size, otherwise zero

Missing peak values are unavailable rather than zero. `peak.maximum` is finite
and nonnegative. `1.0` has PipeWire full-scale meaning and reported values
above `1.0` remain representable.

The declaration element owns the runtime `data-htm-state` value using the same
six `peak.status` tokens. Its identity includes the document generation,
declaration identity, PipeWire generation, target node identity, stream
generation, and target relationship where applicable.

The target is the current node inside `pipewire.nodes`, or exactly
`pipewire.default_sink` or `pipewire.default_source` outside repeats.
Configured defaults, raw IDs, names, DOM IDs, selectors, interpolation, and
parent traversal are invalid.

An enabled closed declaration is `suspended` and contributes no stream demand.
Stream creation and format negotiation use `starting`; samples produce
`ready`; a contained transport failure produces `failed`; stale, unsupported,
unresolved, or denied targets are `unavailable`.

See [`PeakMonitoring`](PeakMonitoring.md) and
[`PeakChannel`](PeakChannel.md).
