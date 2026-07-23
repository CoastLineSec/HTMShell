# `Overlay`

**Module:** `HTMShell.Actions` | **Kind:** Action group | **Scope:** Output

Overlay actions control only the overlay associated with the source output.

## Actions

### `overlay.toggle`

**Allowed source:** Panel

Opens a closed overlay or closes an open overlay. It updates `overlay.status` and `shell.last_action`. Opening maps and renders the overlay. Closing unmaps it. Each valid click toggles the current state.

### `overlay.close`

**Allowed source:** Overlay

Closes the source output's overlay. It updates panel bindings for `overlay.status` and `shell.last_action`. A closed overlay cannot receive pointer actions. The underlying close operation is idempotent.

### `overlay.activate`

**Allowed source:** Overlay

Increments `overlay.activation_count` and updates `shell.last_action` for the source output. Only documents that present a changed value schedule frames.

## Usage

```html
<button id="toggle"
        data-htm-element="action-button"
        data-htm-action="overlay.toggle">
  Toggle overlay
</button>
```

```html
<button id="close"
        data-htm-element="action-button"
        data-htm-action="overlay.close">
  Close
</button>
```

An action from the wrong surface kind is rejected. Removed outputs, stale elements, disabled buttons, and closed overlays cannot dispatch.

## See also

- [`action-button`](../HTMShell.Elements/action-button.md)
- [`Overlay`](../HTMShell.State/Overlay.md)
- [`OverlaySurface`](../HTMShell/OverlaySurface.md)
