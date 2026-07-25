# `peak-monitor`

An explicit local scope and lifecycle owner for PipeWire peak monitoring.

```html
<section id="peaks"
         data-htm-element="peak-monitor"
         data-htm-target="pipewire.default_sink"
         data-htm-enabled="true">
  <span id="status"
        data-htm-element="state-token"
        data-htm-bind="peak.status"></span>
</section>
```

Valid host elements are `div`, `section`, `article`, and `aside`.
`data-htm-enabled` is required and accepts only the literal lowercase values
`true` and `false`. The runtime owns `data-htm-state`; authoring it is invalid.

Inside `pipewire.nodes`, the current item is implicit and
`data-htm-target` is forbidden. Outside repeats, the target is exactly
`pipewire.default_sink` or `pipewire.default_source`. A missing target and all
configured-default, raw-ID, name, DOM-ID, selector, or dynamic targets fail
validation.

The local bindings are `peak.status`, `peak.enabled`, `peak.active`,
`peak.can_enable`, `peak.can_disable`, `peak.maximum`, and
`peak.channel_count`. The exact contextual source `peak.channels` exposes
ordered peak channels.

Action buttons inside the monitor may use `pipewire.peaks.enable`,
`pipewire.peaks.disable`, or `pipewire.peaks.toggle`. They accept no target or
enabled binding. Monitor actions are invalid outside the declaration and
inside `peak.channels`.

Limits are 64 declarations and 32 initially enabled declarations per document,
4 declarations per repeated node item, 8 actions and 4 channel repeats per
monitor, and 128 monitor bindings. Closed surfaces suspend enabled declarations
and release active stream demand.
