# `Clock`

**Module:** `HTMShell.Actions` | **Kind:** Action group | **Scope:** Document

Clock actions control one `clock-text` declaration selected by exact HTML ID.

## Actions

### `clock.enable`

Enables a disabled clock. It samples the current instant immediately, updates text, `datetime`, and the `enabled` token, then adds the clock to shared deadline calculation. It does nothing when already enabled.

### `clock.disable`

Freezes the current text and `datetime`, applies the `disabled` token, and removes the clock's deadline contribution. It does nothing when already disabled.

### `clock.toggle`

Applies enable or disable behavior from the target's current state.

## Usage

```html
<button id="pause"
        data-htm-element="action-button"
        data-htm-action="clock.disable"
        data-htm-target="panel-clock">
  Pause
</button>
```

`data-htm-target` is required for these actions. The ID must resolve to a `clock-text` in the same document generation. Panel and mapped overlay documents may invoke clock actions.

The target is validated during initialization and at dispatch. Normal press, release, descendant targeting, cancellation, and disabled-button rules still apply. Cross-document, cross-output, stale, missing, and non-clock targets are rejected.

## See also

- [`action-button`](../HTMShell.Elements/action-button.md)
- [`clock-text`](../HTMShell.Elements/clock-text.md)
