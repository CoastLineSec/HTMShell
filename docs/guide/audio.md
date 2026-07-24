# PipeWire nodes

HTMShell can present a read-only view of the current PipeWire nodes and default audio relationships. The source starts when a live document uses PipeWire state and is shared by every output.

## Service state

Use `pipewire.availability` to distinguish `unavailable`, `synchronizing`, and `ready`. `pipewire.ready` is `true` only after initial registry synchronization. `pipewire.node_count` contains the number of published nodes.

```html
<span id="pipewire-state"
      data-htm-element="state-token"
      data-htm-bind="pipewire.availability"></span>

<data id="node-count"
      data-htm-element="state-value"
      data-htm-bind="pipewire.node_count"></data>
```

PipeWire absence does not stop the shell. A reconnect clears the old node generation, returns to `synchronizing`, and publishes fresh identities.

## Node list

Use the `pipewire.nodes` repeat source:

```html
<template id="node-row"
          data-htm-element="repeat"
          data-htm-source="pipewire.nodes">
  <div class="node">
    <span data-htm-element="state-text"
          data-htm-local-id="description"
          data-htm-bind="item.description"></span>
    <span data-htm-element="state-token"
          data-htm-local-id="type"
          data-htm-bind="item.node_type"></span>
  </div>
</template>
```

Each item keeps its identity while its properties or order change. `item.raw_id` is a session-local diagnostic number. It is not stable after a reconnect and cannot be used as a DOM ID or action target.

Node projections distinguish audio, video, streams, sinks, sources, direction, node type, and node state. Missing text uses the standard unavailable marker. See the [Node reference](../types/HTMShell.Services.PipeWire/Node.md).

## Defaults

Actual defaults describe the nodes selected by current session policy. Configured defaults describe stored preferences. They can differ.

Each relationship has `unavailable`, `unresolved`, or `available` status. Missing WirePlumber default metadata does not make the PipeWire graph unavailable.

## Exact properties

Inside `pipewire.nodes`, `item.property` reads one static key:

```html
<span data-htm-element="state-text"
      data-htm-local-id="application"
      data-htm-bind="item.property"
      data-htm-property-key="application.name"></span>
```

Common keys include `application.name`, `application.icon-name`, `media.name`, `media.title`, and `media.artist`. A PipeWire node may omit any key. Use `state-token` with the same binding and key to style `available` and `unavailable`.

## Limits and current scope

A document may declare 16 `pipewire.nodes` repeats. A repeated item may contain up to 64 PipeWire bindings and 32 property lookups. A document may request 64 unique property keys, with 256 unique keys process-wide. Property keys are limited to 128 bytes.

The integration is read-only. Volume, mute, channels, default selection, links, peaks, and stream movement are not exposed.

See the tracked [audio inspector example](../../examples/audio-inspector/shell.json).
