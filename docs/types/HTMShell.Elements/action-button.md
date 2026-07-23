# `action-button`

**Module:** `HTMShell.Elements` | **Kind:** Built-in element | **Declaration:** `data-htm-element="action-button"`

`action-button` dispatches one approved action from an ordinary HTML button.

## Usage

```html
<button id="overlay-toggle"
        type="button"
        data-htm-element="action-button"
        data-htm-action="overlay.toggle">
  <img src="assets/overlay.svg" alt="">
  <span>Overlay</span>
</button>
```

## Members

| Item | Requirement |
| --- | --- |
| Allowed tag | `button` |
| `id` | Required, nonempty, and unique in the document |
| `data-htm-element` | Must be `action-button` |
| `data-htm-action` | Required approved action |
| `disabled` | Optional HTML boolean attribute |

The action must be permitted from the document's surface kind. Other HTMShell behavior attributes are rejected.

## Interaction

A left-button press captures the deepest eligible action button under the logical pointer position. A release dispatches exactly once only when it resolves to the same live button.

Images, labels, and token elements nested inside the button resolve to their owning button. Releasing elsewhere does not dispatch.

Pointer leave, surface unmap, output removal, pointer capability loss, or stale document identity cancels the pending action. The presence of `disabled` prevents activation, including `disabled="false"` under HTML boolean-attribute rules.

Actions cannot execute arbitrary methods, shell commands, files, or network requests.

## See also

- [Overlay actions](../HTMShell.Actions/Overlay.md)
- [State and actions guide](../../guide/state-and-actions.md)
