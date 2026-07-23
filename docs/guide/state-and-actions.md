# State and actions

HTMShell attaches typed behavior to ordinary HTML through six built-in declarations. Declarations outside repeats require a unique, nonempty HTML `id`.

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

## Collections

[`repeat`](../types/HTMShell.Elements/repeat.md) expands one inert `template` for each keyed source item. Registered descendants use `data-htm-local-id`. Repeats are read-only and cannot be nested.

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

State has process, output, or surface scope. Process state is shared across outputs. Output state affects one output group. Surface state describes one document surface.

Bindings and actions are fixed names. HTMShell does not evaluate expressions, call arbitrary commands, or run JavaScript. See the [`State`](../types/HTMShell.State/README.md), [`Actions`](../types/HTMShell.Actions/README.md), and [`clock-text`](../types/HTMShell.Elements/clock-text.md) references.
