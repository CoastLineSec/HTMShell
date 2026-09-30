# `HTMShell.Component.StateReferenceInput`

**Kind:** Required live read-only component value | **Status:** Experimental

A state-reference input lets a surface pass one authorized finite state source into a reusable component. The surface chooses the source while the package candidate is prepared. The value remains live after publication, but its source selection never changes.

The component receives no state registry, service registry, provider object, mutable setter, query interface, or authority to discover another source.

## Declaration

A schema version 2 component declares one exact value projection:

```json
{
  "name": "time",
  "type": "state-reference",
  "valueType": "string",
  "required": true
}
```

`valueType` is required and accepts exactly `string`, `number`, `boolean`, or `token`. The declaration must contain `required: true`. Optional values, `required: false`, defaults, null, accepted-type arrays, lists, records, resources, and arbitrary JSON are not supported.

State-reference declarations count toward the existing limit of 64 inputs per component.

## Surface authorization

A schema version 2 panel or overlay may authorize up to 64 aliases in an ordered `stateReferences` array:

```json
{
  "id": "panel",
  "kind": "panel",
  "document": "panel.html",
  "outputs": "all",
  "edge": "top",
  "thickness": 52,
  "reserveSpace": true,
  "stateReferences": [
    {
      "name": "current-time",
      "source": "clock.time",
      "valueType": "string"
    }
  ]
}
```

Each entry contains exactly `name`, `source`, and `valueType`. Alias names use the component input identifier grammar. Duplicate aliases, unknown sources, contextual sources, and source/type mismatches reject the complete candidate.

An alias is visible only to typed state-reference assignments made by that surface root. Another surface cannot use it. Components cannot name or enumerate surface aliases. Existing root state consumers continue using their existing finite source keys and do not gain this alias namespace. An unused alias creates no consumer binding or provider demand.

## Assignment and forwarding

A surface root establishes the initial authority:

```html
<htm-use
  component="controls.clock-label"
  input-time="state:current-time">
</htm-use>
```

The lowercase `state:` form contains one surface-local alias. It permits no slash, query, fragment, percent encoding, authority, or source path. State syntax is interpreted only when the target input is a state-reference. A literal string input may still receive the text `state:current-time`.

A component cannot directly resolve `state:`. It may forward a received value:

```html
<htm-use
  component="nested-clock-label"
  input-time="input:time">
</htm-use>
```

Both declarations must be state-reference inputs with exactly the same `valueType`. Each hop creates distinct bounded provenance, retains the original authorization and source, performs no registry lookup, and adds no provider demand by itself. Forwarding depth is bounded by the component nesting limit of 32.

## Consumers

The existing finite state consumers accept `input.<name>` only in these combinations:

| Consumer | Required `valueType` |
| --- | --- |
| `<span|p|output data-htm-element="state-text" data-htm-bind="input.name">` | `string` |
| `<div|span|section data-htm-element="state-token" data-htm-bind="input.name">` | `token` |
| `<data data-htm-element="state-value" data-htm-bind="input.name">` | `number` |
| `<button data-htm-enabled-bind="input.name">` | `boolean` |

The numeric consumer uses the existing raw numeric format. The Boolean consumer applies the existing enabled/disabled projection to an ordinary component-owned button and does not add action authority. `clock-text` keeps its authored format and time-zone contract and does not consume a state-reference input.

The binding is valid only in component-owned or fallback markup. Projected caller nodes retain caller scope. Siblings cannot inspect one another's inputs, and nested components require explicit forwarding. One state input may feed several compatible consumers.

The `input.<name>` form is not a general expression, string interpolation, ordinary attribute binding, CSS value, repeat context, range value, channel or link context, or peak-monitor binding.

## Eligible sources

The public source table is the existing finite non-contextual `StateBindingKey` set. A source may be authorized only with one of the projections listed here:

