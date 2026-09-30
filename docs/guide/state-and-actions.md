# State and actions

HTMShell attaches typed behavior to ordinary HTML through seven built-in declarations. Declarations outside repeats require a unique, nonempty HTML `id`.

## Text state

[`state-text`](../types/HTMShell.Elements/state-text.md) replaces an element's text with a host-provided value:

```html
<span id="status"
      data-htm-element="state-text"
      data-htm-bind="overlay.status"></span>
```

## Visual state

[`state-token`](../types/HTMShell.Elements/state-token.md) writes one finite token to the runtime-owned `data-htm-state` attribute:

```html
<span id="indicator"
      data-htm-element="state-token"
      data-htm-bind="overlay.status"></span>
```

Style the token with ordinary CSS:

```css
#indicator[data-htm-state="open"] { opacity: 1; }
#indicator[data-htm-state="closed"] { opacity: 0.5; }
```

Authors cannot set `data-htm-state` on a registered element.

## Numeric state

[`state-value`](../types/HTMShell.Elements/state-value.md) writes formatted text and a machine-readable `value` attribute to a semantic `data` element:

```html
<data id="energy"
      data-htm-element="state-value"
      data-htm-bind="battery.energy"
      data-htm-format="energy"></data>
```

## Component state-reference inputs

A schema version 2 surface may authorize one eligible finite scalar source under a surface-local name:

```json
"stateReferences": [
  {
    "name": "current-time",
    "source": "clock.time",
    "valueType": "string"
  }
]
```

The root passes that alias to a required state-reference component input:

```html
<htm-use component="clock-label" input-time="state:current-time"></htm-use>
```

Components cannot discover providers or name a surface alias directly. A component may statically forward a received reference through `input:name`. A component-owned or fallback `state-text`, `state-token`, or `state-value` declaration may consume the matching reference with `data-htm-bind="input.name"`. A component-owned button may consume a Boolean enabled projection with `data-htm-enabled-bind="input.name"`. Projected caller content keeps caller scope.

Authorization, type matching, assignment, forwarding, and consumer plans validate before publication. Live consumer bindings activate only after publication and reuse the existing event-driven provider snapshots, demand accounting, equal-value suppression, batching, and output-local scheduling. Temporary provider unavailability preserves the binding and uses the source's existing typed unavailable projection. Closed retained overlays may retain demand and update retained DOM state, but they do not request or present frames while closed.

See [state-reference inputs](../types/HTMShell.Component/StateReferenceInput.md) for the exact declaration, eligible sources, consumer compatibility, lifecycle, and limits.

## Collections

[`repeat`](../types/HTMShell.Elements/repeat.md) expands one inert `template` for each keyed source item. Registered descendants use `data-htm-local-id`. Repeats cannot be nested. Power repeats are read-only. `pipewire.nodes` also permits its narrow mute buttons and volume range controls.

## Actions

[`action-button`](../types/HTMShell.Elements/action-button.md) dispatches one approved action:

```html
<button id="toggle"
        data-htm-element="action-button"
        data-htm-action="overlay.toggle">
  Toggle overlay
</button>
```

A press must start on the enabled button. Its release must resolve to the same live button. Descendant labels and images resolve to the owning button. Pointer leave, surface unmap, output removal, or pointer loss cancels the pending action. The HTML `disabled` attribute prevents dispatch.

Clock control actions use an exact document-local target:

```html
<button id="pause"
        data-htm-element="action-button"
        data-htm-action="clock.disable"
        data-htm-target="panel-clock">
  Pause
</button>
```

The target must be a [`clock-text`](../types/HTMShell.Elements/clock-text.md) element in the same document. Overlay actions do not accept `data-htm-target`.

Power profile buttons may use `data-htm-enabled-bind` to follow a typed Boolean availability key. An author-provided `disabled` attribute always remains effective.

[`range-control`](../types/HTMShell.Elements/range-control.md) uses a semantic `input type="range"` for one approved PipeWire volume target. It is not a general numeric assignment control.

State has process, output, or surface scope. Process state is shared across outputs. Output state affects one output group. Surface state describes one document surface.

Bindings and actions are fixed names. HTMShell does not evaluate expressions, call arbitrary commands, or run JavaScript. See the [`State`](../types/HTMShell.State/README.md), [`Actions`](../types/HTMShell.Actions/README.md), and [`clock-text`](../types/HTMShell.Elements/clock-text.md) references.
