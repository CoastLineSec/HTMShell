# `Shell`

**Module:** `HTMShell.State` | **Kind:** State group | **Scope:** Output

Shell state reports the last action for one output group.

## Keys

### `shell.last_action`

**Presentation:** Text

Format:

```text
Last action: <action>
```

The initial action is `Ready`. Current overlay actions produce `Opened from panel`, `Closed from panel`, `Closed from overlay`, or `Overlay state updated`.

## Usage

```html
<span id="last-action"
      data-htm-element="state-text"
      data-htm-bind="shell.last_action"></span>
```

The value changes only when an action affects the same output. A document that does not bind this key receives no mutation work.

## See also

- [Overlay actions](../HTMShell.Actions/Overlay.md)
- [`Overlay`](Overlay.md)