| Source | Scope | Accepted `valueType` |
| --- | --- | --- |
| `clock.time` | process | string |
| `upower.availability` | process | string, token |
| `upower.on_battery` | process | string, token |
| `upower.device_count` | process | number |
| `battery.percentage` | process | string, number |
| `battery.status` | process | string, token |
| `battery.warning` | process | token |
| `battery.ready` | process | string, token |
| `battery.type` | process | string, token |
| `battery.energy` | process | number |
| `battery.energy_capacity` | process | number |
| `battery.change_rate` | process | number |
| `battery.time_to_empty` | process | number |
| `battery.time_to_full` | process | number |
| `battery.is_present` | process | string, token |
| `battery.health_percentage` | process | number |
| `battery.health_supported` | process | string, token |
| `battery.icon_name` | process | string |
| `battery.is_laptop_battery` | process | string, token |
| `battery.power_supply` | process | string, token |
| `battery.native_path` | process | string |
| `battery.model` | process | string |
| `power_profile.availability` | process | string, boolean, token |
| `power_profile.current` | process | string, token |
| `power_profile.performance_available` | process | string, boolean, token |
| `power_profile.degradation` | process | string, token |
| `power_profile.hold_count` | process | number |
| `pipewire.availability` | process | string, token |
| `pipewire.ready` | process | string, boolean, token |
| `pipewire.node_count` | process | number |
| `pipewire.link_count` | process | number |
| `pipewire.link_group_count` | process | number |
| `pipewire.default_sink.status` | process | string, token |
| `pipewire.default_sink.name` | process | string |
| `pipewire.default_sink.nickname` | process | string |
| `pipewire.default_sink.description` | process | string |
| `pipewire.default_sink.media_class` | process | string |
| `pipewire.default_sink.raw_id` | process | number |
| `pipewire.default_source.status` | process | string, token |
| `pipewire.default_source.name` | process | string |
| `pipewire.default_source.nickname` | process | string |
| `pipewire.default_source.description` | process | string |
| `pipewire.default_source.media_class` | process | string |
| `pipewire.default_source.raw_id` | process | number |
| `pipewire.configured_sink.status` | process | string, token |
| `pipewire.configured_sink.name` | process | string |
| `pipewire.configured_sink.nickname` | process | string |
| `pipewire.configured_sink.description` | process | string |
| `pipewire.configured_sink.media_class` | process | string |
| `pipewire.configured_sink.raw_id` | process | number |
| `pipewire.configured_source.status` | process | string, token |
| `pipewire.configured_source.name` | process | string |
| `pipewire.configured_source.nickname` | process | string |
| `pipewire.configured_source.description` | process | string |
| `pipewire.configured_source.media_class` | process | string |
| `pipewire.configured_source.raw_id` | process | number |
| `pipewire.default_sink.audio_status` | process | string, token |
| `pipewire.default_sink.volume` | process | number |
| `pipewire.default_sink.mute_state` | process | string, token |
| `pipewire.default_sink.can_set_volume` | process | string, boolean, token |
| `pipewire.default_sink.can_set_mute` | process | string, boolean, token |
| `pipewire.default_sink.can_monitor_peaks` | process | string, boolean, token |
| `pipewire.default_source.audio_status` | process | string, token |
| `pipewire.default_source.volume` | process | number |
| `pipewire.default_source.mute_state` | process | string, token |
| `pipewire.default_source.can_set_volume` | process | string, boolean, token |
| `pipewire.default_source.can_set_mute` | process | string, boolean, token |
| `pipewire.default_source.can_monitor_peaks` | process | string, boolean, token |
| `pipewire.configured_sink.audio_status` | process | string, token |
| `pipewire.configured_sink.volume` | process | number |
| `pipewire.configured_sink.mute_state` | process | string, token |
| `pipewire.configured_sink.can_set_volume` | process | string, boolean, token |
| `pipewire.configured_sink.can_set_mute` | process | string, boolean, token |
| `pipewire.configured_sink.can_clear` | process | string, boolean, token |
| `pipewire.configured_source.audio_status` | process | string, token |
| `pipewire.configured_source.volume` | process | number |
| `pipewire.configured_source.mute_state` | process | string, token |
| `pipewire.configured_source.can_set_volume` | process | string, boolean, token |
| `pipewire.configured_source.can_set_mute` | process | string, boolean, token |
| `pipewire.configured_source.can_clear` | process | string, boolean, token |
| `output.label` | output | string |
| `output.scale` | output | string |
| `surface.template_id` | surface | string |
| `surface.scale_profile` | surface | token |
| `overlay.status` | output | string, token |
| `overlay.activation_count` | output | string |
| `shell.last_action` | output | string |

