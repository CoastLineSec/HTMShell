# PipeWire peak actions

Typed actions controlling one enclosing `peak-monitor` declaration.

## Actions

- `pipewire.peaks.enable`
- `pipewire.peaks.disable`
- `pipewire.peaks.toggle`

They are valid only on `action-button` descendants of a monitor and outside
`peak.channels`. The nearest monitor is the implicit target.
`data-htm-target`, raw node IDs, relation targets, DOM IDs, selectors, dynamic
targets, parent traversal, range controls, and actions outside a monitor are
invalid.

Enable enters `pending` until the declaration is ready, suspended, or
unavailable. Disable completes after that declaration stops contributing
demand. Toggle reads the latest requested enabled state. Buttons use
`data-htm-state` values `idle`, `pending`, `failed`, and `unavailable`.

Control identity includes the document and element generation, monitor
declaration identity, PipeWire generation, target node identity, and exact
operation. Stale stream callbacks cannot complete a recreated button.
