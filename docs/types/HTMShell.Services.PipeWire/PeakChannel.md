# `PeakChannel`

An ordered item in the contextual `peak.channels` repeat.

## Keys

- `item.position`: canonical channel-position token
- `item.position_name`: stable English position name
- `item.index`: zero-based monitor-layout-local diagnostic index
- `item.peak`: latest authoritative perceptual scalar peak
- `item.status`: `unavailable` or `ready`
- `item.is_auxiliary`: typed auxiliary-position classification
- `item.is_custom`: typed custom-position classification

Order follows the negotiated monitor stream and is not joined to volume
channels. The shared channel-position inventory preserves every fixed SPA
position, `aux-1` through `aux-4096`, `custom-1` through
`custom-4294901760`, duplicate positions by ordinal, and future values as
`unknown`.

Identity contains the PipeWire generation, target node identity, monitor stream
generation, peak-layout generation, ordinal, and normalized position. A
peak-only change retains identity. A stream restart, count change, reorder, or
position change replaces it. Indexes and position codes are not targets.

`item.peak` is finite, nonnegative, and cube-root mapped from the absolute F32
sample peak. Values above `1.0` remain representable. Missing data is
unavailable, not a synthetic zero.

`peak.channels` is valid only inside `peak-monitor`. Its clone can contain
ordinary semantic HTML, `state-text`, `state-token`, and `state-value`. Actions,
range controls, clocks, monitors, and nested repeats are invalid.