Repeat collections, repeat-item projections, PipeWire channel and link item state, range-control context, peak-monitor context, lists, records, and runtime image state are outside this profile.

## Values, ownership, and identity

The immutable reference value retains one finite source key, its projection type and scope, the originating surface authorization, a generation-safe source identity, direct assignment identity, and bounded forwarding provenance. It does not copy provider state into a second mutable store.

The provider or surface owns the source. The final component instance owns each consumer binding. Passing the same source at two call sites creates two assignments, while several consumers may share the provider source and existing provider state. Component instance identity does not depend on a current state value.

Source identity, surface authorization identity, direct assignment identity, forwarding identity, consumer-binding identity, and live value version are separate. Provider updates change the provider generation or semantic sequence, not component or assignment identity. Package replacement creates fresh prepared values and output-local bindings.

## Availability and updates

A valid binding remains published while its provider initializes, disconnects, reconnects, or lacks a current value. Availability uses the existing projections:

- `string` uses the source's existing explicit unavailable text;
- `number` uses the unknown numeric projection and is never coerced to zero;
- `boolean` uses `None`;
- `token` uses the source's existing finite unavailable or unknown token.

No hidden literal fallback or null value is synthesized. Reconnection updates the provider generation or semantic value version without recreating the component instance.

Live consumer bindings activate only after successful snapshot publication and live document instantiation. They reuse the existing provider snapshots, demand accounting, update batching, equal-value suppression, dirty-work coalescing, and output-local frame scheduling. No registry lookup occurs per frame. No polling, worker thread, executor, or second reactive runtime is added.

Alias declarations, unused definitions, assignments with no final consumer, and forwarding with no final consumer create no additional demand. Several consumers share existing provider state rather than creating provider objects per component.

A closed retained overlay may retain its current ordinary provider demand and update retained DOM state under the existing lifecycle policy. It requests no presentation, CPU rendering, GPU preparation, SHM frame, or frame callback while closed. Reopening uses the current retained value without adding a duplicate binding.

Process-scoped providers may serve several outputs. Output- and surface-scoped keys resolve in the calling surface's actual scope. Values, consumer bindings, dirty state, callbacks, and presentation remain output-local. Removing one output does not remove demand still required elsewhere.

## Validation and limits

Declarations, surface aliases, direct assignments, forwarding compatibility, consumers, concrete values, and binding totals validate before publication. A failed candidate publishes no partial authorization or binding graph, creates no live subscription or service demand, and leaves the last published snapshot current. Provider unavailability after publication is a live availability condition, not package corruption.

| Unit | Limit |
| --- | ---: |
| State-reference inputs per component | Included in 64 total inputs |
| State-reference aliases per surface | 64 |
| Supplied inputs per invocation | 64 |
| Forwarding depth and diagnostic provenance | 32 |
| Concrete state-reference values per prepared root | 16,384 |
| State consumer bindings per prepared root | 50,000 |

Counters use checked arithmetic. Exact supported boundaries pass and one over rejects without truncation.

State-reference inputs are read-only. Action references, mutable setters, component-local state, optional or default references, generic expressions, conditionals, repeat or contextual state forwarding, dynamic source selection, runtime rebinding, resource-valued state, and runtime image state are not supported.

See [component inputs](Input.md), [components](README.md), [local packages](../../guide/packages.md), [live state](../../guide/state-and-actions.md), and [`ShellManifest`](../HTMShell/ShellManifest.md).

