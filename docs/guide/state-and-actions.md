# State and actions

HTMShell attaches typed behavior to ordinary HTML through three built-in declarations. Every declared element requires a unique, nonempty HTML `id`.

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

State has process, output, or surface scope. Process state is shared across outputs. Output state affects one output group. Surface state describes one document surface.

Bindings and actions are fixed names. HTMShell does not evaluate expressions, call arbitrary commands, or run JavaScript. See the [`State`](../types/HTMShell.State/README.md) and [`Actions`](../types/HTMShell.Actions/README.md) references.
